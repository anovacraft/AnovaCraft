//! Reference lines and shaded bands added by the user.
//!
//! A reference line is a straight line right across a plot at one value of
//! an axis: horizontal at a value of the vertical axis, or vertical at a
//! value of the horizontal axis. Its value is typed ([`ReferenceValue::Fixed`])
//! or computed from the plotted data on that axis
//! ([`ReferenceValue::Statistic`]: mean, median, a quantile, mean ± k standard
//! deviations, or the centre line and 3-sigma limits of an individuals
//! control chart), and is recomputed whenever the data or the row states
//! change. A shaded band fills the stretch between two such values, behind
//! the data. Lines and bands have a key entry and a label written along
//! them, so their color is never the only cue.
//!
//! Statistics are of the data values (on a transformed axis they are
//! computed before the transformation, then drawn on the scale), of the rows
//! that take part in analyses, with frequencies and weights used as in
//! [`crate::summary`]:
//!
//! * mean `Σ fwx / Σ fw`; median and quantiles by the frequency-weighted
//!   quantile of [`crate::stats::weighted_quantile`];
//! * mean ± k·s with `s` the frequency-weighted standard deviation (divisor
//!   `Σf − 1`);
//! * individuals chart (Montgomery, *Introduction to Statistical Quality
//!   Control*, §6.4): centre `x̄`, limits `x̄ ± 3·MR̄/d₂` with `MR̄` the mean
//!   moving range of span 2 of the values in row order and `d₂ = 2/√π ≈ 1.128`,
//!   computed by `stat_quality::charts::individuals_chart` (frequencies and
//!   weights are not used: each row is one observation in time order).
//!
//! A line's statistic is of the whole graph (every panel's rows together),
//! of each panel, or of each subgroup of each panel ([`ReferenceScope`]).

use crate::layer::{KeyGlyph, Layer, LinePattern, Mark, Rgba};
use crate::palette;
use crate::scale::AxisScale;
use crate::summary::{Obs, SummaryStat, summarize};
use crate::view::format_value;

/// Which way a reference line runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ReferenceOrientation {
    /// Across the plot at a value of the vertical axis.
    #[default]
    Horizontal,
    /// Up the plot at a value of the horizontal axis.
    Vertical,
}

impl ReferenceOrientation {
    /// Both, in menu order.
    pub const ALL: [ReferenceOrientation; 2] = [ReferenceOrientation::Horizontal, ReferenceOrientation::Vertical];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Horizontal => "Horizontal (a value of y)",
            Self::Vertical => "Vertical (a value of x)",
        }
    }

    /// Short name for saving.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Horizontal => "horizontal",
            Self::Vertical => "vertical",
        }
    }

    /// The orientation with this [`ReferenceOrientation::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|o| o.key() == key)
    }

    /// The axis whose value places the line (`"x"` or `"y"`).
    #[must_use]
    pub fn axis_letter(self) -> &'static str {
        match self {
            Self::Horizontal => "y",
            Self::Vertical => "x",
        }
    }
}

/// A statistic of the plotted data that places a reference line.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ReferenceStat {
    /// The weighted mean.
    #[default]
    Mean,
    /// The median.
    Median,
    /// The `p`-th quantile, `p` in [0, 1].
    Quantile(f64),
    /// The mean plus `k` standard deviations (`k` may be negative).
    MeanSd(f64),
    /// Centre line of an individuals control chart (the mean of the values).
    IndividualsCentre,
    /// Upper limit of an individuals chart, `x̄ + 3·MR̄/d₂`.
    IndividualsUpper,
    /// Lower limit of an individuals chart, `x̄ − 3·MR̄/d₂`.
    IndividualsLower,
}

impl ReferenceStat {
    /// The statistics offered in menus (quantile and k at typical values).
    pub const CHOICES: [ReferenceStat; 7] = [
        ReferenceStat::Mean,
        ReferenceStat::Median,
        ReferenceStat::Quantile(0.9),
        ReferenceStat::MeanSd(2.0),
        ReferenceStat::IndividualsCentre,
        ReferenceStat::IndividualsUpper,
        ReferenceStat::IndividualsLower,
    ];

