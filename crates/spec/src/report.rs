//! Human-readable preview of a validated device spec.

use crate::types::{BindingSpec, CoilSpec, DeviceSpec, RegisterSpec};

/// Text report produced by `simbus check`.
#[must_use]
pub fn device_report(path: &str, spec: &DeviceSpec) -> String {
    let mut out = String::new();
    out.push_str(&format!("OK  {path}\n\n"));
    out.push_str(&format!("  name         {}\n", spec.name));
    out.push_str(&format!("  type         {}\n", spec.device_type));
    out.push_str(&format!("  version      {}\n", spec.version));
    out.push_str(&format!("  spec_version {}\n", spec.spec_version));
    out.push_str(&format!(
        "  modbus       port {}, unit {}, endianness {}\n",
        spec.modbus.default_port,
        spec.modbus.unit_id,
        spec.modbus.endianness.as_str()
    ));
    let inferred = spec.bindings.is_empty();
    let bindings = spec.resolved_bindings();
    let bind_note = if inferred { " (inferred)" } else { "" };
    out.push_str(&format!(
        "  bindings     {}{bind_note}\n",
        bindings
            .iter()
            .map(describe_binding)
            .collect::<Vec<_>>()
            .join(", ")
    ));
    let unimplemented = spec.unimplemented_protocols();
    if !unimplemented.is_empty() {
        out.push_str("  unimplemented ");
        out.push_str(
            &unimplemented
                .iter()
                .map(|p| format!("{} (specified, not implemented)", p.as_str()))
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push('\n');
    }
    out.push('\n');
    out.push_str(&format!(
        "  counts       holding {}  input {}  coils {}  discrete {}\n",
        spec.registers.holding.len(),
        spec.registers.input.len(),
        spec.registers.coils.len(),
        spec.registers.discrete.len()
    ));
    append_regs(&mut out, "holding", &spec.registers.holding);
    append_regs(&mut out, "input", &spec.registers.input);
    append_coils(&mut out, "coils", &spec.registers.coils);
    append_coils(&mut out, "discrete", &spec.registers.discrete);
    if !spec.alarms.is_empty() {
        out.push_str("\n  alarms\n");
        for alarm in &spec.alarms {
            out.push_str(&format!(
                "    {:<22} {:<10} <- {}\n",
                alarm.name,
                alarm.severity.as_str(),
                alarm.trigger
            ));
        }
    }
    if !spec.scenarios.is_empty() {
        out.push_str("\n  scenarios\n");
        for scenario in &spec.scenarios {
            out.push_str(&format!(
                "    {:<22} {:>3} steps  {}\n",
                scenario.id,
                scenario.steps.len(),
                scenario.name
            ));
        }
    }
    out
}

fn describe_binding(binding: &BindingSpec) -> String {
    match binding {
        BindingSpec::ModbusTcp { port, unit_id } => {
            let port = port.map_or_else(|| "default".to_owned(), |p| p.to_string());
            let unit = unit_id.unwrap_or(1);
            format!("modbus-tcp :{port} unit {unit}")
        }
        BindingSpec::ModbusRtu { device, baudrate } => {
            format!("modbus-rtu {device} @{baudrate}")
        }
        BindingSpec::ModbusTls { port, .. } => format!("modbus-tls :{port}"),
        BindingSpec::SnmpV2c {
            port, community, ..
        } => {
            format!("snmp-v2c :{port} community {community}")
        }
        BindingSpec::Opcua { port } => format!("opcua :{port}"),
        BindingSpec::MqttSparkplug {
            broker,
            group_id,
            edge_node_id,
        } => format!("sparkplug {broker} {group_id}/{edge_node_id}"),
        BindingSpec::BacnetIp {
            port,
            device_instance,
        } => format!("bacnet-ip :{port} instance {device_instance}"),
    }
}

fn append_regs(out: &mut String, title: &str, regs: &[RegisterSpec]) {
    if regs.is_empty() {
        return;
    }
    out.push_str(&format!("\n  {title}\n"));
    for reg in regs {
        let behavior = reg
            .simulation
            .as_ref()
            .map_or("-", crate::BehaviorSpec::kind_name);
        let unit = if reg.unit.is_empty() {
            String::new()
        } else {
            format!(" {}", reg.unit)
        };
        out.push_str(&format!(
            "    {:>5}  {:<24} {:<8} {:<16} default {}{unit}  scale {}\n",
            reg.address,
            reg.name,
            reg.data_type.as_str(),
            behavior,
            reg.default,
            reg.scale
        ));
    }
}

fn append_coils(out: &mut String, title: &str, coils: &[CoilSpec]) {
    if coils.is_empty() {
        return;
    }
    out.push_str(&format!("\n  {title}\n"));
    for coil in coils {
        let trigger = coil.trigger.as_ref().map_or_else(
            || "static".to_owned(),
            |t| {
                format!(
                    "{} {} {}",
                    t.source_register,
                    t.condition.as_str(),
                    t.threshold
                )
            },
        );
        out.push_str(&format!(
            "    {:>5}  {:<24} default {:<5}  {trigger}\n",
            coil.address, coil.name, coil.default
        ));
    }
}
