//! Estimation plots: every observation of each group, and the difference of
//! each group's mean from a reference group's with its confidence interval,
//! so the size and precision of an effect are read from the graph rather
//! than from a p-value (Gardner & Altman, "Confidence intervals rather than
//! P values", *BMJ* 292, 1986; Cumming, *Understanding the New Statistics*,
//! 2012; Ho, Tumkaya, Aryal, Choi & Claridge-Chang, "Moving beyond P values:
//! data analysis with estimation graphics", *Nature Methods* 16, 2019).
//!
//! * **Two groups (Gardner–Altman):** one panel. The groups' points sit in
//!   the first two slots; the third slot holds the mean difference (other
//!   minus reference) with its interval on a floating axis whose zero is
//!   level with the reference group's mean, so the difference reads off the
//!   same units as the data. Dashed lines carry both means across to it.
//! * **More groups (Cumming):** the groups' points in an upper panel, and in
//!   a lower panel each group's mean difference from the reference with its
//!   interval, at the group's own position (the reference shows the zero
//!   line).
//!
//! Intervals for `d = ȳ_g − ȳ_ref`:
//!
//! * **BCa bootstrap** (default): `B` resamples (5000 by default), each
//!   group resampled with replacement within itself (the stratified
//!   bootstrap of independent samples), `d*` computed on each; the
//!   bias-corrected and accelerated interval (Efron 1987) with the
//!   acceleration from the jackknife deleting one observation at a time
//!   within each group (`stat_inference::bootstrap`, Efron & Tibshirani
//!   eq. 15.36) — the interval of Ho et al. and of `scipy.stats.bootstrap`
//!   with `method="BCa"`. Resamples are seeded (`seed + g` for group `g`), so
//!   the interval is the same every time. The bootstrap distribution of `d*`
//!   is drawn as a half violin (Gaussian kernel density, Silverman's
//!   bandwidth).
//! * **Welch t:** `d ± t_{1−α/2, ν} · √(s²_g/n_g + s²_ref/n_ref)` with the
//!   Welch–Satterthwaite `ν` (`stat_inference::means::two_sample_t`).
//!
//! The analyses use rows that are not excluded, inside the graph's filter,
//! with a group and a value; a frequency column repeats a row (rounded to a
//! whole number); weights are not used. The vertical axis is always linear.

use std::collections::BTreeMap;

use stat_core::Result;
use stat_data::{DataTable, RowStateManager};
use stat_inference::Alternative;
use stat_inference::bootstrap::{acceleration, bca_interval, bootstrap_map, jackknife_map};
use stat_inference::means::two_sample_t;
use stat_linear::natural_order;

use crate::element::{Element, ElementKind};
use crate::encoding::{EncodingPlan, invalid, is_continuous};
use crate::frame::{
    AxisKind, AxisSpec, Frame, FrameOptions, GroupStyle, Panel, PanelExtra, PanelPoint, filter_note, row_styles,
    select_rows,
};
use crate::layer::{KeyGlyph, Layer, LinePattern, Mark, MarkShape, TextAnchor};
use crate::palette;
use crate::scale::AxisScale;
use crate::view::format_value;

/// How the interval of a mean difference is computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EstimationInterval {
    /// Bias-corrected and accelerated bootstrap.
    #[default]
    Bca,
    /// Welch's t interval.
    Welch,
}

impl EstimationInterval {
    /// Every method, in menu order.
    pub const ALL: [EstimationInterval; 2] = [EstimationInterval::Bca, EstimationInterval::Welch];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Bca => "BCa bootstrap",
            Self::Welch => "Welch t",
        }
    }

    /// Short name for saving.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Bca => "bca",
            Self::Welch => "welch",
        }
    }

    /// The method with this [`EstimationInterval::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }
}

/// Settings of an estimation plot.
#[derive(Debug, Clone, PartialEq)]
pub struct EstimationSettings {
    /// Interval method.
    pub interval: EstimationInterval,
    /// Bootstrap resamples.
    pub resamples: usize,
    /// Bootstrap seed.
    pub seed: u64,
    /// Two-sided confidence level.
    pub level: f64,
    /// The reference group (`None` or a level not present: the first level).
    pub reference: Option<String>,
}

impl Default for EstimationSettings {
    fn default() -> Self {
        Self { interval: EstimationInterval::Bca, resamples: 5000, seed: 1, level: 0.95, reference: None }
    }
}

