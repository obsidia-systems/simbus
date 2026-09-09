//! Active fault records.

use spec::FaultType;

/// A TTL override applied on each tick.
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveFault {
    /// Kind.
    pub fault_type: FaultType,
    /// Register or coil name; `None` means device-wide.
    pub register_name: Option<String>,
    /// Spike target or noise factor.
    pub value: Option<f64>,
    /// Original duration.
    pub duration_s: f64,
    /// Time remaining.
    pub remaining_s: f64,
}

impl ActiveFault {
    /// Build a fault with remaining time equal to duration.
    #[must_use]
    pub fn new(
        fault_type: FaultType,
        register_name: Option<String>,
        value: Option<f64>,
        duration_s: f64,
    ) -> Self {
        Self {
            fault_type,
            register_name,
            value,
            duration_s,
            remaining_s: duration_s,
        }
    }

    /// Map key used in the fault table.
    #[must_use]
    pub fn key(&self) -> String {
        self.register_name
            .clone()
            .unwrap_or_else(|| "_device".to_owned())
    }
}
