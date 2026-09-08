//! HTTP client for an already-running simbus process (`simbus ctl`).

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use reqwest::Method;
use serde_json::{Value, json};

#[derive(Args, Debug)]
pub struct CtlArgs {
    /// Base URL of the control plane
    #[arg(long, env = "SIMBUS_CTL_URL", default_value = "http://127.0.0.1:8000")]
    url: String,
    /// API key for write endpoints (`x-api-key`)
    #[arg(long, env = "SIMBUS_API_KEY")]
    api_key: Option<String>,
    #[command(subcommand)]
    command: CtlCommand,
}

#[derive(Subcommand, Debug)]
enum CtlCommand {
    /// GET /status
    Status,
    /// GET /config
    Config,
    /// GET /healthz
    Healthz,
    /// GET /readyz
    Readyz,
    /// GET /metrics
    Metrics,
    /// GET /registers
    Registers,
    /// PATCH a holding or input register
    Set {
        address: u16,
        #[arg(long)]
        real_value: Option<f64>,
        #[arg(long)]
        value: Option<u16>,
        #[arg(long)]
        input: bool,
    },
    /// PATCH a coil or discrete input
    Coil {
        address: u16,
        #[arg(long)]
        value: bool,
        #[arg(long)]
        discrete: bool,
    },
    /// GET /faults
    Faults,
    /// POST /faults
    Fault {
        #[arg(long = "type")]
        fault_type: String,
        #[arg(long)]
        register: Option<String>,
        #[arg(long)]
        value: Option<f64>,
        #[arg(long, default_value_t = 30.0)]
        duration: f64,
    },
    /// DELETE /faults
    ClearFaults,
    /// PATCH /simulation
    Tick {
        #[arg(long)]
        interval: f64,
    },
    /// POST /simulation/reset
    Reset,
    /// GET /scenarios
    Scenarios,
    /// POST /scenarios/{id}/run
    Run { id: String },
    /// GET /scenarios/active
    Active,
    /// POST /scenarios/stop
    Stop,
}

pub async fn run(args: CtlArgs) -> Result<()> {
    let base = args.url.trim_end_matches('/');
    let key = args.api_key.as_deref();
    match args.command {
        CtlCommand::Status => get(base, "/status", key).await,
        CtlCommand::Config => get(base, "/config", key).await,
        CtlCommand::Healthz => get(base, "/healthz", key).await,
        CtlCommand::Readyz => get(base, "/readyz", key).await,
        CtlCommand::Metrics => get(base, "/metrics", key).await,
        CtlCommand::Registers => get(base, "/registers", key).await,
        CtlCommand::Set {
            address,
            real_value,
            value,
            input,
        } => {
            let body = register_body(real_value, value)?;
            let path = if input {
                format!("/registers/input/{address}")
            } else {
                format!("/registers/{address}")
            };
            send(Method::PATCH, base, &path, key, Some(body)).await
        }
        CtlCommand::Coil {
            address,
            value,
            discrete,
        } => {
            let path = if discrete {
                format!("/registers/discrete/{address}")
            } else {
                format!("/registers/coils/{address}")
            };
            send(
                Method::PATCH,
                base,
                &path,
                key,
                Some(json!({ "value": value })),
            )
            .await
        }
        CtlCommand::Faults => get(base, "/faults", key).await,
        CtlCommand::Fault {
            fault_type,
            register,
            value,
            duration,
        } => {
            send(
                Method::POST,
                base,
                "/faults",
                key,
                Some(json!({
                    "fault_type": fault_type,
                    "register_name": register,
                    "value": value,
                    "duration_s": duration,
                })),
            )
            .await
        }
        CtlCommand::ClearFaults => send(Method::DELETE, base, "/faults", key, None).await,
        CtlCommand::Tick { interval } => {
            send(
                Method::PATCH,
                base,
                "/simulation",
                key,
                Some(json!({ "tick_interval": interval })),
            )
            .await
        }
        CtlCommand::Reset => send(Method::POST, base, "/simulation/reset", key, None).await,
        CtlCommand::Scenarios => get(base, "/scenarios", key).await,
        CtlCommand::Run { id } => {
            send(
                Method::POST,
                base,
                &format!("/scenarios/{id}/run"),
                key,
                None,
            )
            .await
        }
        CtlCommand::Active => get(base, "/scenarios/active", key).await,
        CtlCommand::Stop => send(Method::POST, base, "/scenarios/stop", key, None).await,
    }
}

fn register_body(real_value: Option<f64>, value: Option<u16>) -> Result<Value> {
    match (real_value, value) {
        (Some(real), None) => Ok(json!({ "real_value": real })),
        (None, Some(raw)) => Ok(json!({ "value": raw })),
        (None, None) => bail!("provide --real-value or --value"),
        (Some(_), Some(_)) => bail!("--real-value and --value are mutually exclusive"),
    }
}

async fn get(base: &str, path: &str, api_key: Option<&str>) -> Result<()> {
    send(Method::GET, base, path, api_key, None).await
}

async fn send(
    method: Method,
    base: &str,
    path: &str,
    api_key: Option<&str>,
    body: Option<Value>,
) -> Result<()> {
    let url = format!("{base}{path}");
    let client = reqwest::Client::new();
    let mut req = client.request(method, &url);
    if let Some(key) = api_key {
        req = req.header("x-api-key", key);
    }
    if let Some(json) = body {
        req = req.json(&json);
    }
    let response = req.send().await.with_context(|| url.clone())?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !text.is_empty() {
        if let Ok(value) = serde_json::from_str::<Value>(&text) {
            println!("{}", serde_json::to_string_pretty(&value)?);
        } else {
            print!("{text}");
            if !text.ends_with('\n') {
                println!();
            }
        }
    }
    if status.is_success() {
        Ok(())
    } else {
        bail!("{url} -> {status}");
    }
}
