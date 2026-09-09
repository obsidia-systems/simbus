//! Device YAML types.

use serde::{Deserialize, Serialize};

use crate::SpecError;
use crate::scenario::{ScenarioSpec, ScenarioStep};

/// Current device language. Documents may use `1` (register map) or `2` (points).
pub const SPEC_VERSION: u32 = 2;

/// Oldest language this crate still loads (`registers:` maps).
pub const SPEC_VERSION_MIN: u32 = 1;

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
        matches!(
            self,
            Self::ModbusTcp | Self::ModbusTls | Self::Opcua | Self::BacnetIp
        )
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
    /// Analog high/low around `state.base` (50% duty).
    Square {
        /// Full high+low cycle in seconds.
        period_seconds: f64,
        /// Peak deviation from center.
        amplitude: f64,
    },
    /// Symmetric ramp `min → max → min`.
    Triangle {
        /// Period in seconds.
        period_seconds: f64,
        /// Low end.
        min: f64,
        /// High end.
        max: f64,
    },
    /// Uniform random in `[min, max]` each tick.
    Uniform {
        /// Inclusive lower bound.
        min: f64,
        /// Inclusive upper bound.
        max: f64,
    },
    /// Discrete jumps at elapsed times.
    Step {
        /// Ordered or unordered steps; engine sorts by `at`.
        steps: Vec<StepEntry>,
    },
    /// Walk `values` in order, one entry every `dwell_seconds`, then repeat.
    Cycle {
        /// Simulation seconds spent on each value.
        dwell_seconds: f64,
        /// Non-empty list of engineering values.
        values: Vec<f64>,
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
            Self::Square {
                period_seconds,
                amplitude,
            } => {
                if *period_seconds <= 0.0 {
                    return Err(SpecError::Validation(format!(
                        "{ctx}: period_seconds must be > 0"
                    )));
                }
                if *amplitude <= 0.0 {
                    return Err(SpecError::Validation(format!(
                        "{ctx}: amplitude must be > 0"
                    )));
                }
                Ok(())
            }
            Self::Triangle {
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
                        "{ctx}: triangle min must be less than max"
                    )));
                }
                Ok(())
            }
            Self::Uniform { min, max } => {
                if min >= max {
                    return Err(SpecError::Validation(format!(
                        "{ctx}: uniform min must be less than max"
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
            Self::Cycle {
                dwell_seconds,
                values,
            } => {
                if *dwell_seconds <= 0.0 {
                    return Err(SpecError::Validation(format!(
                        "{ctx}: dwell_seconds must be > 0"
                    )));
                }
                if values.is_empty() {
                    return Err(SpecError::Validation(format!(
                        "{ctx}: cycle behavior needs at least one value"
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
            Self::Square { .. } => "square",
            Self::Triangle { .. } => "triangle",
            Self::Uniform { .. } => "uniform",
            Self::Step { .. } => "step",
            Self::Cycle { .. } => "cycle",
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
    /// Source analog point / register id.
    #[serde(alias = "source")]
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

impl Default for ModbusSpec {
    fn default() -> Self {
        Self {
            default_port: 502,
            unit_id: 1,
            endianness: Endianness::Big,
        }
    }
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

/// Analog vs binary point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointKind {
    /// Engineering real (`f64` in the engine).
    Analog,
    /// Two-state.
    Binary,
}

impl PointKind {
    /// Canonical YAML string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Analog => "analog",
            Self::Binary => "binary",
        }
    }
}

/// ASHRAE Input / Value / Output class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointClass {
    /// Measured. Tick owns the value.
    Input,
    /// Setpoint / parameter / software point.
    Value,
    /// Command to an actuator.
    Output,
}

impl PointClass {
    /// Canonical YAML string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Value => "value",
            Self::Output => "output",
        }
    }

    /// OPC UA / typical field write for this class.
    #[must_use]
    pub const fn field_writable(self) -> bool {
        matches!(self, Self::Value | Self::Output)
    }
}

/// How OPC UA NodeIds are built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UaNaming {
    /// `ns=N;s=holding/{name}` (language 1).
    #[default]
    SpacePrefix,
    /// `ns=N;s={id}` (language 2).
    PointId,
}

/// One device point (canonical after load).
#[derive(Debug, Clone, PartialEq)]
pub struct PointSpec {
    /// Stable id (`temperature`). Unique in the document.
    pub id: String,
    /// Analog or binary.
    pub kind: PointKind,
    /// Input / value / output.
    pub class: PointClass,
    /// Human text.
    pub description: String,
    /// Engineering unit.
    pub unit: String,
    /// Analog power-on value.
    pub analog_default: f64,
    /// Binary power-on value.
    pub binary_default: bool,
    /// Analog live behavior.
    pub simulation: Option<BehaviorSpec>,
    /// Binary input derived from an analog point.
    pub trigger: Option<TriggerSpec>,
}

impl PointSpec {
    pub(crate) fn from_register(reg: &RegisterSpec, class: PointClass) -> Self {
        Self {
            id: reg.name.clone(),
            kind: PointKind::Analog,
            class,
            description: reg.description.clone(),
            unit: reg.unit.clone(),
            analog_default: reg.default,
            binary_default: false,
            simulation: reg.simulation.clone(),
            trigger: None,
        }
    }

    pub(crate) fn from_coil(coil: &CoilSpec, class: PointClass) -> Self {
        Self {
            id: coil.name.clone(),
            kind: PointKind::Binary,
            class,
            description: coil.description.clone(),
            unit: String::new(),
            analog_default: 0.0,
            binary_default: coil.default,
            simulation: None,
            trigger: coil.trigger.clone(),
        }
    }
}

/// Modbus table for a point export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModbusExportSpace {
    /// FC3 / FC6 / FC16.
    Holding,
    /// FC4.
    Input,
    /// FC1 / FC5 / FC15.
    Coil,
    /// FC2.
    Discrete,
}

fn modbus_export_from_registers(
    registers: &RegisterMapSpec,
) -> std::collections::BTreeMap<String, ModbusExportEntry> {
    let mut export = std::collections::BTreeMap::new();
    for reg in &registers.holding {
        export.insert(
            reg.name.clone(),
            ModbusExportEntry {
                space: ModbusExportSpace::Holding,
                address: reg.address,
                scale: reg.scale,
                data_type: reg.data_type,
            },
        );
    }
    for reg in &registers.input {
        export.insert(
            reg.name.clone(),
            ModbusExportEntry {
                space: ModbusExportSpace::Input,
                address: reg.address,
                scale: reg.scale,
                data_type: reg.data_type,
            },
        );
    }
    for coil in &registers.coils {
        export.insert(
            coil.name.clone(),
            ModbusExportEntry {
                space: ModbusExportSpace::Coil,
                address: coil.address,
                scale: 1,
                data_type: DataType::Uint16,
            },
        );
    }
    for disc in &registers.discrete {
        export.insert(
            disc.name.clone(),
            ModbusExportEntry {
                space: ModbusExportSpace::Discrete,
                address: disc.address,
                scale: 1,
                data_type: DataType::Uint16,
            },
        );
    }
    export
}

/// Lift a language-1 register map into canonical points.
pub(crate) fn lift_v1(spec: &mut DeviceSpec) {
    spec.ua_naming = UaNaming::SpacePrefix;
    spec.points.clear();
    for reg in &spec.registers.holding {
        spec.points
            .push(PointSpec::from_register(reg, PointClass::Value));
    }
    for reg in &spec.registers.input {
        spec.points
            .push(PointSpec::from_register(reg, PointClass::Input));
    }
    for coil in &spec.registers.coils {
        let class = if coil.trigger.is_some() {
            PointClass::Input
        } else {
            PointClass::Value
        };
        spec.points.push(PointSpec::from_coil(coil, class));
    }
    for disc in &spec.registers.discrete {
        spec.points
            .push(PointSpec::from_coil(disc, PointClass::Input));
    }
}

impl ModbusExportSpace {
    /// Canonical YAML string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Holding => "holding",
            Self::Input => "input",
            Self::Coil => "coil",
            Self::Discrete => "discrete",
        }
    }
}

/// One row in a Modbus `export:` map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModbusExportEntry {
    /// Target table.
    pub space: ModbusExportSpace,
    /// Zero-based address.
    pub address: u16,
    /// Analog only. `raw ≈ eng × scale`.
    #[serde(default = "default_scale")]
    pub scale: u32,
    /// Analog only.
    #[serde(default)]
    pub data_type: DataType,
}

