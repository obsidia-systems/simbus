//! Device YAML types.

use serde::{Deserialize, Serialize};

use crate::SpecError;
use crate::scenario::{ScenarioSpec, ScenarioStep};

/// Language version understood by this crate. Device YAML `spec_version` MUST equal this.
pub const SPEC_VERSION: u32 = 1;

/// Byte order for multi-register values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Endianness {
    /// ABCD (most common Modbus layout).
    #[default]
    Big,
    /// DCBA.
    Little,
    /// BADC.
    BigSwap,
    /// CDAB.
    LittleSwap,
}

impl Endianness {
    /// Canonical wire / YAML string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Big => "big",
            Self::Little => "little",
            Self::BigSwap => "big_swap",
            Self::LittleSwap => "little_swap",
        }
    }
}

/// On-wire data type for a register (or register pair).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DataType {
    /// Single 16-bit unsigned register.
    #[default]
    Uint16,
    /// Single 16-bit signed register, stored as two's complement.
    Int16,
    /// Two consecutive registers.
    Uint32,
    /// IEEE-754 float32 across two registers.
    Float32,
}

impl DataType {
    /// Canonical wire / YAML string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Uint16 => "uint16",
            Self::Int16 => "int16",
            Self::Uint32 => "uint32",
            Self::Float32 => "float32",
        }
    }

    /// Number of 16-bit Modbus words occupied by this type.
    #[must_use]
    pub const fn word_count(self) -> u8 {
        match self {
            Self::Uint16 | Self::Int16 => 1,
            Self::Uint32 | Self::Float32 => 2,
        }
    }
}

/// Holding vs input register space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegisterSpace {
    /// Holding registers (FC3 / FC6 / FC16).
    Holding,
    /// Input registers (FC4).
    Input,
}

/// Coil trigger comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerCondition {
    /// Greater than.
    Gt,
    /// Less than.
    Lt,
    /// Equal.
    Eq,
    /// Greater than or equal.
    Gte,
    /// Less than or equal.
    Lte,
}

impl TriggerCondition {
    /// Canonical YAML string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Gt => "gt",
            Self::Lt => "lt",
            Self::Eq => "eq",
            Self::Gte => "gte",
            Self::Lte => "lte",
        }
    }
}

/// Alarm severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlarmSeverity {
    /// Informational.
    Info,
    /// Warning.
    Warning,
    /// Critical.
    Critical,
}

impl AlarmSeverity {
    /// Canonical YAML string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Critical => "critical",
        }
    }
}

/// Protocol identifier used by bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProtocolId {
    /// Modbus TCP slave.
    ModbusTcp,
    /// Modbus RTU slave.
    ModbusRtu,
    /// Modbus TCP with TLS.
    ModbusTls,
    /// SNMP agent.
    SnmpV2c,
    /// OPC UA server.
    Opcua,
    /// MQTT Sparkplug B publisher.
    MqttSparkplug,
    /// BACnet/IP server.
    BacnetIp,
}

impl ProtocolId {
    /// Canonical YAML / wire string (`kebab-case`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ModbusTcp => "modbus-tcp",
            Self::ModbusRtu => "modbus-rtu",
            Self::ModbusTls => "modbus-tls",
            Self::SnmpV2c => "snmp-v2c",
            Self::Opcua => "opcua",
            Self::MqttSparkplug => "mqtt-sparkplug",
            Self::BacnetIp => "bacnet-ip",
        }
    }

    /// Whether this runtime can serve the protocol.
    ///
    /// Unimplemented ids are still valid syntax (spec-first). `simbus check`
    /// reports them; the process refuses to boot if any resolved binding is
    /// unimplemented.
    #[must_use]
    pub const fn is_implemented(self) -> bool {
        matches!(self, Self::ModbusTcp | Self::ModbusTls)
    }
}

