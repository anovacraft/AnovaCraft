//! Whole-panel chart forms that do not use the usual axes: treemaps and
//! parallel coordinates. Each builds its own [`Frame`] (one panel, with its
//! geometry in [`PanelExtra`]) and draws with its own element; points can
//! still be shown on top, and box and click selection work as usual.
//!
//! * **Treemap** — nested rectangles, one level per column on the
//!   horizontal-axis shelf (in order), with areas proportional to the number
//!   of rows (frequency-weighted) or to the sum of the first vertical column.
//!   Rectangles are laid out by the squarified algorithm (Bruls, Huizing & van
//!   Wijk, "Squarified Treemaps", *Data Visualization 2000*), which keeps
//!   aspect ratios near 1.
//! * **Parallel coordinates** — one vertical axis per column (horizontal
//!   shelf, then vertical shelf), each scaled from its minimum (bottom) to
//!   maximum (top), every row a line across them (Inselberg, "The Plane with
//!   Parallel Coordinates", *The Visual Computer* 1(2), 1985; Wegman,
//!   *JASA* 85(411), 1990).
//! * **Multi-vari chart** — the first vertical column against one to four
//!   factors on the horizontal shelf (outermost first; Seder, "Diagnosis with
//!   diagrams", *Industrial Quality Control* 6, 1950): every row at its
//!   innermost cell, each factor's level means marked and joined within each
//!   level of the factor above, the outermost factor's levels as labelled
//!   sections.
//! * **Scatterplot matrix** — every pair of the continuous columns on the
//!   axis shelves in a square grid of panels (Chambers, Cleveland, Kleiner &
//!   Tukey, *Graphical Methods for Data Analysis*, 1983, §5.4): the panel in
//!   row `i`, column `j` plots column `i` up against column `j` across, with
//!   the graph's layers (points, density ellipses, fits, …); the diagonal
//!   panels show each column's histogram (or the dot plot or probability plot
//!   layer when one is on). Brushing any panel selects the rows everywhere.
//! * **Pie and donut charts, radar charts, polar charts and wind roses**
//!   ([`radial`]) — round forms drawn true to shape.
//! * **Ternary plot** ([`ternary`]) — three components normalised to sum 1.
//! * **Waterfall chart** ([`waterfall`]), **ridgeline plot** ([`ridgeline`])
//!   and **vector field** ([`vector`]).

use std::collections::BTreeMap;

use stat_core::Result;
use stat_data::{DataTable, RowStateManager};
use stat_linear::natural_order;

use crate::element::{Element, ElementKind};
use crate::encoding::{EncodingPlan, invalid, is_continuous};
use crate::frame::{
    AxisKind, AxisSpec, ColorKey, Frame, FrameOptions, GroupStyle, Legend, MarkerStyle, Panel, PanelExtra, PanelPoint,
    ParallelAxis, Tile, filter_note, row_styles, select_rows,
};
use crate::layer::{KeyGlyph, Layer, LinePattern, Mark, MarkShape, Region, Rgba, TextAnchor};
use crate::palette;
use crate::scale::AxisScale;
use crate::view::format_value;

pub mod radial;
pub mod ridgeline;
pub mod ternary;
pub mod vector;
pub mod waterfall;

pub use radial::{
    AngleUnit, PieElement, PieGeometry, PieSettings, PieSlice, PolarElement, PolarGeometry, PolarSettings,
    RadarElement, RadarGeometry, RadarPolygon, RadarSettings, WindPiece, WindRoseElement, WindRoseGeometry,
    WindRoseSettings, build_pie_frame, build_polar_frame, build_radar_frame, build_wind_rose_frame,
};
pub use ridgeline::{Ridge, RidgelineElement, RidgelineGeometry, RidgelineSettings, build_ridgeline_frame};
pub use ternary::{TernaryElement, TernaryGeometry, build_ternary_frame, ternary_position};
pub use vector::{Arrow, VectorElement, VectorGeometry, VectorSettings, build_vector_frame};
pub use waterfall::{
    StepKind, WaterfallBar, WaterfallElement, WaterfallGeometry, WaterfallSettings, build_waterfall_frame,
};

/// Settings of a treemap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TreemapSettings {
    /// Size tiles by the sum of the first vertical column (else by row count).
    pub size_by_sum: bool,
    /// Write labels in the tiles.
    pub labels: bool,
}

/// The form of a graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GraphForm {
    /// Panels with axes and layers.
    #[default]
    Panels,
    /// A treemap of the horizontal-shelf columns.
    Treemap(TreemapSettings),
    /// Parallel coordinates of the shelf columns.
    Parallel,
    /// Multi-vari chart of the first vertical column by the horizontal-shelf
    /// columns.
    MultiVari,
    /// Scatterplot matrix of the continuous shelf columns.
    ScatterMatrix,
    /// Clustered heat map of the numeric horizontal-shelf columns.
    ClusterMap(crate::cluster_map::ClusterMapSettings),
    /// Estimation plot of the vertical column by the horizontal column's groups
    /// (settings in [`crate::GraphSpec::estimation`]).
    Estimation,
    /// Sankey diagram of the horizontal-shelf columns as stages.
    Sankey(crate::flows::FlowSettings),
    /// Chord diagram of a "from" and a "to" column.
    Chord(crate::flows::FlowSettings),
    /// Node-link network of the two ends of each edge.
    Network(crate::flows::NetworkSettings),
    /// Pie or donut chart of the first horizontal-shelf column's levels.
    Pie(PieSettings),
    /// Radar (star) chart: one spoke per numeric shelf column.
    Radar(RadarSettings),
    /// Polar chart: points at the horizontal column's angle and the vertical
    /// column's radius.
    Polar(PolarSettings),
    /// Wind rose: stacked counts by direction sector and binned magnitude.
    WindRose(WindRoseSettings),
    /// Ternary plot of three component columns normalised to sum 1.
    Ternary,
    /// Waterfall (bridge) chart: running total of signed contributions.
    Waterfall(WaterfallSettings),
    /// Ridgeline plot: kernel densities per group stacked vertically.
    Ridgeline(RidgelineSettings),
    /// Vector field: an arrow per row from two component columns.
    VectorField(VectorSettings),
}

