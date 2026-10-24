//! Tests of one standard deviation and of a ratio of two variances, assuming
//! normal data (Montgomery & Runger, §§9-4, 10-5).
//!
//! * **One standard deviation:** `χ² = (n − 1)s²/σ₀²` on `n − 1` df; the
//!   interval for `σ` is `(√((n−1)s²/χ²_{1−α/2}), √((n−1)s²/χ²_{α/2}))`.
//! * **Variance ratio:** `F = (s₁²/s₂²)/ρ₀` on `(n₁ − 1, n₂ − 1)` df; the
//!   interval for `σ₁²/σ₂²` is `(F̂/F_{1−α/2}, F̂/F_{α/2})` with `F̂ = s₁²/s₂²`.
//!
//! Both are sensitive to non-normality; the Levene and Brown–Forsythe tests in
//! `stat-linear` are the robust alternatives for comparing groups.

use crate::common::{Alternative, TestResult, check_finite, check_level, invalid, mean_var};
use stat_core::distributions::{ChiSquared, ContinuousDistribution, FisherSnedecor};
use stat_core::{Result, StatError};

/// Chi-square test that the standard deviation equals `sigma0`; the estimate
/// and interval are for the standard deviation.
///
/// # Errors
/// [`StatError::InsufficientData`] for fewer than two values;
/// [`StatError::InvalidArgument`] for `σ₀ ≤ 0`, constant data or a bad level.
pub fn one_sample_std_dev(x: &[f64], sigma0: f64, level: f64, alternative: Alternative) -> Result<TestResult> {
    check_finite(x, "sample values")?;
    check_level(level)?;
    if x.len() < 2 {
        return Err(StatError::InsufficientData { required: 2, found: x.len() });
    }
    if !(sigma0 > 0.0 && sigma0.is_finite()) {
        return Err(invalid(format!("the hypothesized standard deviation must be positive (got {sigma0})")));
    }
    let (_, v) = mean_var(x);
    if v == 0.0 {
        return Err(invalid("the data have no variation"));
    }
    let df = x.len() as f64 - 1.0;
    let chi = ChiSquared::new(df)?;
    let stat = df * v / (sigma0 * sigma0);
    let tail = alternative.tail(level);
    let lo = (df * v / chi.quantile(1.0 - tail)?).sqrt();
    let hi = (df * v / chi.quantile(tail)?).sqrt();
    let (lo, hi) = alternative.bound(lo, hi);
    Ok(TestResult {
        method: "Chi-square test of a standard deviation",
        estimate: v.sqrt(),
        null_value: sigma0,
        std_error: f64::NAN,
        statistic: stat,
        statistic_name: "χ²",
        df: Some(df),
        df2: None,
        p_value: alternative.p_value(chi.cdf(stat)?, chi.sf(stat)?),
        // A standard deviation is non-negative: `Less` gives `[0, upper]`.
        confidence_interval: (lo.max(0.0), hi.max(lo)),
        level,
        alternative,
    })
}

/// F test that `var(a)/var(b) = ratio0`; the estimate and interval are for
/// the variance ratio.
///
/// # Errors
/// [`StatError::InsufficientData`] if a sample has fewer than two values;
/// [`StatError::InvalidArgument`] for zero variance, `ratio0 ≤ 0` or a bad level.
pub fn variance_ratio_test(
    a: &[f64],
    b: &[f64],
    ratio0: f64,
    level: f64,
    alternative: Alternative,
) -> Result<TestResult> {
    check_finite(a, "sample values")?;
    check_finite(b, "sample values")?;
    check_level(level)?;
    for s in [a, b] {
        if s.len() < 2 {
            return Err(StatError::InsufficientData { required: 2, found: s.len() });
        }
    }
    if !(ratio0 > 0.0 && ratio0.is_finite()) {
        return Err(invalid(format!("the hypothesized variance ratio must be positive (got {ratio0})")));
    }
    let ((_, v1), (_, v2)) = (mean_var(a), mean_var(b));
    if v1 == 0.0 || v2 == 0.0 {
        return Err(invalid("a sample has no variation"));
    }
    let (d1, d2) = (a.len() as f64 - 1.0, b.len() as f64 - 1.0);
    let f = FisherSnedecor::new(d1, d2)?;
    let ratio = v1 / v2;
    let stat = ratio / ratio0;
    let tail = alternative.tail(level);
    let (lo, hi) = alternative.bound(ratio / f.quantile(1.0 - tail)?, ratio / f.quantile(tail)?);
    Ok(TestResult {
        method: "F test of a variance ratio",
        estimate: ratio,
        null_value: ratio0,
        std_error: f64::NAN,
        statistic: stat,
        statistic_name: "F",
        df: Some(d1),
        df2: Some(d2),
        p_value: alternative.p_value(f.cdf(stat)?, f.sf(stat)?),
        // A variance ratio is non-negative: `Less` gives `[0, upper]`.
        confidence_interval: (lo.max(0.0), hi),
        level,
        alternative,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::means::tests::{SLEEP_1, SLEEP_2, X, close};

    /// Chi-square and F probabilities and quantiles from SciPy 1.18.
    #[test]
    fn variance_tests() {
        let s = one_sample_std_dev(&X, 0.5, 0.95, Alternative::TwoSided).unwrap();
        close(s.statistic, 6.8, 1e-12);
        close(s.p_value, 0.684_133_387_572_742_2, 1e-10);
        close(s.confidence_interval.0, 0.298_942_462_281_409_3, 1e-10);
        close(s.confidence_interval.1, 0.793_434_819_522_936_8, 1e-10);
        let f = variance_ratio_test(&SLEEP_1, &SLEEP_2, 1.0, 0.95, Alternative::TwoSided).unwrap();
        close(f.statistic, 0.798_342_617_998_392_3, 1e-12);
        close(f.p_value, 0.742_719_931_726_045_3, 1e-10);
        close(f.confidence_interval.0, 0.198_297_013_510_539_4, 1e-10);
        close(f.confidence_interval.1, 3.214_122_716_369_867, 1e-10);
        assert_eq!((f.df, f.df2), (Some(9.0), Some(9.0)));
        assert!(one_sample_std_dev(&X, 0.0, 0.95, Alternative::TwoSided).is_err());
    }

    /// A one-sided `Less` interval for σ or a variance ratio is `[0, upper]`;
    /// the σ upper limit is √(3s²/χ²₀.₀₅,₃) (SciPy 1.18.1 `chi2.ppf`).
    #[test]
    fn less_intervals_start_at_zero() {
        let s = one_sample_std_dev(&[1.0, 2.0, 4.0, 7.0], 1.0, 0.95, Alternative::Less).unwrap();
        assert_eq!(s.confidence_interval.0, 0.0);
        close(s.confidence_interval.1, 7.725_616_433_936_511, 1e-10);
        let f = variance_ratio_test(&SLEEP_1, &SLEEP_2, 1.0, 0.95, Alternative::Less).unwrap();
        assert_eq!(f.confidence_interval.0, 0.0);
        assert!(f.confidence_interval.1.is_finite());
    }
}