/// Fault kinds injected via the API or scenarios.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaultType {
    /// Force a register to an extreme value.
    Spike,
    /// Hold the current register value.
    Freeze,
    /// Force the register to 0.
    Dropout,
    /// Force a named coil to true.
    Alarm,
    /// Multiply gaussian `std_dev`.
    NoiseAmplify,
}

/// Optional vendor identity (FC43 / OPC UA BuildInfo).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct IdentitySpec {
    /// Vendor name.
    #[serde(default)]
    pub vendor: String,
    /// Product name.
    #[serde(default)]
    pub product: String,
    /// Revision string.
    #[serde(default)]
    pub revision: String,
}

/// Drift applied on top of gaussian or sinusoidal behavior.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DriftModifier {
    /// When false, the modifier is ignored.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Change per simulation second (`rate × dt` in the engine).
    pub rate: f64,
    /// Inclusive `[min, max]` clamp.
    pub bounds: (f64, f64),
}

fn default_true() -> bool {
    true
}

impl DriftModifier {
    pub(crate) fn validate(&self, ctx: &str) -> Result<(), SpecError> {
        if self.bounds.0 >= self.bounds.1 {
            return Err(SpecError::Validation(format!(
                "{ctx}: drift bounds[0] must be less than bounds[1]"
            )));
        }
        Ok(())
    }
}

/// Per-register simulation behavior.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "behavior", rename_all = "snake_case")]
pub enum BehaviorSpec {
    /// Always returns `state.base`.
    Constant,
    /// Gaussian noise around `state.base`.
    GaussianNoise {
        /// Standard deviation.
        std_dev: f64,
        /// Optional slow drift of the center.
        #[serde(default)]
        drift: Option<DriftModifier>,
    },
    /// Sine wave around `state.base`.
    Sinusoidal {
        /// Full cycle duration in hours.
        period_hours: f64,
        /// Peak deviation from center.
        amplitude: f64,
        /// Optional slow drift of the center.
        #[serde(default)]
        drift: Option<DriftModifier>,
    },
    /// Linear drift of `state.base`.
    Drift {
        /// Change per simulation second (`rate × dt` in the engine).
        rate: f64,
        /// Inclusive clamp.
        bounds: (f64, f64),
    },
    /// Repeating ramp.
    Sawtooth {
        /// Period in seconds.
        period_seconds: f64,
        /// Ramp start.
        min: f64,
        /// Ramp end.
        max: f64,
    },
    /// Discrete jumps at elapsed times.
    Step {
        /// Ordered or unordered steps; engine sorts by `at`.
        steps: Vec<StepEntry>,
    },
}

/// A scheduled step value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepEntry {
    /// Seconds from simulation start.
    pub at: f64,
    /// Value to hold after `at`.
    pub value: f64,
}

impl BehaviorSpec {
    pub(crate) fn validate(&self, ctx: &str) -> Result<(), SpecError> {
        match self {
            Self::Constant => Ok(()),
            Self::GaussianNoise { std_dev, drift } => {
                if *std_dev <= 0.0 {
                    return Err(SpecError::Validation(format!("{ctx}: std_dev must be > 0")));
                }
                if let Some(drift) = drift {
                    drift.validate(ctx)?;
                }
                Ok(())
            }
            Self::Sinusoidal {
                period_hours,
                amplitude,
                drift,
            } => {
                if *period_hours <= 0.0 {
                    return Err(SpecError::Validation(format!(
                        "{ctx}: period_hours must be > 0"
                    )));
                }
                if *amplitude <= 0.0 {
                    return Err(SpecError::Validation(format!(
                        "{ctx}: amplitude must be > 0"
                    )));
                }
                if let Some(drift) = drift {
                    drift.validate(ctx)?;
                }
                Ok(())
            }
            Self::Drift { bounds, .. } => {
                if bounds.0 >= bounds.1 {
                    return Err(SpecError::Validation(format!(
                        "{ctx}: drift bounds[0] must be less than bounds[1]"
                    )));
                }
                Ok(())
            }
            Self::Sawtooth {
                period_seconds,
                min,
                max,
            } => {
                if *period_seconds <= 0.0 {
                    return Err(SpecError::Validation(format!(
                        "{ctx}: period_seconds must be > 0"
                    )));
                }
                if min >= max {
                    return Err(SpecError::Validation(format!(
                        "{ctx}: sawtooth min must be less than max"
                    )));
                }
                Ok(())
            }
            Self::Step { steps } => {
                if steps.is_empty() {
                    return Err(SpecError::Validation(format!(
                        "{ctx}: step behavior needs at least one step"
                    )));
                }
                if steps.iter().any(|s| s.at < 0.0) {
                    return Err(SpecError::Validation(format!(
                        "{ctx}: step.at must be >= 0"
                    )));
                }
                Ok(())
            }
        }
    }

