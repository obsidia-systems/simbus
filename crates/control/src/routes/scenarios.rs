//! Scenario list / run / stop. Catalog is YAML plus session copies.

use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use serde_json::json;
use spec::ScenarioSpec;
use tokio::time::sleep;

use crate::AppState;
use crate::dto::{ErrorBody, ScenarioInfo};
use crate::routes::require_auth;

fn bundled_info(s: &ScenarioSpec) -> ScenarioInfo {
    ScenarioInfo {
        id: s.id.clone(),
        name: s.name.clone(),
        description: s.description.clone(),
        steps: s.steps.len(),
        source: "bundled",
    }
}

fn session_info(s: &ScenarioSpec) -> ScenarioInfo {
    ScenarioInfo {
        id: s.id.clone(),
        name: s.name.clone(),
        description: s.description.clone(),
        steps: s.steps.len(),
        source: "session",
    }
}

fn resolve_scenario(state: &AppState, id: &str) -> Option<ScenarioSpec> {
    state
        .device
        .spec()
        .scenario(id)
        .cloned()
        .or_else(|| state.scenarios.session_scenario(id))
}

fn stop_playback(state: &AppState) {
    state.scenarios.mark_stopped();
    if let Some(handle) = state.scenario_task.lock().unwrap().take() {
        handle.abort();
    }
}

/// Sleep until `target` of unpaused wall time has elapsed, or playback is cancelled.
async fn wait_unpaused(
    runner: &engine::ScenarioRunner,
    generation: u64,
    start: Instant,
    mut paused: Duration,
    target: Duration,
) -> Option<Duration> {
    loop {
        if !runner.is_running(generation) {
            return None;
        }
        if !runner.device().is_running() {
            let t0 = Instant::now();
            sleep(Duration::from_millis(50)).await;
            paused += t0.elapsed();
            continue;
        }
        let unpaused = start.elapsed().saturating_sub(paused);
        if unpaused >= target {
            return Some(paused);
        }
        let slice = (target - unpaused).min(Duration::from_millis(50));
        sleep(slice).await;
    }
}

#[utoipa::path(get, path = "/scenarios", responses((status = 200)))]
pub async fn list_scenarios(State(state): State<AppState>) -> Json<Vec<ScenarioInfo>> {
    let spec = state.device.spec();
    let mut out: Vec<ScenarioInfo> = spec.scenarios.iter().map(bundled_info).collect();
    let mut session = state.scenarios.session_scenarios();
    session.sort_by(|a, b| a.id.cmp(&b.id));
    out.extend(session.iter().map(session_info));
    Json(out)
}

#[utoipa::path(
    post,
    path = "/scenarios",
    responses((status = 201), (status = 401), (status = 409), (status = 422))
)]
pub async fn install_scenario(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ScenarioSpec>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<ErrorBody>)> {
    require_auth(&state, &headers)?;
    if state.device.spec().scenario(&body.id).is_some() {
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorBody {
                detail: format!("scenario '{}' is bundled in the device YAML", body.id),
            }),
        ));
    }
    if let Err(err) = state.device.spec().validate_guest_scenario(&body) {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ErrorBody {
                detail: err.to_string(),
            }),
        ));
    }
    let id = body.id.clone();
    state.scenarios.install_session(body);
    Ok((
        StatusCode::CREATED,
        Json(json!({ "id": id, "source": "session" })),
    ))
}

#[utoipa::path(
    delete,
    path = "/scenarios/{name}",
    responses((status = 204), (status = 401), (status = 404), (status = 409))
)]
pub async fn uninstall_scenario(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<ErrorBody>)> {
    require_auth(&state, &headers)?;
    if state.device.spec().scenario(&name).is_some() {
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorBody {
                detail: format!("scenario '{name}' is bundled in the device YAML"),
            }),
        ));
    }
    if state.scenarios.session_scenario(&name).is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                detail: format!("Scenario '{name}' not found"),
            }),
        ));
    }
    if state.scenarios.status().scenario_name.as_deref() == Some(name.as_str()) {
        stop_playback(&state);
    }
    state.scenarios.remove_session(&name);
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(post, path = "/scenarios/{name}/run", responses((status = 202), (status = 404)))]
pub async fn run_scenario(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<ErrorBody>)> {
    require_auth(&state, &headers)?;
    let spec = match resolve_scenario(&state, &name) {
        Some(s) => s,
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
    stop_playback(&state);
    let generation = state.scenarios.begin(&spec);
    let runner = state.scenarios.clone();
    let scale = state.time_scale.max(f64::EPSILON);
    let handle = tokio::spawn(async move {
        let mut steps = spec.steps.clone();
        steps.sort_by(|a, b| a.at().total_cmp(&b.at()));
        let start = Instant::now();
        let mut paused = Duration::ZERO;
        for (idx, step) in steps.iter().enumerate() {
            if !runner.is_running(generation) {
                return;
            }
            let target = Duration::from_secs_f64((step.at().max(0.0) / scale).max(0.0));
            let Some(new_paused) = wait_unpaused(&runner, generation, start, paused, target).await
            else {
                return;
            };
            paused = new_paused;
            if !runner.is_running(generation) {
                return;
            }
            runner.device().apply_step(step);
            let sim_elapsed = start.elapsed().saturating_sub(paused).as_secs_f64() * scale;
            runner.note_step(generation, idx + 1, sim_elapsed);
        }
        let sim_elapsed = start.elapsed().saturating_sub(paused).as_secs_f64() * scale;
        runner.mark_completed(generation, sim_elapsed);
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
) -> Result<StatusCode, (StatusCode, Json<ErrorBody>)> {
    require_auth(&state, &headers)?;
    stop_playback(&state);
    Ok(StatusCode::NO_CONTENT)
}
