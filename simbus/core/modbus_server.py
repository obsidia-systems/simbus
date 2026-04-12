"""pymodbus async TCP server — one instance per virtual device.

Phase 2 implementation. The server wraps a pymodbus ModbusTcpServer,
bridging its synchronous DataBlock interface to our async RegisterStore.

Architecture:
  - A custom ModbusBaseDataBlock subclass reads/writes directly to the
    RegisterStore's raw dicts (no asyncio overhead — same event loop).
  - The server runs as an asyncio Task via serve_forever().
  - Stopping cancels the task and calls server.shutdown().
"""

from __future__ import annotations

import asyncio

from simbus.core.store import RegisterStore


class ModbusServerInstance:
    """Wraps a pymodbus TCP server for a single virtual Modbus device."""

    def __init__(self, store: RegisterStore, port: int, unit_id: int) -> None:
        self._store = store
        self._port = port
        self._unit_id = unit_id
        self._server: object | None = None
        self._task: asyncio.Task[None] | None = None

    async def start(self) -> asyncio.Task[None]:
        """Start the Modbus TCP server and return its running Task.

        TODO: Phase 2
          - Build ModbusSlaveContext using StoreBackedDataBlock
          - Construct ModbusTcpServer(context, address=("0.0.0.0", self._port))
          - Create and return asyncio.create_task(server.serve_forever())
        """
        raise NotImplementedError("Phase 2")

    async def stop(self) -> None:
        """Stop the Modbus TCP server.

        TODO: Phase 2
          - Cancel the task
          - Call server.shutdown()
        """
        raise NotImplementedError("Phase 2")

    @property
    def port(self) -> int:
        return self._port

    @property
    def unit_id(self) -> int:
        return self._unit_id
