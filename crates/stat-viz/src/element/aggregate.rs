//! Aggregation layers: bar charts, line charts and heat maps.

use std::collections::BTreeMap;

use stat_data::RowStateManager;

use super::{Element, ElementKind, data_obs, group_color, group_prefix, included_by_slot, percent, position_key};
use crate::element::summaries::error_bar_marks;
use crate::frame::{Panel, PanelPoint};
use crate::layer::{KeyGlyph, Layer, LinePattern, Mark, MarkShape, Ramp, Region, Rgba, TextAnchor};
use crate::palette;
use crate::summary::{ErrorBarLook, ErrorBars, Obs, SummaryStat, error_interval, summarize};
use crate::view::format_value;

/// How the bars of several subgroups share a position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum BarLayout {
    /// Next to each other.
    #[default]
    SideBySide,
    /// Stacked on each other.
    Stacked,
    /// Stacked and scaled so each stack is 100%.
    Percent,
    /// Side by side, each bar drawn as a stem from zero with a dot at its
    /// value (a lollipop chart).
    Lollipop,
}

impl BarLayout {
    /// Every layout, in menu order.
    pub const ALL: [BarLayout; 4] =
        [BarLayout::SideBySide, BarLayout::Stacked, BarLayout::Percent, BarLayout::Lollipop];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::SideBySide => "Side by side",
            Self::Stacked => "Stacked",
            Self::Percent => "Stacked to 100%",
            Self::Lollipop => "Lollipops (stem and dot)",
        }
    }

    /// Short name for saving.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::SideBySide => "side_by_side",
            Self::Stacked => "stacked",
            Self::Percent => "percent",
            Self::Lollipop => "lollipop",
        }
    }

    /// The layout with this [`BarLayout::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.key() == key)
    }
}

/// Rows in `pts` as a sorted list.
fn rows_of(pts: &[&PanelPoint]) -> Vec<usize> {
    let mut r: Vec<usize> = pts.iter().map(|p| p.row).collect();
    r.sort_unstable();
    r.dedup();
    r
}

/// Share (by frequency) of `pts` that is selected.
fn selected_share(pts: &[&PanelPoint], state: &RowStateManager) -> f64 {
    let total: f64 = pts.iter().map(|p| p.freq).sum();
    if total <= 0.0 {
        return 0.0;
    }
    let sel: f64 = pts.iter().filter(|p| p.row < state.len() && state.is_selected(p.row)).map(|p| p.freq).sum();
    sel / total
}

/// Rectangle outline `x0..x1 × y0..y1`.
fn rect(x0: f64, x1: f64, y0: f64, y1: f64) -> Vec<[f64; 2]> {
    vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}

/// Half the smallest gap between distinct x (bar half-width on a numeric axis).
fn numeric_half_width(xs: &[f64]) -> f64 {
    let mut v: Vec<f64> = xs.to_vec();
    v.sort_by(f64::total_cmp);
    v.dedup();
    let gap = v.windows(2).map(|w| w[1] - w[0]).fold(f64::INFINITY, f64::min);
    if gap.is_finite() && gap > 0.0 {
        0.4 * gap
    } else {
        let span = v.last().copied().unwrap_or(0.0).abs().max(1.0);
        0.02 * span
    }
}

/// Bars of a statistic of the vertical variable (or of the count of rows,
/// when there is no vertical column) at each horizontal position, per
/// subgroup: side by side, stacked, or stacked to 100%. Optional error bars
/// around means (side by side only) and value labels. The share of each bar
/// that is selected is filled in the selection color, and clicking a bar
/// selects its rows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarElement {
    /// The statistic (`Count` when there is no vertical column).
    pub stat: SummaryStat,
    /// How subgroups share a position.
    pub layout: BarLayout,
    /// Error bars (means, side by side only).
    pub error_bars: ErrorBars,
    /// Confidence level for confidence-interval bars.
    pub level: f64,
    /// Write each bar's value.
    pub labels: bool,
    /// How the error bars are drawn.
    pub error_look: ErrorBarLook,
}

