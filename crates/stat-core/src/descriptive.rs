//! Descriptive statistics: moments, quantiles, robust estimators, and
//! lag-one autocorrelation.
//!
//! All functions take raw (unsorted) data and ignore nothing: callers remove
//! missing values first. Sums use compensated arithmetic.
//!
//! # Definitions
//! * **Variance** — the corrected two-pass algorithm (Chan, Golub & LeVeque 1983,
//!   "Algorithms for computing the sample variance", *Amer. Statist.* 37):
//!   `s² = (Σ(xᵢ − x̄)² − (Σ(xᵢ − x̄))²/n) / (n − 1)`; the second term removes the
//!   rounding error of `x̄`.
//! * **Skewness** — the adjusted Fisher–Pearson coefficient
//!   `G₁ = √(n(n−1))/(n−2) · m₃/m₂^{3/2}`, and **excess kurtosis**
//!   `G₂ = (n−1)/((n−2)(n−3)) · ((n+1)(m₄/m₂² − 3) + 6)`, with `m_k` the central
//!   sample moments (Joanes & Gill 1998, "Comparing measures of sample skewness
//!   and kurtosis", *The Statistician* 47; their `G₁`, `G₂`).
//! * **Quantiles** — the nine definitions of Hyndman & Fan (1996), "Sample
//!   quantiles in statistical packages", *Amer. Statist.* 50(4). The default is
//!   their recommended type 8 (approximately median-unbiased).
//! * **MAD** — median absolute deviation from the median; the scaled version
//!   multiplies by `1/Φ⁻¹(3/4) ≈ 1.4826` so it estimates σ for normal data.
//! * **Lag-1 autocorrelation** — `r₁ = Σ_{i≥2}(xᵢ − x̄)(xᵢ₋₁ − x̄) / Σ(xᵢ − x̄)²`,
//!   the definition used by the NIST StRD univariate datasets.

use crate::distributions::{ContinuousDistribution, Normal, StudentsT};
use crate::error::{Result, StatError, invalid};
use crate::summation::{mean as compensated_mean, sum_iter};

/// Summary moments of a sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Moments {
    /// Number of observations.
    pub n: usize,
    /// Arithmetic mean.
    pub mean: f64,
    /// Sample variance (divisor `n − 1`); `None` for `n < 2`.
    pub variance: Option<f64>,
    /// Sample standard deviation; `None` for `n < 2`.
    pub std_dev: Option<f64>,
    /// Standard error of the mean `s/√n`; `None` for `n < 2`.
    pub std_error: Option<f64>,
    /// Adjusted skewness `G₁`; `None` for `n < 3` or zero variance.
    pub skewness: Option<f64>,
    /// Adjusted excess kurtosis `G₂`; `None` for `n < 4` or zero variance.
    pub excess_kurtosis: Option<f64>,
    /// Smallest value.
    pub min: f64,
    /// Largest value.
    pub max: f64,
    /// Sum of the values.
    pub sum: f64,
}

impl Moments {
    /// Coefficient of variation `100 s / x̄` (percent); `None` if undefined.
    #[must_use]
    pub fn coefficient_of_variation(&self) -> Option<f64> {
        self.std_dev.filter(|_| self.mean != 0.0).map(|s| 100.0 * s / self.mean)
    }

    /// Two-sided `level` confidence interval for the mean, `x̄ ± t_{1−α/2, n−1} · SE`.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] if `level ∉ (0, 1)`; `Ok(None)` for `n < 2`.
    pub fn mean_confidence_interval(&self, level: f64) -> Result<Option<(f64, f64)>> {
        if !(level > 0.0 && level < 1.0) {
            return Err(invalid(format!("confidence level must lie in (0, 1) (got {level})")));
        }
        let Some(se) = self.std_error else { return Ok(None) };
        let t = StudentsT::new((self.n - 1) as f64)?.quantile(0.5 + level / 2.0)?;
        Ok(Some((self.mean - t * se, self.mean + t * se)))
    }
}

fn check_data(x: &[f64]) -> Result<()> {
    if x.is_empty() {
        return Err(StatError::InsufficientData { required: 1, found: 0 });
    }
    if x.iter().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "descriptive statistics input" });
    }
    Ok(())
}

