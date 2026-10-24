//! Process capability: how well a process fits its specification limits
//! (Montgomery, *Introduction to Statistical Quality Control*, ch. 8; NIST/SEMATECH
//! e-Handbook §6.1.6; Kotz & Johnson, *Process Capability Indices*).
//!
//! With mean `μ`, specification limits `LSL`, `USL` and a standard deviation `σ`:
//!
//! ```text
//! Cp  = (USL − LSL) / 6σ        Cpl = (μ − LSL) / 3σ
//! Cpu = (USL − μ) / 3σ          Cpk = min(Cpl, Cpu)
//! Cpm = (USL − LSL) / 6√(s² + (μ − T)²)   (target T)
//! ```
//!
//! The **within** (potential) indices `Cp, Cpk` use the short-term σ — from the
//! average moving range `MR̄/d₂`, the average subgroup range `R̄/d₂(m)`, or the
//! pooled subgroup standard deviation `s_p/c₄(d + 1)` with `d` pooled degrees of
//! freedom. The **overall** (performance) indices `Pp, Ppk` use the sample
//! standard deviation `s`. With one limit only, the one-sided index is reported
//! as `Cpk`/`Ppk` and `Cp`/`Pp` are undefined.
//!
//! Confidence intervals: `Cp·√(χ²_{α/2, ν}/ν)` to `Cp·√(χ²_{1−α/2, ν}/ν)`, and
//! Bissell's (1990) approximation `Cpk ± z_{1−α/2} √(1/(9n) + Cpk²/(2ν))`, where
//! `ν` is the degrees of freedom of the σ estimate. For the overall indices
//! `ν = n − 1` (the `Pp` interval is then exact under normality). For the
//! within indices `ν` is that of the estimator: `k(m − 1)` for the pooled
//! standard deviation of `k` subgroups of size `m`, and for the range
//! estimators the effective (Patnaik 1950) degrees of freedom matching the
//! estimator's relative variance, `ν = 1/(2·Var(σ̂)/σ²)`:
//! `ν = k·d₂(m)²/(2·d₃(m)²)` for `R̄/d₂` (about `0.9·k(m − 1)`), and for
//! `MR̄/d₂` with `M = n − 1` moving ranges, which are correlated,
//! `ν = (4/π)·M² / (2·(2(1 − 2/π)·M + 2·(4/π)(√3/2 + π/12 − 1)·(M − 1)))`
//! (about `0.6·(n − 1)`; Cryer & Ryan 1990). These within intervals remain
//! approximations.
//!
//! For non-normal positive data a Box–Cox power `λ` may be given: the data,
//! limits and target are transformed (the transform is increasing, so the
//! limits keep their order) and the indices are computed on that scale.
//! Observed nonconforming rates always use the original data and limits.

use crate::constants::{c4, d2, d3};
use stat_core::box_cox::box_cox_transform;
use stat_core::distributions::{ChiSquared, ContinuousDistribution, Normal};
use stat_core::{Result, StatError};

/// Specification limits (at least one of `lower`, `upper`) and an optional target.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SpecLimits {
    /// Lower specification limit.
    pub lower: Option<f64>,
    /// Upper specification limit.
    pub upper: Option<f64>,
    /// Target value (for `Cpm`).
    pub target: Option<f64>,
}

/// How the short-term (within) standard deviation is estimated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WithinSigma {
    /// Average moving range of consecutive values ÷ d₂(2): individual observations.
    #[default]
    MovingRange,
    /// Average range of consecutive subgroups of this size ÷ d₂(m).
    AverageRange {
        /// Subgroup size (2 to 25).
        subgroup_size: usize,
    },
    /// Pooled standard deviation of consecutive subgroups of this size ÷ c₄(d + 1).
    PooledStdDev {
        /// Subgroup size (at least 2).
        subgroup_size: usize,
    },
}

/// Options for [`process_capability`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CapabilityOptions {
    /// Within-σ estimator.
    pub within: WithinSigma,
    /// Confidence level of the intervals, e.g. 0.95.
    pub confidence_level: f64,
    /// Box–Cox power applied before computing the indices (`None`: normal theory on the raw data).
    pub box_cox: Option<f64>,
}

impl Default for CapabilityOptions {
    fn default() -> Self {
        Self { within: WithinSigma::MovingRange, confidence_level: 0.95, box_cox: None }
    }
}

