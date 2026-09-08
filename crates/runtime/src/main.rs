//! simbus device runtime: one process, one device.

#![allow(missing_docs)]

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use control::AppState;
use engine::{Device, ScenarioRunner};
use spec::{BindingSpec, device_report, load_device_from_path, load_device_from_str};
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
}

#[derive(Args, Debug)]
struct RunArgs {
    /// Path to a device YAML file. When omitted, the official default template is used.
    #[arg(short, long, env = "SIMBUS_YAML_PATH")]
    file: Option<PathBuf>,
    /// Modbus TCP port (overrides YAML)
    #[arg(short, long, env = "SIMBUS_MODBUS_PORT")]
    port: Option<u16>,
    /// Override device name
    #[arg(short, long, env = "SIMBUS_DEVICE_NAME")]
    name: Option<String>,
    /// REST API port
    #[arg(long, env = "SIMBUS_API_PORT", default_value_t = 8000)]
    api_port: u16,
    /// Bind address for the REST API
    #[arg(long, env = "SIMBUS_API_HOST", default_value = "0.0.0.0")]
    host: String,
    /// Simulation tick interval in seconds
    #[arg(long, env = "SIMBUS_TICK_INTERVAL", default_value_t = 1.0)]
    tick: f64,
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

fn modbus_listen(spec: &spec::DeviceSpec, port_override: Option<u16>) -> (u16, u8) {
    let mut port = spec.modbus.default_port;
    let mut unit_id = spec.modbus.unit_id;
    for binding in spec.resolved_bindings() {
        if let BindingSpec::ModbusTcp {
            port: bind_port,
            unit_id: bind_unit,
        } = binding
        {
            if let Some(p) = bind_port {
                port = p;
            }
            if let Some(u) = bind_unit {
                unit_id = u;
            }
        }
    }
    if let Some(p) = port_override {
        port = p;
    }
    (port, unit_id)
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

    let spec = load_spec(&args)?;
    let (modbus_port, unit_id) = modbus_listen(&spec, args.port);
    let device = Device::new(spec, args.seed, args.tick);
    device.set_running(true);

    let scenarios = Arc::new(ScenarioRunner::new(device.clone()));
    let modbus_ready = Arc::new(AtomicBool::new(false));
    let cors: Vec<String> = args
        .cors_origins
        .split(',')
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect();

    let state = AppState {
        device: device.clone(),
        scenarios,
        api_key: args.api_key,
        modbus_port,
        modbus_ready: modbus_ready.clone(),
        scenario_task: Arc::new(Mutex::new(None)),
    };

    info!(
        device = %device.spec().name,
        r#type = %device.spec().device_type,
        api_port = args.api_port,
        modbus_port,
        tick_interval = args.tick,
        "simbus started"
    );

    let tick_device = device.clone();
    let mut tick_task = tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs_f64(
            tick_device.tick_interval().max(0.001),
        ));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let dt = tick_device.tick_interval();
            tick_device.tick(dt);
            let period = Duration::from_secs_f64(dt.max(0.001));
            if ticker.period() != period {
                ticker = interval(period);
                ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
            }
        }
    });

    let modbus_device = device.clone();
    let mut modbus_task = tokio::spawn(async move {
        if let Err(err) = modbus::serve(modbus_device, modbus_port, unit_id, modbus_ready).await {
            error!(error = %err, "modbus server failed");
        }
    });

    let api_host = args.host.clone();
    let api_port = args.api_port;
    let mut api_task = tokio::spawn(async move {
        if let Err(err) = control::serve(state, &api_host, api_port, &cors).await {
            error!(error = %err, "api server failed");
        }
    });

    let graceful = tokio::select! {
        () = shutdown_signal() => true,
        res = &mut tick_task => {
            error!(?res, "tick loop ended");
            false
        }
        res = &mut modbus_task => {
            error!(?res, "modbus task ended");
            false
        }
        res = &mut api_task => {
            error!(?res, "api task ended");
            false
        }
    };

    if graceful {
        info!("simbus stopping");
    }
    tick_task.abort();
    modbus_task.abort();
    api_task.abort();
    if graceful {
        Ok(())
    } else {
        bail!("a runtime task ended unexpectedly");
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
}
