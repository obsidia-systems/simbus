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
from simbus.core.modbus_server import ModbusServerInstance
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
        assert result.registers[0] == store.get_holding(
            1)  # 450 = 45.0%RH × 10

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
