"""Fault injection types.

Faults are short-lived overrides applied by the SimulationEngine on each tick.
They expire automatically when their `remaining_s` reaches zero.
The fault API is exposed via REST endpoints in the simulation router.
"""

from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum


class FaultType(StrEnum):
    """Types of faults that can be injected."""

    spike = "spike"  # Force a register to an extreme value for a duration
    freeze = "freeze"  # Stop updating a register (stuck sensor)
    dropout = "dropout"  # Set register to 0 (loss of signal)
    alarm = "alarm"  # Force a named coil to True, bypassing trigger evaluation
    noise_amplify = "noise_amplify"  # Multiply noise std_dev by a factor


@dataclass
class ActiveFault:
    """An active fault currently affecting the simulation."""

    fault_type: FaultType
    register_name: str | None  # None means device-wide (e.g. dropout)
    value: float | None  # spike target, noise factor, etc.
    duration_s: float
    remaining_s: float
