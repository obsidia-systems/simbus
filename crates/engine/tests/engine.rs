use engine::Device;
use spec::{FaultType, RegisterSpace, load_device_from_path};

fn tnh_spec() -> spec::DeviceSpec {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../devices/builtin/generic-tnh-sensor.yaml");
    load_device_from_path(path).unwrap()
}

#[test]
fn initializes_defaults() {
    let device = Device::new(tnh_spec(), Some(1), 1.0);
    let snap = device.snapshot();
    assert_eq!(snap.holding.get(&0), Some(&225));
    assert_eq!(snap.holding.get(&1), Some(&450));
    assert_eq!(snap.coils.get(&0), Some(&false));
}

#[test]
fn spike_fault_overrides_temperature() {
    let device = Device::new(tnh_spec(), Some(1), 1.0);
    device.inject_fault(engine::ActiveFault::new(
        FaultType::Spike,
        Some("temperature".into()),
        Some(42.0),
        10.0,
    ));
    let snap = device.tick(1.0);
    assert_eq!(snap.holding.get(&0), Some(&420));
}

#[test]
fn high_temp_alarm_fires() {
    let device = Device::new(tnh_spec(), Some(1), 1.0);
    device
        .override_register(RegisterSpace::Holding, 0, None, Some(31.0), "test")
        .unwrap();
    let snap = device.tick(1.0);
    assert_eq!(snap.coils.get(&0), Some(&true));
}

#[test]
fn reset_restores_defaults() {
    let device = Device::new(tnh_spec(), Some(1), 1.0);
    device
        .override_register(RegisterSpace::Holding, 0, None, Some(40.0), "test")
        .unwrap();
    device.reset();
    let snap = device.snapshot();
    assert_eq!(snap.holding.get(&0), Some(&225));
}

#[test]
fn faults_apply_to_input_registers() {
    let yaml = r"
name: in
version: '1.0'
type: x
modbus:
  default_port: 502
registers:
  input:
    - address: 0
      name: temperature
      default: 18.0
      scale: 10
      data_type: uint16
      simulation:
        behavior: constant
";
    let spec = spec::load_device_from_str(yaml).unwrap();
    let device = Device::new(spec, Some(1), 1.0);
    device.inject_fault(engine::ActiveFault::new(
        FaultType::Spike,
        Some("temperature".into()),
        Some(40.0),
        5.0,
    ));
    let snap = device.tick(1.0);
    assert_eq!(snap.input.get(&0), Some(&400));
}

#[test]
fn same_seed_replays_the_same_trace() {
    let a = Device::new(tnh_spec(), Some(7), 1.0);
    let b = Device::new(tnh_spec(), Some(7), 1.0);
    for _ in 0..16 {
        assert_eq!(a.tick(1.0).holding, b.tick(1.0).holding);
        assert_eq!(a.snapshot().holding.get(&1), b.snapshot().holding.get(&1));
    }
}

#[test]
fn unseeded_instances_do_not_share_a_trace() {
    let a = Device::new(tnh_spec(), None, 1.0);
    let b = Device::new(tnh_spec(), None, 1.0);
    let mut temps_a = Vec::new();
    let mut temps_b = Vec::new();
    let mut humidity_a = Vec::new();
    let mut humidity_b = Vec::new();
    for _ in 0..16 {
        let sa = a.tick(1.0);
        let sb = b.tick(1.0);
        temps_a.push(sa.holding.get(&0).copied());
        temps_b.push(sb.holding.get(&0).copied());
        humidity_a.push(sa.holding.get(&1).copied());
        humidity_b.push(sb.holding.get(&1).copied());
    }
    assert_ne!(
        temps_a, temps_b,
        "unseeded temperature noise must not clone across processes"
    );
    assert_ne!(
        humidity_a, humidity_b,
        "sinusoidal humidity must not share a t=0 phase across processes"
    );
}

#[test]
fn same_numeric_seed_diverges_across_device_types() {
    let tnh = Device::new(tnh_spec(), Some(1), 1.0);
    let ups_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../devices/builtin/generic-ups.yaml");
    let ups = Device::new(load_device_from_path(ups_path).unwrap(), Some(1), 1.0);
    tnh.tick(1.0);
    ups.tick(1.0);
    assert_ne!(tnh.snapshot().holding, ups.snapshot().holding);
}

fn drift_spec() -> spec::DeviceSpec {
    spec::load_device_from_str(
        r"
name: drift
version: '1.0'
type: x
modbus:
  default_port: 502
registers:
  holding:
    - address: 0
      name: soc
      default: 50.0
      scale: 10
      data_type: uint16
      simulation:
        behavior: drift
        rate: -1.0
        bounds: [0.0, 100.0]
",
    )
    .unwrap()
}

