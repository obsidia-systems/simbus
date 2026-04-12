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
from dataclasses import dataclass
from random import Random

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
    ) -> None:
        self._store = store
        self._config = config
        self._rng = Random(seed)
        self._running = False

        # Per-register simulation state keyed by address
        self._state: dict[int, _RegState] = {
            reg.address: _RegState(base=reg.default)
            for reg in config.registers.holding + config.registers.input
        }

        # Active faults keyed by register name (or "_device" for dropout)
        self._faults: dict[str, ActiveFault] = {}

        # SSE subscriber queues — populated by the API layer
        self.sse_queues: list[asyncio.Queue[str]] = []

    # -----------------------------------------------------------------------
    # Public API
    # -----------------------------------------------------------------------

    async def run(self, tick_interval: float = 1.0) -> None:
        """Run the tick loop until stop() is called."""
        self._running = True
        while self._running:
            self._tick(tick_interval)
            self._publish_snapshot()
            await asyncio.sleep(tick_interval)

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

    def tick_faults(self, dt: float) -> None:
        """Decrement fault timers; remove expired faults."""
        for f in self._faults.values():
            f.remaining_s -= dt
        expired = [key for key, f in self._faults.items()
                   if f.remaining_s <= 0]
        for key in expired:
            del self._faults[key]

    # -----------------------------------------------------------------------
    # Internal tick
    # -----------------------------------------------------------------------

    def _tick(self, dt: float) -> None:
        self.tick_faults(dt)

        _reg_map = {r.name: r for r in self._config.registers.holding}

        for reg in self._config.registers.holding:
            state = self._state[reg.address]
            state.elapsed_s += dt

            if reg.simulation is None:
                continue

            new_val = self._compute(reg.default, reg.simulation, state)

            # Apply faults that override the computed value
            fault = self._faults.get(reg.name) or self._faults.get("_device")
            if fault:
                match fault.fault_type:
                    case FaultType.spike | FaultType.alarm:
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

            self._store.set_holding(
                reg.address, behaviors.scale_to_raw(new_val, reg.scale))

        self._evaluate_alarms()

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
                return default

            case GaussianNoiseBehavior():
                if cfg.drift and cfg.drift.enabled:
                    state.base = behaviors.drift_step(
                        state.base, cfg.drift.rate, cfg.drift.bounds
                    )
                return behaviors.gaussian_noise(state.base, cfg.std_dev, self._rng)

            case SinusoidalBehavior():
                center = default
                if cfg.drift and cfg.drift.enabled:
                    state.base = behaviors.drift_step(
                        state.base, cfg.drift.rate, cfg.drift.bounds
                    )
                    center = state.base
                return behaviors.sinusoidal(center, cfg.amplitude, cfg.period_hours, state.elapsed_s)

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
        """Update coil states based on holding register values and trigger conditions."""
        reg_by_name = {
            r.name: r
            for r in self._config.registers.holding + self._config.registers.input
        }

        for coil in self._config.registers.coils + self._config.registers.discrete:
            if coil.trigger is None:
                continue

            source = reg_by_name.get(coil.trigger.source_register)
            if source is None:
                continue

            raw = self._store.get_holding(source.address)
            scaled = behaviors.raw_to_scaled(raw, source.scale)
            triggered = _check_condition(
                scaled, coil.trigger.condition, coil.trigger.threshold
            )
            self._store.set_coil(coil.address, triggered)

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


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _check_condition(value: float, condition: TriggerCondition, threshold: float) -> bool:
    match condition:
        case TriggerCondition.gt:
            return value > threshold
        case TriggerCondition.lt:
            return value < threshold
        case TriggerCondition.eq:
            return value == threshold
        case TriggerCondition.gte:
            return value >= threshold
        case TriggerCondition.lte:
            return value <= threshold
    return False
