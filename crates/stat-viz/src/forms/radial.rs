//! Round chart forms drawn in the unit circle, true to shape (one unit the
//! same length across and up): pie and donut charts, radar (star) charts,
//! polar charts and wind roses. Angles run clockwise from straight up
//! ("compass" angles), so a pie's first slice and a radar's first spoke start
//! at twelve o'clock, as is usual (Cleveland, *The Elements of Graphing
//! Data*, 1985, discusses reading angles and areas; pies are offered for the
//! familiar share-of-a-whole view, labelled with their shares so the numbers
//! need not be read from angles).
//!
//! * **Pie / donut** — slices by the levels of the first horizontal-shelf
//!   column, sized by row count (frequency-weighted) or by the sum of the
//!   first vertical-shelf column; slice `k` spans the angle
//!   `2π · vₖ / Σ v`. A donut leaves the centre open and writes the total
//!   there.
//! * **Radar** — one spoke per numeric column on the axis shelves, each
//!   scaled from its minimum (near the centre) to its maximum (the rim), as
//!   in parallel coordinates; one polygon per row, or per group the
//!   frequency × weight–weighted mean (Chambers, Cleveland, Kleiner & Tukey,
//!   *Graphical Methods for Data Analysis*, 1983, §5.6, "star plots").
//! * **Polar** — points at angle θ (the horizontal column, in compass
//!   degrees, mathematical degrees or radians) and radius r (the vertical
//!   column, r ≥ 0) at `(r sin θ_c, r cos θ_c)` where θ_c is the compass
//!   angle; optionally joined in row order.
//! * **Wind rose** — rows counted by direction sector (sector `k` of `S`
//!   centred on `k · 360°/S`, so the first is centred on north) and by bins of
//!   the vertical column (round bin edges 1, 2, 2.5 or 5 × 10ⁿ apart, or the
//!   levels of a categorical column), the bins stacked outward from the
//!   lowest; the radius is linear in the count or the percentage of rows.

use std::collections::BTreeMap;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

use stat_core::Result;
use stat_data::{DataTable, RowStateManager};

use crate::element::{Element, ElementKind};
use crate::encoding::{EncodingPlan, invalid, is_continuous};
use crate::frame::{
    ColorKey, Frame, FrameOptions, GroupStyle, Legend, Panel, PanelExtra, PanelPoint, ParallelAxis, RowSet, Styles,
    filter_note, levels_of, row_styles, select_rows,
};
use crate::layer::{KeyGlyph, Layer, LinePattern, Mark, MarkShape, Region, Rgba, TextAnchor};
use crate::palette;
use crate::view::format_value;

use super::{plain_axis, single_panel_frame};

/// Point at radius `r` and compass angle `theta` (radians clockwise from
/// straight up).
#[must_use]
pub fn compass_point(r: f64, theta: f64) -> [f64; 2] {
    [r * theta.sin(), r * theta.cos()]
}

/// Points along the arc of radius `r` from compass angle `a0` to `a1`, about
/// every 2°.
fn arc(r: f64, a0: f64, a1: f64) -> Vec<[f64; 2]> {
    let steps = (((a1 - a0).abs() / (PI / 90.0)).ceil() as usize).max(2);
    (0..=steps).map(|k| compass_point(r, a0 + (a1 - a0) * k as f64 / steps as f64)).collect()
}

/// A closed polyline: `points` with the first repeated at the end.
pub(crate) fn closed(mut points: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    if let Some(&first) = points.first() {
        points.push(first);
    }
    points
}

/// The annular sector between radii `r0 < r1` and compass angles `a0 < a1`:
/// its fill (a band from the inner arc to the outer, which fills non-convex
/// shapes correctly) and its outline (not closed).
fn sector(r0: f64, r1: f64, a0: f64, a1: f64, fill: Rgba, name: &str) -> (Mark, Vec<[f64; 2]>) {
    let upper = arc(r1, a0, a1);
    let lower = if r0 > 0.0 { arc(r0, a0, a1) } else { vec![[0.0, 0.0]; upper.len()] };
    let mut outline = upper.clone();
    if r0 > 0.0 {
        outline.extend(lower.iter().rev().copied());
    } else {
        outline.push([0.0, 0.0]);
    }
    (Mark::Band { lower, upper, color: fill, name: name.to_owned() }, outline)
}

/// The fill of a polygon that is star-shaped about the origin (a fan of
/// triangles from the centre).
fn fan(vertices: &[[f64; 2]], fill: Rgba, name: &str) -> Mark {
    let upper = closed(vertices.to_vec());
    Mark::Band { lower: vec![[0.0, 0.0]; upper.len()], upper, color: fill, name: name.to_owned() }
}

/// `true` if row `r` is selected.
fn is_selected(state: &RowStateManager, r: usize) -> bool {
    r < state.len() && state.is_selected(r)
}

/// `true` if row `r` is excluded.
fn is_excluded(state: &RowStateManager, r: usize) -> bool {
    r < state.len() && state.is_excluded(r)
}

/// Line patterns given to subgroups in turn, so groups differ by more than
/// colour.
pub(crate) const PATTERNS: [LinePattern; 3] = [LinePattern::Solid, LinePattern::Dashed, LinePattern::Dotted];

/// A row's point in a panel at `at`.
pub(crate) fn panel_point(r: usize, at: [f64; 2], styles: &Styles, rs: &RowSet, state: &RowStateManager) -> PanelPoint {
    PanelPoint {
        row: r,
        x: at[0],
        y: at[1],
        group: styles.group_codes[r].unwrap_or(0).min(styles.groups.len().saturating_sub(1)),
        color: styles.color[r],
        shape: styles.shape[r],
        radius: styles.radius[r],
        excluded: is_excluded(state, r),
        filtered: rs.filtered[r],
        freq: rs.freq[r],
        weight: rs.weight[r],
        color_value: styles.color_value[r],
    }
}

/// A panel in the unit circle (no axes of its own).
pub(crate) fn round_panel(points: Vec<PanelPoint>, groups: Vec<GroupStyle>, extra: PanelExtra) -> Panel {
    Panel {
        grid_row: 0,
        grid_col: 0,
        title: None,
        x: plain_axis(),
        y: plain_axis(),
        points,
        groups: if groups.is_empty() {
            vec![GroupStyle { label: String::new(), color: palette::DEFAULT_POINT, shape: MarkShape::Circle }]
        } else {
            groups
        },
        slot_half_width: 0.0,
        extra,
        labels: BTreeMap::new(),
        data_extent: [None; 2],
    }
}

/// A numeric column's values (`None` for a missing or text column).
fn numeric<'a>(table: &'a DataTable, column: Option<&String>) -> Option<&'a [f64]> {
    column.and_then(|c| table.find(c)).and_then(|i| table.column(i)).and_then(|c| c.as_numeric())
}

/// Faint ink for rings, spokes and gridlines.
fn guide(alpha: f64) -> Rgba {
    palette::LINE.with_alpha(alpha)
}

// ---------------------------------------------------------------------------
// Pie and donut
// ---------------------------------------------------------------------------

/// Radius of a donut's hole (the rim is at 1).
pub const DONUT_HOLE: f64 = 0.55;

