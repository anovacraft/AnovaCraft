//! From data and an [`EncodingPlan`] to panels: each row's position on the
//! axes (on their scales), its marker style, its subgroup, and its panel in
//! a grid of small multiples (trellis displays: Becker, Cleveland & Shyu,
//! "The Visual Design and Control of Trellis Display", *JCGS* 5(2), 1996).
//!
//! * Several horizontal columns give several panel columns, several vertical
//!   columns several panel rows (each panel pairs one of each).
//! * `group_x` / `group_y` split every panel column / row by level; `wrap`
//!   instead lays one panel per level into a near-square grid.
//! * A categorical axis places level `k` (in natural order) at position
//!   `k + 1`; a missing horizontal axis is one unnamed category, a missing
//!   vertical axis a numeric axis with every row at 0.
//! * `page` shows one level at a time (the frame's current page); axes and
//!   styles are shared by every page so pages compare directly.
//! * Hidden rows are left out; excluded rows are kept (drawn differently and
//!   left out of fits); values outside an axis scale's domain are dropped and
//!   counted in the frame's notes.
//! * The graph's own [`LocalFilter`] can drop rows outside it or keep them
//!   drawn but out of the analyses.
//! * Frequency and weight columns travel with each point for the elements'
//!   statistics.

use std::collections::BTreeMap;

use crate::encoding::{EncodingPlan, invalid, is_continuous};
use crate::filter::LocalFilter;
use crate::layer::{MarkShape, Rgba};
use crate::palette::{self, DEFAULT_POINT};
use crate::scale::AxisScale;
use stat_core::Result;
use stat_data::{DataTable, RowStateManager};
use stat_linear::natural_order;

/// What an axis shows.
#[derive(Debug, Clone, PartialEq)]
pub enum AxisKind {
    /// Numbers on the axis's scale.
    Continuous,
    /// Levels at positions 1, 2, …, in this order.
    Categorical {
        /// Level labels.
        levels: Vec<String>,
    },
}

/// One axis of a panel.
#[derive(Debug, Clone, PartialEq)]
pub struct AxisSpec {
    /// Column name (empty for a missing axis).
    pub name: String,
    /// Continuous or categorical.
    pub kind: AxisKind,
    /// Scale of a continuous axis.
    pub scale: AxisScale,
    /// How a date-time column on a continuous axis shows its values (`None`
    /// for other axes): ticks and labels then follow the calendar
    /// ([`crate::time_scale`]), and the scale is always linear.
    pub time: Option<stat_data::TemporalDisplay>,
}

impl AxisSpec {
    /// `true` for a categorical axis.
    #[must_use]
    pub fn is_categorical(&self) -> bool {
        matches!(self.kind, AxisKind::Categorical { .. })
    }

    /// Level labels of a categorical axis (empty otherwise).
    #[must_use]
    pub fn levels(&self) -> &[String] {
        match &self.kind {
            AxisKind::Categorical { levels } => levels,
            AxisKind::Continuous => &[],
        }
    }

    /// Axis title: the column name, with the scale for a transformed axis.
    #[must_use]
    pub fn title(&self) -> String {
        match (self.is_categorical(), self.scale) {
            (false, AxisScale::Log10) => format!("{} (log₁₀ scale)", self.name),
            (false, AxisScale::Ln) => format!("{} (ln scale)", self.name),
            (false, AxisScale::Sqrt) => format!("{} (square-root scale)", self.name),
            (false, AxisScale::Logit) => format!("{} (logit scale)", self.name),
            (false, AxisScale::Reciprocal) => format!("{} (reciprocal scale)", self.name),
            _ => self.name.clone(),
        }
    }
}

/// A row drawn in a panel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelPoint {
    /// Dataset row.
    pub row: usize,
    /// Horizontal position (scaled; a categorical position is moved within
    /// its slot when subgroups are dodged).
    pub x: f64,
    /// Vertical position (scaled).
    pub y: f64,
    /// Subgroup index into [`Panel::groups`].
    pub group: usize,
    /// Marker color.
    pub color: Rgba,
    /// Marker shape.
    pub shape: MarkShape,
    /// Marker radius in screen points.
    pub radius: f32,
    /// Excluded from analyses (drawn as a gray cross, left out of fits).
    pub excluded: bool,
    /// Outside the graph's filter and left out of its analyses (drawn hollow).
    pub filtered: bool,
    /// Frequency (1 without a frequency column; 0 when missing).
    pub freq: f64,
    /// Weight (1 without a weight column; 0 when missing).
    pub weight: f64,
    /// Value of a continuous color column (NaN otherwise), for heat maps.
    pub color_value: f64,
}

impl PanelPoint {
    /// `true` if the row takes part in fits, smoothers and summaries: not
    /// excluded, inside the filter, and with positive frequency and weight.
    #[must_use]
    pub fn in_analysis(&self) -> bool {
        !self.excluded && !self.filtered && self.freq > 0.0 && self.weight > 0.0
    }
}

/// Name, color and shape of a subgroup.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupStyle {
    /// Level label (empty for the single group of an unsplit panel).
    pub label: String,
    /// Color of its lines and boxes.
    pub color: Rgba,
    /// Marker shape.
    pub shape: MarkShape,
}

