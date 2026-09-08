//! HTTP routes.

pub mod registers;
pub mod scenarios;
pub mod simulation;
pub mod status;

use axum::Router;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, patch, post};

use crate::AppState;

pub fn api_router() -> Router<AppState> {
    Router::new()
        .route("/status", get(status::get_status))
        .route("/config", get(status::get_config))
        .route("/healthz", get(status::healthz))
        .route("/readyz", get(status::readyz))
        .route("/metrics", get(status::metrics))
        .route("/registers", get(registers::get_registers))
        .route("/registers/{address}", patch(registers::override_holding))
        .route(
            "/registers/input/{address}",
            patch(registers::override_input),
        )
        .route(
            "/registers/coils/{address}",
            patch(registers::override_coil),
        )
        .route(
            "/registers/discrete/{address}",
            patch(registers::override_discrete),
        )
        .route(
            "/faults",
            get(simulation::list_faults)
                .post(simulation::inject_fault)
                .delete(simulation::clear_faults),
        )
        .route("/simulation", patch(simulation::patch_simulation))
        .route("/simulation/reset", post(simulation::reset_simulation))
        .route("/scenarios", get(scenarios::list_scenarios))
        .route("/scenarios/active", get(scenarios::active_scenario))
        .route("/scenarios/{name}/run", post(scenarios::run_scenario))
        .route("/scenarios/stop", post(scenarios::stop_scenario))
}

pub(crate) fn authorize(state: &AppState, headers: &HeaderMap) -> Result<(), StatusCode> {
    let Some(expected) = &state.api_key else {
        return Ok(());
    };
    let provided = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .or_else(|| {
            headers
                .get(http::header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
        });
    if provided == Some(expected.as_str()) {
        Ok(())
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}
