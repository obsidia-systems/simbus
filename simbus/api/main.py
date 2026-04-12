"""FastAPI application factory for a single simbus device instance.

One process = one Modbus device + one REST API.

At startup the lifespan:
  1. Reads DeviceSettings (from app.state if set by CLI, else from env vars).
  2. Loads the DeviceConfig from a built-in type or YAML file.
  3. Initializes RegisterStore with default values.
  4. Starts SimulationEngine as an asyncio Task.
  5. (Phase 2) Starts ModbusServerInstance as an asyncio Task.

All routers access device state via request.app.state.
"""

from __future__ import annotations

import asyncio
from contextlib import asynccontextmanager, suppress
from typing import AsyncGenerator

import structlog
from fastapi import FastAPI

from simbus.config.loader import load_builtin, load_from_file
from simbus.core.modbus_server import ModbusServerInstance
from simbus.core.store import RegisterStore
from simbus.settings import DeviceSettings
from simbus.simulation.engine import SimulationEngine

logger = structlog.get_logger(__name__)


@asynccontextmanager
async def lifespan(app: FastAPI) -> AsyncGenerator[None, None]:
    # Settings injected by CLI; fall back to env vars for direct uvicorn launch
    if not hasattr(app.state, "settings"):
        app.state.settings = DeviceSettings()

    settings: DeviceSettings = app.state.settings

    # --- Load device config (yaml_path takes precedence over device_type) ---
    if settings.yaml_path is not None:
        cfg = load_from_file(settings.yaml_path)
    else:
        cfg = load_builtin(settings.device_type or "generic-tnh-sensor")

    if settings.device_name:
        cfg = cfg.model_copy(update={"name": settings.device_name})

    # --- Initialize state ---
    store = RegisterStore()
    store.initialize(cfg.registers)
    engine = SimulationEngine(store=store, config=cfg, seed=settings.seed)

    server = ModbusServerInstance(
        store=store,
        port=settings.modbus_port,
        unit_id=cfg.modbus.unit_id,
    )

    app.state.config = cfg
    app.state.store = store
    app.state.engine = engine
    app.state.server = server

    # --- Start tasks ---
    server_task = asyncio.create_task(server.serve_forever(), name="modbus-server")
    engine_task = asyncio.create_task(
        engine.run(tick_interval=settings.tick_interval),
        name="simulation-engine",
    )

    logger.info(
        "simbus started",
        device=cfg.name,
        type=cfg.type,
        modbus_port=settings.modbus_port,
        tick_interval=settings.tick_interval,
    )

    yield

    # --- Graceful shutdown ---
    engine.stop()
    engine_task.cancel()
    await server.stop()
    server_task.cancel()
    for task in (engine_task, server_task):
        with suppress(asyncio.CancelledError):
            await task

    logger.info("simbus stopped", device=cfg.name)


def create_app(settings: DeviceSettings | None = None) -> FastAPI:
    """Create the FastAPI application.

    Args:
        settings: Pre-built settings (CLI / tests).
                  If None, settings are read from env vars at startup.
    """
    from simbus.api.routers import registers, simulation, status

    _app = FastAPI(
        title="simbus",
        version="0.0.1",
        description="Industrial Field Device Simulator — device control API",
        lifespan=lifespan,
    )

    if settings is not None:
        _app.state.settings = settings

    _app.include_router(status.router, tags=["status"])
    _app.include_router(registers.router, prefix="/registers", tags=["registers"])
    _app.include_router(simulation.router, tags=["simulation"])

    return _app


# Module-level instance for `uvicorn simbus.api.main:app`
app = create_app()
