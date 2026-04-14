"""Device status and config endpoints."""

from __future__ import annotations

from fastapi import APIRouter, Request

from simbus.api.schemas import (
    CoilInfoResponse,
    ConfigResponse,
    RegisterInfoResponse,
    RegisterMapResponse,
    StatusResponse,
)

router = APIRouter()


@router.get("/status", response_model=StatusResponse, summary="Device status and health")
async def get_status(request: Request) -> StatusResponse:
    cfg = request.app.state.config
    engine = request.app.state.engine
    server = request.app.state.server
    return StatusResponse(
        name=cfg.name,
        type=cfg.type,
        modbus_port=server.port,
        tick_interval=engine.tick_interval,
        simulation="running" if engine._running else "stopped",
        modbus_server=server.status,
    )


@router.get("/config", response_model=ConfigResponse, summary="Full device register map and metadata")
async def get_config(request: Request) -> ConfigResponse:
    cfg = request.app.state.config

    def _reg(r) -> RegisterInfoResponse:  # type: ignore[no-untyped-def]
        return RegisterInfoResponse(
            address=r.address,
            name=r.name,
            description=r.description,
            unit=r.unit,
            scale=r.scale,
            data_type=r.data_type,
            default=r.default,
            behavior=r.simulation.behavior if r.simulation else None,
        )

    def _coil(c) -> CoilInfoResponse:  # type: ignore[no-untyped-def]
        return CoilInfoResponse(
            address=c.address,
            name=c.name,
            description=c.description,
            default=c.default,
        )

    return ConfigResponse(
        name=cfg.name,
        version=cfg.version,
        type=cfg.type,
        description=cfg.description,
        modbus_port=cfg.modbus.default_port,
        unit_id=cfg.modbus.unit_id,
        endianness=cfg.modbus.endianness,
        registers=RegisterMapResponse(
            holding=[_reg(r) for r in cfg.registers.holding],
            input=[_reg(r) for r in cfg.registers.input],
            coils=[_coil(c) for c in cfg.registers.coils],
            discrete=[_coil(c) for c in cfg.registers.discrete],
        ),
    )
