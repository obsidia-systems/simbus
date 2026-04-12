"""Tests for device config schema validation and YAML loader."""

from __future__ import annotations

import pytest
from pydantic import ValidationError

from simbus.config.loader import load_builtin
from simbus.config.schema import (
    DeviceConfig,
    GaussianNoiseBehavior,
    RegisterConfig,
    SinusoidalBehavior,
)

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _minimal_device(**overrides: object) -> dict:  # type: ignore[type-arg]
    base: dict = {  # type: ignore[type-arg]
        "name": "Test Device",
        "version": "1.0",
        "type": "test",
        "modbus": {"default_port": 5020, "unit_id": 1},
        "registers": {},
    }
    base.update(overrides)
    return base


# ---------------------------------------------------------------------------
# DeviceConfig validation
# ---------------------------------------------------------------------------


class TestDeviceConfig:
    def test_valid_minimal(self) -> None:
        cfg = DeviceConfig.model_validate(_minimal_device())
        assert cfg.name == "Test Device"
        assert cfg.modbus.unit_id == 1
        assert cfg.registers.holding == []

    def test_invalid_unit_id_too_high(self) -> None:
        with pytest.raises(ValidationError):
            DeviceConfig.model_validate(
                _minimal_device(modbus={"default_port": 5020, "unit_id": 300})
            )

    def test_invalid_port_too_low(self) -> None:
        with pytest.raises(ValidationError):
            DeviceConfig.model_validate(
                _minimal_device(modbus={"default_port": 80, "unit_id": 1})
            )

    def test_trigger_references_unknown_register(self) -> None:
        with pytest.raises(ValidationError, match="unknown register"):
            DeviceConfig.model_validate(
                _minimal_device(
                    registers={
                        "coils": [
                            {
                                "address": 0,
                                "name": "alarm",
                                "default": False,
                                "trigger": {
                                    "source_register": "nonexistent",
                                    "condition": "gt",
                                    "threshold": 30.0,
                                },
                            }
                        ]
                    }
                )
            )

    def test_alarm_references_unknown_coil(self) -> None:
        with pytest.raises(ValidationError, match="unknown coil"):
            DeviceConfig.model_validate(
                _minimal_device(
                    alarms=[{"name": "Bad Alarm",
                             "severity": "warning", "trigger": "ghost_coil"}]
                )
            )

    def test_valid_coil_with_trigger(self) -> None:
        cfg = DeviceConfig.model_validate(
            _minimal_device(
                registers={
                    "holding": [
                        {"address": 0, "name": "temperature",
                            "default": 22.5, "scale": 10}
                    ],
                    "coils": [
                        {
                            "address": 0,
                            "name": "high_temp_alarm",
                            "default": False,
                            "trigger": {
                                "source_register": "temperature",
                                "condition": "gt",
                                "threshold": 30.0,
                            },
                        }
                    ],
                }
            )
        )
        assert cfg.registers.coils[0].trigger is not None
        assert cfg.registers.coils[0].trigger.threshold == 30.0


# ---------------------------------------------------------------------------
# Behavior parsing
# ---------------------------------------------------------------------------


class TestBehaviorParsing:
    def test_gaussian_noise(self) -> None:
        reg = RegisterConfig.model_validate(
            {
                "address": 0,
                "name": "temp",
                "default": 22.5,
                "scale": 10,
                "simulation": {"behavior": "gaussian_noise", "std_dev": 0.3},
            }
        )
        assert isinstance(reg.simulation, GaussianNoiseBehavior)
        assert reg.simulation.std_dev == 0.3

    def test_gaussian_noise_with_drift(self) -> None:
        reg = RegisterConfig.model_validate(
            {
                "address": 0,
                "name": "temp",
                "default": 22.5,
                "scale": 10,
                "simulation": {
                    "behavior": "gaussian_noise",
                    "std_dev": 0.3,
                    "drift": {"enabled": True, "rate": 0.01, "bounds": [18.0, 35.0]},
                },
            }
        )
        assert isinstance(reg.simulation, GaussianNoiseBehavior)
        assert reg.simulation.drift is not None
        assert reg.simulation.drift.rate == 0.01

    def test_sinusoidal(self) -> None:
        reg = RegisterConfig.model_validate(
            {
                "address": 1,
                "name": "humidity",
                "default": 45.0,
                "scale": 10,
                "simulation": {"behavior": "sinusoidal", "period_hours": 12, "amplitude": 5.0},
            }
        )
        assert isinstance(reg.simulation, SinusoidalBehavior)
        assert reg.simulation.period_hours == 12

    def test_unknown_behavior_rejected(self) -> None:
        with pytest.raises(ValidationError):
            RegisterConfig.model_validate(
                {
                    "address": 0,
                    "name": "temp",
                    "default": 22.5,
                    "scale": 10,
                    "simulation": {"behavior": "telekinesis"},
                }
            )

    def test_std_dev_must_be_positive(self) -> None:
        with pytest.raises(ValidationError):
            RegisterConfig.model_validate(
                {
                    "address": 0,
                    "name": "temp",
                    "default": 22.5,
                    "scale": 10,
                    "simulation": {"behavior": "gaussian_noise", "std_dev": -1.0},
                }
            )


# ---------------------------------------------------------------------------
# Builtin loader
# ---------------------------------------------------------------------------


class TestLoader:
    def test_load_builtin_tnh(self) -> None:
        cfg = load_builtin("generic-tnh-sensor")
        assert cfg.type == "tnh_sensor"
        assert len(cfg.registers.holding) == 2
        assert cfg.registers.holding[0].name == "temperature"
        assert cfg.registers.holding[1].name == "humidity"
        assert len(cfg.registers.coils) == 2
        assert len(cfg.alarms) == 2

    def test_load_builtin_unknown_raises(self) -> None:
        with pytest.raises(ValueError, match="Unknown built-in"):
            load_builtin("nonexistent-device")