impl GraphForm {
    /// Short name for saving.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Panels => "panels",
            Self::Treemap(_) => "treemap",
            Self::Parallel => "parallel",
            Self::MultiVari => "multi-vari",
            Self::ScatterMatrix => "scatter-matrix",
            Self::ClusterMap(_) => "cluster-map",
            Self::Estimation => "estimation",
            Self::Sankey(_) => "sankey",
            Self::Chord(_) => "chord",
            Self::Network(_) => "network",
            Self::Pie(_) => "pie",
            Self::Radar(_) => "radar",
            Self::Polar(_) => "polar",
            Self::WindRose(_) => "wind-rose",
            Self::Ternary => "ternary",
            Self::Waterfall(_) => "waterfall",
            Self::Ridgeline(_) => "ridgeline",
            Self::VectorField(_) => "vector-field",
        }
    }

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Panels => "Panels with layers",
            Self::Treemap(_) => "Treemap",
            Self::Parallel => "Parallel coordinates",
            Self::MultiVari => "Multi-vari chart",
            Self::ScatterMatrix => "Scatterplot matrix",
            Self::ClusterMap(_) => "Clustered heat map",
            Self::Estimation => "Estimation plot",
            Self::Sankey(_) => "Sankey diagram",
            Self::Chord(_) => "Chord diagram",
            Self::Network(_) => "Network",
            Self::Pie(_) => "Pie chart",
            Self::Radar(_) => "Radar chart",
            Self::Polar(_) => "Polar chart",
            Self::WindRose(_) => "Wind rose",
            Self::Ternary => "Ternary plot",
            Self::Waterfall(_) => "Waterfall chart",
            Self::Ridgeline(_) => "Ridgeline plot",
            Self::VectorField(_) => "Vector field",
        }
    }

    /// `true` for the round forms (pie, radar, polar, wind rose, ternary,
    /// chord diagram, network), which are drawn with one unit the same length
    /// across and up so circles stay circles and the triangle stays
    /// equilateral.
    #[must_use]
    pub fn equal_aspect(self) -> bool {
        matches!(
            self,
            Self::Pie(_)
                | Self::Radar(_)
                | Self::Polar(_)
                | Self::WindRose(_)
                | Self::Ternary
                | Self::Chord(_)
                | Self::Network(_)
        )
    }

    /// `true` for the forms that draw their own axes (or none), so the plot
    /// shows no numeric axes, ticks or grid: treemaps, clustered heat maps,
    /// Sankey diagrams and the round forms.
    #[must_use]
    pub fn draws_own_axes(self) -> bool {
        matches!(self, Self::Treemap(_) | Self::ClusterMap(_) | Self::Sankey(_)) || self.equal_aspect()
    }

    /// `true` for the forms whose rows are drawn as shared shapes (slices,
    /// sectors, bars, tiles, group polygons) rather than one mark each: a
    /// click selects the shape's rows before any single row placed inside
    /// it for box selection.
    #[must_use]
    pub fn selects_by_region(self) -> bool {
        match self {
            Self::Pie(_) | Self::WindRose(_) | Self::Waterfall(_) | Self::Treemap(_) => true,
            Self::Radar(r) => r.group_means,
            _ => false,
        }
    }
}

/// An unnamed linear axis (forms that draw in the unit square).
fn plain_axis() -> AxisSpec {
    AxisSpec { name: String::new(), kind: AxisKind::Continuous, scale: AxisScale::Linear, time: None }
}

/// One frame holding a single panel.
fn single_panel_frame(panel: Panel, legend: Legend, notes: Vec<String>, pages: Vec<String>, page: usize) -> Frame {
    Frame {
        panels: vec![panel],
        n_rows: 1,
        n_cols: 1,
        column_headers: vec![String::new()],
        row_headers: vec![String::new()],
        legend,
        notes,
        pages,
        page,
    }
}

// ---------------------------------------------------------------------------
// Treemap
// ---------------------------------------------------------------------------

/// Lay out `values` (all positive) in `rect` (`[x0, y0, x1, y1]`) by the
/// squarified algorithm, in the given order (callers sort descending).
/// Returns one rectangle per value.
#[must_use]
pub fn squarify(values: &[f64], rect: [f64; 4]) -> Vec<[f64; 4]> {
    let mut out = vec![[0.0; 4]; values.len()];
    // Relative sizes, so huge values cannot overflow the total.
    let largest = values.iter().copied().filter(|v| v.is_finite()).fold(0.0_f64, f64::max);
    if largest <= 0.0 || values.iter().any(|v| !v.is_finite()) {
        return out;
    }
    let values: Vec<f64> = values.iter().map(|v| v / largest).collect();
    let total: f64 = values.iter().sum();
    let [mut x0, mut y0, x1, y1] = rect;
    let (w, h) = (x1 - x0, y1 - y0);
    if values.is_empty() || total <= 0.0 || w <= 0.0 || h <= 0.0 {
        return out;
    }
    // Areas in the rectangle's units.
    let scale = w * h / total;
    let areas: Vec<f64> = values.iter().map(|v| v * scale).collect();
    // Worst aspect ratio of a row of areas laid along a side of length `side`.
    let worst = |row: &[f64], side: f64| -> f64 {
        let s: f64 = row.iter().sum();
        let (mx, mn) = row.iter().fold((0.0_f64, f64::INFINITY), |(a, b), &v| (a.max(v), b.min(v)));
        if s <= 0.0 || mn <= 0.0 {
            return f64::INFINITY;
        }
        (side * side * mx / (s * s)).max(s * s / (side * side * mn))
    };
    let mut start = 0;
    while start < areas.len() {
        let (w, h) = (x1 - x0, y1 - y0);
        let side = w.min(h);
        let mut end = start + 1;
        while end < areas.len() && worst(&areas[start..=end], side) <= worst(&areas[start..end], side) {
            end += 1;
        }
        // Lay the row [start, end) along the shorter side.
        let s: f64 = areas[start..end].iter().sum();
        if w >= h {
            // A column on the left, `s / h` wide.
            let cw = if h > 0.0 { s / h } else { 0.0 };
            let mut y = y0;
            for k in start..end {
                let kh = if cw > 0.0 { areas[k] / cw } else { 0.0 };
                out[k] = [x0, y, x0 + cw, y + kh];
                y += kh;
            }
            x0 += cw;
        } else {
            // A row along the bottom, `s / w` tall.
            let rh = if w > 0.0 { s / w } else { 0.0 };
            let mut x = x0;
            for k in start..end {
                let kw = if rh > 0.0 { areas[k] / rh } else { 0.0 };
                out[k] = [x, y0, x + kw, y0 + rh];
                x += kw;
            }
            y0 += rh;
        }
        start = end;
    }
    out
}

/// Recursively tile `rows` by `levels[depth..]` inside `rect`.
#[allow(clippy::too_many_arguments)]
fn tile_level(
    table: &DataTable,
    levels: &[String],
    depth: usize,
    rows: &[usize],
    size: &[f64],
    rect: [f64; 4],
    top_color: Option<Rgba>,
    tiles: &mut Vec<Tile>,
    top_levels: &[String],
    style: &MarkerStyle,
) {
    let Some(column) = levels.get(depth) else { return };
    let Some(col) = table.find(column).and_then(|i| table.column(i)) else { return };
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for &r in rows {
        let label = col.level_text(r);
        let label = if label.trim().is_empty() { "(missing)".to_owned() } else { label };
        groups.entry(label).or_default().push(r);
    }
    let mut entries: Vec<(String, Vec<usize>, f64)> = groups
        .into_iter()
        .map(|(l, rs)| {
            let v: f64 = rs.iter().map(|&r| size[r]).sum();
            (l, rs, v)
        })
        .filter(|e| e.2 > 0.0 && e.2.is_finite())
        .collect();
    // Largest first (ties in natural order), as squarifying expects.
    entries.sort_by(|a, b| b.2.total_cmp(&a.2).then_with(|| natural_order(&a.0, &b.0)));
    let values: Vec<f64> = entries.iter().map(|e| e.2).collect();
    let rects = squarify(&values, rect);
    for ((label, rs, value), r) in entries.into_iter().zip(rects) {
        let color = match top_color {
            Some(c) => c,
            None => {
                let k = top_levels.iter().position(|l| *l == label).unwrap_or(0);
                style.level(&levels[0], &label, k).0
            }
        };
        // Children sit inside a small margin, leaving room for the parent's label.
        let pad_x = 0.004 * (r[2] - r[0]).signum();
        let header = if depth + 1 < levels.len() { 0.035_f64.min(0.25 * (r[3] - r[1])) } else { 0.0 };
        let inner = [r[0] + pad_x, r[1] + 0.004, r[2] - pad_x, r[3] - header - 0.004];
        let mut sorted = rs.clone();
        sorted.sort_unstable();
        tiles.push(Tile { rect: r, label, depth, value, rows: sorted, color });
        if depth + 1 < levels.len() && inner[2] > inner[0] && inner[3] > inner[1] {
            tile_level(table, levels, depth + 1, &rs, size, inner, Some(color), tiles, top_levels, style);
        }
    }
}