/// Settings of a pie chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PieSettings {
    /// Leave the centre open (a donut) and write the total there.
    pub donut: bool,
    /// Size slices by the sum of the first vertical column (else by row count).
    pub size_by_sum: bool,
    /// Write each slice's level, share and value beside it.
    pub labels: bool,
}

impl Default for PieSettings {
    fn default() -> Self {
        Self { donut: false, size_by_sum: false, labels: true }
    }
}

/// One slice of a pie.
#[derive(Debug, Clone, PartialEq)]
pub struct PieSlice {
    /// The level.
    pub label: String,
    /// Count or sum the slice stands for.
    pub value: f64,
    /// Start angle (radians clockwise from straight up).
    pub start: f64,
    /// End angle.
    pub end: f64,
    /// Rows in the slice, ascending.
    pub rows: Vec<usize>,
    /// Fill.
    pub color: Rgba,
    /// Marker shape of the level (drawn at the rim, as in the key).
    pub shape: MarkShape,
}

/// The slices of a pie panel.
#[derive(Debug, Clone, PartialEq)]
pub struct PieGeometry {
    /// The column whose levels are the slices.
    pub column: String,
    /// Slices clockwise from twelve o'clock, in the levels' natural order.
    pub slices: Vec<PieSlice>,
    /// Sum of the slices' values.
    pub total: f64,
    /// The chart's settings.
    pub settings: PieSettings,
}

/// The frame of a pie or donut chart: one slice per level of the first
/// horizontal-shelf column, with area (angle) proportional to its rows'
/// frequency-weighted count, or to the sum of frequency × the first vertical
/// column. Excluded rows, rows outside an analysing filter and rows with a
/// missing level or a missing or negative size are left out of the slices
/// (and noted). Each row's point sits in the middle of its slice, so box
/// selection works; clicking a slice selects its rows.
///
/// # Errors
/// [`stat_core::StatError::InvalidArgument`] without a horizontal column, or
/// when the plan does not fit the table.
pub fn build_pie_frame(
    table: &DataTable,
    state: &RowStateManager,
    plan: &EncodingPlan,
    options: &FrameOptions,
    settings: PieSettings,
) -> Result<Frame> {
    plan.validate(table)?;
    let column = plan
        .x
        .first()
        .ok_or_else(|| invalid("a pie chart needs a column on the horizontal axis (its levels are the slices)"))?
        .clone();
    let rs = select_rows(table, state, plan, options);
    let style = options.style.for_panel(0);
    let styles = row_styles(table, state, plan, &rs.rows, &style);
    let mut notes = Vec::new();
    let values = numeric(table, plan.y.first()).filter(|_| settings.size_by_sum);
    if settings.size_by_sum && values.is_none() {
        notes.push("Sizing by a sum needs a numeric column on the vertical axis; slices show row counts.".to_owned());
    }
    let (codes, levels) = levels_of(table, &column, &rs.rows);
    let mut dropped = 0;
    let mut per_level: Vec<(f64, Vec<usize>)> = vec![(0.0, Vec::new()); levels.len()];
    for &r in rs.page_rows.iter().filter(|&&r| !rs.filtered[r] && !is_excluded(state, r)) {
        let size = match values {
            Some(v) => v.get(r).copied().filter(|x| x.is_finite() && *x >= 0.0).map(|x| x * rs.freq[r]),
            None => Some(rs.freq[r]),
        };
        match (codes[r], size.filter(|s| s.is_finite())) {
            (Some(k), Some(s)) if s > 0.0 => {
                per_level[k].0 += s;
                per_level[k].1.push(r);
            }
            (Some(_), Some(_)) => {}
            _ => dropped += 1,
        }
    }
    if dropped > 0 {
        notes.push(format!("{dropped} row(s) with a missing level or a missing or negative size are left out."));
    }
    let total: f64 = per_level.iter().map(|p| p.0).sum();
    let mut slices = Vec::new();
    let mut angle = 0.0;
    for (k, (value, rows)) in per_level.into_iter().enumerate() {
        if value <= 0.0 || total <= 0.0 || !total.is_finite() {
            continue;
        }
        let (color, shape) = style.level(&column, &levels[k], k);
        let end = angle + TAU * value / total;
        slices.push(PieSlice { label: levels[k].clone(), value, start: angle, end, rows, color, shape });
        angle = end;
    }
    let mid_radius = if settings.donut { 0.5 * (DONUT_HOLE + 1.0) } else { 0.6 };
    let points = slices
        .iter()
        .flat_map(|s| {
            let at = compass_point(mid_radius, 0.5 * (s.start + s.end));
            s.rows.iter().map(move |&r| (r, at))
        })
        .map(|(r, at)| PanelPoint { group: 0, ..panel_point(r, at, &styles, &rs, state) })
        .collect();
    let legend = Legend {
        color: Some(ColorKey::Levels {
            title: column.clone(),
            entries: slices.iter().map(|s| (s.label.clone(), s.color, s.shape)).collect(),
        }),
        ..Legend::default()
    };
    notes.extend(filter_note(state, options, &rs.passes));
    let panel = round_panel(points, Vec::new(), PanelExtra::Pie(PieGeometry { column, slices, total, settings }));
    Ok(single_panel_frame(panel, legend, notes, rs.pages, rs.page))
}

/// Draws the slices of a pie panel: each filled in its level's colour with
/// its marker shape at the rim (so slices are told apart without colour), its
/// share and value written beside it, slices holding selected rows ringed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PieElement;

impl Element for PieElement {
    fn kind(&self) -> ElementKind {
        ElementKind::Pie
    }

    fn layer(&self, panel: &Panel, state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        let PanelExtra::Pie(g) = &panel.extra else { return layer };
        let inner = if g.settings.donut { DONUT_HOLE } else { 0.0 };
        let mut any_ring = false;
        for s in &g.slices {
            let (fill, outline) = sector(inner, 1.0, s.start, s.end, s.color.with_alpha(0.85), &s.label);
            layer.marks.push(fill);
            layer.marks.push(Mark::Line {
                points: closed(outline.clone()),
                color: Rgba::rgb(255, 255, 255),
                width: 1.2,
                pattern: LinePattern::Solid,
                name: String::new(),
            });
            let selected = s.rows.iter().filter(|&&r| is_selected(state, r)).count();
            if selected > 0 {
                any_ring = true;
                layer.marks.push(Mark::Line {
                    points: closed(outline.clone()),
                    color: palette::SELECTED,
                    width: 2.5,
                    pattern: LinePattern::Solid,
                    name: "Selected rows".to_owned(),
                });
            }
            layer.regions.push(Region { outline, rows: s.rows.clone() });
            let mid = 0.5 * (s.start + s.end);
            layer.marks.push(Mark::Points {
                points: vec![compass_point(1.07, mid)],
                color: s.color,
                shape: s.shape,
                radius: 4.0,
                filled: true,
                name: s.label.clone(),
            });
            let share = if g.total > 0.0 { s.value / g.total } else { 0.0 };
            if g.settings.labels && share >= 0.01 {
                let mut text = format!("{}\n{}% ({})", s.label, format_value(100.0 * share), format_value(s.value));
                if selected > 0 {
                    text.push_str(&format!("\n{selected} selected"));
                }
                layer.marks.push(Mark::Text {
                    at: compass_point(1.3, mid),
                    text,
                    color: palette::LABEL,
                    anchor: TextAnchor::Centre,
                    movable: None,
                });
            }
        }
        if g.settings.donut && !g.slices.is_empty() {
            layer.marks.push(Mark::Text {
                at: [0.0, 0.0],
                text: format!("Total\n{}", format_value(g.total)),
                color: palette::LABEL,
                anchor: TextAnchor::Centre,
                movable: None,
            });
        }
        if any_ring {
            layer.key_entry("Slice holds selected rows", palette::SELECTED, KeyGlyph::Line(LinePattern::Solid));
        }
        layer
    }
}

