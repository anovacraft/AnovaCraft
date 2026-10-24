//! Capability of a process judged by counts: the fraction of defective units
//! (binomial) or the defects per unit (Poisson), with exact confidence
//! intervals (Montgomery, *Introduction to Statistical Quality Control*,
//! ch. 7–8; Clopper & Pearson 1934, *Biometrika* 26; Garwood 1936,
//! *Biometrika* 28).
//!
//! # Binomial
//!
//! With `D = Σdᵢ` defective units among `N = Σnᵢ` inspected, the estimated
//! fraction defective is `p̂ = D/N`, with the Clopper–Pearson interval at
//! confidence `1 − α`
//!
//! ```text
//! lower = B(α/2; D, N − D + 1)        (0 when D = 0)
//! upper = B(1 − α/2; D + 1, N − D)    (1 when D = N)
//! ```
//!
//! where `B(q; a, b)` is the beta quantile. The **process Z** is the
//! standard-normal deviate with the same upper-tail area as the fraction
//! defective, `Z = Φ⁻¹(1 − p̂)`, and its interval maps the interval for `p`
//! (`Z` decreases in `p`, so the upper limit for `p` gives the lower limit
//! for `Z`). It is infinite when no defective unit was seen.
//!
//! # Poisson
//!
//! With `C = Σcᵢ` defects over `U = Σuᵢ` units, the defects per unit are
//! `λ̂ = C/U`, with Garwood's exact interval
//!
//! ```text
//! lower = χ²(α/2; 2C) / 2U            (0 when C = 0)
//! upper = χ²(1 − α/2; 2C + 2) / 2U
//! ```
//!
//! Both studies assume a stable process: the attribute chart beside them in
//! the report shows whether that holds.

use stat_core::discrete::beta_quantile;
use stat_core::distributions::{ChiSquared, ContinuousDistribution, Normal};
use stat_core::{Result, StatError};

/// Binomial capability: fraction of defective units.
#[derive(Debug, Clone, PartialEq)]
pub struct BinomialCapability {
    /// Number of samples.
    pub samples: usize,
    /// Total defective units `D`.
    pub defective: f64,
    /// Total units inspected `N`.
    pub inspected: f64,
    /// `p̂ = D/N`.
    pub fraction: f64,
    /// Clopper–Pearson interval for `p`.
    pub interval: (f64, f64),
    /// Process Z, `Φ⁻¹(1 − p̂)` (`+∞` when `D = 0`).
    pub process_z: f64,
    /// Interval for the process Z.
    pub z_interval: (f64, f64),
    /// Confidence level.
    pub level: f64,
    /// Fraction defective of each sample.
    pub by_sample: Vec<f64>,
    /// Cumulative fraction defective after each sample.
    pub cumulative: Vec<f64>,
}

