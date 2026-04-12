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
    big = "big"
    little = "little"
    big_swap = "big_swap"
    little_swap = "little_swap"


class DataType(StrEnum):
    uint16 = "uint16"
    int16 = "int16"
    uint32 = "uint32"
    float32 = "float32"


class TriggerCondition(StrEnum):
    gt = "gt"
    lt = "lt"
    eq = "eq"
    gte = "gte"
    lte = "lte"


class AlarmSeverity(StrEnum):
    info = "info"
    warning = "warning"
    critical = "critical"


# ---------------------------------------------------------------------------
# Behavior configs — discriminated union on "behavior" field
# ---------------------------------------------------------------------------


class DriftModifier(BaseModel):
    """Optional drift applied on top of another behavior (gaussian_noise, sinusoidal)."""

    enabled: bool = True
    rate: float
    bounds: tuple[float, float]

    @model_validator(mode="after")
    def _validate_bounds(self) -> DriftModifier:
        if self.bounds[0] >= self.bounds[1]:
            raise ValueError("drift bounds[0] must be less than bounds[1]")
        return self


class ConstantBehavior(BaseModel):
    behavior: Literal["constant"]


class GaussianNoiseBehavior(BaseModel):
    behavior: Literal["gaussian_noise"]
    std_dev: float = Field(gt=0, description="Standard deviation of the noise")
    drift: DriftModifier | None = None


class SinusoidalBehavior(BaseModel):
    behavior: Literal["sinusoidal"]
    period_hours: float = Field(gt=0, description="Oscillation period in hours")
    amplitude: float = Field(gt=0, description="Peak deviation from center")
    drift: DriftModifier | None = None


class DriftBehavior(BaseModel):
    behavior: Literal["drift"]
    rate: float = Field(description="Change per tick (negative = downward drift)")
    bounds: tuple[float, float]

    @model_validator(mode="after")
    def _validate_bounds(self) -> DriftBehavior:
        if self.bounds[0] >= self.bounds[1]:
            raise ValueError("drift bounds[0] must be less than bounds[1]")
        return self


class SawtoothBehavior(BaseModel):
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
    at: float = Field(ge=0, description="Seconds from simulation start")
    value: float


class StepBehavior(BaseModel):
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
    address: int = Field(ge=0, le=65535)
    name: str
    description: str = ""
    unit: str = ""
    default: float
    scale: int = Field(default=1, ge=1, description="raw = real_value × scale")
    data_type: DataType = DataType.uint16
    simulation: BehaviorConfig | None = None


class TriggerConfig(BaseModel):
    source_register: str = Field(description="Name of the holding/input register to watch")
    condition: TriggerCondition
    threshold: float


class CoilConfig(BaseModel):
    address: int = Field(ge=0, le=65535)
    name: str
    description: str = ""
    default: bool = False
    trigger: TriggerConfig | None = None


class RegisterMapConfig(BaseModel):
    holding: list[RegisterConfig] = []
    input: list[RegisterConfig] = []
    coils: list[CoilConfig] = []
    discrete: list[CoilConfig] = []


# ---------------------------------------------------------------------------
# Modbus config
# ---------------------------------------------------------------------------


class ModbusConfig(BaseModel):
    default_port: int = Field(ge=1024, le=65535)
    unit_id: int = Field(default=1, ge=1, le=247)
    endianness: Endianness = Endianness.big


# ---------------------------------------------------------------------------
# Alarms
# ---------------------------------------------------------------------------


class AlarmConfig(BaseModel):
    name: str
    severity: AlarmSeverity
    trigger: str = Field(description="Name of the coil that activates this alarm")


# ---------------------------------------------------------------------------
# Top-level device config
# ---------------------------------------------------------------------------


class DeviceConfig(BaseModel):
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