/// The frame of a treemap: one panel whose tiles nest the levels of the
/// horizontal-shelf columns; each row's point sits at the centre of its
/// innermost tile (for selection).
///
/// # Errors
/// [`stat_core::StatError::InvalidArgument`] without a horizontal column, or
/// when the plan does not fit the table.
pub fn build_treemap_frame(
    table: &DataTable,
    state: &RowStateManager,
    plan: &EncodingPlan,
    options: &FrameOptions,
    settings: TreemapSettings,
) -> Result<Frame> {
    plan.validate(table)?;
    if plan.x.is_empty() {
        return Err(invalid("a treemap needs one or more columns on the horizontal axis (outer level first)"));
    }
    let rs = select_rows(table, state, plan, options);
    let style = options.style.for_panel(0);
    let styles = row_styles(table, state, plan, &rs.rows, &style);
    let n = table.n_rows();
    let mut notes = Vec::new();
    // Size of each row: its frequency, or frequency × value of the first vertical column.
    let size_column = plan.y.first().filter(|_| settings.size_by_sum);
    let values = size_column.and_then(|c| table.find(c)).and_then(|i| table.column(i)).and_then(|c| c.as_numeric());
    if settings.size_by_sum && values.is_none() {
        notes.push("Sizing by a sum needs a numeric column on the vertical axis; tiles show row counts.".to_owned());
    }
    let mut dropped = 0;
    let size: Vec<f64> = (0..n)
        .map(|r| {
            let f = rs.freq[r];
            match values {
                Some(v) => {
                    let x = v.get(r).copied().unwrap_or(f64::NAN);
                    if x.is_finite() && x >= 0.0 && (f * x).is_finite() {
                        f * x
                    } else {
                        dropped += 1;
                        0.0
                    }
                }
                None => f,
            }
        })
        .collect();
    let used: Vec<usize> = rs
        .page_rows
        .iter()
        .copied()
        .filter(|&r| !rs.filtered[r] && (r >= state.len() || !state.is_excluded(r)) && size[r] > 0.0)
        .collect();
    if dropped > 0 && values.is_some() {
        notes.push(format!("{dropped} row(s) with a missing or negative size are left out of the tiles."));
    }
    let top_levels: Vec<String> = {
        let col = table.find(&plan.x[0]).and_then(|i| table.column(i));
        let mut l: Vec<String> = col
            .map(|c| {
                used.iter()
                    .map(|&r| c.level_text(r))
                    .map(|t| if t.trim().is_empty() { "(missing)".to_owned() } else { t })
                    .collect()
            })
            .unwrap_or_default();
        l.sort_by(|a, b| natural_order(a, b));
        l.dedup();
        l
    };
    let mut tiles = Vec::new();
    tile_level(table, &plan.x, 0, &used, &size, [0.0, 0.0, 1.0, 1.0], None, &mut tiles, &top_levels, &style);
    // Each row's point: the centre of the deepest tile holding it (a parent
    // too thin to subdivide keeps its own rows).
    let mut centre: Vec<Option<[f64; 2]>> = vec![None; n];
    let mut by_depth: Vec<&Tile> = tiles.iter().collect();
    by_depth.sort_by_key(|t| std::cmp::Reverse(t.depth));
    for t in by_depth {
        let c = [0.5 * (t.rect[0] + t.rect[2]), 0.5 * (t.rect[1] + t.rect[3])];
        for &r in &t.rows {
            centre[r].get_or_insert(c);
        }
    }
    let points = used
        .iter()
        .filter_map(|&r| {
            centre[r].map(|[x, y]| PanelPoint {
                row: r,
                x,
                y,
                group: 0,
                color: styles.color[r],
                shape: styles.shape[r],
                radius: styles.radius[r],
                excluded: false,
                filtered: false,
                freq: rs.freq[r],
                weight: rs.weight[r],
                color_value: styles.color_value[r],
            })
        })
        .collect();
    let mut legend = Legend::default();
    let entries: Vec<(String, Rgba, MarkShape)> = top_levels
        .iter()
        .enumerate()
        .map(|(k, l)| {
            let (c, s) = style.level(&plan.x[0], l, k);
            (l.clone(), c, s)
        })
        .collect();
    legend.color = Some(ColorKey::Levels { title: plan.x[0].clone(), entries });
    notes.extend(filter_note(state, options, &rs.passes));
    let panel = Panel {
        grid_row: 0,
        grid_col: 0,
        title: None,
        x: plain_axis(),
        y: plain_axis(),
        points,
        groups: vec![GroupStyle { label: String::new(), color: palette::DEFAULT_POINT, shape: MarkShape::Circle }],
        slot_half_width: 0.0,
        extra: PanelExtra::Tiles(tiles),
        labels: std::collections::BTreeMap::new(),
        data_extent: [None; 2],
    };
    Ok(single_panel_frame(panel, legend, notes, rs.pages, rs.page))
}

/// Draws the tiles of a treemap panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TreemapElement {
    /// Write labels in the tiles.
    pub labels: bool,
}

impl Element for TreemapElement {
    fn kind(&self) -> ElementKind {
        ElementKind::Treemap
    }

    fn layer(&self, panel: &Panel, state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        let PanelExtra::Tiles(tiles) = &panel.extra else { return layer };
        let deepest = tiles.iter().map(|t| t.depth).max().unwrap_or(0);
        let total: f64 = tiles.iter().filter(|t| t.depth == 0).map(|t| t.value).sum();
        for t in tiles {
            let [x0, y0, x1, y1] = t.rect;
            let outline = vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
            // Deeper levels lighter so nesting reads without color alone.
            let alpha = if deepest == 0 { 0.7 } else { 0.85 - 0.5 * t.depth as f64 / deepest as f64 };
            layer.marks.push(Mark::Polygon {
                points: outline.clone(),
                fill: t.color.with_alpha(alpha),
                stroke: Rgba::rgb(255, 255, 255),
                name: t.label.clone(),
            });
            let selected = t.rows.iter().filter(|&&r| r < state.len() && state.is_selected(r)).count();
            if t.depth == deepest {
                if selected > 0 {
                    let mut ring = outline.clone();
                    ring.push(ring[0]);
                    layer.marks.push(Mark::Line {
                        points: ring,
                        color: palette::SELECTED,
                        width: 2.5,
                        pattern: LinePattern::Solid,
                        name: "Selected rows".to_owned(),
                    });
                }
                layer.regions.push(Region { outline, rows: t.rows.clone() });
            }
            let area = (x1 - x0) * (y1 - y0);
            if self.labels && area > 0.003 {
                let share =
                    if total > 0.0 { format!(" · {}%", format_value(100.0 * t.value / total)) } else { String::new() };
                let text = if t.depth == deepest {
                    format!("{}\n{}{share}", t.label, format_value(t.value))
                } else {
                    t.label.clone()
                };
                let (at, anchor) = if t.depth == deepest {
                    ([0.5 * (x0 + x1), 0.5 * (y0 + y1)], TextAnchor::Centre)
                } else {
                    ([x0 + 0.006, y1 - 0.018], TextAnchor::LeftOfText)
                };
                layer.marks.push(Mark::Text { at, text, color: Rgba::rgb(20, 20, 30), anchor, movable: None });
            }
        }
        if layer.marks.iter().any(|m| matches!(m, Mark::Line { name, .. } if name == "Selected rows")) {
            layer.key_entry("Tile holds selected rows", palette::SELECTED, KeyGlyph::Line(LinePattern::Solid));
        }
        layer
    }
}

// ---------------------------------------------------------------------------
// Parallel coordinates
// ---------------------------------------------------------------------------

