"""API integration tests for a single simbus device instance.

Uses starlette.testclient.TestClient which properly triggers the FastAPI
lifespan (startup/shutdown) and runs the event loop internally.

The test app is configured with:
  - device_type="generic-tnh-sensor"  (deterministic, well-known schema)
  - tick_interval=9999.0              (effectively frozen — no auto-ticks)
  - seed=42                           (reproducible RNG if ticks do run)
  - modbus_port=19503                 (avoids collision with other test suites)
"""

from __future__ import annotations

from importlib import resources

import pytest
from starlette.testclient import TestClient

from simbus.api.main import create_app
from simbus.settings import DeviceSettings

# ---------------------------------------------------------------------------
# Shared fixture
# ---------------------------------------------------------------------------


@pytest.fixture(scope="module")
def client() -> TestClient:
    settings = DeviceSettings(
        device_type="generic-tnh-sensor",
        tick_interval=9999.0,
        seed=42,
        modbus_port=19503,
    )
    app = create_app(settings=settings)
    with TestClient(app, raise_server_exceptions=True) as c:
        yield c


# ---------------------------------------------------------------------------
# GET /status
# ---------------------------------------------------------------------------


class TestStatus:
    def test_returns_200(self, client: TestClient) -> None:
        r = client.get("/status")
        assert r.status_code == 200

    def test_response_shape(self, client: TestClient) -> None:
        data = client.get("/status").json()
        assert data["name"] == "Generic T&H Sensor"
        assert data["type"] == "tnh_sensor"
        assert data["modbus_port"] == 19503
        assert data["tick_interval"] == 9999.0

    def test_simulation_running(self, client: TestClient) -> None:
        data = client.get("/status").json()
        assert data["simulation"] == "running"

    def test_modbus_server_listening(self, client: TestClient) -> None:
        data = client.get("/status").json()
        assert data["modbus_server"] == "listening"


# ---------------------------------------------------------------------------
# GET /config
# ---------------------------------------------------------------------------


class TestGetConfig:
    def test_returns_200(self, client: TestClient) -> None:
        assert client.get("/config").status_code == 200

    def test_device_metadata(self, client: TestClient) -> None:
        data = client.get("/config").json()
        assert data["name"] == "Generic T&H Sensor"
        assert data["type"] == "tnh_sensor"
        assert data["version"] == "1.0"
        assert data["unit_id"] == 1
        assert data["endianness"] == "big"

    def test_holding_registers_shape(self, client: TestClient) -> None:
        regs = client.get("/config").json()["registers"]["holding"]
        assert len(regs) == 2
        temp = regs[0]
        assert temp["address"] == 0
        assert temp["name"] == "temperature"
        assert temp["unit"] == "°C"
        assert temp["scale"] == 10
        assert temp["default"] == 22.5
        assert temp["behavior"] == "gaussian_noise"

    def test_coils_shape(self, client: TestClient) -> None:
        coils = client.get("/config").json()["registers"]["coils"]
        assert len(coils) == 2
        assert coils[0]["name"] == "high_temp_alarm"
        assert coils[0]["default"] is False

    def test_register_without_simulation_has_null_behavior(self, client: TestClient) -> None:
        # All tnh-sensor holding registers have simulation, but verify the field exists
        for reg in client.get("/config").json()["registers"]["holding"]:
            assert "behavior" in reg


# ---------------------------------------------------------------------------
# GET /registers
# ---------------------------------------------------------------------------


class TestGetRegisters:
    def test_returns_200(self, client: TestClient) -> None:
        r = client.get("/registers")
        assert r.status_code == 200

    def test_response_has_all_sections(self, client: TestClient) -> None:
        data = client.get("/registers").json()
        assert "holding" in data
        assert "input" in data
        assert "coils" in data
        assert "discrete" in data

    def test_holding_contains_temperature_and_humidity(self, client: TestClient) -> None:
        data = client.get("/registers").json()
        holding = data["holding"]
        # tnh-sensor has registers at addresses 0 and 1
        assert "0" in holding
        assert "1" in holding
        # Values are uint16 integers; behaviors run one tick on startup so
        # we only verify the values are in the valid raw range, not exact defaults.
        assert 0 <= holding["0"] <= 65535
        assert 0 <= holding["1"] <= 65535

    def test_coils_default_false(self, client: TestClient) -> None:
        data = client.get("/registers").json()
        coils = data["coils"]
        assert coils["0"] is False
        assert coils["1"] is False

    def test_input_and_discrete_empty_for_tnh(self, client: TestClient) -> None:
        data = client.get("/registers").json()
        assert data["input"] == {}
        assert data["discrete"] == {}