/// Moments of `x`.
///
/// # Errors
/// [`StatError::InsufficientData`] for empty input; [`StatError::NonFiniteInput`]
/// for NaN or ±∞.
pub fn moments(x: &[f64]) -> Result<Moments> {
    check_data(x)?;
    let n = x.len();
    let nf = n as f64;
    let mean = compensated_mean(x).unwrap_or(f64::NAN);
    let dev = |p: i32| sum_iter(x.iter().map(|&v| (v - mean).powi(p)));
    let (s1, s2) = (dev(1), dev(2));
    // Corrected two-pass sum of squares.
    let ss = (s2 - s1 * s1 / nf).max(0.0);
    let variance = (n >= 2).then(|| ss / (nf - 1.0));
    let std_dev = variance.map(f64::sqrt);
    let m2 = ss / nf;
    let skewness = (n >= 3 && m2 > 0.0).then(|| {
        let g1 = (dev(3) / nf) / m2.powf(1.5);
        (nf * (nf - 1.0)).sqrt() / (nf - 2.0) * g1
    });
    let excess_kurtosis = (n >= 4 && m2 > 0.0).then(|| {
        let g2 = (dev(4) / nf) / (m2 * m2) - 3.0;
        (nf - 1.0) / ((nf - 2.0) * (nf - 3.0)) * ((nf + 1.0) * g2 + 6.0)
    });
    let (min, max) = x.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
    Ok(Moments {
        n,
        mean,
        variance,
        std_dev,
        std_error: std_dev.map(|s| s / nf.sqrt()),
        skewness,
        excess_kurtosis,
        min,
        max,
        sum: sum_iter(x.iter().copied()),
    })
}

/// Sample-quantile definition (Hyndman & Fan 1996, types 1–9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum QuantileMethod {
    /// Type 1: inverse of the empirical CDF.
    InverseCdf,
    /// Type 2: inverse empirical CDF, averaging at discontinuities.
    AveragedInverseCdf,
    /// Type 3: nearest even order statistic.
    NearestEven,
    /// Type 4: linear interpolation of the empirical CDF.
    LinearCdf,
    /// Type 5: piecewise linear with knots at the midpoints `(k − ½)/n`.
    Midpoint,
    /// Type 6: `p(n + 1)`-th order statistic, interpolated (mean-unbiased plotting positions).
    Weibull,
    /// Type 7: `1 + p(n − 1)`-th order statistic, interpolated (mode-based).
    Linear,
    /// Type 8: approximately median-unbiased; recommended by Hyndman & Fan.
    #[default]
    MedianUnbiased,
    /// Type 9: approximately unbiased for normal data.
    NormalUnbiased,
}

impl QuantileMethod {
    /// Hyndman–Fan type number, 1–9.
    #[must_use]
    pub fn type_number(self) -> u8 {
        match self {
            Self::InverseCdf => 1,
            Self::AveragedInverseCdf => 2,
            Self::NearestEven => 3,
            Self::LinearCdf => 4,
            Self::Midpoint => 5,
            Self::Weibull => 6,
            Self::Linear => 7,
            Self::MedianUnbiased => 8,
            Self::NormalUnbiased => 9,
        }
    }
}

/// The `p`-quantile of already **sorted** data.
///
/// With `j = ⌊np + m⌋` and `g = np + m − j` (Hyndman & Fan's notation, `m` per
/// type), the result is `(1 − γ) x₍ⱼ₎ + γ x₍ⱼ₊₁₎`, with order statistics clamped to
/// `x₍₁₎ … x₍ₙ₎`.
///
/// # Errors
/// [`StatError::InsufficientData`] for empty data; [`StatError::InvalidArgument`]
/// if `p ∉ [0, 1]`.
pub fn quantile_sorted(sorted: &[f64], p: f64, method: QuantileMethod) -> Result<f64> {
    let n = sorted.len();
    if n == 0 {
        return Err(StatError::InsufficientData { required: 1, found: 0 });
    }
    if !(0.0..=1.0).contains(&p) {
        return Err(invalid(format!("quantile probability must lie in [0, 1] (got {p})")));
    }
    let nf = n as f64;
    let at = |k: i64| sorted[(k.clamp(1, n as i64) - 1) as usize];
    let interpolate = |m: f64| {
        let h = nf * p + m;
        let j = h.floor();
        let g = h - j;
        let j = j as i64;
        (1.0 - g) * at(j) + g * at(j + 1)
    };
    let value = match method {
        QuantileMethod::InverseCdf | QuantileMethod::AveragedInverseCdf | QuantileMethod::NearestEven => {
            let h = if method == QuantileMethod::NearestEven { nf * p - 0.5 } else { nf * p };
            // These types jump where `np + m` is a whole number, so a product that
            // is whole in exact arithmetic but not in binary (100 × 0.07 =
            // 7.000000000000001) must not cross the jump: snap within 4 ulps.
            let fuzz = 4.0 * f64::EPSILON * h.abs().max(1.0);
            let mut j = h.floor();
            let mut g = h - j;
            if g > 1.0 - fuzz {
                j += 1.0;
                g = 0.0;
            } else if g < fuzz {
                g = 0.0;
            }
            let j = j as i64;
            let gamma = match method {
                QuantileMethod::InverseCdf => {
                    if g > 0.0 {
                        1.0
                    } else {
                        0.0
                    }
                }
                QuantileMethod::AveragedInverseCdf => {
                    if g > 0.0 {
                        1.0
                    } else {
                        0.5
                    }
                }
                _ => {
                    if g == 0.0 && j % 2 == 0 {
                        0.0
                    } else {
                        1.0
                    }
                }
            };
            (1.0 - gamma) * at(j) + gamma * at(j + 1)
        }
        QuantileMethod::LinearCdf => interpolate(0.0),
        QuantileMethod::Midpoint => interpolate(0.5),
        QuantileMethod::Weibull => interpolate(p),
        QuantileMethod::Linear => interpolate(1.0 - p),
        QuantileMethod::MedianUnbiased => interpolate((p + 1.0) / 3.0),
        QuantileMethod::NormalUnbiased => interpolate(p / 4.0 + 3.0 / 8.0),
    };
    Ok(value)
}