/// The frame of a parallel-coordinates chart: one panel with an axis per
/// column at x = 1, 2, … and one point per row and axis at the row's scaled
/// value (0 at the axis minimum, 1 at its maximum; categories evenly
/// spaced).
///
/// # Errors
/// [`stat_core::StatError::InvalidArgument`] with fewer than two columns on
/// the axis shelves, or when the plan does not fit the table.
pub fn build_parallel_frame(
    table: &DataTable,
    state: &RowStateManager,
    plan: &EncodingPlan,
    options: &FrameOptions,
) -> Result<Frame> {
    plan.validate(table)?;
    let mut columns: Vec<&str> = Vec::new();
    for c in plan.x.iter().chain(&plan.y) {
        if !columns.contains(&c.as_str()) {
            columns.push(c);
        }
    }
    if columns.len() < 2 {
        return Err(invalid("parallel coordinates need two or more columns on the axis shelves"));
    }
    let rs = select_rows(table, state, plan, options);
    let style = options.style.for_panel(0);
    let styles = row_styles(table, state, plan, &rs.rows, &style);
    let mut axes = Vec::new();
    // Scaled position of every row on every axis.
    let mut position: Vec<Vec<Option<f64>>> = Vec::new();
    for &c in &columns {
        let col = table.find(c).and_then(|i| table.column(i));
        if is_continuous(table, c) {
            let v = col.and_then(|c| c.as_numeric()).unwrap_or(&[]);
            let (lo, hi) = rs
                .rows
                .iter()
                .filter_map(|&r| v.get(r).copied().filter(|x| x.is_finite()))
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(x), b.max(x)));
            position.push(
                (0..table.n_rows())
                    .map(|r| {
                        let x = v.get(r).copied().filter(|x| x.is_finite())?;
                        Some(if hi > lo { (x - lo) / (hi - lo) } else { 0.5 })
                    })
                    .collect(),
            );
            axes.push(ParallelAxis {
                name: c.to_owned(),
                low: if lo.is_finite() { lo } else { 0.0 },
                high: if hi.is_finite() { hi } else { 0.0 },
                levels: Vec::new(),
            });
        } else {
            let labels: Vec<String> =
                (0..table.n_rows()).map(|r| col.map(|c| c.level_text(r)).unwrap_or_default()).collect();
            let mut levels: Vec<String> =
                rs.rows.iter().map(|&r| labels[r].clone()).filter(|l| !l.trim().is_empty()).collect();
            levels.sort_by(|a, b| natural_order(a, b));
            levels.dedup();
            let k = levels.len();
            position.push(
                labels
                    .iter()
                    .map(|l| {
                        let i = levels.iter().position(|x| x == l)?;
                        Some(if k > 1 { i as f64 / (k - 1) as f64 } else { 0.5 })
                    })
                    .collect(),
            );
            axes.push(ParallelAxis { name: c.to_owned(), low: 0.0, high: 0.0, levels });
        }
    }
    let mut points = Vec::new();
    let mut incomplete = 0;
    for &r in &rs.page_rows {
        if position.iter().any(|p| p[r].is_none()) {
            incomplete += 1;
        }
        for (a, p) in position.iter().enumerate() {
            if let Some(y) = p[r] {
                points.push(PanelPoint {
                    row: r,
                    x: (a + 1) as f64,
                    y,
                    group: styles.group_codes[r].unwrap_or(0).min(styles.groups.len().saturating_sub(1)),
                    color: styles.color[r],
                    shape: styles.shape[r],
                    radius: styles.radius[r],
                    excluded: r < state.len() && state.is_excluded(r),
                    filtered: rs.filtered[r],
                    freq: rs.freq[r],
                    weight: rs.weight[r],
                    color_value: styles.color_value[r],
                });
            }
        }
    }
    let mut notes = Vec::new();
    if incomplete > 0 {
        notes.push(format!("{incomplete} row(s) have missing values; their lines skip those axes."));
    }
    notes.extend(filter_note(state, options, &rs.passes));
    let panel = Panel {
        grid_row: 0,
        grid_col: 0,
        title: None,
        x: AxisSpec {
            name: String::new(),
            kind: AxisKind::Categorical { levels: columns.iter().map(|c| (*c).to_owned()).collect() },
            scale: AxisScale::Linear,
            time: None,
        },
        y: AxisSpec { name: "Scaled value (minimum to maximum)".to_owned(), ..plain_axis() },
        points,
        groups: styles.groups,
        slot_half_width: 0.0,
        extra: PanelExtra::Parallel(axes),
        labels: std::collections::BTreeMap::new(),
        data_extent: [None; 2],
    };
    Ok(single_panel_frame(panel, styles.legend, notes, rs.pages, rs.page))
}

/// Draws the lines and axes of a parallel-coordinates panel. While rows are
/// selected, the other lines fade and the selected ones are drawn on top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ParallelElement;

impl Element for ParallelElement {
    fn kind(&self) -> ElementKind {
        ElementKind::Parallel
    }

    fn layer(&self, panel: &Panel, state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        let PanelExtra::Parallel(axes) = &panel.extra else { return layer };
        let axis_color = Rgba::rgb(120, 125, 140);
        let label_color = Rgba::rgb(60, 64, 76);
        for (a, axis) in axes.iter().enumerate() {
            let x = (a + 1) as f64;
            layer.marks.push(Mark::Line {
                points: vec![[x, 0.0], [x, 1.0]],
                color: axis_color,
                width: 1.5,
                pattern: LinePattern::Solid,
                name: axis.name.clone(),
            });
            if axis.levels.is_empty() {
                for (y, v) in [(0.0, axis.low), (1.0, axis.high)] {
                    layer.marks.push(Mark::Text {
                        at: [x + 0.03, y],
                        text: format_value(v),
                        color: label_color,
                        anchor: TextAnchor::LeftOfText,
                        movable: None,
                    });
                }
            } else {
                let k = axis.levels.len();
                for (i, l) in axis.levels.iter().enumerate() {
                    let y = if k > 1 { i as f64 / (k - 1) as f64 } else { 0.5 };
                    layer.marks.push(Mark::Text {
                        at: [x + 0.03, y],
                        text: l.clone(),
                        color: label_color,
                        anchor: TextAnchor::LeftOfText,
                        movable: None,
                    });
                }
            }
        }
        // Each row's polyline, bucketed by color and selection.
        let any_selected = state.counts().selected > 0;
        let mut by_row: BTreeMap<usize, Vec<&PanelPoint>> = BTreeMap::new();
        for p in &panel.points {
            by_row.entry(p.row).or_default().push(p);
        }
        let mut buckets: BTreeMap<(u32, bool), Vec<[[f64; 2]; 2]>> = BTreeMap::new();
        for (row, mut pts) in by_row {
            pts.sort_by(|a, b| a.x.total_cmp(&b.x));
            let selected = row < state.len() && state.is_selected(row);
            let color = if pts[0].excluded { palette::EXCLUDED } else { pts[0].color };
            let key = (u32::from_be_bytes(color.0), selected);
            let segs = buckets.entry(key).or_default();
            for w in pts.windows(2) {
                segs.push([[w[0].x, w[0].y], [w[1].x, w[1].y]]);
            }
        }
        // Unselected first, so the selection draws over them.
        let mut ordered: Vec<_> = buckets.into_iter().collect();
        ordered.sort_by_key(|((_, sel), _)| *sel);
        for ((rgba, selected), segments) in ordered {
            let color = Rgba(rgba.to_be_bytes());
            let (color, width) = match (any_selected, selected) {
                (true, true) => (palette::SELECTED, 2.2),
                (true, false) => (color.with_alpha(0.15), 1.0),
                _ => (color.with_alpha(0.6), 1.2),
            };
            layer.marks.push(Mark::Segments { segments, color, width, name: "Rows".to_owned() });
        }
        if any_selected {
            layer.key_entry("Selected rows", palette::SELECTED, KeyGlyph::Line(LinePattern::Solid));
        }
        layer
    }
}

