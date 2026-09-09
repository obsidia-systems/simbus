//! Device YAML language (`docs/spec.md`).
//!
//! Existing v1 device files (Modbus implied, `spec_version` omitted) remain valid.
//! Optional `bindings`, `identity`, and `scenarios` fields extend the schema
//! without breaking the catalog. New protocols are added here first; this crate
//! accepts their syntax even when the runtime cannot serve them yet.

#![allow(clippy::missing_errors_doc)]

mod error;
mod load;
mod report;
mod scenario;
mod types;
mod v2;

pub use error::SpecError;
pub use load::{
    load_device_from_path, load_device_from_str, load_scenario_from_path, load_scenario_from_str,
};
pub use report::device_report;
pub use scenario::{
    InjectFaultStep, PointLiteral, ScenarioSpec, ScenarioStep, SetCoilStep, SetPointStep,
    SetRegisterStep, SetTickIntervalStep,
};
pub use types::{
    AlarmSeverity, AlarmSpec, BacnetExportEntry, BacnetObjectType, BehaviorSpec, BindingSpec,
    CoilSpec, DataType, DeviceSpec, DriftModifier, Endianness, FaultType, IdentitySpec,
    ModbusExportEntry, ModbusExportSpace, ModbusSpec, OpcuaExportEntry, PointClass, PointKind,
    PointSpec, ProtocolId, RegisterMapSpec, RegisterSpace, RegisterSpec, SPEC_VERSION,
    SPEC_VERSION_MIN, StepEntry, TriggerCondition, TriggerSpec, UaNaming,
};
