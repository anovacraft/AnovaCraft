//! Vector-field (quiver) plot: an arrow per row from its position `(x, y)`
//! to `(x + s·u, y + s·v)`, where `u` and `v` are the row's horizontal and
//! vertical components and `s` one scale for every arrow, so lengths compare
//! (the "quiver" display of fluid mechanics and meteorology; e.g. Tufte,
//! *Envisioning Information*, 1990, on small multiples of direction).
//!
//! The scale is chosen so the longest arrow is 0.9 × the typical spacing of
//! the points, `√(width · height / n)` of the data's bounding box (the side of
//! the square each of `n` evenly spread points would occupy), times the
//! chosen percentage. Arrows are drawn in the axes' units: with both axes in
//! one unit, Plot settings ▸ Equal scale makes their angles true.

use stat_core::Result;
use stat_data::{DataTable, RowStateManager};

use crate::element::{Element, ElementKind};
use crate::encoding::{EncodingPlan, invalid, is_continuous};
use crate::frame::{
    AxisKind, AxisSpec, Frame, FrameOptions, Panel, PanelExtra, filter_note, point_labels, row_styles, select_rows,
};
use crate::layer::{KeyGlyph, Layer, LinePattern, Mark, Rgba};
use crate::palette;
use crate::view::format_value;

use super::radial::panel_point;
use super::single_panel_frame;

/// Settings of a vector-field plot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VectorSettings {
    /// Arrow length in percent of the automatic length (10–1000).
    pub scale: u16,
}

impl Default for VectorSettings {
    fn default() -> Self {
        Self { scale: 100 }
    }
}

/// One arrow, in drawn (scaled) coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arrow {
    /// Dataset row.
    pub row: usize,
    /// Start (the row's position).
    pub tail: [f64; 2],
    /// Tip.
    pub head: [f64; 2],
    /// Colour (the row's marker colour).
    pub color: Rgba,
    /// The row is excluded.
    pub excluded: bool,
}

/// The arrows of a vector-field panel.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorGeometry {
    /// One arrow per row drawn.
    pub arrows: Vec<Arrow>,
    /// Axis units per unit of the components (`s`).
    pub factor: f64,
    /// The horizontal and vertical component columns.
    pub components: [String; 2],
}