// ---------------------------------------------------------------------------
// Radar
// ---------------------------------------------------------------------------

/// Radius of a spoke's minimum (its maximum is at the rim, 1), so polygons
/// through minima stay visible.
pub const RADAR_HOLE: f64 = 0.12;

/// Radius of the scaled value `t` (0 at a spoke's minimum, 1 at its maximum).
#[must_use]
pub fn radar_radius(t: f64) -> f64 {
    RADAR_HOLE + (1.0 - RADAR_HOLE) * t
}

/// Settings of a radar chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RadarSettings {
    /// One polygon per group (the colour or subgroup column's levels) through
    /// the group's means, instead of one per row.
    pub group_means: bool,
    /// Fill the polygons lightly.
    pub fill: bool,
}

/// One polygon of a radar chart.
#[derive(Debug, Clone, PartialEq)]
pub struct RadarPolygon {
    /// Row (`Row 12`) or group label.
    pub label: String,
    /// Rows it stands for, ascending.
    pub rows: Vec<usize>,
    /// Scaled value (0–1) on each spoke.
    pub values: Vec<f64>,
    /// Line and marker colour.
    pub color: Rgba,
    /// Marker shape at the vertices.
    pub shape: MarkShape,
    /// Line pattern (by subgroup).
    pub pattern: LinePattern,
    /// The row is excluded (row polygons only).
    pub excluded: bool,
}

/// Spokes and polygons of a radar panel.
#[derive(Debug, Clone, PartialEq)]
pub struct RadarGeometry {
    /// One spoke per column, clockwise from straight up: name, minimum and
    /// maximum.
    pub spokes: Vec<ParallelAxis>,
    /// Polygons, in drawing order.
    pub polygons: Vec<RadarPolygon>,
    /// The chart's settings.
    pub settings: RadarSettings,
}

impl RadarGeometry {
    /// The vertices of polygon `p`: spoke `k` at compass angle `2πk/m`.
    #[must_use]
    pub fn vertices(&self, p: &RadarPolygon) -> Vec<[f64; 2]> {
        let m = self.spokes.len().max(1) as f64;
        p.values.iter().enumerate().map(|(k, &t)| compass_point(radar_radius(t), TAU * k as f64 / m)).collect()
    }
}

/// The frame of a radar chart: one spoke per continuous column on the axis
/// shelves (horizontal, then vertical; each once), each scaled from its
/// minimum to its maximum over the rows shown. Rows lacking any spoke's value
/// are left out (and noted). With group means, each group's polygon passes
/// through its rows' means `Σ fᵢwᵢxᵢ / Σ fᵢwᵢ` (excluded rows and rows
/// outside an analysing filter left out); otherwise every row on the page is
/// a polygon with a point at each vertex for box selection.
///
/// # Errors
/// [`stat_core::StatError::InvalidArgument`] with fewer than three continuous
/// columns on the axis shelves, or when the plan does not fit the table.
pub fn build_radar_frame(
    table: &DataTable,
    state: &RowStateManager,
    plan: &EncodingPlan,
    options: &FrameOptions,
    settings: RadarSettings,
) -> Result<Frame> {
    plan.validate(table)?;
    let mut columns: Vec<&String> = Vec::new();
    let mut skipped: Vec<&String> = Vec::new();
    for c in plan.x.iter().chain(&plan.y) {
        if columns.contains(&c) || skipped.contains(&c) {
            continue;
        }
        if is_continuous(table, c) { columns.push(c) } else { skipped.push(c) }
    }
    if columns.len() < 3 {
        return Err(invalid(
            "a radar chart needs three or more continuous columns on the axis shelves (one per spoke)",
        ));
    }
    let rs = select_rows(table, state, plan, options);
    let style = options.style.for_panel(0);
    let styles = row_styles(table, state, plan, &rs.rows, &style);
    let data: Vec<&[f64]> = columns.iter().map(|c| numeric(table, Some(c)).unwrap_or(&[])).collect();
    let mut spokes = Vec::new();
    for (c, v) in columns.iter().zip(&data) {
        let (lo, hi) = rs
            .rows
            .iter()
            .filter_map(|&r| v.get(r).copied().filter(|x| x.is_finite()))
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(x), b.max(x)));
        let (lo, hi) = if lo.is_finite() { (lo, hi) } else { (0.0, 0.0) };
        spokes.push(ParallelAxis { name: (*c).clone(), low: lo, high: hi, levels: Vec::new() });
    }
    let scale = |k: usize, x: f64| {
        let s = &spokes[k];
        if s.high > s.low { ((x - s.low) / (s.high - s.low)).clamp(0.0, 1.0) } else { 0.5 }
    };
    let row_values = |r: usize| -> Option<Vec<f64>> {
        data.iter().map(|v| v.get(r).copied().filter(|x| x.is_finite())).collect::<Option<Vec<f64>>>()
    };
    let n_groups = styles.groups.len().max(1);
    let group_of = |r: usize| styles.group_codes[r].unwrap_or(0).min(n_groups - 1);
    let mut notes = Vec::new();
    let mut polygons = Vec::new();
    let mut points = Vec::new();
    let incomplete = rs.page_rows.iter().filter(|&&r| !rs.filtered[r] && row_values(r).is_none()).count();
    if settings.group_means {
        let mut sums: Vec<(Vec<f64>, f64, Vec<usize>)> = vec![(vec![0.0; columns.len()], 0.0, Vec::new()); n_groups];
        for &r in &rs.page_rows {
            let w = rs.freq[r] * rs.weight[r];
            if rs.filtered[r] || is_excluded(state, r) || w <= 0.0 || styles.group_codes[r].is_none() {
                continue;
            }
            let Some(x) = row_values(r) else { continue };
            let e = &mut sums[group_of(r)];
            for (s, v) in e.0.iter_mut().zip(&x) {
                *s += w * v;
            }
            e.1 += w;
            e.2.push(r);
        }
        for (g, (sum, w, rows)) in sums.into_iter().enumerate() {
            let means: Vec<f64> = sum.iter().map(|s| s / w).collect();
            if w <= 0.0 || means.iter().any(|m| !m.is_finite()) {
                continue;
            }
            let group = &styles.groups[g.min(styles.groups.len() - 1)];
            let label = if group.label.is_empty() { "All rows".to_owned() } else { group.label.clone() };
            polygons.push(RadarPolygon {
                label: format!("{label} mean"),
                rows,
                values: means.iter().enumerate().map(|(k, &m)| scale(k, m)).collect(),
                color: group.color,
                shape: group.shape,
                pattern: PATTERNS[g % PATTERNS.len()],
                excluded: false,
            });
        }
    } else {
        for &r in &rs.page_rows {
            let Some(x) = row_values(r) else { continue };
            let polygon = RadarPolygon {
                label: format!("Row {}", r + 1),
                rows: vec![r],
                values: x.iter().enumerate().map(|(k, &v)| scale(k, v)).collect(),
                color: styles.color[r],
                shape: styles.shape[r],
                pattern: PATTERNS[group_of(r) % PATTERNS.len()],
                excluded: is_excluded(state, r),
            };
            let m = columns.len() as f64;
            for (k, &t) in polygon.values.iter().enumerate() {
                points.push(panel_point(r, compass_point(radar_radius(t), TAU * k as f64 / m), &styles, &rs, state));
            }
            polygons.push(polygon);
        }
    }
    if incomplete > 0 {
        notes.push(format!("{incomplete} row(s) lack a value on some spoke and are left out."));
    }
    if !skipped.is_empty() {
        let names: Vec<&str> = skipped.iter().map(|s| s.as_str()).collect();
        notes.push(format!("Categorical columns are left out of the spokes: {}.", names.join(", ")));
    }
    notes.extend(filter_note(state, options, &rs.passes));
    let geometry = RadarGeometry { spokes, polygons, settings };
    let panel = round_panel(points, styles.groups, PanelExtra::Radar(geometry));
    Ok(single_panel_frame(panel, styles.legend, notes, rs.pages, rs.page))
}

