use spec::{device_report, load_device_from_path, load_device_from_str};

fn tnh_yaml() -> &'static str {
    r"
name: Generic T&H Sensor
version: '1.0'
type: tnh_sensor
modbus:
  default_port: 502
  unit_id: 1
  endianness: big
registers:
  holding:
    - address: 0
      name: temperature
      default: 22.5
      scale: 10
      data_type: uint16
      simulation:
        behavior: gaussian_noise
        std_dev: 0.3
    - address: 1
      name: humidity
      default: 45.0
      scale: 10
      data_type: uint16
      simulation:
        behavior: sinusoidal
        period_hours: 12
        amplitude: 5.0
  coils:
    - address: 0
      name: high_temp_alarm
      default: false
      trigger:
        source_register: temperature
        condition: gt
        threshold: 30.0
alarms:
  - name: High temperature
    severity: critical
    trigger: high_temp_alarm
"
}

#[test]
fn check_report_summarizes_configuration() {
    let spec = load_device_from_str(tnh_yaml()).unwrap();
    let report = device_report("example.yaml", &spec);
    assert!(report.starts_with("OK  example.yaml"));
    assert!(report.contains("type         tnh_sensor"));
    assert!(report.contains("modbus       port 502, unit 1, endianness big"));
    assert!(report.contains("inferred"));
    assert!(report.contains("holding 2"));
    assert!(report.contains("temperature"));
    assert!(report.contains("gaussian_noise"));
    assert!(report.contains("high_temp_alarm"));
    assert!(report.contains("High temperature"));
}

#[test]
fn infers_modbus_tcp_binding() {
    let spec = load_device_from_str(tnh_yaml()).unwrap();
    assert!(spec.bindings.is_empty());
    assert_eq!(spec.resolved_bindings().len(), 1);
}

#[test]
fn rejects_unknown_trigger() {
    let yaml = r"
name: bad
version: '1.0'
type: bad
modbus:
  default_port: 502
  unit_id: 1
registers:
  coils:
    - address: 0
      name: alarm
      trigger:
        source_register: missing
        condition: gt
        threshold: 1.0
";
    let err = spec::load_device_from_str(yaml).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("unknown register"), "{msg}");
}

#[test]
fn rejects_overlap_for_float32() {
    let yaml = r"
name: overlap
version: '1.0'
type: x
modbus:
  default_port: 502
registers:
  holding:
    - address: 0
      name: a
      default: 1.0
      data_type: float32
    - address: 1
      name: b
      default: 2.0
      data_type: uint16
";
    let err = spec::load_device_from_str(yaml).unwrap_err();
    assert!(err.to_string().contains("overlaps"), "{err}");
}

#[test]
fn rejects_empty_scenario() {
    let err = spec::load_scenario_from_str(
        r"
name: empty
description: none
steps: []
",
    )
    .unwrap_err();
    assert!(err.to_string().contains("at least one step"), "{err}");
}

#[test]
fn defaults_spec_version_to_one() {
    let spec = load_device_from_str(tnh_yaml()).unwrap();
    assert_eq!(spec.spec_version, spec::SPEC_VERSION_MIN);
}

#[test]
fn rejects_unknown_spec_version() {
    let yaml = r"
name: future
spec_version: 99
version: '1.0'
type: x
modbus:
  default_port: 502
registers:
  holding:
    - address: 0
      name: a
      default: 1.0
";
    let err = spec::load_device_from_str(yaml).unwrap_err();
    assert!(err.to_string().contains("spec_version"), "{err}");
}

#[test]
fn embedded_scenario_must_reference_real_registers() {
    let yaml = r"
name: tnh
version: '1.0'
type: tnh_sensor
modbus:
  default_port: 502
registers:
  holding:
    - address: 0
      name: temperature
      default: 22.5
scenarios:
  - id: heat-wave
    name: Heat Wave
    steps:
      - at: 0
        action: set_register
        register_name: missing
        value: 30.0
";
    let err = spec::load_device_from_str(yaml).unwrap_err();
    assert!(err.to_string().contains("set_register"), "{err}");
}