/// One panel of the grid.
#[derive(Debug, Clone, PartialEq)]
pub struct Panel {
    /// Grid row (0 at the top).
    pub grid_row: usize,
    /// Grid column.
    pub grid_col: usize,
    /// Title for a wrapped panel (grid panels are labelled by the headers).
    pub title: Option<String>,
    /// Horizontal axis.
    pub x: AxisSpec,
    /// Vertical axis.
    pub y: AxisSpec,
    /// Rows drawn here.
    pub points: Vec<PanelPoint>,
    /// Subgroups (always at least one).
    pub groups: Vec<GroupStyle>,
    /// Half-width of each subgroup's slot on a categorical horizontal axis.
    pub slot_half_width: f64,
    /// Geometry of whole-panel chart forms (treemap tiles, parallel axes).
    pub extra: PanelExtra,
    /// Point labels of the panel's rows (with a label column).
    pub labels: std::collections::BTreeMap<usize, String>,
    /// Extent of every panel's points on each axis (`[x, y]`, drawn units;
    /// the same in every panel of a frame), which axis breaks with segment
    /// shares or scales are laid out on ([`crate::axis_breaks`]).
    pub data_extent: [Option<(f64, f64)>; 2],
}

/// A rectangle of a treemap: `[x0, y0, x1, y1]` in the unit square.
#[derive(Debug, Clone, PartialEq)]
pub struct Tile {
    /// `[x0, y0, x1, y1]`.
    pub rect: [f64; 4],
    /// Label (the level, e.g. `East`).
    pub label: String,
    /// Nesting depth (0 for the top level).
    pub depth: usize,
    /// Count or sum the area stands for.
    pub value: f64,
    /// Rows in the tile.
    pub rows: Vec<usize>,
    /// Fill color.
    pub color: Rgba,
}

/// One vertical axis of a parallel-coordinates panel.
#[derive(Debug, Clone, PartialEq)]
pub struct ParallelAxis {
    /// Column name.
    pub name: String,
    /// Data value drawn at 0 (bottom).
    pub low: f64,
    /// Data value drawn at 1 (top).
    pub high: f64,
    /// Levels of a categorical column, evenly spaced from bottom to top.
    pub levels: Vec<String>,
}

/// Extra geometry a panel carries for whole-panel chart forms.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum PanelExtra {
    /// An ordinary panel.
    #[default]
    None,
    /// Treemap tiles, parents before children.
    Tiles(Vec<Tile>),
    /// Parallel axes at x = 1, 2, ….
    Parallel(Vec<ParallelAxis>),
    /// Means, joins and sections of a multi-vari chart.
    MultiVari(crate::forms::MultiVariGeometry),
    /// A diagonal panel of a scatterplot matrix (one variable across, no
    /// vertical column).
    MatrixDiagonal,
    /// Cells, dendrograms and labels of a clustered heat map.
    ClusterMap(crate::cluster_map::ClusterMapGeometry),
    /// Groups, mean differences and intervals of an estimation plot.
    Estimation(crate::estimation::EstimationGeometry),
    /// Nodes and links of a Sankey diagram, chord diagram or network.
    Flows(crate::flows::FlowGeometry),
    /// Slices of a pie or donut chart.
    Pie(crate::forms::PieGeometry),
    /// Spokes and polygons of a radar chart.
    Radar(crate::forms::RadarGeometry),
    /// Rings and spokes of a polar chart.
    Polar(crate::forms::PolarGeometry),
    /// Stacked sector bars of a wind rose.
    WindRose(crate::forms::WindRoseGeometry),
    /// Triangle, gridlines and edge labels of a ternary plot.
    Ternary(crate::forms::TernaryGeometry),
    /// Floating bars of a waterfall (bridge) chart.
    Waterfall(crate::forms::WaterfallGeometry),
    /// Stacked kernel densities of a ridgeline plot.
    Ridgeline(crate::forms::RidgelineGeometry),
    /// Arrows of a vector-field plot.
    Vectors(crate::forms::VectorGeometry),
}

/// What the color channel shows.
#[derive(Debug, Clone, PartialEq)]
pub enum ColorKey {
    /// One color (and shape) per level.
    Levels {
        /// Column name.
        title: String,
        /// `(level, color, shape)`.
        entries: Vec<(String, Rgba, MarkShape)>,
    },
    /// A sequential ramp from `min` to `max`.
    Gradient {
        /// Column name.
        title: String,
        /// Value at the start of the ramp.
        min: f64,
        /// Value at the end.
        max: f64,
    },
}

/// The key of a graph.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Legend {
    /// Color mapping.
    pub color: Option<ColorKey>,
    /// Shape mapping: column and `(level, shape)`.
    pub shape: Option<(String, Vec<(String, MarkShape)>)>,
    /// Size mapping: column, smallest and largest value.
    pub size: Option<(String, f64, f64)>,
    /// Subgroups (when they are not already shown by color).
    pub groups: Option<(String, Vec<GroupStyle>)>,
}

/// Every panel of a graph with its layout and key.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// Panels, in row-major grid order.
    pub panels: Vec<Panel>,
    /// Grid rows.
    pub n_rows: usize,
    /// Grid columns.
    pub n_cols: usize,
    /// Header above each grid column (empty strings when not split).
    pub column_headers: Vec<String>,
    /// Header beside each grid row.
    pub row_headers: Vec<String>,
    /// The key.
    pub legend: Legend,
    /// Notes about rows or settings that could not be shown.
    pub notes: Vec<String>,
    /// Levels of the page column (empty without one).
    pub pages: Vec<String>,
    /// The page shown (an index into `pages`).
    pub page: usize,
}

impl Frame {
    /// Title of the page shown, e.g. `Region: East` (`None` without pages).
    #[must_use]
    pub fn page_title(&self, plan: &EncodingPlan) -> Option<String> {
        let level = self.pages.get(self.page)?;
        Some(match &plan.page {
            Some(c) => format!("{c}: {level}"),
            None => level.clone(),
        })
    }
}

/// Axis scales, page and row filter of a frame.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FrameOptions {
    /// Scale of continuous horizontal axes.
    pub x_scale: AxisScale,
    /// Scale of continuous vertical axes.
    pub y_scale: AxisScale,
    /// Page shown when the plan has a page column (clamped to the pages).
    pub page: usize,
    /// The graph's own row filter.
    pub filter: LocalFilter,
    /// Marker size, default color and shape, and per-level colors and shapes.
    pub style: MarkerStyle,
}