# ---------------------------------------------------------------------------
# PATCH /registers/{address}
# ---------------------------------------------------------------------------


class TestPatchRegister:
    def test_override_temperature_raw(self, client: TestClient) -> None:
        r = client.patch("/registers/0", json={"value": 300})
        assert r.status_code == 200
        data = r.json()
        assert data["address"] == 0
        assert data["raw_value"] == 300
        assert data["real_value"] == pytest.approx(30.0)  # scale=10

    def test_override_temperature_real_value(self, client: TestClient) -> None:
        r = client.patch("/registers/0", json={"real_value": 27.0})
        assert r.status_code == 200
        data = r.json()
        assert data["raw_value"] == 270
        assert data["real_value"] == pytest.approx(27.0)

    def test_override_reflects_in_snapshot(self, client: TestClient) -> None:
        client.patch("/registers/0", json={"value": 199})
        snap = client.get("/registers").json()
        assert snap["holding"]["0"] == 199

    def test_override_nonexistent_address_returns_404(self, client: TestClient) -> None:
        r = client.patch("/registers/999", json={"value": 100})
        assert r.status_code == 404

    def test_value_must_be_uint16(self, client: TestClient) -> None:
        r = client.patch("/registers/0", json={"value": 70000})
        assert r.status_code == 422

    def test_value_cannot_be_negative(self, client: TestClient) -> None:
        r = client.patch("/registers/0", json={"value": -1})
        assert r.status_code == 422

    def test_rejects_both_fields(self, client: TestClient) -> None:
        r = client.patch("/registers/0", json={"value": 270, "real_value": 27.0})
        assert r.status_code == 422

    def test_rejects_neither_field(self, client: TestClient) -> None:
        r = client.patch("/registers/0", json={})
        assert r.status_code == 422

    def test_restore_default_temperature(self, client: TestClient) -> None:
        # Reset to default so later tests are not affected
        client.patch("/registers/0", json={"value": 225})
        snap = client.get("/registers").json()
        assert snap["holding"]["0"] == 225


# ---------------------------------------------------------------------------
# POST /faults
# ---------------------------------------------------------------------------


class TestInjectFault:
    def test_spike_fault_accepted(self, client: TestClient) -> None:
        r = client.post(
            "/faults",
            json={
                "fault_type": "spike",
                "register_name": "temperature",
                "value": 500.0,
                "duration_s": 30.0,
            },
        )
        assert r.status_code == 202
        assert r.json()["fault_type"] == "spike"

    def test_freeze_fault_accepted(self, client: TestClient) -> None:
        r = client.post(
            "/faults",
            json={
                "fault_type": "freeze",
                "register_name": "temperature",
                "duration_s": 60.0,
            },
        )
        assert r.status_code == 202

    def test_dropout_fault_accepted_no_register(self, client: TestClient) -> None:
        r = client.post(
            "/faults",
            json={
                "fault_type": "dropout",
                "duration_s": 10.0,
            },
        )
        assert r.status_code == 202

    def test_invalid_fault_type_rejected(self, client: TestClient) -> None:
        r = client.post(
            "/faults",
            json={
                "fault_type": "explode",
                "duration_s": 5.0,
            },
        )
        assert r.status_code == 422

    def test_zero_duration_rejected(self, client: TestClient) -> None:
        r = client.post(
            "/faults",
            json={
                "fault_type": "spike",
                "register_name": "temperature",
                "value": 400.0,
                "duration_s": 0.0,
            },
        )
        assert r.status_code == 422


# ---------------------------------------------------------------------------
# GET /faults
# ---------------------------------------------------------------------------


class TestGetFaults:
    def test_returns_list(self, client: TestClient) -> None:
        # Clear first to get a clean state
        client.delete("/faults")
        r = client.get("/faults")
        assert r.status_code == 200
        assert isinstance(r.json(), list)

    def test_injected_fault_appears_in_list(self, client: TestClient) -> None:
        client.delete("/faults")
        client.post(
            "/faults",
            json={
                "fault_type": "freeze",
                "register_name": "humidity",
                "duration_s": 120.0,
            },
        )
        faults = client.get("/faults").json()
        assert len(faults) == 1
        assert faults[0]["fault_type"] == "freeze"
        assert faults[0]["register_name"] == "humidity"
        assert faults[0]["remaining_s"] > 0

    def test_fault_response_has_all_fields(self, client: TestClient) -> None:
        client.delete("/faults")
        client.post(
            "/faults",
            json={
                "fault_type": "spike",
                "register_name": "temperature",
                "value": 999.0,
                "duration_s": 45.0,
            },
        )
        fault = client.get("/faults").json()[0]
        assert "fault_type" in fault
        assert "register_name" in fault
        assert "value" in fault
        assert "duration_s" in fault
        assert "remaining_s" in fault


