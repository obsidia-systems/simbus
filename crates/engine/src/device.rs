//! Single-device simulation state.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use rand::Rng;
use rand::SeedableRng;
use rand::rngs::StdRng;
use spec::{
    BehaviorSpec, CoilSpec, DeviceSpec, FaultType, RegisterSpace, RegisterSpec, ScenarioStep,
    TriggerCondition,
};
use tracing::info;

use crate::bank::{RegisterBank, Snapshot};
use crate::behaviors;
use crate::encode::{raw_to_real, real_to_raw};
use crate::faults::ActiveFault;

/// Device-level errors.
#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    /// Unknown register address.
    #[error("{space:?} register {address} not found")]
    UnknownRegister {
        /// Space.
        space: RegisterSpace,
        /// Address.
        address: u16,
    },
    /// Unknown register name.
    #[error("register '{0}' not found")]
    UnknownRegisterName(String),
    /// Unknown coil or discrete name.
    #[error("coil '{0}' not found")]
    UnknownCoil(String),
    /// Unknown coil address (field-plane write).
    #[error("coil address {address} not found")]
    UnknownCoilAddress {
        /// Coil address.
        address: u16,
    },
}

/// Public view of an active fault.
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveFaultView {
    /// Kind as YAML string.
    pub fault_type: FaultType,
    /// Target name.
    pub register_name: Option<String>,
    /// Optional value.
    pub value: Option<f64>,
    /// Original duration.
    pub duration_s: f64,
    /// Remaining duration.
    pub remaining_s: f64,
}

#[derive(Clone)]
struct RegState {
    base: f64,
    elapsed_s: f64,
    /// Per-register offset so periodic behaviors are not locked to t=0.
    phase_s: f64,
}

struct Inner {
    bank: RegisterBank,
    holding_state: HashMap<u16, RegState>,
    input_state: HashMap<u16, RegState>,
    /// Boot copy so `reset` replays the same seeded trace.
    initial_holding: HashMap<u16, RegState>,
    initial_input: HashMap<u16, RegState>,
    faults: HashMap<String, ActiveFault>,
    rng: StdRng,
    initial_rng: StdRng,
    tick_interval: f64,
    running: bool,
}

/// One virtual field device.
pub struct Device {
    spec: DeviceSpec,
    inner: RwLock<Inner>,
}

impl Device {
    /// Build a device from a validated spec.
    #[must_use]
    pub fn new(spec: DeviceSpec, seed: Option<u64>, tick_interval: f64) -> Arc<Self> {
        let bank = RegisterBank::from_spec(&spec);
        let mut rng = match seed {
            Some(s) => StdRng::seed_from_u64(mix_seed(s, &spec)),
            None => StdRng::from_os_rng(),
        };
        let holding_state: HashMap<u16, RegState> = spec
            .registers
            .holding
            .iter()
            .map(|r| (r.address, init_reg_state(r, &mut rng)))
            .collect();
        let input_state: HashMap<u16, RegState> = spec
            .registers
            .input
            .iter()
            .map(|r| (r.address, init_reg_state(r, &mut rng)))
            .collect();
        Arc::new(Self {
            spec,
            inner: RwLock::new(Inner {
                bank,
                holding_state: holding_state.clone(),
                input_state: input_state.clone(),
                initial_holding: holding_state,
                initial_input: input_state,
                faults: HashMap::new(),
                initial_rng: rng.clone(),
                rng,
                tick_interval,
                running: false,
            }),
        })
    }

    /// Immutable spec.
    #[must_use]
    pub fn spec(&self) -> &DeviceSpec {
        &self.spec
    }

    /// Current tick interval in seconds.
    #[must_use]
    pub fn tick_interval(&self) -> f64 {
        self.inner.read().tick_interval
    }

    /// Update tick interval (takes effect next wait).
    pub fn set_tick_interval(&self, tick_interval: f64) {
        if tick_interval > 0.0 {
            self.inner.write().tick_interval = tick_interval;
        }
    }

    /// Mark the simulation as running.
    pub fn set_running(&self, running: bool) {
        self.inner.write().running = running;
    }

