//! simbus device runtime: one process, one device.

#![allow(missing_docs)]

mod ctl;

use std::future::pending;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use control::AppState;
use engine::{Device, ScenarioRunner};
use spec::{BindingSpec, device_report, load_device_from_path, load_device_from_str};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{MissedTickBehavior, interval};
use tracing::{error, info};

/// Official default template. Fallback when `--file` is omitted and the
/// catalog file is not on disk (see `docs/runtime.md`).
const DEFAULT_YAML: &str = include_str!("../../../devices/builtin/default.yaml");
const DEFAULT_PATH: &str = "devices/builtin/default.yaml";

#[derive(Parser, Debug)]
#[command(
    name = "simbus",
    version,
    about = "Industrial field device simulator — one process, one device"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    #[command(flatten)]
    run: RunArgs,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Validate a device YAML and print a configuration summary
    Check {
        /// Path to the device YAML file
        file: PathBuf,
    },
    /// HTTP client for an already-running process
    Ctl(ctl::CtlArgs),
}

#[derive(Args, Debug)]
struct RunArgs {
    /// Path to a device YAML file. When omitted, the official default template is used.
    #[arg(short, long, env = "SIMBUS_YAML_PATH")]
    file: Option<PathBuf>,
    /// Modbus TCP port (overrides YAML)
    #[arg(short, long, env = "SIMBUS_MODBUS_PORT")]
    port: Option<u16>,
    /// Modbus TLS port (overrides YAML; ignored unless the document has modbus-tls)
    #[arg(long, env = "SIMBUS_MODBUS_TLS_PORT")]
    modbus_tls_port: Option<u16>,
    /// TLS server certificate PEM (overrides YAML certfile)
    #[arg(long, env = "SIMBUS_MODBUS_CERT")]
    modbus_cert: Option<PathBuf>,
    /// TLS server private key PEM (overrides YAML keyfile)
    #[arg(long, env = "SIMBUS_MODBUS_KEY")]
    modbus_key: Option<PathBuf>,
    /// Optional client CA PEM for mTLS (overrides YAML cafile)
    #[arg(long, env = "SIMBUS_MODBUS_CA")]
    modbus_ca: Option<PathBuf>,
    /// Override device name
    #[arg(short, long, env = "SIMBUS_DEVICE_NAME")]
    name: Option<String>,
    /// OPC UA port (overrides YAML; ignored unless the document has opcua)
    #[arg(long, env = "SIMBUS_OPCUA_PORT")]
    opcua_port: Option<u16>,
    /// REST API port
    #[arg(long, env = "SIMBUS_API_PORT", default_value_t = 8000)]
    api_port: u16,
    /// Bind address for the REST API
    #[arg(long, env = "SIMBUS_API_HOST", default_value = "0.0.0.0")]
    host: String,
    /// Simulation tick interval in seconds (wall sample period)
    #[arg(long, env = "SIMBUS_TICK_INTERVAL", default_value_t = 1.0)]
    tick: f64,
    /// Simulation seconds per wall second
    #[arg(long, env = "SIMBUS_TIME_SCALE", default_value_t = 1.0)]
    time_scale: f64,
    /// Seconds between `simulation tick health` logs (`0` disables)
    #[arg(long, env = "SIMBUS_TICK_HEALTH_LOG_INTERVAL", default_value_t = 0.0)]
    tick_health: f64,
    /// Drain wait after SIGINT/SIGTERM, in seconds (`0` aborts immediately)
    #[arg(long, env = "SIMBUS_SHUTDOWN_TIMEOUT", default_value_t = 5.0)]
    shutdown_timeout: f64,
    /// RNG seed for reproducible output
    #[arg(long, env = "SIMBUS_SEED")]
    seed: Option<u64>,
    /// Optional API key required for write endpoints
    #[arg(long, env = "SIMBUS_API_KEY")]
    api_key: Option<String>,
    /// Comma-separated CORS origins (`*` for development)
    #[arg(long, env = "SIMBUS_CORS_ORIGINS", default_value = "*")]
    cors_origins: String,
}