    /// Display name, e.g. `Mean`, `90th percentile`, `Mean + 2 SD`.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Mean => "Mean".to_owned(),
            Self::Median => "Median".to_owned(),
            Self::Quantile(p) => SummaryStat::Percentile(p).label(),
            Self::MeanSd(k) if k < 0.0 => format!("Mean − {} SD", format_value(-k)),
            Self::MeanSd(k) => format!("Mean + {} SD", format_value(k)),
            Self::IndividualsCentre => "Individuals centre line".to_owned(),
            Self::IndividualsUpper => "Individuals upper limit".to_owned(),
            Self::IndividualsLower => "Individuals lower limit".to_owned(),
        }
    }

    /// Name of the kind in menus (without its number).
    #[must_use]
    pub fn kind_label(self) -> &'static str {
        match self {
            Self::Mean => "Mean",
            Self::Median => "Median",
            Self::Quantile(_) => "Quantile",
            Self::MeanSd(_) => "Mean ± k standard deviations",
            Self::IndividualsCentre => "Individuals chart centre line",
            Self::IndividualsUpper => "Individuals chart upper limit (3σ)",
            Self::IndividualsLower => "Individuals chart lower limit (3σ)",
        }
    }

    /// Short name of the kind for saving (the number is saved apart).
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Mean => "mean",
            Self::Median => "median",
            Self::Quantile(_) => "quantile",
            Self::MeanSd(_) => "mean-sd",
            Self::IndividualsCentre => "individuals-centre",
            Self::IndividualsUpper => "individuals-upper",
            Self::IndividualsLower => "individuals-lower",
        }
    }

    /// The number the kind carries (the quantile's p, the multiple k).
    #[must_use]
    pub fn parameter(self) -> Option<f64> {
        match self {
            Self::Quantile(v) | Self::MeanSd(v) => Some(v),
            _ => None,
        }
    }

    /// The statistic with this [`ReferenceStat::key`] and parameter (a
    /// missing parameter takes the menu's value).
    #[must_use]
    pub fn from_key(key: &str, parameter: Option<f64>) -> Option<Self> {
        let base = Self::CHOICES.into_iter().find(|s| s.key() == key)?;
        Some(match (base, parameter) {
            (Self::Quantile(_), Some(p)) if (0.0..=1.0).contains(&p) => Self::Quantile(p),
            (Self::Quantile(_), Some(_)) => return None,
            (Self::MeanSd(_), Some(k)) if k.is_finite() => Self::MeanSd(k),
            (Self::MeanSd(_), Some(_)) => return None,
            (s, _) => s,
        })
    }

    /// `true` for two statistics of the same kind.
    #[must_use]
    pub fn same_kind(self, other: Self) -> bool {
        self.key() == other.key()
    }

    /// The statistic of `obs` (data values in row order, with frequencies
    /// and weights), or `None` with too few usable values (one for the
    /// mean, median and quantiles, two for the others).
    #[must_use]
    pub fn evaluate(self, obs: &[Obs]) -> Option<f64> {
        match self {
            Self::Mean => summarize(obs, SummaryStat::Mean),
            Self::Median => summarize(obs, SummaryStat::Median),
            Self::Quantile(p) => summarize(obs, SummaryStat::Percentile(p)),
            Self::MeanSd(k) => {
                let mean = summarize(obs, SummaryStat::Mean)?;
                let sd = summarize(obs, SummaryStat::StdDev)?;
                Some(mean + k * sd).filter(|v| v.is_finite())
            }
            Self::IndividualsCentre | Self::IndividualsUpper | Self::IndividualsLower => {
                let values: Vec<f64> =
                    obs.iter().filter(|o| o.0.is_finite() && o.1 > 0.0 && o.2 > 0.0).map(|o| o.0).collect();
                let chart = stat_quality::charts::individuals_chart(&values).ok()?;
                let l = chart.location.limits;
                Some(match self {
                    Self::IndividualsCentre => l.center,
                    Self::IndividualsUpper => l.upper,
                    _ => l.lower,
                })
            }
        }
    }
}

