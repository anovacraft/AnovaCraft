//! Equivalence tests: two one-sided tests (TOST; Schuirmann 1987, *J.
//! Pharmacokinet. Biopharm.* 15:657).
//!
//! Equivalence within `(L, U)` is concluded at level `α` when both
//! `H₀₁: θ ≤ L` and `H₀₂: θ ≥ U` are rejected by one-sided t tests:
//!
//! ```text
//! t_L = (θ̂ − L)/SE,  p_L = P(T ≥ t_L);   t_U = (θ̂ − U)/SE,  p_U = P(T ≤ t_U).
//! ```
//!
//! The overall p-value is `max(p_L, p_U)`; equivalently the `1 − 2α`
//! confidence interval `θ̂ ± t_{1−α}·SE` lies inside `(L, U)`.

use crate::common::{check_level, invalid};
use crate::means::{paired_differences, two_sample_parts};
use stat_core::distributions::{ContinuousDistribution, StudentsT};
use stat_core::{Result, StatError};

/// Result of a TOST equivalence test.
#[derive(Debug, Clone, PartialEq)]
pub struct EquivalenceTest {
    /// Name of the test.
    pub method: &'static str,
    /// Estimate of the mean or difference.
    pub estimate: f64,
    /// Lower equivalence bound `L`.
    pub lower_bound: f64,
    /// Upper equivalence bound `U`.
    pub upper_bound: f64,
    /// Standard error of the estimate.
    pub std_error: f64,
    /// Degrees of freedom.
    pub df: f64,
    /// `t` against the lower bound.
    pub t_lower: f64,
    /// p-value of `H₀: θ ≤ L`.
    pub p_lower: f64,
    /// `t` against the upper bound.
    pub t_upper: f64,
    /// p-value of `H₀: θ ≥ U`.
    pub p_upper: f64,
    /// `max(p_lower, p_upper)`.
    pub p_value: f64,
    /// The `1 − 2α` confidence interval.
    pub confidence_interval: (f64, f64),
    /// Significance level `α` of each one-sided test.
    pub alpha: f64,
}

impl EquivalenceTest {
    /// `true` when equivalence is concluded at level `α`.
    #[must_use]
    pub fn equivalent(&self) -> bool {
        self.p_value < self.alpha
    }
}

/// TOST from an estimate, its standard error and degrees of freedom (used by
/// the paired, two-sample and crossover tests).
///
/// # Errors
/// `lower ≥ upper`, non-finite bounds, `α ∉ (0, 0.5)` or a zero standard error.
pub fn tost_from_parts(
    method: &'static str,
    estimate: f64,
    std_error: f64,
    df: f64,
    bounds: (f64, f64),
    alpha: f64,
) -> Result<EquivalenceTest> {
    let (lower, upper) = bounds;
    if lower >= upper || !lower.is_finite() || !upper.is_finite() {
        return Err(invalid(format!("equivalence bounds need lower < upper (got {lower}, {upper})")));
    }
    if !(alpha > 0.0 && alpha < 0.5) {
        return Err(invalid(format!("α must lie in (0, 0.5) (got {alpha})")));
    }
    check_level(1.0 - 2.0 * alpha)?;
    if !(std_error > 0.0 && std_error.is_finite()) {
        return Err(invalid("the standard error is zero: the data have no variation"));
    }
    let t = StudentsT::new(df)?;
    let t_lower = (estimate - lower) / std_error;
    let t_upper = (estimate - upper) / std_error;
    let p_lower = t.sf(t_lower)?;
    let p_upper = t.cdf(t_upper)?;
    let crit = t.quantile(1.0 - alpha)?;
    Ok(EquivalenceTest {
        method,
        estimate,
        lower_bound: lower,
        upper_bound: upper,
        std_error,
        df,
        t_lower,
        p_lower,
        t_upper,
        p_upper,
        p_value: p_lower.max(p_upper),
        confidence_interval: (estimate - crit * std_error, estimate + crit * std_error),
        alpha,
    })
}

/// TOST for one mean lying within `bounds`.
///
/// # Errors
/// Fewer than two values, constant data, `lower ≥ upper`, or `α ∉ (0, 0.5)`.
pub fn tost_one_sample(x: &[f64], bounds: (f64, f64), alpha: f64) -> Result<EquivalenceTest> {
    if x.len() < 2 {
        return Err(StatError::InsufficientData { required: 2, found: x.len() });
    }
    crate::common::check_finite(x, "sample values")?;
    let (m, v) = crate::common::mean_var(x);
    let n = x.len() as f64;
    tost_from_parts("One-sample equivalence (TOST)", m, (v / n).sqrt(), n - 1.0, bounds, alpha)
}

/// TOST for the mean paired difference `a − b`.
///
/// # Errors
/// As [`tost_one_sample`], plus unequal lengths.
pub fn tost_paired(a: &[f64], b: &[f64], bounds: (f64, f64), alpha: f64) -> Result<EquivalenceTest> {
    let d = paired_differences(a, b)?;
    let mut r = tost_one_sample(&d, bounds, alpha)?;
    r.method = "Paired equivalence (TOST)";
    Ok(r)
}

