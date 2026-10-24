//! Control charts with phases and subgroups of varying size.
//!
//! The charts of [`crate::charts`] need equal subgroups and one set of
//! limits. Here each subgroup may have its own size and belongs to a
//! **phase** (a block of time such as "before" and "after" a process change);
//! every phase gets its own center line, process σ and limits, computed from
//! that phase's points only, and run rules restart at each phase boundary.
//!
//! Within a phase, with subgroup sizes `nᵢ`, means `x̄ᵢ`, ranges `Rᵢ` and
//! standard deviations `sᵢ` (Montgomery, *Introduction to Statistical Quality
//! Control*, §6.3.1 on variable sample sizes; NIST/SEMATECH e-Handbook §6.3.2):
//!
//! | chart | σ̂ | center | limits |
//! |---|---|---|---|
//! | X̄ | (from R or S) | x̿ = Σnᵢx̄ᵢ / Σnᵢ | x̿ ± 3σ̂/√nᵢ |
//! | R | mean of Rᵢ/d₂(nᵢ) | d₂(nᵢ)σ̂ | max(0, d₂ − 3d₃)σ̂, (d₂ + 3d₃)σ̂ |
//! | S | mean of sᵢ/c₄(nᵢ) | c₄(nᵢ)σ̂ | max(0, c₄ − 3√(1 − c₄²))σ̂, (c₄ + 3√(1 − c₄²))σ̂ |
//! | Individuals | M̄R/d₂(2) | x̄ | x̄ ± 3σ̂ |
//! | Moving range | | M̄R | 0, D₄(2)M̄R |
//!
//! With equal sizes these are exactly the usual `A₂R̄`, `D₃R̄`, `D₄R̄`, `A₃s̄`,
//! `B₃s̄`, `B₄s̄` limits. Moving ranges are taken within a phase (the first
//! point of each phase has none). The moving-average, three-way, zone and
//! short-run charts build on these; their formulas are in [`crate::extended`].
//!
//! **Options** ([`ChartSettings`], individuals, X̄–R and X̄–S charts only;
//! Montgomery §6.2 and §6.4, Wheeler 1995):
//!
//! - limits at `k`σ instead of 3σ: every `3` above becomes `k` (the zones of
//!   the run rules stay at 1σ and 2σ);
//! - a moving range over `w` consecutive values, `MRᵢ = max − min` of
//!   `xᵢ₋w₊₁ … xᵢ`, with `σ̂ = M̄R/d₂(w)` and moving-range limits
//!   `M̄R · max(0, 1 − k d₃(w)/d₂(w))` and `M̄R · (1 + k d₃(w)/d₂(w))` (the first
//!   `w − 1` points of a phase have none);
//! - **standard (historical) values** `μ₀`, `σ₀` in place of the estimates:
//!   center `μ₀`, location limits `μ₀ ± kσ₀/√nᵢ`, and dispersion center and
//!   limits `d₂σ₀`, `(d₂ ± k d₃)σ₀` (R), `c₄σ₀`, `(c₄ ± k√(1 − c₄²))σ₀` (S)
//!   or `d₂(w)σ₀`, `(d₂(w) ± k d₃(w))σ₀` (moving range), lower limits clipped
//!   at zero.
//!
//! **Run rules** on limits that vary from point to point are applied to the
//! standardized values `zᵢ = (yᵢ − CLᵢ)/σ_zᵢ` with `σ_zᵢ = (UCLᵢ − CLᵢ)/k`,
//! which have constant zones; rule 1 uses each point's actual limits, so a
//! lower limit clipped at zero is honoured. With constant limits this gives
//! exactly the results of [`crate::rules::check_rules`].

use crate::charts::{ChartKind, ChartStatistic, ControlLimits, SubgroupUse};
use crate::constants::{c4, d2, d3};
use crate::extended::{PartSummary, Points, ZoneScore, moving_average_points, short_run_points, zone_scores};
use crate::rules::{RuleViolation, RunRule, check_rules};
use stat_core::summation::sum_iter;
use stat_core::{Result, StatError};

