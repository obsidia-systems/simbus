"""Simulation control endpoints — faults and tick rate.

POST   /faults              → inject a fault
GET    /faults              → list active faults
DELETE /faults              → clear all active faults
PATCH  /simulation          → update tick interval (live, no restart needed)
"""

from __future__ import annotations

from fastapi import APIRouter, Request, status

from simbus.api.schemas import (
    ActiveFaultResponse,
    FaultRequest,
    SimulationPatchRequest,
)
from simbus.simulation.faults import ActiveFault, FaultType

router = APIRouter()


@router.post(
    "/faults",
    status_code=status.HTTP_202_ACCEPTED,
    summary="Inject a fault into the running simulation",
)
async def inject_fault(body: FaultRequest, request: Request) -> dict[str, str]:
    engine = request.app.state.engine
    fault = ActiveFault(
        fault_type=FaultType(body.fault_type),
        register_name=body.register_name,
        value=body.value,
        duration_s=body.duration_s,
        remaining_s=body.duration_s,
    )
    engine.inject_fault(fault)
    return {"status": "accepted", "fault_type": body.fault_type}


@router.get(
    "/faults",
    response_model=list[ActiveFaultResponse],
    summary="List active faults",
)
async def list_faults(request: Request) -> list[ActiveFaultResponse]:
    engine = request.app.state.engine
    return [
        ActiveFaultResponse(
            fault_type=f.fault_type,
            register_name=f.register_name,
            value=f.value,
            duration_s=f.duration_s,
            remaining_s=f.remaining_s,
        )
        for f in engine._faults.values()
    ]


@router.delete(
    "/faults",
    status_code=status.HTTP_204_NO_CONTENT,
    summary="Clear all active faults",
)
async def clear_faults(request: Request) -> None:
    request.app.state.engine.clear_faults()


@router.patch(
    "/simulation",
    summary="Update simulation parameters",
)
async def patch_simulation(body: SimulationPatchRequest, request: Request) -> dict[str, object]:
    settings = request.app.state.settings
    if body.tick_interval is not None:
        settings.tick_interval = body.tick_interval
    return {"tick_interval": settings.tick_interval}
