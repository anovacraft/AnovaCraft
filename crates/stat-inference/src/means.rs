//! Tests and intervals for means.
//!
//! * **One-sample t:** `t = (x̄ − μ₀)/(s/√n)` on `n − 1` df.
//! * **One-sample z** (known σ): `z = (x̄ − μ₀)/(σ/√n)`.
//! * **Paired t:** the one-sample t applied to the differences `dᵢ = aᵢ − bᵢ`.
//! * **Two-sample t, pooled:** `t = (ā − b̄ − Δ₀)/(s_p√(1/n₁ + 1/n₂))` on
//!   `n₁ + n₂ − 2` df, `s_p² = ((n₁−1)s₁² + (n₂−1)s₂²)/(n₁ + n₂ − 2)`.
//! * **Two-sample t, Welch:** standard error `√(s₁²/n₁ + s₂²/n₂)` on the
//!   Welch–Satterthwaite df (Welch 1947, *Biometrika* 34).
//!
//! Confidence intervals are `estimate ± t_{1−α/2}·SE` (two-sided) or one-sided
//! bounds with `t_{1−α}` (e.g. Montgomery & Runger, *Applied Statistics and
//! Probability for Engineers*, chs. 8–10).
//!
//! # Effect sizes
//! Cohen's `d = (ā − b̄)/s_p`; Hedges' `g = d·(1 − 3/(4(n₁ + n₂) − 9))`
//! (Hedges 1981); for paired data `d_z = d̄/s_d` (Cohen 1988, §2.3.5).

use crate::common::{Alternative, TestResult, check_finite, check_level, invalid, mean_var};
use stat_core::distributions::{ContinuousDistribution, Normal, StudentsT};
use stat_core::{Result, StatError};

/// A location test from an estimate, its standard error and t degrees of
/// freedom (`df = ∞` gives the normal z test).
#[allow(clippy::too_many_arguments)]
pub(crate) fn location_test(
    method: &'static str,
    estimate: f64,
    null_value: f64,
    std_error: f64,
    df: f64,
    level: f64,
    alternative: Alternative,
) -> Result<TestResult> {
    check_level(level)?;
    if !(std_error > 0.0 && std_error.is_finite()) {
        return Err(invalid("the standard error is zero: the data have no variation"));
    }
    let stat = (estimate - null_value) / std_error;
    let tail = alternative.tail(level);
    let (lower_tail, upper_tail, crit, name) = if df.is_infinite() {
        let n = Normal::standard();
        (n.cdf(stat)?, n.sf(stat)?, n.quantile(1.0 - tail)?, "z")
    } else {
        let t = StudentsT::new(df)?;
        (t.cdf(stat)?, t.sf(stat)?, t.quantile(1.0 - tail)?, "t")
    };
    let (lo, hi) = alternative.bound(estimate - crit * std_error, estimate + crit * std_error);
    Ok(TestResult {
        method,
        estimate,
        null_value,
        std_error,
        statistic: stat,
        statistic_name: name,
        df: df.is_finite().then_some(df),
        df2: None,
        p_value: alternative.p_value(lower_tail, upper_tail),
        confidence_interval: (lo, hi),
        level,
        alternative,
    })
}

fn check_sample(x: &[f64], min: usize) -> Result<()> {
    check_finite(x, "sample values")?;
    if x.len() < min {
        return Err(StatError::InsufficientData { required: min, found: x.len() });
    }
    Ok(())
}

/// One-sample t test of `mean = μ₀`.
///
/// # Errors
/// [`StatError::InsufficientData`] for fewer than two values,
/// [`StatError::InvalidArgument`] for constant data or a bad level,
/// [`StatError::NonFiniteInput`] for NaN/±∞.
pub fn one_sample_t(x: &[f64], mu0: f64, level: f64, alternative: Alternative) -> Result<TestResult> {
    check_sample(x, 2)?;
    let (m, v) = mean_var(x);
    let n = x.len() as f64;
    location_test("One-sample t", m, mu0, (v / n).sqrt(), n - 1.0, level, alternative)
}

/// One-sample z test of `mean = μ₀` with known standard deviation `σ`.
///
/// # Errors
/// As [`one_sample_t`] (one value suffices), plus `σ ≤ 0`.
pub fn one_sample_z(x: &[f64], mu0: f64, sigma: f64, level: f64, alternative: Alternative) -> Result<TestResult> {
    check_sample(x, 1)?;
    if !(sigma > 0.0 && sigma.is_finite()) {
        return Err(invalid(format!("the known standard deviation must be positive (got {sigma})")));
    }
    let (m, _) = mean_var(x);
    let se = sigma / (x.len() as f64).sqrt();
    location_test("One-sample z (known σ)", m, mu0, se, f64::INFINITY, level, alternative)
}