    /// Whether the tick loop is marked running.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.inner.read().running
    }

    /// Advance simulation by `dt` seconds. `dt <= 0` is a no-op.
    pub fn tick(&self, dt: f64) -> Snapshot {
        let mut inner = self.inner.write();
        if dt <= 0.0 {
            return inner.bank.snapshot();
        }
        tick_faults(&mut inner.faults, dt);
        {
            let Inner {
                bank,
                holding_state,
                input_state,
                faults,
                rng,
                ..
            } = &mut *inner;
            tick_space(
                &self.spec.registers.holding,
                RegisterSpace::Holding,
                dt,
                holding_state,
                bank,
                faults,
                rng,
            );
            tick_space(
                &self.spec.registers.input,
                RegisterSpace::Input,
                dt,
                input_state,
                bank,
                faults,
                rng,
            );
            evaluate_alarms(&self.spec, faults, bank);
        }
        inner.bank.snapshot()
    }

    /// Snapshot without ticking.
    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        self.inner.read().bank.snapshot()
    }

    /// Read Modbus words.
    #[must_use]
    pub fn read_words(&self, space: RegisterSpace, address: u16, count: u16) -> Vec<u16> {
        self.inner.read().bank.read_words(space, address, count)
    }

    /// Write Modbus words and shift `state.base` for every cell written.
    pub fn write_words(
        &self,
        space: RegisterSpace,
        address: u16,
        words: &[u16],
        source: &str,
    ) -> Result<(), DeviceError> {
        let mut inner = self.inner.write();
        let written = inner
            .bank
            .write_words(space, address, words)
            .map_err(|address| DeviceError::UnknownRegister { space, address })?;
        for addr in written {
            if let Some(cell) = inner.bank.get_cell(space, addr) {
                let scale = inner.bank.scale(space, addr).unwrap_or(1);
                update_base_locked(&mut inner, space, addr, raw_to_real(cell, scale), source);
            }
        }
        Ok(())
    }

    /// Read coils.
    #[must_use]
    pub fn read_coils(&self, address: u16, count: u16) -> Vec<bool> {
        self.inner.read().bank.read_coils(address, count)
    }

    /// Write coils. Every address in the range must exist.
    pub fn write_coils(&self, address: u16, values: &[bool]) -> Result<(), DeviceError> {
        self.inner
            .write()
            .bank
            .write_coils(address, values)
            .map_err(|address| DeviceError::UnknownCoilAddress { address })
    }

    /// Read discrete inputs.
    #[must_use]
    pub fn read_discrete(&self, address: u16, count: u16) -> Vec<bool> {
        self.inner.read().bank.read_discrete(address, count)
    }

    /// Override a register from the control API.
    pub fn override_register(
        &self,
        space: RegisterSpace,
        address: u16,
        raw: Option<u16>,
        real: Option<f64>,
        source: &str,
    ) -> Result<(u16, f64), DeviceError> {
        let spec_reg = find_register(&self.spec, space, address)
            .ok_or(DeviceError::UnknownRegister { space, address })?;
        let cell = if let Some(real) = real {
            real_to_raw(real, spec_reg.scale, spec_reg.data_type)
        } else if let Some(raw) = raw {
            real_to_raw(
                f64::from(raw) / f64::from(spec_reg.scale.max(1)),
                spec_reg.scale,
                spec_reg.data_type,
            )
        } else {
            real_to_raw(spec_reg.default, spec_reg.scale, spec_reg.data_type)
        };
        let mut inner = self.inner.write();
        inner.bank.set_cell(space, address, cell);
        let real_now = raw_to_real(cell, spec_reg.scale);
        update_base_locked(&mut inner, space, address, real_now, source);
        let word = inner.bank.read_words(space, address, 1)[0];
        Ok((word, real_now))
    }

    /// Override a coil.
    pub fn override_coil(&self, address: u16, value: bool) -> Result<bool, DeviceError> {
        let mut inner = self.inner.write();
        if !inner.bank.has_coil(address) {
            return Err(DeviceError::UnknownRegister {
                space: RegisterSpace::Holding,
                address,
            });
        }
        inner.bank.set_coil(address, value);
        Ok(value)
    }

    /// Override a discrete input.
    pub fn override_discrete(&self, address: u16, value: bool) -> Result<bool, DeviceError> {
        let mut inner = self.inner.write();
        if !inner.bank.has_discrete(address) {
            return Err(DeviceError::UnknownRegister {
                space: RegisterSpace::Input,
                address,
            });
        }
        inner.bank.set_discrete(address, value);
        Ok(value)
    }

    /// Inject a fault. Freeze latches the current real value at inject time.
    pub fn inject_fault(&self, mut fault: ActiveFault) {
        let mut inner = self.inner.write();
        if fault.fault_type == FaultType::Freeze {
            if let Some(name) = fault.register_name.as_deref() {
                if let Some((space, reg)) = find_numeric_by_name(&self.spec, name) {
                    if let Some(cell) = inner.bank.get_cell(space, reg.address) {
                        fault.value = Some(raw_to_real(cell, reg.scale));
                    }
                }
            }
        }
        info!(
            source = "api",
            fault_type = ?fault.fault_type,
            register_name = fault.register_name.as_deref(),
            duration_s = fault.duration_s,
            "fault injected"
        );
        inner.faults.insert(fault.key(), fault);
    }

    /// Active faults.
    #[must_use]
    pub fn faults(&self) -> Vec<ActiveFaultView> {
        self.inner
            .read()
            .faults
            .values()
            .map(|f| ActiveFaultView {
                fault_type: f.fault_type,
                register_name: f.register_name.clone(),
                value: f.value,
                duration_s: f.duration_s,
                remaining_s: f.remaining_s,
            })
            .collect()
    }

    /// Clear faults.
    pub fn clear_faults(&self) {
        self.inner.write().faults.clear();
        info!(source = "api", "faults cleared");
    }

    /// Reset to boot `RegState`, YAML defaults in the bank, and no faults.
    pub fn reset(&self) {
        let mut inner = self.inner.write();
        inner.bank.reset_from_spec(&self.spec);
        inner.faults.clear();
        let holding = inner.initial_holding.clone();
        let input = inner.initial_input.clone();
        let rng = inner.initial_rng.clone();
        inner.holding_state = holding;
        inner.input_state = input;
        inner.rng = rng;
        info!(source = "api", "simulation reset");
    }

    /// Apply one scenario step immediately.
    pub fn apply_step(&self, step: &ScenarioStep) {
        match step {
            ScenarioStep::SetRegister(s) => {
                let space = if s.register_type == "input" {
                    RegisterSpace::Input
                } else {
                    RegisterSpace::Holding
                };
                let Some(reg) = find_register_by_name(&self.spec, space, &s.register_name) else {
                    tracing::warn!(register = %s.register_name, "scenario step skipped: unknown register");
                    return;
                };
                let _ = self.override_register(space, reg.address, None, Some(s.value), "scenario");
            }
            ScenarioStep::InjectFault(s) => {
                self.inject_fault(ActiveFault::new(
                    s.fault_type,
                    s.register_name.clone(),
                    s.value,
                    s.duration_s,
                ));
            }
            ScenarioStep::SetCoil(s) => {
                if let Some(coil) = self.spec.registers.coils.iter().find(|c| c.name == s.coil) {
                    let _ = self.override_coil(coil.address, s.value);
                } else if let Some(disc) = self
                    .spec
                    .registers
                    .discrete
                    .iter()
                    .find(|c| c.name == s.coil)
                {
                    let _ = self.override_discrete(disc.address, s.value);
                } else {
                    tracing::warn!(coil = %s.coil, "scenario step skipped: unknown coil/discrete");
                }
            }
            ScenarioStep::SetTickInterval(s) => {
                self.set_tick_interval(s.tick_interval);
            }
        }
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// Mix identity into `--seed` so two documents with the same numeric seed
/// do not replay the same noise/phase sequence.
fn mix_seed(seed: u64, spec: &DeviceSpec) -> u64 {
    seed ^ fnv1a64(spec.name.as_bytes())
        ^ fnv1a64(spec.device_type.as_bytes()).rotate_left(32)
        ^ fnv1a64(spec.identity.vendor.as_bytes()).rotate_left(16)
        ^ fnv1a64(spec.identity.product.as_bytes()).rotate_left(48)
        ^ fnv1a64(spec.identity.revision.as_bytes()).rotate_left(8)
}

fn init_reg_state(reg: &RegisterSpec, rng: &mut StdRng) -> RegState {
    let phase_s = match &reg.simulation {
        Some(BehaviorSpec::Sinusoidal { period_hours, .. }) => {
            rng.random_range(0.0..(period_hours * 3600.0).max(1.0))
        }
        Some(BehaviorSpec::Sawtooth { period_seconds, .. }) => {
            rng.random_range(0.0..period_seconds.max(1.0))
        }
        Some(BehaviorSpec::Step { .. }) => rng.random_range(0.0..60.0),
        _ => 0.0,
    };
    let base = match &reg.simulation {
        Some(BehaviorSpec::GaussianNoise { std_dev, .. }) => {
            behaviors::gaussian_noise(reg.default, *std_dev, rng)
        }
        Some(BehaviorSpec::Drift { bounds, .. }) => {
            let span = ((bounds.1 - bounds.0).abs() * 0.02).max(0.01);
            behaviors::gaussian_noise(reg.default, span, rng).clamp(bounds.0, bounds.1)
        }
        _ => reg.default,
    };
    RegState {
        base,
        elapsed_s: 0.0,
        phase_s,
    }
}

fn find_register(spec: &DeviceSpec, space: RegisterSpace, address: u16) -> Option<&RegisterSpec> {
    let regs = match space {
        RegisterSpace::Holding => &spec.registers.holding,
        RegisterSpace::Input => &spec.registers.input,
    };
    regs.iter().find(|r| r.address == address)
}

fn find_numeric_by_name<'a>(
    spec: &'a DeviceSpec,
    name: &str,
) -> Option<(RegisterSpace, &'a RegisterSpec)> {
    find_register_by_name(spec, RegisterSpace::Holding, name)
        .map(|reg| (RegisterSpace::Holding, reg))
        .or_else(|| {
            find_register_by_name(spec, RegisterSpace::Input, name)
                .map(|reg| (RegisterSpace::Input, reg))
        })
}