/// Default marker radius in screen points.
pub const DEFAULT_RADIUS: f32 = 6.5;

/// Marker appearance chosen by the user: size, the color and shape of
/// ungrouped markers (and lines), and colors, shapes and sizes for particular
/// levels of the color, subgroup and shape columns (keyed by `column` and
/// `level`, see [`MarkerStyle::key`]). Levels without a choice take the
/// palette's color and shape for their position. Any of it can be set again
/// for one panel ([`MarkerStyle::panels`]), overriding the graph-wide choice
/// there.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MarkerStyle {
    /// Marker radius in points (`None`: the default).
    pub size: Option<f32>,
    /// Color of ungrouped markers and their lines.
    pub color: Option<Rgba>,
    /// Shape of ungrouped markers.
    pub shape: Option<MarkShape>,
    /// Colors chosen for levels.
    pub level_colors: std::collections::BTreeMap<String, Rgba>,
    /// Shapes chosen for levels.
    pub level_shapes: std::collections::BTreeMap<String, MarkShape>,
    /// Marker radii chosen for levels.
    pub level_sizes: std::collections::BTreeMap<String, f32>,
    /// Choices for single panels (by panel index), over the graph-wide ones.
    /// Their own `panels` are ignored.
    pub panels: std::collections::BTreeMap<usize, MarkerStyle>,
}

impl MarkerStyle {
    /// The key of `level` of `column` in the per-level maps.
    #[must_use]
    pub fn key(column: &str, level: &str) -> String {
        format!("{column}\u{1f}{level}")
    }

    /// Marker radius in points.
    #[must_use]
    pub fn radius(&self) -> f32 {
        self.size.filter(|r| r.is_finite() && *r > 0.0).unwrap_or(DEFAULT_RADIUS).clamp(1.0, 20.0)
    }

    /// Color and shape of the `k`-th level `level` of `column`.
    #[must_use]
    pub fn level(&self, column: &str, level: &str, k: usize) -> (Rgba, MarkShape) {
        let (c, s) = palette::level_style(k);
        let key = Self::key(column, level);
        (self.level_colors.get(&key).copied().unwrap_or(c), self.level_shapes.get(&key).copied().unwrap_or(s))
    }

    /// The radius chosen for `level` of `column`, if any.
    #[must_use]
    pub fn level_size(&self, column: &str, level: &str) -> Option<f32> {
        self.level_sizes
            .get(&Self::key(column, level))
            .copied()
            .filter(|r| r.is_finite() && *r > 0.0)
            .map(|r| r.clamp(1.0, 20.0))
    }

    /// The style in effect in panel `panel`: its own choices over the
    /// graph-wide ones.
    #[must_use]
    pub fn for_panel(&self, panel: usize) -> MarkerStyle {
        let mut out = MarkerStyle { panels: std::collections::BTreeMap::new(), ..self.clone() };
        if let Some(p) = self.panels.get(&panel) {
            out.size = p.size.or(self.size);
            out.color = p.color.or(self.color);
            out.shape = p.shape.or(self.shape);
            out.level_colors.extend(p.level_colors.iter().map(|(k, v)| (k.clone(), *v)));
            out.level_shapes.extend(p.level_shapes.iter().map(|(k, v)| (k.clone(), *v)));
            out.level_sizes.extend(p.level_sizes.iter().map(|(k, v)| (k.clone(), *v)));
        }
        out
    }

    /// `true` when nothing is chosen.
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == MarkerStyle::default()
    }

    /// Shape of the `k`-th level of a shape column.
    #[must_use]
    pub fn shape_level(&self, column: &str, level: &str, k: usize) -> MarkShape {
        self.level_shapes
            .get(&Self::key(column, level))
            .copied()
            .unwrap_or(MarkShape::CYCLE[k % MarkShape::CYCLE.len()])
    }
}

/// A panel's own marker colors, shapes and radii (by row) and subgroup styles.
type PanelStyles = (Vec<Rgba>, Vec<MarkShape>, Vec<f32>, Vec<GroupStyle>);

/// Largest number of panels drawn (levels beyond this are left out).
pub const MAX_PANELS: usize = 64;

/// A column read as levels: the level of each row (`None` when missing) and
/// the levels in natural order.
pub(crate) fn levels_of(table: &DataTable, column: &str, rows: &[usize]) -> (Vec<Option<usize>>, Vec<String>) {
    let Some(col) = table.find(column).and_then(|i| table.column(i)) else {
        return (vec![None; table.n_rows()], Vec::new());
    };
    let labels: Vec<String> = (0..table.n_rows()).map(|r| col.level_text(r)).collect();
    let mut levels: Vec<String> = rows.iter().map(|&r| labels[r].clone()).filter(|l| !l.trim().is_empty()).collect();
    levels.sort_by(|a, b| natural_order(a, b));
    levels.dedup();
    let codes = labels
        .iter()
        .map(|l| if l.trim().is_empty() { None } else { levels.binary_search_by(|v| natural_order(v, l)).ok() })
        .collect();
    (codes, levels)
}

/// Positions of one axis variable for every row.
struct AxisData {
    spec: AxisSpec,
    /// Position per row (`None`: missing or outside the scale's domain).
    positions: Vec<Option<f64>>,
    /// Rows dropped for the scale's domain.
    out_of_domain: usize,
}

