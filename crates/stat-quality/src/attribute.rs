//! Shewhart control charts for attribute data (Montgomery, *Introduction to
//! Statistical Quality Control*, ch. 7; NIST/SEMATECH e-Handbook §6.3.3),
//! Laney's charts for overdispersed proportions and rates, and the
//! rare-event g and t charts.
//!
//! | Chart | Plotted | Centre | Limits |
//! |---|---|---|---|
//! | p  | fraction nonconforming `dᵢ/nᵢ` | `p̄ = Σd/Σn` | `p̄ ± 3√(p̄(1 − p̄)/nᵢ)` |
//! | np | number nonconforming `dᵢ` (constant `n`) | `n p̄` | `n p̄ ± 3√(n p̄(1 − p̄))` |
//! | c  | nonconformities `cᵢ` (constant opportunity) | `c̄` | `c̄ ± 3√c̄` |
//! | u  | nonconformities per unit `cᵢ/nᵢ` | `ū = Σc/Σn` | `ū ± 3√(ū/nᵢ)` |
//! | p′ | `dᵢ/nᵢ` | `p̄` | `p̄ ± 3 σ_pᵢ σ_z` |
//! | u′ | `cᵢ/nᵢ` | `ū` | `ū ± 3 σ_uᵢ σ_z` |
//! | g  | units between events `gᵢ` (counted from 0) | `ḡ` | `ḡ ± 3√(ḡ(ḡ + 1))` |
//! | t  | time between events `tᵢ` | `x̄^3.6` | `(x̄ ± 3 M̄R/d₂(2))^3.6`, with `xᵢ = tᵢ^(1/3.6)` |
//!
//! **Laney p′ and u′** (Laney 2002, "Improved control charts for
//! attributes", *Quality Engineering* 14(4), 531–537): with large subgroups
//! the binomial or Poisson limits are often far too narrow because the true
//! rate itself varies from subgroup to subgroup. Each point is standardized,
//! `zᵢ = (pᵢ − p̄)/σ_pᵢ` with `σ_pᵢ = √(p̄(1 − p̄)/nᵢ)` (u′: `σ_uᵢ = √(ū/nᵢ)`),
//! and the spread of the `zᵢ` is measured by their moving ranges,
//! `σ_z = M̄R(z)/d₂(2)`. The limits widen by that factor; `σ_z = 1` gives the
//! ordinary p or u chart.
//!
//! **g chart** (Benneyan 2001, "Number-between g-type statistical quality
//! control charts for monitoring adverse events", *Health Care Management
//! Science* 4, 305–318): the number of units, cases or days between rare
//! events follows a geometric distribution with mean `ḡ` and variance
//! `ḡ(ḡ + 1)` (counting from 0), giving 3σ limits as above.
//!
//! **t chart** (Nelson 1994, "A control chart for parts-per-million
//! nonconforming items", *Journal of Quality Technology* 26(3), 239–240):
//! times between events are roughly exponential; `t^(1/3.6)` is close to
//! normal, so an individuals chart is built on the transformed times and its
//! center and limits are transformed back (a negative lower limit becomes 0).
//! Run rules are applied on the transformed scale, where the zones are
//! symmetric.
//!
//! Negative lower limits are set to zero (and the p and p′ upper limits are
//! capped at one). With unequal subgroup sizes the p, u, p′ and u′ limits vary
//! from subgroup to subgroup. A subgroup signals when it falls outside its
//! limits.

use crate::charts::ControlLimits;
use crate::constants::d2;
use stat_core::summation::sum_iter;
use stat_core::{Result, StatError};

/// Power of the t chart's transformation, `x = t^(1/3.6)` (Nelson 1994).
pub const T_CHART_POWER: f64 = 3.6;

/// Which attribute chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttributeChartKind {
    /// Fraction nonconforming.
    P,
    /// Number nonconforming, constant subgroup size.
    Np,
    /// Count of nonconformities, constant inspection unit.
    C,
    /// Nonconformities per unit.
    U,
    /// Laney p′: fraction nonconforming with limits widened for
    /// between-subgroup variation.
    LaneyP,
    /// Laney u′: nonconformities per unit with limits widened for
    /// between-subgroup variation.
    LaneyU,
    /// Number of units (or days) between rare events (geometric).
    G,
    /// Time between rare events (Weibull-type power transformation).
    T,
}

