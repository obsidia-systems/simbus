"""Integration tests for ModbusServerInstance.

Starts a real pymodbus TCP server on a fixed port and connects a real
pymodbus async client to verify register reads/writes go through the
RegisterStore correctly.
"""

from __future__ import annotations

import asyncio
from contextlib import suppress

import pytest
from pymodbus.client import AsyncModbusTcpClient

from simbus.config.loader import load_builtin
from simbus.core.modbus_server import (
    ModbusServerInstance,
    _CoilBlock,
    _DiscreteBlock,
    _HoldingBlock,
    _InputBlock,
)
from simbus.core.store import RegisterStore

TEST_PORT = 19502
UNIT_ID = 1


@pytest.fixture
async def running_server():
    """Start a ModbusServerInstance backed by the T&H sensor config."""
    cfg = load_builtin("generic-tnh-sensor")
    store = RegisterStore()
    store.initialize(cfg.registers)

    server = ModbusServerInstance(store=store, port=TEST_PORT, unit_id=UNIT_ID)
    task = asyncio.create_task(server.serve_forever())
    await asyncio.sleep(0.15)  # give the server time to bind

    yield store, server

    await server.stop()
    task.cancel()
    with suppress(asyncio.CancelledError, Exception):
        await task


@pytest.fixture
async def client(running_server):  # noqa: ARG001 — fixture dep, side-effect only
    """Connected pymodbus async TCP client.

    Depends on running_server to ensure the server is up before connecting.
    """
    c = AsyncModbusTcpClient("127.0.0.1", port=TEST_PORT)
    await c.connect()
    yield c
    c.close()


class TestHoldingRegisters:
    async def test_read_default_temperature(self, running_server, client):
        store, _ = running_server
        result = await client.read_holding_registers(0, count=1, device_id=UNIT_ID)
        assert not result.isError()
        assert result.registers[0] == store.get_holding(0)  # 225 = 22.5°C × 10

    async def test_read_default_humidity(self, running_server, client):
        store, _ = running_server
        result = await client.read_holding_registers(1, count=1, device_id=UNIT_ID)
        assert not result.isError()
        assert result.registers[0] == store.get_holding(1)  # 450 = 45.0%RH × 10

    async def test_read_multiple_registers(self, running_server, client):
        store, _ = running_server
        result = await client.read_holding_registers(0, count=2, device_id=UNIT_ID)
        assert not result.isError()
        assert len(result.registers) == 2
        assert result.registers[0] == store.get_holding(0)
        assert result.registers[1] == store.get_holding(1)

    async def test_write_register_reflects_in_store(self, running_server, client):
        store, _ = running_server
        await client.write_register(0, 300, device_id=UNIT_ID)  # 30.0°C
        assert store.get_holding(0) == 300

    async def test_write_then_read_back(self, running_server, client):
        await client.write_register(0, 280, device_id=UNIT_ID)
        result = await client.read_holding_registers(0, count=1, device_id=UNIT_ID)
        assert not result.isError()
        assert result.registers[0] == 280

    async def test_store_write_visible_to_modbus_client(self, running_server, client):
        store, _ = running_server
        store.set_holding(0, 999)
        result = await client.read_holding_registers(0, count=1, device_id=UNIT_ID)
        assert not result.isError()
        assert result.registers[0] == 999


class TestCoils:
    async def test_read_coils_default_false(self, running_server, client):
        result = await client.read_coils(0, count=2, device_id=UNIT_ID)
        assert not result.isError()
        assert result.bits[0] is False
        assert result.bits[1] is False

    async def test_store_coil_visible_to_modbus_client(self, running_server, client):
        store, _ = running_server
        store.set_coil(0, True)
        result = await client.read_coils(0, count=1, device_id=UNIT_ID)
        assert not result.isError()
        assert result.bits[0] is True

    async def test_write_coil(self, running_server, client):
        store, _ = running_server
        await client.write_coil(0, True, device_id=UNIT_ID)
        assert store.get_coil(0) is True

    async def test_write_multiple_coils(self, running_server, client):
        store, _ = running_server
        await client.write_coils(0, values=[True, False], device_id=UNIT_ID)
        assert store.get_coil(0) is True
        assert store.get_coil(1) is False