/// One chart whose limits may change from point to point.
#[derive(Debug, Clone, PartialEq)]
pub struct VaryingChart {
    /// What is plotted.
    pub statistic: ChartStatistic,
    /// Plotted values in time order (`None` where undefined, e.g. the first
    /// moving range of each phase).
    pub values: Vec<Option<f64>>,
    /// Center line and limits at each point.
    pub limits: Vec<ControlLimits>,
    /// Limits are this many σ from the center (3 by default); the run-rule
    /// zones are fractions of it.
    pub sigma_multiple: f64,
}

/// One phase of a [`PhasedChart`].
#[derive(Debug, Clone, PartialEq)]
pub struct PhaseSummary {
    /// Index of the phase's first point.
    pub start: usize,
    /// One past the index of its last point.
    pub end: usize,
    /// Estimated process standard deviation (within subgroups; 1 for the
    /// standardized values of a short-run chart).
    pub sigma: f64,
    /// Center of the location chart.
    pub center: f64,
    /// Smallest and largest subgroup size.
    pub sizes: (usize, usize),
    /// Standard deviation of the subgroup means estimated from their moving
    /// ranges, `M̄R/d₂(2)` (three-way charts only).
    pub between_sigma: Option<f64>,
}

/// Location and dispersion charts over one or more phases.
#[derive(Debug, Clone, PartialEq)]
pub struct PhasedChart {
    /// Which charts.
    pub kind: ChartKind,
    /// Means, individual values, moving averages or standardized values.
    pub location: VaryingChart,
    /// Ranges, standard deviations or moving ranges.
    pub dispersion: VaryingChart,
    /// The within-subgroup range or standard deviation chart of a three-way
    /// chart (whose `dispersion` is the moving range of the means).
    pub within: Option<VaryingChart>,
    /// The phases, in time order.
    pub phases: Vec<PhaseSummary>,
    /// Each part number's estimates, by phase (short-run charts only).
    pub parts: Vec<PartSummary>,
    /// Zone score of every location point (zone charts only).
    pub zones: Vec<ZoneScore>,
}

/// Settings of the charts that need more than the data.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartSettings {
    /// Span `w` of a moving-average chart (at least 2).
    pub span: usize,
    /// Part-number index of each subgroup, for a short-run chart.
    pub parts: Option<Vec<usize>>,
    /// Limits at this many σ (3 by default; individuals, X̄–R and X̄–S
    /// charts only).
    pub sigma_multiple: f64,
    /// Values in each moving range of an individuals chart (2 by default,
    /// 2 to 25).
    pub range_span: usize,
    /// Known process center and σ to set the limits from (individuals, X̄–R
    /// and X̄–S charts only).
    pub standard: Option<StandardValues>,
}

impl Default for ChartSettings {
    fn default() -> Self {
        Self { span: 5, parts: None, sigma_multiple: 3.0, range_span: 2, standard: None }
    }
}

impl ChartSettings {
    /// `true` when only the defaults of the multiple, range span and
    /// standard values are used.
    fn is_plain(&self) -> bool {
        self.sigma_multiple == 3.0 && self.range_span == 2 && self.standard.is_none()
    }
}

/// Known (standard, historical) process center and standard deviation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StandardValues {
    /// Process mean `μ₀`.
    pub center: f64,
    /// Process standard deviation `σ₀` (within subgroups).
    pub sigma: f64,
}

impl VaryingChart {
    /// Run-rule violations, with patterns restarting at each phase in
    /// `phases` (see the module documentation). Ordered by point, then rule.
    #[must_use]
    pub fn violations(&self, rules: &[RunRule], phases: &[PhaseSummary]) -> Vec<RuleViolation> {
        let spans: Vec<(usize, usize)> = phases.iter().map(|p| (p.start, p.end)).collect();
        varying_violations_at(&self.values, &self.limits, rules, &spans, self.sigma_multiple)
    }
}

/// Run-rule violations of `values` against per-point `limits`, with patterns
/// restarting at each `(start, end)` span (see the module documentation).
/// Ordered by point, then rule.
#[must_use]
pub fn varying_violations(
    values: &[Option<f64>],
    limits: &[ControlLimits],
    rules: &[RunRule],
    spans: &[(usize, usize)],
) -> Vec<RuleViolation> {
    varying_violations_at(values, limits, rules, spans, 3.0)
}

