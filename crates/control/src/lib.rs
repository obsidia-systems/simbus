//! HTTP control plane for a single simbus device.

#![allow(clippy::missing_errors_doc)]

mod dto;
mod routes;

use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::get;
use engine::{Device, ScenarioRunner, Snapshot};
use http::StatusCode;
use tokio::net::TcpListener;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use tracing::info;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::routes::api_router;
use crate::routes::registers::stream_registers;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    /// Simulated device.
    pub device: Arc<Device>,
    /// Scenario runner.
    pub scenarios: Arc<ScenarioRunner>,
    /// Optional API key for write endpoints.
    pub api_key: Option<String>,
    /// Modbus listen port advertised in `/status`.
    pub modbus_port: u16,
    /// True when the Modbus socket is accepting connections.
    pub modbus_ready: Arc<AtomicBool>,
    /// In-flight scenario task, aborted on stop or when a new scenario starts.
    pub scenario_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    /// Tick snapshots for SSE (`GET /registers/stream`).
    pub snapshots: watch::Sender<Snapshot>,
    /// Simulation seconds per wall second (`--time-scale`). Boot-only.
    pub time_scale: f64,
}

impl AppState {
    /// Push the current bank to SSE subscribers (tick or session write).
    pub fn publish_snapshot(&self) {
        let _ = self.snapshots.send(self.device.snapshot());
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(
        routes::status::get_status,
        routes::status::get_config,
        routes::status::healthz,
        routes::status::readyz,
        routes::status::metrics,
        routes::registers::get_registers,
        routes::registers::override_holding,
        routes::registers::override_input,
        routes::registers::override_coil,
        routes::registers::override_discrete,
        routes::registers::stream_registers,
        routes::simulation::list_faults,
        routes::simulation::inject_fault,
        routes::simulation::clear_faults,
        routes::simulation::patch_simulation,
        routes::simulation::reset_simulation,
        routes::scenarios::list_scenarios,
        routes::scenarios::install_scenario,
        routes::scenarios::uninstall_scenario,
        routes::scenarios::run_scenario,
        routes::scenarios::active_scenario,
        routes::scenarios::stop_scenario,
    ),
    info(
        title = "simbus control plane",
        description = "Session API for one already-booted device (not the Modbus field plane)"
    )
)]
struct ApiDoc;

/// Build the HTTP router.
pub fn router(state: AppState, cors_origins: &[String]) -> Router {
    let cors = if cors_origins.iter().any(|o| o == "*") {
        CorsLayer::permissive()
    } else {
        let origins = cors_origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect::<Vec<_>>();
        CorsLayer::new().allow_origin(AllowOrigin::list(origins))
    };

    let timed = api_router().layer(TimeoutLayer::with_status_code(
        StatusCode::REQUEST_TIMEOUT,
        Duration::from_secs(30),
    ));

    Router::new()
        .merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .route("/registers/stream", get(stream_registers))
        .merge(timed)
        .layer(TraceLayer::new_for_http())
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(cors)
        .with_state(state)
}

/// Bind and serve until `shutdown` resolves, then drain in-flight requests.
pub async fn serve(
    state: AppState,
    host: &str,
    port: u16,
    cors_origins: &[String],
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let app = router(state, cors_origins);
    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let listener = TcpListener::bind(addr).await?;
    info!(host, port, "api listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
}
