//! Canonical point snapshot, overrides, and SSE.

use std::convert::Infallible;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use engine::PointView;
use serde_json::json;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::WatchStream;

use crate::AppState;
use crate::dto::{ErrorBody, PointOverrideRequest, PointResponse};
use crate::routes::require_auth;

fn to_response(view: PointView) -> PointResponse {
    let value = match view.kind {
        spec::PointKind::Analog => view.analog.map(serde_json::Value::from),
        spec::PointKind::Binary => view.binary.map(serde_json::Value::from),
    };
    PointResponse {
        id: view.id,
        kind: view.kind.as_str().to_owned(),
        class: view.class.as_str().to_owned(),
        description: view.description,
        unit: view.unit,
        value,
    }
}

#[utoipa::path(get, path = "/points", responses((status = 200)))]
pub async fn get_points(State(state): State<AppState>) -> Json<Vec<PointResponse>> {
    Json(
        state
            .device
            .points_snapshot()
            .into_iter()
            .map(to_response)
            .collect(),
    )
}

#[utoipa::path(
    get,
    path = "/points/{id}",
    params(("id" = String, Path, description = "Point id")),
    responses((status = 200), (status = 404))
)]
pub async fn get_point(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<PointResponse>, (StatusCode, Json<ErrorBody>)> {
    state
        .device
        .point_view(&id)
        .map(|view| Json(to_response(view)))
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorBody {
                    detail: format!("point '{id}' not found"),
                }),
            )
        })
}

#[utoipa::path(
    patch,
    path = "/points/{id}",
    params(("id" = String, Path, description = "Point id")),
    responses((status = 200), (status = 401), (status = 404), (status = 422))
)]
pub async fn override_point(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<PointOverrideRequest>,
) -> Result<Json<PointResponse>, (StatusCode, Json<ErrorBody>)> {
    require_auth(&state, &headers)?;
    let (analog, binary) = match &body.value {
        serde_json::Value::Bool(v) => (None, Some(*v)),
        serde_json::Value::Number(n) => {
            let Some(v) = n.as_f64() else {
                return Err((
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(ErrorBody {
                        detail: "value must be a finite number or a boolean".into(),
                    }),
                ));
            };
            (Some(v), None)
        }
        _ => {
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ErrorBody {
                    detail: "value must be a number (analog) or a boolean (binary)".into(),
                }),
            ));
        }
    };
    state
        .device
        .override_point(&id, analog, binary, "api")
        .map(|view| {
            state.publish_snapshot();
            Json(to_response(view))
        })
        .map_err(|e| {
            let status = match e {
                engine::DeviceError::UnknownPoint(_) => StatusCode::NOT_FOUND,
                engine::DeviceError::PointValueMismatch(_) => StatusCode::UNPROCESSABLE_ENTITY,
                _ => StatusCode::NOT_FOUND,
            };
            (
                status,
                Json(ErrorBody {
                    detail: e.to_string(),
                }),
            )
        })
}

#[utoipa::path(get, path = "/points/stream", responses((status = 200)))]
pub async fn stream_points(
    State(state): State<AppState>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let device = state.device.clone();
    let rx = state.snapshots.subscribe();
    let stream = WatchStream::new(rx).map(move |_| {
        let payload = json!(
            device
                .points_snapshot()
                .into_iter()
                .map(to_response)
                .collect::<Vec<_>>()
        );
        Ok(Event::default().data(payload.to_string()))
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
}