/// The frame of a vector-field plot: the first horizontal and vertical
/// columns place each row; the second column on each shelf gives its
/// horizontal and vertical component. Rows with a missing value, or whose
/// arrow leaves an axis scale's domain, are left out (and noted).
///
/// # Errors
/// [`stat_core::StatError::InvalidArgument`] without two continuous columns
/// on each axis shelf, or when the plan does not fit the table.
pub fn build_vector_frame(
    table: &DataTable,
    state: &RowStateManager,
    plan: &EncodingPlan,
    options: &FrameOptions,
    settings: VectorSettings,
) -> Result<Frame> {
    plan.validate(table)?;
    let columns = [plan.x.first(), plan.y.first(), plan.x.get(1), plan.y.get(1)];
    let need = || {
        invalid(
            "a vector field needs x then its horizontal component on the horizontal shelf, and y then its \
             vertical component on the vertical shelf (all continuous)",
        )
    };
    let mut data: Vec<&[f64]> = Vec::new();
    for c in columns {
        let c = c.filter(|c| is_continuous(table, c)).ok_or_else(need)?;
        data.push(table.find(c).and_then(|i| table.column(i)).and_then(|c| c.as_numeric()).unwrap_or(&[]));
    }
    let row = |r: usize| -> Option<[f64; 4]> {
        let v = [0, 1, 2, 3].map(|k| data[k].get(r).copied().unwrap_or(f64::NAN));
        v.iter().all(|x| x.is_finite()).then_some(v)
    };
    let rs = select_rows(table, state, plan, options);
    let style = options.style.for_panel(0);
    let styles = row_styles(table, state, plan, &rs.rows, &style);
    // One scale for every page: from all rows shown.
    let all: Vec<[f64; 4]> = rs.rows.iter().filter_map(|&r| row(r)).collect();
    let span = |k: usize| {
        let (lo, hi) = all.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v[k]), b.max(v[k])));
        if hi > lo { hi - lo } else { 0.0 }
    };
    let (sx, sy) = (span(0), span(1));
    let n = all.len().max(1) as f64;
    let spacing = match (sx > 0.0, sy > 0.0) {
        (true, true) => (sx * sy / n).sqrt(),
        (true, false) | (false, true) => sx.max(sy) / n,
        (false, false) => 1.0,
    };
    let longest = all.iter().map(|v| v[2].hypot(v[3])).fold(0.0_f64, f64::max);
    let factor = if longest > 0.0 && longest.is_finite() {
        0.9 * spacing / longest * f64::from(settings.scale.clamp(10, 1000)) / 100.0
    } else {
        0.0
    };
    let (xs, ys) = (options.x_scale, options.y_scale);
    let mut arrows = Vec::new();
    let mut points = Vec::new();
    let mut dropped = 0;
    for &r in &rs.page_rows {
        let placed = row(r).and_then(|[x, y, u, v]| {
            let tail = [xs.forward(x)?, ys.forward(y)?];
            let head = [xs.forward(x + factor * u)?, ys.forward(y + factor * v)?];
            (head[0].is_finite() && head[1].is_finite()).then_some((tail, head))
        });
        let Some((tail, head)) = placed else {
            dropped += 1;
            continue;
        };
        let p = panel_point(r, tail, &styles, &rs, state);
        arrows.push(Arrow { row: r, tail, head, color: p.color, excluded: p.excluded });
        points.push(p);
    }
    let mut notes = Vec::new();
    if dropped > 0 {
        notes.push(format!("{dropped} row(s) with a missing value, or an arrow off the axis scale, are left out."));
    }
    let components = [columns[2].cloned().unwrap_or_default(), columns[3].cloned().unwrap_or_default()];
    if factor > 0.0 {
        notes.push(format!(
            "Arrows: {} across and {} up, each unit drawn {} axis units long.",
            components[0],
            components[1],
            format_value(factor)
        ));
    }
    notes.extend(filter_note(state, options, &rs.passes));
    let axis = |k: usize, scale| AxisSpec {
        name: columns[k].cloned().unwrap_or_default(),
        kind: AxisKind::Continuous,
        scale,
        time: None,
    };
    let labels = point_labels(table, plan, &points);
    let panel = Panel {
        grid_row: 0,
        grid_col: 0,
        title: None,
        x: axis(0, xs),
        y: axis(1, ys),
        points,
        groups: styles.groups,
        slot_half_width: 0.0,
        extra: PanelExtra::Vectors(VectorGeometry { arrows, factor, components }),
        labels,
        data_extent: [None; 2],
    };
    Ok(single_panel_frame(panel, styles.legend, notes, rs.pages, rs.page))
}

/// The shaft and two head strokes of an arrow (the head 28% of its length,
/// at ±25° to the shaft).
fn arrow_segments(a: &Arrow) -> Vec<[[f64; 2]; 2]> {
    let (dx, dy) = (a.head[0] - a.tail[0], a.head[1] - a.tail[1]);
    if dx == 0.0 && dy == 0.0 {
        return Vec::new();
    }
    let (s, c) = 25f64.to_radians().sin_cos();
    let back = [-0.28 * dx, -0.28 * dy];
    let turn = |sign: f64| [back[0] * c - sign * back[1] * s, sign * back[0] * s + back[1] * c];
    let (l, r) = (turn(1.0), turn(-1.0));
    vec![
        [a.tail, a.head],
        [a.head, [a.head[0] + l[0], a.head[1] + l[1]]],
        [a.head, [a.head[0] + r[0], a.head[1] + r[1]]],
    ]
}

/// Draws the arrows of a vector-field panel in each row's colour (the
/// points layer marks the tails); while rows are selected, theirs are drawn
/// on top in the selection colour and thicker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VectorElement;

impl Element for VectorElement {
    fn kind(&self) -> ElementKind {
        ElementKind::Vectors
    }