/// Quantiles of `x` at each probability in `ps`.
///
/// # Errors
/// As for [`quantile_sorted`], plus [`StatError::NonFiniteInput`] for NaN/∞ data.
pub fn quantiles(x: &[f64], ps: &[f64], method: QuantileMethod) -> Result<Vec<f64>> {
    check_data(x)?;
    let sorted = sorted_copy(x);
    ps.iter().map(|&p| quantile_sorted(&sorted, p, method)).collect()
}

fn sorted_copy(x: &[f64]) -> Vec<f64> {
    let mut v = x.to_vec();
    v.sort_by(f64::total_cmp);
    v
}

/// Median: the middle order statistic for odd `n`, the average of the two
/// middle ones for even `n` (the 0.5 quantile by Hyndman–Fan type 2, which
/// types 5–9 match; types 1, 3 and 4 can differ, e.g. type 1 takes the lower
/// middle value for even `n`).
///
/// # Errors
/// As for [`quantiles`].
pub fn median(x: &[f64]) -> Result<f64> {
    check_data(x)?;
    quantile_sorted(&sorted_copy(x), 0.5, QuantileMethod::AveragedInverseCdf)
}

/// Robust location and scale estimates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RobustSummary {
    /// Median.
    pub median: f64,
    /// Median absolute deviation from the median (raw).
    pub mad: f64,
    /// MAD scaled to estimate σ for normal data (`mad / Φ⁻¹(3/4)`).
    pub mad_scaled: f64,
    /// Interquartile range `Q₃ − Q₁` (quantiles by the chosen method).
    pub iqr: f64,
    /// Proportion trimmed from each tail for the trimmed / winsorized means.
    pub trim: f64,
    /// Mean of the data with `⌊trim·n⌋` values removed from each tail.
    pub trimmed_mean: f64,
    /// Mean with `⌊trim·n⌋` values in each tail replaced by the nearest kept value.
    pub winsorized_mean: f64,
}

/// Robust summary of `x`, trimming `trim` (in `[0, 0.5)`) from each tail.
///
/// # Errors
/// [`StatError::InvalidArgument`] for a trim proportion outside `[0, 0.5)`, plus
/// the errors of [`quantiles`].
pub fn robust_summary(x: &[f64], trim: f64, method: QuantileMethod) -> Result<RobustSummary> {
    check_data(x)?;
    if !(0.0..0.5).contains(&trim) {
        return Err(invalid(format!("trim proportion must lie in [0, 0.5) (got {trim})")));
    }
    let sorted = sorted_copy(x);
    let n = sorted.len();
    let median = quantile_sorted(&sorted, 0.5, QuantileMethod::AveragedInverseCdf)?;
    let mut abs_dev: Vec<f64> = sorted.iter().map(|v| (v - median).abs()).collect();
    abs_dev.sort_by(f64::total_cmp);
    let mad = quantile_sorted(&abs_dev, 0.5, QuantileMethod::AveragedInverseCdf)?;
    let q75 = Normal::standard().quantile(0.75)?;
    let iqr = quantile_sorted(&sorted, 0.75, method)? - quantile_sorted(&sorted, 0.25, method)?;
    let k = (trim * n as f64).floor() as usize;
    let kept = &sorted[k..n - k];
    let trimmed_mean = compensated_mean(kept).unwrap_or(f64::NAN);
    let winsorized: Vec<f64> = (0..n).map(|i| sorted[i.clamp(k, n - 1 - k)]).collect();
    let winsorized_mean = compensated_mean(&winsorized).unwrap_or(f64::NAN);
    Ok(RobustSummary { median, mad, mad_scaled: mad / q75, iqr, trim, trimmed_mean, winsorized_mean })
}