/// Draws a radar panel: rings at 0, ¼, ½, ¾ and all of each spoke's range,
/// the spokes with their names and ranges, and the polygons, each in its
/// colour, marker shape and line pattern (by subgroup). While rows are
/// selected, row polygons fade and the selected ones are drawn on top in the
/// selection colour; a group polygon holding selected rows is ringed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RadarElement;

impl Element for RadarElement {
    fn kind(&self) -> ElementKind {
        ElementKind::Radar
    }

    fn layer(&self, panel: &Panel, state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        let PanelExtra::Radar(g) = &panel.extra else { return layer };
        let m = g.spokes.len();
        if m < 3 {
            return layer;
        }
        let angle = |k: usize| TAU * k as f64 / m as f64;
        for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let ring: Vec<[f64; 2]> = (0..m).map(|k| compass_point(radar_radius(t), angle(k))).collect();
            layer.marks.push(Mark::Line {
                points: closed(ring),
                color: guide(if t == 1.0 { 0.55 } else { 0.25 }),
                width: 0.8,
                pattern: LinePattern::Solid,
                name: String::new(),
            });
        }
        for (k, s) in g.spokes.iter().enumerate() {
            layer.marks.push(Mark::Line {
                points: vec![[0.0, 0.0], compass_point(1.0, angle(k))],
                color: guide(0.45),
                width: 1.0,
                pattern: LinePattern::Solid,
                name: String::new(),
            });
            layer.marks.push(Mark::Text {
                at: compass_point(1.18, angle(k)),
                text: format!("{}\n{} – {}", s.name, format_value(s.low), format_value(s.high)),
                color: palette::LABEL,
                anchor: TextAnchor::Centre,
                movable: None,
            });
        }
        let means = g.settings.group_means;
        let selected = |p: &RadarPolygon| p.rows.iter().any(|&r| is_selected(state, r));
        let any_selected = g.polygons.iter().any(selected);
        // Unselected first, so the selection draws over them.
        let mut order: Vec<&RadarPolygon> = g.polygons.iter().collect();
        if !means {
            order.sort_by_key(|p| selected(p));
        }
        for p in order {
            let vertices = g.vertices(p);
            let sel = selected(p);
            let base = if p.excluded { palette::EXCLUDED } else { p.color };
            let (color, width) = match (means, any_selected, sel) {
                (false, true, true) => (palette::SELECTED, 2.4),
                (false, true, false) => (base.with_alpha(0.25), 1.0),
                (false, false, _) => (base, 1.4),
                (true, ..) => (base, 2.0),
            };
            if g.settings.fill {
                layer.marks.push(fan(&vertices, base.with_alpha(if means { 0.14 } else { 0.06 }), &p.label));
            }
            if means && sel {
                layer.marks.push(Mark::Line {
                    points: closed(vertices.clone()),
                    color: palette::SELECTED,
                    width: 5.0,
                    pattern: LinePattern::Solid,
                    name: "Selected rows".to_owned(),
                });
            }
            layer.marks.push(Mark::Line {
                points: closed(vertices.clone()),
                color,
                width,
                pattern: p.pattern,
                name: p.label.clone(),
            });
            layer.marks.push(Mark::Points {
                points: vertices.clone(),
                color,
                shape: if p.excluded { MarkShape::Cross } else { p.shape },
                radius: if means { 4.5 } else { 3.0 },
                filled: true,
                name: p.label.clone(),
            });
            if means {
                layer.regions.push(Region { outline: vertices, rows: p.rows.clone() });
            }
        }
        if means {
            for p in &g.polygons {
                layer.key_entry(p.label.clone(), p.color, KeyGlyph::Line(p.pattern));
            }
        } else if panel.groups.len() > 1 {
            for (k, grp) in panel.groups.iter().enumerate() {
                layer.key_entry(grp.label.clone(), grp.color, KeyGlyph::Line(PATTERNS[k % PATTERNS.len()]));
            }
        }
        if any_selected {
            let what = if means { "Group holds selected rows" } else { "Selected rows" };
            layer.key_entry(what, palette::SELECTED, KeyGlyph::Line(LinePattern::Solid));
        }
        layer
    }
}

// ---------------------------------------------------------------------------
// Polar
// ---------------------------------------------------------------------------

/// How a polar chart reads its angle column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum AngleUnit {
    /// Degrees clockwise from straight up (compass bearings: 90 is east).
    #[default]
    Compass,
    /// Degrees anticlockwise from the right (mathematical convention).
    Degrees,
    /// Radians anticlockwise from the right.
    Radians,
}

impl AngleUnit {
    /// Every unit, in menu order.
    pub const ALL: [AngleUnit; 3] = [AngleUnit::Compass, AngleUnit::Degrees, AngleUnit::Radians];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Compass => "Degrees clockwise from north",
            Self::Degrees => "Degrees anticlockwise from east",
            Self::Radians => "Radians anticlockwise from east",
        }
    }

    /// Short name for saving.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Compass => "compass",
            Self::Degrees => "degrees",
            Self::Radians => "radians",
        }
    }

    /// The unit with this [`AngleUnit::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|u| u.key() == key)
    }

    /// The compass angle (radians clockwise from straight up) of `value`.
    #[must_use]
    pub fn to_compass(self, value: f64) -> f64 {
        match self {
            Self::Compass => value.to_radians(),
            Self::Degrees => FRAC_PI_2 - value.to_radians(),
            Self::Radians => FRAC_PI_2 - value,
        }
    }
}

/// Settings of a polar chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PolarSettings {
    /// How the angle column is read.
    pub angle: AngleUnit,
    /// Join each subgroup's points in row order.
    pub connect: bool,
}

/// Rings and spokes of a polar panel.
#[derive(Debug, Clone, PartialEq)]
pub struct PolarGeometry {
    /// Radius drawn at the rim (data units).
    pub r_max: f64,
    /// Rings: fraction of the rim radius and label.
    pub rings: Vec<(f64, String)>,
    /// Spokes: compass angle and label.
    pub spokes: Vec<(f64, String)>,
    /// Angle and radius column names.
    pub columns: [String; 2],
    /// The chart's settings.
    pub settings: PolarSettings,
}

