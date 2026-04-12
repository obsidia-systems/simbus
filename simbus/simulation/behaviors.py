"""Pure simulation behavior functions.

All functions are stateless — the SimulationEngine tracks per-register
mutable state (base value, elapsed time) and passes it in as arguments.

This makes every behavior independently unit-testable without any
asyncio infrastructure or device context.
"""

from __future__ import annotations

import math
from random import Random

from simbus.config.schema import StepEntry

# ---------------------------------------------------------------------------
# Core behaviors
# ---------------------------------------------------------------------------


def constant(default: float) -> float:
    """Value is fixed; never changes."""
    return default


def gaussian_noise(base: float, std_dev: float, rng: Random) -> float:
    """Random Gaussian noise centered on `base`.

    On each tick the result oscillates around `base` with the given
    standard deviation. The `base` itself may be shifted by a drift
    modifier managed by the engine.
    """
    return base + rng.gauss(0.0, std_dev)


def sinusoidal(
    center: float,
    amplitude: float,
    period_hours: float,
    elapsed_s: float,
) -> float:
    """Sinusoidal oscillation around `center`.

    Args:
        center:       The midpoint value (may be shifted by drift).
        amplitude:    Peak deviation above/below center.
        period_hours: Full cycle duration in hours.
        elapsed_s:    Total elapsed simulation time in seconds.
    """
    period_s = period_hours * 3600.0
    return center + amplitude * math.sin(2.0 * math.pi * elapsed_s / period_s)


def drift_step(current: float, rate: float, bounds: tuple[float, float]) -> float:
    """Advance a drifting value by `rate` for one tick, clamped to `bounds`.

    Args:
        current: Current base value.
        rate:    Change per tick. Negative value = downward drift.
        bounds:  (min, max) hard limits.
    """
    low, high = bounds
    return max(low, min(high, current + rate))


def sawtooth(
    period_s: float,
    min_val: float,
    max_val: float,
    elapsed_s: float,
) -> float:
    """Repeating linear ramp from `min_val` to `max_val` over `period_s`.

    Resets to `min_val` at the start of each period.
    """
    progress = (elapsed_s % period_s) / period_s
    return min_val + (max_val - min_val) * progress


def step_value(
    default: float,
    steps: list[StepEntry],
    elapsed_s: float,
) -> float:
    """Discrete step changes at scheduled simulation times.

    Returns `default` until the first step threshold is reached, then
    holds each step's value until the next one.
    """
    value = default
    for entry in sorted(steps, key=lambda s: s.at):
        if elapsed_s >= entry.at:
            value = entry.value
    return value


# ---------------------------------------------------------------------------
# Scaling helpers (shared by engine and API layer)
# ---------------------------------------------------------------------------


def scale_to_raw(value: float, scale: int) -> int:
    """Convert a real-world float to a scaled uint16 register integer.

    Example: 22.5 °C, scale=10 → 225
    """
    return int(round(value * scale)) & 0xFFFF


def raw_to_scaled(raw: int, scale: int) -> float:
    """Convert a raw register integer back to a real-world float.

    Example: raw=225, scale=10 → 22.5
    """
    return raw / scale