/// [`varying_violations`] for limits at `multiple` σ: the zones are
/// `(UCL − CL)/multiple` wide.
#[must_use]
pub fn varying_violations_at(
    values: &[Option<f64>],
    limits: &[ControlLimits],
    rules: &[RunRule],
    spans: &[(usize, usize)],
    multiple: f64,
) -> Vec<RuleViolation> {
    let mut out = Vec::new();
    let zone_rules: Vec<RunRule> = rules.iter().copied().filter(|r| *r != RunRule::BeyondLimits).collect();
    let unit = ControlLimits { center: 0.0, lower: -3.0, upper: 3.0 };
    for &(start, end) in spans {
        if rules.contains(&RunRule::BeyondLimits) {
            for i in start..end {
                if let Some(v) = values[i] {
                    let l = &limits[i];
                    if v > l.upper || v < l.lower {
                        out.push(RuleViolation { rule: RunRule::BeyondLimits, index: i, window_start: i });
                    }
                }
            }
        }
        if zone_rules.is_empty() {
            continue;
        }
        let z: Vec<Option<f64>> = (start..end)
            .map(|i| {
                let l = &limits[i];
                let s = (l.upper - l.center) / multiple;
                values[i].map(|v| if s > 0.0 { (v - l.center) / s } else { 0.0 })
            })
            .collect();
        for mut v in check_rules(&z, &unit, &zone_rules) {
            v.index += start;
            v.window_start += start;
            out.push(v);
        }
    }
    out.sort_by_key(|v| (v.index, v.rule));
    out
}

/// Location and dispersion charts of `subgroups` (in time order), with
/// `phases[i]` the phase number of subgroup `i`: consecutive subgroups with the
/// same number form one phase. Pass all zeros for a single phase.
///
/// For [`ChartKind::IndividualsAndMovingRange`] every subgroup must hold one
/// value; for the X̄ charts each needs at least two (at most 25 for ranges).
/// This is [`phased_chart_with`] with default settings.
///
/// # Errors
/// As for [`phased_chart_with`].
pub fn phased_chart(kind: ChartKind, subgroups: &[Vec<f64>], phases: &[usize]) -> Result<PhasedChart> {
    phased_chart_with(kind, subgroups, phases, &ChartSettings::default())
}