/// Round ticks `0 < t₁ < … ≤ top` covering `0 ..= max` (`top ≥ max`).
fn radial_ticks(max: f64) -> (f64, Vec<f64>) {
    if !(max.is_finite() && max > 0.0) {
        return (1.0, vec![0.5, 1.0]);
    }
    let ticks = crate::export::nice_ticks(0.0, max, 4);
    let step = match ticks.as_slice() {
        [a, b, ..] => b - a,
        _ => max,
    };
    let n = (max / step - 1e-9).ceil().max(1.0) as usize;
    let ticks: Vec<f64> = (1..=n).map(|k| k as f64 * step).collect();
    (ticks.last().copied().unwrap_or(max), ticks)
}

/// The frame of a polar chart: each row at the compass angle of the first
/// horizontal column (read in `settings.angle`) and the radius of the first
/// vertical column, the rim at a round radius at or above the largest.
/// Rows with a missing angle, or a missing or negative radius, are left out
/// (and noted).
///
/// # Errors
/// [`stat_core::StatError::InvalidArgument`] without numeric horizontal and
/// vertical columns, or when the plan does not fit the table.
pub fn build_polar_frame(
    table: &DataTable,
    state: &RowStateManager,
    plan: &EncodingPlan,
    options: &FrameOptions,
    settings: PolarSettings,
) -> Result<Frame> {
    plan.validate(table)?;
    let need = || invalid("a polar chart needs a numeric angle column across and a numeric radius column up");
    let angle_col = plan.x.first().filter(|c| is_continuous(table, c)).ok_or_else(need)?;
    let radius_col = plan.y.first().filter(|c| is_continuous(table, c)).ok_or_else(need)?;
    let (a, r) = (numeric(table, Some(angle_col)).unwrap_or(&[]), numeric(table, Some(radius_col)).unwrap_or(&[]));
    let valid = |row: usize| {
        let (t, d) = (a.get(row).copied().unwrap_or(f64::NAN), r.get(row).copied().unwrap_or(f64::NAN));
        (t.is_finite() && d.is_finite() && d >= 0.0).then_some((t, d))
    };
    let rs = select_rows(table, state, plan, options);
    let style = options.style.for_panel(0);
    let styles = row_styles(table, state, plan, &rs.rows, &style);
    let largest = rs.rows.iter().filter_map(|&row| valid(row)).map(|v| v.1).fold(0.0_f64, f64::max);
    let (r_max, ticks) = radial_ticks(largest);
    let mut dropped = 0;
    let mut points = Vec::new();
    for &row in &rs.page_rows {
        match valid(row) {
            Some((t, d)) => {
                let at = compass_point(d / r_max, settings.angle.to_compass(t));
                points.push(panel_point(row, at, &styles, &rs, state));
            }
            None => dropped += 1,
        }
    }
    let spokes: Vec<(f64, String)> = match settings.angle {
        AngleUnit::Radians => {
            const NAMES: [&str; 8] = ["0", "π/4", "π/2", "3π/4", "π", "5π/4", "3π/2", "7π/4"];
            NAMES
                .iter()
                .enumerate()
                .map(|(k, n)| (settings.angle.to_compass(k as f64 * PI / 4.0), (*n).to_owned()))
                .collect()
        }
        unit => (0..12).map(|k| (unit.to_compass(30.0 * k as f64), format!("{}°", 30 * k))).collect(),
    };
    let mut notes = Vec::new();
    if dropped > 0 {
        notes.push(format!("{dropped} row(s) with a missing angle or a missing or negative radius are left out."));
    }
    notes.extend(filter_note(state, options, &rs.passes));
    let geometry = PolarGeometry {
        r_max,
        rings: ticks.iter().map(|&t| (t / r_max, format_value(t))).collect(),
        spokes,
        columns: [angle_col.clone(), radius_col.clone()],
        settings,
    };
    let mut panel = round_panel(points, styles.groups, PanelExtra::Polar(geometry));
    panel.labels = crate::frame::point_labels(table, plan, &panel.points);
    Ok(single_panel_frame(panel, styles.legend, notes, rs.pages, rs.page))
}

/// Draws the rings and spokes of a polar panel (the points layer draws the
/// rows), and, when asked, each subgroup's points joined in row order in its
/// colour and line pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PolarElement;

impl Element for PolarElement {
    fn kind(&self) -> ElementKind {
        ElementKind::Polar
    }

    fn layer(&self, panel: &Panel, _state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        let PanelExtra::Polar(g) = &panel.extra else { return layer };
        for (k, (frac, label)) in g.rings.iter().enumerate() {
            let outer = k + 1 == g.rings.len();
            layer.marks.push(Mark::Line {
                points: arc(*frac, 0.0, TAU),
                color: guide(if outer { 0.6 } else { 0.25 }),
                width: if outer { 1.2 } else { 0.8 },
                pattern: LinePattern::Solid,
                name: String::new(),
            });
            layer.marks.push(Mark::Text {
                at: compass_point(*frac, PI / 12.0),
                text: label.clone(),
                color: palette::LABEL,
                anchor: TextAnchor::LeftOfText,
                movable: None,
            });
        }
        for (theta, label) in &g.spokes {
            layer.marks.push(Mark::Line {
                points: vec![[0.0, 0.0], compass_point(1.0, *theta)],
                color: guide(0.25),
                width: 0.8,
                pattern: LinePattern::Solid,
                name: String::new(),
            });
            layer.marks.push(Mark::Text {
                at: compass_point(1.1, *theta),
                text: label.clone(),
                color: palette::LABEL,
                anchor: TextAnchor::Centre,
                movable: None,
            });
        }
        layer.marks.push(Mark::Text {
            at: [-1.2, -1.15],
            text: format!("Angle: {}\nRadius: {}", g.columns[0], g.columns[1]),
            color: palette::LABEL,
            anchor: TextAnchor::LeftOfText,
            movable: None,
        });
        if g.settings.connect {
            let mut by_group: BTreeMap<usize, Vec<&PanelPoint>> = BTreeMap::new();
            for p in &panel.points {
                by_group.entry(p.group).or_default().push(p);
            }
            for (k, mut pts) in by_group {
                pts.sort_by_key(|p| p.row);
                let color = panel.groups.get(k).map_or(palette::LINE, |s| s.color);
                let pattern = PATTERNS[k % PATTERNS.len()];
                layer.marks.push(Mark::Line {
                    points: pts.iter().map(|p| [p.x, p.y]).collect(),
                    color,
                    width: 1.3,
                    pattern,
                    name: match panel.groups.get(k).filter(|_| panel.groups.len() > 1) {
                        Some(grp) => format!("{}: joined in row order", grp.label),
                        None => "Joined in row order".to_owned(),
                    },
                });
                if panel.groups.len() > 1 {
                    layer.key_entry(panel.groups[k].label.clone(), color, KeyGlyph::Line(pattern));
                }
            }
        }
        layer
    }
}

// ---------------------------------------------------------------------------
// Wind rose
// ---------------------------------------------------------------------------

/// Settings of a wind rose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindRoseSettings {
    /// Number of direction sectors (4 to 72; usually 8, 16 or 36).
    pub sectors: u8,
    /// Rings read as a percentage of the rows counted (else as counts).
    pub percent: bool,
}

impl Default for WindRoseSettings {
    fn default() -> Self {
        Self { sectors: 16, percent: true }
    }
}

