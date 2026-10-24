//! Ternary plot of data: three non-negative component columns, each row
//! divided by its total so the components sum to 1 (closure), drawn in an
//! equilateral triangle true to shape (Aitchison, *The Statistical Analysis
//! of Compositional Data*, 1986, ch. 1; Howarth, "Sources for a history of
//! the ternary diagram", *British Journal for the History of Science* 29,
//! 1996).
//!
//! The first component's corner is at the lower left `(0, 0)`, the second's at
//! the lower right `(1, 0)` and the third's at the top `(½, √3/2)`, so the
//! composition `(a, b, c)` sits at `(b + c/2, c·√3/2)` — the same layout as
//! the ternary contour plots of mixture experiments. Gridlines join points
//! of equal share of a component every 0.1; each component's shares are
//! labelled along one edge, rising towards its corner.

use stat_core::Result;
use stat_data::{DataTable, RowStateManager};

use crate::element::{Element, ElementKind};
use crate::encoding::{EncodingPlan, invalid, is_continuous};
use crate::frame::{Frame, FrameOptions, Panel, PanelExtra, filter_note, row_styles, select_rows};
use crate::layer::{Layer, LinePattern, Mark, TextAnchor};
use crate::palette;
use crate::view::format_value;

use super::radial::{closed, panel_point, round_panel};
use super::single_panel_frame;

/// Height of the unit equilateral triangle, `√3/2`.
pub const TERNARY_HEIGHT: f64 = 0.866_025_403_784_438_6;

/// Position of the composition `p` (shares summing to 1) in the triangle:
/// `(p₁ + p₂/2, p₂·√3/2)` with corners `(0, 0)`, `(1, 0)` and `(½, √3/2)`
/// for the first, second and third component.
#[must_use]
pub fn ternary_position(p: [f64; 3]) -> [f64; 2] {
    [p[1] + p[2] / 2.0, p[2] * TERNARY_HEIGHT]
}

/// What a ternary panel needs besides its points.
#[derive(Debug, Clone, PartialEq)]
pub struct TernaryGeometry {
    /// The component columns, in corner order (lower left, lower right, top).
    pub names: [String; 3],
}

/// The frame of a ternary plot: the first three continuous columns on the
/// axis shelves (horizontal, then vertical; each once) as components. Each
/// row's values are divided by their sum; rows with a missing or negative
/// value, or a zero total, are left out (and noted), as are further columns.
/// Colour, shape, size, subgroup and label columns work as on any panel.
///
/// # Errors
/// [`stat_core::StatError::InvalidArgument`] with fewer than three
/// continuous columns on the axis shelves, or when the plan does not fit the
/// table.
pub fn build_ternary_frame(
    table: &DataTable,
    state: &RowStateManager,
    plan: &EncodingPlan,
    options: &FrameOptions,
) -> Result<Frame> {
    plan.validate(table)?;
    let mut columns: Vec<&String> = Vec::new();
    for c in plan.x.iter().chain(&plan.y) {
        if !columns.contains(&c) && is_continuous(table, c) {
            columns.push(c);
        }
    }
    if columns.len() < 3 {
        return Err(invalid("a ternary plot needs three continuous component columns on the axis shelves"));
    }
    let mut notes = Vec::new();
    if columns.len() > 3 {
        let extra: Vec<&str> = columns[3..].iter().map(|c| c.as_str()).collect();
        notes.push(format!("Only the first three components are plotted; left out: {}.", extra.join(", ")));
    }
    let data: Vec<&[f64]> = columns[..3]
        .iter()
        .map(|c| table.find(c).and_then(|i| table.column(i)).and_then(|c| c.as_numeric()).unwrap_or(&[]))
        .collect();
    let rs = select_rows(table, state, plan, options);
    let style = options.style.for_panel(0);
    let styles = row_styles(table, state, plan, &rs.rows, &style);
    let (mut dropped, mut rescaled) = (0, 0);
    let mut points = Vec::new();
    for &r in &rs.page_rows {
        let v: Vec<f64> = data.iter().map(|d| d.get(r).copied().unwrap_or(f64::NAN)).collect();
        let total: f64 = v.iter().sum();
        if v.iter().any(|x| !x.is_finite() || *x < 0.0) || !(total.is_finite() && total > 0.0) {
            dropped += 1;
            continue;
        }
        if (total - 1.0).abs() > 1e-6 {
            rescaled += 1;
        }
        let at = ternary_position([v[0] / total, v[1] / total, v[2] / total]);
        points.push(panel_point(r, at, &styles, &rs, state));
    }
    if rescaled > 0 {
        notes.push(format!("{rescaled} row(s) do not sum to 1: each is divided by its total."));
    }
    if dropped > 0 {
        notes.push(format!("{dropped} row(s) with a missing or negative component, or a zero total, are left out."));
    }
    notes.extend(filter_note(state, options, &rs.passes));
    let names = [columns[0].clone(), columns[1].clone(), columns[2].clone()];
    let mut panel = round_panel(points, styles.groups, PanelExtra::Ternary(TernaryGeometry { names }));
    panel.labels = crate::frame::point_labels(table, plan, &panel.points);
    Ok(single_panel_frame(panel, styles.legend, notes, rs.pages, rs.page))
}