/// Charts of any [`ChartKind`] over phases (see [`phased_chart`] and
/// [`crate::extended`]).
///
/// Subgroup sizes by kind: one value each for individuals and short-run
/// charts; 2 to 25 for range-based charts and at least 2 for S-based ones;
/// either all single values or all 2 to 25 for moving-average and zone
/// charts. A short-run chart needs `settings.parts` (one part index per
/// subgroup); a moving-average chart a span of at least 2.
///
/// # Errors
/// [`StatError::InvalidArgument`] for `phases` (or `parts`) of the wrong
/// length, a phase with fewer than two points, subgroups of the wrong size, a
/// span below 2, a phase with no spread, or a part with too few values; a
/// multiple outside (0, 6], a moving-range span outside 2–25, a standard σ
/// that is not positive, or any of these on a chart other than individuals,
/// X̄–R and X̄–S;
/// [`StatError::NonFiniteInput`] for NaN or infinite values.
pub fn phased_chart_with(
    kind: ChartKind,
    subgroups: &[Vec<f64>],
    phases: &[usize],
    settings: &ChartSettings,
) -> Result<PhasedChart> {
    if phases.len() != subgroups.len() {
        return Err(invalid("give one phase per subgroup"));
    }
    if subgroups.iter().flatten().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "control chart data" });
    }
    check_sizes(kind, subgroups)?;
    let individual = subgroups.iter().all(|g| g.len() == 1);
    let parts = match (kind, &settings.parts) {
        (ChartKind::ShortRun, Some(p)) if p.len() == subgroups.len() => p.as_slice(),
        (ChartKind::ShortRun, Some(_)) => return Err(invalid("give one part number per value")),
        (ChartKind::ShortRun, None) => return Err(invalid("a short-run chart needs a part number for every value")),
        _ => &[],
    };
    if kind == ChartKind::MovingAverage && settings.span < 2 {
        return Err(invalid("the moving-average span must be at least 2"));
    }
    if !settings.is_plain() {
        if !matches!(kind, ChartKind::IndividualsAndMovingRange | ChartKind::MeanAndRange | ChartKind::MeanAndStdDev) {
            return Err(invalid(
                "limits at another σ multiple, longer moving ranges and standard values apply to individuals, \
                 X̄–R and X̄–S charts only",
            ));
        }
        if !(settings.sigma_multiple > 0.0 && settings.sigma_multiple <= 6.0) {
            return Err(invalid("the σ multiple of the limits must be above 0 and at most 6"));
        }
        if !(2..=25).contains(&settings.range_span) {
            return Err(invalid("a moving range spans 2 to 25 values"));
        }
        if let Some(sv) = settings.standard {
            if !sv.center.is_finite() || !(sv.sigma.is_finite() && sv.sigma > 0.0) {
                return Err(invalid("the standard σ must be positive and the center finite"));
            }
        }
    }
    let base_dispersion = |range: bool| {
        if individual {
            ChartStatistic::MovingRange
        } else if range {
            ChartStatistic::SubgroupRange
        } else {
            ChartStatistic::SubgroupStdDev
        }
    };
    let (location_stat, dispersion_stat, within_stat) = match kind {
        ChartKind::IndividualsAndMovingRange => (ChartStatistic::Individual, ChartStatistic::MovingRange, None),
        ChartKind::MeanAndRange => (ChartStatistic::SubgroupMean, ChartStatistic::SubgroupRange, None),
        ChartKind::MeanAndStdDev => (ChartStatistic::SubgroupMean, ChartStatistic::SubgroupStdDev, None),
        ChartKind::MovingAverage => (ChartStatistic::MovingAverage, base_dispersion(true), None),
        ChartKind::Zone => (
            if individual { ChartStatistic::Individual } else { ChartStatistic::SubgroupMean },
            base_dispersion(true),
            None,
        ),
        ChartKind::ThreeWayRange => {
            (ChartStatistic::SubgroupMean, ChartStatistic::MeanMovingRange, Some(ChartStatistic::SubgroupRange))
        }
        ChartKind::ThreeWayStdDev => {
            (ChartStatistic::SubgroupMean, ChartStatistic::MeanMovingRange, Some(ChartStatistic::SubgroupStdDev))
        }
        ChartKind::ShortRun => (ChartStatistic::StandardizedValue, ChartStatistic::StandardizedMovingRange, None),
    };
    let empty = |statistic| VaryingChart {
        statistic,
        values: Vec::new(),
        limits: Vec::new(),
        sigma_multiple: settings.sigma_multiple,
    };
    let mut out = PhasedChart {
        kind,
        location: empty(location_stat),
        dispersion: empty(dispersion_stat),
        within: within_stat.map(empty),
        phases: Vec::new(),
        parts: Vec::new(),
        zones: Vec::new(),
    };
    let mut start = 0;
    while start < subgroups.len() {
        let end = (start..subgroups.len()).find(|&i| phases[i] != phases[start]).unwrap_or(subgroups.len());
        let groups = &subgroups[start..end];
        let index = out.phases.len();
        if groups.len() < 2 {
            return Err(invalid(format!(
                "phase {} has only one point; a phase needs at least two to set limits",
                index + 1
            )));
        }
        let parts = if parts.is_empty() { &[][..] } else { &parts[start..end] };
        let (sigma, center, between_sigma) = phase(kind, groups, parts, index, settings, &mut out)?;
        let sizes = groups.iter().fold((usize::MAX, 0), |(a, b), g| (a.min(g.len()), b.max(g.len())));
        out.phases.push(PhaseSummary { start, end, sigma, center, sizes, between_sigma });
        start = end;
    }
    if kind == ChartKind::Zone {
        out.zones = zone_scores(&out.location, &out.phases);
    }
    Ok(out)
}