impl Default for BarElement {
    fn default() -> Self {
        Self {
            stat: SummaryStat::Mean,
            layout: BarLayout::SideBySide,
            error_bars: ErrorBars::None,
            level: 0.95,
            labels: false,
            error_look: ErrorBarLook::default(),
        }
    }
}

impl Element for BarElement {
    fn kind(&self) -> ElementKind {
        ElementKind::Bars
    }

    fn layer(&self, panel: &Panel, state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        if panel.y.is_categorical() {
            layer.notes.push("Bars need numbers up (or no vertical column, for counts).".to_owned());
            return layer;
        }
        let counts_only = panel.y.name.is_empty();
        let stat = if counts_only { SummaryStat::Count } else { self.stat };
        let sy = panel.y.scale;
        if sy.forward(0.0).is_none() {
            layer.notes.push("Bars start at zero; use a linear or square-root vertical scale.".to_owned());
            return layer;
        }
        let stacked = matches!(self.layout, BarLayout::Stacked | BarLayout::Percent) && panel.groups.len() > 1;
        let lollipop = self.layout == BarLayout::Lollipop;
        let bars = if stat == SummaryStat::Mean && !stacked { self.error_bars } else { ErrorBars::None };
        if self.error_bars != ErrorBars::None && bars == ErrorBars::None {
            layer.notes.push("Error bars are drawn on side-by-side bars of means only.".to_owned());
        }
        let slots = included_by_slot(panel);
        // Position of each slot's stack: the category centre (undoing the
        // subgroup dodge) when stacking on a category axis.
        let centre = |x: f64| if stacked && panel.x.is_categorical() { x.round() } else { x };
        let half = if panel.x.is_categorical() {
            if stacked { 0.35 } else { panel.slot_half_width * 0.8 }
        } else {
            numeric_half_width(&slots.iter().map(|s| s.0.0).collect::<Vec<_>>())
        };
        // Values in data units, per (position, subgroup).
        let mut values: Vec<(f64, usize, f64, Vec<&PanelPoint>)> = Vec::new();
        for ((x, g), pts) in slots {
            let obs: Vec<Obs> = data_obs(panel, &pts);
            if let Some(v) = summarize(&obs, stat) {
                values.push((centre(x), g, v, pts));
            }
        }
        // Stacks: running totals per position (positives up, negatives down).
        let mut totals: BTreeMap<i64, f64> = BTreeMap::new();
        if self.layout == BarLayout::Percent && stacked {
            for (x, _, v, _) in &values {
                *totals.entry(position_key(*x, 1e9)).or_default() += v.abs();
            }
        }
        let mut up: BTreeMap<i64, f64> = BTreeMap::new();
        let mut down: BTreeMap<i64, f64> = BTreeMap::new();
        let mut skipped = 0;
        for (x, g, v, pts) in &values {
            let key = position_key(*x, 1e9);
            let v = match totals.get(&key) {
                Some(&t) if t > 0.0 => 100.0 * v / t,
                _ => *v,
            };
            let (lo_data, hi_data) = if stacked {
                let acc = if v >= 0.0 { up.entry(key).or_default() } else { down.entry(key).or_default() };
                let start = *acc;
                *acc += v;
                (start, *acc)
            } else {
                (0.0, v)
            };
            let (Some(lo), Some(hi)) = (sy.forward(lo_data), sy.forward(hi_data)) else {
                skipped += 1;
                continue;
            };
            let color = group_color(panel, *g, palette::BAR);
            let name = format!("{}bar", group_prefix(panel, *g));
            let outline = rect(x - half, x + half, lo.min(hi), lo.max(hi));
            let share = selected_share(pts, state);
            if lollipop {
                // A stem from zero and a dot in the subgroup's marker shape;
                // a dot holding selected rows is ringed.
                let shape = panel.groups.get(*g).map_or(MarkShape::Circle, |s| s.shape);
                let dot = if panel.groups.len() > 1 { color } else { palette::BAR };
                layer.marks.push(Mark::Line {
                    points: vec![[*x, lo], [*x, hi]],
                    color: dot,
                    width: 2.0,
                    pattern: LinePattern::Solid,
                    name: name.clone(),
                });
                layer.marks.push(Mark::Points {
                    points: vec![[*x, hi]],
                    color: dot,
                    shape,
                    radius: 6.0,
                    filled: true,
                    name: name.clone(),
                });
                if share > 0.0 {
                    layer.marks.push(Mark::Points {
                        points: vec![[*x, hi]],
                        color: palette::SELECTED,
                        shape: MarkShape::Circle,
                        radius: 9.5,
                        filled: false,
                        name: "Selected rows".to_owned(),
                    });
                }
                layer.regions.push(Region {
                    outline: rect(x - 0.4 * half, x + 0.4 * half, lo.min(hi), lo.max(hi)),
                    rows: rows_of(pts),
                });
            } else {
                layer.marks.push(Mark::Polygon {
                    points: outline.clone(),
                    fill: color.with_alpha(0.55),
                    stroke: color,
                    name: name.clone(),
                });
            }
            if share > 0.0 && !lollipop {
                let top = lo + share * (hi - lo);
                layer.marks.push(Mark::Polygon {
                    points: rect(x - half, x + half, lo.min(top), lo.max(top)),
                    // Dark wash with the selection outline: never mistaken for a group color.
                    fill: Rgba::rgb(20, 20, 30).with_alpha(0.4),
                    stroke: palette::SELECTED,
                    name: "Selected rows".to_owned(),
                });
            }
            if !lollipop {
                layer.regions.push(Region { outline, rows: rows_of(pts) });
            }
            if self.labels {
                let text = if self.layout == BarLayout::Percent && stacked {
                    format!("{}%", format_value(v))
                } else {
                    format_value(v)
                };
                layer.marks.push(Mark::Text {
                    at: [*x, if stacked { 0.5 * (lo + hi) } else { hi }],
                    text,
                    color: palette::BAR_TEXT,
                    anchor: if stacked { TextAnchor::Centre } else { TextAnchor::Above },
                    movable: None,
                });
            }
            if let Some((elo, ehi)) = error_interval(&data_obs(panel, pts), bars, self.level) {
                let mid = sy.forward(0.5 * (elo + ehi));
                if let (Some(a), Some(b), Some(m)) = (sy.forward(elo), sy.forward(ehi), mid) {
                    layer.marks.extend(error_bar_marks(
                        *x,
                        [m, a, b],
                        half * 0.4,
                        palette::BAR_TEXT,
                        &self.error_look,
                        &format!("{}error bar", group_prefix(panel, *g)),
                    ));
                }
            }
        }
        if skipped > 0 {
            layer.notes.push(format!("{skipped} bar(s) fall outside the vertical scale's domain."));
        }
        if !layer.marks.is_empty() {
            let of = if counts_only { String::new() } else { format!(" of {}", panel.y.name) };
            let what = match (self.layout, stacked) {
                (BarLayout::Percent, true) => format!("Bars: {}{of}, % of each stack", stat.label()),
                (BarLayout::Lollipop, _) => format!("Lollipops: {}{of}", stat.label()),
                _ => format!("Bars: {}{of}", stat.label()),
            };
            if lollipop {
                layer.key_entry(what, palette::BAR, KeyGlyph::Marker(MarkShape::Circle, true));
            } else {
                layer.key_entry(what, palette::BAR, KeyGlyph::Area(palette::BAR.with_alpha(0.55)));
            }
            if bars != ErrorBars::None {
                let what = match bars {
                    ErrorBars::Confidence => format!("{} confidence interval for the mean", percent(self.level)),
                    other => format!("Mean {}", other.label()),
                };
                layer.key_entry(what, palette::BAR_TEXT, KeyGlyph::Line(LinePattern::Solid));
            }
            if lollipop && layer.marks.iter().any(|m| matches!(m, Mark::Points { name, .. } if name == "Selected rows"))
            {
                layer.key_entry("Holds selected rows", palette::SELECTED, KeyGlyph::Marker(MarkShape::Circle, false));
            }
            if layer.marks.iter().any(|m| matches!(m, Mark::Polygon { name, .. } if name == "Selected rows")) {
                layer.key_entry(
                    "Share of rows selected",
                    palette::SELECTED,
                    KeyGlyph::Area(palette::SELECTED.with_alpha(0.65)),
                );
            }
        }
        layer
    }
}