impl AttributeChartKind {
    /// Every kind, in menu order.
    pub const ALL: [Self; 8] = [Self::P, Self::Np, Self::C, Self::U, Self::LaneyP, Self::LaneyU, Self::G, Self::T];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::P => "p chart (fraction nonconforming)",
            Self::Np => "np chart (number nonconforming)",
            Self::C => "c chart (nonconformities)",
            Self::U => "u chart (nonconformities per unit)",
            Self::LaneyP => "Laney p′ chart (overdispersed fractions)",
            Self::LaneyU => "Laney u′ chart (overdispersed rates)",
            Self::G => "g chart (units between rare events)",
            Self::T => "t chart (time between rare events)",
        }
    }

    /// Short name ("p", "np", "c", "u", "p′", "u′", "g", "t").
    #[must_use]
    pub fn short(self) -> &'static str {
        match self {
            Self::P => "p",
            Self::Np => "np",
            Self::C => "c",
            Self::U => "u",
            Self::LaneyP => "p′",
            Self::LaneyU => "u′",
            Self::G => "g",
            Self::T => "t",
        }
    }

    /// Stable ASCII key for saved files and scripts: the short name, or
    /// `laney-p` / `laney-u`.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::LaneyP => "laney-p",
            Self::LaneyU => "laney-u",
            other => other.short(),
        }
    }

    /// The kind with this [`AttributeChartKind::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.key() == key)
    }

    /// `true` for the charts that need a subgroup size (p, np, u, p′, u′).
    #[must_use]
    pub fn needs_size(self) -> bool {
        matches!(self, Self::P | Self::Np | Self::U | Self::LaneyP | Self::LaneyU)
    }

    /// `true` for the rare-event charts (g and t), whose rows are the gaps
    /// between events.
    #[must_use]
    pub fn is_rare_event(self) -> bool {
        matches!(self, Self::G | Self::T)
    }

    /// Name of the plotted statistic, for axis titles.
    #[must_use]
    pub fn statistic_label(self) -> &'static str {
        match self {
            Self::P | Self::LaneyP => "Fraction nonconforming",
            Self::Np => "Number nonconforming",
            Self::C => "Nonconformities",
            Self::U | Self::LaneyU => "Nonconformities per unit",
            Self::G => "Units between events",
            Self::T => "Time between events",
        }
    }
}

/// One subgroup of an attribute chart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttributePoint {
    /// The plotted statistic.
    pub statistic: f64,
    /// Lower control limit for this subgroup.
    pub lower: f64,
    /// Upper control limit for this subgroup (capped at one for p and p′).
    pub upper: f64,
    /// Standard deviation of the plotted statistic for this subgroup, the
    /// unit of the run-rule zones: the limits are `center ± 3·sigma` before
    /// clipping at zero (and, for p and p′, capping at one). NaN for the t
    /// chart, whose zones are on the transformed scale.
    pub sigma: f64,
    /// Outside its limits.
    pub signal: bool,
}

/// An attribute control chart.
#[derive(Debug, Clone, PartialEq)]
pub struct AttributeChart {
    /// Kind of chart.
    pub kind: AttributeChartKind,
    /// Centre line.
    pub center: f64,
    /// One point per subgroup.
    pub points: Vec<AttributePoint>,
    /// Laney's between-subgroup factor `σ_z = M̄R(z)/d₂(2)` (p′ and u′ only).
    pub sigma_z: Option<f64>,
    /// Mean `x̄` and `σ̂ = M̄R/d₂(2)` of the transformed times `t^(1/3.6)`
    /// (t chart only).
    pub transformed: Option<(f64, f64)>,
}