fn find_register_by_name<'a>(
    spec: &'a DeviceSpec,
    space: RegisterSpace,
    name: &str,
) -> Option<&'a RegisterSpec> {
    let regs = match space {
        RegisterSpace::Holding => &spec.registers.holding,
        RegisterSpace::Input => &spec.registers.input,
    };
    regs.iter().find(|r| r.name == name)
}

fn update_base_locked(
    inner: &mut Inner,
    space: RegisterSpace,
    address: u16,
    new_base: f64,
    source: &str,
) {
    let state = match space {
        RegisterSpace::Holding => inner.holding_state.get_mut(&address),
        RegisterSpace::Input => inner.input_state.get_mut(&address),
    };
    if let Some(state) = state {
        let old_base = state.base;
        state.base = new_base;
        info!(
            source,
            address, old_base, new_base, "simulation base changed"
        );
    }
}

fn tick_faults(faults: &mut HashMap<String, ActiveFault>, dt: f64) {
    for fault in faults.values_mut() {
        fault.remaining_s -= dt;
    }
    faults.retain(|_, f| {
        if f.remaining_s <= 0.0 {
            info!(
                source = "simulation",
                fault_type = ?f.fault_type,
                register_name = f.register_name.as_deref(),
                "fault expired"
            );
            false
        } else {
            true
        }
    });
}