fn axis_data(table: &DataTable, column: Option<&str>, scale: AxisScale, rows: &[usize], vertical: bool) -> AxisData {
    let n = table.n_rows();
    match column {
        // No vertical column: a numeric axis with every row at 0, so bars
        // and counts have room to rise.
        None if vertical => AxisData {
            spec: AxisSpec { name: String::new(), kind: AxisKind::Continuous, scale: AxisScale::Linear, time: None },
            positions: vec![Some(0.0); n],
            out_of_domain: 0,
        },
        None => AxisData {
            spec: AxisSpec {
                name: String::new(),
                kind: AxisKind::Categorical { levels: vec![String::new()] },
                scale,
                time: None,
            },
            positions: vec![Some(1.0); n],
            out_of_domain: 0,
        },
        Some(name) if is_continuous(table, name) => {
            let column = table.find(name).and_then(|i| table.column(i));
            let values = column.and_then(|c| c.as_numeric()).unwrap_or(&[]);
            // Dates are always on a linear scale: a log of seconds since 1970 means nothing.
            let time = column.and_then(stat_data::Column::temporal);
            let scale = if time.is_some() { AxisScale::Linear } else { scale };
            let mut out_of_domain = 0;
            let positions = (0..n)
                .map(|r| {
                    let v = values.get(r).copied().unwrap_or(f64::NAN);
                    let p = scale.forward(v);
                    if p.is_none() && v.is_finite() && rows.binary_search(&r).is_ok() {
                        out_of_domain += 1;
                    }
                    p
                })
                .collect();
            AxisData {
                spec: AxisSpec { name: name.to_owned(), kind: AxisKind::Continuous, scale, time },
                positions,
                out_of_domain,
            }
        }
        Some(name) => {
            let (codes, levels) = levels_of(table, name, rows);
            AxisData {
                spec: AxisSpec { name: name.to_owned(), kind: AxisKind::Categorical { levels }, scale, time: None },
                positions: codes.iter().map(|c| c.map(|k| k as f64 + 1.0)).collect(),
                out_of_domain: 0,
            }
        }
    }
}

/// The rows a graph shows and how each takes part.
pub(crate) struct RowSet {
    /// Rows shown on any page (not hidden, inside the filter if it hides).
    pub rows: Vec<usize>,
    /// Rows on the current page.
    pub page_rows: Vec<usize>,
    /// Per table row: outside the filter and left out of analyses.
    pub filtered: Vec<bool>,
    /// Per table row: frequency (0 when missing).
    pub freq: Vec<f64>,
    /// Per table row: weight (0 when missing).
    pub weight: Vec<f64>,
    /// Page levels.
    pub pages: Vec<String>,
    /// Current page.
    pub page: usize,
    /// Per table row: passes the filter.
    pub passes: Vec<bool>,
}

/// Apply hiding, the graph's filter and the page to the table's rows.
pub(crate) fn select_rows(
    table: &DataTable,
    state: &RowStateManager,
    plan: &EncodingPlan,
    options: &FrameOptions,
) -> RowSet {
    let n = table.n_rows();
    // The filter: rows outside it are dropped ("show only") or kept but left
    // out of the analyses.
    let filter = &options.filter;
    let passes = if filter.is_active() { filter.matches(table) } else { vec![true; n] };
    let rows: Vec<usize> = (0..n)
        .filter(|&r| r >= state.len() || !state.is_hidden(r))
        .filter(|&r| passes[r] || !filter.show_only)
        .collect();
    let filtered: Vec<bool> = passes.iter().map(|&p| !p && filter.include_only).collect();
    // Pages: levels over every shown row, so axes match across pages.
    let (pages, page, page_rows) = match &plan.page {
        Some(c) => {
            let (codes, levels) = levels_of(table, c, &rows);
            let page = options.page.min(levels.len().saturating_sub(1));
            let keep: Vec<usize> = rows.iter().copied().filter(|&r| codes[r] == Some(page)).collect();
            (levels, page, keep)
        }
        None => (Vec::new(), 0, rows.clone()),
    };
    // Frequency and weight of every row (0 when missing or negative).
    let numeric = |c: &Option<String>| -> Vec<f64> {
        match c.as_deref().and_then(|c| table.find(c)).and_then(|i| table.column(i)).and_then(|c| c.as_numeric()) {
            Some(v) => (0..n).map(|r| v.get(r).copied().filter(|x| x.is_finite() && *x > 0.0).unwrap_or(0.0)).collect(),
            None => vec![1.0; n],
        }
    };
    let (freq, weight) = (numeric(&plan.freq), numeric(&plan.weight));
    RowSet { rows, page_rows, filtered, freq, weight, pages, page, passes }
}

/// Marker style of every row, the key, and the subgroups.
pub(crate) struct Styles {
    pub color: Vec<Rgba>,
    pub color_value: Vec<f64>,
    pub shape: Vec<MarkShape>,
    pub radius: Vec<f32>,
    pub legend: Legend,
    pub group_codes: Vec<Option<usize>>,
    pub groups: Vec<GroupStyle>,
}