/// Where a reference line sits: a typed value or a statistic of the data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReferenceValue {
    /// A value typed by the user (in data units).
    Fixed(f64),
    /// A statistic of the plotted data on the line's axis.
    Statistic(ReferenceStat),
}

impl Default for ReferenceValue {
    fn default() -> Self {
        Self::Fixed(0.0)
    }
}

impl ReferenceValue {
    /// The value for data `obs`.
    #[must_use]
    pub fn evaluate(self, obs: &[Obs]) -> Option<f64> {
        match self {
            Self::Fixed(v) => v.is_finite().then_some(v),
            Self::Statistic(s) => s.evaluate(obs),
        }
    }

    /// `true` for a statistic (which needs data).
    #[must_use]
    pub fn is_statistic(self) -> bool {
        matches!(self, Self::Statistic(_))
    }
}

/// Which rows a statistic of a reference line or band is computed from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ReferenceScope {
    /// Every panel's rows together: one value for the whole graph.
    Whole,
    /// Each panel's own rows.
    #[default]
    Panel,
    /// Each subgroup (color or overlay level) of each panel: one line per
    /// subgroup, in its color.
    Group,
}

impl ReferenceScope {
    /// Every scope, in menu order.
    pub const ALL: [ReferenceScope; 3] = [ReferenceScope::Whole, ReferenceScope::Panel, ReferenceScope::Group];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Whole => "Whole graph",
            Self::Panel => "Each panel",
            Self::Group => "Each subgroup",
        }
    }

    /// Short name for saving.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Whole => "whole",
            Self::Panel => "panel",
            Self::Group => "group",
        }
    }

    /// The scope with this [`ReferenceScope::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == key)
    }
}

/// One reference line.
#[derive(Debug, Clone, PartialEq)]
pub struct ReferenceLine {
    /// Which way it runs.
    pub orientation: ReferenceOrientation,
    /// Where it sits.
    pub value: ReferenceValue,
    /// Its name on the plot and in the key (empty: the value or statistic,
    /// e.g. `y = 5` or `Mean`).
    pub label: String,
    /// Write ` = value` after the name (the value is always shown for an
    /// unnamed typed line).
    pub show_value: bool,
    /// Color (`None`: the reference ink, or each subgroup's color).
    pub color: Option<Rgba>,
    /// Width in screen points.
    pub width: f32,
    /// Dash pattern.
    pub pattern: LinePattern,
    /// Rows a statistic is computed from.
    pub scope: ReferenceScope,
}

impl Default for ReferenceLine {
    fn default() -> Self {
        Self {
            orientation: ReferenceOrientation::Horizontal,
            value: ReferenceValue::Fixed(0.0),
            label: String::new(),
            show_value: true,
            color: None,
            width: 1.5,
            pattern: LinePattern::Dashed,
            scope: ReferenceScope::Panel,
        }
    }
}

impl ReferenceLine {
    /// The line's name without its value: the label, or what places it.
    #[must_use]
    pub fn name(&self) -> String {
        if !self.label.trim().is_empty() {
            return self.label.trim().to_owned();
        }
        match self.value {
            ReferenceValue::Fixed(v) => format!("{} = {}", self.orientation.axis_letter(), format_value(v)),
            ReferenceValue::Statistic(s) => s.label(),
        }
    }

    /// The text written along the line at `value`.
    #[must_use]
    pub fn text(&self, value: f64) -> String {
        let unnamed_fixed = self.label.trim().is_empty() && !self.value.is_statistic();
        if self.show_value && !unnamed_fixed {
            format!("{} = {}", self.name(), format_value(value))
        } else {
            self.name()
        }
    }
}