/// Fewest and most bootstrap resamples accepted.
pub const RESAMPLE_RANGE: (usize, usize) = (200, 100_000);

/// One group of an estimation plot.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupEstimate {
    /// Level label.
    pub label: String,
    /// Position on the horizontal axis.
    pub position: f64,
    /// Observations used (frequencies counted).
    pub n: usize,
    /// Mean (NaN without observations).
    pub mean: f64,
    /// Standard deviation (NaN below two observations).
    pub sd: f64,
}

/// One group's mean difference from the reference.
#[derive(Debug, Clone, PartialEq)]
pub struct MeanDifference {
    /// Index into [`EstimationGeometry::groups`].
    pub group: usize,
    /// `ȳ_g − ȳ_ref`.
    pub difference: f64,
    /// Lower confidence limit.
    pub lower: f64,
    /// Upper confidence limit.
    pub upper: f64,
    /// Density of the bootstrap differences as `[difference, density]`
    /// (empty for the Welch interval).
    pub density: Vec<[f64; 2]>,
}

/// What an estimation plot's panels draw.
#[derive(Debug, Clone, PartialEq)]
pub struct EstimationGeometry {
    /// The groups, in axis order.
    pub groups: Vec<GroupEstimate>,
    /// Index of the reference group.
    pub reference: usize,
    /// Differences of the other groups (those with two or more values).
    pub differences: Vec<MeanDifference>,
    /// Two groups (Gardner–Altman): the difference is on a floating axis in
    /// the same panel.
    pub two_groups: bool,
    /// The lower panel of a Cumming plot (differences only).
    pub lower_panel: bool,
    /// Interval method.
    pub interval: EstimationInterval,
    /// Confidence level.
    pub level: f64,
    /// Bootstrap resamples used.
    pub resamples: usize,
}

impl EstimationGeometry {
    /// A sentence per difference, as written under the graph.
    #[must_use]
    pub fn sentences(&self) -> Vec<String> {
        let method = match self.interval {
            EstimationInterval::Bca => format!("BCa bootstrap, {} resamples", self.resamples),
            EstimationInterval::Welch => "Welch t".to_owned(),
        };
        let r = &self.groups[self.reference].label;
        self.differences
            .iter()
            .map(|d| {
                format!(
                    "Mean difference {} − {r} = {} ({}% CI {} to {}; {method}).",
                    self.groups[d.group].label,
                    format_value(d.difference),
                    format_value(self.level * 100.0),
                    format_value(d.lower),
                    format_value(d.upper)
                )
            })
            .collect()
    }
}

/// Mean and sample standard deviation.
fn mean_sd(v: &[f64]) -> (f64, f64) {
    let n = v.len() as f64;
    let mean = v.iter().sum::<f64>() / n;
    let sd =
        if v.len() > 1 { (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0)).sqrt() } else { f64::NAN };
    (mean, sd)
}

/// The mean difference `mean(other) − mean(reference)` with its interval.
///
/// # Errors
/// From the bootstrap or the t interval (e.g. no variation in either group).
pub fn mean_difference(
    reference: &[f64],
    other: &[f64],
    interval: EstimationInterval,
    resamples: usize,
    seed: u64,
    level: f64,
) -> Result<(f64, f64, f64, Vec<f64>)> {
    let d = mean_sd(other).0 - mean_sd(reference).0;
    match interval {
        EstimationInterval::Welch => {
            let t = two_sample_t(other, reference, 0.0, false, level, Alternative::TwoSided)?;
            Ok((d, t.confidence_interval.0, t.confidence_interval.1, Vec::new()))
        }
        EstimationInterval::Bca => {
            let (na, nb) = (reference.len(), other.len());
            if na < 2 || nb < 2 {
                return Err(invalid("a bootstrap interval needs two or more values in each group"));
            }
            let data: Vec<f64> = reference.iter().chain(other).copied().collect();
            let strata: Vec<usize> = (0..na + nb).map(|i| usize::from(i >= na)).collect();
            // Resamples keep each position in its own group, so the first
            // `na` positions are the reference's draws.
            let replicates = bootstrap_map(na + nb, Some(&strata), resamples, seed, |idx| {
                let a: f64 = idx[..na].iter().map(|&i| data[i]).sum::<f64>() / na as f64;
                let b: f64 = idx[na..].iter().map(|&i| data[i]).sum::<f64>() / nb as f64;
                b - a
            })?;
            let jackknife = jackknife_map(na + nb, |idx| {
                let (mut sa, mut ca, mut sb, mut cb) = (0.0, 0.0, 0.0, 0.0);
                for &i in idx {
                    if i < na {
                        (sa, ca) = (sa + data[i], ca + 1.0);
                    } else {
                        (sb, cb) = (sb + data[i], cb + 1.0);
                    }
                }
                sb / cb - sa / ca
            })?;
            let a = acceleration(&jackknife, Some(&strata))?.unwrap_or(0.0);
            let bca = bca_interval(d, &replicates, a, level)?
                .ok_or_else(|| invalid("the bootstrap interval is undefined (the resamples do not vary)"))?;
            Ok((d, bca.lower, bca.upper, replicates))
        }
    }
}