// ---------------------------------------------------------------------------
// Multi-vari chart
// ---------------------------------------------------------------------------

/// The means and joins of a multi-vari panel.
#[derive(Debug, Clone, PartialEq)]
pub struct MultiVariGeometry {
    /// Factor names, outermost first.
    pub factors: Vec<String>,
    /// Level means of each factor: `(x, mean)` points, outermost factor first.
    pub means: Vec<Vec<[f64; 2]>>,
    /// Joins of the means: `(factor, polyline)`.
    pub joins: Vec<(usize, Vec<[f64; 2]>)>,
    /// Sections of the outermost factor: `(first slot, last slot, label)`
    /// (empty for a single factor).
    pub sections: Vec<(f64, f64, String)>,
    /// Range of the response, for section dividers and headings.
    pub y_range: (f64, f64),
}

/// The frame of a multi-vari chart: one panel whose horizontal axis has a
/// slot per innermost cell (combination of the factors' levels that occurs),
/// with an empty slot between levels of the outer factors; every row's point
/// sits at its cell. Cell and level means are weighted means
/// `Σ fᵢwᵢyᵢ / Σ fᵢwᵢ` of the plan's frequency `f` and weight `w` columns
/// (each 1 when absent), so a frequency counts as that many copies of the
/// row. Excluded rows, and rows whose frequency or weight is zero, negative
/// or missing, are drawn but left out of the means; a mean that overflows
/// (no finite value) is not drawn.
///
/// # Errors
/// [`stat_core::StatError::InvalidArgument`] without one to four horizontal
/// columns and a numeric vertical column, or when the plan does not fit the
/// table.
pub fn build_multivari_frame(
    table: &DataTable,
    state: &RowStateManager,
    plan: &EncodingPlan,
    options: &FrameOptions,
) -> Result<Frame> {
    plan.validate(table)?;
    if plan.x.is_empty() || plan.x.len() > 4 {
        return Err(invalid("a multi-vari chart needs one to four factors on the horizontal axis (outermost first)"));
    }
    let response = plan
        .y
        .first()
        .filter(|c| is_continuous(table, c))
        .ok_or_else(|| invalid("a multi-vari chart needs a numeric column on the vertical axis"))?;
    let y = table.find(response).and_then(|i| table.column(i)).and_then(|c| c.as_numeric()).unwrap_or(&[]);
    let factor_cols: Vec<_> = plan.x.iter().filter_map(|c| table.find(c).and_then(|i| table.column(i))).collect();
    let rs = select_rows(table, state, plan, options);
    let style = options.style.for_panel(0);
    let styles = row_styles(table, state, plan, &rs.rows, &style);
    let labels = |r: usize| -> Option<Vec<String>> {
        let l: Vec<String> = factor_cols.iter().map(|c| c.level_text(r).trim().to_owned()).collect();
        (l.iter().all(|s| !s.is_empty()) && y.get(r).is_some_and(|v| v.is_finite())).then_some(l)
    };
    let shown: Vec<(usize, Vec<String>)> =
        rs.page_rows.iter().filter(|&&r| !rs.filtered[r]).filter_map(|&r| labels(r).map(|l| (r, l))).collect();
    // Level codes in natural order, per factor.
    let depth = factor_cols.len();
    let mut levels: Vec<Vec<String>> = vec![Vec::new(); depth];
    for (_, l) in &shown {
        for (d, v) in l.iter().enumerate() {
            levels[d].push(v.clone());
        }
    }
    for l in &mut levels {
        l.sort_by(|a, b| natural_order(a, b));
        l.dedup();
    }
    let code = |l: &[String]| -> Vec<usize> {
        l.iter().enumerate().map(|(d, v)| levels[d].iter().position(|x| x == v).unwrap_or(0)).collect()
    };
    let cells: Vec<Vec<usize>> = {
        let set: std::collections::BTreeSet<Vec<usize>> = shown.iter().map(|(_, l)| code(l)).collect();
        set.into_iter().collect()
    };
    // Slots: one per cell, with an empty slot where an outer factor changes.
    let mut slot_labels: Vec<String> = Vec::new();
    let mut slot_of: BTreeMap<Vec<usize>, f64> = BTreeMap::new();
    for (k, cell) in cells.iter().enumerate() {
        if k > 0 && depth > 1 && cells[k - 1][..depth - 1] != cell[..depth - 1] {
            slot_labels.push(String::new());
        }
        let label: Vec<&str> =
            cell.iter().enumerate().skip(usize::from(depth > 1)).map(|(d, &c)| levels[d][c].as_str()).collect();
        slot_labels.push(label.join(" / "));
        slot_of.insert(cell.clone(), slot_labels.len() as f64);
    }
    // Means over included rows, by prefix, each row counted frequency ×
    // weight times; rows with no positive frequency or weight leave them.
    let count = |r: usize| rs.freq[r] * rs.weight[r];
    let included: Vec<(usize, Vec<usize>)> = shown
        .iter()
        .filter(|(r, _)| (*r >= state.len() || !state.is_excluded(*r)) && rs.freq[*r] > 0.0 && rs.weight[*r] > 0.0)
        .map(|(r, l)| (*r, code(l)))
        .collect();
    let mut means: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut joins: Vec<(usize, Vec<[f64; 2]>)> = Vec::new();
    let mut prefix_means: Vec<BTreeMap<Vec<usize>, [f64; 2]>> = Vec::new();
    for d in 0..depth {
        let mut groups: BTreeMap<Vec<usize>, (f64, f64)> = BTreeMap::new();
        for (r, c) in &included {
            let e = groups.entry(c[..=d].to_vec()).or_insert((0.0, 0.0));
            e.0 += count(*r) * y[*r];
            e.1 += count(*r);
        }
        // A sum that overflows leaves no finite mean; that mean is not drawn.
        let placed: BTreeMap<Vec<usize>, [f64; 2]> = groups
            .into_iter()
            .filter(|(_, (sum, n))| (sum / n).is_finite())
            .map(|(prefix, (sum, n))| {
                let xs: Vec<f64> = slot_of.iter().filter(|(c, _)| c.starts_with(&prefix)).map(|(_, &x)| x).collect();
                let x = xs.iter().sum::<f64>() / xs.len().max(1) as f64;
                (prefix, [x, sum / n])
            })
            .collect();
        means.push(placed.values().copied().collect());
        let mut by_parent: BTreeMap<Vec<usize>, Vec<[f64; 2]>> = BTreeMap::new();
        for (prefix, p) in &placed {
            by_parent.entry(prefix[..d].to_vec()).or_default().push(*p);
        }
        joins.extend(by_parent.into_values().filter(|l| l.len() >= 2).map(|l| (d, l)));
        prefix_means.push(placed);
    }
    let sections: Vec<(f64, f64, String)> = if depth > 1 {
        (0..levels[0].len())
            .filter_map(|k| {
                let xs: Vec<f64> = slot_of.iter().filter(|(c, _)| c[0] == k).map(|(_, &x)| x).collect();
                let lo = xs.iter().copied().fold(f64::INFINITY, f64::min);
                let hi = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                lo.is_finite().then(|| (lo, hi, format!("{}: {}", plan.x[0], levels[0][k])))
            })
            .collect()
    } else {
        Vec::new()
    };
    let points: Vec<PanelPoint> = shown
        .iter()
        .map(|(r, l)| PanelPoint {
            row: *r,
            x: slot_of[&code(l)],
            y: y[*r],
            group: styles.group_codes[*r].unwrap_or(0).min(styles.groups.len().saturating_sub(1)),
            color: styles.color[*r],
            shape: styles.shape[*r],
            radius: styles.radius[*r],
            excluded: *r < state.len() && state.is_excluded(*r),
            filtered: rs.filtered[*r],
            freq: rs.freq[*r],
            weight: rs.weight[*r],
            color_value: styles.color_value[*r],
        })
        .collect();
    let y_range = points.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.y), b.max(p.y)));
    let mut notes = Vec::new();
    let missing = rs.page_rows.iter().filter(|&&r| !rs.filtered[r] && labels(r).is_none()).count();
    if missing > 0 {
        notes.push(format!("{missing} row(s) without the response or a factor are left out."));
    }
    notes.extend(filter_note(state, options, &rs.passes));
    let panel = Panel {
        grid_row: 0,
        grid_col: 0,
        title: None,
        x: AxisSpec {
            name: plan.x[usize::from(depth > 1)..].join(" / "),
            kind: AxisKind::Categorical { levels: slot_labels },
            scale: AxisScale::Linear,
            time: None,
        },
        y: AxisSpec { name: response.clone(), ..plain_axis() },
        points,
        groups: styles.groups,
        slot_half_width: 0.0,
        extra: PanelExtra::MultiVari(MultiVariGeometry { factors: plan.x.clone(), means, joins, sections, y_range }),
        labels: std::collections::BTreeMap::new(),
        data_extent: [None; 2],
    };
    Ok(single_panel_frame(panel, styles.legend, notes, rs.pages, rs.page))
}