// ---------------------------------------------------------------------------

/// What a line chart connects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum LineConnect {
    /// Rows in table order (a run chart).
    #[default]
    RowOrder,
    /// Rows in order of the horizontal value.
    XOrder,
    /// A statistic at each horizontal position.
    Summary,
}

impl LineConnect {
    /// Every choice, in menu order.
    pub const ALL: [LineConnect; 3] = [LineConnect::RowOrder, LineConnect::XOrder, LineConnect::Summary];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::RowOrder => "Rows in table order",
            Self::XOrder => "Rows in horizontal order",
            Self::Summary => "A statistic at each position",
        }
    }

    /// Short name for saving.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::RowOrder => "rows",
            Self::XOrder => "x",
            Self::Summary => "summary",
        }
    }

    /// The choice with this [`LineConnect::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.key() == key)
    }
}

/// Lines per subgroup joining rows in table order (a run chart), rows in
/// order of x, or a statistic at each horizontal position (with markers).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineElement {
    /// What the line joins.
    pub connect: LineConnect,
    /// The statistic for [`LineConnect::Summary`].
    pub stat: SummaryStat,
}

impl Default for LineElement {
    fn default() -> Self {
        Self { connect: LineConnect::RowOrder, stat: SummaryStat::Mean }
    }
}