/// Placeholder so YAML `temperature: {}` deserializes.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct OpcuaExportEntry {}

/// BACnet object type in `export.object`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BacnetObjectType {
    /// Analog Input.
    AnalogInput,
    /// Analog Value.
    AnalogValue,
    /// Analog Output.
    AnalogOutput,
    /// Binary Input.
    BinaryInput,
    /// Binary Value.
    BinaryValue,
    /// Binary Output.
    BinaryOutput,
}

impl BacnetObjectType {
    /// Canonical YAML string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AnalogInput => "analog-input",
            Self::AnalogValue => "analog-value",
            Self::AnalogOutput => "analog-output",
            Self::BinaryInput => "binary-input",
            Self::BinaryValue => "binary-value",
            Self::BinaryOutput => "binary-output",
        }
    }

    /// Whether this matches `kind` × `class`.
    #[must_use]
    pub const fn matches_point(self, kind: PointKind, class: PointClass) -> bool {
        matches!(
            (self, kind, class),
            (Self::AnalogInput, PointKind::Analog, PointClass::Input)
                | (Self::AnalogValue, PointKind::Analog, PointClass::Value)
                | (Self::AnalogOutput, PointKind::Analog, PointClass::Output)
                | (Self::BinaryInput, PointKind::Binary, PointClass::Input)
                | (Self::BinaryValue, PointKind::Binary, PointClass::Value)
                | (Self::BinaryOutput, PointKind::Binary, PointClass::Output)
        )
    }
}

