"""Simulation tick loop for a single virtual device.

The engine runs as an asyncio Task. On every tick it:
  1. Advances per-register state (base value, elapsed time).
  2. Applies the configured behavior to compute the new register value.
  3. Applies any active faults that override the computed value.
  4. Writes the scaled raw value to the RegisterStore.
  5. Evaluates alarm triggers and updates coils.
  6. Publishes a snapshot to all active SSE subscriber queues.
"""

from __future__ import annotations

import asyncio
import json
import time
from dataclasses import dataclass
from random import Random

import structlog
from simbus.config.schema import (
    ConstantBehavior,
    DeviceConfig,
    DriftBehavior,
    GaussianNoiseBehavior,
    SawtoothBehavior,
    SinusoidalBehavior,
    StepBehavior,
    TriggerCondition,
)
from simbus.core.store import RegisterStore
from simbus.simulation import behaviors
from simbus.simulation.faults import ActiveFault, FaultType

logger = structlog.get_logger(__name__)


@dataclass
class _RegState:
    """Mutable simulation state for a single register."""

    base: float          # current mean/center value (shifts with drift)
    elapsed_s: float = 0.0   # total elapsed simulation time in seconds


class SimulationEngine:
    """Async tick loop that drives register values for one device."""

    def __init__(
        self,
        store: RegisterStore,
        config: DeviceConfig,
        seed: int | None = None,
        tick_interval: float = 1.0,
        tick_health_log_interval: float = 60.0,
    ) -> None:
        self._store = store
        self._config = config
        self._rng = Random(seed)
        self._running = False

        # Mutable — updated live via PATCH /simulation
        self.tick_interval: float = tick_interval
        self.tick_health_log_interval: float = tick_health_log_interval

        # Per-register simulation state keyed by address
        self._state: dict[int, _RegState] = {
            reg.address: _RegState(base=reg.default)
            for reg in config.registers.holding + config.registers.input
        }

        # Active faults keyed by register name (or "_device" for dropout)
        self._faults: dict[str, ActiveFault] = {}

        # SSE subscriber queues — populated by the API layer
        self.sse_queues: list[asyncio.Queue[str]] = []

        self._started_monotonic: float | None = None
        self._next_health_log_at: float | None = None

    # -----------------------------------------------------------------------
    # Public API
    # -----------------------------------------------------------------------

    async def run(self) -> None:
        """Run the tick loop until stop() is called.

        Reads self.tick_interval on every iteration so that live updates
        via PATCH /simulation take effect without restarting the engine.
        """
        self._running = True
        self._started_monotonic = time.monotonic()
        self._next_health_log_at = self._started_monotonic + self.tick_health_log_interval
        while self._running:
            dt = self.tick_interval
            tick_started = time.monotonic()
            self._tick(dt)
            self._publish_snapshot()
            tick_finished = time.monotonic()
            tick_duration_ms = (tick_finished - tick_started) * 1000.0
            loop_drift_ms = max(0.0, (tick_finished - tick_started - dt) * 1000.0)
            self._log_tick_health_if_due(
                now=tick_finished,
                tick_duration_ms=tick_duration_ms,
                loop_drift_ms=loop_drift_ms,
            )
            await asyncio.sleep(dt)

    def stop(self) -> None:
        """Signal the tick loop to stop after the current iteration."""
        self._running = False

    def inject_fault(self, fault: ActiveFault) -> None:
        """Inject a fault that will affect register values on the next tick."""
        key = fault.register_name or "_device"
        self._faults[key] = fault

    def clear_faults(self) -> None:
        """Clear all active faults immediately."""
        self._faults.clear()

    def reset(self) -> None:
        """Reset all registers to YAML defaults and clear all faults.

        The simulation continues running — only values and state are rewound.
        """
        self._store.initialize(self._config.registers)
        self._faults.clear()
        for reg in self._config.registers.holding + self._config.registers.input:
            self._state[reg.address].base = reg.default
            self._state[reg.address].elapsed_s = 0.0

    def update_base(self, address: int, raw_value: int, source: str = "unknown") -> None:
        """Update simulation base for a holding or input register from a raw store value.

        Converts raw → real using the register's scale so the simulation
        continues from the new operating point on the next tick.
        Example: raw=270, scale=10 → base=27.0 → noise oscillates around 27.0°C.

        Called by:
          - PATCH /registers/{address}        (holding registers, via REST)
          - PATCH /registers/input/{address}  (input registers, via REST)
          - FC6/FC16 Modbus write             (holding registers, via Modbus client)
        """
        reg = next(
            (
                r
                for r in self._config.registers.holding + self._config.registers.input
                if r.address == address
            ),
            None,
        )
        if reg is not None and address in self._state:
            old_base = self._state[address].base
            new_base = raw_value / reg.scale
            self._state[address].base = new_base
            logger.info(
                "simulation base changed",
                source=source,
                address=address,
                register_name=reg.name,
                old_base=old_base,
                new_base=new_base,
            )

    def tick_faults(self, dt: float) -> None:
        """Decrement fault timers; remove expired faults."""
        for f in self._faults.values():
            f.remaining_s -= dt
        expired = [key for key, f in self._faults.items()
                   if f.remaining_s <= 0]
        for key in expired:
            fault = self._faults[key]
            del self._faults[key]
            logger.info(
                "fault expired",
                source="simulation",
                fault_type=fault.fault_type,
                register_name=fault.register_name,
            )

    # -----------------------------------------------------------------------
    # Internal tick
    # -----------------------------------------------------------------------

    def _tick(self, dt: float) -> None:
        self.tick_faults(dt)
        self._tick_registers(self._config.registers.holding, is_input=False, dt=dt)
        self._tick_registers(self._config.registers.input, is_input=True, dt=dt)
        self._evaluate_alarms()

    def _tick_registers(
        self,
        registers: list,  # list[RegisterConfig]
        is_input: bool,
        dt: float,
    ) -> None:
        for reg in registers:
            state = self._state[reg.address]
            state.elapsed_s += dt

            if reg.simulation is None:
                continue

            new_val = self._compute(reg.default, reg.simulation, state)

            # Faults only apply to holding registers (input registers are read-only
            # from the Modbus client perspective, so faults target holding only)
            if not is_input:
                fault = self._faults.get(reg.name) or self._faults.get("_device")
                if fault:
                    match fault.fault_type:
                        case FaultType.spike:
                            if fault.value is not None:
                                new_val = fault.value
                        case FaultType.freeze:
                            new_val = behaviors.raw_to_scaled(
                                self._store.get_holding(reg.address), reg.scale
                            )
                        case FaultType.dropout:
                            new_val = 0.0
                        case FaultType.noise_amplify:
                            amplified_std = (
                                getattr(reg.simulation, "std_dev", 0.5)
                                * (fault.value or 10.0)
                            )
                            new_val = behaviors.gaussian_noise(
                                new_val, amplified_std, self._rng)

            raw = behaviors.scale_to_raw(new_val, reg.scale)
            if is_input:
                self._store.set_input(reg.address, raw)
            else:
                self._store.set_holding(reg.address, raw)

    def _compute(
        self,
        default: float,
        cfg: ConstantBehavior
        | GaussianNoiseBehavior
        | SinusoidalBehavior
        | DriftBehavior
        | SawtoothBehavior
        | StepBehavior,
        state: _RegState,
    ) -> float:
        match cfg:
            case ConstantBehavior():
                # state.base is initialized from default; PATCH /registers updates it
                return state.base

            case GaussianNoiseBehavior():
                if cfg.drift and cfg.drift.enabled:
                    state.base = behaviors.drift_step(
                        state.base, cfg.drift.rate, cfg.drift.bounds
                    )
                return behaviors.gaussian_noise(state.base, cfg.std_dev, self._rng)

            case SinusoidalBehavior():
                # Use state.base as center (initialized from default, updatable via PATCH)
                if cfg.drift and cfg.drift.enabled:
                    state.base = behaviors.drift_step(
                        state.base, cfg.drift.rate, cfg.drift.bounds
                    )
                return behaviors.sinusoidal(state.base, cfg.amplitude, cfg.period_hours, state.elapsed_s)

            case DriftBehavior():
                state.base = behaviors.drift_step(
                    state.base, cfg.rate, cfg.bounds)
                return state.base

            case SawtoothBehavior():
                return behaviors.sawtooth(
                    cfg.period_seconds, cfg.min, cfg.max, state.elapsed_s
                )

            case StepBehavior():
                return behaviors.step_value(default, cfg.steps, state.elapsed_s)

        return default  # unreachable — exhaustive match

    def _evaluate_alarms(self) -> None:
        """Update coil and discrete states based on register values and trigger conditions.

        Trigger sources may be holding OR input registers — the correct store is chosen
        automatically. For coils, an active `alarm` fault targeting the coil by name
        forces it to True, bypassing the normal trigger evaluation.
        """
        holding_by_name = {r.name: r for r in self._config.registers.holding}
        input_by_name = {r.name: r for r in self._config.registers.input}
        reg_by_name = {**holding_by_name, **input_by_name}

        for coil in self._config.registers.coils:
            # alarm fault targeting this coil by name takes priority over trigger logic
            alarm_fault = self._faults.get(coil.name)
            if alarm_fault and alarm_fault.fault_type == FaultType.alarm:
                previous = self._store.get_coil(coil.address)
                self._store.set_coil(coil.address, True)
                if not previous:
                    logger.info(
                        "alarm activated",
                        source="fault",
                        alarm_name=coil.name,
                        trigger_type="forced_alarm_fault",
                    )
                continue

            if coil.trigger is None:
                continue

            source = reg_by_name.get(coil.trigger.source_register)
            if source is None:
                continue

            if coil.trigger.source_register in input_by_name:
                raw = self._store.get_input(source.address)
            else:
                raw = self._store.get_holding(source.address)
            scaled = behaviors.raw_to_scaled(raw, source.scale)
            triggered = _check_condition(
                scaled, coil.trigger.condition, coil.trigger.threshold
            )
            previous = self._store.get_coil(coil.address)
            self._store.set_coil(coil.address, triggered)
            if previous != triggered:
                logger.info(
                    "alarm activated" if triggered else "alarm cleared",
                    source="simulation",
                    alarm_name=coil.name,
                    source_register=coil.trigger.source_register,
                    value=scaled,
                    condition=coil.trigger.condition,
                    threshold=coil.trigger.threshold,
                )

        for disc in self._config.registers.discrete:
            if disc.trigger is None:
                continue

            source = reg_by_name.get(disc.trigger.source_register)
            if source is None:
                continue

            if disc.trigger.source_register in input_by_name:
                raw = self._store.get_input(source.address)
            else:
                raw = self._store.get_holding(source.address)
            scaled = behaviors.raw_to_scaled(raw, source.scale)
            triggered = _check_condition(
                scaled, disc.trigger.condition, disc.trigger.threshold
            )
            previous = self._store.get_discrete(disc.address)
            self._store.set_discrete(disc.address, triggered)
            if previous != triggered:
                logger.info(
                    "discrete changed",
                    source="simulation",
                    address=disc.address,
                    discrete_name=disc.name,
                    old_value=previous,
                    new_value=triggered,
                    source_register=disc.trigger.source_register,
                    value=scaled,
                    condition=disc.trigger.condition,
                    threshold=disc.trigger.threshold,
                )

    def _publish_snapshot(self) -> None:
        """Push a JSON snapshot to all active SSE subscriber queues."""
        if not self.sse_queues:
            return

        snap = self._store.snapshot()
        payload = json.dumps(
            {
                "holding": snap.holding,
                "input": snap.input,
                "coils": snap.coils,
                "discrete": snap.discrete,
            }
        )

        for q in self.sse_queues:
            if not q.full():
                q.put_nowait(payload)

    def _log_tick_health_if_due(
        self,
        now: float,
        tick_duration_ms: float,
        loop_drift_ms: float,
    ) -> None:
        """Emit a low-frequency health log for the simulation loop."""
        if self._next_health_log_at is None or self._started_monotonic is None:
            return
        if now < self._next_health_log_at:
            return

        logger.info(
            "simulation tick health",
            source="simulation",
            tick_interval=self.tick_interval,
            tick_duration_ms=round(tick_duration_ms, 3),
            loop_drift_ms=round(loop_drift_ms, 3),
            sse_subscribers=len(self.sse_queues),
            active_faults=len(self._faults),
            uptime_s=round(now - self._started_monotonic, 3),
        )
        self._next_health_log_at = now + self.tick_health_log_interval


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _check_condition(value: float, condition: TriggerCondition, threshold: float) -> bool:
    match condition:
        case TriggerCondition.GT:
            return value > threshold
        case TriggerCondition.LT:
            return value < threshold
        case TriggerCondition.EQ:
            return value == threshold
        case TriggerCondition.GTE:
            return value >= threshold
        case TriggerCondition.LTE:
            return value <= threshold
    return False