impl AttributeChart {
    /// Values and limits on the scale where run rules apply: the plotted
    /// statistic for every chart but the t chart, which uses the transformed
    /// times `t^(1/3.6)` and its symmetric limits `x̄ ± 3σ̂`. The upper limit
    /// returned is `center + 3·sigma` before any capping at one, so the zones
    /// (`ControlLimits::zone_sigma`) are in units of the statistic's σ; a
    /// fraction never exceeds one, so rule 1 is unaffected.
    #[must_use]
    pub fn rule_scale(&self) -> (Vec<Option<f64>>, Vec<ControlLimits>) {
        match self.transformed {
            Some((center, sigma)) => {
                let limits = ControlLimits { center, lower: center - 3.0 * sigma, upper: center + 3.0 * sigma };
                (
                    self.points.iter().map(|p| Some(p.statistic.powf(1.0 / T_CHART_POWER))).collect(),
                    vec![limits; self.points.len()],
                )
            }
            None => (
                self.points.iter().map(|p| Some(p.statistic)).collect(),
                self.points
                    .iter()
                    .map(|p| ControlLimits { center: self.center, lower: p.lower, upper: self.center + 3.0 * p.sigma })
                    .collect(),
            ),
        }
    }
}

/// Build an attribute chart from `counts` (nonconforming units for p/np/p′,
/// nonconformities for c/u/u′, units between events for g, times between
/// events for t) and, for p, np, u, p′ and u′, the subgroup `sizes` (units
/// inspected for p, np and p′; inspection units, which may be fractional, for
/// u and u′).
///
/// # Errors
/// [`StatError::InvalidArgument`] for no subgroups, counts that are not
/// non-negative whole numbers (t chart times need only be non-negative),
/// sizes that are not positive (or, for p, np and p′, not whole), sizes missing or of the wrong length, more nonconforming
/// units than inspected (p, np, p′), unequal sizes on an np chart, no
/// nonconformities at all (a zero centre line), fewer than two subgroups for
/// p′, u′ and t, or no variation between subgroups (p′, u′, t).
pub fn attribute_chart(kind: AttributeChartKind, counts: &[f64], sizes: Option<&[f64]>) -> Result<AttributeChart> {
    if counts.is_empty() {
        return Err(invalid("no subgroups"));
    }
    let whole = |v: f64| v.is_finite() && v >= 0.0 && v.fract() == 0.0;
    if kind == AttributeChartKind::T {
        if !counts.iter().all(|&t| t.is_finite() && t >= 0.0) {
            return Err(invalid("times between events must be finite and non-negative"));
        }
        return t_chart(counts);
    }
    if !counts.iter().all(|&c| whole(c)) {
        return Err(invalid("counts must be whole, non-negative numbers"));
    }
    if kind == AttributeChartKind::G {
        return g_chart(counts);
    }
    let sizes: Vec<f64> = match (kind.needs_size(), sizes) {
        (false, _) => vec![1.0; counts.len()],
        (true, None) => return Err(invalid(format!("a {} chart needs subgroup sizes", kind.short()))),
        (true, Some(s)) => {
            if s.len() != counts.len() {
                return Err(invalid("one subgroup size per count is needed"));
            }
            // p-type sizes count units; u-type sizes are inspection units
            // (an area of opportunity), which may be fractional.
            let binomial = matches!(kind, AttributeChartKind::P | AttributeChartKind::Np | AttributeChartKind::LaneyP);
            if binomial && !s.iter().all(|&n| whole(n) && n > 0.0) {
                return Err(invalid("units inspected must be positive whole numbers"));
            }
            if !s.iter().all(|&n| n.is_finite() && n > 0.0) {
                return Err(invalid("inspection units must be positive numbers"));
            }
            s.to_vec()
        }
    };
    let binomial = matches!(kind, AttributeChartKind::P | AttributeChartKind::Np | AttributeChartKind::LaneyP);
    if binomial && counts.iter().zip(&sizes).any(|(c, n)| c > n) {
        return Err(invalid("a subgroup has more nonconforming units than units inspected"));
    }
    if kind == AttributeChartKind::Np && sizes.iter().any(|&n| n != sizes[0]) {
        return Err(invalid("an np chart needs equal subgroup sizes (use a p chart otherwise)"));
    }
    let total: f64 = counts.iter().sum();
    let units: f64 = sizes.iter().sum();
    if total == 0.0 {
        return Err(invalid("there are no nonconformities, so the center line is zero and no limits exist"));
    }
    let rate = total / units;
    if binomial && rate >= 1.0 {
        return Err(invalid("every unit is nonconforming, so no limits exist"));
    }
    let center = match kind {
        AttributeChartKind::Np => rate * sizes[0],
        _ => rate,
    };
    // The plotted statistic and its binomial or Poisson standard deviation.
    let base: Vec<(f64, f64)> = counts
        .iter()
        .zip(&sizes)
        .map(|(&c, &n)| match kind {
            AttributeChartKind::P | AttributeChartKind::LaneyP => (c / n, (rate * (1.0 - rate) / n).sqrt()),
            AttributeChartKind::Np => (c, (n * rate * (1.0 - rate)).sqrt()),
            AttributeChartKind::C => (c, rate.sqrt()),
            _ => (c / n, (rate / n).sqrt()),
        })
        .collect();
    let sigma_z = if matches!(kind, AttributeChartKind::LaneyP | AttributeChartKind::LaneyU) {
        if counts.len() < 2 {
            return Err(invalid(format!("a {} chart needs at least two subgroups", kind.short())));
        }
        let z: Vec<f64> = base.iter().map(|(x, s)| (x - center) / s).collect();
        let mr = sum_iter(z.windows(2).map(|w| (w[1] - w[0]).abs())) / (z.len() - 1) as f64;
        let s = mr / d2(2)?;
        if s.is_nan() || s <= 0.0 {
            return Err(invalid("the standardized subgroups do not vary, so Laney's σ_z cannot be estimated"));
        }
        Some(s)
    } else {
        None
    };
    let points = base
        .into_iter()
        .map(|(statistic, sigma)| {
            let sigma = sigma * sigma_z.unwrap_or(1.0);
            let lower = (center - 3.0 * sigma).max(0.0);
            let mut upper = center + 3.0 * sigma;
            if matches!(kind, AttributeChartKind::P | AttributeChartKind::LaneyP) {
                upper = upper.min(1.0);
            }
            AttributePoint { statistic, lower, upper, sigma, signal: statistic < lower || statistic > upper }
        })
        .collect();
    Ok(AttributeChart { kind, center, points, sigma_z, transformed: None })
}

