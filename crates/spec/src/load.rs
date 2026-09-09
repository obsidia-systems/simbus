//! Load device and scenario YAML from strings or files.

use std::path::Path;

use crate::types::SPEC_VERSION;
use crate::v2::{finish_v1, load_v2};
use crate::{DeviceSpec, ScenarioSpec, SpecError};

/// Parse and validate a device spec from YAML text.
pub fn load_device_from_str(yaml: &str) -> Result<DeviceSpec, SpecError> {
    let value: serde_yaml::Value = serde_yaml::from_str(yaml)?;
    let version = value
        .get("spec_version")
        .and_then(serde_yaml::Value::as_u64)
        .unwrap_or(1);
    let spec = if version <= 1 {
        let parsed: DeviceSpec = serde_yaml::from_value(value)?;
        finish_v1(parsed)
    } else if version == u64::from(SPEC_VERSION) {
        if value.get("registers").is_some() {
            return Err(SpecError::Validation(
                "language 2: do not mix registers: with points: (use export on bindings)".into(),
            ));
        }
        load_v2(value)?
    } else {
        return Err(SpecError::Validation(format!(
            "spec_version {version} is not supported (this runtime understands 1–{SPEC_VERSION})"
        )));
    };
    spec.validate()?;
    Ok(spec)
}

/// Parse and validate a device spec from a file.
pub fn load_device_from_path(path: impl AsRef<Path>) -> Result<DeviceSpec, SpecError> {
    let path = path.as_ref();
    let yaml = std::fs::read_to_string(path).map_err(|source| SpecError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    load_device_from_str(&yaml)
}

/// Parse and validate a scenario spec from YAML text.
pub fn load_scenario_from_str(yaml: &str) -> Result<ScenarioSpec, SpecError> {
    let spec: ScenarioSpec = serde_yaml::from_str(yaml)?;
    spec.validate()?;
    Ok(spec)
}

/// Parse and validate a scenario spec from a file.
pub fn load_scenario_from_path(path: impl AsRef<Path>) -> Result<ScenarioSpec, SpecError> {
    let path = path.as_ref();
    let yaml = std::fs::read_to_string(path).map_err(|source| SpecError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    load_scenario_from_str(&yaml)
}