    /// Short behavior name used in `/config` and `simbus check`.
    #[must_use]
    pub const fn kind_name(&self) -> &'static str {
        match self {
            Self::Constant => "constant",
            Self::GaussianNoise { .. } => "gaussian_noise",
            Self::Sinusoidal { .. } => "sinusoidal",
            Self::Drift { .. } => "drift",
            Self::Sawtooth { .. } => "sawtooth",
            Self::Step { .. } => "step",
        }
    }
}

/// A holding or input register.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegisterSpec {
    /// Zero-based address of the first word.
    pub address: u16,
    /// Unique name within the device.
    pub name: String,
    /// Human description.
    #[serde(default)]
    pub description: String,
    /// Engineering unit.
    #[serde(default)]
    pub unit: String,
    /// Default real-world value.
    pub default: f64,
    /// `raw ≈ real_value * scale`.
    #[serde(default = "default_scale")]
    pub scale: u32,
    /// Wire type.
    #[serde(default)]
    pub data_type: DataType,
    /// Optional live behavior.
    #[serde(default)]
    pub simulation: Option<BehaviorSpec>,
}

fn default_scale() -> u32 {
    1
}

/// Coil or discrete trigger.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriggerSpec {
    /// Source holding or input register name.
    pub source_register: String,
    /// Comparison.
    pub condition: TriggerCondition,
    /// Threshold in real-world units.
    pub threshold: f64,
}

/// Coil or discrete input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoilSpec {
    /// Zero-based address.
    pub address: u16,
    /// Unique name.
    pub name: String,
    /// Human description.
    #[serde(default)]
    pub description: String,
    /// Default boolean state.
    #[serde(default)]
    pub default: bool,
    /// Optional trigger.
    #[serde(default)]
    pub trigger: Option<TriggerSpec>,
}

/// Full register map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RegisterMapSpec {
    /// Holding registers.
    #[serde(default)]
    pub holding: Vec<RegisterSpec>,
    /// Input registers.
    #[serde(default)]
    pub input: Vec<RegisterSpec>,
    /// Coils.
    #[serde(default)]
    pub coils: Vec<CoilSpec>,
    /// Discrete inputs.
    #[serde(default)]
    pub discrete: Vec<CoilSpec>,
}

/// Legacy / default Modbus listen settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModbusSpec {
    /// Default TCP port.
    pub default_port: u16,
    /// Unit ID 1–247.
    #[serde(default = "default_unit_id")]
    pub unit_id: u8,
    /// Multi-word endianness.
    #[serde(default)]
    pub endianness: Endianness,
}

fn default_unit_id() -> u8 {
    1
}

/// Named alarm metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlarmSpec {
    /// Display name.
    pub name: String,
    /// Severity.
    pub severity: AlarmSeverity,
    /// Coil or discrete name that activates this alarm.
    pub trigger: String,
}

