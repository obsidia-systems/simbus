//! Scenario list / run / stop. Catalog comes from the loaded device spec.

use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use serde_json::json;
use tokio::time::sleep;

use crate::AppState;
use crate::dto::ErrorBody;
use crate::routes::authorize;

#[utoipa::path(get, path = "/scenarios", responses((status = 200)))]
pub async fn list_scenarios(State(state): State<AppState>) -> Json<Vec<serde_json::Value>> {
    let spec = state.device.spec();
    Json(
        spec.scenarios
            .iter()
            .map(|s| {
                json!({
                    "id": s.id,
                    "name": s.name,
                    "description": s.description,
                    "steps": s.steps.len(),
                })
            })
            .collect(),
    )
}

#[utoipa::path(post, path = "/scenarios/{name}/run", responses((status = 202), (status = 404)))]
pub async fn run_scenario(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<ErrorBody>)> {
    authorize(&state, &headers).map_err(|s| {
        (
            s,
            Json(ErrorBody {
                detail: "unauthorized".into(),
            }),
        )
    })?;
    let spec = match state.device.spec().scenario(&name) {
        Some(s) => s.clone(),
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ErrorBody {
                    detail: format!("Scenario '{name}' not found"),
                }),
            ));
        }
    };
    let steps_n = spec.steps.len();
    state.scenarios.mark_stopped();
    if let Some(handle) = state.scenario_task.lock().unwrap().take() {
        handle.abort();
    }
    let generation = state.scenarios.begin(&spec);
    let runner = state.scenarios.clone();
    let handle = tokio::spawn(async move {
        let mut steps = spec.steps.clone();
        steps.sort_by(|a, b| a.at().total_cmp(&b.at()));
        let start = Instant::now();
        for (idx, step) in steps.iter().enumerate() {
            if !runner.is_running(generation) {
                return;
            }
            let target = Duration::from_secs_f64(step.at().max(0.0));
            let now = start.elapsed();
            if target > now {
                sleep(target - now).await;
            }
            if !runner.is_running(generation) {
                return;
            }
            runner.device().apply_step(step);
            runner.note_step(generation, idx + 1, start.elapsed().as_secs_f64());
        }
        runner.mark_completed(generation, start.elapsed().as_secs_f64());
    });
    *state.scenario_task.lock().unwrap() = Some(handle);
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "status": "started", "scenario": name, "steps": steps_n })),
    ))
}

#[utoipa::path(get, path = "/scenarios/active", responses((status = 200)))]
pub async fn active_scenario(State(state): State<AppState>) -> Json<serde_json::Value> {
    let s = state.scenarios.status();
    Json(json!({
        "state": s.state.as_str(),
        "scenario_name": s.scenario_name,
        "step_index": s.step_index,
        "total_steps": s.total_steps,
        "elapsed_s": (s.elapsed_s * 1000.0).round() / 1000.0,
    }))
}

#[utoipa::path(post, path = "/scenarios/stop", responses((status = 204)))]
pub async fn stop_scenario(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    authorize(&state, &headers)?;
    state.scenarios.mark_stopped();
    if let Some(handle) = state.scenario_task.lock().unwrap().take() {
        handle.abort();
    }
    Ok(StatusCode::NO_CONTENT)
}