/// Differences `a − b` of paired observations.
///
/// # Errors
/// [`StatError::DimensionMismatch`] for unequal lengths.
pub fn paired_differences(a: &[f64], b: &[f64]) -> Result<Vec<f64>> {
    if a.len() != b.len() {
        return Err(StatError::DimensionMismatch { context: "paired samples", expected: a.len(), found: b.len() });
    }
    Ok(a.iter().zip(b).map(|(x, y)| x - y).collect())
}

/// Paired t test of `mean(a − b) = Δ₀`.
///
/// # Errors
/// As [`one_sample_t`] on the differences, plus unequal lengths.
pub fn paired_t(a: &[f64], b: &[f64], delta0: f64, level: f64, alternative: Alternative) -> Result<TestResult> {
    let d = paired_differences(a, b)?;
    let mut r = one_sample_t(&d, delta0, level, alternative)?;
    r.method = "Paired t";
    Ok(r)
}

/// Two-sample t test of `mean(a) − mean(b) = Δ₀`, pooled (`equal_variances`)
/// or Welch.
///
/// # Errors
/// [`StatError::InsufficientData`] if a sample has fewer than two values;
/// [`StatError::InvalidArgument`] if both have zero variance.
pub fn two_sample_t(
    a: &[f64],
    b: &[f64],
    delta0: f64,
    equal_variances: bool,
    level: f64,
    alternative: Alternative,
) -> Result<TestResult> {
    let (est, se, df) = two_sample_parts(a, b, equal_variances)?;
    let method = if equal_variances { "Two-sample t (pooled variance)" } else { "Two-sample t (Welch)" };
    location_test(method, est, delta0, se, df, level, alternative)
}

/// Difference, standard error and df of the two-sample t.
pub(crate) fn two_sample_parts(a: &[f64], b: &[f64], equal_variances: bool) -> Result<(f64, f64, f64)> {
    check_sample(a, 2)?;
    check_sample(b, 2)?;
    let (n1, n2) = (a.len() as f64, b.len() as f64);
    let ((m1, v1), (m2, v2)) = (mean_var(a), mean_var(b));
    if v1 == 0.0 && v2 == 0.0 {
        return Err(invalid("both samples have zero variance"));
    }
    Ok(if equal_variances {
        let df = n1 + n2 - 2.0;
        let sp2 = ((n1 - 1.0) * v1 + (n2 - 1.0) * v2) / df;
        (m1 - m2, (sp2 * (1.0 / n1 + 1.0 / n2)).sqrt(), df)
    } else {
        let (q1, q2) = (v1 / n1, v2 / n2);
        let df = (q1 + q2).powi(2) / (q1 * q1 / (n1 - 1.0) + q2 * q2 / (n2 - 1.0));
        (m1 - m2, (q1 + q2).sqrt(), df)
    })
}

/// Standardized mean difference of two independent samples.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectSize {
    /// Cohen's `d` (pooled standard deviation).
    pub cohens_d: f64,
    /// Hedges' `g` (small-sample corrected `d`).
    pub hedges_g: f64,
    /// Glass's `Δ = (ȳ₁ − ȳ₂)/s₂`: the difference scaled by the second
    /// (control) sample's standard deviation alone (Glass 1976); NaN when
    /// that sample has no variation.
    pub glass_delta: f64,
}

/// Cohen's `d`, Hedges' `g` and Glass's `Δ` for `a` against `b` (the
/// control).
///
/// # Errors
/// As [`two_sample_t`].
pub fn effect_size(a: &[f64], b: &[f64]) -> Result<EffectSize> {
    check_sample(a, 2)?;
    check_sample(b, 2)?;
    let (n1, n2) = (a.len() as f64, b.len() as f64);
    let ((m1, v1), (m2, v2)) = (mean_var(a), mean_var(b));
    let sp = (((n1 - 1.0) * v1 + (n2 - 1.0) * v2) / (n1 + n2 - 2.0)).sqrt();
    if sp == 0.0 {
        return Err(invalid("both samples have zero variance"));
    }
    let d = (m1 - m2) / sp;
    let glass_delta = if v2 > 0.0 { (m1 - m2) / v2.sqrt() } else { f64::NAN };
    Ok(EffectSize { cohens_d: d, hedges_g: d * (1.0 - 3.0 / (4.0 * (n1 + n2) - 9.0)), glass_delta })
}

