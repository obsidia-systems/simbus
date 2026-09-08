//! Scenario YAML types.

use serde::{Deserialize, Serialize};

use crate::{FaultType, SpecError};

/// Set a holding or input register to a real-world value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetRegisterStep {
    /// Seconds from scenario start.
    pub at: f64,
    /// Register name.
    pub register_name: String,
    /// Real-world value.
    pub value: f64,
    /// Target space.
    #[serde(default = "default_holding")]
    pub register_type: String,
}

fn default_holding() -> String {
    "holding".to_owned()
}

/// Inject a TTL fault.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InjectFaultStep {
    /// Seconds from scenario start.
    pub at: f64,
    /// Fault kind.
    pub fault_type: FaultType,
    /// Target register or coil name.
    #[serde(default)]
    pub register_name: Option<String>,
    /// Spike target or noise factor.
    #[serde(default)]
    pub value: Option<f64>,
    /// Duration in seconds.
    #[serde(default = "default_duration")]
    pub duration_s: f64,
}

fn default_duration() -> f64 {
    30.0
}

/// Force a coil or discrete input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetCoilStep {
    /// Seconds from scenario start.
    pub at: f64,
    /// Coil or discrete name.
    pub coil: String,
    /// Target state.
    pub value: bool,
}

/// Change tick interval mid-scenario.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetTickIntervalStep {
    /// Seconds from scenario start.
    pub at: f64,
    /// New tick interval.
    pub tick_interval: f64,
}

/// A single timed scenario action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ScenarioStep {
    /// Write a register and shift `state.base`.
    SetRegister(SetRegisterStep),
    /// Inject a fault.
    InjectFault(InjectFaultStep),
    /// Write a coil or discrete.
    SetCoil(SetCoilStep),
    /// Update tick interval.
    SetTickInterval(SetTickIntervalStep),
}

impl ScenarioStep {
    /// Scheduled time in seconds.
    #[must_use]
    pub fn at(&self) -> f64 {
        match self {
            Self::SetRegister(s) => s.at,
            Self::InjectFault(s) => s.at,
            Self::SetCoil(s) => s.at,
            Self::SetTickInterval(s) => s.at,
        }
    }
}

/// Timed event sequence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScenarioSpec {
    /// Stable id used by `POST /scenarios/{id}/run` (`heat-wave`).
    ///
    /// Required when the scenario is embedded in a device document. Standalone
    /// YAML (legacy Python catalog) may omit it.
    #[serde(default)]
    pub id: String,
    /// Display name.
    pub name: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Steps (any order; runner sorts by `at`).
    pub steps: Vec<ScenarioStep>,
}

impl ScenarioSpec {
    /// Validate step constraints.
    pub fn validate(&self) -> Result<(), SpecError> {
        if self.steps.is_empty() {
            return Err(SpecError::Validation(
                "scenario needs at least one step".to_owned(),
            ));
        }
        for step in &self.steps {
            if step.at() < 0.0 {
                return Err(SpecError::Validation("step.at must be >= 0".to_owned()));
            }
            match step {
                ScenarioStep::InjectFault(s) if s.duration_s <= 0.0 => {
                    return Err(SpecError::Validation(
                        "inject_fault duration_s must be > 0".to_owned(),
                    ));
                }
                ScenarioStep::SetTickInterval(s) if s.tick_interval <= 0.0 => {
                    return Err(SpecError::Validation(
                        "tick_interval must be > 0".to_owned(),
                    ));
                }
                ScenarioStep::SetRegister(s)
                    if s.register_type != "holding" && s.register_type != "input" =>
                {
                    return Err(SpecError::Validation(
                        "register_type must be 'holding' or 'input'".to_owned(),
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Id used to run this scenario (`id`, or `name` when `id` is empty).
    #[must_use]
    pub fn run_id(&self) -> &str {
        if self.id.is_empty() {
            &self.name
        } else {
            &self.id
        }
    }
}