/// The frame of an estimation plot: the first horizontal column gives the
/// groups, the first vertical column the values.
///
/// # Errors
/// [`stat_core::StatError::InvalidArgument`] without a categorical
/// horizontal column and a numeric vertical one, with fewer than two
/// groups, or when the plan does not fit the table.
pub fn build_estimation_frame(
    table: &DataTable,
    state: &RowStateManager,
    plan: &EncodingPlan,
    options: &FrameOptions,
    settings: &EstimationSettings,
) -> Result<Frame> {
    plan.validate(table)?;
    let group_col =
        plan.x.first().filter(|c| !is_continuous(table, c)).ok_or_else(|| {
            invalid("an estimation plot needs a categorical column on the horizontal axis (the groups)")
        })?;
    let response = plan
        .y
        .first()
        .filter(|c| is_continuous(table, c))
        .ok_or_else(|| invalid("an estimation plot needs a numeric column on the vertical axis"))?;
    let gcol = table.find(group_col).and_then(|i| table.column(i));
    let y = table.find(response).and_then(|i| table.column(i)).and_then(|c| c.as_numeric()).unwrap_or(&[]);
    let rs = select_rows(table, state, plan, options);
    let label = |r: usize| gcol.map(|c| c.level_text(r).trim().to_owned()).filter(|l| !l.is_empty());
    let shown: Vec<(usize, String)> = rs
        .page_rows
        .iter()
        .filter(|&&r| y.get(r).is_some_and(|v| v.is_finite()))
        .filter_map(|&r| label(r).map(|l| (r, l)))
        .collect();
    let mut levels: Vec<String> = shown.iter().map(|(_, l)| l.clone()).collect();
    levels.sort_by(|a, b| natural_order(a, b));
    levels.dedup();
    if levels.len() < 2 {
        return Err(invalid("an estimation plot needs two or more groups"));
    }
    let reference = settings.reference.as_ref().and_then(|r| levels.iter().position(|l| l == r)).unwrap_or(0);
    let mut values: Vec<Vec<f64>> = vec![Vec::new(); levels.len()];
    let excluded = |r: usize| r < state.len() && state.is_excluded(r);
    for (r, l) in &shown {
        if !excluded(*r) && !rs.filtered[*r] && rs.weight[*r] > 0.0 {
            let k = levels.iter().position(|x| x == l).expect("a level");
            values[k].extend(std::iter::repeat_n(y[*r], rs.freq[*r].round().max(0.0) as usize));
        }
    }
    let groups: Vec<GroupEstimate> = levels
        .iter()
        .enumerate()
        .map(|(k, l)| {
            let (mean, sd) = if values[k].is_empty() { (f64::NAN, f64::NAN) } else { mean_sd(&values[k]) };
            GroupEstimate { label: l.clone(), position: (k + 1) as f64, n: values[k].len(), mean, sd }
        })
        .collect();
    let mut notes = Vec::new();
    if values[reference].len() < 2 {
        return Err(invalid(format!(
            "the reference group {} needs two or more values for an estimation plot",
            levels[reference]
        )));
    }
    let resamples = settings.resamples.clamp(RESAMPLE_RANGE.0, RESAMPLE_RANGE.1);
    let mut differences = Vec::new();
    for k in (0..levels.len()).filter(|&k| k != reference) {
        if values[k].len() < 2 {
            notes.push(format!("{} has fewer than two values; its difference is not shown.", levels[k]));
            continue;
        }
        let seed = settings.seed.wrapping_add(k as u64);
        match mean_difference(&values[reference], &values[k], settings.interval, resamples, seed, settings.level) {
            Ok((difference, lower, upper, replicates)) => {
                let density = if replicates.is_empty() {
                    Vec::new()
                } else {
                    let (lo, hi) =
                        replicates.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
                    let h = crate::density::silverman_bandwidth(&replicates, &[]);
                    crate::density::kde_1d(&replicates, &[], h, lo, hi, 64)
                };
                differences.push(MeanDifference { group: k, difference, lower, upper, density });
            }
            Err(e) => notes.push(format!("{} − {}: {e}", levels[k], levels[reference])),
        }
    }
    if plan.weight.is_some() {
        notes.push("Weights are not used by the estimation plot.".to_owned());
    }
    if options.y_scale != AxisScale::Linear {
        notes.push(
            "An estimation plot uses a linear vertical scale, so differences read in the data's units.".to_owned(),
        );
    }
    notes.extend(filter_note(state, options, &rs.passes));
    let two_groups = levels.len() == 2;
    let style = options.style.for_panel(0);
    let styles = row_styles(table, state, plan, &rs.rows, &style);
    // Points colored by group unless a color column is set.
    let points: Vec<PanelPoint> = shown
        .iter()
        .map(|(r, l)| {
            let k = levels.iter().position(|x| x == l).expect("a level");
            let (c, s) = style.level(group_col, l, k);
            let (color, shape) = if plan.color.is_some() || plan.shape.is_some() {
                (styles.color[*r], styles.shape[*r])
            } else {
                (c, s)
            };
            PanelPoint {
                row: *r,
                x: (k + 1) as f64,
                y: y[*r],
                group: 0,
                color,
                shape,
                radius: styles.radius[*r],
                excluded: excluded(*r),
                filtered: rs.filtered[*r],
                freq: rs.freq[*r],
                weight: rs.weight[*r],
                color_value: styles.color_value[*r],
            }
        })
        .collect();
    let geometry = EstimationGeometry {
        groups,
        reference,
        differences,
        two_groups,
        lower_panel: false,
        interval: settings.interval,
        level: settings.level,
        resamples,
    };
    notes.extend(geometry.sentences());
    let mut slots = levels.clone();
    if two_groups {
        slots.push(format!("{} − {}", levels[1 - reference], levels[reference]));
    }
    let x_axis = AxisSpec {
        name: group_col.clone(),
        kind: AxisKind::Categorical { levels: slots },
        scale: AxisScale::Linear,
        time: None,
    };
    let plain = |name: String| AxisSpec { name, kind: AxisKind::Continuous, scale: AxisScale::Linear, time: None };
    let group = vec![GroupStyle { label: String::new(), color: palette::DEFAULT_POINT, shape: MarkShape::Circle }];
    let upper = Panel {
        grid_row: 0,
        grid_col: 0,
        title: None,
        x: x_axis.clone(),
        y: plain(response.clone()),
        points,
        groups: group.clone(),
        slot_half_width: 0.4,
        extra: PanelExtra::Estimation(geometry.clone()),
        labels: BTreeMap::new(),
        data_extent: [None; 2],
    };
    let mut panels = vec![upper];
    if !two_groups {
        panels.push(Panel {
            grid_row: 1,
            grid_col: 0,
            title: None,
            x: x_axis,
            y: plain(format!("Mean difference from {}", levels[reference])),
            points: Vec::new(),
            groups: group,
            slot_half_width: 0.4,
            extra: PanelExtra::Estimation(EstimationGeometry { lower_panel: true, ..geometry }),
            labels: BTreeMap::new(),
            data_extent: [None; 2],
        });
    }
    let n_rows = panels.len();
    let mut legend = crate::frame::Legend::default();
    if plan.color.is_none() && plan.shape.is_none() {
        legend.color = Some(crate::frame::ColorKey::Levels {
            title: group_col.clone(),
            entries: levels
                .iter()
                .enumerate()
                .map(|(k, l)| {
                    let (c, s) = style.level(group_col, l, k);
                    (l.clone(), c, s)
                })
                .collect(),
        });
    } else {
        legend = styles.legend;
    }
    Ok(Frame {
        panels,
        n_rows,
        n_cols: 1,
        column_headers: vec![String::new()],
        row_headers: vec![String::new(); n_rows],
        legend,
        notes,
        pages: rs.pages,
        page: rs.page,
    })
}

