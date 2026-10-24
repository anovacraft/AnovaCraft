//! Multivariate outlier distances of each observation from the centre of
//! the others.
//!
//! With mean `x̄` and covariance `S` (divisor `n − 1`) of `n` observations
//! on `p` variables:
//!
//! * the **Mahalanobis distance** (Mahalanobis 1936) of row `i` is
//!   `dᵢ = √((xᵢ − x̄)ᵀ S⁻¹ (xᵢ − x̄))`;
//! * **Hotelling's T²** of row `i` is `dᵢ²`. For multivariate normal data,
//!   `n T² / (n − 1)²` follows a Beta(p/2, (n − p − 1)/2) distribution
//!   (Gnanadesikan & Kettenring, *Biometrics* 28, 1972; Tracy, Young &
//!   Mason, *J. Quality Technology* 24(2), 1992), so the upper-α reference
//!   limit is `(n − 1)²/n · B_{1−α}(p/2, (n − p − 1)/2)`, and the
//!   Mahalanobis limit is its square root;
//! * the **jackknifed distance** `d₍ᵢ₎` measures row `i` from the mean and
//!   covariance of the other `n − 1` rows, so an outlier cannot mask itself
//!   by pulling the estimates towards it. Leaving one row out changes the
//!   estimates by a rank-one update, which gives the closed form
//!   `d₍ᵢ₎² = n²(n − 2) dᵢ² / ((n − 1)((n − 1)² − n dᵢ²))`
//!   (Sherman–Morrison; Penny, *Applied Statistics* 45(1), 1996). Row `i`
//!   is then a new observation against a sample of `m = n − 1`, so
//!   `d₍ᵢ₎² ~ p(m + 1)(m − 1)/(m(m − p)) · F(p, m − p)`, and the reference
//!   limit is the square root of that quantity's upper-α point.

use stat_core::discrete::beta_quantile;
use stat_core::{ContinuousDistribution, FisherSnedecor, Result, StatError};

use crate::spd::{cholesky, covariance, mean_vector, quadratic_form};

/// Distances of every observation.
#[derive(Debug, Clone, PartialEq)]
pub struct OutlierDistances {
    /// Observations.
    pub n: usize,
    /// Variables.
    pub p: usize,
    /// Mean vector.
    pub mean: Vec<f64>,
    /// Covariance matrix (divisor `n − 1`).
    pub covariance: Vec<Vec<f64>>,
    /// Squared Mahalanobis distances `dᵢ²` (= Hotelling's T²), in row order.
    pub squared: Vec<f64>,
}

/// Upper reference limits of the three distances at one α.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DistanceLimits {
    /// The α the limits were computed for.
    pub alpha: f64,
    /// Limit of the Mahalanobis distance (√ of the T² limit).
    pub mahalanobis: f64,
    /// Limit of the jackknifed distance.
    pub jackknife: f64,
    /// Limit of Hotelling's T² (scaled beta).
    pub t2: f64,
}

impl OutlierDistances {
    /// Mahalanobis distances `dᵢ`.
    #[must_use]
    pub fn mahalanobis(&self) -> Vec<f64> {
        self.squared.iter().map(|d| d.sqrt()).collect()
    }

    /// Hotelling's T² values, `dᵢ²`.
    #[must_use]
    pub fn t2(&self) -> Vec<f64> {
        self.squared.clone()
    }

    /// Jackknifed distances `d₍ᵢ₎` (∞ where the other rows' covariance is
    /// singular).
    #[must_use]
    pub fn jackknife(&self) -> Vec<f64> {
        let n = self.n as f64;
        self.squared
            .iter()
            .map(|&d2| {
                let denominator = (n - 1.0) * ((n - 1.0) * (n - 1.0) - n * d2);
                if denominator <= 1e-12 * (n - 1.0).powi(3) {
                    f64::INFINITY
                } else {
                    (n * n * (n - 2.0) * d2 / denominator).sqrt()
                }
            })
            .collect()
    }

    /// Reference limits at false-alarm probability `alpha` per row.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] unless `alpha ∈ (0, 1)`.
    pub fn limits(&self, alpha: f64) -> Result<DistanceLimits> {
        if !(alpha > 0.0 && alpha < 1.0) {
            return Err(StatError::InvalidArgument { message: format!("α must lie in (0, 1) (got {alpha})") });
        }
        let (n, p) = (self.n as f64, self.p as f64);
        let t2 = (n - 1.0).powi(2) / n * beta_quantile(p / 2.0, (n - p - 1.0) / 2.0, 1.0 - alpha)?;
        let m = n - 1.0;
        let f = FisherSnedecor::new(p, m - p)?.quantile(1.0 - alpha)?;
        let jackknife = (p * (m + 1.0) * (m - 1.0) / (m * (m - p)) * f).sqrt();
        Ok(DistanceLimits { alpha, mahalanobis: t2.sqrt(), jackknife, t2 })
    }
}

