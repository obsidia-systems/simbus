//! Language 2 device documents (`points:` + explicit `export`).

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::types::{
    AlarmSpec, BindingSpec, CoilSpec, DeviceSpec, IdentitySpec, ModbusExportEntry,
    ModbusExportSpace, ModbusSpec, PointClass, PointKind, PointSpec, RegisterMapSpec, RegisterSpec,
    TriggerSpec, UaNaming, lift_v1,
};
use crate::{BehaviorSpec, ScenarioSpec, SpecError};

#[derive(Debug, Deserialize)]
struct DeviceYamlV2 {
    name: String,
    spec_version: u32,
    version: String,
    #[serde(rename = "type")]
    device_type: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    identity: IdentitySpec,
    points: Vec<PointYaml>,
    #[serde(default)]
    bindings: Vec<BindingSpec>,
    #[serde(default)]
    alarms: Vec<AlarmSpec>,
    #[serde(default)]
    scenarios: Vec<ScenarioSpec>,
}

#[derive(Debug, Deserialize)]
struct PointYaml {
    id: String,
    kind: PointKind,
    class: PointClass,
    #[serde(default)]
    description: String,
    #[serde(default)]
    unit: String,
    default: serde_yaml::Value,
    #[serde(default)]
    simulation: Option<BehaviorSpec>,
    #[serde(default)]
    trigger: Option<TriggerSpec>,
}

pub(crate) fn load_v2(value: serde_yaml::Value) -> Result<DeviceSpec, SpecError> {
    let yaml: DeviceYamlV2 = serde_yaml::from_value(value)?;
    let mut points = Vec::with_capacity(yaml.points.len());
    for row in yaml.points {
        points.push(point_from_yaml(row)?);
    }
    let mut spec = DeviceSpec {
        name: yaml.name,
        spec_version: yaml.spec_version,
        version: yaml.version,
        device_type: yaml.device_type,
        description: yaml.description,
        identity: yaml.identity,
        modbus: modbus_from_bindings(&yaml.bindings),
        registers: RegisterMapSpec::default(),
        alarms: yaml.alarms,
        bindings: yaml.bindings,
        scenarios: yaml.scenarios,
        points,
        ua_naming: UaNaming::PointId,
    };
    spec.registers = materialize_registers(&spec)?;
    Ok(spec)
}

fn point_from_yaml(row: PointYaml) -> Result<PointSpec, SpecError> {
    let mut analog_default = 0.0;
    let mut binary_default = false;
    match row.kind {
        PointKind::Analog => {
            analog_default = yaml_f64(&row.default).ok_or_else(|| {
                SpecError::Validation(format!(
                    "point '{}': analog default must be a number",
                    row.id
                ))
            })?;
        }
        PointKind::Binary => {
            binary_default = yaml_bool(&row.default).ok_or_else(|| {
                SpecError::Validation(format!(
                    "point '{}': binary default must be a boolean",
                    row.id
                ))
            })?;
        }
    }
    Ok(PointSpec {
        id: row.id,
        kind: row.kind,
        class: row.class,
        description: row.description,
        unit: row.unit,
        analog_default,
        binary_default,
        simulation: row.simulation,
        trigger: row.trigger,
    })
}

