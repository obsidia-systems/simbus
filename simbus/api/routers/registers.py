"""Register read/write + SSE streaming endpoints.

GET    /registers                  → snapshot of all register values
PATCH  /registers/{addr}           → set holding register (raw int or real-world float)
PATCH  /registers/input/{addr}     → set input register  (raw int or real-world float)
PATCH  /registers/coils/{addr}     → set coil (true / false)
PATCH  /registers/discrete/{addr}  → set discrete input (true / false)
GET    /registers/stream           → SSE live register updates (text/event-stream)

Value formats for numeric registers:
  {"value": 270}          — raw uint16 (Modbus wire format), scale must be known by caller
  {"real_value": 27.0}    — physical units (°C, %RH, V…), API applies the register scale

  Both update the simulation base so noise/drift continues from the new value.
  Example: PATCH {"real_value": 27.0} on temperature (scale=10) writes raw 270 and
  sets state.base=27.0 → gaussian_noise now oscillates around 27.0 °C.

SSE design:
  Each connection creates an asyncio.Queue and appends it to engine.sse_queues.
  The SimulationEngine pushes a JSON snapshot on every tick. The generator yields
  SSE frames until the client disconnects, then removes the queue from the list.
"""

from __future__ import annotations

import asyncio
from typing import AsyncGenerator

from fastapi import APIRouter, HTTPException, Request, status
from fastapi.responses import StreamingResponse

from simbus.api.schemas import CoilOverrideRequest, ErrorResponse, RegisterOverrideRequest, RegisterSnapshotResponse
from simbus.simulation.behaviors import scale_to_raw

router = APIRouter()


def _to_raw(body: RegisterOverrideRequest, scale: int) -> int:
    """Resolve a RegisterOverrideRequest to a raw uint16 integer.

    If ``real_value`` is set the physical value is converted using *scale*
    (``raw = round(real_value × scale) & 0xFFFF``), which correctly encodes
    negative values as two's-complement uint16 for int16 registers.
    If ``value`` (raw) is set it is returned unchanged.
    """
    if body.real_value is not None:
        return scale_to_raw(body.real_value, scale)
    return body.value  # type: ignore[return-value]


@router.get(
    "",
    response_model=RegisterSnapshotResponse,
    summary="Get current register snapshot",
)
async def get_registers(request: Request) -> RegisterSnapshotResponse:
    snap = request.app.state.store.snapshot()
    return RegisterSnapshotResponse(
        holding=snap.holding,
        input=snap.input,
        coils=snap.coils,
        discrete=snap.discrete,
    )


@router.patch(
    "/{address}",
    responses={404: {"model": ErrorResponse}},
    summary="Override a holding register value",
)
async def override_register(
    address: int,
    body: RegisterOverrideRequest,
    request: Request,
) -> dict[str, object]:
    """Set a holding register and update the simulation operating point.

    Accepts either a raw uint16 integer (``value``) or a real-world physical
    value (``real_value``). The simulation continues from the new operating
    point — behaviors like gaussian_noise and sinusoidal oscillate around the
    new base instead of snapping back to the YAML default.
    """
    store = request.app.state.store
    cfg = request.app.state.config
    reg = next((r for r in cfg.registers.holding if r.address == address), None)
    if reg is None:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Holding register {address} not found on this device",
        )
    raw = _to_raw(body, reg.scale)
    store.set_holding(address, raw)
    request.app.state.engine.update_base(address, raw)
    return {"address": address, "raw_value": raw, "real_value": raw / reg.scale}


@router.patch(
    "/input/{address}",
    responses={404: {"model": ErrorResponse}},
    summary="Override an input register value",
)
async def override_input_register(
    address: int,
    body: RegisterOverrideRequest,
    request: Request,
) -> dict[str, object]:
    """Set an input register and update the simulation operating point.

    Input registers are read-only for Modbus clients (FC4), but the simulation
    API can write them directly. Accepts raw uint16 (``value``) or real-world
    float (``real_value``).
    """
    store = request.app.state.store
    cfg = request.app.state.config
    reg = next((r for r in cfg.registers.input if r.address == address), None)
    if reg is None:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Input register {address} not found on this device",
        )
    raw = _to_raw(body, reg.scale)
    store.set_input(address, raw)
    request.app.state.engine.update_base(address, raw)
    return {"address": address, "raw_value": raw, "real_value": raw / reg.scale}


@router.patch(
    "/coils/{address}",
    responses={404: {"model": ErrorResponse}},
    summary="Override a coil value",
)
async def override_coil(
    address: int,
    body: CoilOverrideRequest,
    request: Request,
) -> dict[str, object]:
    """Set a coil state directly.

    For coils with a trigger condition the value takes effect immediately but
    will be re-evaluated on the next engine tick. Use this for static coils
    (no trigger) or to test momentary state changes.
    """
    store = request.app.state.store
    if address not in store.coils_raw:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Coil {address} not found on this device",
        )
    store.set_coil(address, body.value)
    return {"address": address, "value": body.value}


@router.patch(
    "/discrete/{address}",
    responses={404: {"model": ErrorResponse}},
    summary="Override a discrete input value",
)
async def override_discrete(
    address: int,
    body: CoilOverrideRequest,
    request: Request,
) -> dict[str, object]:
    """Set a discrete input state directly.

    Discrete inputs are read-only for Modbus clients (FC2), but the simulation
    API can write them directly. For discrete inputs with a trigger condition the
    value will be re-evaluated on the next engine tick.
    """
    store = request.app.state.store
    if address not in store.discrete_raw:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Discrete input {address} not found on this device",
        )
    store.set_discrete(address, body.value)
    return {"address": address, "value": body.value}


@router.get(
    "/stream",
    summary="SSE stream of live register snapshots",
    response_class=StreamingResponse,
)
async def stream_registers(request: Request) -> StreamingResponse:
    """Server-Sent Events — one JSON frame per simulation tick."""
    engine = request.app.state.engine
    queue: asyncio.Queue[str] = asyncio.Queue(maxsize=100)
    engine.sse_queues.append(queue)

    return StreamingResponse(
        _sse_generator(queue, engine.sse_queues),
        media_type="text/event-stream",
        headers={
            "Cache-Control": "no-cache",
            "X-Accel-Buffering": "no",  # disable nginx buffering
        },
    )


async def _sse_generator(
    queue: asyncio.Queue[str],
    sse_queues: list[asyncio.Queue[str]],
) -> AsyncGenerator[str, None]:
    try:
        while True:
            payload = await queue.get()
            yield f"data: {payload}\n\n"
    finally:
        if queue in sse_queues:
            sse_queues.remove(queue)
