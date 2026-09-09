use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use control::AppState;
use engine::{Device, ScenarioRunner};
use http_body_util::BodyExt;
use spec::load_device_from_path;
use tokio::sync::watch;
use tokio::time::timeout;
use tower::ServiceExt;

fn tnh_state() -> AppState {
    tnh_state_with_key(None)
}

fn tnh_state_with_key(api_key: Option<String>) -> AppState {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../devices/builtin/generic-tnh-sensor.yaml");
    let spec = load_device_from_path(path).unwrap();
    let device = Device::new(spec, Some(1), 1.0);
    device.set_running(true);
    let (snapshots, _) = watch::channel(device.snapshot());
    AppState {
        device: device.clone(),
        scenarios: Arc::new(ScenarioRunner::new(device)),
        api_key,
        modbus_port: 5020,
        modbus_tls_port: None,
        opcua_port: None,
        modbus_ready: Arc::new(AtomicBool::new(true)),
        modbus_tls_ready: Arc::new(AtomicBool::new(true)),
        opcua_ready: Arc::new(AtomicBool::new(true)),
        scenario_task: Arc::new(Mutex::new(None)),
        snapshots,
        time_scale: 1.0,
    }
}

fn app() -> axum::Router {
    control::router(tnh_state(), &["*".to_owned()])
}

#[tokio::test]
async fn healthz_ok() {
    let response = app()
        .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn status_reports_device() {
    let response = app()
        .oneshot(Request::get("/status").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["name"], "Generic T&H Sensor");
    assert_eq!(json["modbus_port"], 5020);
    assert!(json["modbus_tls_port"].is_null());
    assert!(json["opcua_port"].is_null());
    assert_eq!(json["simulation"], "running");
    assert_eq!(json["time_scale"], 1.0);
}

#[tokio::test]
async fn config_uses_snake_case_endianness() {
    let response = app()
        .oneshot(Request::get("/config").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["endianness"], "big");
    assert_eq!(json["registers"]["holding"][0]["data_type"], "uint16");
    assert_eq!(
        json["registers"]["holding"][0]["behavior"],
        "gaussian_noise"
    );
}

#[tokio::test]
async fn lists_bundled_scenarios() {
    let response = app()
        .oneshot(Request::get("/scenarios").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let ids: Vec<&str> = json
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"heat-wave"));
    assert!(ids.contains(&"fast-alarm-test"));
}