impl Element for LineElement {
    fn kind(&self) -> ElementKind {
        ElementKind::Line
    }

    fn layer(&self, panel: &Panel, _state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        if panel.y.is_categorical() {
            layer.notes.push("Line charts need numbers up.".to_owned());
            return layer;
        }
        let mut lines: BTreeMap<usize, Vec<[f64; 2]>> = BTreeMap::new();
        match self.connect {
            LineConnect::RowOrder | LineConnect::XOrder => {
                let mut pts: Vec<&PanelPoint> = panel.points.iter().filter(|p| p.in_analysis()).collect();
                if self.connect == LineConnect::XOrder {
                    pts.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.row.cmp(&b.row)));
                } else {
                    pts.sort_by_key(|p| p.row);
                }
                let mut values: BTreeMap<usize, Vec<f64>> = BTreeMap::new();
                for p in pts {
                    lines.entry(p.group).or_default().push([p.x, p.y]);
                    values.entry(p.group).or_default().push(panel.y.scale.inverse(p.y));
                }
                if self.connect == LineConnect::RowOrder {
                    for (g, v) in values {
                        if let Some(note) = run_chart_note(&v, &group_prefix(panel, g)) {
                            layer.notes.push(note);
                        }
                    }
                }
            }
            LineConnect::Summary => {
                for ((x, g), pts) in included_by_slot(panel) {
                    let obs = data_obs(panel, &pts);
                    if let Some(v) = summarize(&obs, self.stat).and_then(|v| panel.y.scale.forward(v)) {
                        lines.entry(g).or_default().push([x, v]);
                    }
                }
            }
        }
        for (g, points) in lines {
            if points.len() < 2 {
                continue;
            }
            let color = group_color(panel, g, palette::LINE);
            let name = format!("{}line", group_prefix(panel, g));
            if self.connect == LineConnect::Summary {
                layer.marks.push(Mark::Points {
                    points: points.clone(),
                    color,
                    shape: MarkShape::Circle,
                    radius: 3.5,
                    filled: true,
                    name: name.clone(),
                });
            }
            layer.marks.push(Mark::Line { points, color, width: 1.8, pattern: LinePattern::Solid, name });
        }
        if !layer.marks.is_empty() {
            let what = match self.connect {
                LineConnect::Summary => format!("Line through the {}", self.stat.label().to_lowercase()),
                other => format!("Line joining {}", other.label().to_lowercase()),
            };
            layer.key_entry(what, palette::LINE, KeyGlyph::Line(LinePattern::Solid));
        }
        layer
    }
}

