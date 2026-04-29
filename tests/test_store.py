"""Unit tests for RegisterStore."""

from __future__ import annotations

from simbus.config.loader import load_builtin
from simbus.core.store import RegisterStore


class TestRegisterStore:
    def test_raw_properties_return_internal_dicts(self) -> None:
        cfg = load_builtin("generic-tnh-sensor")
        store = RegisterStore()
        store.initialize(cfg.registers)

        assert store.holding_raw == {0: 225, 1: 450}
        assert store.input_raw == {}
        assert store.coils_raw == {0: False, 1: False}
        assert store.discrete_raw == {}

    def test_raw_properties_reflect_changes(self) -> None:
        cfg = load_builtin("generic-tnh-sensor")
        store = RegisterStore()
        store.initialize(cfg.registers)

        store.set_holding(0, 999)
        assert store.holding_raw[0] == 999

        store.set_coil(0, True)
        assert store.coils_raw[0] is True

    def test_snapshot(self) -> None:
        cfg = load_builtin("generic-tnh-sensor")
        store = RegisterStore()
        store.initialize(cfg.registers)

        snap = store.snapshot()
        assert snap.holding == {0: 225, 1: 450}
        assert snap.coils == {0: False, 1: False}