#[test]
fn embedded_scenario_is_part_of_the_contract() {
    let yaml = r"
name: tnh
version: '1.0'
type: tnh_sensor
modbus:
  default_port: 502
registers:
  holding:
    - address: 0
      name: temperature
      default: 22.5
scenarios:
  - id: heat-wave
    name: Heat Wave
    description: Rise then spike.
    steps:
      - at: 0
        action: set_register
        register_name: temperature
        value: 22.0
      - at: 2
        action: inject_fault
        fault_type: spike
        register_name: temperature
        value: 42.0
        duration_s: 10
";
    let spec = load_device_from_str(yaml).unwrap();
    assert_eq!(spec.scenarios.len(), 1);
    assert_eq!(spec.scenarios[0].id, "heat-wave");
    let report = device_report("tnh.yaml", &spec);
    assert!(report.contains("spec_version 1"));
    assert!(report.contains("heat-wave"));
}

#[test]
fn snmp_binding_is_valid_syntax_but_unimplemented() {
    let yaml = r"
name: snmp-box
version: '1.0'
type: meter
modbus:
  default_port: 502
bindings:
  - protocol: snmp-v2c
    port: 161
    community: public
registers:
  holding:
    - address: 0
      name: watts
      default: 1.0
";
    let spec = load_device_from_str(yaml).unwrap();
    let unimplemented = spec.unimplemented_protocols();
    assert_eq!(unimplemented.len(), 1);
    assert!(!unimplemented[0].is_implemented());
    let report = device_report("snmp.yaml", &spec);
    assert!(report.contains("specified, not implemented"));
}

#[test]
fn modbus_tls_is_implemented_with_default_port() {
    let yaml = r"
name: tls-box
version: '1.0'
type: meter
modbus:
  default_port: 502
bindings:
  - protocol: modbus-tls
    certfile: cert.pem
    keyfile: key.pem
registers:
  holding:
    - address: 0
      name: watts
      default: 1.0
";
    let spec = load_device_from_str(yaml).unwrap();
    assert!(spec.unimplemented_protocols().is_empty());
    match &spec.resolved_bindings()[0] {
        spec::BindingSpec::ModbusTls {
            port,
            certfile,
            keyfile,
            cafile,
            ..
        } => {
            assert_eq!(*port, 802);
            assert_eq!(certfile, "cert.pem");
            assert_eq!(keyfile, "key.pem");
            assert!(cafile.is_none());
        }
        other => panic!("expected modbus-tls, got {other:?}"),
    }
    let report = device_report("tls.yaml", &spec);
    assert!(!report.contains("specified, not implemented"));
    assert!(report.contains("modbus-tls :802"));
}

#[test]
fn modbus_tls_parses_cafile() {
    let yaml = r"
name: mtls-box
version: '1.0'
type: meter
modbus:
  default_port: 502
bindings:
  - protocol: modbus-tcp
  - protocol: modbus-tls
    port: 8802
    certfile: /tmp/cert.pem
    keyfile: /tmp/key.pem
    cafile: /tmp/ca.pem
registers:
  holding:
    - address: 0
      name: watts
      default: 1.0
";
    let spec = load_device_from_str(yaml).unwrap();
    assert!(spec.unimplemented_protocols().is_empty());
    let report = device_report("mtls.yaml", &spec);
    assert!(report.contains("modbus-tls :8802 mTLS"));
}

#[test]
fn opcua_is_implemented_with_default_port() {
    let yaml = r"
name: ua-box
version: '1.0'
type: meter
modbus:
  default_port: 502
bindings:
  - protocol: opcua
registers:
  holding:
    - address: 0
      name: watts
      default: 1.0
";
    let spec = load_device_from_str(yaml).unwrap();
    assert!(spec.unimplemented_protocols().is_empty());
    match &spec.resolved_bindings()[0] {
        spec::BindingSpec::Opcua { port, .. } => assert_eq!(*port, 4840),
        other => panic!("expected opcua, got {other:?}"),
    }
    let report = device_report("opcua.yaml", &spec);
    assert!(!report.contains("specified, not implemented"));
    assert!(report.contains("opcua :4840"));
}

fn holding_sim(body: &str) -> String {
    format!(
        r"
name: x
version: '1.0'
type: x
modbus:
  default_port: 502
registers:
  holding:
    - address: 0
      name: a
      default: 1.0
      simulation:
{body}
"
    )
}