/// A protocol binding declared on the device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "protocol", rename_all = "kebab-case")]
pub enum BindingSpec {
    /// Modbus TCP slave.
    ModbusTcp {
        /// Listen port.
        #[serde(default)]
        port: Option<u16>,
        /// Unit ID override.
        #[serde(default)]
        unit_id: Option<u8>,
    },
    /// Modbus RTU slave.
    ModbusRtu {
        /// Serial device path.
        device: String,
        /// Baud rate.
        #[serde(default = "default_baud")]
        baudrate: u32,
    },
    /// Modbus TLS slave (IANA 802).
    ModbusTls {
        /// Listen port.
        #[serde(default = "default_modbus_tls_port")]
        port: u16,
        /// PEM certificate path (required at boot).
        certfile: String,
        /// PEM key path (required at boot).
        keyfile: String,
        /// Optional PEM CA; when set, the server requires a client certificate.
        #[serde(default)]
        cafile: Option<String>,
    },
    /// SNMP v2c agent.
    #[serde(rename = "snmp-v2c")]
    SnmpV2c {
        /// UDP port.
        #[serde(default = "default_snmp_port")]
        port: u16,
        /// Community string.
        #[serde(default = "default_community")]
        community: String,
        /// Optional OID map path.
        #[serde(default)]
        map: Option<String>,
    },
    /// OPC UA server.
    Opcua {
        /// Listen port.
        #[serde(default = "default_opcua_port")]
        port: u16,
    },
    /// Sparkplug B MQTT publisher.
    MqttSparkplug {
        /// Broker URL, e.g. `mqtt://broker:1883`.
        broker: String,
        /// Sparkplug group id.
        group_id: String,
        /// Edge node id.
        edge_node_id: String,
    },
    /// BACnet/IP server.
    BacnetIp {
        /// UDP port.
        #[serde(default = "default_bacnet_port")]
        port: u16,
        /// BACnet device instance.
        device_instance: u32,
    },
}

fn default_baud() -> u32 {
    9600
}

fn default_modbus_tls_port() -> u16 {
    802
}

fn default_snmp_port() -> u16 {
    161
}

fn default_community() -> String {
    "public".to_owned()
}

fn default_opcua_port() -> u16 {
    4840
}

fn default_bacnet_port() -> u16 {
    47808
}

impl BindingSpec {
    /// Protocol id for this binding.
    #[must_use]
    pub const fn protocol_id(&self) -> ProtocolId {
        match self {
            Self::ModbusTcp { .. } => ProtocolId::ModbusTcp,
            Self::ModbusRtu { .. } => ProtocolId::ModbusRtu,
            Self::ModbusTls { .. } => ProtocolId::ModbusTls,
            Self::SnmpV2c { .. } => ProtocolId::SnmpV2c,
            Self::Opcua { .. } => ProtocolId::Opcua,
            Self::MqttSparkplug { .. } => ProtocolId::MqttSparkplug,
            Self::BacnetIp { .. } => ProtocolId::BacnetIp,
        }
    }
}

/// Top-level device specification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceSpec {
    /// Display name.
    pub name: String,
    /// Language version. MUST be `1` for this crate. Omitted files default to `1`.
    #[serde(default = "default_spec_version")]
    pub spec_version: u32,
    /// Map / product version string (not the language version).
    pub version: String,
    /// Device type key.
    #[serde(rename = "type")]
    pub device_type: String,
    /// Human description.
    #[serde(default)]
    pub description: String,
    /// Optional identity.
    #[serde(default)]
    pub identity: IdentitySpec,
    /// Modbus defaults (required for v1 files).
    pub modbus: ModbusSpec,
    /// Register map.
    #[serde(default)]
    pub registers: RegisterMapSpec,
    /// Alarm metadata.
    #[serde(default)]
    pub alarms: Vec<AlarmSpec>,
    /// Protocol bindings. Empty means infer Modbus TCP from `modbus`.
    #[serde(default)]
    pub bindings: Vec<BindingSpec>,
    /// Scenarios bundled with this device. Loaded at boot; not auto-run.
    #[serde(default)]
    pub scenarios: Vec<ScenarioSpec>,
}