/// Colors, shapes, sizes and subgroups of `rows` from the plan. With no
/// colour, shape or subgroup column (and no fixed marker colour), rows draw
/// their own row colours and markers, keyed by the column they came from.
pub(crate) fn row_styles(
    table: &DataTable,
    state: &RowStateManager,
    plan: &EncodingPlan,
    rows: &[usize],
    style: &MarkerStyle,
) -> Styles {
    let n = table.n_rows();
    let n_rows = n;
    let (base_color, base_shape) = (style.color.unwrap_or(DEFAULT_POINT), style.shape.unwrap_or(MarkShape::Circle));
    let mut color = vec![base_color; n_rows];
    let mut color_value = vec![f64::NAN; n_rows];
    let mut shape = vec![base_shape; n_rows];
    let base_radius = style.radius();
    let mut radius = vec![base_radius; n_rows];
    // Rows whose size comes from a level choice (color, then subgroup, then shape level).
    let mut sized = vec![false; n_rows];
    let mut legend = Legend::default();
    if plan.color.is_none() && plan.shape.is_none() && plan.overlay.is_none() && style.color.is_none() {
        for &r in rows.iter().filter(|&&r| r < state.len()) {
            if let Some(c) = state.color(r) {
                color[r] = crate::row_styles::rgba(c);
            }
            if let Some(m) = state.marker(r) {
                shape[r] = crate::row_styles::mark_shape(m);
            }
        }
        legend.color = crate::row_styles::row_style_key(table, state, rows);
    }
    let mut color_levels: Option<(Vec<Option<usize>>, Vec<String>)> = None;
    if let Some(c) = &plan.color {
        if is_continuous(table, c) {
            let values = table.find(c).and_then(|i| table.column(i)).and_then(|col| col.as_numeric()).unwrap_or(&[]);
            let (lo, hi) = rows
                .iter()
                .filter_map(|&r| values.get(r).copied().filter(|v| v.is_finite()))
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)));
            if lo.is_finite() {
                for &r in rows {
                    let v = values.get(r).copied().unwrap_or(f64::NAN);
                    if v.is_finite() {
                        color[r] = palette::ramp(if hi > lo { (v - lo) / (hi - lo) } else { 0.5 });
                    }
                }
                legend.color = Some(ColorKey::Gradient { title: c.clone(), min: lo, max: hi });
                for &r in rows {
                    color_value[r] = values.get(r).copied().unwrap_or(f64::NAN);
                }
            }
        } else {
            let (codes, levels) = levels_of(table, c, rows);
            for &r in rows {
                if let Some(k) = codes[r] {
                    let (col, shp) = style.level(c, &levels[k], k);
                    color[r] = col;
                    if plan.shape.is_none() {
                        shape[r] = shp;
                    }
                    if let Some(size) = style.level_size(c, &levels[k]) {
                        (radius[r], sized[r]) = (size, true);
                    }
                }
            }
            let entries = levels
                .iter()
                .enumerate()
                .map(|(k, l)| {
                    let (col, shp) = style.level(c, l, k);
                    (l.clone(), col, if plan.shape.is_none() { shp } else { base_shape })
                })
                .collect();
            legend.color = Some(ColorKey::Levels { title: c.clone(), entries });
            color_levels = Some((codes, levels));
        }
    }
    if let (Some(o), None) = (&plan.overlay, &color_levels) {
        let (codes, levels) = levels_of(table, o, rows);
        for &r in rows {
            if let Some(size) = codes[r].and_then(|k| style.level_size(o, &levels[k])) {
                (radius[r], sized[r]) = (size, true);
            }
        }
    }
    if let Some(s) = &plan.shape {
        let (codes, levels) = levels_of(table, s, rows);
        for &r in rows {
            if let Some(k) = codes[r] {
                shape[r] = style.shape_level(s, &levels[k], k);
                // Shape levels are the only groups: they may have colors too.
                if plan.color.is_none() && plan.overlay.is_none() {
                    if let Some(c) = style.level_colors.get(&MarkerStyle::key(s, &levels[k])) {
                        color[r] = *c;
                    }
                }
                if let Some(size) = style.level_size(s, &levels[k]).filter(|_| !sized[r]) {
                    radius[r] = size;
                }
            }
        }
        legend.shape = Some((
            s.clone(),
            levels.iter().enumerate().map(|(k, l)| (l.clone(), style.shape_level(s, l, k))).collect(),
        ));
    }
    if let Some(s) = &plan.size {
        let values = table.find(s).and_then(|i| table.column(i)).and_then(|c| c.as_numeric()).unwrap_or(&[]);
        let (lo, hi) = rows
            .iter()
            .filter_map(|&r| values.get(r).copied().filter(|v| v.is_finite()))
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)));
        if lo.is_finite() {
            for &r in rows {
                let v = values.get(r).copied().unwrap_or(f64::NAN);
                if v.is_finite() {
                    // Area grows with the value's position in its range: from
                    // half to one and a half times the marker size.
                    let t = if hi > lo { ((v - lo) / (hi - lo)).sqrt() } else { 0.5 };
                    radius[r] *= (0.5 + t) as f32;
                }
            }
            legend.size = Some((s.clone(), lo, hi));
        }
    }

    // Subgroups: the overlay column, or else categorical color levels.
    let (group_codes, groups, group_title): (Vec<Option<usize>>, Vec<GroupStyle>, Option<String>) =
        match (&plan.overlay, &color_levels) {
            (Some(o), _) => {
                let (codes, levels) = levels_of(table, o, rows);
                let styles = levels
                    .iter()
                    .enumerate()
                    .map(|(k, l)| {
                        let (c, s) = style.level(o, l, k);
                        GroupStyle { label: l.clone(), color: c, shape: s }
                    })
                    .collect();
                // Points take the subgroup's style unless color is mapped.
                if plan.color.is_none() {
                    for &r in rows {
                        if let Some(k) = codes[r] {
                            let (c, s) = style.level(o, &levels[k], k);
                            color[r] = c;
                            if plan.shape.is_none() {
                                shape[r] = s;
                            }
                        }
                    }
                }
                (codes, styles, Some(o.clone()))
            }
            (None, Some((codes, levels))) => {
                let styles = levels
                    .iter()
                    .enumerate()
                    .map(|(k, l)| {
                        let (c, s) = style.level(plan.color.as_deref().unwrap_or(""), l, k);
                        GroupStyle { label: l.clone(), color: c, shape: s }
                    })
                    .collect();
                (codes.clone(), styles, None)
            }
            (None, None) => (
                vec![Some(0); n_rows],
                vec![GroupStyle { label: String::new(), color: base_color, shape: base_shape }],
                None,
            ),
        };
    let groups = if groups.is_empty() {
        vec![GroupStyle { label: String::new(), color: base_color, shape: base_shape }]
    } else {
        groups
    };
    // Subgroups already keyed by color need no second key.
    if let Some(t) = group_title.filter(|t| plan.color.as_deref() != Some(t.as_str())) {
        legend.groups = Some((t, groups.clone()));
    }
    Styles { color, color_value, shape, radius, legend, group_codes, groups }
}