fn tick_space(
    registers: &[RegisterSpec],
    space: RegisterSpace,
    dt: f64,
    states: &mut HashMap<u16, RegState>,
    bank: &mut RegisterBank,
    faults: &HashMap<String, ActiveFault>,
    rng: &mut StdRng,
) {
    for reg in registers {
        let Some(state) = states.get_mut(&reg.address) else {
            continue;
        };
        state.elapsed_s += dt;
        let Some(sim) = &reg.simulation else {
            continue;
        };
        let mut new_val = compute(reg.default, sim, state, dt, rng);

        let fault = faults.get(&reg.name).or_else(|| faults.get("_device"));
        if let Some(fault) = fault {
            match fault.fault_type {
                FaultType::Spike => {
                    if let Some(v) = fault.value {
                        new_val = v;
                    }
                }
                FaultType::Freeze => {
                    if let Some(v) = fault.value {
                        new_val = v;
                    } else if let Some(cell) = bank.get_cell(space, reg.address) {
                        new_val = raw_to_real(cell, reg.scale);
                    }
                }
                FaultType::Dropout => new_val = 0.0,
                FaultType::NoiseAmplify => {
                    let factor = fault.value.unwrap_or(10.0);
                    let std_dev = match sim {
                        BehaviorSpec::GaussianNoise { std_dev, .. } => *std_dev,
                        _ => 0.5,
                    };
                    new_val = behaviors::gaussian_noise(new_val, std_dev * factor, rng);
                }
                FaultType::Alarm => {}
            }
        }

        bank.set_cell(
            space,
            reg.address,
            real_to_raw(new_val, reg.scale, reg.data_type),
        );
    }
}