fn check_sizes(kind: ChartKind, subgroups: &[Vec<f64>]) -> Result<()> {
    let range_based =
        matches!(kind, ChartKind::MeanAndRange | ChartKind::ThreeWayRange | ChartKind::MovingAverage | ChartKind::Zone);
    let all_single = subgroups.iter().all(|g| g.len() == 1);
    for (i, g) in subgroups.iter().enumerate() {
        let n = g.len();
        let ok = match kind.subgroup_use() {
            SubgroupUse::Individuals => n == 1,
            SubgroupUse::Either if all_single => true,
            _ if range_based => (2..=25).contains(&n),
            _ => n >= 2,
        };
        if !ok {
            let what = kind.label();
            return Err(invalid(match kind.subgroup_use() {
                SubgroupUse::Individuals => format!(
                    "a {what} chart needs one value per point, but subgroup {} has {n}; choose an X̄ chart",
                    i + 1
                ),
                SubgroupUse::Either => format!(
                    "a {what} chart needs single values or subgroups of 2 to 25 throughout, but subgroup {} has {n}",
                    i + 1
                ),
                SubgroupUse::Subgroups if range_based => {
                    format!("{what} charts need subgroups of 2 to 25 values, but subgroup {} has {n}", i + 1)
                }
                SubgroupUse::Subgroups => {
                    format!("{what} charts need subgroups of at least 2 values, but subgroup {} has {n}", i + 1)
                }
            }));
        }
    }
    Ok(())
}

/// Append one phase's points and limits; returns its σ̂, center and (for
/// three-way charts) the σ of the subgroup means.
fn phase(
    kind: ChartKind,
    groups: &[Vec<f64>],
    parts: &[usize],
    index: usize,
    settings: &ChartSettings,
    out: &mut PhasedChart,
) -> Result<(f64, f64, Option<f64>)> {
    let span = settings.span;
    let individual = groups.iter().all(|g| g.len() == 1);
    let range = !matches!(kind, ChartKind::MeanAndStdDev | ChartKind::ThreeWayStdDev);
    match kind {
        ChartKind::IndividualsAndMovingRange | ChartKind::MeanAndRange | ChartKind::MeanAndStdDev | ChartKind::Zone => {
            let (sigma, center, loc, disp) = if individual {
                individuals_points_with(groups, settings)?
            } else {
                subgroup_points_with(groups, range, settings)?
            };
            extend(&mut out.location, loc);
            extend(&mut out.dispersion, disp);
            Ok((sigma, center, None))
        }
        ChartKind::MovingAverage => {
            let (sigma, center, loc, disp) =
                if individual { individuals_points(groups)? } else { subgroup_points(groups, range)? };
            let means: Vec<f64> = loc.iter().map(|p| p.0.unwrap_or(f64::NAN)).collect();
            let sizes: Vec<usize> = groups.iter().map(Vec::len).collect();
            extend(&mut out.location, moving_average_points(&means, &sizes, sigma, center, span));
            extend(&mut out.dispersion, disp);
            Ok((sigma, center, None))
        }
        ChartKind::ThreeWayRange | ChartKind::ThreeWayStdDev => {
            let (sigma, _, means, within) = subgroup_points(groups, range)?;
            let means: Vec<Vec<f64>> = means.iter().map(|p| vec![p.0.unwrap_or(f64::NAN)]).collect();
            let (between, center, loc, disp) = individuals_points(&means).map_err(|_| {
                invalid("the subgroup means of a phase do not vary, so the three-way limits cannot be set")
            })?;
            extend(&mut out.location, loc);
            extend(&mut out.dispersion, disp);
            if let Some(w) = out.within.as_mut() {
                extend(w, within);
            }
            Ok((sigma, center, Some(between)))
        }
        ChartKind::ShortRun => {
            let x: Vec<f64> = groups.iter().map(|g| g[0]).collect();
            let (loc, disp, summaries) = short_run_points(&x, parts, index)?;
            extend(&mut out.location, loc);
            extend(&mut out.dispersion, disp);
            out.parts.extend(summaries);
            Ok((1.0, 0.0, None))
        }
    }
}

