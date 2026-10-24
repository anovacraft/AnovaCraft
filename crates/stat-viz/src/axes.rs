//! A graph's own axis settings: titles typed by the user (read as math
//! markup, [`crate::math_text`]), axis breaks on continuous axes, and a
//! second vertical axis on the right of each panel.
//!
//! **Axis breaks.** A break `[a, b]` removes that stretch of a continuous
//! axis: values up to `a` are drawn where they were, values from `b` on are
//! drawn `b − a` lower, so the two pieces meet at `a`, and the spine there
//! carries the usual pair of slanted break marks (Cleveland, *The Elements
//! of Graphing Data*, rev. ed., 1994, §2.4, on showing a scale break).
//! Points inside the removed stretch cannot be placed and are left out (a
//! note says how many); lines run on across the break, and areas (bars,
//! bands) are cut at its edges. The break is given in data units and placed
//! on the axis's scale (on a log axis, `[a, b]` is removed between `log a`
//! and `log b`). Statistics are computed from the data before the break is
//! applied, so they do not change. An axis can have several breaks, and
//! each piece between them a scale and a share of the axis of its own
//! ([`crate::axis_breaks`], which builds the piecewise map).
//!
//! **Right axis.** Columns on the right-axis shelf are drawn in every panel
//! against a scale of their own, shown on the right side (spine, ticks,
//! title). The layers are built exactly as for the left axis (one frame per
//! right-axis column, aligned panel by panel with the main frame) and then
//! drawn through the linear map that takes the right range onto the left
//! one. So that the axis a series belongs to is never told by color alone,
//! right-axis lines are dashed, ungrouped right-axis markers take the next
//! palette color *and* shape, grouped ones are drawn hollow, and every key
//! entry for them ends in "(right axis)".

use stat_data::{DataTable, RowStateManager};

use crate::element::GraphSpec;
use crate::frame::{AxisSpec, Frame, Panel};
use crate::layer::{KeyEntry, KeyGlyph, Layer, LinePattern, Mark, MarkShape, Rgba};
use crate::scale::AxisScale;

/// A stretch of a continuous axis that is left out, in data units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisBreak {
    /// Lower end (data units).
    pub from: f64,
    /// Upper end (data units).
    pub to: f64,
}

impl AxisBreak {
    /// The break on an axis drawn on `scale`, in drawn units, lower end
    /// first; `None` when either end is outside the scale's domain or the
    /// ends coincide.
    #[must_use]
    pub fn drawn(&self, scale: AxisScale) -> Option<(f64, f64)> {
        let (a, b) = (scale.forward(self.from)?, scale.forward(self.to)?);
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        (lo.is_finite() && hi.is_finite() && hi > lo).then_some((lo, hi))
    }
}

/// Drawn position of `v` with the stretch `(lo, hi)` removed: unchanged up
/// to `lo`, `hi − lo` lower from `hi` on, NaN inside.
#[must_use]
pub fn break_map(v: f64, (lo, hi): (f64, f64)) -> f64 {
    if v <= lo {
        v
    } else if v >= hi {
        v - (hi - lo)
    } else {
        f64::NAN
    }
}

/// [`break_map`] for areas: values inside the break go to its edge.
#[must_use]
pub fn break_clamp(v: f64, (lo, hi): (f64, f64)) -> f64 {
    if v <= lo {
        v
    } else if v >= hi {
        v - (hi - lo)
    } else {
        lo
    }
}

/// The value drawn at position `u` of a broken axis (the inverse of
/// [`break_map`]; the break point itself reads as its lower end).
#[must_use]
pub fn break_unmap(u: f64, (lo, hi): (f64, f64)) -> f64 {
    if u <= lo { u } else { u + (hi - lo) }
}

/// Titles, breaks, markup and the plot style of a graph (what its Plot
/// settings set).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GraphAxes {
    /// Title above the graph replacing the default ("Y by X"); read as markup.
    pub title: Option<String>,
    /// Horizontal axis title replacing the column name; read as markup.
    pub x_title: Option<String>,
    /// Vertical axis title; read as markup.
    pub y_title: Option<String>,
    /// Right axis title; read as markup.
    pub y2_title: Option<String>,
    /// Breaks of the horizontal axis.
    pub x_breaks: crate::axis_breaks::AxisBreaks,
    /// Breaks of the (left) vertical axis.
    pub y_breaks: crate::axis_breaks::AxisBreaks,
    /// Read markup in titles made from column names, in key entries and in
    /// text labels too (off: those are shown exactly as written).
    pub markup_labels: bool,
    /// The rest of the graph's plot style (ticks, grid, number format, key,
    /// equal scale, …), without the titles and breaks above; `None` while
    /// everything is at its default.
    pub style: Option<crate::plot_style::PlotStyle>,
}