#[test]
fn drift_is_scaled_by_dt() {
    let a = Device::new(drift_spec(), Some(3), 1.0);
    let b = Device::new(drift_spec(), Some(3), 1.0);
    let once = a.tick(1.0);
    b.tick(0.5);
    let twice = b.tick(0.5);
    assert_eq!(once.holding.get(&0), twice.holding.get(&0));
}

#[test]
fn zero_dt_is_noop() {
    let device = Device::new(tnh_spec(), Some(1), 1.0);
    device.inject_fault(engine::ActiveFault::new(
        FaultType::Spike,
        Some("temperature".into()),
        Some(42.0),
        1.0,
    ));
    let before = device.snapshot();
    let after = device.tick(0.0);
    assert_eq!(before.holding, after.holding);
    assert_eq!(device.faults().len(), 1);
    assert!((device.faults()[0].remaining_s - 1.0).abs() < f64::EPSILON);
}

#[test]
fn freeze_holds_current_cell() {
    let spec = spec::load_device_from_str(
        r"
name: freeze
version: '1.0'
type: x
modbus:
  default_port: 502
registers:
  holding:
    - address: 0
      name: temperature
      default: 18.0
      scale: 10
      data_type: uint16
      simulation:
        behavior: constant
",
    )
    .unwrap();
    let device = Device::new(spec, Some(1), 1.0);
    let live = device.tick(1.0);
    assert_eq!(live.holding.get(&0), Some(&180));
    device.inject_fault(engine::ActiveFault::new(
        FaultType::Freeze,
        Some("temperature".into()),
        None,
        10.0,
    ));
    device
        .override_register(RegisterSpace::Holding, 0, None, Some(40.0), "test")
        .unwrap();
    let frozen = device.tick(1.0);
    assert_eq!(frozen.holding.get(&0), Some(&180));
    let still = device.tick(1.0);
    assert_eq!(still.holding.get(&0), Some(&180));
}

#[test]
fn device_wide_dropout_zeros_holding_and_input() {
    let spec = spec::load_device_from_str(
        r"
name: drop
version: '1.0'
type: x
modbus:
  default_port: 502
registers:
  holding:
    - address: 0
      name: a
      default: 12.0
      scale: 10
      data_type: uint16
      simulation:
        behavior: constant
  input:
    - address: 0
      name: b
      default: 8.0
      scale: 10
      data_type: uint16
      simulation:
        behavior: constant
",
    )
    .unwrap();
    let device = Device::new(spec, Some(1), 1.0);
    device.inject_fault(engine::ActiveFault::new(
        FaultType::Dropout,
        None,
        None,
        5.0,
    ));
    let snap = device.tick(1.0);
    assert_eq!(snap.holding.get(&0), Some(&0));
    assert_eq!(snap.input.get(&0), Some(&0));
}

#[test]
fn alarm_fault_forces_coil() {
    let spec = spec::load_device_from_str(
        r"
name: alarm
version: '1.0'
type: x
modbus:
  default_port: 502
registers:
  holding:
    - address: 0
      name: temperature
      default: 20.0
      scale: 10
      data_type: uint16
      simulation:
        behavior: constant
  coils:
    - address: 0
      name: high_temp_alarm
      default: false
      trigger:
        source_register: temperature
        condition: gt
        threshold: 30.0
",
    )
    .unwrap();
    let device = Device::new(spec, Some(1), 1.0);
    device.inject_fault(engine::ActiveFault::new(
        FaultType::Alarm,
        Some("high_temp_alarm".into()),
        None,
        5.0,
    ));
    let snap = device.tick(1.0);
    assert_eq!(snap.coils.get(&0), Some(&true));
    assert_eq!(snap.holding.get(&0), Some(&200));
}

#[test]
fn eq_trigger_uses_half_lsb() {
    let spec = spec::load_device_from_str(
        r"
name: eq
version: '1.0'
type: x
modbus:
  default_port: 502
registers:
  holding:
    - address: 0
      name: setpoint
      default: 22.5
      scale: 10
      data_type: uint16
      simulation:
        behavior: constant
  coils:
    - address: 0
      name: at_setpoint
      default: false
      trigger:
        source_register: setpoint
        condition: eq
        threshold: 22.51
",
    )
    .unwrap();
    let device = Device::new(spec, Some(1), 1.0);
    let snap = device.tick(1.0);
    assert_eq!(snap.coils.get(&0), Some(&true));
}

