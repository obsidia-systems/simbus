//! Scenario YAML types.

use serde::{Deserialize, Serialize};

use crate::{FaultType, SpecError};

/// Analog vs binary literal for `set_point`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PointLiteral {
    /// Boolean.
    Bool(bool),
    /// Signed integer (YAML `50`).
    Int(i64),
    /// Float.
    Float(f64),
}

impl PointLiteral {
    /// Analog engineering value.
    #[must_use]
    pub fn as_analog(&self) -> Option<f64> {
        match *self {
            Self::Float(v) => Some(v),
            Self::Int(v) => Some(v as f64),
            Self::Bool(_) => None,
        }
    }

    /// Binary value.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match *self {
            Self::Bool(v) => Some(v),
            Self::Int(0) => Some(false),
            Self::Int(1) => Some(true),
            _ => None,
        }
    }
}

/// Set a point by id (language 2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetPointStep {
    /// Seconds from scenario start.
    pub at: f64,
    /// Point id.
    pub point: String,
    /// Engineering or boolean value.
    pub value: PointLiteral,
}

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
    /// Target register, coil, or point id.
    #[serde(default, alias = "point")]
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
    /// Write a canonical point (language 2).
    SetPoint(SetPointStep),
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
            Self::SetPoint(s) => s.at,
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
