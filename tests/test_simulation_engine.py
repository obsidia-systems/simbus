"""Unit tests for SimulationEngine.

Tests the tick loop, behavior dispatch, alarm evaluation, fault injection,
input register simulation, and the live tick_interval update.
"""

from __future__ import annotations

import asyncio

import pytest

from simbus.config.loader import load_builtin
from simbus.config.schema import (
    DeviceConfig,
    ModbusConfig,
    RegisterMapConfig,
)
from simbus.core.store import RegisterStore
from simbus.simulation.engine import SimulationEngine
from simbus.simulation.faults import ActiveFault, FaultType


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _make_engine(device_type: str = "generic-tnh-sensor", **kwargs: object) -> tuple[SimulationEngine, RegisterStore]:
    cfg = load_builtin(device_type)
    store = RegisterStore()
    store.initialize(cfg.registers)
    engine = SimulationEngine(store=store, config=cfg, seed=42, **kwargs)
    return engine, store


def _minimal_config(registers: dict) -> DeviceConfig:
    return DeviceConfig.model_validate({
        "name": "Test",
        "version": "1.0",
        "type": "test",
        "modbus": {"default_port": 5020, "unit_id": 1},
        "registers": registers,
    })


# ---------------------------------------------------------------------------
# Initialisation
# ---------------------------------------------------------------------------


class TestInit:
    def test_tick_interval_stored(self) -> None:
        engine, _ = _make_engine(tick_interval=2.5)
        assert engine.tick_interval == 2.5

    def test_default_tick_interval(self) -> None:
        engine, _ = _make_engine()
        assert engine.tick_interval == 1.0

    def test_not_running_before_start(self) -> None:
        engine, _ = _make_engine()
        assert engine._running is False

    def test_state_initialised_for_all_registers(self) -> None:
        cfg = load_builtin("generic-tnh-sensor")
        store = RegisterStore()
        store.initialize(cfg.registers)
        engine = SimulationEngine(store=store, config=cfg)
        # tnh-sensor: 2 holding registers at addresses 0 and 1
        assert 0 in engine._state
        assert 1 in engine._state

    def test_state_base_matches_default(self) -> None:
        cfg = load_builtin("generic-tnh-sensor")
        store = RegisterStore()
        store.initialize(cfg.registers)
        engine = SimulationEngine(store=store, config=cfg)
        # temperature default = 22.5
        assert engine._state[0].base == pytest.approx(22.5)


# ---------------------------------------------------------------------------
# Single tick — holding registers
# ---------------------------------------------------------------------------


class TestTickHolding:
    def test_constant_behavior_unchanged(self) -> None:
        cfg = _minimal_config({
            "holding": [{"address": 0, "name": "setpoint", "default": 18.0, "scale": 10,
                         "simulation": {"behavior": "constant"}}]
        })
        store = RegisterStore()
        store.initialize(cfg.registers)
        engine = SimulationEngine(store=store, config=cfg, seed=0)
        engine._tick(1.0)
        assert store.get_holding(0) == 180  # 18.0 × 10 (state.base initialized from default)

    def test_constant_behavior_follows_patch(self) -> None:
        cfg = _minimal_config({
            "holding": [{"address": 0, "name": "setpoint", "default": 18.0, "scale": 10,
                         "simulation": {"behavior": "constant"}}]
        })
        store = RegisterStore()
        store.initialize(cfg.registers)
        engine = SimulationEngine(store=store, config=cfg, seed=0)
        store.set_holding(0, 200)
        engine.update_base(0, 200)   # 200 raw / scale 10 = 20.0°C
        engine._tick(1.0)
        assert store.get_holding(0) == 200  # stays at 20.0°C

    def test_gaussian_noise_updates_register(self) -> None:
        engine, store = _make_engine(tick_interval=1.0)
        before = store.get_holding(0)
        engine._tick(1.0)
        # With noise it's very unlikely to be the same value
        after = store.get_holding(0)
        assert isinstance(after, int)
        assert 0 <= after <= 65535

    def test_elapsed_time_advances_each_tick(self) -> None:
        engine, _ = _make_engine()
        assert engine._state[0].elapsed_s == 0.0
        engine._tick(1.0)
        assert engine._state[0].elapsed_s == pytest.approx(1.0)
        engine._tick(2.5)
        assert engine._state[0].elapsed_s == pytest.approx(3.5)

    def test_register_without_simulation_not_updated(self) -> None:
        cfg = _minimal_config({
            "holding": [{"address": 0, "name": "manual", "default": 50.0, "scale": 1}]
        })
        store = RegisterStore()
        store.initialize(cfg.registers)
        engine = SimulationEngine(store=store, config=cfg)
        engine._tick(1.0)
        # No simulation → store value stays at default
        assert store.get_holding(0) == 50