/// One set of indices (within or overall).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CapabilityIndices {
    /// The σ used.
    pub sigma: f64,
    /// `Cp` / `Pp` (both limits needed).
    pub cp: Option<f64>,
    /// `Cpl` / `Ppl`.
    pub cpl: Option<f64>,
    /// `Cpu` / `Ppu`.
    pub cpu: Option<f64>,
    /// `Cpk` / `Ppk`.
    pub cpk: f64,
    /// Confidence interval for `cp`.
    pub cp_interval: Option<(f64, f64)>,
    /// Confidence interval for `cpk` (Bissell).
    pub cpk_interval: (f64, f64),
    /// Expected nonconforming parts per million below the lower and above the
    /// upper limit, under a normal distribution with this σ.
    pub expected_ppm: (f64, f64),
    /// Degrees of freedom of the σ estimate used for the intervals (see the
    /// module docs; fractional for the range estimators).
    pub degrees_of_freedom: f64,
}

/// Result of [`process_capability`].
#[derive(Debug, Clone, PartialEq)]
pub struct Capability {
    /// Number of observations.
    pub n: usize,
    /// Mean (on the analysis scale).
    pub mean: f64,
    /// Limits and target on the analysis scale.
    pub limits: SpecLimits,
    /// Short-term indices `Cp`, `Cpk`.
    pub within: CapabilityIndices,
    /// Long-term indices `Pp`, `Ppk`.
    pub overall: CapabilityIndices,
    /// Taguchi index `Cpm` (needs both limits and a target).
    pub cpm: Option<f64>,
    /// Observed nonconforming parts per million below and above the limits.
    pub observed_ppm: (f64, f64),
    /// Box–Cox power used, if any.
    pub box_cox: Option<f64>,
}

/// Compute capability indices of `values` (in time order) against `limits`.
///
/// # Errors
/// [`StatError::InvalidArgument`] for no limits, `LSL ≥ USL`, non-finite data,
/// too few values (two, or two complete subgroups), a zero spread, a bad
/// confidence level or subgroup size, or a Box–Cox power with non-positive
/// data or limits.
pub fn process_capability(values: &[f64], limits: &SpecLimits, options: &CapabilityOptions) -> Result<Capability> {
    if values.iter().any(|v| !v.is_finite()) {
        return Err(invalid("values must be finite"));
    }
    check_limits(limits, options.confidence_level)?;
    let level = options.confidence_level;
    let n = values.len();
    if n < 2 {
        return Err(invalid("at least two observations are needed"));
    }
    // Analysis scale.
    let (x, scaled) = match options.box_cox {
        None => (values.to_vec(), *limits),
        Some(lambda) => {
            let t = |v: Option<f64>| -> Result<Option<f64>> {
                v.map(|v| {
                    box_cox_transform(&[v], lambda)
                        .map(|t| t[0])
                        .map_err(|_| invalid("Box–Cox needs positive specification limits and target"))
                })
                .transpose()
            };
            let x = box_cox_transform(values, lambda).map_err(|_| invalid("Box–Cox needs positive data"))?;
            (x, SpecLimits { lower: t(limits.lower)?, upper: t(limits.upper)?, target: t(limits.target)? })
        }
    };
    let mean = x.iter().sum::<f64>() / n as f64;
    let overall_sigma = (x.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / (n - 1) as f64).sqrt();
    let (within_sigma, within_df) = within_sigma(&x, options.within)?;
    if !(overall_sigma > 0.0 && within_sigma > 0.0) {
        return Err(invalid("the data have no spread, so capability cannot be estimated"));
    }
    let within = indices(mean, within_sigma, &scaled, n, within_df, level)?;
    let overall = indices(mean, overall_sigma, &scaled, n, (n - 1) as f64, level)?;
    let cpm = match (scaled.lower, scaled.upper, scaled.target) {
        (Some(l), Some(u), Some(t)) => {
            Some((u - l) / (6.0 * (overall_sigma * overall_sigma + (mean - t) * (mean - t)).sqrt()))
        }
        _ => None,
    };
    let below = limits.lower.map_or(0, |l| values.iter().filter(|&&v| v < l).count());
    let above = limits.upper.map_or(0, |u| values.iter().filter(|&&v| v > u).count());
    let per_million = |k: usize| 1e6 * k as f64 / n as f64;
    Ok(Capability {
        n,
        mean,
        limits: scaled,
        within,
        overall,
        cpm,
        observed_ppm: (per_million(below), per_million(above)),
        box_cox: options.box_cox,
    })
}