fn default_spec_version() -> u32 {
    SPEC_VERSION
}

impl DeviceSpec {
    /// Bindings to start, inferring Modbus TCP when the list is empty.
    #[must_use]
    pub fn resolved_bindings(&self) -> Vec<BindingSpec> {
        if self.bindings.is_empty() {
            vec![BindingSpec::ModbusTcp {
                port: Some(self.modbus.default_port),
                unit_id: Some(self.modbus.unit_id),
            }]
        } else {
            self.bindings.clone()
        }
    }

    /// Protocols declared on this device that this runtime cannot serve.
    #[must_use]
    pub fn unimplemented_protocols(&self) -> Vec<ProtocolId> {
        self.resolved_bindings()
            .iter()
            .map(BindingSpec::protocol_id)
            .filter(|p| !p.is_implemented())
            .collect()
    }

    /// Bundled scenario by id.
    #[must_use]
    pub fn scenario(&self, id: &str) -> Option<&ScenarioSpec> {
        self.scenarios.iter().find(|s| s.id == id)
    }

    /// Validate cross-references and numeric constraints.
    pub fn validate(&self) -> Result<(), SpecError> {
        if self.spec_version != SPEC_VERSION {
            return Err(SpecError::Validation(format!(
                "spec_version {} is not supported (this runtime understands {})",
                self.spec_version, SPEC_VERSION
            )));
        }
        if self.modbus.unit_id == 0 || self.modbus.unit_id > 247 {
            return Err(SpecError::Validation(
                "modbus.unit_id must be between 1 and 247".to_owned(),
            ));
        }
        if self.modbus.default_port == 0 {
            return Err(SpecError::Validation(
                "modbus.default_port must be >= 1".to_owned(),
            ));
        }

        let mut names = std::collections::HashSet::new();
        for (space, regs) in [
            ("holding", &self.registers.holding),
            ("input", &self.registers.input),
        ] {
            let mut occupied: std::collections::HashSet<u16> = std::collections::HashSet::new();
            for reg in regs {
                if !names.insert(reg.name.clone()) {
                    return Err(SpecError::Validation(format!(
                        "duplicate register name '{}'",
                        reg.name
                    )));
                }
                if reg.scale == 0 {
                    return Err(SpecError::Validation(format!(
                        "register '{}': scale must be >= 1",
                        reg.name
                    )));
                }
                if let Some(sim) = &reg.simulation {
                    sim.validate(&format!("register '{}'", reg.name))?;
                }
                let words = u16::from(reg.data_type.word_count());
                for offset in 0..words {
                    let addr = reg.address.saturating_add(offset);
                    if !occupied.insert(addr) {
                        return Err(SpecError::Validation(format!(
                            "{space} address {addr} overlaps another register"
                        )));
                    }
                }
            }
        }

        let mut coil_names: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for coil in self.registers.coils.iter().chain(&self.registers.discrete) {
            if !coil_names.insert(coil.name.as_str()) {
                return Err(SpecError::Validation(format!(
                    "duplicate coil/discrete name '{}'",
                    coil.name
                )));
            }
            if let Some(trigger) = &coil.trigger {
                if !names.contains(&trigger.source_register) {
                    return Err(SpecError::Validation(format!(
                        "coil '{}': trigger references unknown register '{}'",
                        coil.name, trigger.source_register
                    )));
                }
            }
        }

        for alarm in &self.alarms {
            if !coil_names.contains(alarm.trigger.as_str()) {
                return Err(SpecError::Validation(format!(
                    "alarm '{}': references unknown coil '{}'",
                    alarm.name, alarm.trigger
                )));
            }
        }

        self.validate_scenarios()?;
        Ok(())
    }