fn extend(chart: &mut VaryingChart, points: Points) {
    for (v, l) in points {
        chart.values.push(v);
        chart.limits.push(l);
    }
}

fn no_spread() -> StatError {
    invalid("a phase has no variation within subgroups, so limits cannot be set")
}

/// Individuals and moving-range points of single-value subgroups under
/// `settings` (σ multiple, moving-range span, standard values); returns σ̂
/// (or σ₀), the center and both charts' points.
fn individuals_points_with(groups: &[Vec<f64>], settings: &ChartSettings) -> Result<(f64, f64, Points, Points)> {
    if settings.is_plain() {
        return individuals_points(groups);
    }
    let (k, w) = (settings.sigma_multiple, settings.range_span);
    let x: Vec<f64> = groups.iter().map(|g| g[0]).collect();
    if x.len() < w {
        return Err(invalid(format!("a moving range of {w} values needs at least {w} points in each phase")));
    }
    let moving: Vec<f64> = x
        .windows(w)
        .map(|v| v.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b)) - v.iter().fold(f64::INFINITY, |a, &b| a.min(b)))
        .collect();
    let (d2w, d3w) = (d2(w)?, d3(w)?);
    let mr_bar = sum_iter(moving.iter().copied()) / moving.len() as f64;
    let (sigma, center, mr_limits) = match settings.standard {
        Some(sv) => (
            sv.sigma,
            sv.center,
            ControlLimits {
                center: d2w * sv.sigma,
                lower: ((d2w - k * d3w) * sv.sigma).max(0.0),
                upper: (d2w + k * d3w) * sv.sigma,
            },
        ),
        None => {
            let sigma = mr_bar / d2w;
            if sigma.is_nan() || sigma <= 0.0 {
                return Err(no_spread());
            }
            let center = sum_iter(x.iter().copied()) / x.len() as f64;
            let ratio = k * d3w / d2w;
            (
                sigma,
                center,
                ControlLimits {
                    center: mr_bar,
                    lower: (mr_bar * (1.0 - ratio)).max(0.0),
                    upper: mr_bar * (1.0 + ratio),
                },
            )
        }
    };
    let location_limits = ControlLimits { center, lower: center - k * sigma, upper: center + k * sigma };
    let location = x.iter().map(|&v| (Some(v), location_limits)).collect();
    let dispersion =
        (0..x.len()).map(|i| (if i + 1 < w { None } else { Some(moving[i + 1 - w]) }, mr_limits)).collect();
    Ok((sigma, center, location, dispersion))
}

/// X̄ and R (or S) points under `settings` (σ multiple, standard values).
fn subgroup_points_with(
    groups: &[Vec<f64>],
    range: bool,
    settings: &ChartSettings,
) -> Result<(f64, f64, Points, Points)> {
    if settings.is_plain() {
        return subgroup_points(groups, range);
    }
    let k = settings.sigma_multiple;
    let (estimated, grand) = match subgroup_points(groups, range) {
        Ok((sigma, grand, _, _)) => (sigma, grand),
        // Standard values need no spread in the data.
        Err(_) if settings.standard.is_some() => (f64::NAN, f64::NAN),
        Err(e) => return Err(e),
    };
    let (sigma, center) = settings.standard.map_or((estimated, grand), |sv| (sv.sigma, sv.center));
    let mut loc = Vec::with_capacity(groups.len());
    let mut disp = Vec::with_capacity(groups.len());
    for g in groups {
        let n = g.len();
        let m = sum_iter(g.iter().copied()) / n as f64;
        let s = if range {
            g.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b)) - g.iter().fold(f64::INFINITY, |a, &b| a.min(b))
        } else {
            (sum_iter(g.iter().map(|&x| (x - m) * (x - m))) / (n as f64 - 1.0)).sqrt()
        };
        let (bias, sd) = if range {
            (d2(n)?, d3(n)?)
        } else {
            let c = c4(n)?;
            (c, (1.0 - c * c).sqrt())
        };
        let half = k * sigma / (n as f64).sqrt();
        loc.push((Some(m), ControlLimits { center, lower: center - half, upper: center + half }));
        disp.push((
            Some(s),
            ControlLimits {
                center: bias * sigma,
                lower: ((bias - k * sd) * sigma).max(0.0),
                upper: (bias + k * sd) * sigma,
            },
        ));
    }
    Ok((sigma, center, loc, disp))
}