/// Check specification limits (at least one, finite, ordered) and a
/// confidence level in (0, 1).
pub(crate) fn check_limits(limits: &SpecLimits, level: f64) -> Result<()> {
    if limits.lower.is_none() && limits.upper.is_none() {
        return Err(invalid("give at least one specification limit"));
    }
    let finite = |v: Option<f64>| v.is_none_or(f64::is_finite);
    if !(finite(limits.lower) && finite(limits.upper) && finite(limits.target)) {
        return Err(invalid("specification limits must be finite"));
    }
    if let (Some(l), Some(u)) = (limits.lower, limits.upper) {
        if l >= u {
            return Err(invalid("the lower specification limit must be below the upper one"));
        }
    }
    if !(level > 0.0 && level < 1.0) {
        return Err(invalid("the confidence level must be between 0 and 1"));
    }
    Ok(())
}

/// The within σ and its degrees of freedom (see the module docs).
fn within_sigma(x: &[f64], method: WithinSigma) -> Result<(f64, f64)> {
    let subgroups = |m: usize| -> Result<Vec<&[f64]>> {
        if m < 2 {
            return Err(invalid("subgroups need at least two observations"));
        }
        let groups: Vec<&[f64]> = x.chunks_exact(m).collect();
        if groups.len() < 2 {
            return Err(invalid("at least two complete subgroups are needed"));
        }
        Ok(groups)
    };
    match method {
        WithinSigma::MovingRange => {
            let mr = x.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>() / (x.len() - 1) as f64;
            Ok((mr / d2(2)?, moving_range_df(x.len() - 1)))
        }
        WithinSigma::AverageRange { subgroup_size } => {
            let groups = subgroups(subgroup_size)?;
            let range = |g: &[f64]| {
                g.iter().copied().fold(f64::NEG_INFINITY, f64::max) - g.iter().copied().fold(f64::INFINITY, f64::min)
            };
            let r_bar = groups.iter().map(|g| range(g)).sum::<f64>() / groups.len() as f64;
            let (d2, d3) = (d2(subgroup_size)?, d3(subgroup_size)?);
            Ok((r_bar / d2, groups.len() as f64 * d2 * d2 / (2.0 * d3 * d3)))
        }
        WithinSigma::PooledStdDev { subgroup_size } => {
            let groups = subgroups(subgroup_size)?;
            let mut ss = 0.0;
            for g in &groups {
                let m = g.iter().sum::<f64>() / g.len() as f64;
                ss += g.iter().map(|v| (v - m) * (v - m)).sum::<f64>();
            }
            let df = groups.len() * (subgroup_size - 1);
            Ok(((ss / df as f64).sqrt() / c4(df + 1)?, df as f64))
        }
    }
}

/// Effective degrees of freedom of `MR̄/d₂(2)` from `m` moving ranges of
/// independent normal values. Each range is `|D|` with `D ~ N(0, 2σ²)`, so
/// `Var|D| = 2σ²(1 − 2/π)`; neighbouring differences have correlation −½,
/// giving `Cov(|Dᵢ|, |Dᵢ₊₁|) = (4/π)σ²(√(1 − ρ²) + ρ·asin ρ − 1)` with ρ = −½;
/// ranges further apart are independent. Then `ν = 1/(2·Var(σ̂)/σ²)`.
fn moving_range_df(m: usize) -> f64 {
    use std::f64::consts::PI;
    let m = m as f64;
    let var_one = 2.0 * (1.0 - 2.0 / PI);
    let rho: f64 = -0.5;
    let cov_next = 4.0 / PI * ((1.0 - rho * rho).sqrt() + rho * rho.asin() - 1.0);
    // Var(MR̄)/σ², divided by d₂(2)² = 4/π.
    let rel_var = (m * var_one + 2.0 * (m - 1.0) * cov_next) / (m * m) / (4.0 / PI);
    1.0 / (2.0 * rel_var)
}