    /// Validate one scenario against this device's register and coil names.
    ///
    /// Used for YAML `scenarios:` and for session install (`POST /scenarios`).
    pub fn validate_guest_scenario(&self, scenario: &ScenarioSpec) -> Result<(), SpecError> {
        if scenario.id.is_empty() {
            return Err(SpecError::Validation(format!(
                "scenario '{}': embedded scenarios must set id",
                scenario.name
            )));
        }
        if !scenario_id_ok(&scenario.id) {
            return Err(SpecError::Validation(format!(
                "scenario '{}': id must be lowercase kebab-case (got '{}')",
                scenario.name, scenario.id
            )));
        }
        scenario.validate()?;
        let register_names: std::collections::HashSet<String> = self
            .registers
            .holding
            .iter()
            .chain(&self.registers.input)
            .map(|r| r.name.clone())
            .collect();
        let coil_names: std::collections::HashSet<&str> = self
            .registers
            .coils
            .iter()
            .chain(&self.registers.discrete)
            .map(|c| c.name.as_str())
            .collect();
        let ctx = format!("scenario '{}'", scenario.id);
        for step in &scenario.steps {
            match step {
                ScenarioStep::SetRegister(s) => {
                    let in_holding = self
                        .registers
                        .holding
                        .iter()
                        .any(|r| r.name == s.register_name);
                    let in_input = self
                        .registers
                        .input
                        .iter()
                        .any(|r| r.name == s.register_name);
                    let ok = match s.register_type.as_str() {
                        "holding" => in_holding,
                        "input" => in_input,
                        _ => false,
                    };
                    if !ok {
                        return Err(SpecError::Validation(format!(
                            "{ctx}: set_register '{}' not found in {}",
                            s.register_name, s.register_type
                        )));
                    }
                }
                ScenarioStep::SetCoil(s) => {
                    if !coil_names.contains(s.coil.as_str()) {
                        return Err(SpecError::Validation(format!(
                            "{ctx}: set_coil '{}' not found",
                            s.coil
                        )));
                    }
                }
                ScenarioStep::InjectFault(s) => {
                    let Some(target) = s.register_name.as_deref() else {
                        return Err(SpecError::Validation(format!(
                            "{ctx}: inject_fault requires register_name"
                        )));
                    };
                    match s.fault_type {
                        FaultType::Alarm => {
                            if !coil_names.contains(target) {
                                return Err(SpecError::Validation(format!(
                                    "{ctx}: inject_fault alarm target '{target}' is not a coil"
                                )));
                            }
                        }
                        _ => {
                            if !register_names.contains(target) {
                                return Err(SpecError::Validation(format!(
                                    "{ctx}: inject_fault target '{target}' is not a register"
                                )));
                            }
                        }
                    }
                }
                ScenarioStep::SetTickInterval(_) => {}
            }
        }
        Ok(())
    }

    fn validate_scenarios(&self) -> Result<(), SpecError> {
        let mut ids = std::collections::HashSet::new();
        for scenario in &self.scenarios {
            if !ids.insert(scenario.id.as_str()) {
                return Err(SpecError::Validation(format!(
                    "duplicate scenario id '{}'",
                    scenario.id
                )));
            }
            self.validate_guest_scenario(scenario)?;
        }
        Ok(())
    }
}

fn scenario_id_ok(id: &str) -> bool {
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_lowercase()
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

#[cfg(test)]
mod wire_format_tests {
    use super::{DataType, Endianness};

    #[test]
    fn wire_strings_match_yaml() {
        assert_eq!(Endianness::Big.as_str(), "big");
        assert_eq!(Endianness::BigSwap.as_str(), "big_swap");
        assert_eq!(Endianness::LittleSwap.as_str(), "little_swap");
        assert_eq!(DataType::Uint16.as_str(), "uint16");
        assert_eq!(DataType::Float32.as_str(), "float32");
        assert_eq!(DataType::Float32.word_count(), 2);
    }
}
