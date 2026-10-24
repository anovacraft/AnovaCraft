//! Time-weighted control charts, which accumulate information across samples
//! and so detect small sustained shifts sooner than Shewhart charts
//! (Montgomery, *Introduction to Statistical Quality Control*, ch. 9; NIST/SEMATECH
//! e-Handbook §6.3.2.3–6.3.2.4).
//!
//! * **EWMA** (Roberts 1959): `zᵢ = λxᵢ + (1 − λ)zᵢ₋₁`, `z₀ = μ₀`, with limits
//!   `μ₀ ± Lσ √(λ/(2 − λ) · [1 − (1 − λ)^{2i}])` that widen to their
//!   steady-state value.
//! * **Tabular CUSUM** (Page 1954): `Cᵢ⁺ = max(0, xᵢ − (μ₀ + K) + Cᵢ₋₁⁺)` and
//!   `Cᵢ⁻ = max(0, (μ₀ − K) − xᵢ + Cᵢ₋₁⁻)`, with reference value `K = kσ` and
//!   decision interval `H = hσ`. Each signal also reports the number of
//!   consecutive non-zero sums, which estimates when the shift began.
//!
//! When `σ` is not given it is estimated from the average moving range,
//! `σ̂ = MR̄ / d₂` with `d₂ = 2/√π ≈ 1.128` for spans of two.

use stat_core::{Result, StatError};

/// `d₂` for moving ranges of span two: `2/√π`.
const D2_SPAN_TWO: f64 = std::f64::consts::FRAC_2_SQRT_PI;

/// In-control process mean and standard deviation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcessTarget {
    /// Target (in-control) mean `μ₀`.
    pub mean: f64,
    /// In-control standard deviation `σ`.
    pub sigma: f64,
}

/// One point of an EWMA chart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EwmaPoint {
    /// The EWMA statistic `zᵢ`.
    pub statistic: f64,
    /// Lower control limit at this sample.
    pub lower: f64,
    /// Upper control limit at this sample.
    pub upper: f64,
    /// `zᵢ` lies outside its limits.
    pub signal: bool,
}

/// An EWMA chart.
#[derive(Debug, Clone, PartialEq)]
pub struct EwmaChart {
    /// Target mean and standard deviation used.
    pub target: ProcessTarget,
    /// Smoothing weight `λ`.
    pub lambda: f64,
    /// Limit width `L` (in steady-state standard deviations of `z`).
    pub width: f64,
    /// One point per observation.
    pub points: Vec<EwmaPoint>,
}

/// One sample of a tabular CUSUM.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CusumPoint {
    /// Upper cumulative sum `Cᵢ⁺`.
    pub upper: f64,
    /// Lower cumulative sum `Cᵢ⁻` (reported as a non-negative number).
    pub lower: f64,
    /// Consecutive samples with `C⁺ > 0` up to this one.
    pub upper_run: usize,
    /// Consecutive samples with `C⁻ > 0` up to this one.
    pub lower_run: usize,
    /// `Cᵢ⁺ > H`: an upward shift.
    pub upper_signal: bool,
    /// `Cᵢ⁻ > H`: a downward shift.
    pub lower_signal: bool,
}

/// A tabular CUSUM chart.
#[derive(Debug, Clone, PartialEq)]
pub struct CusumChart {
    /// Target mean and standard deviation used.
    pub target: ProcessTarget,
    /// Reference value `K = kσ` (in data units).
    pub reference: f64,
    /// Decision interval `H = hσ` (in data units).
    pub decision: f64,
    /// One point per observation.
    pub points: Vec<CusumPoint>,
}

impl CusumChart {
    /// Estimated process mean after the first upward or downward signal:
    /// `μ₀ + K + C⁺/N⁺` or `μ₀ − K − C⁻/N⁻` (Montgomery §9.1.2), with the
    /// index of that signal. `None` if the chart never signals.
    #[must_use]
    pub fn shift_estimate(&self) -> Option<(usize, f64)> {
        self.points.iter().enumerate().find_map(|(i, p)| {
            if p.upper_signal {
                Some((i, self.target.mean + self.reference + p.upper / p.upper_run as f64))
            } else if p.lower_signal {
                Some((i, self.target.mean - self.reference - p.lower / p.lower_run as f64))
            } else {
                None
            }
        })
    }
}