/// Distances of the observations `rows` (each `p` finite values).
///
/// # Errors
/// [`StatError::InvalidArgument`] for no variables, rows of unequal length,
/// or a covariance matrix that is singular (a constant column, or columns
/// that are exact linear combinations of others);
/// [`StatError::InsufficientData`] for fewer than `p + 2` rows (the
/// jackknife needs `n − 1 > p`); [`StatError::NonFiniteInput`] for NaN/∞.
pub fn outlier_distances(rows: &[Vec<f64>]) -> Result<OutlierDistances> {
    let p = rows.first().map_or(0, Vec::len);
    if p == 0 {
        return Err(StatError::InvalidArgument { message: "choose at least one column".to_owned() });
    }
    if let Some(bad) = rows.iter().find(|r| r.len() != p) {
        return Err(StatError::DimensionMismatch { context: "observation length", expected: p, found: bad.len() });
    }
    if rows.iter().flatten().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "outlier distance input" });
    }
    let n = rows.len();
    if n < p + 2 {
        return Err(StatError::InsufficientData { required: p + 2, found: n });
    }
    let mean = mean_vector(rows, p);
    let cov = covariance(rows, &mean);
    let l = cholesky(&cov).ok_or_else(|| StatError::InvalidArgument {
        message: "the covariance matrix is singular: a column is constant or a combination of the others".to_owned(),
    })?;
    let squared = rows
        .iter()
        .map(|r| {
            let d: Vec<f64> = r.iter().zip(&mean).map(|(x, m)| x - m).collect();
            quadratic_form(&l, &d)
        })
        .collect();
    Ok(OutlierDistances { n, p, mean, covariance: cov, squared })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> Vec<Vec<f64>> {
        vec![
            vec![2.0, 3.0],
            vec![3.0, 5.0],
            vec![4.0, 4.0],
            vec![5.0, 7.0],
            vec![6.0, 6.0],
            vec![7.0, 9.0],
            vec![8.0, 2.0],
        ]
    }

    #[test]
    fn distances_match_an_independent_computation() {
        // Reference: exact rational arithmetic in Python (fractions module):
        // S⁻¹ by Gauss–Jordan elimination, dᵢ² = eᵢᵀ S⁻¹ eᵢ, and d₍ᵢ₎² from
        // the mean and covariance of the other six rows, recomputed for each
        // row (no closed form).
        let want_sq = [
            2.280_166_435_506_241,
            0.880_258_899_676_375_4,
            0.358_760_980_120_203_4,
            0.625_057_790_106_333_8,
            0.281_091_077_207_582_1,
            2.899_676_375_404_531,
            4.674_988_441_978_733_5,
        ];
        let want_jack = [
            4.646_317_829_457_364,
            1.204_627_621_113_521_4,
            0.437_443_628_398_402_3,
            0.807_067_812_798_471_8,
            0.337_263_851_908_203_35,
            7.540_533_113_492_717,
            58.287_220_026_350_46,
        ];
        let d = outlier_distances(&data()).unwrap();
        let jack = d.jackknife();
        for i in 0..7 {
            assert!((d.squared[i] - want_sq[i]).abs() < 1e-13, "d² row {i}: {}", d.squared[i]);
            assert!((jack[i] * jack[i] - want_jack[i]).abs() < 1e-12, "jackknife row {i}: {}", jack[i] * jack[i]);
        }
        // Σ dᵢ² = p (n − 1) exactly.
        assert!((d.squared.iter().sum::<f64>() - 12.0).abs() < 1e-12);
        assert_eq!(d.t2(), d.squared);
        assert!((d.mahalanobis()[0] - want_sq[0].sqrt()).abs() < 1e-14);
    }

    #[test]
    fn limits_follow_beta_and_f() {
        let d = outlier_distances(&data()).unwrap();
        let l = d.limits(0.05).unwrap();
        // T² limit: (n − 1)²/n · B₀.₉₅(1, 2). Beta(1, 2) has CDF 1 − (1 − x)², so
        // its 0.95 quantile is 1 − √0.05 (exact).
        let want = 36.0 / 7.0 * (1.0 - 0.05_f64.sqrt());
        assert!((l.t2 - want).abs() < 1e-12);
        assert!((l.mahalanobis - want.sqrt()).abs() < 1e-12);
        // Jackknife: p n (n − 2)/((n − 1)(n − 1 − p)) F₀.₉₅(2, 4); F(2, 4) has
        // upper tail (1 + 2f/4)^(−2), so F₀.₉₅ = 2(√20 − 1) (exact).
        let f = 2.0 * (20.0_f64.sqrt() - 1.0);
        let want_j = (2.0 * 7.0 * 5.0 / (6.0 * 4.0) * f).sqrt();
        assert!((l.jackknife - want_j).abs() < 1e-10, "{} vs {want_j}", l.jackknife);
        assert!(d.limits(0.0).is_err());
    }

    #[test]
    fn errors() {
        assert!(outlier_distances(&data()[..3]).is_err(), "n < p + 2");
        assert!(outlier_distances(&[vec![1.0, 1.0], vec![2.0, 2.0], vec![3.0, 3.0], vec![4.0, 4.0]]).is_err());
        assert!(outlier_distances(&[vec![1.0], vec![f64::NAN], vec![3.0]]).is_err());
        assert!(outlier_distances(&[]).is_err());
    }
}