/// One stacked piece of a wind rose: rows of one magnitude bin in one sector.
#[derive(Debug, Clone, PartialEq)]
pub struct WindPiece {
    /// Sector index (0 centred on north, then clockwise).
    pub sector: usize,
    /// Magnitude bin index (0 the lowest, innermost).
    pub bin: usize,
    /// Inner radius.
    pub r0: f64,
    /// Outer radius.
    pub r1: f64,
    /// Start compass angle (radians).
    pub a0: f64,
    /// End compass angle.
    pub a1: f64,
    /// Frequency-weighted count.
    pub count: f64,
    /// Rows, ascending.
    pub rows: Vec<usize>,
}

/// The pieces, bins and rings of a wind-rose panel.
#[derive(Debug, Clone, PartialEq)]
pub struct WindRoseGeometry {
    /// Stacked pieces.
    pub pieces: Vec<WindPiece>,
    /// Bin labels, lowest first (`0–2`, … or a categorical column's levels).
    pub bins: Vec<String>,
    /// Rings: fraction of the rim radius and label (counts or percentages).
    pub rings: Vec<(f64, String)>,
    /// The magnitude column (`None`: every row in one bin).
    pub magnitude: Option<String>,
    /// Total count of the rows drawn.
    pub total: f64,
    /// The chart's settings.
    pub settings: WindRoseSettings,
}

impl WindRoseGeometry {
    /// Colour of bin `k`: the sequential ramp from the lowest bin (dark) to
    /// the highest (light), so order reads without hue.
    #[must_use]
    pub fn bin_color(&self, k: usize) -> Rgba {
        let n = self.bins.len();
        if n <= 1 { palette::BAR } else { palette::ramp(0.9 * k as f64 / (n - 1) as f64) }
    }
}

/// The frame of a wind rose: rows counted (frequency-weighted) by direction
/// sector (the first horizontal column, in compass degrees, any whole turn)
/// and by magnitude bin (the first vertical column binned at round edges from
/// zero, its levels if categorical, or one bin without a vertical column),
/// stacked outward from the lowest bin. The longest sector reaches the rim.
/// Excluded rows, rows outside an analysing filter, and rows with a missing
/// direction or a missing or negative magnitude are left out (and noted).
///
/// # Errors
/// [`stat_core::StatError::InvalidArgument`] without a numeric horizontal
/// column, or when the plan does not fit the table.
pub fn build_wind_rose_frame(
    table: &DataTable,
    state: &RowStateManager,
    plan: &EncodingPlan,
    options: &FrameOptions,
    settings: WindRoseSettings,
) -> Result<Frame> {
    plan.validate(table)?;
    let direction_col = plan
        .x
        .first()
        .filter(|c| is_continuous(table, c))
        .ok_or_else(|| invalid("a wind rose needs a numeric direction column (compass degrees) across"))?;
    let direction = numeric(table, Some(direction_col)).unwrap_or(&[]);
    let rs = select_rows(table, state, plan, options);
    let style = options.style.for_panel(0);
    let styles = row_styles(table, state, plan, &rs.rows, &style);
    let magnitude = plan.y.first().cloned();
    // The bin of every row (None: missing or negative).
    let (bin_of, bins): (Vec<Option<usize>>, Vec<String>) = match &magnitude {
        None => (vec![Some(0); table.n_rows()], vec!["All rows".to_owned()]),
        Some(c) if is_continuous(table, c) => {
            let v = numeric(table, Some(c)).unwrap_or(&[]);
            let value = |r: usize| v.get(r).copied().filter(|x| x.is_finite() && *x >= 0.0);
            let largest = rs.rows.iter().filter_map(|&r| value(r)).fold(0.0_f64, f64::max);
            let (top, edges) = radial_ticks(largest);
            let step = top / edges.len().max(1) as f64;
            let n = edges.len().max(1);
            let codes =
                (0..table.n_rows()).map(|r| value(r).map(|x| ((x / step).floor() as usize).min(n - 1))).collect();
            let labels = (0..n)
                .map(|k| format!("{} – {}", format_value(k as f64 * step), format_value((k + 1) as f64 * step)))
                .collect();
            (codes, labels)
        }
        Some(c) => levels_of(table, c, &rs.rows),
    };
    let sectors = usize::from(settings.sectors.clamp(4, 72));
    let width = TAU / sectors as f64;
    let mut counts: Vec<Vec<(f64, Vec<usize>)>> = vec![vec![(0.0, Vec::new()); bins.len()]; sectors];
    let mut dropped = 0;
    for &r in rs.page_rows.iter().filter(|&&r| !rs.filtered[r] && !is_excluded(state, r) && rs.freq[r] > 0.0) {
        let d = direction.get(r).copied().unwrap_or(f64::NAN);
        match (d.is_finite(), bin_of[r]) {
            (true, Some(b)) => {
                let s = ((d.rem_euclid(360.0).to_radians() / width + 0.5).floor() as usize) % sectors;
                counts[s][b].0 += rs.freq[r];
                counts[s][b].1.push(r);
            }
            _ => dropped += 1,
        }
    }
    let total: f64 = counts.iter().flatten().map(|c| c.0).sum();
    let longest = counts.iter().map(|s| s.iter().map(|c| c.0).sum::<f64>()).fold(0.0_f64, f64::max);
    let mut pieces = Vec::new();
    let mut points = Vec::new();
    for (s, row) in counts.into_iter().enumerate() {
        let centre = s as f64 * width;
        let (a0, a1) = (centre - 0.45 * width, centre + 0.45 * width);
        let mut acc = 0.0;
        for (b, (count, rows)) in row.into_iter().enumerate() {
            if count <= 0.0 || longest <= 0.0 {
                continue;
            }
            let (r0, r1) = (acc / longest, (acc + count) / longest);
            acc += count;
            let at = compass_point(0.5 * (r0 + r1), centre);
            points.extend(rows.iter().map(|&r| PanelPoint { group: 0, ..panel_point(r, at, &styles, &rs, state) }));
            pieces.push(WindPiece { sector: s, bin: b, r0, r1, a0, a1, count, rows });
        }
    }
    let scale = if settings.percent && total > 0.0 { 100.0 / total } else { 1.0 };
    let (_, ticks) = radial_ticks(longest * scale);
    let rings = ticks
        .iter()
        .filter(|&&t| longest > 0.0 && t <= longest * scale * (1.0 + 1e-9))
        .map(|&t| {
            (t / (longest * scale), if settings.percent { format!("{}%", format_value(t)) } else { format_value(t) })
        })
        .collect();
    let mut notes = Vec::new();
    if dropped > 0 {
        notes.push(format!(
            "{dropped} row(s) with a missing direction or a missing or negative magnitude are left out."
        ));
    }
    notes.extend(filter_note(state, options, &rs.passes));
    let geometry = WindRoseGeometry { pieces, bins, rings, magnitude, total, settings };
    let panel = round_panel(points, Vec::new(), PanelExtra::WindRose(geometry));
    Ok(single_panel_frame(panel, Legend::default(), notes, rs.pages, rs.page))
}

/// Draws a wind rose: rings with their counts or percentages, compass
/// spokes and labels, and the stacked pieces in a sequential ramp from the
/// lowest magnitude bin outward (named in the key); pieces holding selected
/// rows are ringed, and clicking one selects its rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WindRoseElement;

