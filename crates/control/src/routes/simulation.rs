//! Faults and simulation control.

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use engine::ActiveFault;
use serde_json::json;

use crate::AppState;
use crate::dto::{ErrorBody, FaultRequest, FaultResponse, SimulationPatchRequest};
use crate::routes::authorize;

#[utoipa::path(get, path = "/faults", responses((status = 200)))]
pub async fn list_faults(State(state): State<AppState>) -> Json<Vec<FaultResponse>> {
    Json(
        state
            .device
            .faults()
            .into_iter()
            .map(|f| FaultResponse {
                fault_type: f.fault_type,
                register_name: f.register_name,
                value: f.value,
                duration_s: f.duration_s,
                remaining_s: f.remaining_s,
            })
            .collect(),
    )
}

#[utoipa::path(
    post,
    path = "/faults",
    responses((status = 202), (status = 401), (status = 422))
)]
pub async fn inject_fault(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<FaultRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<ErrorBody>)> {
    authorize(&state, &headers).map_err(|s| {
        (
            s,
            Json(ErrorBody {
                detail: "unauthorized".into(),
            }),
        )
    })?;
    if body.duration_s <= 0.0 {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ErrorBody {
                detail: "duration_s must be > 0".into(),
            }),
        ));
    }
    state.device.inject_fault(ActiveFault::new(
        body.fault_type,
        body.register_name,
        body.value,
        body.duration_s,
    ));
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "status": "accepted", "fault_type": body.fault_type })),
    ))
}

#[utoipa::path(delete, path = "/faults", responses((status = 204), (status = 401)))]
pub async fn clear_faults(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    authorize(&state, &headers)?;
    state.device.clear_faults();
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    patch,
    path = "/simulation",
    responses((status = 200), (status = 401), (status = 422))
)]
pub async fn patch_simulation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SimulationPatchRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorBody>)> {
    authorize(&state, &headers).map_err(|s| {
        (
            s,
            Json(ErrorBody {
                detail: "unauthorized".into(),
            }),
        )
    })?;
    if let Some(tick) = body.tick_interval {
        if tick <= 0.0 {
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ErrorBody {
                    detail: "tick_interval must be > 0".into(),
                }),
            ));
        }
        state.device.set_tick_interval(tick);
    }
    Ok(Json(
        json!({ "tick_interval": state.device.tick_interval() }),
    ))
}

#[utoipa::path(post, path = "/simulation/reset", responses((status = 204), (status = 401)))]
pub async fn reset_simulation(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    authorize(&state, &headers)?;
    state.device.reset();
    Ok(StatusCode::NO_CONTENT)
}
