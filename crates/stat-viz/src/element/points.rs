//! Raw data points.

use std::collections::BTreeMap;

use stat_data::RowStateManager;

use super::{Element, ElementKind};
use crate::frame::{Panel, PanelPoint};
use crate::layer::{KeyGlyph, Layer, Mark, MarkShape, Rgba};
use crate::palette;
use crate::stats::{JitterKind, MAX_SWARM_POINTS, jitter_offset, swarm_offsets};

/// Raw data points: jittered along categorical axes, colored, shaped and
/// sized by the encoding; excluded rows as gray crosses; rows outside the
/// graph's filter (when drawn) hollow; selected rows ringed; optionally, while
/// anything is selected, unselected points are drawn lighter (still clearly
/// visible) so the selection stands out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointsElement {
    /// Jitter along categorical axes.
    pub jitter: JitterKind,
    /// Jitter half-width, in category slots (0.5 = touching neighbours).
    pub jitter_width: f64,
    /// Fade unselected points while there is a selection.
    pub fade_unselected: bool,
    /// With a label column, label only the selected rows.
    pub label_selected_only: bool,
}

impl Default for PointsElement {
    fn default() -> Self {
        Self { jitter: JitterKind::Uniform, jitter_width: 0.25, fade_unselected: false, label_selected_only: false }
    }
}

/// A point style: (color, shape, radius in half points, faded, hollow).
type StyleKey = (u32, u8, u32, bool, bool);
/// The points sharing a style, with the style to draw them in.
type StyleBucket = (Rgba, MarkShape, f32, bool, Vec<[f64; 2]>);

/// Most point labels drawn for all rows (beyond this only selected rows).
pub const MAX_POINT_LABELS: usize = 300;

/// Opacity of unselected points while some rows are selected.
pub const FADED_ALPHA: f64 = 0.55;

/// The plot area (width, height in screen points) a swarm is laid out for:
/// the layer does not know the size of the window it is drawn in, so the
/// swarm assumes a panel about this size (the canvas's default) and markers
/// just touch there; in a much smaller panel they overlap a little, in a
/// larger one small gaps open.
pub const SWARM_PANEL: [f64; 2] = [480.0, 300.0];

impl PointsElement {
    /// Drawn position of `p`, jittered along categorical axes (within the
    /// subgroup's slot).
    #[must_use]
    pub fn position(&self, panel: &Panel, p: &PanelPoint) -> [f64; 2] {
        let slot = panel.slot_half_width.min(0.5);
        let width = self.jitter_width.clamp(0.0, 1.0) * slot / 0.4;
        let dx = if panel.x.is_categorical() { jitter_offset(self.jitter, width, p.row, 0) } else { 0.0 };
        let dy = if panel.y.is_categorical() { jitter_offset(self.jitter, width.min(0.4), p.row, 1) } else { 0.0 };
        [p.x + dx, p.y + dy]
    }

