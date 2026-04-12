"""Device status endpoint."""

from __future__ import annotations

from fastapi import APIRouter, Request

from simbus.api.schemas import StatusResponse

router = APIRouter()


@router.get("/status", response_model=StatusResponse, summary="Device status")
async def get_status(request: Request) -> StatusResponse:
    cfg = request.app.state.config
    settings = request.app.state.settings
    return StatusResponse(
        name=cfg.name,
        type=cfg.type,
        modbus_port=settings.modbus_port,
        tick_interval=settings.tick_interval,
    )