/// The label-column text of each of `points`' rows (empty without a label
/// column; blank labels left out).
pub(crate) fn point_labels(
    table: &DataTable,
    plan: &EncodingPlan,
    points: &[PanelPoint],
) -> std::collections::BTreeMap<usize, String> {
    match plan.label.as_deref().and_then(|c| table.find(c)).and_then(|i| table.column(i)) {
        Some(col) => {
            points.iter().map(|p| (p.row, col.level_text(p.row))).filter(|(_, t)| !t.trim().is_empty()).collect()
        }
        None => std::collections::BTreeMap::new(),
    }
}

/// The note about rows outside the graph's filter, if any.
pub(crate) fn filter_note(state: &RowStateManager, options: &FrameOptions, passes: &[bool]) -> Option<String> {
    let filter = &options.filter;
    if !filter.is_active() {
        return None;
    }
    let outside = passes.iter().enumerate().filter(|&(r, p)| !p && (r >= state.len() || !state.is_hidden(r))).count();
    if outside == 0 {
        return None;
    }
    let fate = match (filter.show_only, filter.include_only) {
        (true, _) => "not shown",
        (false, true) => "drawn hollow and left out of fits and summaries",
        (false, false) => "shown and analysed as usual",
    };
    Some(format!("{outside} row(s) outside the filter ({}) are {fate}.", filter.describe()))
}

