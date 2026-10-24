//! Bivariate association between two continuous variables.
//!
//! * **Pearson** `r = Sxy / √(Sxx Syy)` with compensated sums of centered
//!   products. Test of ρ = 0: `t = r √((n−2)/(1−r²))` on `n − 2` df. Confidence
//!   interval by Fisher's z-transform: `tanh(atanh r ± z_{1−α/2}/√(n−3))`
//!   (Fisher 1921, *Metron* 1).
//! * **Spearman** ρₛ — Pearson's r applied to ranks, ties receiving the average of
//!   the ranks they span (Spearman 1904). Its test uses the same t
//!   approximation, adequate for `n ≳ 10`.

use crate::distributions::{ContinuousDistribution, Normal, StudentsT};
use crate::error::{Result, StatError, invalid};
use crate::summation::{mean, sum_iter};

/// A correlation coefficient with its test and interval.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Correlation {
    /// Number of pairs.
    pub n: usize,
    /// The coefficient.
    pub estimate: f64,
    /// t statistic for H₀: no association; `None` when |r| = 1 or `n < 3`.
    pub t_statistic: Option<f64>,
    /// Two-sided p-value of the t test.
    pub p_value: Option<f64>,
    /// Fisher-z confidence interval (`None` for `n < 4` or |r| = 1).
    pub confidence_interval: Option<(f64, f64)>,
}

/// Pearson correlation, covariance and the variables' standard deviations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PearsonSummary {
    /// Correlation and inference.
    pub correlation: Correlation,
    /// Sample covariance (divisor `n − 1`).
    pub covariance: f64,
}

fn check_pairs(x: &[f64], y: &[f64]) -> Result<()> {
    if x.len() != y.len() {
        return Err(StatError::DimensionMismatch { context: "paired samples", expected: x.len(), found: y.len() });
    }
    if x.len() < 3 {
        return Err(StatError::InsufficientData { required: 3, found: x.len() });
    }
    if x.iter().chain(y).any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "correlation input" });
    }
    Ok(())
}

fn pearson_r(x: &[f64], y: &[f64]) -> Result<(f64, f64)> {
    let (mx, my) = (mean(x).unwrap_or(0.0), mean(y).unwrap_or(0.0));
    let sxy = sum_iter(x.iter().zip(y).map(|(a, b)| (a - mx) * (b - my)));
    let sxx = sum_iter(x.iter().map(|a| (a - mx) * (a - mx)));
    let syy = sum_iter(y.iter().map(|b| (b - my) * (b - my)));
    if sxx <= 0.0 || syy <= 0.0 {
        return Err(invalid("correlation is undefined when a variable is constant"));
    }
    // One square root of the product keeps exactly proportional data at |r| = 1;
    // fall back to separate roots only if the product would overflow or underflow.
    let product = sxx * syy;
    let norm = if product.is_normal() { product.sqrt() } else { sxx.sqrt() * syy.sqrt() };
    let r = (sxy / norm).clamp(-1.0, 1.0);
    Ok((r, sxy / (x.len() as f64 - 1.0)))
}

fn inference(n: usize, r: f64, level: f64) -> Result<Correlation> {
    if !(level > 0.0 && level < 1.0) {
        return Err(invalid(format!("confidence level must lie in (0, 1) (got {level})")));
    }
    let nf = n as f64;
    let exact = r.abs() >= 1.0;
    let (t_statistic, p_value) = if exact {
        (None, None)
    } else {
        let t = r * ((nf - 2.0) / (1.0 - r * r)).sqrt();
        (Some(t), Some(StudentsT::new(nf - 2.0)?.two_sided_p_value(t)?))
    };
    let confidence_interval = if n >= 4 && !exact {
        let z = Normal::standard().quantile(0.5 + level / 2.0)?;
        let (centre, half) = (r.atanh(), z / (nf - 3.0).sqrt());
        Some(((centre - half).tanh(), (centre + half).tanh()))
    } else {
        None
    };
    Ok(Correlation { n, estimate: r, t_statistic, p_value, confidence_interval })
}

