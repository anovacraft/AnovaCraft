//! Capability of non-normal processes: percentile-based performance indices
//! from a distribution fitted to the data, a Johnson transformation, or the
//! empirical percentiles.
//!
//! The normal-theory indices put the natural process spread at `6σ`, the
//! width between the 0.135% and 99.865% points of a normal distribution.
//! Clements (1989), "Process capability calculations for non-normal
//! distributions", *Quality Progress* 22(9), and ISO 22514-2 replace those
//! points by the percentiles `X₀.₀₀₁₃₅`, `X₀.₅` and `X₀.₉₉₈₆₅` of the process
//! distribution:
//!
//! ```text
//! Pp  = (USL − LSL) / (X₀.₉₉₈₆₅ − X₀.₀₀₁₃₅)
//! Ppl = (X₀.₅ − LSL) / (X₀.₅ − X₀.₀₀₁₃₅)
//! Ppu = (USL − X₀.₅) / (X₀.₉₉₈₆₅ − X₀.₅)
//! Ppk = min(Ppl, Ppu)
//! ```
//!
//! which reduce to the normal-theory `Pp`, `Ppk` (with the sample standard
//! deviation) for a normal fit. The expected nonconforming fractions are
//! `F̂(LSL)` and `1 − F̂(USL)` under the fitted distribution `F̂`, and the
//! *equivalent* `Ppk` is the normal-theory index with the same worse-side
//! fraction, `min(−Φ⁻¹(p_L), −Φ⁻¹(p_U)) / 3` (ISO 22514-2, the "Z" method).
//!
//! Percentiles come from:
//!
//! * a maximum-likelihood fit of one family ([`stat_core::distribution_fit`]),
//!   or of whichever of the fourteen families has the smallest AIC;
//! * a Johnson transformation `z = γ + η·g((x − ε)/λ)` (Slifker–Shapiro
//!   percentile fit; Chou, Polansky & Mason 1998, "Transforming non-normal
//!   data to normality in statistical process control", *Journal of Quality
//!   Technology* 30(2)): `X_p = g⁻¹(Φ⁻¹(p))`, `p_L = Φ(z(LSL))`;
//! * the sample itself (Hyndman–Fan type 6 percentiles, `p(n + 1)`-th order
//!   statistic, interpolated and clamped to the sample): with fewer than 740
//!   values the outer percentiles are the sample extremes, so these indices
//!   are only rough guides for small samples, and the expected rates are the
//!   observed ones.

use stat_core::descriptive::{QuantileMethod, quantile_sorted};
use stat_core::distribution_fit::{
    FitFamily, FittedDistribution, JohnsonFamily, JohnsonTransformation, fit_distribution, johnson_transformation,
};
use stat_core::distributions::{ContinuousDistribution, Normal};
use stat_core::{Result, StatError};

use crate::capability::SpecLimits;

/// How the process distribution is modelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum CapabilityModel {
    /// Normal-theory indices (the standard study; not handled here).
    #[default]
    Normal,
    /// A maximum-likelihood fit of this family.
    Distribution(FitFamily),
    /// The family with the smallest AIC among those that can be fitted.
    BestFit,
    /// A Johnson transformation to normality.
    Johnson,
    /// Empirical percentiles.
    Nonparametric,
}

impl CapabilityModel {
    /// Display name.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Normal => "Normal".to_owned(),
            Self::Distribution(f) => f.label().to_owned(),
            Self::BestFit => "Best fit by AIC".to_owned(),
            Self::Johnson => "Johnson transformation".to_owned(),
            Self::Nonparametric => "Nonparametric (sample percentiles)".to_owned(),
        }
    }

    /// Stable key for saved files and scripts: `normal`, a family key
    /// (e.g. `weibull`), `best-fit`, `johnson` or `nonparametric`.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Distribution(f) => f.key(),
            Self::BestFit => "best-fit",
            Self::Johnson => "johnson",
            Self::Nonparametric => "nonparametric",
        }
    }

    /// The model with this [`CapabilityModel::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "normal" => Some(Self::Normal),
            "best-fit" => Some(Self::BestFit),
            "johnson" => Some(Self::Johnson),
            "nonparametric" => Some(Self::Nonparametric),
            other => FitFamily::from_key(other).map(Self::Distribution),
        }
    }
}

/// The probabilities of the lower, middle and upper reference percentiles.
pub const REFERENCE_PROBABILITIES: [f64; 3] = [0.00135, 0.5, 0.99865];

/// What the percentiles were taken from.
#[derive(Debug, Clone, PartialEq)]
pub enum PercentileSource {
    /// A fitted distribution.
    Fitted(FittedDistribution),
    /// A Johnson transformation.
    Johnson(JohnsonTransformation),
    /// The sample.
    Sample,
}