/// Lay out the panels of `plan` for `table`, skipping hidden rows.
///
/// # Errors
/// [`stat_core::StatError::InvalidArgument`] if the plan does not fit the
/// table ([`EncodingPlan::validate`]) or has no position column.
pub fn build_frame(
    table: &DataTable,
    state: &RowStateManager,
    plan: &EncodingPlan,
    options: &FrameOptions,
) -> Result<Frame> {
    plan.validate(table)?;
    if plan.is_empty() {
        return Err(invalid("assign a column to the horizontal or vertical axis"));
    }
    let mut notes = Vec::new();
    let RowSet { rows, page_rows, filtered, freq, weight, pages, page, passes } =
        select_rows(table, state, plan, options);

    // Position variables (a missing axis is one unnamed category).
    let xs: Vec<Option<&str>> =
        if plan.x.is_empty() { vec![None] } else { plan.x.iter().map(|s| Some(s.as_str())).collect() };
    let ys: Vec<Option<&str>> =
        if plan.y.is_empty() { vec![None] } else { plan.y.iter().map(|s| Some(s.as_str())).collect() };
    let (xs, ys) = if plan.wrap.is_some() && (xs.len() > 1 || ys.len() > 1) {
        notes.push("Wrapped panels show the first horizontal and vertical column only.".to_owned());
        (vec![xs[0]], vec![ys[0]])
    } else {
        (xs, ys)
    };
    let x_data: Vec<AxisData> = xs.iter().map(|c| axis_data(table, *c, options.x_scale, &rows, false)).collect();
    let y_data: Vec<AxisData> = ys.iter().map(|c| axis_data(table, *c, options.y_scale, &rows, true)).collect();
    for d in x_data.iter().chain(&y_data) {
        if d.out_of_domain > 0 {
            notes.push(format!(
                "{} value(s) of {} are not shown: the {} scale takes {} only.",
                d.out_of_domain,
                d.spec.name,
                d.spec.scale.label().to_lowercase(),
                d.spec.scale.domain().unwrap_or("all values")
            ));
        }
    }

    // Panel splits.
    let split = |c: &Option<String>| c.as_deref().map(|name| levels_of(table, name, &rows));
    let (gx, gy, wrap) = (split(&plan.group_x), split(&plan.group_y), split(&plan.wrap));
    let count = |s: &Option<(Vec<Option<usize>>, Vec<String>)>| s.as_ref().map_or(1, |(_, l)| l.len().max(1));

    // Marker styles and subgroups.
    let Styles { color, color_value, shape, radius, legend, group_codes, groups } =
        row_styles(table, state, plan, &rows, &options.style);
    let n_groups = groups.len();
    // Panels with their own marker choices: (colors, shapes, radii, subgroup styles).
    let panel_styles: BTreeMap<usize, PanelStyles> = options
        .style
        .panels
        .keys()
        .map(|&i| {
            let s = row_styles(table, state, plan, &rows, &options.style.for_panel(i));
            (i, (s.color, s.shape, s.radius, s.groups))
        })
        .collect();

    // Grid geometry.
    let (n_gx, n_gy) = (count(&gx), count(&gy));
    // Each cell: (grid row, grid col, x variable, y variable, wrap level).
    type Cell = (usize, usize, usize, usize, usize);
    let (grid_rows, grid_cols, cells): (usize, usize, Vec<Cell>) = match &wrap {
        Some((_, levels)) => {
            let k = levels.len().clamp(1, MAX_PANELS);
            if levels.len() > MAX_PANELS {
                notes.push(format!("Only the first {MAX_PANELS} of {} panels are shown.", levels.len()));
            }
            let cols = (k as f64).sqrt().ceil() as usize;
            let rows_n = k.div_ceil(cols);
            // (grid row, grid col, x var, y var, wrap level)
            (rows_n, cols, (0..k).map(|l| (l / cols, l % cols, 0, 0, l)).collect())
        }
        None => {
            let rows_n = ys.len() * n_gy;
            let cols = xs.len() * n_gx;
            if rows_n * cols > MAX_PANELS {
                return Err(invalid(format!(
                    "this would draw {} panels; at most {MAX_PANELS} are supported",
                    rows_n * cols
                )));
            }
            let mut cells = Vec::new();
            for gr in 0..rows_n {
                for gc in 0..cols {
                    cells.push((gr, gc, gc / n_gx, gr / n_gy, 0));
                }
            }
            (rows_n, cols, cells)
        }
    };
    // "Column: level", with the axis variable in front when there are several.
    let header = |names: &[Option<&str>],
                  split: &Option<(Vec<Option<usize>>, Vec<String>)>,
                  split_name: Option<&str>,
                  count: usize,
                  i: usize| {
        let var = names[i / count].unwrap_or("");
        let level = split
            .as_ref()
            .and_then(|(_, l)| l.get(i % count))
            .map(|l| split_name.map_or_else(|| l.clone(), |n| format!("{n}: {l}")));
        match level {
            Some(l) if names.len() > 1 && !var.is_empty() => format!("{var} · {l}"),
            Some(l) => l,
            None if names.len() > 1 => var.to_owned(),
            None => String::new(),
        }
    };
    let (column_headers, row_headers) = if wrap.is_some() {
        (vec![String::new(); grid_cols], vec![String::new(); grid_rows])
    } else {
        (
            (0..grid_cols).map(|c| header(&xs, &gx, plan.group_x.as_deref(), n_gx, c)).collect(),
            (0..grid_rows).map(|r| header(&ys, &gy, plan.group_y.as_deref(), n_gy, r)).collect(),
        )
    };

    // Panels.
    let mut panels = Vec::with_capacity(cells.len());
    for (grid_row, grid_col, xi, yi, wrap_level) in cells {
        let own = panel_styles.get(&panels.len());
        let (color, shape, radius, groups) = match own {
            Some((c, s, r, g)) => (c, s, r, g),
            None => (&color, &shape, &radius, &groups),
        };
        let (xd, yd) = (&x_data[xi], &y_data[yi]);
        let dodge = xd.spec.is_categorical() && n_groups > 1;
        let slot_half_width = if dodge { 0.4 / n_groups as f64 } else { 0.4 };
        let in_panel = |r: usize| -> bool {
            match &wrap {
                Some((codes, _)) => codes[r] == Some(wrap_level),
                None => {
                    let ok_x = gx.as_ref().is_none_or(|(codes, _)| codes[r] == Some(grid_col % n_gx));
                    let ok_y = gy.as_ref().is_none_or(|(codes, _)| codes[r] == Some(grid_row % n_gy));
                    ok_x && ok_y
                }
            }
        };
        let mut points = Vec::new();
        for &r in &page_rows {
            if !in_panel(r) {
                continue;
            }
            let (Some(x), Some(y)) = (xd.positions[r], yd.positions[r]) else { continue };
            let group = if n_groups > 1 {
                match group_codes[r] {
                    Some(g) => g,
                    None => continue, // missing subgroup: cannot be placed
                }
            } else {
                0
            };
            let x = if dodge { x - 0.4 + slot_half_width * (2 * group + 1) as f64 } else { x };
            points.push(PanelPoint {
                row: r,
                x,
                y,
                group,
                color: color[r],
                shape: shape[r],
                radius: radius[r],
                excluded: r < state.len() && state.is_excluded(r),
                filtered: filtered[r],
                freq: freq[r],
                weight: weight[r],
                color_value: color_value[r],
            });
        }
        let title = wrap.as_ref().and_then(|(_, levels)| levels.get(wrap_level)).map(|l| match &plan.wrap {
            Some(w) => format!("{w}: {l}"),
            None => l.clone(),
        });
        let labels = point_labels(table, plan, &points);
        panels.push(Panel {
            grid_row,
            grid_col,
            title,
            x: xd.spec.clone(),
            y: yd.spec.clone(),
            points,
            groups: groups.clone(),
            slot_half_width,
            extra: PanelExtra::None,
            labels,
            data_extent: [None; 2],
        });
    }
    set_data_extent(&mut panels);
    notes.extend(filter_note(state, options, &passes));
    Ok(Frame { panels, n_rows: grid_rows, n_cols: grid_cols, column_headers, row_headers, legend, notes, pages, page })
}

