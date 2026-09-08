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
        modbus_ready: Arc::new(AtomicBool::new(true)),
        scenario_task: Arc::new(Mutex::new(None)),
        snapshots,
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
    assert_eq!(json["simulation"], "running");
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
