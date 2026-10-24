//! Between/within capability: a process made of several subgroups — batches
//! in time order, or parallel streams such as mould cavities or filling
//! heads — whose means differ, so that neither the within-subgroup σ nor the
//! overall σ alone describes it (Bothe, *Measuring Process Capability*,
//! 1997; Wheeler, *Advanced Topics in Statistical Process Control*, 1995;
//! Montgomery, *Introduction to Statistical Quality Control*, ch. 8).
//!
//! With `k` subgroups of sizes `nᵢ`, means `x̄ᵢ` and standard deviations `sᵢ`:
//!
//! * **Within** σ_w: the pooled standard deviation
//!   `s_p = √(Σ(nᵢ − 1)sᵢ² / Σ(nᵢ − 1))` divided by `c₄(d + 1)` with
//!   `d = Σ(nᵢ − 1)`, or the average of `Rᵢ/d₂(nᵢ)` (subgroups of 2 to 25).
//! * **Between** σ_b, either
//!   - from the **moving range of subgroup means** (batches in time order):
//!     `σ_x̄ = MR̄/d₂(2)` with `MR̄` the average of `|x̄ᵢ − x̄ᵢ₋₁|`, and
//!     `σ_b² = max(0, σ_x̄² − σ_w²/ñ)` with `ñ` the harmonic mean subgroup size
//!     (equal to `n` for equal subgroups), or
//!   - from **variance components** of the one-way layout (streams, order
//!     irrelevant): `σ_b² = max(0, (MSB − MSW)/n₀)` with
//!     `n₀ = (N − Σnᵢ²/N)/(k − 1)`; here `σ_w² = MSW` and the within option is
//!     not used.
//! * **Between/within** `σ_B/W = √(σ_b² + σ_w²)`.
//! * **Overall** σ: the sample standard deviation of all values.
//!
//! Each σ gives a set of indices `Cp = (USL − LSL)/6σ`,
//! `Cpk = min(μ − LSL, USL − μ)/3σ` with the intervals of
//! [`crate::capability`] (computed with the total number of values, so they
//! are approximate for the within and between/within sets).

use crate::capability::{CapabilityIndices, SpecLimits, check_limits, indices};
use crate::constants::{MAX_RANGE_SUBGROUP, c4, d2};
use stat_core::{Result, StatError};

/// How the within-subgroup σ is estimated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WithinEstimate {
    /// Pooled standard deviation ÷ c₄(d + 1).
    #[default]
    PooledStdDev,
    /// Average of the subgroup ranges each divided by d₂(nᵢ).
    AverageRange,
}

/// How the between-subgroup σ is estimated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BetweenEstimate {
    /// Moving range of consecutive subgroup means (time-ordered batches).
    #[default]
    MovingRange,
    /// One-way ANOVA variance components (streams; order does not matter).
    VarianceComponents,
}

/// Options for [`between_within_capability`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BetweenWithinOptions {
    /// Within-σ estimator.
    pub within: WithinEstimate,
    /// Between-σ estimator.
    pub between: BetweenEstimate,
    /// Confidence level of the intervals.
    pub confidence_level: f64,
}

impl Default for BetweenWithinOptions {
    fn default() -> Self {
        Self { within: WithinEstimate::default(), between: BetweenEstimate::default(), confidence_level: 0.95 }
    }
}

