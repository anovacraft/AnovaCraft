//! Shewhart variables control charts (Shewhart 1931; NIST/SEMATECH e-Handbook
//! §6.3.2).
//!
//! Each chart pair has a **location** chart (subgroup means or individual values)
//! and a **dispersion** chart (ranges, standard deviations or moving ranges).
//! Limits are the conventional 3-sigma limits estimated from the data (Phase I):
//!
//! | chart | center | limits | process σ̂ |
//! |---|---|---|---|
//! | X̄ (from R) | x̿ | x̿ ± A₂R̄ | R̄/d₂ |
//! | R | R̄ | D₃R̄, D₄R̄ | |
//! | X̄ (from s) | x̿ | x̿ ± A₃s̄ | s̄/c₄ |
//! | s | s̄ | B₃s̄, B₄s̄ | |
//! | Individuals | x̄ | x̄ ± 3 M̄R/d₂(2) | M̄R/d₂(2) |
//! | Moving range | M̄R | 0, D₄(2) M̄R | |
//!
//! Means use compensated summation. Subgroup standard deviations use the `n − 1`
//! divisor.

use crate::constants::{ChartConstants, d2};
use crate::rules::{RuleViolation, RunRule, check_rules};
use stat_core::summation::{mean, sum_iter};
use stat_core::{Result, StatError};

/// What a chart plots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChartStatistic {
    /// Subgroup means.
    SubgroupMean,
    /// Subgroup ranges.
    SubgroupRange,
    /// Subgroup standard deviations.
    SubgroupStdDev,
    /// Individual observations.
    Individual,
    /// Moving ranges of two consecutive observations.
    MovingRange,
    /// Moving averages of the last `w` individual values or subgroup means.
    MovingAverage,
    /// Moving ranges of two consecutive subgroup means (three-way chart).
    MeanMovingRange,
    /// Standardized values `(x − x̄ₚ)/σ̂ₚ` of a short-run chart.
    StandardizedValue,
    /// Moving ranges of two consecutive standardized values.
    StandardizedMovingRange,
}

impl ChartStatistic {
    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::SubgroupMean => "Mean",
            Self::SubgroupRange => "Range",
            Self::SubgroupStdDev => "Standard deviation",
            Self::Individual => "Individual value",
            Self::MovingRange => "Moving range",
            Self::MovingAverage => "Moving average",
            Self::MeanMovingRange => "Moving range of means",
            Self::StandardizedValue => "Standardized value (Z)",
            Self::StandardizedMovingRange => "Moving range of Z",
        }
    }

    /// `true` for charts of spread, whose lower limit is often clipped at zero.
    #[must_use]
    pub fn is_dispersion(self) -> bool {
        matches!(
            self,
            Self::SubgroupRange
                | Self::SubgroupStdDev
                | Self::MovingRange
                | Self::MeanMovingRange
                | Self::StandardizedMovingRange
        )
    }
}

/// Center line and control limits of one chart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControlLimits {
    /// Center line.
    pub center: f64,
    /// Lower control limit (may be clipped at zero for dispersion charts).
    pub lower: f64,
    /// Upper control limit.
    pub upper: f64,
}

impl ControlLimits {
    /// Standard deviation of the plotted statistic implied by the limits,
    /// `(UCL − CL)/3`, used to define the 1σ and 2σ zones of the run rules.
    /// (The upper limit is never clipped, so this is valid for every chart.)
    #[must_use]
    pub fn zone_sigma(&self) -> f64 {
        (self.upper - self.center) / 3.0
    }
}

/// One control chart: plotted values and limits.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlChart {
    /// What is plotted.
    pub statistic: ChartStatistic,
    /// Plotted values in time order. `None` where the statistic is undefined
    /// (the first moving range).
    pub values: Vec<Option<f64>>,
    /// Center line and limits.
    pub limits: ControlLimits,
}