/// Draws the means, joins and sections of a multi-vari panel: each factor's
/// means in its own colour and marker shape (named in the key), joined
/// within each level of the factor above.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MultiVariElement;

impl Element for MultiVariElement {
    fn kind(&self) -> ElementKind {
        ElementKind::MultiVari
    }

    fn layer(&self, panel: &Panel, _state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        let PanelExtra::MultiVari(g) = &panel.extra else { return layer };
        let (y0, y1) = g.y_range;
        if y0.is_finite() && y1 >= y0 {
            let top = y1 + 0.08 * (y1 - y0).max(1e-9);
            for w in g.sections.windows(2) {
                let x = 0.5 * (w[0].1 + w[1].0);
                layer.marks.push(Mark::Line {
                    points: vec![[x, y0], [x, top]],
                    color: palette::LINE.with_alpha(0.5),
                    width: 1.0,
                    pattern: LinePattern::Dashed,
                    name: String::new(),
                });
            }
            for (lo, hi, label) in &g.sections {
                layer.marks.push(Mark::Text {
                    at: [0.5 * (lo + hi), top],
                    text: label.clone(),
                    color: palette::LABEL,
                    anchor: TextAnchor::Above,
                    movable: None,
                });
            }
        }
        let depth = g.factors.len();
        // Innermost first, so the outer factors' larger markers sit on top.
        for d in (0..depth).rev() {
            let (color, shape) = palette::level_style(depth - 1 - d);
            for (_, line) in g.joins.iter().filter(|(k, _)| *k == d) {
                layer.marks.push(Mark::Line {
                    points: line.clone(),
                    color,
                    width: if d + 1 == depth { 1.4 } else { 2.0 },
                    pattern: LinePattern::Solid,
                    name: format!("{} means", g.factors[d]),
                });
            }
            layer.marks.push(Mark::Points {
                points: g.means[d].clone(),
                color,
                shape,
                radius: 4.5 + (depth - 1 - d) as f32,
                filled: true,
                name: format!("{} means", g.factors[d]),
            });
            layer.key.push(crate::layer::KeyEntry {
                label: format!("{} means", g.factors[d]),
                color,
                glyph: KeyGlyph::Marker(shape, true),
            });
        }
        layer
    }
}

// ---------------------------------------------------------------------------
// Scatterplot matrix
// ---------------------------------------------------------------------------

