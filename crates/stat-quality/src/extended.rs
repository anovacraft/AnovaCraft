//! Further Shewhart-type variables charts: moving-average, three-way
//! (between/within), zone and short-run standardized charts.
//!
//! They are built phase by phase by [`crate::phased::phased_chart_with`], so
//! each phase gets its own estimates and limits and run rules restart at
//! every phase. Within a phase, with `k` points (individual values `xᵢ`, or
//! subgroup means `x̄ᵢ` of sizes `nᵢ`) and σ̂ the within-subgroup process
//! standard deviation (`M̄R/d₂(2)` for individual values, the mean of
//! `Rᵢ/d₂(nᵢ)` for subgroups):
//!
//! **Moving average** of span `w` (Montgomery, *Introduction to Statistical
//! Quality Control*, §9.3): `Mᵢ` is the mean of the last `wᵢ = min(i, w)`
//! points, plotted against
//!
//! ```text
//! CL = x̿,   limits = x̿ ± 3 σ̂ √(Σⱼ 1/nⱼ) / wᵢ        (j over the wᵢ points averaged)
//! ```
//!
//! which is `x̿ ± 3σ̂/√(n wᵢ)` for equal subgroups of `n`. The companion chart
//! is the moving range (individual values) or range chart. Consecutive moving
//! averages share points, so only rule 1 is meaningful.
//!
//! **Three-way chart** (Wheeler 1995, *Advanced Topics in Statistical Process
//! Control*, SPC Press): when the subgroup means vary more than the
//! within-subgroup spread allows, the means are charted as individual values,
//! with limits from their own moving ranges `MRᵢ = |x̄ᵢ − x̄ᵢ₋₁|`:
//!
//! ```text
//! means:          CL = mean of x̄ᵢ,  limits = CL ± 3 M̄R/d₂(2)   (= CL ± 2.660 M̄R)
//! moving ranges:  CL = M̄R,          limits = 0, D₄(2) M̄R        (D₄(2) = 3.267)
//! within:         the usual R (or S) chart with σ̂ within subgroups
//! ```
//!
//! **Zone chart** (Jaehn 1987, "Zone control charts — SPC made easy",
//! *Quality* 26(10); run length properties in Davis, Homer & Woodall 1990,
//! *Communications in Statistics — Theory and Methods* 19(5)): the
//! individuals or X̄ chart, each point scored by the zone it falls in,
//! measured from the center line in units of the statistic's σ:
//!
//! | zone | within 1σ | 1σ–2σ | 2σ–3σ | beyond 3σ |
//! |---|---|---|---|---|
//! | score | 1 | 2 | 4 | 8 |
//!
//! Scores accumulate while points stay on one side of the center line; a
//! point on the other side (or on the line) starts a new sum with its own
//! score. A cumulative score of 8 or more signals, and the sum restarts after
//! the signal. (A boundary value belongs to the inner zone.)
//!
//! **Short-run Z chart** (Griffith 1996, *Statistical Process Control
//! Methods for Long and Short Runs*, 2nd ed., ASQ Quality Press; Wheeler
//! 1991, *Short Run SPC*): individual values of several part numbers in one
//! time-ordered chart. Each part `p` has its own mean `x̄ₚ` and
//! `σ̂ₚ = M̄Rₚ/d₂(2)`, from the moving ranges between consecutive values of
//! that part, and each value is standardized:
//!
//! ```text
//! Zᵢ = (xᵢ − x̄ₚ₍ᵢ₎) / σ̂ₚ₍ᵢ₎,         CL = 0,       limits = ±3
//! MR of Z: |Zᵢ − Zᵢ₋₁|,             CL = d₂(2),   limits = 0, d₂(2) + 3d₃(2)   (σ_Z = 1)
//! ```
//!
//! Every part needs at least two values (with some spread) in each phase.

use crate::charts::ControlLimits;
use crate::constants::{d2, d3};
use crate::phased::{PhaseSummary, VaryingChart};
use stat_core::summation::sum_iter;
use stat_core::{Result, StatError};

/// Scores of the zones within 1σ, 1σ–2σ, 2σ–3σ and beyond 3σ of the center
/// line (Jaehn 1987).
pub const ZONE_WEIGHTS: [u32; 4] = [1, 2, 4, 8];

/// Cumulative zone score at which a zone chart signals.
pub const ZONE_SIGNAL: u32 = 8;

/// One point's zone score on a zone chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoneScore {
    /// The point's own score ([`ZONE_WEIGHTS`]; 0 for a missing value).
    pub score: u32,
    /// The score accumulated on the point's side of the center line.
    pub cumulative: u32,
    /// The cumulative score reached [`ZONE_SIGNAL`].
    pub signal: bool,
}

/// One part number's estimates on a short-run chart.
#[derive(Debug, Clone, PartialEq)]
pub struct PartSummary {
    /// Index of the phase (0 for a single phase).
    pub phase: usize,
    /// The part number's index, as given to the chart.
    pub part: usize,
    /// Values of this part in the phase.
    pub n: usize,
    /// Mean of the part's values.
    pub mean: f64,
    /// `M̄Rₚ / d₂(2)` from moving ranges between consecutive values of the part.
    pub sigma: f64,
}