/// `σ̂ = MR̄ / d₂` from the moving ranges of consecutive observations.
///
/// # Errors
/// [`StatError::InvalidArgument`] with fewer than two observations or a
/// non-finite value.
pub fn moving_range_sigma(values: &[f64]) -> Result<f64> {
    check_values(values)?;
    if values.len() < 2 {
        return Err(invalid("at least two observations are needed to estimate σ from moving ranges"));
    }
    let mr: f64 = values.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>() / (values.len() - 1) as f64;
    Ok(mr / D2_SPAN_TWO)
}

/// The target to use: `target` if given, otherwise the sample mean and the
/// moving-range estimate of `σ`.
///
/// # Errors
/// As [`moving_range_sigma`]; also a non-positive or non-finite `σ`.
pub fn resolve_target(values: &[f64], target: Option<ProcessTarget>) -> Result<ProcessTarget> {
    let t = match target {
        Some(t) => t,
        None => ProcessTarget {
            mean: values.iter().sum::<f64>() / values.len().max(1) as f64,
            sigma: moving_range_sigma(values)?,
        },
    };
    if !(t.mean.is_finite() && t.sigma.is_finite() && t.sigma > 0.0) {
        return Err(invalid("the target mean must be finite and σ positive"));
    }
    Ok(t)
}

/// EWMA chart of individual observations.
///
/// # Errors
/// [`StatError::InvalidArgument`] for no data, a non-finite value, `λ` outside
/// `(0, 1]`, a non-positive `L`, or a target that cannot be estimated.
pub fn ewma_chart(values: &[f64], target: Option<ProcessTarget>, lambda: f64, width: f64) -> Result<EwmaChart> {
    check_values(values)?;
    if !(lambda > 0.0 && lambda <= 1.0) {
        return Err(invalid("the EWMA weight λ must be in (0, 1]"));
    }
    if !(width > 0.0 && width.is_finite()) {
        return Err(invalid("the limit width L must be positive"));
    }
    let t = resolve_target(values, target)?;
    let mut z = t.mean;
    let mut decay = 1.0; // (1 − λ)^{2i}
    let points = values
        .iter()
        .map(|&x| {
            z = lambda * x + (1.0 - lambda) * z;
            decay *= (1.0 - lambda) * (1.0 - lambda);
            let half = width * t.sigma * (lambda / (2.0 - lambda) * (1.0 - decay)).sqrt();
            let (lower, upper) = (t.mean - half, t.mean + half);
            EwmaPoint { statistic: z, lower, upper, signal: z < lower || z > upper }
        })
        .collect();
    Ok(EwmaChart { target: t, lambda, width, points })
}

/// Tabular CUSUM of individual observations with `K = kσ`, `H = hσ`.
///
/// # Errors
/// [`StatError::InvalidArgument`] for no data, a non-finite value, a negative
/// `k`, a non-positive `h`, or a target that cannot be estimated.
pub fn cusum_chart(values: &[f64], target: Option<ProcessTarget>, k: f64, h: f64) -> Result<CusumChart> {
    check_values(values)?;
    if !(k >= 0.0 && k.is_finite()) {
        return Err(invalid("the reference value k must be non-negative"));
    }
    if !(h > 0.0 && h.is_finite()) {
        return Err(invalid("the decision interval h must be positive"));
    }
    let t = resolve_target(values, target)?;
    let (reference, decision) = (k * t.sigma, h * t.sigma);
    let (mut up, mut down, mut up_run, mut down_run) = (0.0_f64, 0.0_f64, 0, 0);
    let points = values
        .iter()
        .map(|&x| {
            up = (x - (t.mean + reference) + up).max(0.0);
            down = ((t.mean - reference) - x + down).max(0.0);
            up_run = if up > 0.0 { up_run + 1 } else { 0 };
            down_run = if down > 0.0 { down_run + 1 } else { 0 };
            CusumPoint {
                upper: up,
                lower: down,
                upper_run: up_run,
                lower_run: down_run,
                upper_signal: up > decision,
                lower_signal: down > decision,
            }
        })
        .collect();
    Ok(CusumChart { target: t, reference, decision, points })
}