impl ControlChart {
    /// Apply run rules to this chart. See [`check_rules`].
    #[must_use]
    pub fn violations(&self, rules: &[RunRule]) -> Vec<RuleViolation> {
        check_rules(&self.values, &self.limits, rules)
    }

    /// Run rules conventionally applied to this kind of chart: all eight for
    /// location charts; only points beyond the limits for dispersion charts, whose
    /// statistic is skewed so the zone tests do not have their nominal false-alarm
    /// rates.
    #[must_use]
    pub fn default_rules(&self) -> &'static [RunRule] {
        if self.statistic.is_dispersion() { &[RunRule::BeyondLimits] } else { &RunRule::ALL }
    }
}

/// Which set of charts.
///
/// The first three are the classic Shewhart pairs of this module; the others
/// are built by [`crate::phased::phased_chart_with`] (see [`crate::extended`]
/// for their formulas).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChartKind {
    /// Subgroup means with ranges.
    MeanAndRange,
    /// Subgroup means with standard deviations.
    MeanAndStdDev,
    /// Individual values with moving ranges.
    IndividualsAndMovingRange,
    /// Moving averages of span `w` of individual values or subgroup means,
    /// with the moving range or range chart.
    MovingAverage,
    /// Three-way chart: individuals chart of subgroup means, moving range of
    /// the means, and within-subgroup ranges (Wheeler 1995).
    ThreeWayRange,
    /// Three-way chart with within-subgroup standard deviations.
    ThreeWayStdDev,
    /// Zone chart: points scored by zone, the scores accumulated on each side
    /// of the center line (Jaehn 1987).
    Zone,
    /// Short-run standardized (Z) and moving-range charts for several part
    /// numbers, with a mean and σ estimated per part (Griffith 1996).
    ShortRun,
}

/// Whether a chart kind plots individual values, subgroups, or either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubgroupUse {
    /// One value per point.
    Individuals,
    /// Subgroups of at least two values.
    Subgroups,
    /// Either individual values or subgroups of 2 to 25.
    Either,
}

impl ChartKind {
    /// Every kind, in menu order.
    pub const ALL: [Self; 8] = [
        Self::IndividualsAndMovingRange,
        Self::MeanAndRange,
        Self::MeanAndStdDev,
        Self::MovingAverage,
        Self::ThreeWayRange,
        Self::ThreeWayStdDev,
        Self::Zone,
        Self::ShortRun,
    ];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::MeanAndRange => "X̄ and R",
            Self::MeanAndStdDev => "X̄ and S",
            Self::IndividualsAndMovingRange => "Individuals and Moving Range",
            Self::MovingAverage => "Moving Average",
            Self::ThreeWayRange => "Three-Way (between/within, R)",
            Self::ThreeWayStdDev => "Three-Way (between/within, S)",
            Self::Zone => "Zone",
            Self::ShortRun => "Short-Run Z and Moving Range",
        }
    }

    /// Whether the kind plots individual values, subgroups or either.
    #[must_use]
    pub fn subgroup_use(self) -> SubgroupUse {
        match self {
            Self::IndividualsAndMovingRange | Self::ShortRun => SubgroupUse::Individuals,
            Self::MeanAndRange | Self::MeanAndStdDev | Self::ThreeWayRange | Self::ThreeWayStdDev => {
                SubgroupUse::Subgroups
            }
            Self::MovingAverage | Self::Zone => SubgroupUse::Either,
        }
    }

    /// `true` for the three-way charts, which add a within-subgroup chart.
    #[must_use]
    pub fn is_three_way(self) -> bool {
        matches!(self, Self::ThreeWayRange | Self::ThreeWayStdDev)
    }

    /// `true` when Nelson's pattern rules (2–8) are meaningful on the
    /// location chart. They are not on a moving-average chart, whose points
    /// are correlated, nor on a zone chart, whose zone scores take their
    /// place; rule 1 (a point beyond a limit) applies to every chart.
    #[must_use]
    pub fn pattern_rules_apply(self) -> bool {
        !matches!(self, Self::MovingAverage | Self::Zone)
    }
}