/// g chart: `ḡ ± 3√(ḡ(ḡ + 1))`, lower limit clipped at zero.
fn g_chart(counts: &[f64]) -> Result<AttributeChart> {
    let center = sum_iter(counts.iter().copied()) / counts.len() as f64;
    if center <= 0.0 {
        return Err(invalid("every count between events is zero, so no limits exist"));
    }
    let sigma = (center * (center + 1.0)).sqrt();
    let (lower, upper) = ((center - 3.0 * sigma).max(0.0), center + 3.0 * sigma);
    let points = counts
        .iter()
        .map(|&g| AttributePoint { statistic: g, lower, upper, sigma, signal: g < lower || g > upper })
        .collect();
    Ok(AttributeChart { kind: AttributeChartKind::G, center, points, sigma_z: None, transformed: None })
}

/// t chart: an individuals chart of `t^(1/3.6)`, transformed back.
fn t_chart(times: &[f64]) -> Result<AttributeChart> {
    if times.len() < 2 {
        return Err(invalid("a t chart needs at least two times between events"));
    }
    let x: Vec<f64> = times.iter().map(|t| t.powf(1.0 / T_CHART_POWER)).collect();
    let mean = sum_iter(x.iter().copied()) / x.len() as f64;
    let sigma = sum_iter(x.windows(2).map(|w| (w[1] - w[0]).abs())) / (x.len() - 1) as f64 / d2(2)?;
    if sigma.is_nan() || sigma <= 0.0 {
        return Err(invalid("the times between events do not vary, so no limits exist"));
    }
    let back = |v: f64| v.max(0.0).powf(T_CHART_POWER);
    let (center, lower, upper) = (back(mean), back(mean - 3.0 * sigma), back(mean + 3.0 * sigma));
    let points = times
        .iter()
        .map(|&t| AttributePoint { statistic: t, lower, upper, sigma: f64::NAN, signal: t < lower || t > upper })
        .collect();
    Ok(AttributeChart { kind: AttributeChartKind::T, center, points, sigma_z: None, transformed: Some((mean, sigma)) })
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests;