fn load_spec(args: &RunArgs) -> Result<spec::DeviceSpec> {
    let mut spec = if let Some(path) = &args.file {
        info!(path = %path.display(), "loading device yaml");
        load_device_from_path(path).with_context(|| format!("load {}", path.display()))?
    } else {
        let on_disk = Path::new(DEFAULT_PATH);
        if on_disk.is_file() {
            info!(path = %on_disk.display(), "loading default template");
            load_device_from_path(on_disk).with_context(|| format!("load {}", on_disk.display()))?
        } else {
            info!("loading embedded default template");
            load_device_from_str(DEFAULT_YAML).context("parse embedded default template")?
        }
    };
    if let Some(name) = &args.name {
        spec.name.clone_from(name);
    }
    let unimplemented = spec.unimplemented_protocols();
    if !unimplemented.is_empty() {
        let names: Vec<&str> = unimplemented.iter().map(|p| p.as_str()).collect();
        bail!(
            "device requests unimplemented protocol(s): {}. specified in the contract; not served by this runtime. see docs/spec.md",
            names.join(", ")
        );
    }
    Ok(spec)
}

fn check_file(path: &Path) -> Result<String> {
    let spec = load_device_from_path(path).with_context(|| format!("load {}", path.display()))?;
    Ok(device_report(&path.display().to_string(), &spec))
}

#[derive(Debug)]
struct TlsBind {
    port: u16,
    certfile: PathBuf,
    keyfile: PathBuf,
    cafile: Option<PathBuf>,
}

#[derive(Debug)]
struct FieldListeners {
    tcp: Option<(u16, u8)>,
    tls: Option<TlsBind>,
    opcua: Option<u16>,
}

fn require_pem(path: &Path, label: &str) -> Result<()> {
    if !path.is_file() {
        bail!("modbus-tls {label} not found: {}", path.display());
    }
    Ok(())
}

