"""Simulation control endpoints — faults and tick rate.

POST   /faults              → inject a fault
GET    /faults              → list active faults
DELETE /faults              → clear all active faults
PATCH  /simulation          → update tick interval (live, no restart needed)
"""

from __future__ import annotations

import structlog
from fastapi import APIRouter, Request, status

from simbus.api.schemas import (
    ActiveFaultResponse,
    FaultRequest,
    SimulationPatchRequest,
)
from simbus.simulation.faults import ActiveFault, FaultType

router = APIRouter()
logger = structlog.get_logger(__name__)


@router.post(
    "/faults",
    status_code=status.HTTP_202_ACCEPTED,
    summary="Inject a fault into the running simulation",
)
async def inject_fault(body: FaultRequest, request: Request) -> dict[str, str]:
    """Inject a fault. Expires automatically after `duration_s` seconds."""
    engine = request.app.state.engine
    fault = ActiveFault(
        fault_type=FaultType(body.fault_type),
        register_name=body.register_name,
        value=body.value,
        duration_s=body.duration_s,
        remaining_s=body.duration_s,
    )
    engine.inject_fault(fault)
    logger.info(
        "fault injected",
        source="api",
        fault_type=body.fault_type,
        register_name=body.register_name,
        value=body.value,
        duration_s=body.duration_s,
    )
    return {"status": "accepted", "fault_type": body.fault_type}


@router.get(
    "/faults",
    response_model=list[ActiveFaultResponse],
    summary="List active faults",
)
async def list_faults(request: Request) -> list[ActiveFaultResponse]:
    """Return all currently active faults with their remaining duration."""
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
    active_faults = len(request.app.state.engine._faults)
    request.app.state.engine.clear_faults()
    logger.info("faults cleared", source="api", cleared_count=active_faults)


@router.patch(
    "/simulation",
    summary="Update simulation parameters",
)
async def patch_simulation(body: SimulationPatchRequest, request: Request) -> dict[str, object]:
    """Update simulation parameters. Changes take effect on the next tick without restarting."""
    engine = request.app.state.engine
    if body.tick_interval is not None:
        old_tick_interval = engine.tick_interval
        engine.tick_interval = body.tick_interval
        logger.info(
            "simulation tick interval updated",
            source="api",
            old_tick_interval=old_tick_interval,
            new_tick_interval=engine.tick_interval,
        )
    return {"tick_interval": engine.tick_interval}


@router.post(
    "/simulation/reset",
    status_code=status.HTTP_204_NO_CONTENT,
    summary="Reset all registers to YAML defaults and clear faults",
)
async def reset_simulation(request: Request) -> None:
    """Reset all registers to YAML defaults, clear faults, and rewind simulation time. Engine keeps running."""
    request.app.state.engine.reset()
    logger.info("simulation reset", source="api")
