"""Pydantic v2 models for the device YAML schema.

A DeviceConfig fully describes a virtual Modbus device:
register map, default values, simulation behaviors, and alarm triggers.
"""

from __future__ import annotations

from enum import StrEnum
from typing import Annotated, Literal

from pydantic import BaseModel, Field, model_validator

# ---------------------------------------------------------------------------
# Enumerations
# ---------------------------------------------------------------------------


class Endianness(StrEnum):
    """Modbus register byte order. "big" is the most common, but some devices use "little"."""
    BIG = "big"
    LITTLE = "little"
    BIG_SWAP = "big_swap"
    LITTLE_SWAP = "little_swap"


class DataType(StrEnum):
    """Data types for registers. Determines how raw register values are interpreted."""
    UINT16 = "uint16"
    INT16 = "int16"
    UINT32 = "uint32"
    FLOAT32 = "float32"


class TriggerCondition(StrEnum):
    """Condition for activating a coil trigger or alarm."""
    GT = "gt"
    LT = "lt"
    EQ = "eq"
    GTE = "gte"
    LTE = "lte"


class AlarmSeverity(StrEnum):
    """Severity levels for alarms."""
    INFO = "info"
    WARNING = "warning"
    CRITICAL = "critical"


# ---------------------------------------------------------------------------
# Behavior configs — discriminated union on "behavior" field
# ---------------------------------------------------------------------------


class DriftModifier(BaseModel):
    """Optional drift applied on top of another behavior(gaussian_noise, sinusoidal)."""

    enabled: bool = True
    rate: float
    bounds: tuple[float, float]

    @model_validator(mode="after")
    def _validate_bounds(self) -> DriftModifier:
        if self.bounds[0] >= self.bounds[1]:
            raise ValueError("drift bounds[0] must be less than bounds[1]")
        return self


class ConstantBehavior(BaseModel):
    """Behavior that always returns the same value (no "ticks" or time component)."""
    behavior: Literal["constant"]


class GaussianNoiseBehavior(BaseModel):
    """Behavior that adds normally distributed noise to a base value."""
    behavior: Literal["gaussian_noise"]
    std_dev: float = Field(gt=0, description="Standard deviation of the noise")
    drift: DriftModifier | None = None


class SinusoidalBehavior(BaseModel):
    """Behavior that simulates a sinusoidal waveform."""
    behavior: Literal["sinusoidal"]
    period_hours: float = Field(
        gt=0, description="Oscillation period in hours")
    amplitude: float = Field(gt=0, description="Peak deviation from center")
    drift: DriftModifier | None = None


class DriftBehavior(BaseModel):
    """Behavior that simulates a steady drift over time, with optional bounds."""
    behavior: Literal["drift"]
    rate: float = Field(
        description="Change per tick (negative = downward drift)")
    bounds: tuple[float, float]

    @model_validator(mode="after")
    def _validate_bounds(self) -> DriftBehavior:
        if self.bounds[0] >= self.bounds[1]:
            raise ValueError("drift bounds[0] must be less than bounds[1]")
        return self


class SawtoothBehavior(BaseModel):
    """Behavior that simulates a sawtooth waveform."""
    behavior: Literal["sawtooth"]
    period_seconds: float = Field(gt=0)
    min: float
    max: float

    @model_validator(mode="after")
    def _validate_range(self) -> SawtoothBehavior:
        if self.min >= self.max:
            raise ValueError("sawtooth min must be less than max")
        return self


class StepEntry(BaseModel):
    """Defines a single step change at a specific time."""
    at: float = Field(ge=0, description="Seconds from simulation start")
    value: float


class StepBehavior(BaseModel):
    """Behavior that simulates discrete step changes at specified times."""
    behavior: Literal["step"]
    steps: list[StepEntry] = Field(min_length=1)


BehaviorConfig = Annotated[
    ConstantBehavior
    | GaussianNoiseBehavior
    | SinusoidalBehavior
    | DriftBehavior
    | SawtoothBehavior
    | StepBehavior,
    Field(discriminator="behavior"),
]


# ---------------------------------------------------------------------------
# Register / Coil configs
# ---------------------------------------------------------------------------


class RegisterConfig(BaseModel):
    """Configuration for a single holding/input register."""
    address: int = Field(ge=0, le=65535)
    name: str
    description: str = ""
    unit: str = ""
    default: float
    scale: int = Field(default=1, ge=1, description="raw = real_value × scale")
    data_type: DataType = DataType.UINT16
    simulation: BehaviorConfig | None = None


class TriggerConfig(BaseModel):
    """Configuration for a coil trigger that activates based on a register value."""
    source_register: str = Field(
        description="Name of the holding/input register to watch")
    condition: TriggerCondition
    threshold: float


class CoilConfig(BaseModel):
    """Configuration for a single coil/discrete input."""
    address: int = Field(ge=0, le=65535)
    name: str
    description: str = ""
    default: bool = False
    trigger: TriggerConfig | None = None


class RegisterMapConfig(BaseModel):
    """Complete register map for the device, including holding registers, input registers, coils, and discrete inputs."""
    holding: list[RegisterConfig] = []
    input: list[RegisterConfig] = []
    coils: list[CoilConfig] = []
    discrete: list[CoilConfig] = []


# ---------------------------------------------------------------------------
# Modbus config
# ---------------------------------------------------------------------------


class ModbusConfig(BaseModel):
    """Configuration for the Modbus server, including communication parameters and defaults."""
    default_port: int = Field(ge=1024, le=65535)
    unit_id: int = Field(default=1, ge=1, le=247)
    endianness: Endianness = Endianness.BIG


# ---------------------------------------------------------------------------
# Alarms
# ---------------------------------------------------------------------------


class AlarmConfig(BaseModel):
    """Configuration for an alarm that is triggered by a coil and has a specified severity level."""
    name: str
    severity: AlarmSeverity
    trigger: str = Field(
        description="Name of the coil that activates this alarm")


# ---------------------------------------------------------------------------
# Top-level device config
# ---------------------------------------------------------------------------


class DeviceConfig(BaseModel):
    """Top-level configuration for a virtual Modbus device, including metadata, Modbus settings, register map, and alarms."""
    name: str
    version: str
    type: str
    description: str = ""
    modbus: ModbusConfig
    registers: RegisterMapConfig = RegisterMapConfig()
    alarms: list[AlarmConfig] = []

    @model_validator(mode="after")
    def _validate_references(self) -> DeviceConfig:
        reg_names = {
            r.name for r in self.registers.holding + self.registers.input
        }
        coil_names = {
            c.name for c in self.registers.coils + self.registers.discrete
        }

        for coil in self.registers.coils + self.registers.discrete:
            if coil.trigger and coil.trigger.source_register not in reg_names:
                raise ValueError(
                    f"Coil '{coil.name}': trigger references unknown register "
                    f"'{coil.trigger.source_register}'"
                )

        for alarm in self.alarms:
            if alarm.trigger not in coil_names:
                raise ValueError(
                    f"Alarm '{alarm.name}': references unknown coil '{alarm.trigger}'"
                )

        return self