/// Individuals and moving-range points of single-value subgroups; returns
/// σ̂ = M̄R/d₂(2), the center and both charts' points.
fn individuals_points(groups: &[Vec<f64>]) -> Result<(f64, f64, Points, Points)> {
    let x: Vec<f64> = groups.iter().map(|g| g[0]).collect();
    let moving: Vec<f64> = x.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
    let mr_bar = sum_iter(moving.iter().copied()) / moving.len() as f64;
    let center = sum_iter(x.iter().copied()) / x.len() as f64;
    let sigma = mr_bar / d2(2)?;
    if sigma.is_nan() || sigma <= 0.0 {
        return Err(no_spread());
    }
    let d4 = 1.0 + 3.0 * d3(2)? / d2(2)?;
    let location_limits = ControlLimits { center, lower: center - 3.0 * sigma, upper: center + 3.0 * sigma };
    let mr_limits = ControlLimits { center: mr_bar, lower: 0.0, upper: d4 * mr_bar };
    let location = x.iter().map(|&v| (Some(v), location_limits)).collect();
    let dispersion = (0..x.len()).map(|i| (if i == 0 { None } else { Some(moving[i - 1]) }, mr_limits)).collect();
    Ok((sigma, center, location, dispersion))
}

/// X̄ and R (or S) points of subgroups of two or more; returns σ̂, x̿ and both
/// charts' points.
fn subgroup_points(groups: &[Vec<f64>], range: bool) -> Result<(f64, f64, Points, Points)> {
    let means: Vec<f64> = groups.iter().map(|g| sum_iter(g.iter().copied()) / g.len() as f64).collect();
    let total: usize = groups.iter().map(Vec::len).sum();
    let grand = sum_iter(groups.iter().zip(&means).map(|(g, m)| g.len() as f64 * m)) / total as f64;
    let spread: Vec<f64> = if range {
        groups
            .iter()
            .map(|g| {
                g.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b)) - g.iter().fold(f64::INFINITY, |a, &b| a.min(b))
            })
            .collect()
    } else {
        groups
            .iter()
            .zip(&means)
            .map(|(g, &m)| (sum_iter(g.iter().map(|&x| (x - m) * (x - m))) / (g.len() as f64 - 1.0)).sqrt())
            .collect()
    };
    // Each subgroup's unbiasing constant and the standard deviation of its statistic, per unit σ.
    let factors = groups
        .iter()
        .map(|g| -> Result<(f64, f64)> {
            let n = g.len();
            if range {
                Ok((d2(n)?, d3(n)?))
            } else {
                let c = c4(n)?;
                Ok((c, (1.0 - c * c).sqrt()))
            }
        })
        .collect::<Result<Vec<_>>>()?;
    let sigma = sum_iter(spread.iter().zip(&factors).map(|(s, f)| s / f.0)) / groups.len() as f64;
    if sigma.is_nan() || sigma <= 0.0 {
        return Err(no_spread());
    }
    let mut location = Vec::with_capacity(groups.len());
    let mut dispersion = Vec::with_capacity(groups.len());
    for ((g, (&m, &s)), &(bias, sd)) in groups.iter().zip(means.iter().zip(&spread)).zip(&factors) {
        let half = 3.0 * sigma / (g.len() as f64).sqrt();
        location.push((Some(m), ControlLimits { center: grand, lower: grand - half, upper: grand + half }));
        dispersion.push((
            Some(s),
            ControlLimits {
                center: bias * sigma,
                lower: ((bias - 3.0 * sd) * sigma).max(0.0),
                upper: (bias + 3.0 * sd) * sigma,
            },
        ));
    }
    Ok((sigma, grand, location, dispersion))
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests;