fn yaml_f64(value: &serde_yaml::Value) -> Option<f64> {
    match value {
        serde_yaml::Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

fn yaml_bool(value: &serde_yaml::Value) -> Option<bool> {
    value.as_bool()
}

fn modbus_from_bindings(bindings: &[BindingSpec]) -> ModbusSpec {
    for binding in bindings {
        match binding {
            BindingSpec::ModbusTcp {
                port,
                unit_id,
                endianness,
                ..
            } => {
                return ModbusSpec {
                    default_port: port.unwrap_or(502),
                    unit_id: unit_id.unwrap_or(1),
                    endianness: *endianness,
                };
            }
            BindingSpec::ModbusTls { port, .. } => {
                return ModbusSpec {
                    default_port: *port,
                    unit_id: 1,
                    endianness: crate::Endianness::Big,
                };
            }
            _ => {}
        }
    }
    ModbusSpec::default()
}

fn materialize_registers(spec: &DeviceSpec) -> Result<RegisterMapSpec, SpecError> {
    let mut map = RegisterMapSpec::default();
    let Some(export) = first_modbus_export(&spec.bindings) else {
        // No Modbus plane at all (for example an `opcua`-only or `bacnet-ip`-only
        // document): the engine bank is still address-backed in this version, so
        // give every point a private cell. Nothing is published on Modbus because
        // no Modbus listener is started.
        if !has_modbus_binding(&spec.bindings) {
            back_all_points(spec, &mut map);
        }
        return Ok(map);
    };
    for (id, entry) in export {
        let Some(point) = spec.point(id) else {
            continue;
        };
        match entry.space {
            ModbusExportSpace::Holding | ModbusExportSpace::Input => {
                let reg = RegisterSpec {
                    address: entry.address,
                    name: point.id.clone(),
                    description: point.description.clone(),
                    unit: point.unit.clone(),
                    default: point.analog_default,
                    scale: entry.scale,
                    data_type: entry.data_type,
                    simulation: point.simulation.clone(),
                };
                if entry.space == ModbusExportSpace::Holding {
                    map.holding.push(reg);
                } else {
                    map.input.push(reg);
                }
            }
            ModbusExportSpace::Coil | ModbusExportSpace::Discrete => {
                let coil = CoilSpec {
                    address: entry.address,
                    name: point.id.clone(),
                    description: point.description.clone(),
                    default: point.binary_default,
                    trigger: point.trigger.clone(),
                };
                if entry.space == ModbusExportSpace::Coil {
                    map.coils.push(coil);
                } else {
                    map.discrete.push(coil);
                }
            }
        }
    }
    Ok(map)
}

fn has_modbus_binding(bindings: &[BindingSpec]) -> bool {
    bindings.iter().any(|binding| {
        matches!(
            binding,
            BindingSpec::ModbusTcp { .. } | BindingSpec::ModbusTls { .. }
        )
    })
}

/// Give each point a backing cell when the document has no Modbus plane.
///
/// Analog points use `float32` with `scale` 1 so the cell holds the
/// engineering value (BACnet REAL, OPC UA Float) without raw quantization.
fn back_all_points(spec: &DeviceSpec, map: &mut RegisterMapSpec) {
    let mut word = 0u16;
    let mut bit = 0u16;
    for point in &spec.points {
        match point.kind {
            PointKind::Analog => {
                map.holding.push(RegisterSpec {
                    address: word,
                    name: point.id.clone(),
                    description: point.description.clone(),
                    unit: point.unit.clone(),
                    default: point.analog_default,
                    scale: 1,
                    data_type: crate::DataType::Float32,
                    simulation: point.simulation.clone(),
                });
                word = word.saturating_add(2);
            }
            PointKind::Binary => {
                map.coils.push(CoilSpec {
                    address: bit,
                    name: point.id.clone(),
                    description: point.description.clone(),
                    default: point.binary_default,
                    trigger: point.trigger.clone(),
                });
                bit = bit.saturating_add(1);
            }
        }
    }
}

fn first_modbus_export(bindings: &[BindingSpec]) -> Option<&BTreeMap<String, ModbusExportEntry>> {
    for binding in bindings {
        match binding {
            BindingSpec::ModbusTcp { export, .. } | BindingSpec::ModbusTls { export, .. }
                if !export.is_empty() =>
            {
                return Some(export);
            }
            _ => {}
        }
    }
    None
}

/// Apply language-1 lift after serde.
pub(crate) fn finish_v1(mut spec: DeviceSpec) -> DeviceSpec {
    lift_v1(&mut spec);
    spec
}
