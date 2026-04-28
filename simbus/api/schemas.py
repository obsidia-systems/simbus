"""Pydantic request/response schemas for the device control API."""

from __future__ import annotations

from typing import Literal

from pydantic import BaseModel, Field, model_validator

# ---------------------------------------------------------------------------
# Status
# ---------------------------------------------------------------------------


class StatusResponse(BaseModel):
    name: str
    type: str
    modbus_port: int
    tick_interval: float
    simulation: Literal["running", "stopped"] = "running"
    modbus_server: Literal["listening", "stopped", "error"] = "stopped"


# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------


class RegisterInfoResponse(BaseModel):
    address: int
    name: str
    description: str
    unit: str
    scale: int
    data_type: str
    default: float
    behavior: str | None = None


class CoilInfoResponse(BaseModel):
    address: int
    name: str
    description: str
    default: bool


class RegisterMapResponse(BaseModel):
    holding: list[RegisterInfoResponse]
    input: list[RegisterInfoResponse]
    coils: list[CoilInfoResponse]
    discrete: list[CoilInfoResponse]


class ConfigResponse(BaseModel):
    name: str
    version: str
    type: str
    description: str
    modbus_port: int
    unit_id: int
    endianness: str
    registers: RegisterMapResponse


# ---------------------------------------------------------------------------
# Registers
# ---------------------------------------------------------------------------


class RegisterSnapshotResponse(BaseModel):
    holding: dict[int, int]
    input: dict[int, int]
    coils: dict[int, bool]
    discrete: dict[int, bool]


class RegisterOverrideRequest(BaseModel):
    value: int | None = Field(
        default=None,
        ge=0,
        le=65535,
        description="Raw register value (uint16, 0–65535). Use this when you know the Modbus wire value.",
    )
    real_value: float | None = Field(
        default=None,
        description=(
            "Real-world value in physical units (e.g. 27.0 for 27.0 °C). "
            "The API applies the register's scale automatically. "
            "Mutually exclusive with 'value'."
        ),
    )

    @model_validator(mode="after")
    def _exactly_one_field(self) -> RegisterOverrideRequest:
        if self.value is None and self.real_value is None:
            raise ValueError("Provide 'value' (raw uint16) or 'real_value' (physical units).")
        if self.value is not None and self.real_value is not None:
            raise ValueError("'value' and 'real_value' are mutually exclusive.")
        return self


class CoilOverrideRequest(BaseModel):
    value: bool = Field(description="Coil state to set (true / false)")


# ---------------------------------------------------------------------------
# Faults
# ---------------------------------------------------------------------------


class FaultRequest(BaseModel):
    fault_type: Literal["spike", "freeze", "dropout", "alarm", "noise_amplify"]
    register_name: str | None = Field(
        default=None,
        description="Target register name. Omit for device-wide faults (dropout).",
    )
    value: float | None = Field(
        default=None,
        description="Spike target value or noise amplification factor.",
    )
    duration_s: float = Field(default=30.0, gt=0, description="Fault duration in seconds")


class ActiveFaultResponse(BaseModel):
    fault_type: str
    register_name: str | None
    value: float | None
    duration_s: float
    remaining_s: float


# ---------------------------------------------------------------------------
# Simulation
# ---------------------------------------------------------------------------


class SimulationPatchRequest(BaseModel):
    tick_interval: float | None = Field(default=None, gt=0)


# ---------------------------------------------------------------------------
# Errors
# ---------------------------------------------------------------------------


class ErrorResponse(BaseModel):
    detail: str
