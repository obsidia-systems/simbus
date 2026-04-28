"""Unit tests for pure simulation behavior functions."""

from __future__ import annotations

from random import Random

import pytest

from simbus.config.schema import StepEntry
from simbus.simulation.behaviors import (
    constant,
    drift_step,
    gaussian_noise,
    raw_to_scaled,
    sawtooth,
    scale_to_raw,
    sinusoidal,
    step_value,
)


class TestConstant:
    def test_returns_default(self) -> None:
        assert constant(22.5) == 22.5

    def test_zero(self) -> None:
        assert constant(0.0) == 0.0


class TestGaussianNoise:
    def test_mean_close_to_base(self) -> None:
        rng = Random(42)
        results = [gaussian_noise(22.5, 0.1, rng) for _ in range(500)]
        mean = sum(results) / len(results)
        assert abs(mean - 22.5) < 0.05

    def test_small_std_dev_stays_close(self) -> None:
        rng = Random(0)
        for _ in range(50):
            val = gaussian_noise(10.0, 0.001, rng)
            assert abs(val - 10.0) < 0.02


class TestSinusoidal:
    def test_at_zero_time_returns_center(self) -> None:
        val = sinusoidal(20.0, 5.0, 1.0, 0.0)
        assert val == pytest.approx(20.0)

    def test_peak_at_quarter_period(self) -> None:
        period_h = 1.0
        period_s = 3600.0
        peak = sinusoidal(0.0, 5.0, period_h, period_s / 4)
        assert peak == pytest.approx(5.0, abs=1e-6)

    def test_trough_at_three_quarter_period(self) -> None:
        period_h = 1.0
        period_s = 3600.0
        trough = sinusoidal(0.0, 5.0, period_h, 3 * period_s / 4)
        assert trough == pytest.approx(-5.0, abs=1e-6)

    def test_center_offset_applied(self) -> None:
        # at t=0, sin(0)=0, so result == center
        assert sinusoidal(30.0, 10.0, 2.0, 0.0) == pytest.approx(30.0)

    def test_completes_full_cycle(self) -> None:
        period_h = 1.0
        period_s = 3600.0
        val_0 = sinusoidal(20.0, 5.0, period_h, 0.0)
        val_full = sinusoidal(20.0, 5.0, period_h, period_s)
        assert val_0 == pytest.approx(val_full, abs=1e-6)


class TestDriftStep:
    def test_increases_by_rate(self) -> None:
        assert drift_step(10.0, 0.5, (0.0, 100.0)) == pytest.approx(10.5)

    def test_decreases_with_negative_rate(self) -> None:
        assert drift_step(50.0, -1.0, (0.0, 100.0)) == pytest.approx(49.0)

    def test_clamped_at_upper_bound(self) -> None:
        assert drift_step(99.8, 0.5, (0.0, 100.0)) == pytest.approx(100.0)

    def test_clamped_at_lower_bound(self) -> None:
        assert drift_step(0.2, -0.5, (0.0, 100.0)) == pytest.approx(0.0)

    def test_stays_at_bounds_when_already_there(self) -> None:
        assert drift_step(100.0, 1.0, (0.0, 100.0)) == pytest.approx(100.0)
        assert drift_step(0.0, -1.0, (0.0, 100.0)) == pytest.approx(0.0)


class TestSawtooth:
    def test_starts_at_min(self) -> None:
        assert sawtooth(60.0, 0.0, 10.0, 0.0) == pytest.approx(0.0)

    def test_midpoint(self) -> None:
        assert sawtooth(60.0, 0.0, 10.0, 30.0) == pytest.approx(5.0, abs=1e-6)

    def test_approaches_max_before_reset(self) -> None:
        # at t=59.99... should be close to max
        val = sawtooth(60.0, 0.0, 10.0, 59.9)
        assert val > 9.9

    def test_wraps_at_period(self) -> None:
        val_0 = sawtooth(60.0, 0.0, 10.0, 0.0)
        val_60 = sawtooth(60.0, 0.0, 10.0, 60.0)
        assert val_0 == pytest.approx(val_60)

    def test_custom_range(self) -> None:
        val = sawtooth(100.0, 50.0, 150.0, 50.0)
        assert val == pytest.approx(100.0, abs=1e-6)


class TestStepValue:
    def test_returns_default_before_any_step(self) -> None:
        steps = [StepEntry(at=10.0, value=50.0)]
        assert step_value(22.5, steps, 5.0) == pytest.approx(22.5)

    def test_returns_step_value_at_threshold(self) -> None:
        steps = [StepEntry(at=10.0, value=50.0)]
        assert step_value(22.5, steps, 10.0) == pytest.approx(50.0)

    def test_returns_step_value_after_threshold(self) -> None:
        steps = [StepEntry(at=10.0, value=50.0)]
        assert step_value(22.5, steps, 15.0) == pytest.approx(50.0)

    def test_last_step_wins(self) -> None:
        steps = [StepEntry(at=10.0, value=50.0), StepEntry(at=20.0, value=75.0)]
        assert step_value(0.0, steps, 25.0) == pytest.approx(75.0)

    def test_step_ordering_independent_of_definition_order(self) -> None:
        steps = [StepEntry(at=20.0, value=75.0), StepEntry(at=10.0, value=50.0)]
        assert step_value(0.0, steps, 15.0) == pytest.approx(50.0)

    def test_empty_steps_returns_default(self) -> None:
        assert step_value(22.5, [], 100.0) == pytest.approx(22.5)


class TestScaling:
    def test_scale_to_raw_basic(self) -> None:
        assert scale_to_raw(22.5, 10) == 225

    def test_scale_to_raw_rounds(self) -> None:
        assert scale_to_raw(22.55, 10) == 226

    def test_raw_to_scaled(self) -> None:
        assert raw_to_scaled(225, 10) == pytest.approx(22.5)

    def test_round_trip(self) -> None:
        original = 22.5
        assert raw_to_scaled(scale_to_raw(original, 10), 10) == pytest.approx(original)

    def test_scale_to_raw_clamps_uint16(self) -> None:
        result = scale_to_raw(700.0, 100)
        assert 0 <= result <= 65535