/// One row in a BACnet `export:` map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BacnetExportEntry {
    /// ASHRAE object type.
    pub object: BacnetObjectType,
    /// Object instance.
    pub instance: u32,
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
        /// Multi-word endianness (language 2; language 1 uses top-level `modbus`).
        #[serde(default)]
        endianness: Endianness,
        /// Point id → PDU address. Required and non-empty in language 2.
        #[serde(default)]
        export: std::collections::BTreeMap<String, ModbusExportEntry>,
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
        /// Point id → PDU address. Required and non-empty in language 2.
        #[serde(default)]
        export: std::collections::BTreeMap<String, ModbusExportEntry>,
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
        /// Point ids to publish. Required and non-empty in language 2.
        #[serde(default)]
        export: std::collections::BTreeMap<String, OpcuaExportEntry>,
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
        /// Point id → object + instance. Required and non-empty in language 2.
        #[serde(default)]
        export: std::collections::BTreeMap<String, BacnetExportEntry>,
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
    /// Language version. Omitted files default to `1`.
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
    /// Modbus defaults (language 1 YAML, or derived from a language 2 binding).
    #[serde(default)]
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
    /// Canonical points (filled at load: language 2 YAML or language 1 lift).
    #[serde(default, skip)]
    pub points: Vec<PointSpec>,
    /// OPC UA NodeId style.
    #[serde(default, skip)]
    pub ua_naming: UaNaming,
}

fn default_spec_version() -> u32 {
    SPEC_VERSION_MIN
}

