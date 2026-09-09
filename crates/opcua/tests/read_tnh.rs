use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use engine::Device;
use spec::load_device_from_path;
use ua::client::{ClientBuilder, IdentityToken};
use ua::types::{
    EndpointDescription, MessageSecurityMode, NodeId, TimestampsToReturn, UserTokenPolicy, Variant,
};

fn ephemeral_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

#[tokio::test]
async fn reads_tnh_temperature_as_engineering_float() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../devices/builtin/generic-tnh-sensor.yaml");
    let spec = load_device_from_path(&path).unwrap();
    let device = Device::new(spec, Some(1), 1.0);
    let port = ephemeral_port();
    let ready = Arc::new(AtomicBool::new(false));
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let ready_c = ready.clone();
    let device_c = device.clone();
    let server = tokio::spawn(async move {
        let stop = async move {
            let _ = stop_rx.await;
        };
        opcua::serve(device_c, port, ready_c, stop).await
    });

    tokio::time::timeout(Duration::from_secs(15), async {
        while !ready.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("opcua server did not become ready");

    let mut client = ClientBuilder::new()
        .application_name("simbus-test")
        .application_uri("urn:simbus:test")
        .create_sample_keypair(false)
        .trust_server_certs(true)
        .session_retry_limit(1)
        .pki_dir("/dev/null")
        .client()
        .unwrap();

    let url = format!("opc.tcp://127.0.0.1:{port}/");
    let endpoint: EndpointDescription = (
        url.as_str(),
        "None",
        MessageSecurityMode::None,
        UserTokenPolicy::anonymous(),
    )
        .into();
    let (session, event_loop) = client
        .connect_to_endpoint_directly(endpoint, IdentityToken::Anonymous)
        .unwrap();
    let handle = event_loop.spawn();
    assert!(
        tokio::time::timeout(Duration::from_secs(10), session.wait_for_connection())
            .await
            .expect("connect timeout")
    );

    let ns = find_simbus_namespace(&session).await;
    // Language 2 addresses points by id (`ns=N;s={id}`), not `holding/{name}`.
    let node = NodeId::new(ns, "temperature");
    let values = session
        .read(&[node.into()], TimestampsToReturn::Neither, 0.0)
        .await
        .unwrap();
    let value = values[0]
        .value
        .clone()
        .unwrap_or_else(|| panic!("temperature value missing: {:?}", values[0].status));
    let temp = match value {
        Variant::Float(n) => f64::from(n),
        Variant::Double(n) => n,
        other => panic!("expected float, got {other:?}"),
    };
    assert!((temp - 22.5).abs() < 0.05, "expected ~22.5, got {temp}");

    let _ = session.disconnect().await;
    handle.abort();
    let _ = stop_tx.send(());
    let _ = tokio::time::timeout(Duration::from_secs(5), server).await;
}

async fn find_simbus_namespace(session: &ua::client::Session) -> u16 {
    let ns_array = NodeId::new(0, 2255);
    let values = session
        .read(&[ns_array.into()], TimestampsToReturn::Neither, 0.0)
        .await
        .unwrap();
    let Some(Variant::Array(array)) = values[0].value.clone() else {
        panic!("namespace array missing: {:?}", values[0].value);
    };
    for (idx, v) in array.values.iter().enumerate() {
        if let Variant::String(s) = v {
            if s.as_ref() == "urn:simbus:tnh_sensor" {
                return u16::try_from(idx).unwrap();
            }
        }
    }
    panic!(
        "urn:simbus:tnh_sensor not in namespace array: {:?}",
        array.values
    );
}