/// Pearson product-moment correlation of paired samples, with covariance.
///
/// # Errors
/// [`StatError::DimensionMismatch`] for unequal lengths,
/// [`StatError::InsufficientData`] for fewer than 3 pairs,
/// [`StatError::NonFiniteInput`] for NaN/∞, and [`StatError::InvalidArgument`] if
/// either variable is constant or `level ∉ (0, 1)`.
pub fn pearson(x: &[f64], y: &[f64], level: f64) -> Result<PearsonSummary> {
    check_pairs(x, y)?;
    let (r, covariance) = pearson_r(x, y)?;
    Ok(PearsonSummary { correlation: inference(x.len(), r, level)?, covariance })
}

/// Spearman rank correlation of paired samples (average ranks for ties).
///
/// # Errors
/// As for [`pearson`].
pub fn spearman(x: &[f64], y: &[f64], level: f64) -> Result<Correlation> {
    check_pairs(x, y)?;
    let (r, _) = pearson_r(&average_ranks(x), &average_ranks(y))?;
    inference(x.len(), r, level)
}

/// Ranks 1…n, ties receiving the mean of the ranks they span.
#[must_use]
pub fn average_ranks(x: &[f64]) -> Vec<f64> {
    let mut order: Vec<usize> = (0..x.len()).collect();
    order.sort_by(|&a, &b| x[a].total_cmp(&x[b]));
    let mut ranks = vec![0.0; x.len()];
    let mut i = 0;
    while i < order.len() {
        let mut j = i;
        while j + 1 < order.len() && x[order[j + 1]] == x[order[i]] {
            j += 1;
        }
        let rank = (i + j) as f64 / 2.0 + 1.0;
        for &k in &order[i..=j] {
            ranks[k] = rank;
        }
        i = j + 1;
    }
    ranks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pearson_matches_exact_reference() {
        let x = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let y = [2.0, 1.0, 4.0, 3.0, 7.0, 8.0, 6.0, 9.0];
        let s = pearson(&x, &y, 0.95).unwrap();
        let c = s.correlation;
        // Exact rational reference: 0.8964214570007952.
        assert!((c.estimate - 0.896_421_457_000_795_2).abs() < 1e-15);
        // t and p from the definitions.
        let t = c.estimate * (6.0 / (1.0 - c.estimate * c.estimate)).sqrt();
        assert!((c.t_statistic.unwrap() - t).abs() < 1e-12);
        let p = StudentsT::new(6.0).unwrap().two_sided_p_value(t).unwrap();
        assert!((c.p_value.unwrap() - p).abs() < 1e-15);
        let (lo, hi) = c.confidence_interval.unwrap();
        assert!(lo < c.estimate && c.estimate < hi && hi < 1.0);
        // Fisher-z interval is symmetric on the z scale.
        assert!(((lo.atanh() + hi.atanh()) / 2.0 - c.estimate.atanh()).abs() < 1e-12);
        // Covariance: Sxy / (n − 1) with Sxy = 45 (x̄ = 4.5, ȳ = 5).
        assert!((s.covariance - 45.0 / 7.0).abs() < 1e-13);
    }

    #[test]
    fn spearman_with_ties_matches_exact_reference() {
        let x = [10.0, 20.0, 20.0, 30.0, 40.0, 40.0, 40.0, 50.0];
        let y = [3.0, 1.0, 2.0, 5.0, 4.0, 6.0, 6.0, 8.0];
        assert_eq!(average_ranks(&x), vec![1.0, 2.5, 2.5, 4.0, 6.0, 6.0, 6.0, 8.0]);
        let c = spearman(&x, &y, 0.95).unwrap();
        assert!((c.estimate - 0.852_111_641_992_643_4).abs() < 1e-15);
    }

    #[test]
    fn perfect_and_invalid_cases() {
        let x = [1.0, 2.0, 3.0, 4.0];
        let c = pearson(&x, &[2.0, 4.0, 6.0, 8.0], 0.95).unwrap().correlation;
        assert_eq!(c.estimate, 1.0);
        assert_eq!((c.t_statistic, c.p_value, c.confidence_interval), (None, None, None));
        // Monotone but nonlinear: Spearman is exactly 1.
        assert_eq!(spearman(&x, &[1.0, 8.0, 27.0, 64.0], 0.95).unwrap().estimate, 1.0);
        assert!(pearson(&x, &[1.0, 1.0, 1.0, 1.0], 0.95).is_err(), "constant variable");
        assert!(pearson(&x, &[1.0, 2.0], 0.95).is_err());
        assert!(pearson(&[1.0, 2.0], &[1.0, 2.0], 0.95).is_err(), "too few pairs");
    }
}