    /// Drawn position of every point of `panel` (in the order of
    /// `panel.points`). With [`JitterKind::Swarm`] on a panel with one
    /// categorical axis, the points of each slot (category, or subgroup
    /// within it) are laid out by [`swarm_offsets`] in screen points of a
    /// [`SWARM_PANEL`]-sized plot, then converted to category units; a
    /// swarm wider than the jitter width is narrowed to fit (markers may
    /// then overlap). Slots with more than [`MAX_SWARM_POINTS`] points, and
    /// panels with two categorical or two continuous axes, use uniform jitter
    /// or none as usual.
    #[must_use]
    pub fn positions(&self, panel: &Panel) -> Vec<[f64; 2]> {
        let mut out: Vec<[f64; 2]> = panel.points.iter().map(|p| self.position(panel, p)).collect();
        if self.jitter != JitterKind::Swarm {
            return out;
        }
        let across = match (panel.x.is_categorical(), panel.y.is_categorical()) {
            (true, false) => 0,
            (false, true) => 1,
            _ => return out,
        };
        let along = 1 - across;
        let (lo, hi) = out
            .iter()
            .map(|p| p[along])
            .filter(|v| v.is_finite())
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)));
        if !lo.is_finite() {
            return out;
        }
        let (a, b) = crate::view::auto_range(lo, hi);
        let value_per_point = ((b - a) / SWARM_PANEL[along]).max(f64::MIN_POSITIVE);
        let levels = if across == 0 { panel.x.levels().len() } else { panel.y.levels().len() }.max(1);
        let across_per_point = levels as f64 / SWARM_PANEL[across];
        let slot = panel.slot_half_width.min(0.5);
        let width = self.jitter_width.clamp(0.0, 1.0) * slot / 0.4;
        let limit = if across == 0 { width } else { width.min(0.4) };
        let mut slots: BTreeMap<i64, Vec<usize>> = BTreeMap::new();
        for (i, p) in panel.points.iter().enumerate() {
            let centre = if across == 0 { p.x } else { p.y };
            if centre.is_finite() {
                slots.entry(crate::element::position_key(centre, 1e9)).or_default().push(i);
            }
        }
        for members in slots.values() {
            if members.len() > MAX_SWARM_POINTS {
                for &i in members {
                    let p = &panel.points[i];
                    out[i][across] += jitter_offset(JitterKind::Uniform, limit, p.row, across as u64);
                }
                continue;
            }
            let values: Vec<f64> = members.iter().map(|&i| out[i][along] / value_per_point).collect();
            let radii: Vec<f64> = members.iter().map(|&i| f64::from(panel.points[i].radius)).collect();
            let offsets = swarm_offsets(&values, &radii);
            let widest = offsets.iter().fold(0.0_f64, |m, o| m.max(o.abs())) * across_per_point;
            let squeeze = if widest > limit && widest > 0.0 { limit / widest } else { 1.0 };
            for (&i, o) in members.iter().zip(offsets) {
                out[i][across] += o * across_per_point * squeeze;
            }
        }
        out
    }
}

impl Element for PointsElement {
    fn kind(&self) -> ElementKind {
        ElementKind::Points
    }