#[test]
fn step_overwrites_patch_on_next_tick() {
    let spec = spec::load_device_from_str(
        r"
name: step
version: '1.0'
type: x
modbus:
  default_port: 502
registers:
  holding:
    - address: 0
      name: mode
      default: 0.0
      scale: 1
      data_type: uint16
      simulation:
        behavior: step
        steps:
          - at: 0
            value: 10.0
",
    )
    .unwrap();
    let device = Device::new(spec, Some(1), 1.0);
    device
        .override_register(RegisterSpace::Holding, 0, None, Some(5.0), "test")
        .unwrap();
    let snap = device.tick(1.0);
    assert_eq!(snap.holding.get(&0), Some(&10));
}

#[test]
fn reset_replays_the_boot_trace() {
    let a = Device::new(tnh_spec(), Some(11), 1.0);
    let mut first = Vec::new();
    for _ in 0..8 {
        first.push(a.tick(1.0).holding);
    }
    a.reset();
    let mut second = Vec::new();
    for _ in 0..8 {
        second.push(a.tick(1.0).holding);
    }
    assert_eq!(first, second);
}

#[test]
fn identity_is_mixed_into_seed() {
    let yaml = |vendor: &str| {
        format!(
            r"
name: same
version: '1.0'
type: sensor
identity:
  vendor: {vendor}
modbus:
  default_port: 502
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
"
        )
    };
    let a = Device::new(
        spec::load_device_from_str(&yaml("acme")).unwrap(),
        Some(1),
        1.0,
    );
    let b = Device::new(
        spec::load_device_from_str(&yaml("other")).unwrap(),
        Some(1),
        1.0,
    );
    assert_ne!(a.tick(1.0).holding.get(&0), b.tick(1.0).holding.get(&0));
}

fn two_holdings() -> spec::DeviceSpec {
    spec::load_device_from_str(
        r"
name: pair
version: '1.0'
type: x
modbus:
  default_port: 502
registers:
  holding:
    - address: 0
      name: a
      default: 1.0
      scale: 1
      data_type: uint16
      simulation:
        behavior: constant
    - address: 1
      name: b
      default: 2.0
      scale: 1
      data_type: uint16
      simulation:
        behavior: constant
",
    )
    .unwrap()
}

#[test]
fn write_words_updates_adjacent_uint16_and_bases() {
    let device = Device::new(two_holdings(), Some(1), 1.0);
    device
        .write_words(RegisterSpace::Holding, 0, &[10, 20], "test")
        .unwrap();
    let snap = device.tick(1.0);
    assert_eq!(snap.holding.get(&0), Some(&10));
    assert_eq!(snap.holding.get(&1), Some(&20));
}

#[test]
fn write_words_rejects_holes_without_partial_apply() {
    let device = Device::new(two_holdings(), Some(1), 1.0);
    let err = device
        .write_words(RegisterSpace::Holding, 0, &[10, 20, 30], "test")
        .unwrap_err();
    assert!(matches!(
        err,
        engine::DeviceError::UnknownRegister { address: 2, .. }
    ));
    let snap = device.snapshot();
    assert_eq!(snap.holding.get(&0), Some(&1));
    assert_eq!(snap.holding.get(&1), Some(&2));
}

#[test]
fn write_words_rejects_partial_float32() {
    let spec = spec::load_device_from_str(
        r"
name: f32
version: '1.0'
type: x
modbus:
  default_port: 502
registers:
  holding:
    - address: 0
      name: value
      default: 1.0
      scale: 1
      data_type: float32
      simulation:
        behavior: constant
",
    )
    .unwrap();
    let device = Device::new(spec, Some(1), 1.0);
    assert!(
        device
            .write_words(RegisterSpace::Holding, 0, &[0x3f80], "test")
            .is_err()
    );
    assert!(
        device
            .write_words(RegisterSpace::Holding, 1, &[0], "test")
            .is_err()
    );
    let words = engine::encode_words(
        engine::real_to_raw(18.5, 1, spec::DataType::Float32),
        spec::Endianness::Big,
    );
    device
        .write_words(RegisterSpace::Holding, 0, &words, "test")
        .unwrap();
    let snap = device.tick(1.0);
    assert_eq!(snap.holding.get(&0), Some(&words[0]));
    assert_eq!(snap.holding.get(&1), Some(&words[1]));
}

#[test]
fn write_coils_rejects_unmapped() {
    let device = Device::new(tnh_spec(), Some(1), 1.0);
    let err = device.write_coils(0, &[true, true, true]).unwrap_err();
    assert!(matches!(
        err,
        engine::DeviceError::UnknownCoilAddress { address: 2 }
    ));
    assert_eq!(device.snapshot().coils.get(&0), Some(&false));
}