/// Record the extent of every panel's points on each axis in each panel.
fn set_data_extent(panels: &mut [Panel]) {
    let mut ext = [(f64::INFINITY, f64::NEG_INFINITY); 2];
    for p in panels.iter().flat_map(|p| p.points.iter()) {
        for (e, v) in ext.iter_mut().zip([p.x, p.y]) {
            if v.is_finite() {
                *e = (e.0.min(v), e.1.max(v));
            }
        }
    }
    let extent = ext.map(|(lo, hi)| (hi > lo).then_some((lo, hi)));
    for p in panels {
        p.data_extent = extent;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::{Column, MeasurementLevel};

    fn table() -> DataTable {
        let mut code = Column::numeric("code", vec![1.0, 2.0, 1.0, 2.0, 1.0, 2.0]);
        code.level = MeasurementLevel::Nominal;
        DataTable::new(
            "t",
            vec![
                Column::numeric("x", vec![1.0, 2.0, 3.0, 4.0, 5.0, 100.0]),
                Column::numeric("y", vec![2.0, 4.0, 6.0, 8.0, 10.0, -1.0]),
                Column::text("g", ["a", "b", "a", "b", "a", ""].map(String::from).to_vec()),
                Column::text("h", ["p", "p", "q", "q", "p", "q"].map(String::from).to_vec()),
                code,
            ],
        )
        .unwrap()
    }

    #[test]
    fn single_panel_with_styles() {
        let t = table();
        let mut state = RowStateManager::new(6);
        state.set_hidden([5], true);
        state.set_excluded([0], true);
        let plan =
            EncodingPlan { x: vec!["x".into()], y: vec!["y".into()], color: Some("g".into()), ..Default::default() };
        let f = build_frame(&t, &state, &plan, &FrameOptions::default()).unwrap();
        assert_eq!((f.n_rows, f.n_cols, f.panels.len()), (1, 1, 1));
        let p = &f.panels[0];
        assert_eq!(p.points.len(), 5, "hidden row left out");
        assert!(p.points[0].excluded);
        assert_eq!(p.groups.len(), 2, "categorical color forms subgroups");
        assert_ne!(p.points[0].shape, p.points[1].shape, "levels differ in shape as well as color");
        assert!(matches!(f.legend.color, Some(ColorKey::Levels { .. })));
    }

    #[test]
    fn categorical_axes_scales_and_notes() {
        let t = table();
        let state = RowStateManager::new(6);
        // Numeric codes marked nominal are categories; y on a log scale drops −1.
        let plan = EncodingPlan { x: vec!["code".into()], y: vec!["y".into()], ..Default::default() };
        let f = build_frame(&t, &state, &plan, &FrameOptions { y_scale: AxisScale::Log10, ..FrameOptions::default() })
            .unwrap();
        let p = &f.panels[0];
        assert_eq!(p.x.levels(), ["1", "2"]);
        assert_eq!(p.points.len(), 5);
        assert!((p.points[1].y - 4f64.log10()).abs() < 1e-12);
        assert_eq!(f.notes.len(), 1, "{:?}", f.notes);
        assert!(f.notes[0].contains("1 value(s) of y"));
        // y only: one unnamed category on x.
        let only_y = EncodingPlan { y: vec!["y".into()], ..Default::default() };
        let f = build_frame(&t, &state, &only_y, &FrameOptions::default()).unwrap();
        assert_eq!(f.panels[0].x.levels(), [""]);
        assert!(f.panels[0].points.iter().all(|p| p.x == 1.0));
    }

    #[test]
    fn grids_wraps_and_dodging() {
        let t = table();
        let state = RowStateManager::new(6);
        let plan = EncodingPlan {
            x: vec!["x".into()],
            y: vec!["y".into(), "x".into()],
            group_x: Some("h".into()),
            ..Default::default()
        };
        let f = build_frame(&t, &state, &plan, &FrameOptions::default()).unwrap();
        assert_eq!((f.n_rows, f.n_cols, f.panels.len()), (2, 2, 4));
        assert_eq!(f.column_headers, ["h: p", "h: q"]);
        assert_eq!(f.row_headers, ["y", "x"]);
        let counts: Vec<usize> = f.panels.iter().map(|p| p.points.len()).collect();
        assert_eq!(counts, [3, 3, 3, 3]);
        // Wrapped panels.
        let wrap =
            EncodingPlan { x: vec!["x".into()], y: vec!["y".into()], wrap: Some("g".into()), ..Default::default() };
        let f = build_frame(&t, &state, &wrap, &FrameOptions::default()).unwrap();
        assert_eq!(f.panels.len(), 2);
        assert_eq!(f.panels[1].title.as_deref(), Some("g: b"));
        // Subgroups on a categorical axis are dodged within each slot.
        let dodge =
            EncodingPlan { x: vec!["h".into()], y: vec!["y".into()], overlay: Some("g".into()), ..Default::default() };
        let f = build_frame(&t, &state, &dodge, &FrameOptions::default()).unwrap();
        let p = &f.panels[0];
        assert_eq!(p.points.len(), 5, "the row with no subgroup is left out");
        let a = p.points.iter().find(|q| q.group == 0 && q.row == 0).unwrap();
        let b = p.points.iter().find(|q| q.group == 1 && q.row == 1).unwrap();
        assert!((a.x - 0.8).abs() < 1e-12 && (b.x - 1.2).abs() < 1e-12, "{} {}", a.x, b.x);
        assert!(f.legend.groups.is_some());
        let same = EncodingPlan { color: Some("g".into()), ..dodge };
        let f = build_frame(&t, &state, &same, &FrameOptions::default()).unwrap();
        assert!(f.legend.groups.is_none(), "subgroups shown by color are keyed once");
        assert!(matches!(f.legend.color, Some(ColorKey::Levels { .. })));
        // Errors.
        assert!(build_frame(&t, &state, &EncodingPlan::default(), &FrameOptions::default()).is_err());
        let bad = EncodingPlan { x: vec!["zzz".into()], ..Default::default() };
        assert!(build_frame(&t, &state, &bad, &FrameOptions::default()).is_err());
    }

    #[test]
    fn size_and_continuous_color() {
        let t = table();
        let state = RowStateManager::new(6);
        let plan = EncodingPlan {
            x: vec!["x".into()],
            y: vec!["y".into()],
            color: Some("x".into()),
            size: Some("y".into()),
            shape: Some("h".into()),
            ..Default::default()
        };
        let f = build_frame(&t, &state, &plan, &FrameOptions::default()).unwrap();
        let p = &f.panels[0];
        assert_eq!(p.groups.len(), 1, "a continuous color does not split subgroups");
        let smallest = p.points.iter().find(|q| q.row == 5).unwrap();
        let largest = p.points.iter().find(|q| q.row == 4).unwrap();
        assert!(smallest.radius < largest.radius);
        assert_eq!(smallest.color, palette::ramp(1.0), "x = 100 is the top of the ramp");
        assert!(matches!(f.legend.color, Some(ColorKey::Gradient { .. })));
        assert_eq!(f.legend.shape.as_ref().unwrap().1.len(), 2);
    }
}
