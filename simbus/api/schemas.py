"""Pydantic request/response schemas for the device control API."""

from __future__ import annotations

from typing import Literal

from pydantic import BaseModel, Field


# ---------------------------------------------------------------------------
# Status
# ---------------------------------------------------------------------------


class StatusResponse(BaseModel):
    name: str
    type: str
    modbus_port: int
    tick_interval: float
    status: Literal["running", "fault"] = "running"


# ---------------------------------------------------------------------------
# Registers
# ---------------------------------------------------------------------------


class RegisterSnapshotResponse(BaseModel):
    holding: dict[int, int]
    input: dict[int, int]
    coils: dict[int, bool]
    discrete: dict[int, bool]


class RegisterOverrideRequest(BaseModel):
    value: int = Field(ge=0, le=65535, description="Raw uint16 register value")


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
