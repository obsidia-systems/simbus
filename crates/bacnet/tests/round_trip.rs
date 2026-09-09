//! BACnet/IP round trip against a language-2 fixture.
//!
//! Boots `bacnet::serve` on an ephemeral UDP port, discovers the device with a
//! directed Who-Is, reads an Analog Input Present_Value, then writes an Analog
//! Value and checks the engine moved (`docs/bacnet.md` §1).

use std::net::{Ipv4Addr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use bacnet_client::client::BACnetClient;
use bacnet_types::enums::{ObjectType, PropertyIdentifier};
use bacnet_types::primitives::ObjectIdentifier;
use engine::Device;
use spec::load_device_from_str;

const YAML: &str = r"
name: BACnet Fixture
spec_version: 2
version: '1.0'
type: bacnet_fixture
identity:
  vendor: Obsidia Systems
  product: simbus
  revision: '0.3.0'
points:
  - id: temperature
    kind: analog
    class: input
    unit: degC
    default: 22.5
  - id: setpoint
    kind: analog
    class: value
    unit: degC
    default: 21.0
bindings:
  - protocol: bacnet-ip
    device_instance: 1001
    export:
      temperature:
        object: analog-input
        instance: 1
      setpoint:
        object: analog-value
        instance: 1
";

fn ephemeral_udp_port() -> u16 {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    socket.local_addr().unwrap().port()
}

fn bip_mac(port: u16) -> Vec<u8> {
    let [hi, lo] = port.to_be_bytes();
    vec![127, 0, 0, 1, hi, lo]
}

/// Application-tagged REAL (Clause 20.2.6).
fn app_real(value: f32) -> Vec<u8> {
    let mut out = vec![0x44];
    out.extend_from_slice(&value.to_be_bytes());
    out
}

fn decode_app_real(bytes: &[u8]) -> f32 {
    assert_eq!(
        bytes.first().copied(),
        Some(0x44),
        "expected app-tagged REAL"
    );
    f32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]])
}

#[tokio::test]
async fn read_property_and_write_property_round_trip() {
    let spec = load_device_from_str(YAML).unwrap();
    let device = Device::new(spec, Some(1), 1.0);
    device.set_running(true);

    let port = ephemeral_udp_port();
    let ready = Arc::new(AtomicBool::new(false));
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let mut server = tokio::spawn({
        let device = device.clone();
        let ready = ready.clone();
        async move {
            let stop = async move {
                let _ = stop_rx.await;
            };
            bacnet::serve_on(
                device,
                Ipv4Addr::LOCALHOST,
                port,
                Ipv4Addr::LOCALHOST,
                1001,
                ready,
                stop,
            )
            .await
        }
    });

    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if ready.load(Ordering::SeqCst) {
                return;
            }
            tokio::select! {
                res = &mut server => panic!("bacnet server ended early: {res:?}"),
                () = tokio::time::sleep(Duration::from_millis(20)) => {}
            }
        }
    })
    .await
    .expect("bacnet server did not become ready");

    let mut client = BACnetClient::bip_builder()
        .interface(Ipv4Addr::LOCALHOST)
        .port(0)
        .broadcast_address(Ipv4Addr::LOCALHOST)
        .apdu_timeout_ms(2000)
        .build()
        .await
        .unwrap();

    let mac = bip_mac(port);

    // Who-Is is served. Its I-Am answer is a BACnet/IP broadcast to
    // (broadcast address, this device's own UDP port), so on a single-socket
    // loopback test the reply cannot land on the client's ephemeral port: this
    // asserts the dispatch task keeps serving, not the I-Am payload.
    client.who_is_directed(&mac, None, None).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // ReadProperty on the Analog Input: engineering REAL, not a Modbus raw word.
    let analog_input = ObjectIdentifier::new(ObjectType::ANALOG_INPUT, 1).unwrap();
    let ack = client
        .read_property(&mac, analog_input, PropertyIdentifier::PRESENT_VALUE, None)
        .await
        .unwrap();
    assert!((decode_app_real(&ack.property_value) - 22.5).abs() < 0.001);

    // WriteProperty on the Analog Value reaches `state.base`.
    let analog_value = ObjectIdentifier::new(ObjectType::ANALOG_VALUE, 1).unwrap();
    client
        .write_property(
            &mac,
            analog_value,
            PropertyIdentifier::PRESENT_VALUE,
            None,
            app_real(25.0),
            None,
        )
        .await
        .unwrap();

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let value = device.point_view("setpoint").unwrap().analog.unwrap();
            if (value - 25.0).abs() < 0.01 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("engine setpoint did not follow the BACnet write");

    // The write is visible on the wire too.
    let ack = client
        .read_property(&mac, analog_value, PropertyIdentifier::PRESENT_VALUE, None)
        .await
        .unwrap();
    assert!((decode_app_real(&ack.property_value) - 25.0).abs() < 0.001);

    client.stop().await.unwrap();
    let _ = stop_tx.send(());
    server.await.unwrap().unwrap();
}