# ---------------------------------------------------------------------------
# DELETE /faults
# ---------------------------------------------------------------------------


class TestClearFaults:
    def test_clear_returns_204(self, client: TestClient) -> None:
        client.post(
            "/faults",
            json={
                "fault_type": "dropout",
                "duration_s": 999.0,
            },
        )
        r = client.delete("/faults")
        assert r.status_code == 204

    def test_list_empty_after_clear(self, client: TestClient) -> None:
        client.post(
            "/faults",
            json={
                "fault_type": "dropout",
                "duration_s": 999.0,
            },
        )
        client.delete("/faults")
        assert client.get("/faults").json() == []


# ---------------------------------------------------------------------------
# PATCH /simulation
# ---------------------------------------------------------------------------


class TestPatchSimulation:
    def test_update_tick_interval(self, client: TestClient) -> None:
        r = client.patch("/simulation", json={"tick_interval": 2.5})
        assert r.status_code == 200
        assert r.json()["tick_interval"] == 2.5

    def test_tick_interval_reflected_in_status(self, client: TestClient) -> None:
        client.patch("/simulation", json={"tick_interval": 5.0})
        data = client.get("/status").json()
        assert data["tick_interval"] == 5.0

    def test_zero_tick_interval_rejected(self, client: TestClient) -> None:
        r = client.patch("/simulation", json={"tick_interval": 0.0})
        assert r.status_code == 422

    def test_negative_tick_interval_rejected(self, client: TestClient) -> None:
        r = client.patch("/simulation", json={"tick_interval": -1.0})
        assert r.status_code == 422

    def test_empty_body_is_noop(self, client: TestClient) -> None:
        # patch with no fields — should succeed and return current state
        r = client.patch("/simulation", json={})
        assert r.status_code == 200

    def test_restore_tick_interval(self, client: TestClient) -> None:
        client.patch("/simulation", json={"tick_interval": 9999.0})
        data = client.get("/status").json()
        assert data["tick_interval"] == 9999.0


# ---------------------------------------------------------------------------
# POST /simulation/reset
# ---------------------------------------------------------------------------


class TestSimulationReset:
    def test_reset_returns_204(self, client: TestClient) -> None:
        assert client.post("/simulation/reset").status_code == 204

    def test_reset_restores_overridden_register(self, client: TestClient) -> None:
        # Override temperature to an extreme value
        client.patch("/registers/0", json={"value": 999})
        assert client.get("/registers").json()["holding"]["0"] == 999
        # Reset — should go back to default (22.5°C × 10 = 225)
        client.post("/simulation/reset")
        assert client.get("/registers").json()["holding"]["0"] == 225

    def test_reset_clears_faults(self, client: TestClient) -> None:
        client.post("/faults", json={"fault_type": "dropout", "duration_s": 999.0})
        assert len(client.get("/faults").json()) > 0
        client.post("/simulation/reset")
        assert client.get("/faults").json() == []

    def test_simulation_still_running_after_reset(self, client: TestClient) -> None:
        client.post("/simulation/reset")
        assert client.get("/status").json()["simulation"] == "running"


# ---------------------------------------------------------------------------
# CORS headers
# ---------------------------------------------------------------------------


class TestCORS:
    def test_cors_header_present_on_status(self, client: TestClient) -> None:
        r = client.get("/status", headers={"Origin": "http://localhost:5173"})
        assert r.headers.get("access-control-allow-origin") in ("*", "http://localhost:5173")

    def test_cors_preflight(self, client: TestClient) -> None:
        r = client.options(
            "/registers",
            headers={
                "Origin": "http://localhost:5173",
                "Access-Control-Request-Method": "GET",
            },
        )
        assert r.status_code in (200, 204)


# ---------------------------------------------------------------------------
# Device name override via settings
# ---------------------------------------------------------------------------


class TestDeviceNameOverride:
    def test_name_override(self) -> None:
        settings = DeviceSettings(
            device_type="generic-tnh-sensor",
            device_name="Lab Sensor A",
            tick_interval=9999.0,
            modbus_port=19504,
        )
        app = create_app(settings=settings)
        with TestClient(app) as c:
            data = c.get("/status").json()
            assert data["name"] == "Lab Sensor A"