    fn layer(&self, panel: &Panel, state: &RowStateManager) -> Layer {
        let any_selected = self.fade_unselected && state.counts().selected > 0;
        let selected = |r: usize| r < state.len() && state.is_selected(r);
        // Bucket by style: (color, shape, radius in half points, faded).
        let mut buckets: BTreeMap<StyleKey, StyleBucket> = BTreeMap::new();
        let (mut excluded, mut rings) = (Vec::new(), Vec::new());
        let drawn = self.positions(panel);
        for (p, &at) in panel.points.iter().zip(&drawn) {
            if selected(p.row) {
                rings.push((at, p.radius));
            }
            if p.excluded {
                excluded.push(at);
                continue;
            }
            let faded = any_selected && !selected(p.row);
            // Rows outside the graph's filter (still drawn) are hollow.
            let hollow = p.filtered;
            let key = (u32::from_be_bytes(p.color.0), p.shape as u8, (p.radius * 2.0).round() as u32, faded, hollow);
            let entry = buckets.entry(key).or_insert_with(|| {
                let color = if faded { p.color.with_alpha(FADED_ALPHA) } else { p.color };
                (color, p.shape, (p.radius * 2.0).round() / 2.0, hollow, Vec::new())
            });
            entry.4.push(at);
        }
        let mut marks: Vec<Mark> = Vec::new();
        // Faded points first, so full-strength ones draw over them.
        let mut ordered: Vec<_> = buckets.into_iter().collect();
        ordered.sort_by_key(|(k, _)| !k.3);
        for (_, (color, shape, radius, hollow, points)) in ordered {
            let name = if hollow { "Outside the filter" } else { "Rows" }.to_owned();
            marks.push(Mark::Points { points, color, shape, radius, filled: !hollow, name });
        }
        if !excluded.is_empty() {
            let color = if any_selected { palette::EXCLUDED.with_alpha(0.5) } else { palette::EXCLUDED };
            marks.push(Mark::Points {
                points: excluded,
                color,
                shape: MarkShape::Cross,
                radius: 4.0,
                filled: true,
                name: "Excluded".to_owned(),
            });
        }
        // Rings sized to each marker; grouped by radius.
        let mut by_radius: BTreeMap<u32, Vec<[f64; 2]>> = BTreeMap::new();
        for (at, r) in rings {
            by_radius.entry(((r + 3.5) * 2.0).round() as u32).or_default().push(at);
        }
        for (r2, points) in by_radius {
            marks.push(Mark::Points {
                points,
                color: palette::SELECTED,
                shape: MarkShape::Circle,
                radius: r2 as f32 / 2.0,
                filled: false,
                name: "Selected".to_owned(),
            });
        }
        let mut layer = Layer { marks, ..Layer::default() };
        // Point labels: every labelled row, or (when asked, or when there are
        // too many to read) the selected ones.
        if !panel.labels.is_empty() {
            let too_many = panel.labels.len() > MAX_POINT_LABELS;
            let only_selected = self.label_selected_only || too_many;
            if too_many && !self.label_selected_only {
                layer
                    .notes
                    .push(format!("More than {MAX_POINT_LABELS} rows have labels: only selected rows are labelled."));
            }
            for (p, &at) in panel.points.iter().zip(&drawn) {
                if only_selected && !selected(p.row) {
                    continue;
                }
                if let Some(text) = panel.labels.get(&p.row) {
                    layer.marks.push(Mark::Text {
                        at,
                        text: format!("  {text}"),
                        color: palette::LABEL,
                        anchor: crate::layer::TextAnchor::LeftOfText,
                        movable: None,
                    });
                }
            }
        }
        if layer.marks.iter().any(|m| matches!(m, Mark::Points { name, .. } if name == "Outside the filter")) {
            layer.key_entry(
                "Outside the filter (left out of analyses)",
                palette::DEFAULT_POINT,
                KeyGlyph::Marker(MarkShape::Circle, false),
            );
        }
        if layer.marks.iter().any(|m| matches!(m, Mark::Points { name, .. } if name == "Excluded")) {
            layer.key_entry("Excluded", palette::EXCLUDED, KeyGlyph::Marker(MarkShape::Cross, false));
        }
        layer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoding::EncodingPlan;
    use crate::frame::{FrameOptions, build_frame};
    use stat_data::{Column, DataTable};

    /// A swarm keeps every category's points apart (in the nominal panel's
    /// screen points), inside the category's slot, and box selection sees
    /// the same positions as the drawing.
    #[test]
    fn swarm_layout_separates_points_within_their_slot() {
        let groups: Vec<String> = (0..60).map(|i| if i % 2 == 0 { "A" } else { "B" }.to_owned()).collect();
        let values: Vec<f64> = (0..60).map(|i| 10.0 + f64::from((i * 7) % 13) * 0.1).collect();
        let t = DataTable::new("s", vec![Column::text("g", groups), Column::numeric("y", values)]).unwrap();
        let state = RowStateManager::new(60);
        let plan = EncodingPlan { x: vec!["g".into()], y: vec!["y".into()], ..Default::default() };
        let frame = build_frame(&t, &state, &plan, &FrameOptions::default()).unwrap();
        let panel = &frame.panels[0];
        let swarm = PointsElement { jitter: JitterKind::Swarm, jitter_width: 0.45, ..PointsElement::default() };
        let at = swarm.positions(panel);
        assert_eq!(at.len(), panel.points.len());
        let (lo, hi) = crate::view::auto_range(10.0, 11.2);
        let (sx, sy) = (SWARM_PANEL[0] / 2.0, SWARM_PANEL[1] / (hi - lo));
        for i in 0..at.len() {
            let p = &panel.points[i];
            assert!((at[i][0] - p.x).abs() <= 0.45 * panel.slot_half_width / 0.4 + 1e-9, "inside the slot");
            assert_eq!(at[i][1], p.y, "values are not moved");
            for j in i + 1..at.len() {
                if (panel.points[j].x - p.x).abs() < 1e-9 {
                    let d = (((at[i][0] - at[j][0]) * sx).powi(2) + ((at[i][1] - at[j][1]) * sy).powi(2)).sqrt();
                    assert!(d >= 2.0 * f64::from(p.radius) - 1e-6, "{i} and {j} overlap");
                }
            }
        }
        let spec = crate::GraphSpec { points: Some(swarm), ..crate::GraphSpec::default() };
        let rows: Vec<[f64; 2]> = spec.row_positions(panel).into_iter().map(|(_, p)| p).collect();
        assert_eq!(rows, at);
        // Two continuous axes: the swarm does nothing.
        let plan = EncodingPlan { x: vec!["y".into()], y: vec!["y".into()], ..Default::default() };
        let frame = build_frame(&t, &state, &plan, &FrameOptions::default()).unwrap();
        let p = &frame.panels[0];
        assert!(swarm.positions(p).iter().zip(&p.points).all(|(a, q)| a[0] == q.x && a[1] == q.y));
    }
}