/// Indices, intervals and expected nonconforming rates for one σ (see the
/// module docs); `n` observations and the σ estimate's degrees of freedom `df`
/// set the interval widths.
pub(crate) fn indices(
    mean: f64,
    sigma: f64,
    limits: &SpecLimits,
    n: usize,
    df: f64,
    level: f64,
) -> Result<CapabilityIndices> {
    let cp = match (limits.lower, limits.upper) {
        (Some(l), Some(u)) => Some((u - l) / (6.0 * sigma)),
        _ => None,
    };
    let cpl = limits.lower.map(|l| (mean - l) / (3.0 * sigma));
    let cpu = limits.upper.map(|u| (u - mean) / (3.0 * sigma));
    let cpk = cpl.into_iter().chain(cpu).fold(f64::INFINITY, f64::min);
    let alpha = 1.0 - level;
    let chi = ChiSquared::new(df)?;
    let (lo_q, hi_q) = (chi.quantile(alpha / 2.0)?, chi.quantile(1.0 - alpha / 2.0)?);
    let cp_interval = cp.map(|c| (c * (lo_q / df).sqrt(), c * (hi_q / df).sqrt()));
    let z = Normal::standard().quantile(1.0 - alpha / 2.0)?;
    let half = z * (1.0 / (9.0 * n as f64) + cpk * cpk / (2.0 * df)).sqrt();
    let normal = Normal::standard();
    let below = limits.lower.map_or(Ok(0.0), |l| normal.cdf((l - mean) / sigma))?;
    let above = limits.upper.map_or(Ok(0.0), |u| normal.sf((u - mean) / sigma))?;
    Ok(CapabilityIndices {
        sigma,
        cp,
        cpl,
        cpu,
        cpk,
        cp_interval,
        cpk_interval: (cpk - half, cpk + half),
        expected_ppm: (1e6 * below, 1e6 * above),
        degrees_of_freedom: df,
    })
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Piston-ring inside diameters (mm), in production order; limits 74.00 ± 0.05.
    /// Reference values from NumPy/SciPy with the formulas in the module docs.
    const RINGS: [f64; 20] = [
        74.030, 74.002, 74.019, 73.992, 74.008, 73.995, 73.992, 74.001, 74.011, 74.004, 73.988, 74.024, 74.021, 74.005,
        74.002, 74.002, 73.996, 73.993, 74.015, 74.009,
    ];
    const SPEC: SpecLimits = SpecLimits { lower: Some(73.95), upper: Some(74.05), target: Some(74.0) };

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol * b.abs().max(1.0), "{a} vs {b}");
    }

    #[test]
    fn two_sided_indices_and_intervals() {
        let c = process_capability(&RINGS, &SPEC, &CapabilityOptions::default()).unwrap();
        close(c.mean, 74.005_45, 1e-12);
        close(c.within.sigma, 0.011_241_088_896_532_336, 1e-12);
        close(c.overall.sigma, 0.011_677_576_352_723_71, 1e-12);
        close(c.within.cp.unwrap(), 1.482_655_890_374_381_2, 1e-10);
        close(c.within.cpk, 1.321_046_398_323_256_3, 1e-10);
        close(c.overall.cp.unwrap(), 1.427_236_796_681_559_6, 1e-10);
        close(c.overall.cpl.unwrap(), 1.582_805_607_520_155, 1e-10);
        close(c.overall.cpk, 1.271_667_985_842_964, 1e-10);
        let (lo, hi) = c.overall.cp_interval.unwrap();
        close(lo, 0.977_176_854_175_923, 1e-9);
        close(hi, 1.876_732_181_396_197_5, 1e-9);
        close(c.overall.cpk_interval.0, 0.841_761_346_445_657_5, 1e-9);
        close(c.overall.cpk_interval.1, 1.701_574_625_240_270_6, 1e-9);
        close(c.cpm.unwrap(), 1.293_318_212_707_013, 1e-10);
        close(c.overall.expected_ppm.0, 1.025_075_856_640_972, 1e-8);
        close(c.overall.expected_ppm.1, 68.090_375_530_100_87, 1e-8);
        close(c.within.expected_ppm.1, 36.985_311_615_907_776, 1e-8);
        assert_eq!(c.observed_ppm, (0.0, 0.0));
    }

    #[test]
    fn subgroup_estimators_and_one_sided_limits() {
        let pooled = CapabilityOptions { within: WithinSigma::PooledStdDev { subgroup_size: 5 }, ..Default::default() };
        let c = process_capability(&RINGS, &SPEC, &pooled).unwrap();
        close(c.within.sigma, 0.012_172_967_842_158_348, 1e-12);
        close(c.within.cpk, 1.219_916_144_735_546, 1e-10);
        // Average range of 4 subgroups of 5: R̄/d₂(5).
        let ranges = CapabilityOptions { within: WithinSigma::AverageRange { subgroup_size: 5 }, ..Default::default() };
        let r = process_capability(&RINGS, &SPEC, &ranges).unwrap();
        let r_bar: f64 = RINGS
            .chunks(5)
            .map(|g| g.iter().copied().fold(f64::MIN, f64::max) - g.iter().copied().fold(f64::MAX, f64::min))
            .sum::<f64>()
            / 4.0;
        close(r.within.sigma, r_bar / 2.325_929_419_7, 1e-6);
        // Upper limit only: Cpk = Cpu, no Cp, no lower tail.
        let upper = SpecLimits { lower: None, upper: Some(74.02), target: None };
        let u = process_capability(&RINGS, &upper, &CapabilityOptions::default()).unwrap();
        assert!(u.within.cp.is_none() && u.within.cpl.is_none() && u.cpm.is_none());
        assert_eq!(u.within.cpk, u.within.cpu.unwrap());
        assert_eq!(u.within.expected_ppm.0, 0.0);
        // Three of the 20 rings exceed 74.02 (74.030, 74.024, 74.021): 150 000 ppm observed.
        assert_eq!(u.observed_ppm, (0.0, 150_000.0));
    }

    /// The within intervals use the degrees of freedom of the σ estimator.
    /// Effective ν checked against an independent NumPy simulation of
    /// Var(σ̂)/σ² (10⁶ replicates: 11.683 for MR̄/d₂ with n = 20, 14.47 for
    /// R̄/d₂ with 4 subgroups of 5); intervals against SciPy's χ² and normal
    /// quantiles with that ν.
    #[test]
    fn within_intervals_use_the_estimators_degrees_of_freedom() {
        let c = process_capability(&RINGS, &SPEC, &CapabilityOptions::default()).unwrap();
        close(c.within.degrees_of_freedom, 11.683, 0.005);
        close(c.overall.degrees_of_freedom, 19.0, 1e-15);
        let (lo, hi) = c.within.cp_interval.unwrap();
        close(lo, 0.890_659_479_345_961_7, 1e-9);
        close(hi, 2.075_251_929_089_867_4, 1e-9);
        close(c.within.cpk_interval.0, 0.765_890_864_472_056_9, 1e-9);
        close(c.within.cpk_interval.1, 1.876_201_932_174_455_9, 1e-9);
        let ranges = CapabilityOptions { within: WithinSigma::AverageRange { subgroup_size: 5 }, ..Default::default() };
        let r = process_capability(&RINGS, &SPEC, &ranges).unwrap();
        close(r.within.degrees_of_freedom, 14.47, 0.005 * 14.47);
        let pooled = CapabilityOptions { within: WithinSigma::PooledStdDev { subgroup_size: 5 }, ..Default::default() };
        let p = process_capability(&RINGS, &SPEC, &pooled).unwrap();
        close(p.within.degrees_of_freedom, 16.0, 1e-15);
        // For 50 individual values ν ≈ 0.6·(n − 1), the figure usually quoted.
        assert!((moving_range_df(49) / (0.62 * 49.0) - 1.0).abs() < 0.03);
    }

    #[test]
    fn box_cox_scale_and_invalid_input() {
        // λ = 1 is a shift: identical indices.
        let one = CapabilityOptions { box_cox: Some(1.0), ..Default::default() };
        let a = process_capability(&RINGS, &SPEC, &one).unwrap();
        let b = process_capability(&RINGS, &SPEC, &CapabilityOptions::default()).unwrap();
        close(a.overall.cpk, b.overall.cpk, 1e-9);
        // Log scale: limits transformed, order kept.
        let log = CapabilityOptions { box_cox: Some(0.0), ..Default::default() };
        let l = process_capability(&RINGS, &SPEC, &log).unwrap();
        close(l.limits.upper.unwrap(), 74.05_f64.ln(), 1e-15);
        assert!(l.overall.cpk > 1.0);
        let neg = [-1.0, 2.0, 3.0];
        assert!(process_capability(&neg, &SPEC, &log).is_err());
        assert!(process_capability(&RINGS, &SpecLimits::default(), &CapabilityOptions::default()).is_err());
        let crossed = SpecLimits { lower: Some(2.0), upper: Some(1.0), target: None };
        assert!(process_capability(&RINGS, &crossed, &CapabilityOptions::default()).is_err());
        assert!(process_capability(&[1.0, 1.0, 1.0], &SPEC, &CapabilityOptions::default()).is_err());
        let small = CapabilityOptions { within: WithinSigma::PooledStdDev { subgroup_size: 15 }, ..Default::default() };
        assert!(process_capability(&RINGS, &SPEC, &small).is_err(), "only one complete subgroup");
    }
}