impl GraphAxes {
    /// `true` when nothing is set.
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// A title: the custom one (always markup) or the default (markup when
    /// [`GraphAxes::markup_labels`] is on).
    #[must_use]
    pub fn title_text(&self, custom: Option<&str>, default: &str) -> crate::math_text::MathText {
        match custom {
            Some(t) => crate::math_text::MathText::parse(t),
            None => crate::math_text::MathText::new(default, self.markup_labels),
        }
    }

    /// The map of a broken axis, if it has breaks and is continuous;
    /// `extent` is the data's extent on it in drawn units (for segment
    /// shares and scales).
    #[must_use]
    pub fn break_of(&self, axis: &AxisSpec, vertical: bool, extent: Option<(f64, f64)>) -> Option<Gap> {
        if axis.is_categorical() || axis.time.is_some() {
            return None;
        }
        let b = if vertical { &self.y_breaks } else { &self.x_breaks };
        if b.is_empty() {
            return None;
        }
        b.map(axis.scale, extent)
    }
}

/// Apply the panel's axis breaks to `layer` (drawn units in, broken drawn
/// units out). Returns how many markers fell inside a break.
pub(crate) fn apply_breaks(layer: &mut Layer, x: Option<&Gap>, y: Option<&Gap>) -> usize {
    if x.is_none() && y.is_none() {
        return 0;
    }
    let point = |p: [f64; 2]| [x.map_or(p[0], |b| b.map(p[0])), y.map_or(p[1], |b| b.map(p[1]))];
    let area = |p: [f64; 2]| [x.map_or(p[0], |b| b.clamp(p[0])), y.map_or(p[1], |b| b.clamp(p[1]))];
    let before: usize = markers(layer);
    layer.map_coordinates(&point, &area);
    before - markers(layer)
}

fn markers(layer: &Layer) -> usize {
    layer.marks.iter().map(|m| if let Mark::Points { points, .. } = m { points.len() } else { 0 }).sum()
}

/// The note for markers hidden by a break.
pub(crate) fn break_note(hidden: usize) -> String {
    format!("{hidden} point(s) inside an axis break are not drawn.")
}

/// The map of a broken axis ([`crate::axis_breaks::BrokenAxis`]).
pub type Gap = crate::axis_breaks::BrokenAxis;

/// Everything drawn against the right axis.
#[derive(Debug, Clone, PartialEq)]
pub struct RightAxis {
    /// One frame per right-axis column, its panels aligned index by index
    /// with the main frame's.
    pub frames: Vec<Frame>,
    /// `layers[column][panel][element]`, in right-axis drawn units.
    pub layers: Vec<Vec<Vec<Layer>>>,
    /// The default range of the right axis (shared by every panel).
    pub range: (f64, f64),
    /// The right axis (its first column's, with the scale).
    pub axis: AxisSpec,
    /// Default title: the columns' names.
    pub title: String,
    /// Key entries telling the series apart: the left column and each
    /// right-axis column with its marker (when not split by groups).
    pub series: Vec<KeyEntry>,
    /// Notes (e.g. a categorical column left off the right axis).
    pub notes: Vec<String>,
}

impl RightAxis {
    /// The map from right-axis drawn units onto the left axis's, for a
    /// panel showing `left` on the left and `right` on the right.
    pub fn to_left(left: (f64, f64), right: (f64, f64)) -> impl Fn(f64) -> f64 {
        let k = if right.1 > right.0 { (left.1 - left.0) / (right.1 - right.0) } else { 1.0 };
        move |v| left.0 + (v - right.0) * k
    }

    /// The layers of panel `panel` mapped into the left axis's coordinates,
    /// for views `left` and `right` of the two vertical axes.
    #[must_use]
    pub fn panel_layers(&self, panel: usize, left: (f64, f64), right: (f64, f64)) -> Vec<Layer> {
        let f = Self::to_left(left, right);
        let map = |p: [f64; 2]| [p[0], f(p[1])];
        self.layers
            .iter()
            .filter_map(|per_panel| per_panel.get(panel))
            .flatten()
            .map(|l| {
                let mut l = l.clone();
                l.map_coordinates(&map, &map);
                l
            })
            .collect()
    }