#[test]
fn accepts_square_triangle_uniform_cycle() {
    let yaml = holding_sim(
        r"        behavior: square
        period_seconds: 10
        amplitude: 5.0",
    );
    let spec = load_device_from_str(&yaml).unwrap();
    assert_eq!(
        spec.registers.holding[0]
            .simulation
            .as_ref()
            .unwrap()
            .kind_name(),
        "square"
    );

    let yaml = holding_sim(
        r"        behavior: triangle
        period_seconds: 8
        min: 0.0
        max: 10.0",
    );
    assert_eq!(
        load_device_from_str(&yaml).unwrap().registers.holding[0]
            .simulation
            .as_ref()
            .unwrap()
            .kind_name(),
        "triangle"
    );

    let yaml = holding_sim(
        r"        behavior: uniform
        min: 10.0
        max: 90.0",
    );
    assert_eq!(
        load_device_from_str(&yaml).unwrap().registers.holding[0]
            .simulation
            .as_ref()
            .unwrap()
            .kind_name(),
        "uniform"
    );

    let yaml = holding_sim(
        r"        behavior: cycle
        dwell_seconds: 5
        values: [0.0, 25.0, 50.0]",
    );
    let spec = load_device_from_str(&yaml).unwrap();
    assert_eq!(
        spec.registers.holding[0]
            .simulation
            .as_ref()
            .unwrap()
            .kind_name(),
        "cycle"
    );
    let report = device_report("cycle.yaml", &spec);
    assert!(report.contains("cycle"));
}

#[test]
fn rejects_invalid_square_triangle_uniform_cycle() {
    let err = load_device_from_str(&holding_sim(
        r"        behavior: square
        period_seconds: 0
        amplitude: 5.0",
    ))
    .unwrap_err();
    assert!(err.to_string().contains("period_seconds"), "{err}");

    let err = load_device_from_str(&holding_sim(
        r"        behavior: triangle
        period_seconds: 8
        min: 10.0
        max: 10.0",
    ))
    .unwrap_err();
    assert!(err.to_string().contains("triangle min"), "{err}");

    let err = load_device_from_str(&holding_sim(
        r"        behavior: uniform
        min: 90.0
        max: 10.0",
    ))
    .unwrap_err();
    assert!(err.to_string().contains("uniform min"), "{err}");

    let err = load_device_from_str(&holding_sim(
        r"        behavior: cycle
        dwell_seconds: 1
        values: []",
    ))
    .unwrap_err();
    assert!(err.to_string().contains("at least one value"), "{err}");
}

#[test]
fn language_two_points_and_explicit_export() {
    let yaml = r"
name: Example
spec_version: 2
version: '1.0'
type: example
points:
  - id: analog_a
    kind: analog
    class: value
    unit: units
    default: 50.0
    simulation:
      behavior: constant
  - id: analog_a_high
    kind: binary
    class: input
    default: false
    trigger:
      source: analog_a
      condition: gt
      threshold: 80.0
bindings:
  - protocol: modbus-tcp
    port: 502
    unit_id: 1
    endianness: big
    export:
      analog_a:
        space: holding
        address: 0
        scale: 10
        data_type: uint16
      analog_a_high:
        space: coil
        address: 0
scenarios:
  - id: demo-spike
    name: Demo
    steps:
      - at: 0
        action: set_point
        point: analog_a
        value: 95.0
";
    let spec = load_device_from_str(yaml).unwrap();
    assert_eq!(spec.spec_version, 2);
    assert_eq!(spec.points.len(), 2);
    assert_eq!(spec.registers.holding[0].name, "analog_a");
    assert_eq!(spec.registers.coils[0].name, "analog_a_high");
    assert!(spec.bindings[0].protocol_id() == spec::ProtocolId::ModbusTcp);
}

#[test]
fn language_two_requires_modbus_export() {
    let yaml = r"
name: Example
spec_version: 2
version: '1.0'
type: example
points:
  - id: analog_a
    kind: analog
    class: value
    default: 1.0
bindings:
  - protocol: modbus-tcp
    port: 502
";
    let err = load_device_from_str(yaml).unwrap_err();
    assert!(err.to_string().contains("export"), "{err}");
}

#[test]
fn language_two_rejects_mixed_registers() {
    let yaml = r"
name: Example
spec_version: 2
version: '1.0'
type: example
points:
  - id: analog_a
    kind: analog
    class: value
    default: 1.0
registers:
  holding:
    - address: 0
      name: analog_a
      default: 1.0
bindings:
  - protocol: modbus-tcp
    port: 502
    export:
      analog_a:
        space: holding
        address: 0
";
    let err = load_device_from_str(yaml).unwrap_err();
    assert!(err.to_string().contains("mix"), "{err}");
}

#[test]
fn builtin_default_is_language_two() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../devices/builtin/default.yaml");
    let spec = load_device_from_path(path).unwrap();
    assert_eq!(spec.spec_version, 2);
    assert_eq!(spec.points.len(), 6);
    assert_eq!(spec.ua_naming, spec::UaNaming::PointId);
    assert_eq!(spec.registers.holding.len(), 4);
}