/// The four run-chart tests of `values` in row order (runs about the
/// median: clustering, mixing; runs up and down: trend, oscillation), each a
/// one-sided p-value; `None` with fewer than 10 values or when a test cannot
/// run (all values on one side of the median).
fn run_chart_note(values: &[f64], prefix: &str) -> Option<String> {
    if values.len() < 10 {
        return None;
    }
    let t = stat_inference::runs::run_chart_tests(values).ok()?;
    let p = |x: f64| if x < 1e-4 { "< 0.0001".to_owned() } else { format!("= {x:.4}") };
    Some(format!(
        "Run chart tests ({prefix}line, {} values): clustering p {}, mixing p {}, trend p {}, oscillation p {}",
        values.len(),
        p(t.clustering()),
        p(t.mixing()),
        p(t.trend()),
        p(t.oscillation()),
    ))
}

// ---------------------------------------------------------------------------

/// A heat map: one cell per pair of categories across and up, shaded light
/// to dark by the number of rows (frequency-weighted) or, with a continuous
/// color column, by a statistic of its values; optionally labelled. Clicking
/// a cell selects its rows; cells holding selected rows are outlined.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeatmapElement {
    /// Statistic of the color column's values (ignored without one: counts).
    pub stat: SummaryStat,
    /// Write each cell's value.
    pub labels: bool,
}

impl Default for HeatmapElement {
    fn default() -> Self {
        Self { stat: SummaryStat::Mean, labels: true }
    }
}

/// Most cells that get a written value.
const MAX_LABELLED_CELLS: usize = 400;

impl Element for HeatmapElement {
    fn kind(&self) -> ElementKind {
        ElementKind::Heatmap
    }

    fn layer(&self, panel: &Panel, state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        if !(panel.x.is_categorical() && panel.y.is_categorical()) {
            layer
                .notes
                .push("Heat maps need categories on both axes (for two numeric columns use binned counts).".to_owned());
            return layer;
        }
        let by_value = panel.points.iter().any(|p| p.color_value.is_finite());
        let mut cells: BTreeMap<(i64, i64), Vec<&PanelPoint>> = BTreeMap::new();
        for p in panel.points.iter().filter(|p| p.in_analysis()) {
            cells.entry((p.x.round() as i64, p.y.round() as i64)).or_default().push(p);
        }
        let values: Vec<((i64, i64), f64, &Vec<&PanelPoint>)> = cells
            .iter()
            .filter_map(|(&k, pts)| {
                let v = if by_value {
                    let obs: Vec<Obs> = pts.iter().map(|p| (p.color_value, p.freq, p.weight)).collect();
                    summarize(&obs, self.stat)
                } else {
                    Some(pts.iter().map(|p| p.freq).sum())
                };
                v.map(|v| (k, v, pts))
            })
            .collect();
        let (lo, hi) = values.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v.1), b.max(v.1)));
        if !lo.is_finite() {
            return layer;
        }
        let label_cells = self.labels && values.len() <= MAX_LABELLED_CELLS;
        for ((cx, cy), v, pts) in &values {
            let (x, y) = (*cx as f64, *cy as f64);
            let t = if hi > lo { (v - lo) / (hi - lo) } else { 0.5 };
            // Counts: light (few) to dark (many). Color-column values use the
            // same direction as the points' color ramp.
            let shade = if by_value { 1.0 - t } else { t };
            let fill: Rgba = palette::ramp(1.0 - shade);
            let outline = rect(x - 0.5, x + 0.5, y - 0.5, y + 0.5);
            layer.marks.push(Mark::Polygon { points: outline.clone(), fill, stroke: fill, name: "Cell".to_owned() });
            if pts.iter().any(|p| p.row < state.len() && state.is_selected(p.row)) {
                let mut ring = rect(x - 0.47, x + 0.47, y - 0.47, y + 0.47);
                ring.push(ring[0]);
                layer.marks.push(Mark::Line {
                    points: ring,
                    color: palette::SELECTED,
                    width: 2.5,
                    pattern: LinePattern::Solid,
                    name: "Selected rows".to_owned(),
                });
            }
            if label_cells {
                // Light text on the dark end of the ramp.
                let color = if shade > 0.55 { Rgba::rgb(255, 255, 255) } else { Rgba::rgb(20, 20, 30) };
                layer.marks.push(Mark::Text {
                    at: [x, y],
                    text: format_value(*v),
                    color,
                    anchor: TextAnchor::Centre,
                    movable: None,
                });
            }
            layer.regions.push(Region { outline, rows: rows_of(pts) });
        }
        let title =
            if by_value { format!("{} of the color column", self.stat.label()) } else { "Rows per cell".to_owned() };
        layer.ramp = Some(Ramp { title, min: lo, max: hi, reversed: !by_value });
        if layer.marks.iter().any(|m| matches!(m, Mark::Line { name, .. } if name == "Selected rows")) {
            layer.key_entry("Cell holds selected rows", palette::SELECTED, KeyGlyph::Line(LinePattern::Solid));
        }
        layer
    }
}