/// A location chart and its companion dispersion chart.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartPair {
    /// Which pair.
    pub kind: ChartKind,
    /// Means or individual values.
    pub location: ControlChart,
    /// Ranges, standard deviations or moving ranges.
    pub dispersion: ControlChart,
    /// Estimated process standard deviation (within-subgroup).
    pub sigma: f64,
    /// Observations per subgroup (1 for individuals).
    pub subgroup_size: usize,
}

/// Split `values` into consecutive subgroups of `size`. Returns the subgroups and
/// the number of trailing values that did not fill a subgroup.
///
/// # Errors
/// [`StatError::InvalidArgument`] if `size == 0`.
pub fn consecutive_subgroups(values: &[f64], size: usize) -> Result<(Vec<Vec<f64>>, usize)> {
    if size == 0 {
        return Err(invalid("subgroup size must be at least 1"));
    }
    let groups = values.chunks_exact(size).map(<[f64]>::to_vec).collect();
    Ok((groups, values.len() % size))
}

/// X̄ and R charts.
///
/// # Errors
/// [`StatError::InvalidArgument`] for fewer than two subgroups, unequal subgroup
/// sizes, or a size outside 2..=25; [`StatError::NonFiniteInput`] for NaN/∞.
pub fn mean_and_range_chart(subgroups: &[Vec<f64>]) -> Result<ChartPair> {
    let n = check_subgroups(subgroups)?;
    let k = ChartConstants::for_subgroup_size(n)?;
    let means = subgroup_means(subgroups);
    let ranges: Vec<f64> = subgroups
        .iter()
        .map(|g| g.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b)) - g.iter().fold(f64::INFINITY, |a, &b| a.min(b)))
        .collect();
    let grand = average(&means);
    let r_bar = average(&ranges);
    Ok(ChartPair {
        kind: ChartKind::MeanAndRange,
        location: chart(ChartStatistic::SubgroupMean, &means, grand, k.mean_from_range * r_bar),
        dispersion: ControlChart {
            statistic: ChartStatistic::SubgroupRange,
            values: ranges.into_iter().map(Some).collect(),
            limits: ControlLimits { center: r_bar, lower: k.range_lower * r_bar, upper: k.range_upper * r_bar },
        },
        sigma: r_bar / k.d2,
        subgroup_size: n,
    })
}

/// X̄ and S charts.
///
/// # Errors
/// As for [`mean_and_range_chart`].
pub fn mean_and_sd_chart(subgroups: &[Vec<f64>]) -> Result<ChartPair> {
    let n = check_subgroups(subgroups)?;
    let k = ChartConstants::for_subgroup_size(n)?;
    let means = subgroup_means(subgroups);
    let sds: Vec<f64> = subgroups
        .iter()
        .zip(&means)
        .map(|(g, &m)| (sum_iter(g.iter().map(|&x| (x - m) * (x - m))) / (n as f64 - 1.0)).sqrt())
        .collect();
    let grand = average(&means);
    let s_bar = average(&sds);
    Ok(ChartPair {
        kind: ChartKind::MeanAndStdDev,
        location: chart(ChartStatistic::SubgroupMean, &means, grand, k.mean_from_sd * s_bar),
        dispersion: ControlChart {
            statistic: ChartStatistic::SubgroupStdDev,
            values: sds.into_iter().map(Some).collect(),
            limits: ControlLimits { center: s_bar, lower: k.sd_lower * s_bar, upper: k.sd_upper * s_bar },
        },
        sigma: s_bar / k.c4,
        subgroup_size: n,
    })
}