/// The frame of a scatterplot matrix: the continuous columns of the
/// horizontal then vertical shelves (each once), crossed in an `m × m` grid
/// (row `i` plots column `i` up, column `j` plots column `j` across). The
/// diagonal panels have no vertical column (every row at 0), so
/// one-variable layers draw there, and are marked
/// [`PanelExtra::MatrixDiagonal`]. Panel splits are not used (a note says
/// so); color, shape, size, overlay, page and the filter work as usual. Both
/// axes of a column use the horizontal scale, so it reads the same across
/// and up.
///
/// # Errors
/// [`stat_core::StatError::InvalidArgument`] if the plan does not fit the
/// table or has fewer than two continuous columns on the axis shelves.
pub fn build_scatter_matrix_frame(
    table: &DataTable,
    state: &RowStateManager,
    plan: &EncodingPlan,
    options: &FrameOptions,
) -> Result<Frame> {
    plan.validate(table)?;
    let mut columns: Vec<String> = Vec::new();
    let mut skipped = Vec::new();
    for c in plan.x.iter().chain(&plan.y) {
        if columns.contains(c) || skipped.contains(c) {
            continue;
        }
        if is_continuous(table, c) {
            columns.push(c.clone());
        } else {
            skipped.push(c.clone());
        }
    }
    if columns.len() < 2 {
        return Err(invalid("a scatterplot matrix needs two or more continuous columns on the axis shelves"));
    }
    let mut matrix = plan.clone();
    matrix.x.clone_from(&columns);
    matrix.y = columns;
    let split = matrix.group_x.take().is_some() | matrix.group_y.take().is_some() | matrix.wrap.take().is_some();
    let options = FrameOptions { y_scale: options.x_scale, ..options.clone() };
    let mut frame = crate::frame::build_frame(table, state, &matrix, &options)?;
    for panel in &mut frame.panels {
        if panel.x.name == panel.y.name {
            panel.y = plain_axis();
            for p in &mut panel.points {
                p.y = 0.0;
            }
            panel.extra = PanelExtra::MatrixDiagonal;
        }
    }
    if !skipped.is_empty() {
        frame.notes.push(format!("Categorical columns are left out of the matrix: {}.", skipped.join(", ")));
    }
    if split {
        frame.notes.push("A scatterplot matrix does not split panels by level; use color or shape instead.".to_owned());
    }
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    #[test]
    fn multivari_slots_means_and_joins() {
        let t = DataTable::new(
            "m",
            vec![
                Column::text("batch", ["A", "A", "A", "A", "B", "B", "B", "B"].map(String::from).to_vec()),
                Column::text("part", ["1", "1", "2", "2", "1", "1", "2", "2"].map(String::from).to_vec()),
                Column::numeric("y", vec![1.0, 3.0, 5.0, 7.0, 10.0, 12.0, 14.0, 16.0]),
            ],
        )
        .unwrap();
        let mut state = RowStateManager::new(8);
        let plan = EncodingPlan { x: vec!["batch".into(), "part".into()], y: vec!["y".into()], ..Default::default() };
        let f = build_multivari_frame(&t, &state, &plan, &FrameOptions::default()).unwrap();
        let p = &f.panels[0];
        assert_eq!(p.x.levels(), ["1", "2", "", "1", "2"]);
        let PanelExtra::MultiVari(g) = &p.extra else { panic!() };
        assert_eq!(g.means[1], [[1.0, 2.0], [2.0, 6.0], [4.0, 11.0], [5.0, 15.0]]);
        assert_eq!(g.means[0], [[1.5, 4.0], [4.5, 13.0]]);
        assert_eq!(g.joins.len(), 3);
        assert_eq!(g.sections[1], (4.0, 5.0, "batch: B".to_owned()));
        assert_eq!(p.points.len(), 8);
        let layer = MultiVariElement.layer(p, &state);
        assert_eq!(layer.key.len(), 2);
        // Excluded rows stay as points but leave the means.
        state.set_excluded([0], true);
        let f = build_multivari_frame(&t, &state, &plan, &FrameOptions::default()).unwrap();
        let PanelExtra::MultiVari(g) = &f.panels[0].extra else { panic!() };
        assert_eq!(g.means[1][0], [1.0, 3.0]);
        assert!(f.panels[0].points.iter().any(|q| q.row == 0 && q.excluded));
        let bad = EncodingPlan { x: vec!["batch".into()], y: vec!["part".into()], ..Default::default() };
        assert!(build_multivari_frame(&t, &state, &bad, &FrameOptions::default()).is_err());
    }

    /// Cell and level means count each row `frequency × weight` times
    /// (they ignored both columns): a frequency matches repeating the row,
    /// and rows with a zero or missing frequency or weight leave the means.
    #[test]
    fn multivari_means_use_frequency_and_weight() {
        let text = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let t = DataTable::new(
            "m",
            vec![
                Column::text("batch", text(&["A", "A", "A", "B", "B", "B"])),
                Column::text("part", text(&["1", "1", "2", "1", "2", "2"])),
                Column::numeric("y", vec![1.0, 4.0, 6.0, 10.0, 20.0, 30.0]),
                Column::numeric("f", vec![2.0, 1.0, 3.0, 1.0, 1.0, 0.0]),
                Column::numeric("w", vec![1.0, 2.0, 1.0, 0.5, f64::NAN, 1.0]),
            ],
        )
        .unwrap();
        let state = RowStateManager::new(6);
        let base = EncodingPlan { x: vec!["batch".into(), "part".into()], y: vec!["y".into()], ..Default::default() };
        let geometry = |plan: &EncodingPlan, table: &DataTable| {
            let state = RowStateManager::new(table.n_rows());
            let f = build_multivari_frame(table, &state, plan, &FrameOptions::default()).unwrap();
            let PanelExtra::MultiVari(g) = f.panels[0].extra.clone() else { panic!() };
            g
        };
        // Frequency alone equals the table with each row repeated f times.
        let freq = EncodingPlan { freq: Some("f".into()), ..base.clone() };
        let g = geometry(&freq, &t);
        let keep = [0, 0, 1, 2, 2, 2, 3, 4];
        let pick = |c: &str| t.column(t.find(c).unwrap()).unwrap();
        let expanded = DataTable::new(
            "e",
            vec![
                Column::text("batch", keep.iter().map(|&r| pick("batch").level_text(r)).collect()),
                Column::text("part", keep.iter().map(|&r| pick("part").level_text(r)).collect()),
                Column::numeric("y", keep.iter().map(|&r| pick("y").as_numeric().unwrap()[r]).collect()),
            ],
        )
        .unwrap();
        assert_eq!(g.means, geometry(&base, &expanded).means);
        // By hand: A/1 (1·2 + 4·1)/3 = 2, A/2 = 6, B/1 = 10, B/2 = 20 (row 5 has
        // frequency 0); A = (2 + 4 + 18)/6 = 4, B = 15.
        assert_eq!(g.means[1], [[1.0, 2.0], [2.0, 6.0], [4.0, 10.0], [5.0, 20.0]]);
        assert_eq!(g.means[0], [[1.5, 4.0], [4.5, 15.0]]);
        // Frequency × weight: A/1 (1·2 + 4·2)/4 = 2.5; A = (2 + 8 + 18)/7 = 4;
        // B/2 has no row with a positive frequency and weight, so B is B/1.
        let both = EncodingPlan { weight: Some("w".into()), ..freq };
        let g = geometry(&both, &t);
        assert_eq!(g.means[1], [[1.0, 2.5], [2.0, 6.0], [4.0, 10.0]]);
        assert_eq!(g.means[0], [[1.5, 4.0], [4.5, 10.0]]);
        let f = build_multivari_frame(&t, &state, &both, &FrameOptions::default()).unwrap();
        assert_eq!(f.panels[0].points.len(), 6, "every row is still drawn");
    }

    #[test]
    fn scatter_matrix_crosses_columns_with_histograms_on_the_diagonal() {
        let t = DataTable::new(
            "m",
            vec![
                Column::numeric("a", vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]),
                Column::numeric("b", vec![2.0, 1.0, 4.0, 3.0, 6.0, f64::NAN]),
                Column::numeric("c", vec![9.0, 7.0, 8.0, 5.0, 6.0, 4.0]),
                Column::text("g", ["x", "x", "y", "y", "z", "z"].map(String::from).to_vec()),
            ],
        )
        .unwrap();
        let mut state = RowStateManager::new(6);
        let plan = EncodingPlan {
            x: vec!["a".into(), "b".into(), "g".into()],
            y: vec!["c".into(), "a".into()],
            group_x: Some("g".into()),
            ..Default::default()
        };
        let f = build_scatter_matrix_frame(&t, &state, &plan, &FrameOptions::default()).unwrap();
        assert_eq!((f.n_rows, f.n_cols, f.panels.len()), (3, 3, 9));
        assert_eq!(f.column_headers, ["a", "b", "c"]);
        assert_eq!(f.notes.len(), 2, "categorical column and panel split noted: {:?}", f.notes);
        for p in &f.panels {
            let diagonal = p.grid_row == p.grid_col;
            assert_eq!(diagonal, matches!(p.extra, PanelExtra::MatrixDiagonal));
            if diagonal {
                assert!(p.y.name.is_empty() && p.points.iter().all(|q| q.y == 0.0));
            } else {
                assert_eq!(p.y.name, ["a", "b", "c"][p.grid_row]);
            }
        }
        // Row 5 lacks b: it is missing from b's panels only.
        assert_eq!(f.panels[1].points.len(), 5);
        assert_eq!(f.panels[2].points.len(), 6);
        // Layers: the diagonal draws the histogram only, the others the
        // ellipses and points only.
        state.select([0, 1], stat_data::SelectionMode::Replace);
        let spec = crate::GraphSpec {
            encoding: plan,
            form: GraphForm::ScatterMatrix,
            ellipse: Some(crate::EllipseElement::default()),
            ..crate::GraphSpec::default()
        };
        let kinds: Vec<ElementKind> = spec.elements().iter().map(|e| e.kind()).collect();
        assert_eq!(kinds, [ElementKind::Histogram, ElementKind::Ellipse, ElementKind::Points]);
        let layers = spec.layers(&f, &state);
        for (p, l) in f.panels.iter().zip(&layers) {
            let drawn: Vec<bool> = l.iter().map(|x| !x.marks.is_empty()).collect();
            if p.grid_row == p.grid_col {
                assert_eq!(drawn, [true, false, false]);
            } else {
                assert_eq!(drawn, [false, true, true], "panel {}, {}", p.grid_row, p.grid_col);
                assert!(l.iter().all(|x| x.notes.is_empty()), "{l:?}");
            }
        }
        let one = EncodingPlan { x: vec!["a".into(), "g".into()], ..Default::default() };
        assert!(build_scatter_matrix_frame(&t, &state, &one, &FrameOptions::default()).is_err());
    }

    #[test]
    fn squarified_layout_fills_the_rectangle() {
        let values = [6.0, 6.0, 4.0, 3.0, 2.0, 2.0, 1.0];
        let rects = squarify(&values, [0.0, 0.0, 6.0, 4.0]);
        let total: f64 = values.iter().sum();
        for (v, r) in values.iter().zip(&rects) {
            let area = (r[2] - r[0]) * (r[3] - r[1]);
            assert!((area - v * 24.0 / total).abs() < 1e-9, "{r:?}");
            assert!(r[0] >= -1e-12 && r[2] <= 6.0 + 1e-9 && r[1] >= -1e-12 && r[3] <= 4.0 + 1e-9);
        }
        // Tiles do not overlap.
        for i in 0..rects.len() {
            for j in i + 1..rects.len() {
                let (a, b) = (rects[i], rects[j]);
                let overlap = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0) * (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
                assert!(overlap < 1e-9, "{a:?} {b:?}");
            }
        }
        // Squarified tiles stay reasonably square.
        let worst = rects.iter().map(|r| {
            let (w, h) = (r[2] - r[0], r[3] - r[1]);
            w.max(h) / w.min(h)
        });
        assert!(worst.fold(0.0, f64::max) < 3.0);
        assert!(squarify(&[], [0.0, 0.0, 1.0, 1.0]).is_empty());
        assert_eq!(squarify(&[1.0], [0.0, 0.0, 0.0, 1.0]), [[0.0; 4]]);
    }

    fn table() -> DataTable {
        DataTable::new(
            "t",
            vec![
                Column::text("region", ["E", "E", "W", "W", "W", "N"].map(String::from).to_vec()),
                Column::text("kind", ["a", "b", "a", "a", "b", "a"].map(String::from).to_vec()),
                Column::numeric("sales", vec![10.0, 30.0, 5.0, 5.0, 50.0, f64::NAN]),
                Column::numeric("cost", vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]),
            ],
        )
        .unwrap()
    }

    #[test]
    fn treemap_nests_and_sizes() {
        let t = table();
        let state = RowStateManager::new(6);
        let plan =
            EncodingPlan { x: vec!["region".into(), "kind".into()], y: vec!["sales".into()], ..Default::default() };
        let counts =
            build_treemap_frame(&t, &state, &plan, &FrameOptions::default(), TreemapSettings::default()).unwrap();
        let PanelExtra::Tiles(tiles) = &counts.panels[0].extra else { panic!() };
        let top: Vec<(&str, f64)> =
            tiles.iter().filter(|t| t.depth == 0).map(|t| (t.label.as_str(), t.value)).collect();
        assert_eq!(top, [("W", 3.0), ("E", 2.0), ("N", 1.0)], "largest first, by row count");
        assert_eq!(tiles.iter().filter(|t| t.depth == 1).count(), 5);
        assert_eq!(counts.panels[0].points.len(), 6);
        // By sum: N (no sales) disappears; W = 60.
        let sums = build_treemap_frame(
            &t,
            &state,
            &plan,
            &FrameOptions::default(),
            TreemapSettings { size_by_sum: true, labels: true },
        )
        .unwrap();
        let PanelExtra::Tiles(tiles) = &sums.panels[0].extra else { panic!() };
        let top: Vec<(&str, f64)> =
            tiles.iter().filter(|t| t.depth == 0).map(|t| (t.label.as_str(), t.value)).collect();
        assert_eq!(top, [("W", 60.0), ("E", 40.0)]);
        assert!(sums.notes.iter().any(|n| n.contains("left out")));
        // Children nest inside their parent.
        let w = tiles.iter().find(|t| t.depth == 0 && t.label == "W").unwrap().rect;
        for c in tiles.iter().filter(|t| t.depth == 1 && t.rows.iter().all(|r| [2, 3, 4].contains(r))) {
            assert!(c.rect[0] >= w[0] && c.rect[2] <= w[2] && c.rect[1] >= w[1] && c.rect[3] <= w[3]);
        }
        let layer = TreemapElement { labels: true }.layer(&sums.panels[0], &state);
        assert!(layer.all_finite() && !layer.regions.is_empty());
        assert!(
            build_treemap_frame(
                &t,
                &state,
                &EncodingPlan { y: vec!["sales".into()], ..Default::default() },
                &FrameOptions::default(),
                TreemapSettings::default()
            )
            .is_err()
        );
    }

    #[test]
    fn parallel_axes_scale_each_column() {
        let t = table();
        let mut state = RowStateManager::new(6);
        let plan =
            EncodingPlan { x: vec!["sales".into(), "cost".into()], y: vec!["region".into()], ..Default::default() };
        let f = build_parallel_frame(&t, &state, &plan, &FrameOptions::default()).unwrap();
        let p = &f.panels[0];
        let PanelExtra::Parallel(axes) = &p.extra else { panic!() };
        assert_eq!(axes.len(), 3);
        assert_eq!((axes[0].low, axes[0].high), (5.0, 50.0));
        assert_eq!(axes[2].levels, ["E", "N", "W"]);
        // Row 4 (sales 50, cost 5, W) sits at the top of sales and region.
        let r4: Vec<[f64; 2]> = p.points.iter().filter(|q| q.row == 4).map(|q| [q.x, q.y]).collect();
        assert_eq!(r4, [[1.0, 1.0], [2.0, 0.8], [3.0, 1.0]]);
        assert!(f.notes.iter().any(|n| n.contains("missing")), "row 5 lacks sales");
        state.select([4], stat_data::SelectionMode::Replace);
        let layer = ParallelElement.layer(p, &state);
        assert!(layer.all_finite());
        assert!(layer.marks.iter().any(|m| matches!(m, Mark::Segments { color, .. } if *color == palette::SELECTED)));
        assert!(
            build_parallel_frame(
                &t,
                &state,
                &EncodingPlan { x: vec!["sales".into()], ..Default::default() },
                &FrameOptions::default()
            )
            .is_err()
        );
    }

    /// A top-level tile too thin to hold its sub-tiles used to leave its rows
    /// without a point, so they could not be clicked or box-selected.
    #[test]
    fn treemap_rows_in_thin_tiles_get_points() {
        // One big group of 10 000 rows and a tiny one of ten sub-levels.
        let n_big = 10_000;
        let mut outer: Vec<String> = vec!["big".to_owned(); n_big];
        let mut inner: Vec<String> = (0..n_big).map(|i| format!("b{}", i % 3)).collect();
        for i in 0..10 {
            outer.push("tiny".to_owned());
            inner.push(format!("t{i}"));
        }
        let n = outer.len();
        let t = DataTable::new("t", vec![Column::text("outer", outer), Column::text("inner", inner)]).unwrap();
        let state = RowStateManager::new(n);
        let plan = EncodingPlan { x: vec!["outer".into(), "inner".into()], ..Default::default() };
        let frame =
            build_treemap_frame(&t, &state, &plan, &FrameOptions::default(), TreemapSettings::default()).unwrap();
        let PanelExtra::Tiles(tiles) = &frame.panels[0].extra else { panic!() };
        let tiny = tiles.iter().find(|t| t.label == "tiny").unwrap();
        assert!(!tiles.iter().any(|t| t.depth == 1 && t.label.starts_with('t')), "the tiny tile is not subdivided");
        assert_eq!(frame.panels[0].points.len(), n, "every row has a point");
        let c = [0.5 * (tiny.rect[0] + tiny.rect[2]), 0.5 * (tiny.rect[1] + tiny.rect[3])];
        let in_tiny: Vec<_> = frame.panels[0].points.iter().filter(|p| p.row >= n_big).collect();
        assert_eq!(in_tiny.len(), 10);
        assert!(in_tiny.iter().all(|p| (p.x - c[0]).abs() < 1e-12 && (p.y - c[1]).abs() < 1e-12));
    }
}