/// Lag-1 autocorrelation (NIST StRD definition); `None` for `n < 2` or zero variance.
#[must_use]
pub fn lag1_autocorrelation(x: &[f64]) -> Option<f64> {
    if x.len() < 2 || x.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let m = compensated_mean(x)?;
    let den = sum_iter(x.iter().map(|&v| (v - m) * (v - m)));
    let num = sum_iter(x.windows(2).map(|w| (w[1] - m) * (w[0] - m)));
    (den > 0.0).then(|| num / den)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        ((a - b) / b).abs() <= tol
    }

    #[test]
    fn moments_match_exact_rational_reference() {
        // Reference values computed with exact rational arithmetic.
        let x = [2.0, 3.0, 5.0, 7.0, 11.0, 13.0, 17.0, 19.0, 23.0, 29.0, 31.0, 61.0];
        let m = moments(&x).unwrap();
        assert!(close(m.mean, 18.416_666_666_666_668, 1e-15));
        assert!(close(m.variance.unwrap(), 273.537_878_787_878_8, 1e-14));
        assert!(close(m.skewness.unwrap(), 1.638_061_744_083_93, 1e-13));
        assert!(close(m.excess_kurtosis.unwrap(), 3.355_850_776_299_269_5, 1e-13));
        assert_eq!((m.min, m.max, m.sum, m.n), (2.0, 61.0, 221.0, 12));
        assert!(close(m.std_error.unwrap(), m.std_dev.unwrap() / 12f64.sqrt(), 1e-15));
    }

    #[test]
    fn symmetric_data_have_zero_skew_and_known_kurtosis() {
        let x = [1.0, 2.0, 3.0, 4.0, 5.0];
        let m = moments(&x).unwrap();
        assert!(m.skewness.unwrap().abs() < 1e-15);
        // m₂ = 2, m₄ = 6.8 ⇒ g₂ = −1.3; G₂ = 4/(3·2) · (6·(−1.3) + 6) = −1.2.
        assert!(close(m.excess_kurtosis.unwrap(), -1.2, 1e-14));
    }

    #[test]
    fn small_samples_and_bad_input() {
        let m = moments(&[4.0]).unwrap();
        assert_eq!((m.variance, m.skewness, m.excess_kurtosis), (None, None, None));
        let m = moments(&[1.0, 3.0, 5.0]).unwrap();
        assert!(m.skewness.is_some() && m.excess_kurtosis.is_none());
        assert!(moments(&[]).is_err());
        assert!(moments(&[1.0, f64::NAN]).is_err());
        let c = moments(&[7.0; 5]).unwrap();
        assert_eq!(c.variance, Some(0.0));
        assert_eq!(c.skewness, None, "undefined for constant data");
    }

    #[test]
    fn confidence_interval_and_cv() {
        let m = moments(&[2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]).unwrap();
        let (lo, hi) = m.mean_confidence_interval(0.95).unwrap().unwrap();
        // t(7, 0.975) = 2.364624251592785.
        let half = 2.364_624_251_592_785 * m.std_error.unwrap();
        assert!(close(lo, 5.0 - half, 1e-12) && close(hi, 5.0 + half, 1e-12));
        assert!(close(m.coefficient_of_variation().unwrap(), 100.0 * m.std_dev.unwrap() / 5.0, 1e-15));
        assert!(m.mean_confidence_interval(1.5).is_err());
    }

    /// Hyndman & Fan types 1–9 at p = 0.25 for x = 1..10, worked from the
    /// definitions (these are also the values R's `quantile(1:10, 0.25, type = k)`
    /// prints).
    #[test]
    fn quantile_types_match_definitions() {
        let x: Vec<f64> = (1..=10).map(f64::from).collect();
        let expected = [3.0, 3.0, 2.0, 2.5, 3.0, 2.75, 3.25, 2.916_666_666_666_667, 2.9375];
        let methods = [
            QuantileMethod::InverseCdf,
            QuantileMethod::AveragedInverseCdf,
            QuantileMethod::NearestEven,
            QuantileMethod::LinearCdf,
            QuantileMethod::Midpoint,
            QuantileMethod::Weibull,
            QuantileMethod::Linear,
            QuantileMethod::MedianUnbiased,
            QuantileMethod::NormalUnbiased,
        ];
        for (m, e) in methods.iter().zip(expected) {
            let q = quantiles(&x, &[0.25], *m).unwrap()[0];
            assert!((q - e).abs() < 1e-12, "type {}: {q} vs {e}", m.type_number());
        }
        // Extremes and the median for every type.
        for m in methods {
            let q = quantiles(&x, &[0.0, 0.5, 1.0], m).unwrap();
            assert_eq!((q[0], q[2]), (1.0, 10.0), "type {}", m.type_number());
            assert!((4.5..=6.0).contains(&q[1]), "type {} median {}", m.type_number(), q[1]);
        }
        assert_eq!(median(&x).unwrap(), 5.5);
        assert_eq!(median(&[3.0, 1.0, 2.0]).unwrap(), 2.0);
        assert!(quantiles(&x, &[1.5], QuantileMethod::Linear).is_err());
        assert_eq!(QuantileMethod::default(), QuantileMethod::MedianUnbiased);
    }

    /// Discontinuous types where `np` is whole only in exact arithmetic. From
    /// the definitions (Hyndman & Fan 1996) with `np = 7` exactly for
    /// `n = 100`, `p = 0.07`: type 1 gives `x₍₇₎`, type 2 `(x₍₇₎ + x₍₈₎)/2`,
    /// type 3 (`np − ½ = 6.5`) `x₍₇₎`; likewise `n = 100`, `p = 0.29` (`np = 29`,
    /// computed as 28.999999999999996).
    #[test]
    fn discontinuous_types_snap_whole_products() {
        let x: Vec<f64> = (1..=100).map(f64::from).collect();
        let at = |p: f64, m: QuantileMethod| quantile_sorted(&x, p, m).unwrap();
        assert_eq!(at(0.07, QuantileMethod::InverseCdf), 7.0);
        assert_eq!(at(0.07, QuantileMethod::AveragedInverseCdf), 7.5);
        assert_eq!(at(0.07, QuantileMethod::NearestEven), 7.0);
        assert_eq!(at(0.29, QuantileMethod::InverseCdf), 29.0);
        assert_eq!(at(0.29, QuantileMethod::AveragedInverseCdf), 29.5);
        assert_eq!(at(0.29, QuantileMethod::NearestEven), 29.0);
        // Genuinely fractional products are unaffected.
        assert_eq!(at(0.071, QuantileMethod::InverseCdf), 8.0);
        assert_eq!(at(0.071, QuantileMethod::AveragedInverseCdf), 8.0);
    }

    #[test]
    fn robust_estimates() {
        let x = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 100.0];
        let r = robust_summary(&x, 0.1, QuantileMethod::Linear).unwrap();
        assert_eq!(r.median, 5.5);
        // |x − 5.5| sorted: 0.5, 0.5, 1.5, 1.5, 2.5, 2.5, 3.5, 3.5, 4.5, 94.5 ⇒ MAD = 2.5.
        assert_eq!(r.mad, 2.5);
        assert!(close(r.mad_scaled, 2.5 * 1.482_602_218_505_602, 1e-12));
        assert_eq!(r.trimmed_mean, 5.5, "drops 1 and 100");
        assert!(close(r.winsorized_mean, (2.0 + 2.0 + 3.0 + 4.0 + 5.0 + 6.0 + 7.0 + 8.0 + 9.0 + 9.0) / 10.0, 1e-15));
        assert_eq!(r.iqr, 7.75 - 3.25);
        assert!(robust_summary(&x, 0.5, QuantileMethod::Linear).is_err());
    }

    #[test]
    fn autocorrelation() {
        // Alternating series: strongly negative.
        let x = [1.0, -1.0, 1.0, -1.0, 1.0, -1.0];
        assert!(lag1_autocorrelation(&x).unwrap() < -0.8);
        assert_eq!(lag1_autocorrelation(&[1.0]), None);
        assert_eq!(lag1_autocorrelation(&[2.0, 2.0]), None);
        // NumAcc1 (NIST StRD): certified −0.5.
        assert!(close(lag1_autocorrelation(&[10_000_001.0, 10_000_003.0, 10_000_002.0]).unwrap(), -0.5, 1e-14));
    }
}