/// Draws the means, differences, intervals and the floating axis of an
/// estimation plot (its points are the graph's points layer, best laid out
/// as a swarm).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EstimationElement;

/// Line from `a` to `b`.
fn line(a: [f64; 2], b: [f64; 2], color: crate::layer::Rgba, width: f32, pattern: LinePattern, name: &str) -> Mark {
    Mark::Line { points: vec![a, b], color, width, pattern, name: name.to_owned() }
}

/// The half violin of a bootstrap distribution: `density` (`[value, d]`)
/// laid on its side from `x0` to the right, at most `width` wide, shifted
/// up by `offset`.
fn half_violin(density: &[[f64; 2]], x0: f64, width: f64, offset: f64) -> Option<Mark> {
    let top = density.iter().map(|p| p[1]).fold(0.0_f64, f64::max);
    if density.len() < 2 || top <= 0.0 {
        return None;
    }
    let mut points: Vec<[f64; 2]> = density.iter().map(|&[v, d]| [x0 + width * d / top, v + offset]).collect();
    points.push([x0, density[density.len() - 1][0] + offset]);
    points.push([x0, density[0][0] + offset]);
    Some(Mark::Polygon {
        points,
        fill: palette::LINE.with_alpha(0.22),
        stroke: palette::LINE.with_alpha(0.5),
        name: "Bootstrap distribution of the difference".to_owned(),
    })
}