/// One shaded band between two values of an axis.
#[derive(Debug, Clone, PartialEq)]
pub struct ReferenceBand {
    /// Horizontal: between two values of the vertical axis.
    pub orientation: ReferenceOrientation,
    /// One edge.
    pub from: ReferenceValue,
    /// The other edge.
    pub to: ReferenceValue,
    /// Its name in the key (empty: `from–to`).
    pub label: String,
    /// Fill color (`None`: the band ink, or each subgroup's color).
    pub color: Option<Rgba>,
    /// Opacity of the fill, 0–1.
    pub opacity: f32,
    /// Draw a thin line at each edge.
    pub edges: bool,
    /// Write each edge's value beside it.
    pub edge_labels: bool,
    /// Rows the statistics are computed from.
    pub scope: ReferenceScope,
}

impl Default for ReferenceBand {
    fn default() -> Self {
        Self {
            orientation: ReferenceOrientation::Horizontal,
            from: ReferenceValue::Fixed(0.0),
            to: ReferenceValue::Fixed(1.0),
            label: String::new(),
            color: None,
            opacity: 0.18,
            edges: true,
            edge_labels: true,
            scope: ReferenceScope::Panel,
        }
    }
}

impl ReferenceBand {
    /// The band's name in the key.
    #[must_use]
    pub fn name(&self) -> String {
        if !self.label.trim().is_empty() {
            return self.label.trim().to_owned();
        }
        let end = |v: ReferenceValue| match v {
            ReferenceValue::Fixed(v) => format_value(v),
            ReferenceValue::Statistic(s) => s.label(),
        };
        format!("{} from {} to {}", self.orientation.axis_letter(), end(self.from), end(self.to))
    }
}

/// The reference lines and bands of one plot.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReferenceLook {
    /// Lines, drawn over the data in this order.
    pub lines: Vec<ReferenceLine>,
    /// Bands, drawn behind the data in this order.
    pub bands: Vec<ReferenceBand>,
}

impl ReferenceLook {
    /// `true` when nothing differs from the defaults.
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// `true` when there is nothing to draw.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty() && self.bands.is_empty()
    }

    /// `true` when some value is a statistic of the data.
    #[must_use]
    pub fn uses_data(&self) -> bool {
        self.lines.iter().any(|l| l.value.is_statistic())
            || self.bands.iter().any(|b| b.from.is_statistic() || b.to.is_statistic())
    }
}

/// Plotted data values (data units, rows in order) of one set of rows.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ValueSet {
    /// The subgroup's name (empty for a panel or the whole graph).
    pub name: String,
    /// The subgroup's color (`None` for a panel or the whole graph).
    pub color: Option<Rgba>,
    /// Horizontal values with frequencies and weights.
    pub x: Vec<Obs>,
    /// Vertical values with frequencies and weights.
    pub y: Vec<Obs>,
}

impl ValueSet {
    fn axis(&self, o: ReferenceOrientation) -> &[Obs] {
        match o {
            ReferenceOrientation::Horizontal => &self.y,
            ReferenceOrientation::Vertical => &self.x,
        }
    }
}

/// The data a plot's reference statistics are computed from.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReferenceData {
    /// Every panel's rows.
    pub whole: ValueSet,
    /// This panel's rows.
    pub panel: ValueSet,
    /// This panel's subgroups (empty or one when the panel is not split).
    pub groups: Vec<ValueSet>,
}

impl ReferenceData {
    /// Data with one set of rows for the whole plot (a report plot).
    #[must_use]
    pub fn single(values: ValueSet) -> Self {
        Self { whole: values.clone(), panel: values, groups: Vec::new() }
    }

    fn sets(&self, scope: ReferenceScope) -> Vec<&ValueSet> {
        match scope {
            ReferenceScope::Whole => vec![&self.whole],
            ReferenceScope::Panel => vec![&self.panel],
            ReferenceScope::Group if self.groups.len() > 1 => self.groups.iter().collect(),
            ReferenceScope::Group => vec![&self.panel],
        }
    }
}