/// `d_z = mean(d)/sd(d)` for paired differences.
///
/// # Errors
/// As [`one_sample_t`].
pub fn paired_effect_size(differences: &[f64]) -> Result<f64> {
    check_sample(differences, 2)?;
    let (m, v) = mean_var(differences);
    if v == 0.0 {
        return Err(invalid("the differences have no variation"));
    }
    Ok(m / v.sqrt())
}

/// Fieller confidence limits for a ratio of two means.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RatioLimits {
    /// The limits (an infinite one for a one-sided bound).
    Bounded(f64, f64),
    /// The denominator mean is not significantly different from zero at the
    /// confidence level, so the confidence set is not an interval (the
    /// whole line, or the line minus an interval).
    Unbounded,
}

/// Ratio of two independent means, `ρ = μ₁/μ₂`, with Fieller's interval.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RatioOfMeans {
    /// `x̄₁ / x̄₂`.
    pub estimate: f64,
    /// Hypothesized ratio `ρ₀`.
    pub null_value: f64,
    /// `t = (x̄₁ − ρ₀x̄₂) / SE(ρ₀)`.
    pub statistic: f64,
    /// Degrees of freedom of the test.
    pub df: f64,
    /// p-value of the test against the alternative.
    pub p_value: f64,
    /// Fieller limits.
    pub limits: RatioLimits,
    /// Degrees of freedom of the limits (Welch: evaluated at the estimate).
    pub interval_df: f64,
    /// Confidence level.
    pub level: f64,
    /// Pooled variance (`true`) or unequal variances.
    pub equal_variances: bool,
}