impl EstimationElement {
    /// A difference with its interval at `x`, shifted up by `offset`.
    fn difference(layer: &mut Layer, d: &MeanDifference, x: f64, offset: f64) {
        if let Some(v) = half_violin(&d.density, x + 0.06, 0.3, offset) {
            layer.marks.push(v);
        }
        layer.marks.push(line(
            [x, d.lower + offset],
            [x, d.upper + offset],
            palette::LABEL,
            2.2,
            LinePattern::Solid,
            "Confidence interval",
        ));
        layer.marks.push(Mark::Points {
            points: vec![[x, d.difference + offset]],
            color: palette::LABEL,
            shape: MarkShape::Circle,
            radius: 5.0,
            filled: true,
            name: "Mean difference".to_owned(),
        });
    }

    /// Mean ± SD beside each group's points.
    fn group_summaries(layer: &mut Layer, g: &EstimationGeometry) {
        for e in g.groups.iter().filter(|e| e.mean.is_finite()) {
            let x = e.position + 0.42;
            if e.sd.is_finite() {
                layer.marks.push(line(
                    [x, e.mean - e.sd],
                    [x, e.mean + e.sd],
                    palette::LINE,
                    1.4,
                    LinePattern::Solid,
                    "Mean ± SD",
                ));
            }
            layer.marks.push(Mark::Points {
                points: vec![[x, e.mean]],
                color: palette::LINE,
                shape: MarkShape::Square,
                radius: 3.0,
                filled: true,
                name: "Mean ± SD".to_owned(),
            });
        }
    }
}

impl Element for EstimationElement {
    fn kind(&self) -> ElementKind {
        ElementKind::Estimation
    }