/// Poisson capability: defects per unit.
#[derive(Debug, Clone, PartialEq)]
pub struct PoissonCapability {
    /// Number of samples.
    pub samples: usize,
    /// Total defects `C`.
    pub defects: f64,
    /// Total units `U`.
    pub units: f64,
    /// Defects per unit `C/U`.
    pub dpu: f64,
    /// Garwood interval for the defects per unit.
    pub interval: (f64, f64),
    /// Confidence level.
    pub level: f64,
    /// Defects per unit of each sample.
    pub by_sample: Vec<f64>,
    /// Cumulative defects per unit after each sample.
    pub cumulative: Vec<f64>,
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn check_level(level: f64) -> Result<()> {
    if level > 0.0 && level < 1.0 { Ok(()) } else { Err(invalid("the confidence level must be between 0 and 1")) }
}

fn cumulative(counts: &[f64], sizes: &[f64]) -> Vec<f64> {
    let (mut c, mut n) = (0.0, 0.0);
    counts
        .iter()
        .zip(sizes)
        .map(|(&x, &m)| {
            c += x;
            n += m;
            c / n
        })
        .collect()
}

/// Clopper–Pearson interval for `d` successes in `n` trials (`n ≥ 1`).
///
/// # Errors
/// [`StatError::InvalidArgument`] for `d > n`, `n = 0` or a bad level.
pub fn clopper_pearson(d: f64, n: f64, level: f64) -> Result<(f64, f64)> {
    check_level(level)?;
    if n < 1.0 || d < 0.0 || d > n {
        return Err(invalid("need 0 ≤ defectives ≤ inspected and at least one unit"));
    }
    let a = 1.0 - level;
    let lower = if d == 0.0 { 0.0 } else { beta_quantile(d, n - d + 1.0, a / 2.0)? };
    let upper = if d == n { 1.0 } else { beta_quantile(d + 1.0, n - d, 1.0 - a / 2.0)? };
    Ok((lower, upper))
}

/// Garwood's exact interval for a Poisson count `c` over exposure `u`, as a rate.
///
/// # Errors
/// [`StatError::InvalidArgument`] for a negative count, non-positive
/// exposure or a bad level.
pub fn garwood(c: f64, u: f64, level: f64) -> Result<(f64, f64)> {
    check_level(level)?;
    if c < 0.0 || u.is_nan() || u <= 0.0 {
        return Err(invalid("need a non-negative count and positive units"));
    }
    let a = 1.0 - level;
    let lower = if c == 0.0 { 0.0 } else { ChiSquared::new(2.0 * c)?.quantile(a / 2.0)? / (2.0 * u) };
    let upper = ChiSquared::new(2.0 * c + 2.0)?.quantile(1.0 - a / 2.0)? / (2.0 * u);
    Ok((lower, upper))
}

/// `Φ⁻¹(1 − p)`, infinite at the ends.
fn z_of(p: f64) -> Result<f64> {
    if p <= 0.0 {
        Ok(f64::INFINITY)
    } else if p >= 1.0 {
        Ok(f64::NEG_INFINITY)
    } else {
        Normal::standard().quantile(1.0 - p)
    }
}

fn check_counts(counts: &[f64], sizes: &[f64], whole_sizes: bool) -> Result<()> {
    if counts.is_empty() {
        return Err(invalid("no samples"));
    }
    if counts.len() != sizes.len() {
        return Err(invalid("one sample size per count is needed"));
    }
    if !counts.iter().all(|&c| c.is_finite() && c >= 0.0 && c.fract() == 0.0) {
        return Err(invalid("counts must be whole, non-negative numbers"));
    }
    let size_ok = |n: f64| n.is_finite() && n > 0.0 && (!whole_sizes || n.fract() == 0.0);
    if !sizes.iter().all(|&n| size_ok(n)) {
        let what = if whole_sizes { "positive whole numbers" } else { "positive" };
        return Err(invalid(format!("sample sizes must be {what}")));
    }
    Ok(())
}

/// Binomial capability of `defective` units among `inspected` per sample.
///
/// # Errors
/// [`StatError::InvalidArgument`] for no samples, counts or sizes that are not
/// whole numbers (sizes positive), more defectives than inspected in a
/// sample, or a bad confidence level.
pub fn binomial_capability(defective: &[f64], inspected: &[f64], level: f64) -> Result<BinomialCapability> {
    check_counts(defective, inspected, true)?;
    check_level(level)?;
    if defective.iter().zip(inspected).any(|(d, n)| d > n) {
        return Err(invalid("a sample has more defective units than units inspected"));
    }
    let d: f64 = defective.iter().sum();
    let n: f64 = inspected.iter().sum();
    let fraction = d / n;
    let interval = clopper_pearson(d, n, level)?;
    Ok(BinomialCapability {
        samples: defective.len(),
        defective: d,
        inspected: n,
        fraction,
        interval,
        process_z: z_of(fraction)?,
        z_interval: (z_of(interval.1)?, z_of(interval.0)?),
        level,
        by_sample: defective.iter().zip(inspected).map(|(d, n)| d / n).collect(),
        cumulative: cumulative(defective, inspected),
    })
}

/// Poisson capability of `defects` over `units` per sample (units need not
/// be whole: e.g. square metres inspected).
///
/// # Errors
/// [`StatError::InvalidArgument`] for no samples, counts that are not whole
/// non-negative numbers, non-positive units, or a bad confidence level.
pub fn poisson_capability(defects: &[f64], units: &[f64], level: f64) -> Result<PoissonCapability> {
    check_counts(defects, units, false)?;
    check_level(level)?;
    let c: f64 = defects.iter().sum();
    let u: f64 = units.iter().sum();
    Ok(PoissonCapability {
        samples: defects.len(),
        defects: c,
        units: u,
        dpu: c / u,
        interval: garwood(c, u, level)?,
        level,
        by_sample: defects.iter().zip(units).map(|(c, u)| c / u).collect(),
        cumulative: cumulative(defects, units),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol * b.abs().max(1.0), "{a} vs {b}");
    }

    /// References: an independent Python computation that inverts the exact
    /// binomial and Poisson tail sums by bisection (see docs/validation.md).
    #[test]
    fn binomial_with_clopper_pearson() {
        let d = [3.0, 1.0, 0.0, 2.0];
        let n = [50.0, 50.0, 40.0, 60.0];
        let b = binomial_capability(&d, &n, 0.95).unwrap();
        assert_eq!(b.samples, 4);
        close(b.fraction, 0.03, 1e-15);
        close(b.interval.0, CP_LOWER, 1e-9);
        close(b.interval.1, CP_UPPER, 1e-9);
        close(b.process_z, 1.880_793_608_151_250_6, 1e-9);
        close(b.z_interval.0, Z_LOWER, 1e-8);
        close(b.z_interval.1, Z_UPPER, 1e-8);
        close(b.cumulative[1], 0.04, 1e-15);
        close(b.by_sample[3], 2.0 / 60.0, 1e-15);
        // No defectives: the upper limit is 1 − (α/2)^(1/N); Z is infinite.
        let z = binomial_capability(&[0.0, 0.0], &[30.0, 20.0], 0.95).unwrap();
        assert_eq!(z.interval.0, 0.0);
        close(z.interval.1, 1.0 - 0.025f64.powf(1.0 / 50.0), 1e-10);
        assert!(z.process_z.is_infinite() && z.z_interval.1.is_infinite());
        assert!(binomial_capability(&[6.0], &[5.0], 0.95).is_err());
        assert!(binomial_capability(&[1.5], &[5.0], 0.95).is_err());
        assert!(binomial_capability(&[1.0], &[5.0], 1.0).is_err());
    }

    #[test]
    fn poisson_with_garwood() {
        let c = [4.0, 7.0, 2.0];
        let u = [10.0, 12.5, 7.5];
        let p = poisson_capability(&c, &u, 0.95).unwrap();
        close(p.dpu, 13.0 / 30.0, 1e-15);
        close(p.interval.0, G_LOWER, 1e-9);
        close(p.interval.1, G_UPPER, 1e-9);
        close(p.cumulative[1], 11.0 / 22.5, 1e-15);
        // No defects: upper limit −ln(α/2)/U.
        let z = poisson_capability(&[0.0], &[10.0], 0.95).unwrap();
        assert_eq!(z.interval.0, 0.0);
        close(z.interval.1, -(0.025f64.ln()) / 10.0, 1e-10);
        assert!(poisson_capability(&[1.0], &[0.0], 0.95).is_err());
    }

    const CP_LOWER: f64 = 0.011_087_455_672_254_72;
    const CP_UPPER: f64 = 0.064_150_626_705_026_97;
    const Z_LOWER: f64 = 1.520_834_981_791_745;
    const Z_UPPER: f64 = 2.287_358_416_922_769_4;
    const G_LOWER: f64 = 0.230_731_749_700_126_9;
    const G_UPPER: f64 = 0.741_013_197_271_962_6;
}