/// A point of a chart with its limits.
pub(crate) type Points = Vec<(Option<f64>, ControlLimits)>;

/// Moving averages of span `span` of `means` (subgroup sizes `sizes`), with
/// center `center` and within σ `sigma` (see the module documentation).
pub(crate) fn moving_average_points(means: &[f64], sizes: &[usize], sigma: f64, center: f64, span: usize) -> Points {
    (0..means.len())
        .map(|i| {
            let lo = (i + 1).saturating_sub(span);
            let w = (i + 1 - lo) as f64;
            let value = sum_iter(means[lo..=i].iter().copied()) / w;
            let spread = sum_iter(sizes[lo..=i].iter().map(|&n| 1.0 / n as f64)).sqrt() / w;
            let half = 3.0 * sigma * spread;
            (Some(value), ControlLimits { center, lower: center - half, upper: center + half })
        })
        .collect()
}

/// Location (Z) and moving-range points of one phase of a short-run chart,
/// with each part's estimates. `parts[i]` is the part index of value `x[i]`.
///
/// # Errors
/// [`StatError::InvalidArgument`] for a part with fewer than two values in
/// the phase, or whose values do not vary between consecutive readings.
pub(crate) fn short_run_points(x: &[f64], parts: &[usize], phase: usize) -> Result<(Points, Points, Vec<PartSummary>)> {
    let mut ids: Vec<usize> = parts.to_vec();
    ids.sort_unstable();
    ids.dedup();
    let mut summaries = Vec::with_capacity(ids.len());
    let mut by_part = vec![(f64::NAN, f64::NAN); x.len()];
    for &p in &ids {
        let values: Vec<f64> = x.iter().zip(parts).filter(|(_, q)| **q == p).map(|(v, _)| *v).collect();
        if values.len() < 2 {
            return Err(invalid(format!(
                "part {} has only one value in phase {}; each part needs at least two",
                p + 1,
                phase + 1
            )));
        }
        let mean = sum_iter(values.iter().copied()) / values.len() as f64;
        let mr = sum_iter(values.windows(2).map(|w| (w[1] - w[0]).abs())) / (values.len() - 1) as f64;
        let sigma = mr / d2(2)?;
        if sigma.is_nan() || sigma <= 0.0 {
            return Err(invalid(format!(
                "part {} does not vary in phase {}, so it cannot be standardized",
                p + 1,
                phase + 1
            )));
        }
        for (slot, _) in by_part.iter_mut().zip(parts).filter(|(_, q)| **q == p) {
            *slot = (mean, sigma);
        }
        summaries.push(PartSummary { phase, part: p, n: values.len(), mean, sigma });
    }
    let z: Vec<f64> = x.iter().zip(&by_part).map(|(v, (m, s))| (v - m) / s).collect();
    let unit = ControlLimits { center: 0.0, lower: -3.0, upper: 3.0 };
    let (d2v, d3v) = (d2(2)?, d3(2)?);
    let mr_limits = ControlLimits { center: d2v, lower: (d2v - 3.0 * d3v).max(0.0), upper: d2v + 3.0 * d3v };
    let location = z.iter().map(|&v| (Some(v), unit)).collect();
    let dispersion =
        (0..z.len()).map(|i| (if i == 0 { None } else { Some((z[i] - z[i - 1]).abs()) }, mr_limits)).collect();
    Ok((location, dispersion, summaries))
}

/// Zone scores of a location chart's points (see the module documentation),
/// restarting at each phase.
#[must_use]
pub fn zone_scores(chart: &VaryingChart, phases: &[PhaseSummary]) -> Vec<ZoneScore> {
    let mut out = vec![ZoneScore { score: 0, cumulative: 0, signal: false }; chart.values.len()];
    for p in phases {
        let (mut side, mut total) = (0_i8, 0_u32);
        for i in p.start..p.end {
            let Some(v) = chart.values[i] else {
                (side, total) = (0, 0);
                continue;
            };
            let l = &chart.limits[i];
            let s = l.zone_sigma();
            let z = if s > 0.0 { (v - l.center) / s } else { 0.0 };
            let score = match z.abs() {
                a if a > 3.0 => ZONE_WEIGHTS[3],
                a if a > 2.0 => ZONE_WEIGHTS[2],
                a if a > 1.0 => ZONE_WEIGHTS[1],
                _ => ZONE_WEIGHTS[0],
            };
            let this_side = if z > 0.0 {
                1
            } else if z < 0.0 {
                -1
            } else {
                0
            };
            total = if this_side != 0 && this_side == side { total + score } else { score };
            side = this_side;
            let signal = total >= ZONE_SIGNAL;
            out[i] = ZoneScore { score, cumulative: total, signal };
            if signal {
                (side, total) = (0, 0);
            }
        }
    }
    out
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests;