/// How the plot's axes are drawn: their scales and whether they are
/// categorical.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ReferenceAxes {
    /// Horizontal scale.
    pub x: AxisScale,
    /// Vertical scale.
    pub y: AxisScale,
    /// The horizontal axis shows categories.
    pub x_categorical: bool,
    /// The vertical axis shows categories.
    pub y_categorical: bool,
}

impl ReferenceAxes {
    fn scale(&self, o: ReferenceOrientation) -> AxisScale {
        match o {
            ReferenceOrientation::Horizontal => self.y,
            ReferenceOrientation::Vertical => self.x,
        }
    }

    fn categorical(&self, o: ReferenceOrientation) -> bool {
        match o {
            ReferenceOrientation::Horizontal => self.y_categorical,
            ReferenceOrientation::Vertical => self.x_categorical,
        }
    }
}

/// The reference marks of a plot.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReferenceLayers {
    /// Bands, drawn behind the data.
    pub back: Layer,
    /// Lines, drawn over the data.
    pub front: Layer,
}

/// The marks of `look` for a plot with axes `axes` showing `data`: the
/// bands (with their edge lines and labels) in [`ReferenceLayers::back`],
/// the lines in [`ReferenceLayers::front`], each with one key entry. Values
/// outside an axis scale's domain, and statistics without enough data, are
/// left out with a note.
#[must_use]
pub fn reference_layers(look: &ReferenceLook, data: &ReferenceData, axes: ReferenceAxes) -> ReferenceLayers {
    let mut out = ReferenceLayers::default();
    let place = |o: ReferenceOrientation, v: f64| axes.scale(o).forward(v);
    let mut notes: Vec<String> = Vec::new();
    let mut note = |text: String| {
        if !notes.contains(&text) {
            notes.push(text);
        }
    };
    for band in &look.bands {
        let o = band.orientation;
        if (band.from.is_statistic() || band.to.is_statistic()) && axes.categorical(o) {
            note(format!("{}: statistics need a continuous {} axis.", band.name(), o.axis_letter()));
            continue;
        }
        let sets = data.sets(band.scope);
        let grouped = sets.len() > 1;
        let mut drawn = false;
        for set in sets {
            let obs = set.axis(o);
            let (Some(a), Some(b)) = (band.from.evaluate(obs), band.to.evaluate(obs)) else {
                note(format!("{}: too few values for its statistics.", band.name()));
                continue;
            };
            let (Some(pa), Some(pb)) = (place(o, a), place(o, b)) else {
                note(format!("{}: an edge is outside the axis scale.", band.name()));
                continue;
            };
            let ink = band.color.or(set.color.filter(|_| grouped)).unwrap_or(palette::REFERENCE_BAND);
            let prefix = if grouped && !set.name.is_empty() { format!("{}: ", set.name) } else { String::new() };
            out.back.marks.push(Mark::Span {
                from: pa,
                to: pb,
                vertical: o == ReferenceOrientation::Vertical,
                color: ink.with_alpha(f64::from(band.opacity.clamp(0.0, 1.0))),
                name: format!("{prefix}{}", band.name()),
            });
            for (at, v) in [(pa, a), (pb, b)] {
                if band.edges || band.edge_labels {
                    out.back.marks.push(Mark::Rule {
                        at,
                        vertical: o == ReferenceOrientation::Vertical,
                        color: ink,
                        width: if band.edges { 1.0 } else { 0.0 },
                        pattern: LinePattern::Solid,
                        label: if band.edge_labels { format!("{prefix}{}", format_value(v)) } else { String::new() },
                        name: format!("{prefix}{} edge", band.name()),
                    });
                }
            }
            drawn = true;
        }
        if drawn {
            let ink = band.color.unwrap_or(palette::REFERENCE_BAND);
            out.back.key_entry(
                band.name(),
                ink,
                KeyGlyph::Area(ink.with_alpha(f64::from(band.opacity.clamp(0.0, 1.0)))),
            );
        }
    }
    for line in &look.lines {
        let o = line.orientation;
        if line.value.is_statistic() && axes.categorical(o) {
            note(format!("{}: statistics need a continuous {} axis.", line.name(), o.axis_letter()));
            continue;
        }
        let sets = data.sets(line.scope);
        let grouped = sets.len() > 1;
        let mut drawn = false;
        for set in sets {
            let Some(v) = line.value.evaluate(set.axis(o)) else {
                note(format!("{}: too few values for its statistic.", line.name()));
                continue;
            };
            let Some(at) = place(o, v) else {
                note(format!("{}: {} is outside the axis scale.", line.name(), format_value(v)));
                continue;
            };
            let ink = line.color.or(set.color.filter(|_| grouped)).unwrap_or(palette::REFERENCE);
            let prefix = if grouped && !set.name.is_empty() { format!("{}: ", set.name) } else { String::new() };
            out.front.marks.push(Mark::Rule {
                at,
                vertical: o == ReferenceOrientation::Vertical,
                color: ink,
                width: line.width.max(0.25),
                pattern: line.pattern,
                label: format!("{prefix}{}", line.text(v)),
                name: format!("{prefix}{}", line.name()),
            });
            drawn = true;
        }
        if drawn {
            let ink =
                if grouped && line.color.is_none() { palette::LINE } else { line.color.unwrap_or(palette::REFERENCE) };
            out.front.key_entry(line.name(), ink, KeyGlyph::Line(line.pattern));
        }
    }
    out.front.notes = notes;
    out
}