fn field_listeners(spec: &spec::DeviceSpec, args: &RunArgs) -> Result<FieldListeners> {
    let mut unit_id = spec.modbus.unit_id;
    let mut tcp_port: Option<u16> = None;
    let mut tls: Option<TlsBind> = None;
    let mut opcua: Option<u16> = None;
    for binding in spec.resolved_bindings() {
        match binding {
            BindingSpec::ModbusTcp {
                port: bind_port,
                unit_id: bind_unit,
            } => {
                if let Some(u) = bind_unit {
                    unit_id = u;
                }
                tcp_port = Some(bind_port.unwrap_or(spec.modbus.default_port));
            }
            BindingSpec::ModbusTls {
                port,
                certfile,
                keyfile,
                cafile,
            } => {
                let certfile = args
                    .modbus_cert
                    .clone()
                    .unwrap_or_else(|| PathBuf::from(certfile));
                let keyfile = args
                    .modbus_key
                    .clone()
                    .unwrap_or_else(|| PathBuf::from(keyfile));
                let cafile = args.modbus_ca.clone().or_else(|| cafile.map(PathBuf::from));
                tls = Some(TlsBind {
                    port: args.modbus_tls_port.unwrap_or(port),
                    certfile,
                    keyfile,
                    cafile,
                });
            }
            BindingSpec::Opcua { port } => {
                opcua = Some(args.opcua_port.unwrap_or(port));
            }
            _ => {}
        }
    }
    if let Some(p) = args.port {
        if tcp_port.is_some() {
            tcp_port = Some(p);
        }
    }
    if let Some(ref tls) = tls {
        require_pem(&tls.certfile, "certfile")?;
        require_pem(&tls.keyfile, "keyfile")?;
        if let Some(ref ca) = tls.cafile {
            require_pem(ca, "cafile")?;
        }
    }
    Ok(FieldListeners {
        tcp: tcp_port.map(|port| (port, unit_id)),
        tls,
        opcua,
    })
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler");
        tokio::select! {
            _ = ctrl_c => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = ctrl_c.await;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Some(Command::Check { file }) = cli.command {
        match check_file(&file) {
            Ok(report) => {
                print!("{report}");
                return Ok(());
            }
            Err(err) => {
                eprintln!("FAIL  {}", file.display());
                eprintln!("{err:#}");
                std::process::exit(1);
            }
        }
    }
    if let Some(Command::Ctl(args)) = cli.command {
        return ctl::run(args).await;
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .compact()
        .init();

    let args = cli.run;
    if args.tick <= 0.0 {
        bail!("--tick must be > 0");
    }
    if args.time_scale <= 0.0 {
        bail!("--time-scale must be > 0");
    }
    if args.tick_health < 0.0 {
        bail!("--tick-health must be >= 0");
    }
    if args.shutdown_timeout < 0.0 {
        bail!("--shutdown-timeout must be >= 0");
    }

    let spec = load_spec(&args)?;
    let listeners = field_listeners(&spec, &args)?;
    let tcp = listeners.tcp;
    let tls = listeners.tls;
    let opcua = listeners.opcua;
    let modbus_port = tcp.map(|(p, _)| p).unwrap_or(spec.modbus.default_port);
    let modbus_tls_port = tls.as_ref().map(|t| t.port);
    let opcua_port = opcua;
    let device = Device::new(spec, args.seed, args.tick);
    device.set_running(true);

    let scenarios = Arc::new(ScenarioRunner::new(device.clone()));
    let tcp_ready = Arc::new(AtomicBool::new(tcp.is_none()));
    let tls_ready = Arc::new(AtomicBool::new(tls.is_none()));
    let opcua_ready = Arc::new(AtomicBool::new(opcua.is_none()));
    let cors: Vec<String> = args
        .cors_origins
        .split(',')
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect();

    let (snapshots, _) = watch::channel(device.snapshot());
    let state = AppState {
        device: device.clone(),
        scenarios,
        api_key: args.api_key,
        modbus_port,
        modbus_tls_port,
        opcua_port,
        modbus_ready: tcp_ready.clone(),
        modbus_tls_ready: tls_ready.clone(),
        opcua_ready: opcua_ready.clone(),
        scenario_task: Arc::new(Mutex::new(None)),
        snapshots: snapshots.clone(),
        time_scale: args.time_scale,
    };

    info!(
        device = %device.spec().name,
        r#type = %device.spec().device_type,
        api_port = args.api_port,
        modbus_port,
        modbus_tls_port,
        opcua_port,
        tick_interval = args.tick,
        time_scale = args.time_scale,
        "simbus started"
    );

    let (cancel_tx, _) = watch::channel(false);

    let tick_device = device.clone();
    let tick_snapshots = snapshots;
    let tick_scale = args.time_scale;
    let tick_health = args.tick_health;
    let mut tick_shutdown = cancel_tx.subscribe();
    let mut tick_task = tokio::spawn(async move {
        run_tick_loop(
            tick_device,
            tick_snapshots,
            tick_scale,
            tick_health,
            &mut tick_shutdown,
        )
        .await;
    });

    let mut tcp_task: Option<JoinHandle<()>> = None;
    if let Some((port, unit_id)) = tcp {
        let mut shutdown = cancel_tx.subscribe();
        let modbus_device = device.clone();
        let ready = tcp_ready;
        tcp_task = Some(tokio::spawn(async move {
            let stop = async move {
                let _ = shutdown.wait_for(|stop| *stop).await;
            };
            if let Err(err) = modbus::serve(modbus_device, port, unit_id, ready, stop).await {
                error!(error = %err, "modbus server failed");
            }
        }));
    }

    let mut tls_task: Option<JoinHandle<()>> = None;
    if let Some(tls) = tls {
        let mut shutdown = cancel_tx.subscribe();
        let modbus_device = device.clone();
        let ready = tls_ready;
        let unit_id = tcp.map(|(_, u)| u).unwrap_or(device.spec().modbus.unit_id);
        tls_task = Some(tokio::spawn(async move {
            let stop = async move {
                let _ = shutdown.wait_for(|stop| *stop).await;
            };
            if let Err(err) = modbus::serve_tls(
                modbus_device,
                modbus::TlsOptions {
                    port: tls.port,
                    unit_id,
                    certfile: &tls.certfile,
                    keyfile: &tls.keyfile,
                    cafile: tls.cafile.as_deref(),
                },
                ready,
                stop,
            )
            .await
            {
                error!(error = %err, "modbus tls server failed");
            }
        }));
    }

    let mut ua_task: Option<JoinHandle<()>> = None;
    if let Some(port) = opcua {
        let mut shutdown = cancel_tx.subscribe();
        let ua_device = device.clone();
        let ready = opcua_ready;
        ua_task = Some(tokio::spawn(async move {
            let stop = async move {
                let _ = shutdown.wait_for(|stop| *stop).await;
            };
            if let Err(err) = opcua::serve(ua_device, port, ready, stop).await {
                error!(error = %err, "opcua server failed");
            }
        }));
    }

    let mut api_shutdown = cancel_tx.subscribe();
    let api_host = args.host.clone();
    let api_port = args.api_port;
    let mut api_task = tokio::spawn(async move {
        let stop = async move {
            let _ = api_shutdown.wait_for(|stop| *stop).await;
        };
        if let Err(err) = control::serve(state, &api_host, api_port, &cors, stop).await {
            error!(error = %err, "api server failed");
        }
    });

    let graceful = tokio::select! {
        () = shutdown_signal() => true,
        res = &mut tick_task => {
            error!(?res, "tick loop ended");
            false
        }
        _ = wait_optional_task(&mut tcp_task) => {
            error!("modbus tcp task ended");
            false
        }
        _ = wait_optional_task(&mut tls_task) => {
            error!("modbus tls task ended");
            false
        }
        _ = wait_optional_task(&mut ua_task) => {
            error!("opcua task ended");
            false
        }
        res = &mut api_task => {
            error!(?res, "api task ended");
            false
        }
    };

    if graceful {
        info!("simbus stopping");
        let _ = cancel_tx.send(true);
        if args.shutdown_timeout <= 0.0 {
            tick_task.abort();
            abort_optional(&tcp_task);
            abort_optional(&tls_task);
            abort_optional(&ua_task);
            api_task.abort();
        } else {
            tokio::select! {
                _ = async {
                    join_optional(&mut tcp_task).await;
                    join_optional(&mut tls_task).await;
                    join_optional(&mut ua_task).await;
                    let _ = tokio::join!(&mut tick_task, &mut api_task);
                } => {}
                () = tokio::time::sleep(Duration::from_secs_f64(args.shutdown_timeout)) => {
                    tick_task.abort();
                    abort_optional(&tcp_task);
                    abort_optional(&tls_task);
                    abort_optional(&ua_task);
                    api_task.abort();
                }
            }
        }
        Ok(())
    } else {
        tick_task.abort();
        abort_optional(&tcp_task);
        abort_optional(&tls_task);
        abort_optional(&ua_task);
        api_task.abort();
        bail!("a runtime task ended unexpectedly")
    }
}

async fn wait_optional_task(task: &mut Option<JoinHandle<()>>) {
    match task {
        Some(handle) => {
            let _ = handle.await;
        }
        None => pending().await,
    }
}

async fn join_optional(task: &mut Option<JoinHandle<()>>) {
    if let Some(handle) = task {
        let _ = handle.await;
    }
}

fn abort_optional(task: &Option<JoinHandle<()>>) {
    if let Some(handle) = task {
        handle.abort();
    }
}

async fn run_tick_loop(
    device: Arc<Device>,
    snapshots: watch::Sender<engine::Snapshot>,
    time_scale: f64,
    health_interval: f64,
    shutdown: &mut watch::Receiver<bool>,
) {
    let started = Instant::now();
    let mut last_health = started;
    let mut prev_wake: Option<Instant> = None;
    let mut ticker = interval(Duration::from_secs_f64(device.tick_interval().max(0.001)));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = ticker.tick() => {}
            _ = shutdown.wait_for(|stop| *stop) => break,
        }
        if *shutdown.borrow() {
            break;
        }
        let woke = Instant::now();
        let wall = device.tick_interval().max(0.001);
        let drift_ms = prev_wake.map_or(0.0, |prev| {
            (woke.duration_since(prev).as_secs_f64() - wall) * 1000.0
        });
        prev_wake = Some(woke);
        if !device.is_running() {
            continue;
        }
        let t0 = Instant::now();
        let snap = device.tick(wall * time_scale);
        let tick_duration_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let _ = snapshots.send(snap);
        if health_interval > 0.0 && last_health.elapsed().as_secs_f64() >= health_interval {
            last_health = Instant::now();
            info!(
                tick_interval = wall,
                time_scale,
                tick_duration_ms,
                loop_drift_ms = drift_ms,
                sse_subscribers = snapshots.receiver_count(),
                active_faults = device.faults().len(),
                uptime_s = started.elapsed().as_secs_f64(),
                "simulation tick health"
            );
        }
        let period = Duration::from_secs_f64(wall);
        if ticker.period() != period {
            ticker = interval(period);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_yaml() -> &'static str {
        r"
name: Check Fixture
version: '1.0'
type: fixture
modbus:
  default_port: 502
  unit_id: 1
registers:
  holding:
    - address: 0
      name: temperature
      default: 22.5
      scale: 10
      data_type: uint16
"
    }

    #[test]
    fn parses_file_flag() {
        let cli =
            Cli::try_parse_from(["simbus", "--file", "devices/builtin/default.yaml"]).unwrap();
        assert!(cli.command.is_none());
        assert_eq!(
            cli.run.file.as_deref(),
            Some(Path::new("devices/builtin/default.yaml"))
        );
        assert_eq!(cli.run.api_port, 8000);
    }

    #[test]
    fn parses_without_file() {
        let cli = Cli::try_parse_from(["simbus"]).unwrap();
        assert!(cli.run.file.is_none());
    }

    #[test]
    fn parses_check_subcommand() {
        let cli = Cli::try_parse_from(["simbus", "check", "devices/community/demo.yaml"]).unwrap();
        match cli.command {
            Some(Command::Check { file }) => {
                assert_eq!(file, PathBuf::from("devices/community/demo.yaml"));
            }
            other => panic!("expected check, got {other:?}"),
        }
    }

    #[test]
    fn env_style_tick_and_seed() {
        let cli = Cli::try_parse_from([
            "simbus",
            "--file",
            "devices/custom.yaml",
            "--tick",
            "0.5",
            "--seed",
            "9",
        ])
        .unwrap();
        assert_eq!(cli.run.tick, 0.5);
        assert_eq!(cli.run.seed, Some(9));
        assert!(cli.run.file.is_some());
    }

    #[test]
    fn parses_time_scale_and_health() {
        let cli = Cli::try_parse_from([
            "simbus",
            "--time-scale",
            "60",
            "--tick-health",
            "15",
            "--shutdown-timeout",
            "2",
        ])
        .unwrap();
        assert_eq!(cli.run.time_scale, 60.0);
        assert_eq!(cli.run.tick_health, 15.0);
        assert_eq!(cli.run.shutdown_timeout, 2.0);
        assert_eq!(cli.run.tick, 1.0);
    }

    #[test]
    fn parses_ctl_status() {
        let cli =
            Cli::try_parse_from(["simbus", "ctl", "--url", "http://127.0.0.1:8001", "status"])
                .unwrap();
        match cli.command {
            Some(Command::Ctl(args)) => {
                assert!(format!("{args:?}").contains("8001"));
            }
            other => panic!("expected ctl, got {other:?}"),
        }
    }

    #[test]
    fn check_file_prints_ok_summary() {
        let dir = std::env::temp_dir().join(format!("simbus-check-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("fixture.yaml");
        std::fs::write(&path, sample_yaml()).unwrap();
        let report = check_file(&path).unwrap();
        assert!(report.starts_with("OK  "));
        assert!(report.contains("Check Fixture"));
        assert!(report.contains("temperature"));
        std::fs::remove_file(&path).ok();
        std::fs::remove_dir(&dir).ok();
    }

    #[test]
    fn loads_default_when_no_file() {
        let cli = Cli::try_parse_from(["simbus"]).unwrap();
        let spec = load_spec(&cli.run).unwrap();
        assert_eq!(spec.device_type, "example");
        assert_eq!(spec.name, "Example Device");
    }

    #[test]
    fn embedded_default_parses() {
        let spec = load_device_from_str(DEFAULT_YAML).unwrap();
        assert_eq!(spec.device_type, "example");
        assert_eq!(spec.name, "Example Device");
    }

    #[test]
    fn check_file_rejects_invalid_yaml() {
        let dir = std::env::temp_dir().join(format!("simbus-check-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.yaml");
        std::fs::write(&path, "name: nope\n").unwrap();
        assert!(check_file(&path).is_err());
        std::fs::remove_file(&path).ok();
        std::fs::remove_dir(&dir).ok();
    }

    fn tls_yaml(cert: &str, key: &str) -> String {
        format!(
            r"
name: tls-fix
version: '1.0'
type: fixture
modbus:
  default_port: 502
  unit_id: 1
bindings:
  - protocol: modbus-tcp
  - protocol: modbus-tls
    certfile: {cert}
    keyfile: {key}
registers:
  holding:
    - address: 0
      name: temperature
      default: 22.5
      scale: 10
      data_type: uint16
"
        )
    }

    #[test]
    fn parses_tls_flags() {
        let cli = Cli::try_parse_from([
            "simbus",
            "--modbus-cert",
            "cert.pem",
            "--modbus-key",
            "key.pem",
            "--modbus-tls-port",
            "8802",
            "--modbus-ca",
            "ca.pem",
        ])
        .unwrap();
        assert_eq!(cli.run.modbus_tls_port, Some(8802));
        assert_eq!(cli.run.modbus_cert.as_deref(), Some(Path::new("cert.pem")));
        assert_eq!(cli.run.modbus_key.as_deref(), Some(Path::new("key.pem")));
        assert_eq!(cli.run.modbus_ca.as_deref(), Some(Path::new("ca.pem")));
    }

    #[test]
    fn default_bindings_are_tcp_only() {
        let spec = load_device_from_str(sample_yaml()).unwrap();
        let args = Cli::try_parse_from(["simbus"]).unwrap().run;
        let listeners = field_listeners(&spec, &args).unwrap();
        assert_eq!(listeners.tcp, Some((502, 1)));
        assert!(listeners.tls.is_none());
        assert!(listeners.opcua.is_none());
    }

    #[test]
    fn tls_boot_fails_without_pem() {
        let spec =
            load_device_from_str(&tls_yaml("/no/such/cert.pem", "/no/such/key.pem")).unwrap();
        let args = Cli::try_parse_from(["simbus"]).unwrap().run;
        let err = field_listeners(&spec, &args).unwrap_err();
        assert!(err.to_string().contains("certfile not found"));
    }

    #[test]
    fn port_override_does_not_change_tls_port() {
        let dir = std::env::temp_dir().join(format!("simbus-tls-pem-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cert = dir.join("cert.pem");
        let key = dir.join("key.pem");
        std::fs::write(&cert, "placeholder").unwrap();
        std::fs::write(&key, "placeholder").unwrap();
        let spec = load_device_from_str(&tls_yaml(
            &cert.display().to_string(),
            &key.display().to_string(),
        ))
        .unwrap();
        let args = Cli::try_parse_from(["simbus", "--port", "1502", "--modbus-tls-port", "8802"])
            .unwrap()
            .run;
        let (tcp, tls) = {
            let listeners = field_listeners(&spec, &args).unwrap();
            (listeners.tcp, listeners.tls)
        };
        assert_eq!(tcp, Some((1502, 1)));
        assert_eq!(tls.unwrap().port, 8802);
        std::fs::remove_dir_all(&dir).ok();
    }

    fn opcua_yaml() -> &'static str {
        r"
name: ua-fix
version: '1.0'
type: fixture
modbus:
  default_port: 502
  unit_id: 1
bindings:
  - protocol: modbus-tcp
  - protocol: opcua
    port: 4840
registers:
  holding:
    - address: 0
      name: temperature
      default: 22.5
      scale: 10
      data_type: uint16
"
    }

    #[test]
    fn parses_opcua_port_flag() {
        let cli = Cli::try_parse_from(["simbus", "--opcua-port", "14840"]).unwrap();
        assert_eq!(cli.run.opcua_port, Some(14840));
    }

    #[test]
    fn opcua_port_override_requires_binding() {
        let spec = load_device_from_str(sample_yaml()).unwrap();
        let args = Cli::try_parse_from(["simbus", "--opcua-port", "14840"])
            .unwrap()
            .run;
        let listeners = field_listeners(&spec, &args).unwrap();
        assert!(listeners.opcua.is_none());
    }

    #[test]
    fn opcua_binding_uses_yaml_then_override() {
        let spec = load_device_from_str(opcua_yaml()).unwrap();
        let args = Cli::try_parse_from(["simbus"]).unwrap().run;
        let listeners = field_listeners(&spec, &args).unwrap();
        assert_eq!(listeners.tcp, Some((502, 1)));
        assert_eq!(listeners.opcua, Some(4840));

        let args = Cli::try_parse_from(["simbus", "--opcua-port", "14840"])
            .unwrap()
            .run;
        let listeners = field_listeners(&spec, &args).unwrap();
        assert_eq!(listeners.opcua, Some(14840));
    }
}