class TestYamlPath:
    def test_load_from_yaml_path(self) -> None:
        pkg = resources.files("simbus.builtin")
        yaml_path = str(pkg / "generic-ups.yaml")
        settings = DeviceSettings(
            yaml_path=yaml_path,
            tick_interval=9999.0,
            modbus_port=19505,
        )
        app = create_app(settings=settings)
        with TestClient(app) as c:
            data = c.get("/status").json()
            assert data["name"] == "Generic UPS"
            assert data["type"] == "ups"

    def test_fallback_settings_when_none_provided(self) -> None:
        """When create_app() receives no settings, lifespan falls back to env vars."""
        app = create_app()
        with TestClient(app) as c:
            data = c.get("/status").json()
            assert data["type"] == "tnh_sensor"


# ---------------------------------------------------------------------------
# PATCH /registers/coils and /registers/discrete
# ---------------------------------------------------------------------------


class TestCoilAndDiscrete:
    def test_override_coil(self) -> None:
        settings = DeviceSettings(
            device_type="generic-door-contact",
            tick_interval=9999.0,
            modbus_port=19506,
        )
        app = create_app(settings=settings)
        with TestClient(app) as c:
            r = c.patch("/registers/coils/0", json={"value": True})
            assert r.status_code == 200
            assert r.json()["value"] is True
            assert c.get("/registers").json()["coils"]["0"] is True

    def test_override_coil_404(self) -> None:
        settings = DeviceSettings(
            device_type="generic-door-contact",
            tick_interval=9999.0,
            modbus_port=19507,
        )
        app = create_app(settings=settings)
        with TestClient(app) as c:
            r = c.patch("/registers/coils/99", json={"value": True})
            assert r.status_code == 404

    def test_override_discrete(self) -> None:
        settings = DeviceSettings(
            device_type="generic-door-contact",
            tick_interval=9999.0,
            modbus_port=19508,
        )
        app = create_app(settings=settings)
        with TestClient(app) as c:
            r = c.patch("/registers/discrete/0", json={"value": False})
            assert r.status_code == 200
            assert r.json()["value"] is False
            assert c.get("/registers").json()["discrete"]["0"] is False

    def test_override_discrete_404(self) -> None:
        settings = DeviceSettings(
            device_type="generic-door-contact",
            tick_interval=9999.0,
            modbus_port=19509,
        )
        app = create_app(settings=settings)
        with TestClient(app) as c:
            r = c.patch("/registers/discrete/99", json={"value": True})
            assert r.status_code == 404


# ---------------------------------------------------------------------------
# PATCH /registers/input
# ---------------------------------------------------------------------------


class TestInputRegister:
    def test_override_input_register(self, tmp_path) -> None:
        yaml_file = tmp_path / "input-device.yaml"
        yaml_file.write_text(
            "name: Input Device\n"
            "version: '1.0'\n"
            "type: input_test\n"
            "modbus:\n"
            "  default_port: 502\n"
            "  unit_id: 1\n"
            "registers:\n"
            "  input:\n"
            "    - address: 0\n"
            "      name: temp_ro\n"
            "      default: 25.0\n"
            "      scale: 10\n"
        )
        settings = DeviceSettings(
            yaml_path=str(yaml_file),
            tick_interval=9999.0,
            modbus_port=19510,
        )
        app = create_app(settings=settings)
        with TestClient(app) as c:
            r = c.patch("/registers/input/0", json={"value": 350})
            assert r.status_code == 200
            assert r.json()["raw_value"] == 350
            assert r.json()["real_value"] == pytest.approx(35.0)
            assert c.get("/registers").json()["input"]["0"] == 350

    def test_override_input_register_404(self, tmp_path) -> None:
        yaml_file = tmp_path / "input-device.yaml"
        yaml_file.write_text(
            "name: Input Device\n"
            "version: '1.0'\n"
            "type: input_test\n"
            "modbus:\n"
            "  default_port: 502\n"
            "  unit_id: 1\n"
            "registers:\n"
            "  input:\n"
            "    - address: 0\n"
            "      name: temp_ro\n"
            "      default: 25.0\n"
            "      scale: 10\n"
        )
        settings = DeviceSettings(
            yaml_path=str(yaml_file),
            tick_interval=9999.0,
            modbus_port=19511,
        )
        app = create_app(settings=settings)
        with TestClient(app) as c:
            r = c.patch("/registers/input/99", json={"value": 100})
            assert r.status_code == 404
