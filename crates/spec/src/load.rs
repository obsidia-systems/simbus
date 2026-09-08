//! Load device and scenario YAML from strings or files.

use std::path::Path;

use crate::{DeviceSpec, ScenarioSpec, SpecError};

/// Parse and validate a device spec from YAML text.
pub fn load_device_from_str(yaml: &str) -> Result<DeviceSpec, SpecError> {
    let spec: DeviceSpec = serde_yaml::from_str(yaml)?;
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