    fn layer(&self, panel: &Panel, state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        let PanelExtra::Vectors(g) = &panel.extra else { return layer };
        let mut buckets: std::collections::BTreeMap<(u32, bool), Vec<[[f64; 2]; 2]>> =
            std::collections::BTreeMap::new();
        for a in &g.arrows {
            let selected = a.row < state.len() && state.is_selected(a.row);
            let color = if a.excluded { palette::EXCLUDED } else { a.color };
            buckets.entry((u32::from_be_bytes(color.0), selected)).or_default().extend(arrow_segments(a));
        }
        let mut ordered: Vec<_> = buckets.into_iter().collect();
        ordered.sort_by_key(|((_, sel), _)| *sel);
        let mut any_selected = false;
        for ((rgba, selected), segments) in ordered {
            any_selected |= selected;
            let (color, width) = if selected { (palette::SELECTED, 2.2) } else { (Rgba(rgba.to_be_bytes()), 1.3) };
            layer.marks.push(Mark::Segments { segments, color, width, name: "Arrows".to_owned() });
        }
        if any_selected {
            layer.key_entry("Selected rows' arrows", palette::SELECTED, KeyGlyph::Line(LinePattern::Solid));
        }
        layer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::{Column, SelectionMode};

    #[test]
    fn arrows_share_one_scale() {
        let t = DataTable::new(
            "v",
            vec![
                Column::numeric("x", vec![0.0, 1.0, 0.0, 1.0, 2.0]),
                Column::numeric("y", vec![0.0, 0.0, 1.0, 1.0, f64::NAN]),
                Column::numeric("u", vec![1.0, 0.0, -2.0, 0.0, 1.0]),
                Column::numeric("v", vec![0.0, 1.0, 0.0, 0.0, 1.0]),
            ],
        )
        .unwrap();
        let mut state = RowStateManager::new(5);
        let plan =
            EncodingPlan { x: vec!["x".into(), "u".into()], y: vec!["y".into(), "v".into()], ..Default::default() };
        let f = build_vector_frame(&t, &state, &plan, &FrameOptions::default(), VectorSettings::default()).unwrap();
        let PanelExtra::Vectors(g) = &f.panels[0].extra else { panic!() };
        // Spacing √(1·1/4) = 0.5; the longest component vector is 2.
        assert!((g.factor - 0.9 * 0.5 / 2.0).abs() < 1e-12);
        assert_eq!(g.arrows.len(), 4);
        assert!((g.arrows[2].head[0] - (0.0 - 2.0 * g.factor)).abs() < 1e-12 && g.arrows[2].head[1] == 1.0);
        assert_eq!(g.arrows[3].head, g.arrows[3].tail, "a zero vector is a dot");
        assert!(f.notes.iter().any(|n| n.contains("1 row")) && f.notes.iter().any(|n| n.starts_with("Arrows:")));
        state.select([1], SelectionMode::Replace);
        let layer = VectorElement.layer(&f.panels[0], &state);
        assert!(layer.all_finite());
        let selected =
            layer.marks.iter().find(|m| matches!(m, Mark::Segments { color, .. } if *color == palette::SELECTED));
        assert!(matches!(selected, Some(Mark::Segments { segments, .. }) if segments.len() == 3));
        // Double length.
        let f = build_vector_frame(&t, &state, &plan, &FrameOptions::default(), VectorSettings { scale: 200 }).unwrap();
        let PanelExtra::Vectors(h) = &f.panels[0].extra else { panic!() };
        assert!((h.factor - 2.0 * g.factor).abs() < 1e-12);
        let bad = EncodingPlan { x: vec!["x".into()], y: vec!["y".into()], ..Default::default() };
        assert!(build_vector_frame(&t, &state, &bad, &FrameOptions::default(), VectorSettings::default()).is_err());
    }

    #[test]
    fn arrow_heads_point_back_along_the_shaft() {
        let a = Arrow { row: 0, tail: [0.0, 0.0], head: [1.0, 0.0], color: palette::LINE, excluded: false };
        let s = arrow_segments(&a);
        assert_eq!(s.len(), 3);
        for [h, end] in &s[1..] {
            assert_eq!(*h, [1.0, 0.0]);
            assert!(end[0] < 1.0 && (end[1].abs() - 0.28 * 25f64.to_radians().sin()).abs() < 1e-12);
        }
    }
}