impl ReferenceLayers {
    /// `true` when there is nothing to draw.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.back.marks.is_empty() && self.front.marks.is_empty()
    }

    /// The smallest and largest value the lines and bands sit at on the
    /// horizontal axis (`[0]`) and the vertical axis (`[1]`), drawn units.
    #[must_use]
    pub fn extent(&self) -> [Option<(f64, f64)>; 2] {
        let mut out = [None, None];
        for m in self.back.marks.iter().chain(&self.front.marks) {
            let (vertical, values) = match m {
                Mark::Rule { at, vertical, .. } => (*vertical, vec![*at]),
                Mark::Span { from, to, vertical, .. } => (*vertical, vec![*from, *to]),
                _ => continue,
            };
            let slot: &mut Option<(f64, f64)> = &mut out[usize::from(!vertical)];
            for v in values.into_iter().filter(|v| v.is_finite()) {
                *slot = Some(slot.map_or((v, v), |(lo, hi)| (lo.min(v), hi.max(v))));
            }
        }
        out
    }
}

/// `range` widened to show `extent` too: a side that has to move goes past
/// the extent by 6% of the new span (the default padding); a range already
/// showing it is returned unchanged.
#[must_use]
pub fn widen_range(range: (f64, f64), extent: Option<(f64, f64)>) -> (f64, f64) {
    let Some((lo, hi)) = extent else { return range };
    if lo >= range.0 && hi <= range.1 {
        return range;
    }
    let (a, b) = (range.0.min(lo), range.1.max(hi));
    let pad = 0.06 * if b > a { b - a } else { a.abs().max(1.0) };
    (if lo < range.0 { a - pad } else { range.0 }, if hi > range.1 { b + pad } else { range.1 })
}