/// Result of [`between_within_capability`].
#[derive(Debug, Clone, PartialEq)]
pub struct BetweenWithin {
    /// Total number of values.
    pub n: usize,
    /// Number of subgroups.
    pub subgroups: usize,
    /// Grand mean.
    pub mean: f64,
    /// Subgroup means, in subgroup order.
    pub means: Vec<f64>,
    /// Within-subgroup σ.
    pub sigma_within: f64,
    /// Between-subgroup σ (zero when the subgroup means vary no more than
    /// the within variation explains).
    pub sigma_between: f64,
    /// `√(σ_b² + σ_w²)`.
    pub sigma_between_within: f64,
    /// Sample standard deviation of all values.
    pub sigma_overall: f64,
    /// Standard deviation of the subgroup means: `MR̄/d₂(2)` for the moving
    /// range method, the sample SD of the means otherwise.
    pub sigma_means: f64,
    /// Subgroup size used to scale the within variance (harmonic mean `ñ`, or
    /// `n₀` for variance components).
    pub effective_size: f64,
    /// Indices with σ_w.
    pub within: CapabilityIndices,
    /// Indices with σ_B/W.
    pub between_within: CapabilityIndices,
    /// Indices with the overall σ.
    pub overall: CapabilityIndices,
    /// Observed nonconforming parts per million below and above the limits.
    pub observed_ppm: (f64, f64),
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

fn sum_sq(v: &[f64]) -> f64 {
    let m = mean(v);
    v.iter().map(|x| (x - m) * (x - m)).sum()
}

/// Between/within capability of `groups` (subgroups in order) against `limits`.
///
/// # Errors
/// [`StatError::InvalidArgument`] for invalid limits or level, non-finite
/// values, fewer than two subgroups, an empty subgroup, no subgroup with two
/// or more values, a range subgroup outside 2–25 values, or no spread.
pub fn between_within_capability(
    groups: &[Vec<f64>],
    limits: &SpecLimits,
    options: &BetweenWithinOptions,
) -> Result<BetweenWithin> {
    let level = options.confidence_level;
    check_limits(limits, level)?;
    if groups.len() < 2 {
        return Err(invalid("at least two subgroups are needed"));
    }
    if groups.iter().any(Vec::is_empty) {
        return Err(invalid("every subgroup needs at least one value"));
    }
    if groups.iter().flatten().any(|v| !v.is_finite()) {
        return Err(invalid("values must be finite"));
    }
    let k = groups.len();
    let all: Vec<f64> = groups.iter().flatten().copied().collect();
    let n = all.len();
    let grand = mean(&all);
    let means: Vec<f64> = groups.iter().map(|g| mean(g)).collect();
    let df_within: usize = groups.iter().map(|g| g.len() - 1).sum();
    if df_within == 0 {
        return Err(invalid("at least one subgroup needs two or more values to estimate the within variation"));
    }
    let ss_within: f64 = groups.iter().map(|g| sum_sq(g)).sum();
    let msw = ss_within / df_within as f64;
    let (sigma_within, sigma_between, sigma_means, effective_size) = match options.between {
        BetweenEstimate::MovingRange => {
            let sigma_w = match options.within {
                WithinEstimate::PooledStdDev => msw.sqrt() / c4(df_within + 1)?,
                WithinEstimate::AverageRange => {
                    let mut total = 0.0;
                    let mut used = 0usize;
                    for g in groups.iter().filter(|g| g.len() >= 2) {
                        if g.len() > MAX_RANGE_SUBGROUP {
                            return Err(invalid(format!(
                                "ranges need subgroups of at most {MAX_RANGE_SUBGROUP} values; use the pooled SD"
                            )));
                        }
                        let hi = g.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                        let lo = g.iter().copied().fold(f64::INFINITY, f64::min);
                        total += (hi - lo) / d2(g.len())?;
                        used += 1;
                    }
                    total / used as f64
                }
            };
            let mr = means.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>() / (k - 1) as f64;
            let sigma_means = mr / d2(2)?;
            let harmonic = k as f64 / groups.iter().map(|g| 1.0 / g.len() as f64).sum::<f64>();
            let between_sq = (sigma_means * sigma_means - sigma_w * sigma_w / harmonic).max(0.0);
            (sigma_w, between_sq.sqrt(), sigma_means, harmonic)
        }
        BetweenEstimate::VarianceComponents => {
            let msb = groups.iter().zip(&means).map(|(g, m)| g.len() as f64 * (m - grand) * (m - grand)).sum::<f64>()
                / (k - 1) as f64;
            let total = n as f64;
            let n0 = (total - groups.iter().map(|g| (g.len() * g.len()) as f64).sum::<f64>() / total) / (k - 1) as f64;
            let between_sq = ((msb - msw) / n0).max(0.0);
            (msw.sqrt(), between_sq.sqrt(), (sum_sq(&means) / (k - 1) as f64).sqrt(), n0)
        }
    };
    let sigma_between_within = (sigma_between * sigma_between + sigma_within * sigma_within).sqrt();
    let sigma_overall = (sum_sq(&all) / (n - 1) as f64).sqrt();
    if sigma_within.is_nan() || sigma_within <= 0.0 || sigma_overall <= 0.0 {
        return Err(invalid("the data have no spread within subgroups, so capability cannot be estimated"));
    }
    let below = limits.lower.map_or(0, |l| all.iter().filter(|&&v| v < l).count());
    let above = limits.upper.map_or(0, |u| all.iter().filter(|&&v| v > u).count());
    let ppm = |c: usize| 1e6 * c as f64 / n as f64;
    Ok(BetweenWithin {
        n,
        subgroups: k,
        mean: grand,
        within: indices(grand, sigma_within, limits, n, (n - 1) as f64, level)?,
        between_within: indices(grand, sigma_between_within, limits, n, (n - 1) as f64, level)?,
        overall: indices(grand, sigma_overall, limits, n, (n - 1) as f64, level)?,
        means,
        sigma_within,
        sigma_between,
        sigma_between_within,
        sigma_overall,
        sigma_means,
        effective_size,
        observed_ppm: (ppm(below), ppm(above)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol * b.abs().max(1.0), "{a} vs {b}");
    }

    /// Six batches of four with batch-to-batch shifts (one batch of three).
    fn batches() -> Vec<Vec<f64>> {
        vec![
            vec![10.2, 10.4, 10.1, 10.3],
            vec![10.8, 10.6, 10.9, 10.7],
            vec![9.9, 10.1, 10.0, 9.8],
            vec![10.5, 10.4, 10.7],
            vec![10.1, 10.3, 10.2, 10.0],
            vec![10.9, 11.1, 10.8, 11.0],
        ]
    }

    const LIMITS: SpecLimits = SpecLimits { lower: Some(9.0), upper: Some(12.0), target: None };

    /// References: an independent NumPy computation of the formulas in the
    /// module docs (see docs/validation.md).
    #[test]
    fn moving_range_between_and_pooled_within() {
        let r = between_within_capability(&batches(), &LIMITS, &BetweenWithinOptions::default()).unwrap();
        assert_eq!((r.n, r.subgroups), (23, 6));
        close(r.mean, MEAN, 1e-12);
        close(r.sigma_within, SW_POOLED, 1e-10);
        close(r.sigma_means, S_MEANS_MR, 1e-10);
        close(r.effective_size, HARMONIC, 1e-12);
        close(r.sigma_between, SB_MR, 1e-10);
        close(r.sigma_between_within, (SB_MR * SB_MR + SW_POOLED * SW_POOLED).sqrt(), 1e-10);
        close(r.sigma_overall, S_OVERALL, 1e-10);
        close(r.between_within.cp.unwrap(), 3.0 / (6.0 * r.sigma_between_within), 1e-12);
        close(r.between_within.cpk, (12.0 - MEAN).min(MEAN - 9.0) / (3.0 * r.sigma_between_within), 1e-12);
        assert!(r.within.cpk > r.between_within.cpk, "between variation lowers the index");
        let ranges = BetweenWithinOptions { within: WithinEstimate::AverageRange, ..Default::default() };
        let r = between_within_capability(&batches(), &LIMITS, &ranges).unwrap();
        close(r.sigma_within, SW_RANGE, 1e-8);
    }

    #[test]
    fn variance_components_between() {
        let o = BetweenWithinOptions { between: BetweenEstimate::VarianceComponents, ..Default::default() };
        let r = between_within_capability(&batches(), &LIMITS, &o).unwrap();
        close(r.sigma_within, SW_MSW, 1e-10);
        close(r.effective_size, N0, 1e-12);
        close(r.sigma_between, SB_VC, 1e-10);
        // No between variation: σ_b is zero, not negative.
        let flat = vec![vec![1.0, 2.0, 3.0], vec![3.0, 2.0, 1.0], vec![2.0, 1.0, 3.0]];
        let r = between_within_capability(&flat, &LIMITS, &o).unwrap();
        assert_eq!(r.sigma_between, 0.0);
        assert!(between_within_capability(&flat[..1], &LIMITS, &o).is_err());
        assert!(between_within_capability(&[vec![1.0], vec![2.0]], &LIMITS, &o).is_err());
        assert!(between_within_capability(&flat, &SpecLimits::default(), &o).is_err());
    }

    const MEAN: f64 = 10.426_086_956_521_74;
    const SW_POOLED: f64 = 0.134_058_060_147_880_55;
    const S_MEANS_MR: f64 = 0.543_552_514_277_692_2;
    const HARMONIC: f64 = 3.789_473_684_210_526_5;
    const SB_MR: f64 = 0.539_172_365_627_604_3;
    const S_OVERALL: f64 = 0.380_451_157_185_718_87;
    const SW_RANGE: f64 = 0.150_973_761_830_875_88;
    const SW_MSW: f64 = 0.132_102_158_923_148_78;
    const N0: f64 = 3.826_086_956_521_739_2;
    const SB_VC: f64 = 0.382_604_127_008_014_94;
}
