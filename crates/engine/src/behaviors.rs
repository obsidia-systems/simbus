//! Pure behavior functions.

use rand::Rng;
use rand_distr::{Distribution, Normal};
use spec::StepEntry;

/// Fixed value.
#[must_use]
pub(crate) fn constant(default: f64) -> f64 {
    default
}

/// Gaussian sample around `base`.
pub(crate) fn gaussian_noise<R: Rng>(base: f64, std_dev: f64, rng: &mut R) -> f64 {
    let std_dev = std_dev.max(f64::EPSILON);
    match Normal::new(base, std_dev) {
        Ok(dist) => dist.sample(rng),
        Err(_) => base,
    }
}

/// Sine wave around `center`.
#[must_use]
pub(crate) fn sinusoidal(center: f64, amplitude: f64, period_hours: f64, elapsed_s: f64) -> f64 {
    let period_s = period_hours * 3600.0;
    if period_s == 0.0 {
        return center;
    }
    center + amplitude * (2.0 * std::f64::consts::PI * elapsed_s / period_s).sin()
}

/// Advance a drifting value by `delta` (already `rate × dt`), clamped to `bounds`.
#[must_use]
pub(crate) fn drift_step(current: f64, delta: f64, bounds: (f64, f64)) -> f64 {
    (current + delta).clamp(bounds.0, bounds.1)
}

/// Linear ramp that resets each period.
#[must_use]
pub(crate) fn sawtooth(period_s: f64, min_val: f64, max_val: f64, elapsed_s: f64) -> f64 {
    if period_s <= 0.0 {
        return min_val;
    }
    let progress = elapsed_s.rem_euclid(period_s) / period_s;
    min_val + (max_val - min_val) * progress
}

/// Analog high then low around `center` (50% duty).
#[must_use]
pub(crate) fn square(center: f64, amplitude: f64, period_s: f64, elapsed_s: f64) -> f64 {
    if period_s <= 0.0 {
        return center + amplitude;
    }
    let progress = elapsed_s.rem_euclid(period_s) / period_s;
    if progress < 0.5 {
        center + amplitude
    } else {
        center - amplitude
    }
}

/// Symmetric ramp `min → max → min` in one period.
#[must_use]
pub(crate) fn triangle(period_s: f64, min_val: f64, max_val: f64, elapsed_s: f64) -> f64 {
    if period_s <= 0.0 {
        return min_val;
    }
    let tau = elapsed_s.rem_euclid(period_s) / period_s;
    if tau < 0.5 {
        min_val + (max_val - min_val) * (tau * 2.0)
    } else {
        max_val - (max_val - min_val) * ((tau - 0.5) * 2.0)
    }
}

/// Uniform sample in `[min_val, max_val]`.
pub(crate) fn uniform<R: Rng>(min_val: f64, max_val: f64, rng: &mut R) -> f64 {
    if min_val >= max_val {
        return min_val;
    }
    rng.random_range(min_val..=max_val)
}

/// Walk `values` in order, one index every `dwell_s`, then repeat.
#[must_use]
pub(crate) fn cycle(values: &[f64], dwell_s: f64, elapsed_s: f64) -> f64 {
    let Some(&first) = values.first() else {
        return 0.0;
    };
    if dwell_s <= 0.0 {
        return first;
    }
    let n = values.len() as f64;
    let idx = (elapsed_s / dwell_s).div_euclid(1.0).rem_euclid(n);
    values[(idx as usize).min(values.len() - 1)]
}

/// Last step whose `at` is `<= elapsed_s`, otherwise `default`.
#[must_use]
pub(crate) fn step_value(default: f64, steps: &[StepEntry], elapsed_s: f64) -> f64 {
    let mut value = default;
    let mut ordered = steps.to_vec();
    ordered.sort_by(|a, b| a.at.total_cmp(&b.at));
    for entry in ordered {
        if elapsed_s >= entry.at {
            value = entry.value;
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_passthrough() {
        assert!((constant(22.5) - 22.5).abs() < f64::EPSILON);
    }

    #[test]
    fn sinusoidal_peak() {
        let period_s = 3600.0;
        let peak = sinusoidal(0.0, 5.0, 1.0, period_s / 4.0);
        assert!((peak - 5.0).abs() < 1e-9);
    }

    #[test]
    fn drift_clamps() {
        assert!((drift_step(99.0, 5.0, (0.0, 100.0)) - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn sawtooth_starts_at_min() {
        assert!((sawtooth(10.0, 1.0, 5.0, 0.0) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn square_is_high_then_low() {
        assert!((square(20.0, 5.0, 10.0, 0.0) - 25.0).abs() < f64::EPSILON);
        assert!((square(20.0, 5.0, 10.0, 4.9) - 25.0).abs() < f64::EPSILON);
        assert!((square(20.0, 5.0, 10.0, 5.0) - 15.0).abs() < f64::EPSILON);
        assert!((square(20.0, 5.0, 10.0, 9.9) - 15.0).abs() < f64::EPSILON);
    }

    #[test]
    fn triangle_peaks_at_half_period() {
        assert!((triangle(10.0, 0.0, 10.0, 0.0) - 0.0).abs() < f64::EPSILON);
        assert!((triangle(10.0, 0.0, 10.0, 5.0) - 10.0).abs() < f64::EPSILON);
        assert!((triangle(10.0, 0.0, 10.0, 2.5) - 5.0).abs() < 1e-9);
        assert!((triangle(10.0, 0.0, 10.0, 7.5) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn cycle_walks_then_repeats() {
        let values = [0.0, 10.0, 20.0];
        assert!((cycle(&values, 2.0, 0.0) - 0.0).abs() < f64::EPSILON);
        assert!((cycle(&values, 2.0, 1.9) - 0.0).abs() < f64::EPSILON);
        assert!((cycle(&values, 2.0, 2.0) - 10.0).abs() < f64::EPSILON);
        assert!((cycle(&values, 2.0, 4.0) - 20.0).abs() < f64::EPSILON);
        assert!((cycle(&values, 2.0, 6.0) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn uniform_stays_in_range() {
        use rand::SeedableRng;
        use rand::rngs::StdRng;
        let mut rng = StdRng::seed_from_u64(7);
        for _ in 0..400 {
            let v = uniform(10.0, 20.0, &mut rng);
            assert!((10.0..=20.0).contains(&v), "{v}");
        }
    }

    #[test]
    fn step_holds_last() {
        let steps = vec![
            StepEntry {
                at: 2.0,
                value: 10.0,
            },
            StepEntry {
                at: 5.0,
                value: 20.0,
            },
        ];
        assert!((step_value(0.0, &steps, 0.0) - 0.0).abs() < f64::EPSILON);
        assert!((step_value(0.0, &steps, 2.0) - 10.0).abs() < f64::EPSILON);
        assert!((step_value(0.0, &steps, 6.0) - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn gaussian_noise_is_centered_on_base() {
        use rand::SeedableRng;
        use rand::rngs::StdRng;
        let mut rng = StdRng::seed_from_u64(42);
        let samples: Vec<f64> = (0..4_000)
            .map(|_| gaussian_noise(22.5, 0.3, &mut rng))
            .collect();
        let mean = samples.iter().sum::<f64>() / samples.len() as f64;
        assert!(
            (mean - 22.5).abs() < 0.03,
            "mean {mean} should sit on the operating point, not a shared trend"
        );
    }
}