/// The reference data of `panel` within `frame`: the rows that take part in
/// analyses, in row order, in data units; subgroups by the panel's groups.
#[must_use]
pub fn panel_reference_data(frame: &crate::frame::Frame, panel: &crate::frame::Panel) -> ReferenceData {
    let set = |pts: &mut dyn Iterator<Item = (&crate::frame::Panel, &crate::frame::PanelPoint)>| {
        let mut rows: Vec<(usize, Obs, Obs)> = pts
            .filter(|(_, p)| p.in_analysis())
            .map(|(q, p)| {
                (p.row, (q.x.scale.inverse(p.x), p.freq, p.weight), (q.y.scale.inverse(p.y), p.freq, p.weight))
            })
            .collect();
        rows.sort_by_key(|r| r.0);
        // Categorical positions are not values.
        let x = if panel.x.is_categorical() { Vec::new() } else { rows.iter().map(|r| r.1).collect() };
        let y = if panel.y.is_categorical() { Vec::new() } else { rows.iter().map(|r| r.2).collect() };
        ValueSet { x, y, ..ValueSet::default() }
    };
    let whole = set(&mut frame.panels.iter().flat_map(|q| q.points.iter().map(move |p| (q, p))));
    let own = set(&mut panel.points.iter().map(|p| (panel, p)));
    let groups = if panel.groups.len() > 1 {
        panel
            .groups
            .iter()
            .enumerate()
            .map(|(g, style)| ValueSet {
                name: style.label.clone(),
                color: Some(style.color),
                ..set(&mut panel.points.iter().filter(|p| p.group == g).map(|p| (panel, p)))
            })
            .collect()
    } else {
        Vec::new()
    };
    ReferenceData { whole, panel: own, groups }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(v: &[f64]) -> Vec<Obs> {
        v.iter().map(|&x| (x, 1.0, 1.0)).collect()
    }

    /// Values checked against independent computations: Python 3's
    /// `statistics` module (`mean`, `median`, `stdev`), NumPy's `quantile` and the individuals chart
    /// formula by hand (MR̄ = 17/9, σ̂ = MR̄/d₂ with d₂ = 2/√π).
    #[test]
    fn statistics_match_references() {
        let v = obs(&[10.0, 12.0, 11.5, 9.0, 13.0, 12.5, 10.5, 11.0, 14.0, 12.0]);
        let close = |a: Option<f64>, b: f64| assert!((a.unwrap() - b).abs() < 1e-9 * b.abs().max(1.0), "{a:?} vs {b}");
        close(ReferenceStat::Mean.evaluate(&v), 11.55);
        close(ReferenceStat::Median.evaluate(&v), 11.75);
        // NumPy `quantile` (linear interpolation, its default method).
        close(ReferenceStat::Quantile(0.25).evaluate(&v), 10.625);
        close(ReferenceStat::Quantile(0.9).evaluate(&v), 13.1);
        let sd = 1.480_427_866_080_163_5;
        close(ReferenceStat::MeanSd(2.0).evaluate(&v), 11.55 + 2.0 * sd);
        close(ReferenceStat::MeanSd(-1.5).evaluate(&v), 11.55 - 1.5 * sd);
        // Moving ranges: 2, 0.5, 2.5, 4, 0.5, 2, 0.5, 3, 2 → 17/9.
        let sigma = 17.0 / 9.0 / (2.0 / std::f64::consts::PI.sqrt());
        close(ReferenceStat::IndividualsCentre.evaluate(&v), 11.55);
        close(ReferenceStat::IndividualsUpper.evaluate(&v), 11.55 + 3.0 * sigma);
        close(ReferenceStat::IndividualsLower.evaluate(&v), 11.55 - 3.0 * sigma);
        assert_eq!(ReferenceStat::MeanSd(1.0).evaluate(&obs(&[3.0])), None);
        assert_eq!(ReferenceStat::IndividualsUpper.evaluate(&obs(&[3.0])), None);
        for s in ReferenceStat::CHOICES {
            assert_eq!(ReferenceStat::from_key(s.key(), s.parameter()), Some(s));
        }
        assert_eq!(ReferenceStat::from_key("quantile", Some(1.5)), None);
    }

    #[test]
    fn layers_put_bands_behind_and_lines_in_front_with_key_entries() {
        let look = ReferenceLook {
            lines: vec![
                ReferenceLine { value: ReferenceValue::Fixed(5.0), ..ReferenceLine::default() },
                ReferenceLine {
                    value: ReferenceValue::Statistic(ReferenceStat::Mean),
                    label: "Average".into(),
                    orientation: ReferenceOrientation::Vertical,
                    ..ReferenceLine::default()
                },
            ],
            bands: vec![ReferenceBand { label: "Spec".into(), ..ReferenceBand::default() }],
        };
        let data = ReferenceData::single(ValueSet {
            x: obs(&[1.0, 2.0, 6.0]),
            y: obs(&[0.0, 0.0, 0.0]),
            ..ValueSet::default()
        });
        let r = reference_layers(&look, &data, ReferenceAxes::default());
        let rules: Vec<(f64, bool, &str)> = r
            .front
            .marks
            .iter()
            .filter_map(|m| match m {
                Mark::Rule { at, vertical, label, .. } => Some((*at, *vertical, label.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(rules, [(5.0, false, "y = 5"), (3.0, true, "Average = 3")]);
        assert!(matches!(r.back.marks[0], Mark::Span { from, to, .. } if from == 0.0 && to == 1.0));
        assert_eq!(r.back.marks.len(), 3, "the span and its two edges");
        let keys: Vec<&str> = r.front.key.iter().chain(&r.back.key).map(|k| k.label.as_str()).collect();
        assert_eq!(keys, ["y = 5", "Average", "Spec"]);
        assert!(r.front.notes.is_empty());
        assert_eq!(r.extent(), [Some((3.0, 3.0)), Some((0.0, 5.0))]);
        assert_eq!(widen_range((0.0, 10.0), Some((2.0, 8.0))), (0.0, 10.0));
        let (lo, hi) = widen_range((0.0, 10.0), Some((5.0, 20.0)));
        assert_eq!(lo, 0.0);
        assert!((hi - 21.2).abs() < 1e-12, "{hi}");
    }

    #[test]
    fn scales_groups_and_missing_data() {
        let look = ReferenceLook {
            lines: vec![
                ReferenceLine { value: ReferenceValue::Fixed(100.0), ..ReferenceLine::default() },
                ReferenceLine { value: ReferenceValue::Fixed(-1.0), ..ReferenceLine::default() },
                ReferenceLine {
                    value: ReferenceValue::Statistic(ReferenceStat::Median),
                    scope: ReferenceScope::Group,
                    ..ReferenceLine::default()
                },
            ],
            bands: Vec::new(),
        };
        let a = ValueSet {
            name: "A".into(),
            color: Some(Rgba::rgb(1, 2, 3)),
            y: obs(&[10.0, 1000.0, 100.0]),
            ..ValueSet::default()
        };
        let b = ValueSet { name: "B".into(), color: Some(Rgba::rgb(4, 5, 6)), y: obs(&[1.0]), ..ValueSet::default() };
        let data = ReferenceData { whole: ValueSet::default(), panel: ValueSet::default(), groups: vec![a, b] };
        let axes = ReferenceAxes { y: AxisScale::Log10, ..ReferenceAxes::default() };
        let r = reference_layers(&look, &data, axes);
        let rules: Vec<(f64, Rgba, &str)> = r
            .front
            .marks
            .iter()
            .filter_map(|m| match m {
                Mark::Rule { at, color, label, .. } => Some((*at, *color, label.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(rules[0].0, 2.0, "100 drawn at log₁₀ 100");
        assert_eq!(rules[1], (2.0, Rgba::rgb(1, 2, 3), "A: Median = 100"));
        assert_eq!(rules[2], (0.0, Rgba::rgb(4, 5, 6), "B: Median = 1"));
        assert!(r.front.notes.iter().any(|n| n.contains("outside the axis scale")), "{:?}", r.front.notes);
        // No data at all: the statistic is left out with a note.
        let empty = reference_layers(&look, &ReferenceData::default(), ReferenceAxes::default());
        assert!(empty.front.notes.iter().any(|n| n.contains("too few values")));
        // Statistics on a category axis are refused.
        let cat = reference_layers(&look, &data, ReferenceAxes { y_categorical: true, ..ReferenceAxes::default() });
        assert!(cat.front.notes.iter().any(|n| n.contains("continuous y axis")));
    }
}
