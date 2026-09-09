//! Wall-clock-agnostic scenario playback.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;
use spec::ScenarioSpec;

use crate::Device;

/// High-level runner state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioState {
    /// Nothing loaded.
    Idle,
    /// Steps are being applied.
    Running,
    /// All steps applied.
    Completed,
    /// Cancelled.
    Stopped,
}

impl ScenarioState {
    /// Wire string used by the HTTP API.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Stopped => "stopped",
        }
    }
}

/// Snapshot of runner progress.
#[derive(Debug, Clone, PartialEq)]
pub struct ScenarioStatus {
    /// State.
    pub state: ScenarioState,
    /// Active scenario name.
    pub scenario_name: Option<String>,
    /// 1-based index of last executed step.
    pub step_index: usize,
    /// Total steps.
    pub total_steps: usize,
    /// Elapsed seconds.
    pub elapsed_s: f64,
}

/// Tracks scenario progress. The HTTP layer drives sleeps and applies steps.
pub struct ScenarioRunner {
    device: std::sync::Arc<Device>,
    status: Mutex<ScenarioStatus>,
    generation: AtomicU64,
    session: Mutex<HashMap<String, ScenarioSpec>>,
}

impl ScenarioRunner {
    /// Bind a runner to a device.
    #[must_use]
    pub fn new(device: std::sync::Arc<Device>) -> Self {
        Self {
            device,
            status: Mutex::new(ScenarioStatus {
                state: ScenarioState::Idle,
                scenario_name: None,
                step_index: 0,
                total_steps: 0,
                elapsed_s: 0.0,
            }),
            generation: AtomicU64::new(0),
            session: Mutex::new(HashMap::new()),
        }
    }

    /// Device this runner drives.
    #[must_use]
    pub fn device(&self) -> std::sync::Arc<Device> {
        self.device.clone()
    }

    /// Current status copy.
    #[must_use]
    pub fn status(&self) -> ScenarioStatus {
        self.status.lock().clone()
    }

    /// Session-installed scenarios (not in the YAML).
    #[must_use]
    pub fn session_scenarios(&self) -> Vec<ScenarioSpec> {
        self.session.lock().values().cloned().collect()
    }

    /// Look up a session copy by id.
    #[must_use]
    pub fn session_scenario(&self, id: &str) -> Option<ScenarioSpec> {
        self.session.lock().get(id).cloned()
    }

    /// Install or replace a session copy. Returns the previous copy if any.
    pub fn install_session(&self, spec: ScenarioSpec) -> Option<ScenarioSpec> {
        self.session.lock().insert(spec.id.clone(), spec)
    }

    /// Remove a session copy. Returns whether it existed.
    pub fn remove_session(&self, id: &str) -> bool {
        self.session.lock().remove(id).is_some()
    }

    /// Whether this playback generation should keep running.
    #[must_use]
    pub fn is_running(&self, generation: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == generation
            && self.status.lock().state == ScenarioState::Running
    }

    /// Begin a scenario. Returns the generation token the playback task must use.
    pub fn begin(&self, scenario: &ScenarioSpec) -> u64 {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let mut status = self.status.lock();
        *status = ScenarioStatus {
            state: ScenarioState::Running,
            scenario_name: Some(scenario.run_id().to_owned()),
            step_index: 0,
            total_steps: scenario.steps.len(),
            elapsed_s: 0.0,
        };
        generation
    }

    /// Record a completed step.
    pub fn note_step(&self, generation: u64, index: usize, elapsed_s: f64) {
        if self.generation.load(Ordering::SeqCst) != generation {
            return;
        }
        let mut status = self.status.lock();
        status.step_index = index;
        status.elapsed_s = elapsed_s;
    }

    /// Mark completed.
    pub fn mark_completed(&self, generation: u64, elapsed_s: f64) {
        if self.generation.load(Ordering::SeqCst) != generation {
            return;
        }
        let mut status = self.status.lock();
        status.state = ScenarioState::Completed;
        status.elapsed_s = elapsed_s;
    }

    /// Mark stopped (the async task is cancelled by the caller).
    pub fn mark_stopped(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        let mut status = self.status.lock();
        status.state = ScenarioState::Stopped;
    }
}