fn check_values(values: &[f64]) -> Result<()> {
    if values.is_empty() {
        return Err(invalid("no observations"));
    }
    if values.iter().any(|v| !v.is_finite()) {
        return Err(invalid("observations must be finite"));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Montgomery, *Introduction to Statistical Quality Control*, Example 9.1
    /// (Table 9.1): 20 observations from N(10, 1) followed by 10 from N(11, 1).
    const SHIFTED: [f64; 30] = [
        9.45, 7.99, 9.29, 11.66, 12.16, 10.18, 8.04, 11.46, 9.20, 10.34, 9.03, 11.47, 10.51, 9.40, 10.08, 9.37, 10.62,
        10.31, 8.52, 10.84, 10.90, 9.33, 12.29, 11.50, 10.60, 11.08, 10.38, 11.62, 11.31, 10.52,
    ];
    const TARGET: ProcessTarget = ProcessTarget { mean: 10.0, sigma: 1.0 };

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol, "{a} vs {b}");
    }

    #[test]
    fn cusum_reproduces_the_published_table() {
        let c = cusum_chart(&SHIFTED, Some(TARGET), 0.5, 5.0).unwrap();
        // Published: C⁺ = 3.35, 4.47, 5.28, 5.30 at samples 27–30; first signal at
        // sample 29 with N⁺ = 7, so the shift began after sample 22.
        for (i, e) in [(26, 3.35), (27, 4.47), (28, 5.28), (29, 5.30)] {
            close(c.points[i].upper, e, 1e-9);
        }
        let first = c.points.iter().position(|p| p.upper_signal).unwrap();
        assert_eq!(first, 28);
        assert_eq!(c.points[28].upper_run, 7);
        assert!(c.points.iter().all(|p| !p.lower_signal));
        let (at, mean) = c.shift_estimate().unwrap();
        assert_eq!(at, 28);
        close(mean, 10.0 + 0.5 + 5.28 / 7.0, 1e-9); // ≈ 11.25
        assert_eq!((c.reference, c.decision), (0.5, 5.0));
    }

    #[test]
    fn ewma_matches_direct_evaluation() {
        // λ = 0.1, L = 2.7 (Montgomery Example 9.2), values from NumPy.
        let e = ewma_chart(&SHIFTED, Some(TARGET), 0.1, 2.7).unwrap();
        close(e.points[0].statistic, 9.945, 1e-12);
        close(e.points[0].upper, 10.27, 1e-12);
        close(e.points[1].upper, 10.363_247_849_270_99, 1e-12);
        close(e.points[9].statistic, 10.023_159_729_995_005, 1e-12);
        close(e.points[27].statistic, 10.573_137_171_751_515, 1e-12);
        close(e.points[28].statistic, 10.646_823_454_576_364, 1e-12);
        close(e.points[28].upper, 10.618_734_995_875_531, 1e-12);
        let first = e.points.iter().position(|p| p.signal).unwrap();
        assert_eq!(first, 28, "first signal at sample 29");
        // Limits approach the steady state Lσ√(λ/(2 − λ)).
        let steady = 10.0 + 2.7 * (0.1_f64 / 1.9).sqrt();
        assert!(e.points.windows(2).all(|w| w[1].upper >= w[0].upper) && e.points[29].upper < steady);
        // λ = 1 is a Shewhart individuals chart with limits ±L σ.
        let s = ewma_chart(&SHIFTED, Some(TARGET), 1.0, 3.0).unwrap();
        assert!(s.points.iter().zip(&SHIFTED).all(|(p, x)| p.statistic == *x && (p.upper - 13.0).abs() < 1e-12));
    }

    #[test]
    fn estimated_targets_and_invalid_input() {
        // σ̂ = MR̄/d₂: moving ranges of [1, 3, 2, 6] are 2, 1, 4 → MR̄ = 7/3.
        close(moving_range_sigma(&[1.0, 3.0, 2.0, 6.0]).unwrap(), 7.0 / 3.0 / D2_SPAN_TWO, 1e-14);
        let t = resolve_target(&[1.0, 3.0, 2.0, 6.0], None).unwrap();
        close(t.mean, 3.0, 1e-14);
        assert!(ewma_chart(&[], None, 0.2, 3.0).is_err());
        assert!(ewma_chart(&SHIFTED, None, 0.0, 3.0).is_err());
        assert!(ewma_chart(&SHIFTED, None, 0.2, 0.0).is_err());
        assert!(cusum_chart(&SHIFTED, None, -0.5, 5.0).is_err());
        assert!(cusum_chart(&SHIFTED, None, 0.5, 0.0).is_err());
        assert!(cusum_chart(&[5.0], None, 0.5, 5.0).is_err(), "σ cannot be estimated from one value");
        assert!(cusum_chart(&[1.0, f64::NAN], Some(TARGET), 0.5, 5.0).is_err());
        assert!(cusum_chart(&[1.0, 1.0, 1.0], None, 0.5, 5.0).is_err(), "σ̂ = 0");
    }
}