#[cfg(test)]
mod tests {
    use super::run_chart_note;

    #[test]
    fn run_chart_note_reports_four_tests() {
        // A steady rise: one run up, so the trend p-value is tiny.
        let rising: Vec<f64> = (0..20).map(f64::from).collect();
        let note = run_chart_note(&rising, "").unwrap();
        assert!(note.contains("trend p < 0.0001"), "{note}");
        assert!(note.contains("20 values"));
        assert!(run_chart_note(&rising[..9], "").is_none(), "too few values");
        assert!(run_chart_note(&[1.0; 12], "").is_none(), "no values on either side of the median");
    }

    /// Lollipops: a stem from zero and a dot at each mean, in the subgroup's
    /// shape; selected dots ringed; clicking a stem selects its rows.
    #[test]
    fn lollipops_draw_stems_and_dots() {
        use super::{BarElement, BarLayout, Element};
        use crate::layer::{Mark, MarkShape};
        use stat_data::{Column, DataTable, RowStateManager, SelectionMode};
        let t = DataTable::new(
            "l",
            vec![
                Column::text("g", ["a", "a", "b", "b"].map(String::from).to_vec()),
                Column::numeric("y", vec![2.0, 4.0, -1.0, -3.0]),
            ],
        )
        .unwrap();
        let mut state = RowStateManager::new(4);
        state.select([2], SelectionMode::Replace);
        let plan = crate::EncodingPlan {
            x: vec!["g".into()],
            y: vec!["y".into()],
            overlay: Some("g".into()),
            ..Default::default()
        };
        let frame = crate::build_frame(&t, &state, &plan, &crate::FrameOptions::default()).unwrap();
        let layer = BarElement { layout: BarLayout::Lollipop, ..BarElement::default() }.layer(&frame.panels[0], &state);
        let dots: Vec<(f64, MarkShape)> = layer
            .marks
            .iter()
            .filter_map(|m| match m {
                Mark::Points { points, shape, filled: true, .. } => Some((points[0][1], *shape)),
                _ => None,
            })
            .collect();
        assert_eq!(dots, [(3.0, MarkShape::Circle), (-2.0, MarkShape::Square)]);
        let stems =
            layer.marks.iter().filter(|m| matches!(m, Mark::Line { points, .. } if points[0][1] == 0.0)).count();
        assert_eq!(stems, 2);
        assert!(
            layer
                .marks
                .iter()
                .any(|m| matches!(m, Mark::Points { name, filled: false, .. } if name == "Selected rows"))
        );
        assert_eq!(layer.regions.len(), 2);
        assert!(!layer.marks.iter().any(|m| matches!(m, Mark::Polygon { .. })), "no bar bodies");
        assert!(layer.key.iter().any(|k| k.label.starts_with("Lollipops")));
        assert_eq!(BarLayout::from_key("lollipop"), Some(BarLayout::Lollipop));
    }