/// Individuals and moving range (span 2) charts.
///
/// # Errors
/// [`StatError::InvalidArgument`] for fewer than two values;
/// [`StatError::NonFiniteInput`] for NaN/∞.
pub fn individuals_chart(values: &[f64]) -> Result<ChartPair> {
    if values.len() < 2 {
        return Err(invalid("an individuals chart needs at least two observations"));
    }
    if !values.iter().all(|v| v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "control chart data" });
    }
    let k = ChartConstants::for_subgroup_size(2)?;
    let moving: Vec<f64> = values.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
    let mr_bar = average(&moving);
    let center = average(values);
    let sigma = mr_bar / d2(2)?;
    Ok(ChartPair {
        kind: ChartKind::IndividualsAndMovingRange,
        location: chart(ChartStatistic::Individual, values, center, 3.0 * sigma),
        dispersion: ControlChart {
            statistic: ChartStatistic::MovingRange,
            values: std::iter::once(None).chain(moving.into_iter().map(Some)).collect(),
            limits: ControlLimits { center: mr_bar, lower: 0.0, upper: k.range_upper * mr_bar },
        },
        sigma,
        subgroup_size: 1,
    })
}

fn chart(statistic: ChartStatistic, values: &[f64], center: f64, half_width: f64) -> ControlChart {
    ControlChart {
        statistic,
        values: values.iter().copied().map(Some).collect(),
        limits: ControlLimits { center, lower: center - half_width, upper: center + half_width },
    }
}

fn check_subgroups(subgroups: &[Vec<f64>]) -> Result<usize> {
    if subgroups.len() < 2 {
        return Err(invalid("a subgroup chart needs at least two subgroups"));
    }
    let n = subgroups[0].len();
    if let Some(g) = subgroups.iter().find(|g| g.len() != n) {
        return Err(invalid(format!("all subgroups must have the same size (found {n} and {})", g.len())));
    }
    if subgroups.iter().flatten().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "control chart data" });
    }
    Ok(n)
}

fn subgroup_means(subgroups: &[Vec<f64>]) -> Vec<f64> {
    subgroups.iter().map(|g| average(g)).collect()
}