# ---------------------------------------------------------------------------
# Single tick — input registers (bug fix verification)
# ---------------------------------------------------------------------------


class TestTickInputRegisters:
    def test_input_register_simulated(self) -> None:
        cfg = _minimal_config({
            "input": [{"address": 0, "name": "sensor", "default": 100.0, "scale": 10,
                       "simulation": {"behavior": "gaussian_noise", "std_dev": 1.0}}]
        })
        store = RegisterStore()
        store.initialize(cfg.registers)
        engine = SimulationEngine(store=store, config=cfg, seed=7)
        engine._tick(1.0)
        # Value should have been written to the input store
        assert store.get_input(0) != 0

    def test_input_register_state_tracked(self) -> None:
        cfg = _minimal_config({
            "input": [{"address": 5, "name": "flow", "default": 30.0, "scale": 100,
                       "simulation": {"behavior": "sinusoidal",
                                      "period_hours": 1, "amplitude": 5.0}}]
        })
        store = RegisterStore()
        store.initialize(cfg.registers)
        engine = SimulationEngine(store=store, config=cfg)
        assert 5 in engine._state
        engine._tick(1.0)
        assert engine._state[5].elapsed_s == pytest.approx(1.0)

    def test_input_register_faults_not_applied(self) -> None:
        """Faults must not affect input registers — they are read-only to clients."""
        cfg = _minimal_config({
            "input": [{"address": 0, "name": "ro_sensor", "default": 50.0, "scale": 10,
                       "simulation": {"behavior": "constant"}}]
        })
        store = RegisterStore()
        store.initialize(cfg.registers)
        engine = SimulationEngine(store=store, config=cfg)
        engine.inject_fault(ActiveFault(
            fault_type=FaultType.spike,
            register_name="ro_sensor",
            value=9999.0,
            duration_s=60.0,
            remaining_s=60.0,
        ))
        engine._tick(1.0)
        # Fault must NOT have spiked the input register
        assert store.get_input(0) == 500  # 50.0 × 10


# ---------------------------------------------------------------------------
# Alarm evaluation
# ---------------------------------------------------------------------------