impl Element for WindRoseElement {
    fn kind(&self) -> ElementKind {
        ElementKind::WindRose
    }

    fn layer(&self, panel: &Panel, state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        let PanelExtra::WindRose(g) = &panel.extra else { return layer };
        for (frac, label) in &g.rings {
            layer.marks.push(Mark::Line {
                points: arc(*frac, 0.0, TAU),
                color: guide(0.3),
                width: 0.8,
                pattern: LinePattern::Solid,
                name: String::new(),
            });
            layer.marks.push(Mark::Text {
                at: compass_point(*frac, 0.875 * PI),
                text: label.clone(),
                color: palette::LABEL,
                anchor: TextAnchor::LeftOfText,
                movable: None,
            });
        }
        const COMPASS: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
        for (k, name) in COMPASS.iter().enumerate() {
            let theta = k as f64 * PI / 4.0;
            layer.marks.push(Mark::Line {
                points: vec![[0.0, 0.0], compass_point(1.0, theta)],
                color: guide(0.25),
                width: 0.8,
                pattern: LinePattern::Solid,
                name: String::new(),
            });
            layer.marks.push(Mark::Text {
                at: compass_point(1.1, theta),
                text: (*name).to_owned(),
                color: palette::LABEL,
                anchor: TextAnchor::Centre,
                movable: None,
            });
        }
        let mut any_ring = false;
        for p in &g.pieces {
            let color = g.bin_color(p.bin);
            let (fill, outline) = sector(p.r0, p.r1, p.a0, p.a1, color.with_alpha(0.9), &g.bins[p.bin]);
            layer.marks.push(fill);
            layer.marks.push(Mark::Line {
                points: closed(outline.clone()),
                color: guide(0.6),
                width: 0.7,
                pattern: LinePattern::Solid,
                name: String::new(),
            });
            if p.rows.iter().any(|&r| is_selected(state, r)) {
                any_ring = true;
                layer.marks.push(Mark::Line {
                    points: closed(outline.clone()),
                    color: palette::SELECTED,
                    width: 2.2,
                    pattern: LinePattern::Solid,
                    name: "Selected rows".to_owned(),
                });
            }
            layer.regions.push(Region { outline, rows: p.rows.clone() });
        }
        let what = if g.settings.percent { "% of rows" } else { "rows" };
        layer.marks.push(Mark::Text {
            at: [-1.2, -1.15],
            text: format!("Rings: {what} (of {})", format_value(g.total)),
            color: palette::LABEL,
            anchor: TextAnchor::LeftOfText,
            movable: None,
        });
        for (k, b) in g.bins.iter().enumerate() {
            let color = g.bin_color(k);
            let label = match &g.magnitude {
                Some(m) => format!("{m}: {b}"),
                None => b.clone(),
            };
            layer.key_entry(label, palette::LINE, KeyGlyph::Area(color.with_alpha(0.9)));
        }
        if any_ring {
            layer.key_entry("Piece holds selected rows", palette::SELECTED, KeyGlyph::Line(LinePattern::Solid));
        }
        layer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::{Column, SelectionMode};

    fn text(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    fn pie_table() -> DataTable {
        DataTable::new(
            "p",
            vec![
                Column::text("kind", text(&["a", "b", "b", "c", "c", "c", ""])),
                Column::numeric("amount", vec![10.0, 5.0, 5.0, 20.0, f64::NAN, 50.0, 3.0]),
            ],
        )
        .unwrap()
    }

    #[test]
    fn pie_slices_share_the_circle() {
        let t = pie_table();
        let mut state = RowStateManager::new(7);
        let plan = EncodingPlan { x: vec!["kind".into()], y: vec!["amount".into()], ..Default::default() };
        let f = build_pie_frame(&t, &state, &plan, &FrameOptions::default(), PieSettings::default()).unwrap();
        let PanelExtra::Pie(g) = &f.panels[0].extra else { panic!() };
        let values: Vec<(&str, f64)> = g.slices.iter().map(|s| (s.label.as_str(), s.value)).collect();
        assert_eq!(values, [("a", 1.0), ("b", 2.0), ("c", 3.0)], "row counts in natural order");
        assert!((g.slices[2].end - TAU).abs() < 1e-12 && g.slices[0].start == 0.0);
        assert!((g.slices[1].end - g.slices[1].start - TAU / 3.0).abs() < 1e-12, "b is a third");
        assert!(f.notes.iter().any(|n| n.contains("1 row")), "the row without a level: {:?}", f.notes);
        // By sum: c = 20 + 50 (the missing amount is left out).
        let sums = PieSettings { size_by_sum: true, donut: true, labels: true };
        let f = build_pie_frame(&t, &state, &plan, &FrameOptions::default(), sums).unwrap();
        let PanelExtra::Pie(g) = &f.panels[0].extra else { panic!() };
        assert_eq!(g.total, 90.0);
        assert_eq!(g.slices[2].rows, [3, 5]);
        // Selecting a row rings its slice; the slice's region selects its rows.
        state.select([0], SelectionMode::Replace);
        let layer = PieElement.layer(&f.panels[0], &state);
        assert!(layer.all_finite());
        assert_eq!(
            layer.marks.iter().filter(|m| matches!(m, Mark::Line { name, .. } if name == "Selected rows")).count(),
            1
        );
        assert_eq!(layer.regions.len(), 3);
        let mid = 0.5 * (g.slices[1].start + g.slices[1].end);
        assert_eq!(crate::view::region_rows(std::slice::from_ref(&layer), compass_point(0.8, mid)), Some(vec![1, 2]));
        assert_eq!(crate::view::region_rows(std::slice::from_ref(&layer), [0.0, 0.0]), None, "the donut's hole");
        assert!(layer.marks.iter().any(|m| matches!(m, Mark::Text { text, .. } if text.starts_with("Total\n90"))));
        assert!(layer.marks.iter().any(|m| matches!(m, Mark::Text { text, .. } if text.contains("1 selected"))));
        // Every row's point lies in its slice.
        for p in &f.panels[0].points {
            let s = g.slices.iter().find(|s| s.rows.contains(&p.row)).unwrap();
            let theta = p.x.atan2(p.y).rem_euclid(TAU);
            assert!(theta >= s.start && theta <= s.end);
        }
        assert!(build_pie_frame(&t, &state, &EncodingPlan::default(), &FrameOptions::default(), sums).is_err());
    }

    fn radar_table() -> DataTable {
        DataTable::new(
            "r",
            vec![
                Column::numeric("a", vec![1.0, 2.0, 3.0, 4.0]),
                Column::numeric("b", vec![10.0, 20.0, 30.0, f64::NAN]),
                Column::numeric("c", vec![5.0, 5.0, 6.0, 7.0]),
                Column::text("g", text(&["x", "x", "y", "y"])),
            ],
        )
        .unwrap()
    }

    #[test]
    fn radar_spokes_scale_and_group_means() {
        let t = radar_table();
        let mut state = RowStateManager::new(4);
        let plan = EncodingPlan {
            x: vec!["a".into(), "b".into()],
            y: vec!["c".into(), "g".into()],
            color: Some("g".into()),
            ..Default::default()
        };
        let f = build_radar_frame(&t, &state, &plan, &FrameOptions::default(), RadarSettings::default()).unwrap();
        let PanelExtra::Radar(g) = &f.panels[0].extra else { panic!() };
        assert_eq!(g.spokes.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
        assert_eq!((g.spokes[1].low, g.spokes[1].high), (10.0, 30.0));
        assert_eq!(g.polygons.len(), 3, "row 3 lacks b");
        assert_eq!(g.polygons[1].values, [1.0 / 3.0, 0.5, 0.0]);
        assert_eq!(g.polygons[2].pattern, LinePattern::Dashed, "the second group's pattern");
        assert_eq!(f.panels[0].points.len(), 9);
        assert_eq!(f.notes.len(), 2, "{:?}", f.notes);
        // Row 0 at the minimum of every spoke: on the inner ring.
        let v = g.vertices(&g.polygons[0]);
        assert!((v[0][1] - RADAR_HOLE).abs() < 1e-12 && v[0][0].abs() < 1e-12);
        // Group means: x = rows 0, 1; y = row 2 only (row 3 incomplete).
        let means = RadarSettings { group_means: true, fill: true };
        let f = build_radar_frame(&t, &state, &plan, &FrameOptions::default(), means).unwrap();
        let PanelExtra::Radar(g) = &f.panels[0].extra else { panic!() };
        assert_eq!(g.polygons.len(), 2);
        assert_eq!(g.polygons[0].rows, [0, 1]);
        assert!((g.polygons[0].values[0] - 0.5 / 3.0).abs() < 1e-12, "mean a = 1.5");
        assert!(f.panels[0].points.is_empty());
        state.select([2], SelectionMode::Replace);
        let layer = RadarElement.layer(&f.panels[0], &state);
        assert!(layer.all_finite());
        assert_eq!(layer.regions.len(), 2);
        assert!(layer.key.iter().any(|k| k.label == "y mean" && k.glyph == KeyGlyph::Line(LinePattern::Dashed)));
        assert!(layer.key.iter().any(|k| k.label == "Group holds selected rows"));
        let two = EncodingPlan { x: vec!["a".into(), "b".into()], ..Default::default() };
        assert!(build_radar_frame(&t, &state, &two, &FrameOptions::default(), means).is_err());
    }

    #[test]
    fn polar_points_and_angle_units() {
        let t = DataTable::new(
            "p",
            vec![
                Column::numeric("theta", vec![0.0, 90.0, 180.0, 270.0, 45.0]),
                Column::numeric("r", vec![1.0, 2.0, 3.0, -1.0, 4.0]),
            ],
        )
        .unwrap();
        let state = RowStateManager::new(5);
        let plan = EncodingPlan { x: vec!["theta".into()], y: vec!["r".into()], ..Default::default() };
        let f = build_polar_frame(&t, &state, &plan, &FrameOptions::default(), PolarSettings::default()).unwrap();
        let PanelExtra::Polar(g) = &f.panels[0].extra else { panic!() };
        assert_eq!(g.r_max, 4.0);
        let p = &f.panels[0].points;
        assert_eq!(p.len(), 4, "the negative radius is left out");
        let close = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() < 1e-12 && (a[1] - b[1]).abs() < 1e-12;
        assert!(close([p[0].x, p[0].y], [0.0, 0.25]), "north");
        assert!(close([p[1].x, p[1].y], [0.5, 0.0]), "90° compass is east");
        assert!(f.notes.iter().any(|n| n.contains("1 row")));
        // Mathematical degrees: 90 is straight up.
        let math = PolarSettings { angle: AngleUnit::Degrees, connect: true };
        let f = build_polar_frame(&t, &state, &plan, &FrameOptions::default(), math).unwrap();
        let p = &f.panels[0].points;
        assert!(close([p[1].x, p[1].y], [0.0, 0.5]) && close([p[0].x, p[0].y], [0.25, 0.0]));
        let layer = PolarElement.layer(&f.panels[0], &state);
        assert!(layer.all_finite());
        assert!(layer.marks.iter().any(|m| matches!(m, Mark::Line { name, .. } if name.contains("row order"))));
        assert!((AngleUnit::Radians.to_compass(PI) - (-FRAC_PI_2)).abs() < 1e-12);
        for u in AngleUnit::ALL {
            assert_eq!(AngleUnit::from_key(u.key()), Some(u));
        }
        let bad = EncodingPlan { x: vec!["theta".into()], ..Default::default() };
        assert!(build_polar_frame(&t, &state, &bad, &FrameOptions::default(), math).is_err());
    }

    #[test]
    fn wind_rose_counts_by_sector_and_bin() {
        // Directions near north fall in sector 0 either side of 0°.
        let t = DataTable::new(
            "w",
            vec![
                Column::numeric("dir", vec![350.0, 5.0, 10.0, 90.0, 95.0, 180.0, f64::NAN, 720.0]),
                Column::numeric("speed", vec![1.0, 3.0, 5.0, 1.0, 1.5, 9.0, 2.0, 0.0]),
            ],
        )
        .unwrap();
        let mut state = RowStateManager::new(8);
        let plan = EncodingPlan { x: vec!["dir".into()], y: vec!["speed".into()], ..Default::default() };
        let settings = WindRoseSettings { sectors: 4, percent: true };
        let f = build_wind_rose_frame(&t, &state, &plan, &FrameOptions::default(), settings).unwrap();
        let PanelExtra::WindRose(g) = &f.panels[0].extra else { panic!() };
        assert_eq!(g.bins, ["0 – 2.5", "2.5 – 5", "5 – 7.5", "7.5 – 10"]);
        assert_eq!(g.total, 7.0);
        let north: Vec<(usize, f64)> = g.pieces.iter().filter(|p| p.sector == 0).map(|p| (p.bin, p.count)).collect();
        assert_eq!(north, [(0, 2.0), (1, 1.0), (2, 1.0)], "350, 720 (= 0) in the lowest bin; 5 and 10");
        let n_top = g.pieces.iter().filter(|p| p.sector == 0).map(|p| p.r1).fold(0.0, f64::max);
        assert!((n_top - 1.0).abs() < 1e-12, "the longest sector reaches the rim");
        let east: Vec<&WindPiece> = g.pieces.iter().filter(|p| p.sector == 1).collect();
        assert_eq!((east.len(), east[0].rows.clone(), east[0].r1), (1, vec![3, 4], 0.5));
        assert!(f.notes.iter().any(|n| n.contains("1 row")));
        assert_eq!(f.panels[0].points.len(), 7);
        state.select([5], SelectionMode::Replace);
        let layer = WindRoseElement.layer(&f.panels[0], &state);
        assert!(layer.all_finite());
        assert_eq!(layer.key.len(), 5);
        assert_eq!(layer.regions.len(), g.pieces.len());
        // Without a magnitude column: one bin per sector.
        let one = EncodingPlan { x: vec!["dir".into()], ..Default::default() };
        let f = build_wind_rose_frame(&t, &state, &one, &FrameOptions::default(), settings).unwrap();
        let PanelExtra::WindRose(g) = &f.panels[0].extra else { panic!() };
        assert_eq!(g.bins, ["All rows"]);
        assert_eq!(g.pieces.iter().map(|p| p.count).sum::<f64>(), 7.0);
        assert!(
            build_wind_rose_frame(&t, &state, &EncodingPlan::default(), &FrameOptions::default(), settings).is_err()
        );
    }
}