class TestDiscreteInputs:
    async def test_read_discrete_inputs(self, running_server, client):
        store, _ = running_server
        # tnh-sensor has no discrete inputs, but the block should still return empty list
        result = await client.read_discrete_inputs(0, count=1, device_id=UNIT_ID)
        assert not result.isError()


class TestServerLifecycle:
    async def test_server_stop(self):
        cfg = load_builtin("generic-tnh-sensor")
        store = RegisterStore()
        store.initialize(cfg.registers)
        server = ModbusServerInstance(store=store, port=TEST_PORT + 1, unit_id=UNIT_ID)
        task = asyncio.create_task(server.serve_forever())
        await asyncio.sleep(0.15)
        await server.stop()
        task.cancel()
        with suppress(asyncio.CancelledError, Exception):
            await task
        assert server.status == "stopped"


class TestDataBlockDirect:
    def test_validate_and_reset(self):
        store = RegisterStore()
        store.initialize(load_builtin("generic-tnh-sensor").registers)

        for block in [
            _HoldingBlock(store),
            _InputBlock(store),
            _CoilBlock(store),
            _DiscreteBlock(store),
        ]:
            assert block.validate(0, count=1) is True
            block.reset()  # no-op, must not raise

    def test_input_block_setvalues_noop(self):
        from simbus.core.modbus_server import _InputBlock

        store = RegisterStore()
        store.initialize(load_builtin("generic-tnh-sensor").registers)
        block = _InputBlock(store)
        block.setValues(0, [100])  # no-op, must not raise

    def test_discrete_block_setvalues_noop(self):
        from simbus.core.modbus_server import _DiscreteBlock

        store = RegisterStore()
        store.initialize(load_builtin("generic-tnh-sensor").registers)
        block = _DiscreteBlock(store)
        block.setValues(0, [True])  # no-op, must not raise


class TestHoldingWriteCallback:
    async def test_modbus_write_triggers_callback(self):
        cfg = load_builtin("generic-tnh-sensor")
        store = RegisterStore()
        store.initialize(cfg.registers)
        callback_calls = []

        def on_write(addr: int, raw: int, source: str = "") -> None:
            callback_calls.append((addr, raw, source))

        server = ModbusServerInstance(store=store, port=TEST_PORT + 2, unit_id=UNIT_ID, on_holding_write=on_write)
        task = asyncio.create_task(server.serve_forever())
        await asyncio.sleep(0.15)

        client = AsyncModbusTcpClient("127.0.0.1", port=TEST_PORT + 2)
        await client.connect()
        await client.write_register(0, 300, device_id=UNIT_ID)

        assert len(callback_calls) == 1
        assert callback_calls[0] == (0, 300, "modbus")

        client.close()
        await server.stop()
        task.cancel()
        with suppress(asyncio.CancelledError, Exception):
            await task


class TestInputRegisters:
    def test_input_block_getvalues(self):
        from simbus.core.modbus_server import _InputBlock

        store = RegisterStore()
        store.initialize(load_builtin("generic-tnh-sensor").registers)
        store.set_input(0, 500)
        block = _InputBlock(store)
        assert block.getValues(1, count=1) == [500]  # pymodbus adds +1 offset internally


class TestServerException:
    async def test_server_status_stopped_after_exception(self):
        from unittest.mock import patch

        cfg = load_builtin("generic-tnh-sensor")
        store = RegisterStore()
        store.initialize(cfg.registers)
        server = ModbusServerInstance(store=store, port=TEST_PORT + 4, unit_id=UNIT_ID)

        async def _boom(*args) -> None:
            raise RuntimeError("boom")

        mock_server_cls = type("MockModbusTcpServer", (), {"serve_forever": _boom})
        with (
            patch("simbus.core.modbus_server.ModbusTcpServer", return_value=mock_server_cls()),
            pytest.raises(RuntimeError, match="boom"),
        ):
            await server.serve_forever()

        assert server.status == "stopped"

    def test_port_and_unit_id_properties(self):
        cfg = load_builtin("generic-tnh-sensor")
        store = RegisterStore()
        store.initialize(cfg.registers)
        server = ModbusServerInstance(store=store, port=19599, unit_id=42)
        assert server.port == 19599
        assert server.unit_id == 42