fn compute(
    default: f64,
    spec: &BehaviorSpec,
    state: &mut RegState,
    dt: f64,
    rng: &mut StdRng,
) -> f64 {
    match spec {
        BehaviorSpec::Constant => behaviors::constant(state.base),
        BehaviorSpec::GaussianNoise { std_dev, drift } => {
            if let Some(drift) = drift.as_ref().filter(|d| d.enabled) {
                state.base = behaviors::drift_step(state.base, drift.rate * dt, drift.bounds);
            }
            behaviors::gaussian_noise(state.base, *std_dev, rng)
        }
        BehaviorSpec::Sinusoidal {
            period_hours,
            amplitude,
            drift,
        } => {
            if let Some(drift) = drift.as_ref().filter(|d| d.enabled) {
                state.base = behaviors::drift_step(state.base, drift.rate * dt, drift.bounds);
            }
            behaviors::sinusoidal(
                state.base,
                *amplitude,
                *period_hours,
                state.elapsed_s + state.phase_s,
            )
        }
        BehaviorSpec::Drift { rate, bounds } => {
            state.base = behaviors::drift_step(state.base, *rate * dt, *bounds);
            state.base
        }
        BehaviorSpec::Sawtooth {
            period_seconds,
            min,
            max,
        } => behaviors::sawtooth(*period_seconds, *min, *max, state.elapsed_s + state.phase_s),
        BehaviorSpec::Step { steps } => {
            behaviors::step_value(default, steps, state.elapsed_s + state.phase_s)
        }
    }
}

fn evaluate_alarms(
    spec: &DeviceSpec,
    faults: &HashMap<String, ActiveFault>,
    bank: &mut RegisterBank,
) {
    let holding_by_name: HashMap<&str, &RegisterSpec> = spec
        .registers
        .holding
        .iter()
        .map(|r| (r.name.as_str(), r))
        .collect();
    let input_by_name: HashMap<&str, &RegisterSpec> = spec
        .registers
        .input
        .iter()
        .map(|r| (r.name.as_str(), r))
        .collect();

    for coil in &spec.registers.coils {
        if let Some(fault) = faults.get(&coil.name) {
            if fault.fault_type == FaultType::Alarm {
                let previous = bank.get_coil(coil.address);
                bank.set_coil(coil.address, true);
                if !previous {
                    info!(source = "fault", alarm_name = %coil.name, "alarm activated");
                }
                continue;
            }
        }
        apply_trigger(coil, true, &holding_by_name, &input_by_name, bank);
    }
    for disc in &spec.registers.discrete {
        apply_trigger(disc, false, &holding_by_name, &input_by_name, bank);
    }
}

fn apply_trigger(
    coil: &CoilSpec,
    is_coil: bool,
    holding_by_name: &HashMap<&str, &RegisterSpec>,
    input_by_name: &HashMap<&str, &RegisterSpec>,
    bank: &mut RegisterBank,
) {
    let Some(trigger) = &coil.trigger else {
        return;
    };
    let (space, source) = if let Some(reg) = input_by_name.get(trigger.source_register.as_str()) {
        (RegisterSpace::Input, *reg)
    } else if let Some(reg) = holding_by_name.get(trigger.source_register.as_str()) {
        (RegisterSpace::Holding, *reg)
    } else {
        return;
    };
    let Some(cell) = bank.get_cell(space, source.address) else {
        return;
    };
    let scaled = raw_to_real(cell, source.scale);
    let triggered = check_condition(scaled, trigger.condition, trigger.threshold, source.scale);
    if is_coil {
        let previous = bank.get_coil(coil.address);
        bank.set_coil(coil.address, triggered);
        if previous != triggered {
            if triggered {
                info!(
                    source = "simulation",
                    alarm_name = %coil.name,
                    value = scaled,
                    "alarm activated"
                );
            } else {
                info!(
                    source = "simulation",
                    alarm_name = %coil.name,
                    value = scaled,
                    "alarm cleared"
                );
            }
        }
    } else {
        let previous = bank.get_discrete(coil.address);
        bank.set_discrete(coil.address, triggered);
        if previous != triggered {
            info!(
                source = "simulation",
                discrete_name = %coil.name,
                value = scaled,
                "discrete changed"
            );
        }
    }
}

fn check_condition(value: f64, condition: TriggerCondition, threshold: f64, scale: u32) -> bool {
    match condition {
        TriggerCondition::Gt => value > threshold,
        TriggerCondition::Lt => value < threshold,
        TriggerCondition::Eq => {
            let lsb = 0.5 / f64::from(scale.max(1));
            (value - threshold).abs() <= lsb
        }
        TriggerCondition::Gte => value >= threshold,
        TriggerCondition::Lte => value <= threshold,
    }
}
