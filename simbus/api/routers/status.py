"""Device status endpoint."""

from __future__ import annotations

from fastapi import APIRouter, Request

from simbus.api.schemas import StatusResponse

router = APIRouter()


@router.get("/status", response_model=StatusResponse, summary="Device status and health")
async def get_status(request: Request) -> StatusResponse:
    cfg = request.app.state.config
    settings = request.app.state.settings
    engine = request.app.state.engine
    server = request.app.state.server
    return StatusResponse(
        name=cfg.name,
        type=cfg.type,
        modbus_port=settings.modbus_port,
        tick_interval=engine.tick_interval,
        simulation="running" if engine._running else "stopped",
        modbus_server=server.status,
    )