    /// Where each row of panel `panel` is drawn on the right axis, mapped
    /// into the left axis's coordinates (for box and click selection).
    #[must_use]
    pub fn row_positions(
        &self,
        spec: &GraphSpec,
        panel: usize,
        left: (f64, f64),
        right: (f64, f64),
    ) -> Vec<(usize, [f64; 2])> {
        let f = Self::to_left(left, right);
        self.frames
            .iter()
            .filter_map(|fr| fr.panels.get(panel))
            .flat_map(|p| spec.row_positions(p))
            .map(|(r, p)| (r, [p[0], f(p[1])]))
            .collect()
    }

    /// The panels of right-axis column `k`.
    #[must_use]
    pub fn panels(&self, k: usize) -> &[Panel] {
        self.frames.get(k).map_or(&[], |f| f.panels.as_slice())
    }
}

/// Marker color and shape of the `k`-th right-axis column when the panels
/// are not split by groups: the palette's style after the left column's.
#[must_use]
pub fn right_style(k: usize) -> (Rgba, MarkShape) {
    crate::palette::level_style(k + 1)
}

/// Restyle a right-axis layer: dashed lines, hollow markers when `hollow`,
/// "(right axis)" after each key entry, notes starting "Right axis:".
fn restyle(layer: &mut Layer, hollow: bool) {
    for m in &mut layer.marks {
        match m {
            Mark::Line { pattern, .. } if *pattern == LinePattern::Solid => *pattern = LinePattern::Dashed,
            Mark::Points { filled, .. } if hollow => *filled = false,
            _ => {}
        }
    }
    for n in &mut layer.notes {
        *n = format!("Right axis: {n}");
    }
    for e in &mut layer.key {
        e.label = format!("{} (right axis)", e.label);
        match &mut e.glyph {
            KeyGlyph::Line(p) if *p == LinePattern::Solid => *p = LinePattern::Dashed,
            KeyGlyph::Marker(_, filled) if hollow => *filled = false,
            _ => {}
        }
    }
}