    /// Positions of 10¹⁰ and more used to share one slot (an `as i64` cast
    /// saturated), so a bar chart drew a single bar holding every row.
    #[test]
    fn bars_at_large_positions_stay_apart() {
        use super::{BarElement, BarLayout, Element};
        use crate::layer::Mark;
        use stat_data::{Column, DataTable, RowStateManager};
        let bars = |x: Vec<f64>, y: Vec<f64>, g: Option<Vec<&str>>, layout: BarLayout| -> Vec<(f64, f64, f64)> {
            let n = x.len();
            let mut cols = vec![Column::numeric("x", x), Column::numeric("y", y)];
            let mut plan = crate::EncodingPlan { x: vec!["x".into()], y: vec!["y".into()], ..Default::default() };
            if let Some(g) = g {
                cols.push(Column::text("g", g.into_iter().map(String::from).collect()));
                plan.overlay = Some("g".into());
            }
            let t = DataTable::new("t", cols).unwrap();
            let state = RowStateManager::new(n);
            let frame = crate::build_frame(&t, &state, &plan, &crate::FrameOptions::default()).unwrap();
            let layer = BarElement { layout, ..BarElement::default() }.layer(&frame.panels[0], &state);
            layer
                .marks
                .iter()
                .filter_map(|m| match m {
                    Mark::Polygon { points, .. } => {
                        let (x0, x1, y0, y1) = points.iter().fold(
                            (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY),
                            |(a, b, c, d), p| (a.min(p[0]), b.max(p[0]), c.min(p[1]), d.max(p[1])),
                        );
                        Some((0.5 * (x0 + x1), y0, y1))
                    }
                    _ => None,
                })
                .collect()
        };
        let side = bars(vec![1e10, 2e10, 3e10], vec![1.0, 2.0, 3.0], None, BarLayout::SideBySide);
        assert_eq!(side.len(), 3, "{side:?}");
        for ((x, lo, hi), (want_x, want_h)) in side.iter().zip([(1e10, 1.0), (2e10, 2.0), (3e10, 3.0)]) {
            assert!((x - want_x).abs() <= 1e-6 * want_x, "{x} vs {want_x}");
            assert!(lo.abs() < 1e-12 && (hi - want_h).abs() < 1e-12, "{lo}..{hi}");
        }
        // 100% stacks: two subgroups at each of two positions, each stack 0–100.
        let pct = bars(
            vec![1e10, 1e10, 2e10, 2e10],
            vec![1.0, 3.0, 2.0, 2.0],
            Some(vec!["a", "b", "a", "b"]),
            BarLayout::Percent,
        );
        assert_eq!(pct.len(), 4, "{pct:?}");
        for centre in [1e10, 2e10] {
            let mut stack: Vec<(f64, f64)> =
                pct.iter().filter(|b| (b.0 - centre).abs() <= 1e-6 * centre).map(|b| (b.1, b.2)).collect();
            stack.sort_by(|a, b| a.0.total_cmp(&b.0));
            assert_eq!(stack.len(), 2, "{centre}: {pct:?}");
            assert!(stack[0].0.abs() < 1e-9 && (stack[1].1 - 100.0).abs() < 1e-9, "{stack:?}");
            assert!((stack[0].1 - stack[1].0).abs() < 1e-9, "{stack:?}");
        }
    }
}
