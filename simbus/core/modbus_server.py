"""pymodbus 3.12.x async TCP server — one instance per virtual device.

Architecture:
  Four custom DataBlock subclasses bridge the pymodbus request/response
  cycle to our RegisterStore. Because pymodbus runs entirely within the
  same asyncio event loop as the simulation engine, and getValues/setValues
  are synchronous (no awaits), reads and writes are cooperative-safe with no
  locking needed.

  ModbusDeviceContext
    ├── _HoldingBlock  (fc=3, fc=6/16 writes)  → store.holding
    ├── _InputBlock    (fc=4, read-only)         → store.input
    ├── _CoilBlock     (fc=1, fc=5/15 writes)   → store.coils
    └── _DiscreteBlock (fc=2, read-only)         → store.discrete
"""

from __future__ import annotations

import structlog
from pymodbus.datastore import ModbusDeviceContext, ModbusServerContext
from pymodbus.datastore.store import BaseModbusDataBlock
from pymodbus.server import ModbusTcpServer

from simbus.core.store import RegisterStore

logger = structlog.get_logger(__name__)


# ---------------------------------------------------------------------------
# Custom DataBlocks — bridge pymodbus ↔ RegisterStore
# ---------------------------------------------------------------------------


def _addr(address: int) -> int:
    """Compensate for ModbusDeviceContext's internal +1 offset.

    ModbusDeviceContext.getValues / setValues always increments the address
    by 1 before calling the DataBlock (Modbus 1-based register convention).
    Our RegisterStore uses 0-based addressing matching the YAML definitions,
    so we subtract 1 here to keep them aligned.

        Modbus PDU address 0  →  DeviceContext calls block(1)  →  store[0]
        Modbus PDU address 1  →  DeviceContext calls block(2)  →  store[1]
    """
    return address - 1


class _HoldingBlock(BaseModbusDataBlock):
    """Holding registers (FC3 read / FC6, FC16 write)."""

    def __init__(self, store: RegisterStore) -> None:
        self._store = store

    def validate(self, address: int, count: int = 1) -> bool:
        """Always accept the request and let the store handle out-of-range addresses."""
        return True

    def getValues(self, address: int, count: int = 1) -> list[int]:
        base = _addr(address)
        return [self._store.get_holding(base + i) for i in range(count)]

    def setValues(self, address: int, values: list[int]) -> None:
        base = _addr(address)
        for i, v in enumerate(values):
            self._store.set_holding(base + i, int(v))

    def reset(self) -> None:
        pass


class _InputBlock(BaseModbusDataBlock):
    """Input registers (FC4 read-only)."""

    def __init__(self, store: RegisterStore) -> None:
        self._store = store

    def validate(self, address: int, count: int = 1) -> bool:
        """Always accept the request and let the store handle out-of-range addresses."""
        return True

    def getValues(self, address: int, count: int = 1) -> list[int]:
        base = _addr(address)
        return [self._store.get_input(base + i) for i in range(count)]

    def setValues(self, address: int, values: list[int]) -> None:
        pass  # read-only from Modbus client perspective

    def reset(self) -> None:
        pass


class _CoilBlock(BaseModbusDataBlock):
    """Coils (FC1 read / FC5, FC15 write)."""

    def __init__(self, store: RegisterStore) -> None:
        self._store = store

    def validate(self, address: int, count: int = 1) -> bool:
        """Always accept the request and let the store handle out-of-range addresses."""
        return True

    def getValues(self, address: int, count: int = 1) -> list[bool]:
        base = _addr(address)
        return [self._store.get_coil(base + i) for i in range(count)]

    def setValues(self, address: int, values: list[bool]) -> None:
        base = _addr(address)
        for i, v in enumerate(values):
            self._store.set_coil(base + i, bool(v))

    def reset(self) -> None:
        pass


class _DiscreteBlock(BaseModbusDataBlock):
    """Discrete inputs (FC2 read-only)."""

    def __init__(self, store: RegisterStore) -> None:
        self._store = store

    def validate(self, address: int, count: int = 1) -> bool:
        """Always accept the request and let the store handle out-of-range addresses."""
        return True

    def getValues(self, address: int, count: int = 1) -> list[bool]:
        base = _addr(address)
        return [self._store.get_discrete(base + i) for i in range(count)]

    def setValues(self, address: int, values: list[bool]) -> None:
        pass  # read-only

    def reset(self) -> None:
        pass


# ---------------------------------------------------------------------------
# Server instance
# ---------------------------------------------------------------------------


class ModbusServerInstance:
    """Wraps a pymodbus TCP server for a single virtual Modbus device.

    Usage:
        server = ModbusServerInstance(store, port=5020, unit_id=1)
        task = asyncio.create_task(server.serve_forever())
        # ... later ...
        await server.stop()
        task.cancel()
    """

    def __init__(self, store: RegisterStore, port: int, unit_id: int) -> None:
        self._store = store
        self._port = port
        self._unit_id = unit_id
        self._server: ModbusTcpServer | None = None
        self._status: str = "stopped"  # "stopped" | "listening" | "error"

    async def serve_forever(self) -> None:
        """Build the pymodbus server and run until cancelled."""
        device_ctx = ModbusDeviceContext(
            hr=_HoldingBlock(self._store),
            ir=_InputBlock(self._store),
            co=_CoilBlock(self._store),
            di=_DiscreteBlock(self._store),
        )
        server_ctx = ModbusServerContext(devices=device_ctx, single=True)

        self._server = ModbusTcpServer(
            context=server_ctx,
            address=("0.0.0.0", self._port),
        )

        self._status = "listening"
        logger.info("modbus server listening",
                    port=self._port, unit_id=self._unit_id)
        try:
            await self._server.serve_forever()
        except Exception:
            self._status = "error"
            raise
        finally:
            self._status = "stopped"

    async def stop(self) -> None:
        """Shutdown the Modbus TCP server gracefully."""
        if self._server is not None:
            await self._server.shutdown()
            self._server = None
            self._status = "stopped"
            logger.info("modbus server stopped", port=self._port)

    @property
    def status(self) -> str:
        """Current status: 'stopped', 'listening', or 'error'."""
        return self._status

    @property
    def port(self) -> int:
        """TCP port number the server is listening on (or was configured to listen on)."""
        return self._port

    @property
    def unit_id(self) -> int:
        """Unit ID of the Modbus device."""
        return self._unit_id
