"""Pydantic v2 models for scenario YAML files.

A scenario is a timed sequence of simulation events that can be replayed
against a running device to test alarm pipelines, SCADA logic, or
operator training scenarios.
"""

from __future__ import annotations

from typing import Annotated, Literal

from pydantic import BaseModel, Field

from simbus.simulation.faults import FaultType

# ---------------------------------------------------------------------------
# Steps — discriminated union on "action"
# ---------------------------------------------------------------------------


class SetRegisterStep(BaseModel):
    """Set a holding or input register to a specific real-world value."""

    action: Literal["set_register"]
    at: float = Field(ge=0, description="Seconds from scenario start")
    register_name: str = Field(description="Register name (must exist on the device)")
    value: float = Field(description="Real-world value (scaled automatically)")
    register_type: Literal["holding", "input"] = "holding"


class InjectFaultStep(BaseModel):
    """Inject a fault that expires automatically after duration_s."""

    action: Literal["inject_fault"]
    at: float = Field(ge=0, description="Seconds from scenario start")
    fault_type: FaultType
    register_name: str | None = Field(
        default=None,
        description="Target register or coil name (None for device-wide dropout)",
    )
    value: float | None = Field(
        default=None,
        description="Spike target value or noise amplification factor",
    )
    duration_s: float = Field(default=30.0, gt=0, description="Fault duration in seconds")


class SetCoilStep(BaseModel):
    """Force a coil or discrete input to a boolean state."""

    action: Literal["set_coil"]
    at: float = Field(ge=0, description="Seconds from scenario start")
    coil: str = Field(description="Coil or discrete input name")
    value: bool = Field(description="Target boolean state")


class SetTickIntervalStep(BaseModel):
    """Change the simulation tick interval mid-scenario."""

    action: Literal["set_tick_interval"]
    at: float = Field(ge=0, description="Seconds from scenario start")
    tick_interval: float = Field(gt=0, description="New tick interval in seconds")


StepConfig = Annotated[
    SetRegisterStep | InjectFaultStep | SetCoilStep | SetTickIntervalStep,
    Field(discriminator="action"),
]


# ---------------------------------------------------------------------------
# Top-level scenario config
# ---------------------------------------------------------------------------


class ScenarioConfig(BaseModel):
    """Top-level configuration for a timed simulation scenario."""

    name: str
    description: str = ""
    steps: list[StepConfig] = Field(min_length=1)