    fn layer(&self, panel: &Panel, _state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        let PanelExtra::Estimation(g) = &panel.extra else { return layer };
        let level = format!("{}% CI", format_value(g.level * 100.0));
        if g.lower_panel {
            // Cumming plot: differences against the reference's zero line.
            let k = g.groups.len() as f64;
            layer.marks.push(line([0.5, 0.0], [k + 0.5, 0.0], palette::LINE, 1.0, LinePattern::Dashed, "Zero"));
            let r = &g.groups[g.reference];
            layer.marks.push(Mark::Text {
                at: [r.position, 0.0],
                text: "reference".to_owned(),
                color: palette::LABEL,
                anchor: TextAnchor::Above,
                movable: None,
            });
            for d in &g.differences {
                Self::difference(&mut layer, d, g.groups[d.group].position, 0.0);
            }
        } else {
            Self::group_summaries(&mut layer, g);
            if let (true, Some(d)) = (g.two_groups, g.differences.first()) {
                // Gardner–Altman: the floating axis, zero level with the reference mean.
                let base = g.groups[g.reference].mean;
                let axis_x = 2.7;
                let (lo, hi) = (d.lower.min(0.0).min(d.difference), d.upper.max(0.0).max(d.difference));
                let lo = d.density.first().map_or(lo, |p| lo.min(p[0]));
                let hi = d.density.last().map_or(hi, |p| hi.max(p[0]));
                let pad = 0.05 * (hi - lo).max(f64::MIN_POSITIVE);
                let ticks = crate::export::nice_tick_labels(lo - pad, hi + pad, 4);
                if let (Some(first), Some(last)) = (ticks.first(), ticks.last()) {
                    layer.marks.push(line(
                        [axis_x, base + first.0],
                        [axis_x, base + last.0],
                        palette::LINE,
                        1.0,
                        LinePattern::Solid,
                        "Difference axis",
                    ));
                }
                for (t, text) in &ticks {
                    layer.marks.push(line(
                        [axis_x - 0.04, base + t],
                        [axis_x, base + t],
                        palette::LINE,
                        1.0,
                        LinePattern::Solid,
                        "Difference axis",
                    ));
                    layer.marks.push(Mark::Text {
                        at: [axis_x - 0.06, base + t],
                        text: text.clone(),
                        color: palette::LABEL,
                        anchor: TextAnchor::RightOfText,
                        movable: None,
                    });
                }
                let other = &g.groups[d.group];
                for e in [&g.groups[g.reference], other] {
                    layer.marks.push(line(
                        [e.position + 0.42, e.mean],
                        [3.4, e.mean],
                        palette::LINE.with_alpha(0.7),
                        1.0,
                        LinePattern::Dashed,
                        "Group mean",
                    ));
                }
                Self::difference(&mut layer, d, 3.05, base);
                layer.marks.push(Mark::Text {
                    at: [axis_x, base + ticks.last().map_or(hi, |t| t.0)],
                    text: "Mean difference".to_owned(),
                    color: palette::LABEL,
                    anchor: TextAnchor::Above,
                    movable: None,
                });
            }
            layer.key_entry("Mean ± SD", palette::LINE, KeyGlyph::Marker(MarkShape::Square, true));
        }
        if !g.differences.is_empty() && (g.lower_panel || g.two_groups) {
            layer.key_entry(
                format!("Mean difference with {level}"),
                palette::LABEL,
                KeyGlyph::Marker(MarkShape::Circle, true),
            );
            if g.interval == EstimationInterval::Bca {
                layer.key_entry(
                    "Bootstrap distribution of the difference",
                    palette::LINE,
                    KeyGlyph::Area(palette::LINE.with_alpha(0.22)),
                );
            }
        }
        layer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    const CONTROL: [f64; 12] = [19.2, 22.1, 20.5, 18.7, 21.3, 23.0, 20.1, 19.8, 22.6, 21.0, 18.9, 20.7];
    const TREATED: [f64; 12] = [23.4, 25.1, 22.8, 26.3, 24.0, 21.9, 25.7, 24.4, 23.1, 27.0, 24.8, 22.5];

    /// Welch interval against R 4.3:
    /// `t.test(treated, control, var.equal = FALSE)$conf.int`.
    #[test]
    fn welch_interval_matches_r() {
        let (d, lo, hi, reps) = mean_difference(&CONTROL, &TREATED, EstimationInterval::Welch, 0, 0, 0.95).unwrap();
        assert!(reps.is_empty());
        assert!((d - WELCH.0).abs() < 1e-12);
        assert!((lo - WELCH.1).abs() < 1e-9 && (hi - WELCH.2).abs() < 1e-9, "{lo} {hi}");
    }

    /// BCa interval against SciPy 1.18 `stats.bootstrap((treated, control),
    /// mean difference, method="BCa", n_resamples=200000)`
    /// with 20 000 resamples here: a different
    /// random stream, so equal within Monte Carlo error.
    #[test]
    fn bca_interval_matches_scipy_within_monte_carlo_error() {
        let (d, lo, hi, reps) = mean_difference(&CONTROL, &TREATED, EstimationInterval::Bca, 20_000, 7, 0.95).unwrap();
        assert_eq!(reps.len(), 20_000);
        assert!((d - WELCH.0).abs() < 1e-12);
        assert!((lo - SCIPY_BCA.0).abs() < 0.03 && (hi - SCIPY_BCA.1).abs() < 0.03, "{lo} {hi} vs {SCIPY_BCA:?}");
        // Seeded: the same interval every time.
        let again = mean_difference(&CONTROL, &TREATED, EstimationInterval::Bca, 20_000, 7, 0.95).unwrap();
        assert_eq!((again.1, again.2), (lo, hi));
    }

    /// R 4.3: difference, and `t.test(treated, control)$conf.int`.
    const WELCH: (f64, f64, f64) = (3.591_666_666_666_665, 2.318_153_283_026_722_6, 4.865_180_050_306_607);
    /// SciPy 1.18 BCa limits averaged over seeds 1, 2, 3 (2.45 / 4.75,
    /// 2.45 / 4.7417, 2.4583 / 4.75).
    const SCIPY_BCA: (f64, f64) = (2.452_8, 4.747_2);

    fn table(groups: &[&str]) -> DataTable {
        let mut g = Vec::new();
        let mut y = Vec::new();
        for (k, name) in groups.iter().enumerate() {
            for (i, v) in CONTROL.iter().enumerate() {
                g.push((*name).to_owned());
                y.push(v + k as f64 * 1.5 + if i % 2 == 0 { 0.3 } else { -0.3 });
            }
        }
        DataTable::new("e", vec![Column::text("Group", g), Column::numeric("Score", y)]).unwrap()
    }

    #[test]
    fn two_groups_give_a_floating_axis_and_more_give_a_lower_panel() {
        let plan = EncodingPlan { x: vec!["Group".into()], y: vec!["Score".into()], ..Default::default() };
        let t = table(&["A", "B"]);
        let state = RowStateManager::new(t.n_rows());
        let settings = EstimationSettings { resamples: 1000, ..EstimationSettings::default() };
        let f = build_estimation_frame(&t, &state, &plan, &FrameOptions::default(), &settings).unwrap();
        assert_eq!(f.panels.len(), 1);
        assert_eq!(f.panels[0].x.levels(), ["A", "B", "B − A"]);
        let PanelExtra::Estimation(g) = &f.panels[0].extra else { panic!() };
        assert!(g.two_groups && (g.differences[0].difference - 1.5).abs() < 1e-9);
        assert!(f.notes.iter().any(|n| n.starts_with("Mean difference B − A = 1.5")), "{:?}", f.notes);
        let layer = EstimationElement.layer(&f.panels[0], &state);
        assert!(layer.all_finite());
        assert!(layer.marks.iter().any(|m| matches!(m, Mark::Text { text, .. } if text == "Mean difference")));
        // The difference point sits at the other group's mean on the main axis.
        let at = layer.marks.iter().find_map(|m| match m {
            Mark::Points { points, name, .. } if name == "Mean difference" => Some(points[0]),
            _ => None,
        });
        assert!((at.unwrap()[1] - g.groups[1].mean).abs() < 1e-9);
        // A chosen reference.
        let other = EstimationSettings { reference: Some("B".into()), ..settings.clone() };
        let f = build_estimation_frame(&t, &state, &plan, &FrameOptions::default(), &other).unwrap();
        assert_eq!(f.panels[0].x.levels(), ["A", "B", "A − B"]);
        // Four groups: a Cumming plot.
        let t = table(&["A", "B", "C", "D"]);
        let state = RowStateManager::new(t.n_rows());
        let welch = EstimationSettings { interval: EstimationInterval::Welch, ..settings };
        let f = build_estimation_frame(&t, &state, &plan, &FrameOptions::default(), &welch).unwrap();
        assert_eq!((f.panels.len(), f.n_rows), (2, 2));
        assert_eq!(f.panels[1].y.name, "Mean difference from A");
        let PanelExtra::Estimation(g) = &f.panels[1].extra else { panic!() };
        assert!(g.lower_panel && g.differences.len() == 3);
        assert!((g.differences[2].difference - 4.5).abs() < 1e-9);
        let layer = EstimationElement.layer(&f.panels[1], &state);
        assert!(layer.all_finite() && layer.marks.len() > 3);
        let bad = EncodingPlan { x: vec!["Score".into()], y: vec!["Score".into()], ..Default::default() };
        assert!(build_estimation_frame(&t, &state, &bad, &FrameOptions::default(), &welch).is_err());
    }
}
