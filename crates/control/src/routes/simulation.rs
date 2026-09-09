//! Faults and simulation control.

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use engine::ActiveFault;
use serde_json::json;
use spec::{DeviceSpec, FaultType};

use crate::AppState;
use crate::dto::{ErrorBody, FaultRequest, FaultResponse, SimulationPatchRequest};
use crate::routes::require_auth;

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

fn has_numeric(spec: &DeviceSpec, name: &str) -> bool {
    spec.registers
        .holding
        .iter()
        .chain(spec.registers.input.iter())
        .any(|r| r.name == name)
}

fn has_coil(spec: &DeviceSpec, name: &str) -> bool {
    spec.registers.coils.iter().any(|c| c.name == name)
}

fn validate_fault(
    spec: &DeviceSpec,
    body: &FaultRequest,
) -> Result<(), (StatusCode, Json<ErrorBody>)> {
    if body.duration_s <= 0.0 {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ErrorBody {
                detail: "duration_s must be > 0".into(),
            }),
        ));
    }
    match body.fault_type {
        FaultType::Dropout if body.register_name.is_none() => Ok(()),
        FaultType::Spike | FaultType::Freeze | FaultType::NoiseAmplify | FaultType::Dropout => {
            let Some(name) = body.register_name.as_deref() else {
                return Err((
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(ErrorBody {
                        detail:
                            "register_name is required (null is only valid for device-wide dropout)"
                                .into(),
                    }),
                ));
            };
            if body.fault_type == FaultType::Spike && body.value.is_none() {
                return Err((
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(ErrorBody {
                        detail: "spike requires 'value' (real-world units)".into(),
                    }),
                ));
            }
            if has_numeric(spec, name) {
                Ok(())
            } else {
                Err((
                    StatusCode::NOT_FOUND,
                    Json(ErrorBody {
                        detail: format!("register '{name}' not found"),
                    }),
                ))
            }
        }
        FaultType::Alarm => {
            let Some(name) = body.register_name.as_deref() else {
                return Err((
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(ErrorBody {
                        detail: "alarm requires register_name (coil name)".into(),
                    }),
                ));
            };
            if has_coil(spec, name) {
                Ok(())
            } else {
                Err((
                    StatusCode::NOT_FOUND,
                    Json(ErrorBody {
                        detail: format!("coil '{name}' not found"),
                    }),
                ))
            }
        }
    }
}

#[utoipa::path(
    post,
    path = "/faults",
    responses((status = 202), (status = 401), (status = 404), (status = 422))
)]
pub async fn inject_fault(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<FaultRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<ErrorBody>)> {
    require_auth(&state, &headers)?;
    validate_fault(state.device.spec(), &body)?;
    state.device.inject_fault(ActiveFault::new(
        body.fault_type,
        body.register_name,
        body.value,
        body.duration_s,
    ));
    state.publish_snapshot();
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "status": "accepted", "fault_type": body.fault_type })),
    ))
}

#[utoipa::path(delete, path = "/faults", responses((status = 204), (status = 401)))]
pub async fn clear_faults(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, (StatusCode, Json<ErrorBody>)> {
    require_auth(&state, &headers)?;
    state.device.clear_faults();
    state.publish_snapshot();
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
    require_auth(&state, &headers)?;
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
    if let Some(running) = body.running {
        state.device.set_running(running);
    }
    Ok(Json(json!({
        "tick_interval": state.device.tick_interval(),
        "running": state.device.is_running(),
    })))
}

#[utoipa::path(post, path = "/simulation/reset", responses((status = 204), (status = 401)))]
pub async fn reset_simulation(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, (StatusCode, Json<ErrorBody>)> {
    require_auth(&state, &headers)?;
    state.device.reset();
    state.publish_snapshot();
    Ok(StatusCode::NO_CONTENT)
}