impl DeviceSpec {
    /// Bindings to start, inferring Modbus TCP when the list is empty.
    #[must_use]
    pub fn resolved_bindings(&self) -> Vec<BindingSpec> {
        if self.bindings.is_empty() {
            if self.spec_version >= 2 {
                Vec::new()
            } else {
                vec![BindingSpec::ModbusTcp {
                    port: Some(self.modbus.default_port),
                    unit_id: Some(self.modbus.unit_id),
                    endianness: self.modbus.endianness,
                    export: modbus_export_from_registers(&self.registers),
                }]
            }
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
        if self.spec_version < SPEC_VERSION_MIN || self.spec_version > SPEC_VERSION {
            return Err(SpecError::Validation(format!(
                "spec_version {} is not supported (this runtime understands {}–{})",
                self.spec_version, SPEC_VERSION_MIN, SPEC_VERSION
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
            let known_coil = coil_names.contains(alarm.trigger.as_str());
            let known_binary = self
                .points
                .iter()
                .any(|p| p.kind == PointKind::Binary && p.id == alarm.trigger);
            if !known_coil && !known_binary {
                return Err(SpecError::Validation(format!(
                    "alarm '{}': references unknown coil '{}'",
                    alarm.name, alarm.trigger
                )));
            }
        }

        self.validate_points()?;
        if self.spec_version >= 2 {
            self.validate_v2_bindings()?;
        }
        self.validate_scenarios()?;
        Ok(())
    }

    fn validate_points(&self) -> Result<(), SpecError> {
        if self.spec_version >= 2 && self.points.is_empty() {
            return Err(SpecError::Validation(
                "language 2: points must not be empty".into(),
            ));
        }
        let mut ids = std::collections::HashSet::new();
        for point in &self.points {
            if point.id.is_empty() {
                return Err(SpecError::Validation("point id must not be empty".into()));
            }
            if !ids.insert(point.id.as_str()) {
                return Err(SpecError::Validation(format!(
                    "duplicate point id '{}'",
                    point.id
                )));
            }
            match point.kind {
                PointKind::Analog => {
                    if point.trigger.is_some() {
                        return Err(SpecError::Validation(format!(
                            "point '{}': analog points cannot have trigger",
                            point.id
                        )));
                    }
                    if let Some(sim) = &point.simulation {
                        sim.validate(&format!("point '{}'", point.id))?;
                    }
                }
                PointKind::Binary => {
                    if point.simulation.is_some() {
                        return Err(SpecError::Validation(format!(
                            "point '{}': binary points cannot have simulation",
                            point.id
                        )));
                    }
                }
            }
        }
        for point in &self.points {
            if let Some(trigger) = &point.trigger {
                let Some(src) = self.point(&trigger.source_register) else {
                    return Err(SpecError::Validation(format!(
                        "point '{}': trigger references unknown point '{}'",
                        point.id, trigger.source_register
                    )));
                };
                if src.kind != PointKind::Analog {
                    return Err(SpecError::Validation(format!(
                        "point '{}': trigger source '{}' must be analog",
                        point.id, trigger.source_register
                    )));
                }
            }
        }
        Ok(())
    }

    fn validate_v2_bindings(&self) -> Result<(), SpecError> {
        for binding in &self.bindings {
            match binding {
                BindingSpec::ModbusTcp { export, .. } | BindingSpec::ModbusTls { export, .. } => {
                    if export.is_empty() {
                        return Err(SpecError::Validation(
                            "language 2: modbus bindings require a non-empty export".into(),
                        ));
                    }
                    self.validate_modbus_export(export)?;
                }
                BindingSpec::Opcua { export, .. } => {
                    if export.is_empty() {
                        return Err(SpecError::Validation(
                            "language 2: opcua bindings require a non-empty export".into(),
                        ));
                    }
                    for id in export.keys() {
                        if self.point(id).is_none() {
                            return Err(SpecError::Validation(format!(
                                "opcua export '{id}' is not a point"
                            )));
                        }
                    }
                }
                BindingSpec::BacnetIp { export, .. } => {
                    if export.is_empty() {
                        return Err(SpecError::Validation(
                            "language 2: bacnet-ip bindings require a non-empty export".into(),
                        ));
                    }
                    let mut seen = std::collections::HashSet::new();
                    for (id, entry) in export {
                        let Some(_point) = self.point(id) else {
                            return Err(SpecError::Validation(format!(
                                "bacnet export '{id}' is not a point"
                            )));
                        };
                        if !seen.insert((entry.object, entry.instance)) {
                            return Err(SpecError::Validation(format!(
                                "bacnet export '{id}': duplicate {} instance {}",
                                entry.object.as_str(),
                                entry.instance
                            )));
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn validate_modbus_export(
        &self,
        export: &std::collections::BTreeMap<String, ModbusExportEntry>,
    ) -> Result<(), SpecError> {
        let mut holding: std::collections::HashSet<u16> = std::collections::HashSet::new();
        let mut input: std::collections::HashSet<u16> = std::collections::HashSet::new();
        let mut coils: std::collections::HashSet<u16> = std::collections::HashSet::new();
        let mut discrete: std::collections::HashSet<u16> = std::collections::HashSet::new();
        for (id, entry) in export {
            let Some(point) = self.point(id) else {
                return Err(SpecError::Validation(format!(
                    "modbus export '{id}' is not a point"
                )));
            };
            match (point.kind, entry.space) {
                (PointKind::Analog, ModbusExportSpace::Holding | ModbusExportSpace::Input) => {
                    if entry.scale == 0 {
                        return Err(SpecError::Validation(format!(
                            "modbus export '{id}': scale must be >= 1"
                        )));
                    }
                }
                (PointKind::Binary, ModbusExportSpace::Coil | ModbusExportSpace::Discrete) => {}
                (_kind, space) => {
                    return Err(SpecError::Validation(format!(
                        "modbus export '{id}': {} point cannot use space {}",
                        point.kind.as_str(),
                        space.as_str()
                    )));
                }
            }
            let occupied = match entry.space {
                ModbusExportSpace::Holding => &mut holding,
                ModbusExportSpace::Input => &mut input,
                ModbusExportSpace::Coil => &mut coils,
                ModbusExportSpace::Discrete => &mut discrete,
            };
            let words = if point.kind == PointKind::Analog {
                u16::from(entry.data_type.word_count())
            } else {
                1
            };
            for offset in 0..words {
                let addr = entry.address.saturating_add(offset);
                if !occupied.insert(addr) {
                    return Err(SpecError::Validation(format!(
                        "modbus {} address {addr} overlaps another export",
                        entry.space.as_str()
                    )));
                }
            }
        }
        Ok(())
    }

    /// Notes when a BACnet object does not match `kind` × `class`, or when the
    /// point has no engine cell because this document also binds Modbus and did
    /// not export that id there (`docs/bacnet.md` §4).
    #[must_use]
    pub fn bacnet_export_warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        for binding in &self.bindings {
            if let BindingSpec::BacnetIp { export, .. } = binding {
                for (id, entry) in export {
                    if let Some(point) = self.point(id) {
                        if !entry.object.matches_point(point.kind, point.class) {
                            out.push(format!(
                                "bacnet export '{id}' is {} but point is {} {}",
                                entry.object.as_str(),
                                point.kind.as_str(),
                                point.class.as_str()
                            ));
                        }
                        if !self.point_is_backed(id) {
                            out.push(format!(
                                "bacnet export '{id}' has no engine cell: this document binds Modbus but does not export '{id}' there"
                            ));
                        }
                    }
                }
            }
        }
        out
    }

    /// Whether a point currently has a cell in the engine bank.
    fn point_is_backed(&self, id: &str) -> bool {
        self.registers.holding.iter().any(|r| r.name == id)
            || self.registers.input.iter().any(|r| r.name == id)
            || self.registers.coils.iter().any(|c| c.name == id)
            || self.registers.discrete.iter().any(|c| c.name == id)
    }

    /// Look up a canonical point.
    #[must_use]
    pub fn point(&self, id: &str) -> Option<&PointSpec> {
        self.points.iter().find(|p| p.id == id)
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
                ScenarioStep::SetPoint(s) => {
                    let Some(point) = self.point(&s.point) else {
                        return Err(SpecError::Validation(format!(
                            "{ctx}: set_point '{}' not found",
                            s.point
                        )));
                    };
                    match point.kind {
                        crate::PointKind::Analog => {
                            if s.value.as_analog().is_none() {
                                return Err(SpecError::Validation(format!(
                                    "{ctx}: set_point '{}' needs a numeric value",
                                    s.point
                                )));
                            }
                        }
                        crate::PointKind::Binary => {
                            if s.value.as_bool().is_none() {
                                return Err(SpecError::Validation(format!(
                                    "{ctx}: set_point '{}' needs a boolean value",
                                    s.point
                                )));
                            }
                        }
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
                    let analog = self
                        .points
                        .iter()
                        .any(|p| p.kind == crate::PointKind::Analog && p.id == target)
                        || register_names.contains(target);
                    let binary = self
                        .points
                        .iter()
                        .any(|p| p.kind == crate::PointKind::Binary && p.id == target)
                        || coil_names.contains(target);
                    match s.fault_type {
                        FaultType::Alarm => {
                            if !binary {
                                return Err(SpecError::Validation(format!(
                                    "{ctx}: inject_fault alarm target '{target}' is not a binary point"
                                )));
                            }
                        }
                        _ => {
                            if !analog {
                                return Err(SpecError::Validation(format!(
                                    "{ctx}: inject_fault target '{target}' is not an analog point"
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