#[tokio::test]
async fn unknown_scenario_is_404() {
    let response = app()
        .oneshot(
            Request::post("/scenarios/not-from-this-device/run")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn writes_require_api_key_when_configured() {
    let app = control::router(tnh_state_with_key(Some("secret".into())), &["*".to_owned()]);
    let denied = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/simulation/reset")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);

    let allowed = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/simulation/reset")
                .header("x-api-key", "secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(allowed.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn openapi_lists_session_routes() {
    let response = app()
        .oneshot(
            Request::get("/api-docs/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let paths = json["paths"].as_object().unwrap();
    for path in [
        "/status",
        "/config",
        "/registers",
        "/registers/stream",
        "/registers/{address}",
        "/faults",
        "/simulation",
        "/simulation/reset",
        "/scenarios",
        "/scenarios/{name}",
        "/scenarios/{name}/run",
    ] {
        assert!(paths.contains_key(path), "missing OpenAPI path {path}");
    }
}

#[tokio::test]
async fn sse_emits_current_snapshot() {
    use tower::{Service, ServiceExt};
    let mut app = app();
    let request = Request::get("/registers/stream")
        .body(Body::empty())
        .unwrap();
    let response = ServiceExt::<Request<Body>>::ready(&mut app)
        .await
        .unwrap()
        .call(request)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let frame = timeout(Duration::from_secs(2), response.into_body().frame())
        .await
        .expect("sse frame timed out")
        .expect("body error")
        .expect("eof");
    let bytes = frame.into_data().expect("data frame");
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("holding") || text.contains("data:"),
        "unexpected sse payload: {text}"
    );
}

fn json_request(method: &str, uri: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_owned()))
        .unwrap()
}

#[tokio::test]
async fn get_registers_returns_defaults() {
    let response = app()
        .oneshot(Request::get("/registers").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["holding"]["0"], 225);
}

#[tokio::test]
async fn patch_holding_shifts_base() {
    let response = app()
        .oneshot(json_request(
            "PATCH",
            "/registers/0",
            r#"{"real_value": 27.0}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["raw_value"], 270);
    assert_eq!(json["real_value"], 27.0);
}

#[tokio::test]
async fn patch_rejects_both_value_fields() {
    let response = app()
        .oneshot(json_request(
            "PATCH",
            "/registers/0",
            r#"{"value": 270, "real_value": 27.0}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn inject_fault_rejects_unknown_register() {
    let response = app()
        .oneshot(json_request(
            "POST",
            "/faults",
            r#"{"fault_type":"spike","register_name":"nope","value":35.0,"duration_s":5}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn inject_spike_requires_value() {
    let response = app()
        .oneshot(json_request(
            "POST",
            "/faults",
            r#"{"fault_type":"spike","register_name":"temperature","duration_s":5}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn inject_known_spike_is_accepted() {
    let response = app()
        .oneshot(json_request(
            "POST",
            "/faults",
            r#"{"fault_type":"spike","register_name":"temperature","value":35.0,"duration_s":5}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
}

#[tokio::test]
async fn readyz_unavailable_when_not_running() {
    let state = tnh_state();
    state.device.set_running(false);
    let response = control::router(state, &["*".to_owned()])
        .oneshot(Request::get("/readyz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn status_and_readyz_dual_bind() {
    let mut state = tnh_state();
    state.modbus_tls_port = Some(802);
    state.modbus_tls_ready = Arc::new(AtomicBool::new(false));
    let app = control::router(state.clone(), &["*".to_owned()]);
    let status = app
        .clone()
        .oneshot(Request::get("/status").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let body = status.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["modbus_tls_port"], 802);
    assert_eq!(json["modbus_server"], "stopped");
    let readyz = app
        .oneshot(Request::get("/readyz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(readyz.status(), StatusCode::SERVICE_UNAVAILABLE);

    state
        .modbus_tls_ready
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let app = control::router(state, &["*".to_owned()]);
    let status = app
        .clone()
        .oneshot(Request::get("/status").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let body = status.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["modbus_server"], "listening");
    let readyz = app
        .oneshot(Request::get("/readyz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(readyz.status(), StatusCode::OK);
}

#[tokio::test]
async fn readyz_tls_only() {
    let mut state = tnh_state();
    state.modbus_tls_port = Some(802);
    state.modbus_ready = Arc::new(AtomicBool::new(true));
    state.modbus_tls_ready = Arc::new(AtomicBool::new(true));
    let response = control::router(state, &["*".to_owned()])
        .oneshot(Request::get("/readyz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn status_and_readyz_opcua() {
    let mut state = tnh_state();
    state.opcua_port = Some(4840);
    state.opcua_ready = Arc::new(AtomicBool::new(false));
    let app = control::router(state.clone(), &["*".to_owned()]);
    let status = app
        .clone()
        .oneshot(Request::get("/status").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let body = status.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["opcua_port"], 4840);
    assert_eq!(json["modbus_server"], "stopped");
    let readyz = app
        .oneshot(Request::get("/readyz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(readyz.status(), StatusCode::SERVICE_UNAVAILABLE);

    state
        .opcua_ready
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let app = control::router(state, &["*".to_owned()]);
    let readyz = app
        .oneshot(Request::get("/readyz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(readyz.status(), StatusCode::OK);
}

#[tokio::test]
async fn pause_and_resume_via_patch() {
    let state = tnh_state();
    let app = control::router(state.clone(), &["*".to_owned()]);
    let paused = app
        .clone()
        .oneshot(json_request(
            "PATCH",
            "/simulation",
            r#"{"running": false}"#,
        ))
        .await
        .unwrap();
    assert_eq!(paused.status(), StatusCode::OK);
    let body = paused.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["running"], false);
    assert!(!state.device.is_running());

    let readyz = app
        .clone()
        .oneshot(Request::get("/readyz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(readyz.status(), StatusCode::SERVICE_UNAVAILABLE);

    let resumed = app
        .oneshot(json_request("PATCH", "/simulation", r#"{"running": true}"#))
        .await
        .unwrap();
    assert_eq!(resumed.status(), StatusCode::OK);
    assert!(state.device.is_running());
}

#[tokio::test]
async fn install_session_scenario_and_reject_bundled_id() {
    let app = app();
    let created = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/scenarios",
            r#"{"id":"lab-spike","name":"Lab spike","steps":[{"action":"set_register","at":0,"register_name":"temperature","value":30.0}]}"#,
        ))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);

    let listed = app
        .clone()
        .oneshot(Request::get("/scenarios").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let body = listed.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let lab = json
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "lab-spike")
        .unwrap();
    assert_eq!(lab["source"], "session");

    let conflict = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/scenarios",
            r#"{"id":"heat-wave","name":"Nope","steps":[{"action":"set_register","at":0,"register_name":"temperature","value":30.0}]}"#,
        ))
        .await
        .unwrap();
    assert_eq!(conflict.status(), StatusCode::CONFLICT);

    let bad = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/scenarios",
            r#"{"id":"lab-bad","name":"Bad","steps":[{"action":"set_register","at":0,"register_name":"nope","value":1.0}]}"#,
        ))
        .await
        .unwrap();
    assert_eq!(bad.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let removed = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/scenarios/lab-spike")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(removed.status(), StatusCode::NO_CONTENT);
}