/// Ratio of the mean of `a` to the mean of `b` with Fieller's (1954)
/// confidence limits and a t test of `ρ = ρ₀`.
///
/// The test uses `x̄₁ − ρ₀x̄₂`, whose variance is `s_p²(1/n₁ + ρ₀²/n₂)` on
/// `n₁ + n₂ − 2` df (pooled) or `s₁²/n₁ + ρ₀²s₂²/n₂` on Welch–Satterthwaite
/// df (unequal variances; Tamhane & Logan, *Biometrical Journal* 46, 2004).
/// The limits are the values of `ρ` the two-sided (or one-sided) test does
/// not reject: the roots of
/// `(x̄₂² − t²v₂)ρ² − 2x̄₁x̄₂ρ + (x̄₁² − t²v₁) = 0` with `v₁ = s_p²/n₁`,
/// `v₂ = s_p²/n₂` (or `s₁²/n₁`, `s₂²/n₂`, the unequal-variance df taken at
/// `ρ̂`); they exist only when `x̄₂² > t²v₂`, i.e. the denominator mean is
/// significantly non-zero (Kirkwood, *Biometrics* 37, 1981).
///
/// # Errors
/// Fewer than two values in a sample, non-finite values, a level outside
/// (0, 1), a zero denominator mean, or both samples without variation.
pub fn ratio_of_means(
    a: &[f64],
    b: &[f64],
    null_value: f64,
    equal_variances: bool,
    level: f64,
    alternative: Alternative,
) -> Result<RatioOfMeans> {
    check_level(level)?;
    check_sample(a, 2)?;
    check_sample(b, 2)?;
    if !null_value.is_finite() {
        return Err(invalid("the hypothesized ratio must be a finite number"));
    }
    let (n1, n2) = (a.len() as f64, b.len() as f64);
    let ((m1, s1), (m2, s2)) = (mean_var(a), mean_var(b));
    if m2 == 0.0 {
        return Err(invalid("the second group's mean is zero, so the ratio is not defined"));
    }
    if s1 == 0.0 && s2 == 0.0 {
        return Err(invalid("both samples have zero variance"));
    }
    let (v1, v2) = if equal_variances {
        let sp2 = ((n1 - 1.0) * s1 + (n2 - 1.0) * s2) / (n1 + n2 - 2.0);
        (sp2 / n1, sp2 / n2)
    } else {
        (s1 / n1, s2 / n2)
    };
    let df_at = |rho: f64| {
        if equal_variances {
            n1 + n2 - 2.0
        } else {
            let (q1, q2) = (v1, rho * rho * v2);
            (q1 + q2).powi(2) / (q1 * q1 / (n1 - 1.0) + q2 * q2 / (n2 - 1.0))
        }
    };
    let estimate = m1 / m2;
    let df = df_at(null_value);
    let statistic = (m1 - null_value * m2) / (v1 + null_value * null_value * v2).sqrt();
    let t = StudentsT::new(df)?;
    // t grows with ρ̂ − ρ₀ when x̄₂ > 0 and shrinks when x̄₂ < 0.
    let signed = if m2 > 0.0 { statistic } else { -statistic };
    let p_value = match alternative {
        Alternative::TwoSided => 2.0 * t.sf(statistic.abs())?,
        Alternative::Greater => t.sf(signed)?,
        Alternative::Less => t.cdf(signed)?,
    }
    .min(1.0);
    let interval_df = df_at(estimate);
    let tail = if alternative == Alternative::TwoSided { (1.0 - level) / 2.0 } else { 1.0 - level };
    let q = StudentsT::new(interval_df)?.quantile(1.0 - tail)?;
    let qa = m2 * m2 - q * q * v2;
    let limits = if qa <= 0.0 {
        RatioLimits::Unbounded
    } else {
        let half = ((m1 * m2).powi(2) - qa * (m1 * m1 - q * q * v1)).max(0.0).sqrt();
        let (lo, hi) = ((m1 * m2 - half) / qa, (m1 * m2 + half) / qa);
        match alternative {
            Alternative::TwoSided => RatioLimits::Bounded(lo, hi),
            Alternative::Greater => RatioLimits::Bounded(lo, f64::INFINITY),
            Alternative::Less => RatioLimits::Bounded(f64::NEG_INFINITY, hi),
        }
    };
    Ok(RatioOfMeans { estimate, null_value, statistic, df, p_value, limits, interval_df, level, equal_variances })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Student's (1908) sleep data: extra hours of sleep under two drugs for
    /// the same ten patients.
    pub(crate) const SLEEP_1: [f64; 10] = [0.7, -1.6, -0.2, -1.2, -0.1, 3.4, 3.7, 0.8, 0.0, 2.0];
    pub(crate) const SLEEP_2: [f64; 10] = [1.9, 0.8, 1.1, 0.1, -0.1, 4.4, 5.5, 1.6, 4.6, 3.4];
    pub(crate) const X: [f64; 10] = [5.1, 4.9, 5.6, 5.8, 6.0, 5.2, 4.7, 5.5, 5.9, 5.3];

    pub(crate) fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol * b.abs().max(1e-300), "{a} vs {b}");
    }

    /// Published results (R `t.test` documentation, sleep data): paired
    /// t = −4.0621, df = 9, p = 0.002833, 95% CI (−2.4599, −0.7001); Welch
    /// t = −1.8608, df = 17.776, p = 0.07939, CI (−3.3655, 0.2055).
    /// Full-precision values from SciPy 1.18.
    #[test]
    fn sleep_data() {
        let p = paired_t(&SLEEP_1, &SLEEP_2, 0.0, 0.95, Alternative::TwoSided).unwrap();
        close(p.statistic, -4.062_127_683_382_037, 1e-12);
        close(p.p_value, 0.002_832_890_197_384_273, 1e-9);
        close(p.confidence_interval.0, -2.459_885_763_276_983, 1e-10);
        close(p.confidence_interval.1, -0.700_114_236_723_017_5, 1e-9);
        assert_eq!(p.df, Some(9.0));
        let w = two_sample_t(&SLEEP_1, &SLEEP_2, 0.0, false, 0.95, Alternative::TwoSided).unwrap();
        close(w.statistic, -1.860_813_467_486_852_6, 1e-12);
        close(w.df.unwrap(), 17.776_473_516_178_488, 1e-12);
        close(w.p_value, 0.079_394_140_187_358_3, 1e-9);
        close(w.confidence_interval.0, -3.365_483_230_711_710_4, 1e-10);
        close(w.confidence_interval.1, 0.205_483_230_711_711_37, 1e-9);
        let pooled = two_sample_t(&SLEEP_1, &SLEEP_2, 0.0, true, 0.95, Alternative::TwoSided).unwrap();
        close(pooled.p_value, 0.079_186_714_215_938_22, 1e-9);
        close(pooled.confidence_interval.0, -3.363_874_032_287_598_4, 1e-10);
        let less = two_sample_t(&SLEEP_1, &SLEEP_2, 0.0, true, 0.95, Alternative::Less).unwrap();
        close(less.p_value, 0.039_593_357_107_969_11, 1e-9);
        assert_eq!(less.confidence_interval.0, f64::NEG_INFINITY);
        close(less.confidence_interval.1, -0.107_622_168_300_397_06, 1e-9);
        let e = effect_size(&SLEEP_1, &SLEEP_2).unwrap();
        close(e.cohens_d, -0.832_181_081_349_539_5, 1e-12);
        close(e.hedges_g, -0.797_018_500_447_446_4, 1e-12);
        // R: (mean(a) - mean(b)) / sd(b).
        close(e.glass_delta, -0.789_112_746_959_089_5, 1e-12);
        let d = paired_differences(&SLEEP_1, &SLEEP_2).unwrap();
        close(paired_effect_size(&d).unwrap(), -1.284_557_562_591_054_6, 1e-12);
    }

    #[test]
    fn one_sample_tests() {
        let t = one_sample_t(&X, 5.0, 0.95, Alternative::TwoSided).unwrap();
        close(t.statistic, 2.910_427_500_435_992, 1e-12);
        close(t.p_value, 0.017_299_208_775_527_028, 1e-9);
        close(t.confidence_interval.0, 5.089_096_235_867_847_5, 1e-12);
        close(t.confidence_interval.1, 5.710_903_764_132_151, 1e-12);
        let g = one_sample_t(&X, 5.0, 0.95, Alternative::Greater).unwrap();
        close(g.p_value, 0.008_649_604_387_763_514, 1e-9);
        close(g.confidence_interval.0, 5.148_062_725_165_752, 1e-12);
        assert_eq!(g.confidence_interval.1, f64::INFINITY);
        let z = one_sample_z(&X, 5.0, 0.4, 0.95, Alternative::TwoSided).unwrap();
        close(z.statistic, 3.162_277_660_168_375_5, 1e-12);
        close(z.p_value, 0.001_565_402_258_002_569_4, 1e-9);
        assert_eq!((z.statistic_name, z.df), ("z", None));
        assert!(one_sample_t(&[1.0], 0.0, 0.95, Alternative::TwoSided).is_err());
        assert!(one_sample_t(&[2.0, 2.0], 0.0, 0.95, Alternative::TwoSided).is_err());
        assert!(paired_t(&[1.0, 2.0], &[1.0], 0.0, 0.95, Alternative::TwoSided).is_err());
    }

    /// Fieller limits and the ratio test against an independent SciPy
    /// computation (`tests/data/ratio_reference.py`, limits by root finding).
    #[test]
    #[allow(clippy::excessive_precision)]
    fn ratio_of_means_matches_scipy() {
        let a = [12.1, 14.3, 11.8, 13.9, 15.2, 12.7, 14.8, 13.3];
        let b = [8.2, 9.1, 7.7, 9.8, 8.9, 8.4, 9.5];
        let cases = [
            (
                true,
                [
                    1.5355113636363638,
                    0.44705641647902372,
                    13.0,
                    0.66219185501108324,
                    1.3741426353912531,
                    1.7260954215182758,
                ],
            ),
            (
                false,
                [
                    1.5355113636363638,
                    0.51191259736162598,
                    12.987069477917604,
                    0.61730549627000864,
                    1.3907106845792712,
                    1.6950217665201077,
                ],
            ),
        ];
        for (pooled, r) in cases {
            let f = ratio_of_means(&a, &b, 1.5, pooled, 0.95, Alternative::TwoSided).unwrap();
            let close = |x: f64, y: f64| assert!((x - y).abs() <= 1e-10 * (1.0 + y.abs()), "{x} vs {y}");
            close(f.estimate, r[0]);
            close(f.statistic, r[1]);
            close(f.df, r[2]);
            close(f.p_value, r[3]);
            let RatioLimits::Bounded(lo, hi) = f.limits else { panic!("bounded") };
            close(lo, r[4]);
            close(hi, r[5]);
        }
        // A denominator mean indistinguishable from zero gives no interval.
        let f = ratio_of_means(&a, &[-1.0, 2.0, -0.5, 1.0], 1.0, true, 0.95, Alternative::TwoSided).unwrap();
        assert_eq!(f.limits, RatioLimits::Unbounded);
        // One-sided bounds.
        let g = ratio_of_means(&a, &b, 1.5, true, 0.95, Alternative::Greater).unwrap();
        let RatioLimits::Bounded(lo, hi) = g.limits else { panic!("bounded") };
        assert!(lo > 1.3741426353912531 && hi.is_infinite());
        close_p(g.p_value, 0.66219185501108324 / 2.0);
    }

    fn close_p(x: f64, y: f64) {
        assert!((x - y).abs() <= 1e-10, "{x} vs {y}");
    }
}