/// Draws the triangle of a ternary panel with gridlines every 0.1, share
/// labels every 0.2 along the edges and the component names at the corners
/// (the points layer draws the rows).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TernaryElement;

impl Element for TernaryElement {
    fn kind(&self) -> ElementKind {
        ElementKind::Ternary
    }

    fn layer(&self, panel: &Panel, _state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        let PanelExtra::Ternary(g) = &panel.extra else { return layer };
        let pos = ternary_position;
        let mut segments = Vec::new();
        for k in 1..10 {
            let t = k as f64 / 10.0;
            segments.push([pos([t, 1.0 - t, 0.0]), pos([t, 0.0, 1.0 - t])]);
            segments.push([pos([1.0 - t, t, 0.0]), pos([0.0, t, 1.0 - t])]);
            segments.push([pos([1.0 - t, 0.0, t]), pos([0.0, 1.0 - t, t])]);
        }
        layer.marks.push(Mark::Segments {
            segments,
            color: palette::LINE.with_alpha(0.2),
            width: 0.8,
            name: String::new(),
        });
        layer.marks.push(Mark::Line {
            points: closed(vec![pos([1.0, 0.0, 0.0]), pos([0.0, 1.0, 0.0]), pos([0.0, 0.0, 1.0])]),
            color: palette::LINE,
            width: 1.3,
            pattern: LinePattern::Solid,
            name: String::new(),
        });
        // Outward normals of the left, bottom and right edges.
        let (left, bottom, right) = ([-0.866, 0.5], [0.0, -1.0], [0.866, 0.5]);
        let shift = |p: [f64; 2], n: [f64; 2], d: f64| [p[0] + d * n[0], p[1] + d * n[1]];
        let text = |at: [f64; 2], text: String| Mark::Text {
            at,
            text,
            color: palette::LABEL,
            anchor: TextAnchor::Centre,
            movable: None,
        };
        for k in [2, 4, 6, 8] {
            let t = k as f64 / 10.0;
            let label = format_value(t);
            layer.marks.push(text(shift(pos([t, 0.0, 1.0 - t]), left, 0.05), label.clone()));
            layer.marks.push(text(shift(pos([1.0 - t, t, 0.0]), bottom, 0.05), label.clone()));
            layer.marks.push(text(shift(pos([0.0, 1.0 - t, t]), right, 0.05), label));
        }
        layer.marks.push(text([-0.06, -0.08], g.names[0].clone()));
        layer.marks.push(text([1.06, -0.08], g.names[1].clone()));
        layer.marks.push(text([0.5, TERNARY_HEIGHT + 0.07], g.names[2].clone()));
        layer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    #[test]
    fn ternary_closes_compositions() {
        let t = DataTable::new(
            "t",
            vec![
                Column::numeric("sand", vec![1.0, 0.0, 0.0, 20.0, 1.0, -1.0]),
                Column::numeric("silt", vec![0.0, 1.0, 0.0, 30.0, f64::NAN, 1.0]),
                Column::numeric("clay", vec![0.0, 0.0, 1.0, 50.0, 1.0, 1.0]),
                Column::numeric("extra", vec![1.0; 6]),
            ],
        )
        .unwrap();
        let state = RowStateManager::new(6);
        let plan = EncodingPlan {
            x: vec!["sand".into(), "silt".into()],
            y: vec!["clay".into(), "extra".into()],
            ..Default::default()
        };
        let f = build_ternary_frame(&t, &state, &plan, &FrameOptions::default()).unwrap();
        let p = &f.panels[0].points;
        assert_eq!(p.len(), 4);
        assert_eq!([p[0].x, p[0].y], [0.0, 0.0]);
        assert_eq!([p[1].x, p[1].y], [1.0, 0.0]);
        assert_eq!([p[2].x, p[2].y], [0.5, TERNARY_HEIGHT]);
        // (0.2, 0.3, 0.5) after dividing by 100.
        assert!((p[3].x - 0.55).abs() < 1e-12 && (p[3].y - 0.5 * TERNARY_HEIGHT).abs() < 1e-12);
        assert_eq!(f.notes.len(), 3, "{:?}", f.notes);
        assert!((TERNARY_HEIGHT - 3f64.sqrt() / 2.0).abs() < 1e-15);
        let layer = TernaryElement.layer(&f.panels[0], &state);
        assert!(layer.all_finite());
        assert!(layer.marks.iter().any(|m| matches!(m, Mark::Text { text, .. } if text == "clay")));
        let two = EncodingPlan { x: vec!["sand".into(), "silt".into()], ..Default::default() };
        assert!(build_ternary_frame(&t, &state, &two, &FrameOptions::default()).is_err());
    }
}
