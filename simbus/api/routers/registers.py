"""Register read/write + SSE streaming endpoints.

GET    /registers           → snapshot of all register values
PATCH  /registers/{addr}    → set a new operating point for a holding register
GET    /registers/stream    → SSE live register updates (text/event-stream)

PATCH behavior:
  Writes the raw value to the store AND updates the simulation base so that
  noise/drift continues from the new value instead of snapping back.
  Example: temperature default=22.5°C, PATCH value=270 (27.0°C) →
  gaussian_noise now oscillates around 27.0°C on subsequent ticks.

SSE design:
  Each connection creates an asyncio.Queue and appends it to
  engine.sse_queues. The SimulationEngine pushes a JSON snapshot on
  every tick. The generator yields SSE frames until the client disconnects,
  then removes the queue from the list.
"""

from __future__ import annotations

import asyncio
from typing import AsyncGenerator

from fastapi import APIRouter, HTTPException, Request, status
from fastapi.responses import StreamingResponse

from simbus.api.schemas import ErrorResponse, RegisterOverrideRequest, RegisterSnapshotResponse

router = APIRouter()


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
) -> dict[str, int]:
    store = request.app.state.store
    if address not in store.holding_raw:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Holding register {address} not found on this device",
        )
    store.set_holding(address, body.value)
    request.app.state.engine.update_base(address, body.value)
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