impl GraphSpec {
    /// What the right-axis shelf draws, or `None` without right-axis
    /// columns (or in a whole-panel chart form, which has no right axis).
    /// Categorical columns are left off the right axis with a note.
    ///
    /// # Errors
    /// As [`crate::frame::build_frame`].
    pub fn right_axis(&self, table: &DataTable, state: &RowStateManager) -> stat_core::Result<Option<RightAxis>> {
        if self.encoding.y2.is_empty() || self.form != crate::forms::GraphForm::Panels {
            return Ok(None);
        }
        let mut notes = Vec::new();
        let columns: Vec<&String> = self
            .encoding
            .y2
            .iter()
            .filter(|c| {
                let ok = crate::encoding::is_continuous(table, c);
                if !ok {
                    notes.push(format!("{c} is not continuous: the right axis takes continuous columns only."));
                }
                ok
            })
            .collect();
        if columns.is_empty() {
            return Ok(None);
        }
        let grouped = self.encoding.color.is_some() || self.encoding.overlay.is_some() || self.encoding.shape.is_some();
        let left_rows = self.encoding.y.len().max(1);
        let mut frames = Vec::new();
        let mut layers = Vec::new();
        let mut series = Vec::new();
        if !grouped {
            let color = self.options.style.color.unwrap_or(crate::palette::DEFAULT_POINT);
            let shape = self.options.style.shape.unwrap_or(MarkShape::Circle);
            let name = self.encoding.y.join(", ");
            if !name.is_empty() {
                series.push(KeyEntry {
                    label: format!("{name} (left axis)"),
                    color,
                    glyph: KeyGlyph::Marker(shape, true),
                });
            }
        }
        for (k, column) in columns.iter().enumerate() {
            let encoding = crate::encoding::EncodingPlan {
                y: vec![(*column).clone(); left_rows],
                y2: Vec::new(),
                ..self.encoding.clone()
            };
            let mut options = self.options.clone();
            if !grouped {
                let (color, shape) = right_style(k);
                options.style.color = Some(color);
                options.style.shape = Some(shape);
                series.push(KeyEntry {
                    label: format!("{column} (right axis)"),
                    color,
                    glyph: KeyGlyph::Marker(shape, true),
                });
            }
            let frame = crate::frame::build_frame(table, state, &encoding, &options)?;
            let spec = GraphSpec {
                encoding,
                options,
                // Reference lines belong to the left axis only.
                axes: crate::axes::GraphAxes {
                    y_breaks: crate::axis_breaks::AxisBreaks::default(),
                    style: self.axes.style.clone().map(|s| crate::plot_style::PlotStyle {
                        reference: crate::plot_style::ReferenceLook::default(),
                        ..s
                    }),
                    ..self.axes.clone()
                },
                ..self.clone()
            };
            let mut per_panel = spec.layers(&frame, state);
            for l in per_panel.iter_mut().flatten() {
                restyle(l, grouped);
            }
            frames.push(frame);
            layers.push(per_panel);
        }
        // One range for every right-axis column.
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for p in layers.iter().flatten().flatten().flat_map(|l| l.marks.iter()).flat_map(|m| m.coordinates()) {
            lo = lo.min(p[1]);
            hi = hi.max(p[1]);
        }
        let axis = frames[0].panels.first().map_or_else(
            || AxisSpec {
                name: columns[0].clone(),
                kind: crate::frame::AxisKind::Continuous,
                scale: self.options.y_scale,
                time: None,
            },
            |p| AxisSpec { name: columns.iter().map(|c| c.as_str()).collect::<Vec<_>>().join(", "), ..p.y.clone() },
        );
        let range = match axis.time {
            Some(t) => crate::time_scale::time_range(lo, hi, t.format),
            None => crate::view::auto_range(lo, hi),
        };
        let title = axis.title();
        Ok(Some(RightAxis { frames, layers, range, axis, title, series, notes }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoding::EncodingPlan;
    use stat_data::Column;

    #[test]
    fn breaks_map_piecewise() {
        let b = (10.0, 90.0);
        assert_eq!(break_map(5.0, b), 5.0);
        assert_eq!(break_map(100.0, b), 20.0);
        assert!(break_map(50.0, b).is_nan());
        assert_eq!(break_clamp(50.0, b), 10.0);
        assert_eq!(break_unmap(20.0, b), 100.0);
        assert_eq!(break_unmap(break_map(95.0, b), b), 95.0);
        assert_eq!(AxisBreak { from: 90.0, to: 10.0 }.drawn(AxisScale::Linear), Some((10.0, 90.0)));
        let log = AxisBreak { from: 10.0, to: 1000.0 }.drawn(AxisScale::Log10).unwrap();
        assert!((log.0 - 1.0).abs() < 1e-12 && (log.1 - 3.0).abs() < 1e-12);
        assert_eq!(AxisBreak { from: -1.0, to: 10.0 }.drawn(AxisScale::Log10), None);
        assert_eq!(AxisBreak { from: 3.0, to: 3.0 }.drawn(AxisScale::Linear), None);
    }

    fn table() -> DataTable {
        DataTable::new(
            "t",
            vec![
                Column::numeric("x", vec![1.0, 2.0, 3.0, 4.0]),
                Column::numeric("y", vec![1.0, 2.0, 500.0, 3.0]),
                Column::numeric("z", vec![100.0, 300.0, 200.0, 400.0]),
                Column::text("g", ["a", "b", "a", "b"].map(String::from).to_vec()),
            ],
        )
        .unwrap()
    }

    /// A break moves the upper data down, hides markers inside it and leaves
    /// statistics alone; without a break every layer is unchanged.
    #[test]
    fn breaks_apply_to_layers_and_positions() {
        let t = table();
        let state = RowStateManager::new(4);
        let mut spec = GraphSpec {
            encoding: EncodingPlan { x: vec!["x".into()], y: vec!["y".into()], ..Default::default() },
            ..GraphSpec::default()
        };
        let frame = spec.frame(&t, &state).unwrap();
        let plain = spec.layers(&frame, &state);
        spec.axes.y_breaks = crate::AxisBreaks::single(4.0, 450.0);
        let broken = spec.layers(&frame, &state);
        let ys = |layers: &Vec<Vec<Layer>>| -> Vec<f64> {
            layers[0][0].marks.iter().flat_map(|m| m.coordinates()).map(|p| p[1]).collect()
        };
        let (a, b) = (ys(&plain), ys(&broken));
        assert!(a.contains(&500.0) && b.contains(&54.0), "{b:?}");
        let pos = spec.row_positions(&frame.panels[0]);
        assert!(pos.iter().any(|(r, p)| *r == 2 && (p[1] - 54.0).abs() < 1e-9));
        // Inside the break: hidden, with a note.
        spec.axes.y_breaks = crate::AxisBreaks::single(1.5, 450.0);
        let hidden = spec.layers(&frame, &state);
        assert!(
            hidden[0][0].notes.iter().any(|n| n.contains("2 point(s) inside an axis break")),
            "{:?}",
            hidden[0][0].notes
        );
        assert_eq!(spec.row_positions(&frame.panels[0]).len(), 2);
        spec.axes = GraphAxes::default();
        assert_eq!(spec.layers(&frame, &state), plain);
    }

    /// A graph with two breaks, a log segment and shares: the frame says the
    /// segments' scales differ, every row keeps a finite drawn position, the
    /// pieces take their shares of the data's extent, and ticks are labelled
    /// from each piece's own scale.
    #[test]
    fn breaks_with_segment_scales_and_shares() {
        let t = table();
        let state = RowStateManager::new(4);
        let mut spec = GraphSpec {
            encoding: EncodingPlan { x: vec!["x".into()], y: vec!["z".into()], ..Default::default() },
            ..GraphSpec::default()
        };
        spec.axes.y_breaks = crate::AxisBreaks {
            breaks: vec![AxisBreak { from: 110.0, to: 190.0 }, AxisBreak { from: 310.0, to: 390.0 }],
            segments: vec![
                crate::AxisSegment { scale: Some(AxisScale::Log10), share: Some(50.0) },
                crate::AxisSegment::default(),
                crate::AxisSegment { scale: None, share: Some(20.0) },
            ],
            mark: crate::BreakMark::Zigzag,
        };
        let frame = spec.frame(&t, &state).unwrap();
        assert!(
            frame.notes.iter().any(|n| n.contains("log (base 10) below 110") && n.contains("linear above 390")),
            "{:?}",
            frame.notes
        );
        let (_, by) = spec.panel_breaks(&frame.panels[0]);
        let by = by.expect("a broken vertical axis");
        assert_eq!(by.mark, crate::BreakMark::Zigzag);
        let pos = spec.row_positions(&frame.panels[0]);
        assert_eq!(pos.len(), 4);
        // z = 100, 300, 200, 400: the log piece holds 100 only, so its share
        // is drawn from 100 to 110 (half the axis), the top piece from 390
        // to 400 takes a fifth.
        let y = |row: usize| pos.iter().find(|(r, _)| *r == row).unwrap().1[1];
        let total = y(3) - y(0);
        assert!(((by.map(110.0) - y(0)) / total - 0.5).abs() < 1e-9);
        assert!(((y(3) - by.map(390.0)) / total - 0.2).abs() < 1e-9);
        assert!(by.label_at(&frame.panels[0].y, by.map(300.0)).starts_with("300"));
    }

    #[test]
    fn right_axis_is_aligned_restyled_and_keyed() {
        let t = table();
        let state = RowStateManager::new(4);
        let mut spec = GraphSpec {
            encoding: EncodingPlan {
                x: vec!["x".into()],
                y: vec!["y".into()],
                y2: vec!["z".into()],
                ..Default::default()
            },
            line: Some(crate::element::LineElement::default()),
            ..GraphSpec::default()
        };
        let right = spec.right_axis(&t, &state).unwrap().unwrap();
        assert_eq!(right.frames.len(), 1);
        assert_eq!(right.frames[0].panels.len(), spec.frame(&t, &state).unwrap().panels.len());
        assert_eq!(right.title, "z");
        assert!(right.range.0 <= 100.0 && right.range.1 >= 400.0);
        let labels: Vec<&str> = right.series.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, ["y (left axis)", "z (right axis)"]);
        let (c, s) = right_style(0);
        assert!(matches!(right.series[1].glyph, KeyGlyph::Marker(shape, true) if shape == s));
        assert_eq!(right.series[1].color, c);
        let layers = &right.layers[0][0];
        assert!(
            layers.iter().flat_map(|l| &l.marks).any(|m| matches!(m, Mark::Line { pattern: LinePattern::Dashed, .. }))
        );
        assert!(layers.iter().flat_map(|l| &l.marks).any(|m| matches!(m, Mark::Points { shape, .. } if *shape == s)));
        // Mapped into the left range: the right range's ends land on the left's.
        let mapped = right.panel_layers(0, (0.0, 10.0), right.range);
        let ys: Vec<f64> = mapped.iter().flat_map(|l| &l.marks).flat_map(|m| m.coordinates()).map(|p| p[1]).collect();
        assert!(ys.iter().all(|y| (0.0..=10.0).contains(y)), "{ys:?}");
        let rows = right.row_positions(&spec, 0, (0.0, 10.0), right.range);
        assert_eq!(rows.len(), 4);
        // Grouped: hollow markers and labelled key entries, no series key.
        spec.encoding.color = Some("g".into());
        let right = spec.right_axis(&t, &state).unwrap().unwrap();
        assert!(right.series.is_empty());
        let marks: Vec<&Mark> = right.layers[0][0].iter().flat_map(|l| &l.marks).collect();
        assert!(marks.iter().any(|m| matches!(m, Mark::Points { filled: false, .. })));
        // A categorical column is left off with a note.
        spec.encoding.y2 = vec!["g".into()];
        assert!(spec.right_axis(&t, &state).unwrap().is_none());
        spec.encoding.y2.clear();
        assert!(spec.right_axis(&t, &state).unwrap().is_none());
    }
}