/// TOST for the ratio of paired measurements `a/b` (ratio of geometric
/// means): the paired test on `ln a − ln b` with bounds `ln L`, `ln U`. The
/// estimate and interval stay on the log scale; `exp` gives the ratio.
///
/// # Errors
/// As [`tost_paired`], plus non-positive values or ratio bounds.
pub fn tost_paired_ratio(a: &[f64], b: &[f64], bounds: (f64, f64), alpha: f64) -> Result<EquivalenceTest> {
    if a.iter().chain(b).any(|&v| v.is_nan() || v <= 0.0) {
        return Err(invalid("the ratio scale needs positive values (they are log-transformed)"));
    }
    if !(bounds.0 > 0.0 && bounds.1 > 0.0) {
        return Err(invalid("ratio equivalence bounds must be positive"));
    }
    let la: Vec<f64> = a.iter().map(|v| v.ln()).collect();
    let lb: Vec<f64> = b.iter().map(|v| v.ln()).collect();
    let mut r = tost_paired(&la, &lb, (bounds.0.ln(), bounds.1.ln()), alpha)?;
    r.method = "Paired equivalence of the ratio (TOST, log scale)";
    Ok(r)
}

/// TOST for `mean(a) − mean(b)`, pooled or Welch.
///
/// # Errors
/// As [`crate::means::two_sample_t`], plus invalid bounds or `α`.
pub fn tost_two_sample(
    a: &[f64],
    b: &[f64],
    bounds: (f64, f64),
    equal_variances: bool,
    alpha: f64,
) -> Result<EquivalenceTest> {
    let (est, se, df) = two_sample_parts(a, b, equal_variances)?;
    let method =
        if equal_variances { "Two-sample equivalence (TOST, pooled)" } else { "Two-sample equivalence (TOST, Welch)" };
    tost_from_parts(method, est, se, df, bounds, alpha)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::means::tests::{SLEEP_1, SLEEP_2, X, close};

    /// One-sided t probabilities evaluated independently with SciPy 1.18.
    #[test]
    fn one_sample_tost() {
        let e = tost_one_sample(&X, (4.8, 5.8), 0.05).unwrap();
        close(e.p_lower, 0.000_904_267_805_236_497_8, 1e-9);
        close(e.p_upper, 0.008_649_604_387_763_412, 1e-9);
        close(e.p_value, e.p_upper, 0.0);
        close(e.confidence_interval.0, 5.148_062_725_165_753, 1e-12);
        close(e.confidence_interval.1, 5.651_937_274_834_246, 1e-12);
        assert!(e.equivalent());
        assert!(!tost_one_sample(&X, (5.0, 5.6), 0.05).unwrap().equivalent());
        assert!(tost_one_sample(&X, (5.0, 5.0), 0.05).is_err());
    }

    #[test]
    fn paired_and_two_sample_agree_with_t_tests() {
        // The TOST p against a bound equals the one-sided t test at that bound.
        let e = tost_paired(&SLEEP_1, &SLEEP_2, (-2.0, 2.0), 0.05).unwrap();
        let t = crate::means::paired_t(&SLEEP_1, &SLEEP_2, -2.0, 0.95, crate::Alternative::Greater).unwrap();
        close(e.p_lower, t.p_value, 1e-12);
        let w = tost_two_sample(&SLEEP_1, &SLEEP_2, (-3.0, 3.0), false, 0.05).unwrap();
        let t = crate::means::two_sample_t(&SLEEP_1, &SLEEP_2, 3.0, false, 0.95, crate::Alternative::Less).unwrap();
        close(w.p_upper, t.p_value, 1e-12);
        assert!(!w.equivalent(), "the sleep difference is too uncertain for ±3");
    }

    /// The ratio test is the difference test on logs with log bounds.
    #[test]
    fn paired_ratio_is_the_log_difference_test() {
        let a = [1.2, 2.3, 1.9, 3.1, 2.2, 1.7];
        let b = [1.1, 2.4, 1.7, 3.0, 2.1, 1.8];
        let r = tost_paired_ratio(&a, &b, (0.8, 1.25), 0.05).unwrap();
        let la: Vec<f64> = a.iter().map(|v: &f64| v.ln()).collect();
        let lb: Vec<f64> = b.iter().map(|v: &f64| v.ln()).collect();
        let d = tost_paired(&la, &lb, (0.8f64.ln(), 1.25f64.ln()), 0.05).unwrap();
        close(r.p_value, d.p_value, 1e-14);
        close(r.estimate, d.estimate, 1e-14);
        // ln(geometric mean ratio) = mean of ln(a/b).
        let gm = la.iter().zip(&lb).map(|(x, y)| x - y).sum::<f64>() / 6.0;
        close(r.estimate, gm, 1e-14);
        assert!(tost_paired_ratio(&[1.0, -1.0], &[1.0, 1.0], (0.8, 1.25), 0.05).is_err());
        assert!(tost_paired_ratio(&a, &b, (-0.2, 0.2), 0.05).is_err());
    }
}