fn average(v: &[f64]) -> f64 {
    mean(v).unwrap_or(f64::NAN)
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    /// NIST/SEMATECH e-Handbook §6.3.2.2, flow-rate example (batches 1–10).
    /// The handbook prints x̄ = 50.81, M̄R = 1.8778, UCL = 55.8041 and
    /// LCL = 45.8159, computed with d₂ rounded to 1.128.
    #[test]
    fn individuals_chart_matches_nist_example() {
        let x = [49.6, 47.6, 49.9, 51.3, 47.8, 51.2, 52.6, 52.4, 53.6, 52.1];
        let c = individuals_chart(&x).unwrap();
        let mr_printed = [2.0, 2.3, 1.4, 3.5, 3.4, 1.4, 0.2, 1.2, 1.5];
        for (got, want) in c.dispersion.values[1..].iter().zip(mr_printed) {
            assert!(close(got.unwrap(), want, 1e-12));
        }
        assert_eq!(c.dispersion.values[0], None, "first moving range is undefined");
        assert!(close(c.location.limits.center, 50.81, 1e-12));
        assert!(close(c.dispersion.limits.center, 1.877_777_777_777_8, 1e-12));
        // With the rounded constant the handbook's figures are reproduced exactly…
        let rounded_half_width = 3.0 * c.dispersion.limits.center / 1.128;
        assert!(close(50.81 + rounded_half_width, 55.8041, 5e-5));
        assert!(close(50.81 - rounded_half_width, 45.8159, 5e-5));
        // …and with the exact d₂(2) = 2/√π the limits differ only in the third decimal.
        assert!(close(c.location.limits.upper, 55.8041, 2e-3));
        assert!(close(c.location.limits.lower, 45.8159, 2e-3));
        assert!(c.location.violations(&RunRule::ALL).is_empty(), "the handbook finds the process in control");
        // MR chart: LCL 0, UCL = D₄(2) M̄R ≈ 3.267 M̄R.
        assert_eq!(c.dispersion.limits.lower, 0.0);
        assert!(close(c.dispersion.limits.upper / c.dispersion.limits.center, 3.267, 5e-4));
    }

    /// Small example worked by hand: three subgroups of four.
    #[test]
    fn mean_and_range_by_hand() {
        let g = vec![vec![10.0, 12.0, 11.0, 13.0], vec![9.0, 11.0, 10.0, 10.0], vec![12.0, 14.0, 13.0, 13.0]];
        let c = mean_and_range_chart(&g).unwrap();
        // Means 11.5, 10.0, 13.0 → x̿ = 11.5; ranges 3, 2, 2 → R̄ = 7/3.
        let means: Vec<f64> = c.location.values.iter().map(|v| v.unwrap()).collect();
        assert_eq!(means, vec![11.5, 10.0, 13.0]);
        assert!(close(c.location.limits.center, 11.5, 1e-12));
        let r_bar = 7.0 / 3.0;
        assert!(close(c.dispersion.limits.center, r_bar, 1e-12));
        let k = ChartConstants::for_subgroup_size(4).unwrap();
        assert!(close(c.location.limits.upper, 11.5 + k.mean_from_range * r_bar, 1e-12));
        assert!(close(c.dispersion.limits.upper, k.range_upper * r_bar, 1e-12));
        assert_eq!(c.dispersion.limits.lower, 0.0, "D3 = 0 for n = 4");
        assert!(close(c.sigma, r_bar / k.d2, 1e-12));
        // A₂ ≈ 0.729 for n = 4, so UCL ≈ 11.5 + 0.729 · 2.333 = 13.20.
        assert!(close(c.location.limits.upper, 13.201, 1e-3));
    }

    #[test]
    fn mean_and_sd_by_hand() {
        let g = vec![vec![1.0, 2.0, 3.0], vec![2.0, 4.0, 6.0], vec![3.0, 3.0, 3.0]];
        let c = mean_and_sd_chart(&g).unwrap();
        // Standard deviations 1, 2, 0 → s̄ = 1.
        let sds: Vec<f64> = c.dispersion.values.iter().map(|v| v.unwrap()).collect();
        assert!(close(sds[0], 1.0, 1e-15) && close(sds[1], 2.0, 1e-15) && sds[2] == 0.0);
        let k = ChartConstants::for_subgroup_size(3).unwrap();
        assert!(close(c.dispersion.limits.center, 1.0, 1e-15));
        assert!(close(c.dispersion.limits.upper, k.sd_upper, 1e-12));
        assert!(close(c.location.limits.center, 3.0, 1e-15));
        assert!(close(c.location.limits.upper, 3.0 + k.mean_from_sd, 1e-12));
        assert!(close(c.sigma, 1.0 / k.c4, 1e-12));
    }

    #[test]
    fn zone_sigma_is_a_third_of_the_upper_distance() {
        let l = ControlLimits { center: 10.0, lower: 4.0, upper: 16.0 };
        assert_eq!(l.zone_sigma(), 2.0);
    }

    #[test]
    fn subgrouping_and_validation() {
        let (groups, left) = consecutive_subgroups(&[1.0, 2.0, 3.0, 4.0, 5.0], 2).unwrap();
        assert_eq!(groups, vec![vec![1.0, 2.0], vec![3.0, 4.0]]);
        assert_eq!(left, 1);
        assert!(consecutive_subgroups(&[1.0], 0).is_err());
        assert!(mean_and_range_chart(&[vec![1.0, 2.0]]).is_err(), "one subgroup");
        assert!(mean_and_range_chart(&[vec![1.0, 2.0], vec![1.0]]).is_err(), "unequal sizes");
        assert!(mean_and_range_chart(&[vec![1.0], vec![2.0]]).is_err(), "size 1");
        assert!(mean_and_sd_chart(&[vec![1.0, f64::NAN], vec![1.0, 2.0]]).is_err());
        assert!(individuals_chart(&[1.0]).is_err());
    }

    #[test]
    fn default_rule_sets() {
        let c = individuals_chart(&[1.0, 2.0, 3.0]).unwrap();
        assert_eq!(c.location.default_rules().len(), 8);
        assert_eq!(c.dispersion.default_rules(), &[RunRule::BeyondLimits]);
    }
}