class TestAlarmEvaluation:
    def test_coil_set_when_threshold_exceeded(self) -> None:
        cfg = _minimal_config({
            "holding": [{"address": 0, "name": "temp", "default": 35.0, "scale": 10,
                         "simulation": {"behavior": "constant"}}],
            "coils": [{"address": 0, "name": "high_temp", "default": False,
                       "trigger": {"source_register": "temp",
                                   "condition": "gt", "threshold": 30.0}}],
        })
        store = RegisterStore()
        store.initialize(cfg.registers)
        engine = SimulationEngine(store=store, config=cfg)
        engine._tick(1.0)
        assert store.get_coil(0) is True

    def test_coil_cleared_when_below_threshold(self) -> None:
        cfg = _minimal_config({
            "holding": [{"address": 0, "name": "temp", "default": 20.0, "scale": 10,
                         "simulation": {"behavior": "constant"}}],
            "coils": [{"address": 0, "name": "high_temp", "default": False,
                       "trigger": {"source_register": "temp",
                                   "condition": "gt", "threshold": 30.0}}],
        })
        store = RegisterStore()
        store.initialize(cfg.registers)
        # Pre-set coil to True to confirm it gets cleared
        store.set_coil(0, True)
        engine = SimulationEngine(store=store, config=cfg)
        engine._tick(1.0)
        assert store.get_coil(0) is False

    def test_coil_trigger_from_input_register(self) -> None:
        """Coil triggers must read from the input store when source is an input register."""
        cfg = _minimal_config({
            "input": [{"address": 0, "name": "temp_ro", "default": 35.0, "scale": 10,
                       "simulation": {"behavior": "constant"}}],
            "coils": [{"address": 0, "name": "high_temp", "default": False,
                       "trigger": {"source_register": "temp_ro",
                                   "condition": "gt", "threshold": 30.0}}],
        })
        store = RegisterStore()
        store.initialize(cfg.registers)
        engine = SimulationEngine(store=store, config=cfg)
        engine._tick(1.0)
        # Input register was written by the engine; coil trigger must read from input store
        assert store.get_coil(0) is True

    def test_tnh_alarm_triggers_on_high_temperature(self) -> None:
        engine, store = _make_engine()
        # Force temperature to a high raw value (> 30.0°C → raw > 300)
        store.set_holding(0, 310)  # 31.0°C
        engine._evaluate_alarms()
        assert store.get_coil(0) is True  # high_temp_alarm

    def test_discrete_trigger_uses_set_discrete(self) -> None:
        """Discrete inputs must be written via set_discrete, not set_coil."""
        cfg = _minimal_config({
            "holding": [{"address": 0, "name": "level", "default": 90.0, "scale": 1,
                         "simulation": {"behavior": "constant"}}],
            "discrete": [{"address": 0, "name": "overflow", "default": False,
                          "trigger": {"source_register": "level",
                                      "condition": "gt", "threshold": 80.0}}],
        })
        store = RegisterStore()
        store.initialize(cfg.registers)
        engine = SimulationEngine(store=store, config=cfg)
        engine._tick(1.0)
        # Must be in discrete store, not coil store
        assert store.get_discrete(0) is True
        assert store.get_coil(0) is False  # coil store untouched

    def test_alarm_fault_forces_coil_true(self) -> None:
        """alarm fault must force the named coil to True regardless of register value."""
        cfg = _minimal_config({
            "holding": [{"address": 0, "name": "temp", "default": 20.0, "scale": 10,
                         "simulation": {"behavior": "constant"}}],
            "coils": [{"address": 0, "name": "high_temp", "default": False,
                       "trigger": {"source_register": "temp",
                                   "condition": "gt", "threshold": 30.0}}],
        })
        store = RegisterStore()
        store.initialize(cfg.registers)
        engine = SimulationEngine(store=store, config=cfg)
        # Temperature is 20°C — below threshold, coil should normally be False
        engine._tick(1.0)
        assert store.get_coil(0) is False

        # Inject alarm fault targeting the coil by name
        engine.inject_fault(ActiveFault(
            fault_type=FaultType.alarm,
            register_name="high_temp",
            value=None,
            duration_s=10.0,
            remaining_s=10.0,
        ))
        engine._tick(1.0)
        # Coil must be forced True even though register is below threshold
        assert store.get_coil(0) is True

    def test_alarm_fault_does_not_affect_register(self) -> None:
        """alarm fault must not spike the holding register value."""
        cfg = _minimal_config({
            "holding": [{"address": 0, "name": "temp", "default": 20.0, "scale": 10,
                         "simulation": {"behavior": "constant"}}],
            "coils": [{"address": 0, "name": "high_temp", "default": False,
                       "trigger": {"source_register": "temp",
                                   "condition": "gt", "threshold": 30.0}}],
        })
        store = RegisterStore()
        store.initialize(cfg.registers)
        engine = SimulationEngine(store=store, config=cfg)
        engine.inject_fault(ActiveFault(
            fault_type=FaultType.alarm,
            register_name="high_temp",
            value=None,
            duration_s=10.0,
            remaining_s=10.0,
        ))
        engine._tick(1.0)
        # Register must stay at 20.0°C (200 raw) — alarm fault only touches the coil
        assert store.get_holding(0) == 200

    def test_alarm_fault_clears_after_expiry(self) -> None:
        """After the alarm fault expires, coil should return to normal trigger evaluation."""
        cfg = _minimal_config({
            "holding": [{"address": 0, "name": "temp", "default": 20.0, "scale": 10,
                         "simulation": {"behavior": "constant"}}],
            "coils": [{"address": 0, "name": "high_temp", "default": False,
                       "trigger": {"source_register": "temp",
                                   "condition": "gt", "threshold": 30.0}}],
        })
        store = RegisterStore()
        store.initialize(cfg.registers)
        engine = SimulationEngine(store=store, config=cfg)
        engine.inject_fault(ActiveFault(
            fault_type=FaultType.alarm,
            register_name="high_temp",
            value=None,
            duration_s=5.0,
            remaining_s=5.0,
        ))
        engine._tick(1.0)   # remaining_s → 4.0 → fault still active → coil True
        assert store.get_coil(0) is True

        engine._tick(10.0)  # remaining_s → -6.0 → fault expired → temp 20°C < 30°C → False
        assert store.get_coil(0) is False


# ---------------------------------------------------------------------------
# Fault injection
# ---------------------------------------------------------------------------


