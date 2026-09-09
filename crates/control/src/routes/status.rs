//! Status, config, and probes.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use spec::BehaviorSpec;

use crate::AppState;
use crate::dto::{CoilInfo, ConfigResponse, RegisterInfo, RegisterMapResponse, StatusResponse};

fn behavior_name(spec: Option<&BehaviorSpec>) -> Option<String> {
    spec.map(|b| b.kind_name().to_owned())
}

#[utoipa::path(get, path = "/status", responses((status = 200)))]
pub async fn get_status(State(state): State<AppState>) -> Json<StatusResponse> {
    let spec = state.device.spec();
    Json(StatusResponse {
        name: spec.name.clone(),
        device_type: spec.device_type.clone(),
        modbus_port: state.modbus_port,
        modbus_tls_port: state.modbus_tls_port,
        opcua_port: state.opcua_port,
        tick_interval: state.device.tick_interval(),
        time_scale: state.time_scale,
        simulation: if state.device.is_running() {
            "running"
        } else {
            "stopped"
        },
        modbus_server: if state.field_listening() {
            "listening"
        } else {
            "stopped"
        },
    })
}

#[utoipa::path(get, path = "/config", responses((status = 200)))]
pub async fn get_config(State(state): State<AppState>) -> Json<ConfigResponse> {
    let spec = state.device.spec();
    let map_regs = |regs: &[spec::RegisterSpec]| {
        regs.iter()
            .map(|r| RegisterInfo {
                address: r.address,
                name: r.name.clone(),
                description: r.description.clone(),
                unit: r.unit.clone(),
                scale: r.scale,
                data_type: r.data_type.as_str().to_owned(),
                default: r.default,
                behavior: behavior_name(r.simulation.as_ref()),
            })
            .collect()
    };
    let map_coils = |coils: &[spec::CoilSpec]| {
        coils
            .iter()
            .map(|c| CoilInfo {
                address: c.address,
                name: c.name.clone(),
                description: c.description.clone(),
                default: c.default,
            })
            .collect()
    };
    Json(ConfigResponse {
        name: spec.name.clone(),
        version: spec.version.clone(),
        device_type: spec.device_type.clone(),
        description: spec.description.clone(),
        modbus_port: spec.modbus.default_port,
        unit_id: spec.modbus.unit_id,
        endianness: spec.modbus.endianness.as_str().to_owned(),
        spec_version: spec.spec_version,
        scenarios: spec
            .scenarios
            .iter()
            .map(|s| crate::dto::ScenarioInfo {
                id: s.id.clone(),
                name: s.name.clone(),
                description: s.description.clone(),
                steps: s.steps.len(),
                source: "bundled",
            })
            .collect(),
        registers: RegisterMapResponse {
            holding: map_regs(&spec.registers.holding),
            input: map_regs(&spec.registers.input),
            coils: map_coils(&spec.registers.coils),
            discrete: map_coils(&spec.registers.discrete),
        },
    })
}

#[utoipa::path(get, path = "/healthz", responses((status = 200)))]
pub async fn healthz() -> StatusCode {
    StatusCode::OK
}

#[utoipa::path(get, path = "/readyz", responses((status = 200), (status = 503)))]
pub async fn readyz(State(state): State<AppState>) -> StatusCode {
    if state.field_listening() && state.device.is_running() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

#[utoipa::path(get, path = "/metrics", responses((status = 200)))]
pub async fn metrics(State(state): State<AppState>) -> String {
    let snap = state.device.snapshot();
    format!(
        "# HELP simbus_running 1 if the simulation loop is running\n# TYPE simbus_running gauge\nsimbus_running {}\n\
         # HELP simbus_tick_interval_seconds Wall sample period\n# TYPE simbus_tick_interval_seconds gauge\nsimbus_tick_interval_seconds {}\n\
         # HELP simbus_time_scale Simulation seconds per wall second\n# TYPE simbus_time_scale gauge\nsimbus_time_scale {}\n\
         # HELP simbus_active_faults Active fault count\n# TYPE simbus_active_faults gauge\nsimbus_active_faults {}\n\
         # HELP simbus_holding_registers Holding register word count\n# TYPE simbus_holding_registers gauge\nsimbus_holding_registers {}\n\
         # HELP simbus_input_registers Input register word count\n# TYPE simbus_input_registers gauge\nsimbus_input_registers {}\n\
         # HELP simbus_coils Coil count\n# TYPE simbus_coils gauge\nsimbus_coils {}\n\
         # HELP simbus_discrete Discrete input count\n# TYPE simbus_discrete gauge\nsimbus_discrete {}\n\
         # HELP simbus_sse_subscribers Live GET /registers/stream receivers\n# TYPE simbus_sse_subscribers gauge\nsimbus_sse_subscribers {}\n",
        i32::from(state.device.is_running()),
        state.device.tick_interval(),
        state.time_scale,
        state.device.faults().len(),
        snap.holding.len(),
        snap.input.len(),
        snap.coils.len(),
        snap.discrete.len(),
        state.snapshots.receiver_count(),
    )
}