/// Result of [`percentile_capability`].
#[derive(Debug, Clone, PartialEq)]
pub struct PercentileCapability {
    /// The model requested ([`CapabilityModel::BestFit`] stays as asked; the
    /// family chosen is in `source`).
    pub model: CapabilityModel,
    /// Where the percentiles came from.
    pub source: PercentileSource,
    /// AIC of every family tried for [`CapabilityModel::BestFit`], ascending
    /// (empty otherwise).
    pub candidates: Vec<(FitFamily, f64)>,
    /// `X₀.₀₀₁₃₅`, `X₀.₅`, `X₀.₉₉₈₆₅`.
    pub percentiles: [f64; 3],
    /// `Pp` (both limits needed).
    pub pp: Option<f64>,
    /// `Ppl` (lower limit needed).
    pub ppl: Option<f64>,
    /// `Ppu` (upper limit needed).
    pub ppu: Option<f64>,
    /// `Ppk = min(Ppl, Ppu)`.
    pub ppk: f64,
    /// Expected nonconforming parts per million below the lower and above the
    /// upper limit (the observed rates for the nonparametric model).
    pub expected_ppm: (f64, f64),
    /// Normal-theory `Ppk` with the same worse-side expected fraction (`None`
    /// when no nonconforming parts are expected on either side).
    pub ppk_equivalent: Option<f64>,
}

impl PercentileCapability {
    /// The cumulative distribution of the model at `x` (`None` for the
    /// nonparametric model).
    #[must_use]
    pub fn cdf(&self, x: f64) -> Option<f64> {
        match &self.source {
            PercentileSource::Fitted(f) => Some(f.cdf(x)),
            PercentileSource::Johnson(j) => Some(johnson_cdf(j, x)),
            PercentileSource::Sample => None,
        }
    }

    /// The density of the model at `x` (`None` for the nonparametric model).
    #[must_use]
    pub fn pdf(&self, x: f64) -> Option<f64> {
        match &self.source {
            PercentileSource::Fitted(f) => Some(f.pdf(x)),
            PercentileSource::Johnson(j) => {
                // f(x) = φ(z(x)) · dz/dx, by a central difference.
                let h = 1e-6 * x.abs().max(j.lambda.abs()).max(1e-12);
                let (a, b) = (j.transform(x - h), j.transform(x + h));
                let z = j.transform(x);
                Some(if a.is_finite() && b.is_finite() && z.is_finite() {
                    (-0.5 * z * z).exp() / (2.0 * std::f64::consts::PI).sqrt() * (b - a) / (2.0 * h)
                } else {
                    0.0
                })
            }
            PercentileSource::Sample => None,
        }
    }

    /// The estimated threshold (lower end of the support) of a fitted
    /// family with one, else `None`.
    #[must_use]
    pub fn lower_bound(&self) -> Option<f64> {
        match &self.source {
            PercentileSource::Fitted(f) if f.family.has_threshold() => Some(f.lower_bound()),
            _ => None,
        }
    }

