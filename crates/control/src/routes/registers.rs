//! Register snapshot, overrides, and SSE.

use std::convert::Infallible;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use serde_json::json;
use spec::RegisterSpace;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::WatchStream;

use crate::AppState;
use crate::dto::{CoilOverrideRequest, ErrorBody, RegisterOverrideRequest, SnapshotResponse};
use crate::routes::require_auth;

#[utoipa::path(get, path = "/registers", responses((status = 200)))]
pub async fn get_registers(State(state): State<AppState>) -> Json<SnapshotResponse> {
    let snap = state.device.snapshot();
    Json(SnapshotResponse {
        holding: snap.holding,
        input: snap.input,
        coils: snap.coils,
        discrete: snap.discrete,
    })
}

async fn override_numeric(
    state: &AppState,
    headers: &HeaderMap,
    space: RegisterSpace,
    address: u16,
    body: RegisterOverrideRequest,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorBody>)> {
    require_auth(state, headers)?;
    if body.value.is_none() && body.real_value.is_none() {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ErrorBody {
                detail: "Provide 'value' (raw uint16) or 'real_value' (physical units).".into(),
            }),
        ));
    }
    if body.value.is_some() && body.real_value.is_some() {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ErrorBody {
                detail: "'value' and 'real_value' are mutually exclusive.".into(),
            }),
        ));
    }
    state
        .device
        .override_register(space, address, body.value, body.real_value, "api")
        .map(|(raw, real)| {
            state.publish_snapshot();
            Json(json!({ "address": address, "raw_value": raw, "real_value": real }))
        })
        .map_err(|e| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorBody {
                    detail: e.to_string(),
                }),
            )
        })
}

#[utoipa::path(
    patch,
    path = "/registers/{address}",
    params(("address" = u16, Path, description = "Holding address")),
    responses((status = 200), (status = 401), (status = 404), (status = 422))
)]
pub async fn override_holding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(address): Path<u16>,
    Json(body): Json<RegisterOverrideRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorBody>)> {
    override_numeric(&state, &headers, RegisterSpace::Holding, address, body).await
}

#[utoipa::path(
    patch,
    path = "/registers/input/{address}",
    params(("address" = u16, Path, description = "Input address")),
    responses((status = 200), (status = 401), (status = 404), (status = 422))
)]
pub async fn override_input(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(address): Path<u16>,
    Json(body): Json<RegisterOverrideRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorBody>)> {
    override_numeric(&state, &headers, RegisterSpace::Input, address, body).await
}

#[utoipa::path(
    patch,
    path = "/registers/coils/{address}",
    params(("address" = u16, Path, description = "Coil address")),
    responses((status = 200), (status = 401), (status = 404))
)]
pub async fn override_coil(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(address): Path<u16>,
    Json(body): Json<CoilOverrideRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorBody>)> {
    require_auth(&state, &headers)?;
    state
        .device
        .override_coil(address, body.value)
        .map(|value| {
            state.publish_snapshot();
            Json(json!({ "address": address, "value": value }))
        })
        .map_err(|e| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorBody {
                    detail: e.to_string(),
                }),
            )
        })
}

#[utoipa::path(
    patch,
    path = "/registers/discrete/{address}",
    params(("address" = u16, Path, description = "Discrete address")),
    responses((status = 200), (status = 401), (status = 404))
)]
pub async fn override_discrete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(address): Path<u16>,
    Json(body): Json<CoilOverrideRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorBody>)> {
    require_auth(&state, &headers)?;
    state
        .device
        .override_discrete(address, body.value)
        .map(|value| {
            state.publish_snapshot();
            Json(json!({ "address": address, "value": value }))
        })
        .map_err(|e| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorBody {
                    detail: e.to_string(),
                }),
            )
        })
}

#[utoipa::path(get, path = "/registers/stream", responses((status = 200)))]
pub async fn stream_registers(
    State(state): State<AppState>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let rx = state.snapshots.subscribe();
    let stream = WatchStream::new(rx).map(|snap| {
        let payload = serde_json::json!({
            "holding": snap.holding,
            "input": snap.input,
            "coils": snap.coils,
            "discrete": snap.discrete,
        });
        Ok(Event::default().data(payload.to_string()))
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
}