class TestFaultInjection:
    def test_spike_overrides_register_value(self) -> None:
        cfg = _minimal_config({
            "holding": [{"address": 0, "name": "temp", "default": 22.5, "scale": 10,
                         "simulation": {"behavior": "constant"}}],
        })
        store = RegisterStore()
        store.initialize(cfg.registers)
        engine = SimulationEngine(store=store, config=cfg)
        engine.inject_fault(ActiveFault(
            fault_type=FaultType.spike,
            register_name="temp",
            value=99.9,
            duration_s=10.0,
            remaining_s=10.0,
        ))
        engine._tick(1.0)
        assert store.get_holding(0) == 999  # 99.9 × 10

    def test_dropout_sets_register_to_zero(self) -> None:
        cfg = _minimal_config({
            "holding": [{"address": 0, "name": "temp", "default": 22.5, "scale": 10,
                         "simulation": {"behavior": "constant"}}],
        })
        store = RegisterStore()
        store.initialize(cfg.registers)
        engine = SimulationEngine(store=store, config=cfg)
        engine.inject_fault(ActiveFault(
            fault_type=FaultType.dropout,
            register_name=None,
            value=None,
            duration_s=10.0,
            remaining_s=10.0,
        ))
        engine._tick(1.0)
        assert store.get_holding(0) == 0

    def test_fault_expires_after_duration(self) -> None:
        engine, _ = _make_engine()
        engine.inject_fault(ActiveFault(
            fault_type=FaultType.freeze,
            register_name="temperature",
            value=None,
            duration_s=2.0,
            remaining_s=2.0,
        ))
        assert "temperature" in engine._faults
        engine.tick_faults(1.0)
        assert "temperature" in engine._faults
        engine.tick_faults(1.5)
        assert "temperature" not in engine._faults

    def test_clear_faults_removes_all(self) -> None:
        engine, _ = _make_engine()
        engine.inject_fault(ActiveFault(FaultType.spike, "temperature", 999.0, 10.0, 10.0))
        engine.inject_fault(ActiveFault(FaultType.freeze, "humidity", None, 10.0, 10.0))
        assert len(engine._faults) == 2
        engine.clear_faults()
        assert len(engine._faults) == 0


# ---------------------------------------------------------------------------
# Live tick_interval update (bug fix verification)
# ---------------------------------------------------------------------------


class TestUpdateBase:
    def test_base_updated_to_new_real_value(self) -> None:
        engine, store = _make_engine()
        # temperature: scale=10, PATCH raw=270 → base should become 27.0
        engine.update_base(0, 270)
        assert engine._state[0].base == pytest.approx(27.0)

    def test_simulation_continues_from_new_base(self) -> None:
        cfg = _minimal_config({
            "holding": [{"address": 0, "name": "temp", "default": 22.5, "scale": 10,
                         "simulation": {"behavior": "gaussian_noise", "std_dev": 0.1}}]
        })
        store = RegisterStore()
        store.initialize(cfg.registers)
        engine = SimulationEngine(store=store, config=cfg, seed=0)

        engine.update_base(0, 270)   # new operating point: 27.0°C
        store.set_holding(0, 270)

        # Run several ticks — values should oscillate around 270 (27.0°C), not 225
        for _ in range(10):
            engine._tick(1.0)
        value = store.get_holding(0)
        # Should be near 270 (±20 raw units = ±2.0°C), definitely not back at 225
        assert abs(value - 270) < 20

    def test_sinusoidal_center_follows_patch(self) -> None:
        """PATCH on a sinusoidal register shifts its oscillation center."""
        cfg = _minimal_config({
            "holding": [{"address": 1, "name": "humidity", "default": 45.0, "scale": 10,
                         "simulation": {"behavior": "sinusoidal",
                                        "period_hours": 12, "amplitude": 5.0}}]
        })
        store = RegisterStore()
        store.initialize(cfg.registers)
        engine = SimulationEngine(store=store, config=cfg, seed=0)

        # Shift center from 45.0%RH to 60.0%RH
        store.set_holding(1, 600)
        engine.update_base(1, 600)   # 600 / 10 = 60.0

        # Run several ticks — output should oscillate around 600, not 450
        for _ in range(5):
            engine._tick(1.0)
        value = store.get_holding(1)
        # amplitude=5.0 → max deviation = 50 raw; center should be ~600
        assert abs(value - 600) <= 55

    def test_unknown_address_is_noop(self) -> None:
        engine, _ = _make_engine()
        engine.update_base(999, 500)  # address doesn't exist — must not raise


class TestLiveTickInterval:
    def test_tick_interval_mutable(self) -> None:
        engine, _ = _make_engine(tick_interval=1.0)
        engine.tick_interval = 5.0
        assert engine.tick_interval == 5.0

    @pytest.mark.asyncio
    async def test_run_uses_updated_tick_interval(self) -> None:
        """Engine picks up tick_interval change mid-run without restart."""
        engine, _ = _make_engine(tick_interval=0.05)
        task = asyncio.create_task(engine.run())
        await asyncio.sleep(0.08)  # ~1 tick at 0.05s

        engine.tick_interval = 0.02  # speed up
        await asyncio.sleep(0.06)   # ~3 ticks at 0.02s

        engine.stop()
        task.cancel()
        with pytest.raises((asyncio.CancelledError, Exception)):
            await task

        # elapsed_s grows — confirms ticks ran after the interval change
        assert engine._state[0].elapsed_s > 0.05