    /// The model's quantile at probability `p` (empirical for the
    /// nonparametric model, which needs `sorted`, the data in ascending order).
    #[must_use]
    pub fn quantile(&self, p: f64, sorted: &[f64]) -> f64 {
        match &self.source {
            PercentileSource::Fitted(f) => f.quantile(p),
            PercentileSource::Johnson(j) => standard_normal().quantile(p).map_or(f64::NAN, |z| j.inverse(z)),
            PercentileSource::Sample => quantile_sorted(sorted, p, QuantileMethod::Weibull).unwrap_or(f64::NAN),
        }
    }
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn standard_normal() -> Normal {
    Normal::standard()
}

/// `Φ(z)`.
fn phi(z: f64) -> f64 {
    standard_normal().cdf(z).unwrap_or(f64::NAN)
}

/// `P(X ≤ x)` under a Johnson transformation: `Φ(z(x))`, with 0 or 1 outside
/// the support of a bounded or log-normal (SB, SL) member.
fn johnson_cdf(j: &JohnsonTransformation, x: f64) -> f64 {
    let z = j.transform(x);
    if z.is_finite() {
        phi(z)
    } else if x <= j.epsilon || j.family == JohnsonFamily::Su {
        0.0
    } else {
        1.0
    }
}

/// Percentile-based capability of `values` against `limits` under `model`
/// (see the module documentation). Only the overall (performance) indices are
/// defined: a non-normal model has no short-term σ to scale.
///
/// # Errors
/// [`StatError::InvalidArgument`] for [`CapabilityModel::Normal`] (use
/// [`crate::process_capability`]), no limits, `LSL ≥ USL`, non-finite data or
/// limits, too few values (four; ten for the nonparametric model), a family
/// that cannot be fitted (e.g. a Weibull to non-positive data), no family
/// fitting for best fit, a failed Johnson fit, or percentiles that coincide
/// (no spread).
pub fn percentile_capability(
    values: &[f64],
    limits: &SpecLimits,
    model: CapabilityModel,
) -> Result<PercentileCapability> {
    if values.iter().any(|v| !v.is_finite()) {
        return Err(invalid("values must be finite"));
    }
    if limits.lower.is_none() && limits.upper.is_none() {
        return Err(invalid("give at least one specification limit"));
    }
    let finite = |v: Option<f64>| v.is_none_or(f64::is_finite);
    if !(finite(limits.lower) && finite(limits.upper)) {
        return Err(invalid("specification limits must be finite"));
    }
    if let (Some(l), Some(u)) = (limits.lower, limits.upper) {
        if l >= u {
            return Err(invalid("the lower specification limit must be below the upper one"));
        }
    }
    let n = values.len();
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let mut candidates = Vec::new();
    let source = match model {
        CapabilityModel::Normal => {
            return Err(invalid("normal-theory capability is computed by process_capability"));
        }
        CapabilityModel::Distribution(family) => PercentileSource::Fitted(fit_distribution(values, family)?),
        CapabilityModel::BestFit => {
            let mut best: Option<FittedDistribution> = None;
            for family in FitFamily::ALL {
                if let Ok(fit) = fit_distribution(values, family) {
                    if fit.aic().is_finite() {
                        candidates.push((family, fit.aic()));
                        if best.as_ref().is_none_or(|b| fit.aic() < b.aic()) {
                            best = Some(fit);
                        }
                    }
                }
            }
            candidates.sort_by(|a, b| a.1.total_cmp(&b.1));
            PercentileSource::Fitted(best.ok_or_else(|| invalid("no distribution could be fitted"))?)
        }
        CapabilityModel::Johnson => PercentileSource::Johnson(johnson_transformation(values)?),
        CapabilityModel::Nonparametric => {
            if n < 10 {
                return Err(invalid("sample percentiles need at least ten values"));
            }
            PercentileSource::Sample
        }
    };
    let mut out = PercentileCapability {
        model,
        source,
        candidates,
        percentiles: [f64::NAN; 3],
        pp: None,
        ppl: None,
        ppu: None,
        ppk: f64::NAN,
        expected_ppm: (0.0, 0.0),
        ppk_equivalent: None,
    };
    let [lo, mid, hi] = REFERENCE_PROBABILITIES.map(|p| out.quantile(p, &sorted));
    if !([lo, mid, hi].iter().all(|v| v.is_finite()) && lo < mid && mid < hi) {
        return Err(invalid("the fitted percentiles have no spread, so capability cannot be estimated"));
    }
    out.percentiles = [lo, mid, hi];
    out.pp = match (limits.lower, limits.upper) {
        (Some(l), Some(u)) => Some((u - l) / (hi - lo)),
        _ => None,
    };
    out.ppl = limits.lower.map(|l| (mid - l) / (mid - lo));
    out.ppu = limits.upper.map(|u| (u - mid) / (hi - mid));
    out.ppk = match (out.ppl, out.ppu) {
        (Some(a), Some(b)) => a.min(b),
        (Some(a), None) | (None, Some(a)) => a,
        (None, None) => unreachable!("a limit was checked above"),
    };
    let (p_low, p_high) = match &out.source {
        PercentileSource::Fitted(f) => {
            (limits.lower.map_or(0.0, |l| f.cdf(l)), limits.upper.map_or(0.0, |u| f.ln_sf(u).exp()))
        }
        PercentileSource::Johnson(j) => (
            limits.lower.map_or(0.0, |l| johnson_cdf(j, l)),
            limits.upper.map_or(0.0, |u| {
                let z = j.transform(u);
                if z.is_finite() { standard_normal().sf(z).unwrap_or(f64::NAN) } else { 1.0 - johnson_cdf(j, u) }
            }),
        ),
        PercentileSource::Sample => (
            limits.lower.map_or(0.0, |l| values.iter().filter(|&&v| v < l).count() as f64 / n as f64),
            limits.upper.map_or(0.0, |u| values.iter().filter(|&&v| v > u).count() as f64 / n as f64),
        ),
    };
    out.expected_ppm = (1e6 * p_low, 1e6 * p_high);
    let z = |p: f64, side: Option<f64>| -> Option<f64> {
        side?;
        (p > 0.0).then(|| standard_normal().quantile(p).map(|q| -q / 3.0).ok()).flatten()
    };
    out.ppk_equivalent = match (z(p_low, limits.lower), z(p_high, limits.upper)) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    Ok(out)
}

/// Bootstrap percentile intervals for the indices of a
/// [`PercentileCapability`] (see [`bootstrap_percentile_intervals`]).
#[derive(Debug, Clone, PartialEq)]
pub struct BootstrapIntervals {
    /// Two-sided confidence level.
    pub level: f64,
    /// Resamples requested.
    pub resamples: usize,
    /// Resamples whose model could be fitted and whose indices were used.
    pub used: usize,
    /// Seed of the resampling.
    pub seed: u64,
    /// Interval for `Pp` (both limits given).
    pub pp: Option<(f64, f64)>,
    /// Interval for `Ppl` (lower limit given).
    pub ppl: Option<(f64, f64)>,
    /// Interval for `Ppu` (upper limit given).
    pub ppu: Option<(f64, f64)>,
    /// Interval for `Ppk`.
    pub ppk: (f64, f64),
}

/// Smallest number of usable resamples for which intervals are reported.
pub const MIN_BOOTSTRAP_RESAMPLES: usize = 20;

/// Bootstrap percentile intervals (Efron & Tibshirani 1993, *An Introduction
/// to the Bootstrap*, ch. 13) for the percentile-based indices of `estimate`,
/// computed from `values` against `limits`.
///
/// Each of `resamples` resamples draws `n` values with replacement (SplitMix64
/// seeded with `seed`, so results are reproducible) and recomputes the
/// indices under the same model; for [`CapabilityModel::BestFit`] the family
/// chosen on the full sample is refitted, so the intervals are conditional on
/// that choice. A resample whose model cannot be fitted is skipped and not
/// counted in `used`. The interval for each index is the `α/2` and `1 − α/2`
/// sample quantiles (Hyndman–Fan type 7) of the resampled values, with
/// `α = 1 − level`.
///
/// # Errors
/// [`StatError::InvalidArgument`] for a level outside (0, 1), fewer than
/// [`MIN_BOOTSTRAP_RESAMPLES`] resamples requested, or fewer than that many
/// resamples that could be fitted; errors of [`percentile_capability`] on
/// invalid data or limits.
pub fn bootstrap_percentile_intervals(
    values: &[f64],
    limits: &SpecLimits,
    estimate: &PercentileCapability,
    resamples: usize,
    seed: u64,
    level: f64,
) -> Result<BootstrapIntervals> {
    if !(level > 0.0 && level < 1.0) {
        return Err(invalid("the confidence level must lie between 0 and 1"));
    }
    if resamples < MIN_BOOTSTRAP_RESAMPLES {
        return Err(invalid(format!("use at least {MIN_BOOTSTRAP_RESAMPLES} bootstrap resamples")));
    }
    let model = match (&estimate.source, estimate.model) {
        (PercentileSource::Fitted(f), CapabilityModel::BestFit) => CapabilityModel::Distribution(f.family),
        (_, m) => m,
    };
    let n = values.len();
    let mut rng = stat_core::rng::Rng::new(seed);
    let mut sample = vec![0.0; n];
    let (mut pp, mut ppl, mut ppu, mut ppk) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for _ in 0..resamples {
        for v in &mut sample {
            *v = values[rng.below(n)];
        }
        let Ok(c) = percentile_capability(&sample, limits, model) else {
            continue;
        };
        if !c.ppk.is_finite() {
            continue;
        }
        pp.extend(c.pp.filter(|v| v.is_finite()));
        ppl.extend(c.ppl.filter(|v| v.is_finite()));
        ppu.extend(c.ppu.filter(|v| v.is_finite()));
        ppk.push(c.ppk);
    }
    if ppk.len() < MIN_BOOTSTRAP_RESAMPLES {
        return Err(invalid(format!(
            "only {} of {resamples} bootstrap resamples could be fitted, too few for intervals",
            ppk.len()
        )));
    }
    let alpha = 1.0 - level;
    let interval = |mut v: Vec<f64>| -> Option<(f64, f64)> {
        if v.len() < MIN_BOOTSTRAP_RESAMPLES {
            return None;
        }
        v.sort_by(f64::total_cmp);
        let q = |p: f64| quantile_sorted(&v, p, QuantileMethod::Linear).ok();
        Some((q(alpha / 2.0)?, q(1.0 - alpha / 2.0)?))
    };
    let used = ppk.len();
    Ok(BootstrapIntervals {
        level,
        resamples,
        used,
        seed,
        pp: estimate.pp.and_then(|_| interval(pp)),
        ppl: estimate.ppl.and_then(|_| interval(ppl)),
        ppu: estimate.ppu.and_then(|_| interval(ppu)),
        ppk: interval(ppk).ok_or_else(|| invalid("too few bootstrap resamples could be fitted"))?,
    })
}

#[cfg(test)]
mod tests;
