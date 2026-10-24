//! Toolkit-independent helpers for showing a [`Frame`] interactively: shared
//! default axis ranges for linked panels, the drawn position of every row (for
//! box and click selection), and tick labels on transformed scales.

use std::collections::BTreeMap;

use crate::element::GraphSpec;
use crate::frame::{AxisSpec, Frame, Panel};
use crate::layer::Layer;
use crate::scale::AxisScale;

/// Fraction of the data range added beyond the data on each side of a
/// continuous axis.
pub const RANGE_MARGIN: f64 = 0.06;

/// Default range of a continuous axis showing data from `data_min` to
/// `data_max`:
///
/// * the data padded by [`RANGE_MARGIN`] of their span on each side;
/// * when no value is negative, the lower end is never below 0 (padding
///   stops at the origin rather than showing negative values that cannot
///   occur), while data with negative values (residuals, deviations) are
///   padded freely;
/// * a single value `v` gets `[0, 1.15 v]` (`v > 0`), `[0, 1]` (`v = 0`) or
///   `[1.15 v, 0]` (`v < 0`);
/// * `[0, 1]` when there are no finite data.
///
/// The result is always finite (clamped to ±`f64::MAX` for data near the
/// limits of binary64).
#[must_use]
pub fn auto_range(data_min: f64, data_max: f64) -> (f64, f64) {
    let (lo, hi) = if data_min <= data_max { (data_min, data_max) } else { (data_max, data_min) };
    if !(lo.is_finite() && hi.is_finite()) {
        return (0.0, 1.0);
    }
    // Results are kept finite for data near the limits of binary64.
    let finite = |v: f64| v.clamp(-f64::MAX, f64::MAX);
    if hi - lo <= f64::EPSILON * hi.abs().max(lo.abs()).max(f64::MIN_POSITIVE) {
        return match lo {
            v if v > 0.0 => (0.0, finite(v * 1.15)),
            v if v < 0.0 => (finite(v * 1.15), 0.0),
            _ => (0.0, 1.0),
        };
    }
    // The span itself can overflow (e.g. −10³⁰⁸ to 10³⁰⁸); its parts cannot.
    let span = hi - lo;
    let pad = if span.is_finite() { span * RANGE_MARGIN } else { hi * RANGE_MARGIN - lo * RANGE_MARGIN };
    let min = if lo >= 0.0 { (lo - pad).max(0.0) } else { finite(lo - pad) };
    (min, finite(hi + pad))
}

/// Default ranges of every axis of a frame, keyed by axis name, so that
/// panels showing the same column share one range (linked panning and
/// rescaling across small multiples).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SharedRanges {
    /// Horizontal axes: name → `(min, max)` in drawn (scaled) units.
    pub x: BTreeMap<String, (f64, f64)>,
    /// Vertical axes.
    pub y: BTreeMap<String, (f64, f64)>,
}

impl SharedRanges {
    /// Ranges covering everything every panel draws (`layers[panel][element]`).
    /// A categorical axis with `L` levels spans `0.5 ..= L + 0.5`; a continuous
    /// axis spans its drawn coordinates as [`auto_range`] pads them.
    #[must_use]
    pub fn of(frame: &Frame, layers: &[Vec<Layer>]) -> Self {
        // Per axis name: running (min, max), or the level count of a categorical axis.
        let mut acc: [BTreeMap<String, Span>; 2] = [BTreeMap::new(), BTreeMap::new()];
        for (i, panel) in frame.panels.iter().enumerate() {
            let panel_layers = layers.get(i).map_or(&[][..], Vec::as_slice);
            for (d, axis) in [&panel.x, &panel.y].into_iter().enumerate() {
                if axis.is_categorical() {
                    let n = axis.levels().len().max(1);
                    let entry = acc[d].entry(axis.name.clone()).or_insert(Span::Levels(n));
                    if let Span::Levels(m) = entry {
                        *m = (*m).max(n);
                    }
                    continue;
                }
                let time = axis.time.map(|t| t.format);
                let entry =
                    acc[d].entry(axis.name.clone()).or_insert(Span::Values(f64::INFINITY, f64::NEG_INFINITY, time));
                if let Span::Values(lo, hi, _) = entry {
                    for p in panel_layers.iter().flat_map(|l| l.marks.iter()).flat_map(|m| m.coordinates()) {
                        *lo = lo.min(p[d]);
                        *hi = hi.max(p[d]);
                    }
                }
            }
        }
        let [x, y] = acc.map(|m| m.into_iter().map(|(k, s)| (k, s.range())).collect());
        Self { x, y }
    }
}

/// What an axis must show.
enum Span {
    /// Data from the first to the second value, with the date format of a
    /// date-time axis.
    Values(f64, f64, Option<stat_data::TemporalFormat>),
    /// This many categorical levels.
    Levels(usize),
}

impl Span {
    fn range(self) -> (f64, f64) {
        match self {
            Span::Levels(n) => (0.5, n as f64 + 0.5),
            Span::Values(lo, hi, Some(format)) => crate::time_scale::time_range(lo, hi, format),
            Span::Values(lo, hi, None) => auto_range(lo, hi),
        }
    }
}

impl GraphSpec {
    /// Where each row of `panel` is drawn (with the points layer's jitter, or
    /// the default jitter when points are off): `(row, [x, y])`. Box and click
    /// selection test these positions, so a selection always matches what is
    /// on screen.
    #[must_use]
    pub fn row_positions(&self, panel: &Panel) -> Vec<(usize, [f64; 2])> {
        let points = self.points.unwrap_or_default();
        let (bx, by) = self.panel_breaks(panel);
        let broken = |v: f64, b: Option<&crate::axes::Gap>| b.map_or(v, |b| b.map(v));
        panel
            .points
            .iter()
            .zip(points.positions(panel))
            .map(|(p, at)| (p.row, [broken(at[0], bx.as_ref()), broken(at[1], by.as_ref())]))
            .filter(|(_, at)| at[0].is_finite() && at[1].is_finite())
            .collect()
    }
}

/// Rows whose drawn position lies inside the box `[x0, x1] × [y0, y1]`
/// (corners in any order), ascending without duplicates.
#[must_use]
pub fn rows_in_box(positions: &[(usize, [f64; 2])], a: [f64; 2], b: [f64; 2]) -> Vec<usize> {
    let (x0, x1) = (a[0].min(b[0]), a[0].max(b[0]));
    let (y0, y1) = (a[1].min(b[1]), a[1].max(b[1]));
    let mut rows: Vec<usize> = positions
        .iter()
        .filter(|(_, p)| p[0] >= x0 && p[0] <= x1 && p[1] >= y0 && p[1] <= y1)
        .map(|(r, _)| *r)
        .collect();
    rows.sort_unstable();
    rows.dedup();
    rows
}

/// Rows whose drawn position lies inside the closed polygon `outline` (a
/// lasso; the last vertex joins the first), by the even–odd crossing rule.
/// Fewer than three vertices enclose nothing. Ascending without duplicates.
#[must_use]
pub fn rows_in_polygon(positions: &[(usize, [f64; 2])], outline: &[[f64; 2]]) -> Vec<usize> {
    if outline.len() < 3 {
        return Vec::new();
    }
    let mut rows: Vec<usize> =
        positions.iter().filter(|(_, p)| point_in_polygon(*p, outline)).map(|(r, _)| *r).collect();
    rows.sort_unstable();
    rows.dedup();
    rows
}

/// Rows of the topmost region (bar, cell, tile) of `layers` containing the
/// drawn position `p`, if any. Later layers and later regions are on top.
#[must_use]
pub fn region_rows(layers: &[crate::layer::Layer], p: [f64; 2]) -> Option<Vec<usize>> {
    layers
        .iter()
        .rev()
        .flat_map(|l| l.regions.iter().rev())
        .find(|r| point_in_polygon(p, &r.outline))
        .map(|r| r.rows.clone())
}

/// Even–odd test: does a ray to the right of `p` cross the outline an odd
/// number of times?
#[must_use]
pub fn point_in_polygon(p: [f64; 2], outline: &[[f64; 2]]) -> bool {
    let mut inside = false;
    let n = outline.len();
    for i in 0..n {
        let (a, b) = (outline[i], outline[(i + n - 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
            if p[0] < x {
                inside = !inside;
            }
        }
    }
    inside
}

/// Label of the tick at drawn position `t` on `axis`: the level name on a
/// categorical axis (blank between levels), else the data value
/// (`scale.inverse(t)`) to about four significant digits.
#[must_use]
pub fn tick_label(axis: &AxisSpec, t: f64) -> String {
    if axis.is_categorical() {
        let k = t.round();
        if (t - k).abs() > 1e-6 || k < 1.0 {
            return String::new();
        }
        return axis.levels().get(k as usize - 1).cloned().unwrap_or_default();
    }
    let v = axis.scale.inverse(t);
    if !v.is_finite() {
        return String::new();
    }
    format_value(v)
}

/// A compact number: integers as such, otherwise four significant digits,
/// switching to exponent form for very large or small magnitudes.
#[must_use]
pub fn format_value(v: f64) -> String {
    if v == 0.0 {
        return "0".to_owned();
    }
    let a = v.abs();
    if !(1e-4..1e7).contains(&a) {
        return format!("{v:.3e}");
    }
    if (v - v.round()).abs() < 1e-9 * a.max(1.0) {
        return format!("{}", v.round() as i64);
    }
    let digits = (3 - a.log10().floor() as i32).max(0) as usize;
    let s = format!("{v:.digits$}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_owned() } else { s }
}

/// Tick positions (in drawn units) for a transformed continuous axis spanning
/// `lo ..= hi` in drawn units: "nice" data values mapped through the scale —
/// 1, 2, 5 × 10ᵏ (or fewer per decade on long ranges) on log and reciprocal
/// scales, familiar proportions on a logit scale, and an even 1-2-5 step on a
/// square-root scale. Linear axes return an empty list (their ticks come from
/// the plot's own spacer).
#[must_use]
pub fn scaled_ticks(scale: AxisScale, lo: f64, hi: f64) -> Vec<f64> {
    if scale == AxisScale::Linear || !(lo.is_finite() && hi.is_finite()) || hi <= lo {
        return Vec::new();
    }
    let within = |out: &mut Vec<f64>, v: f64| {
        if let Some(t) = scale.forward(v) {
            if t >= lo && t <= hi {
                out.push(t);
            }
        }
    };
    let mut out = Vec::new();
    match scale {
        AxisScale::Log10 | AxisScale::Ln | AxisScale::Reciprocal => {
            // Data range; on the reciprocal scale drawn positions at or above
            // zero stand for ever larger values, so cap the range there.
            let (dlo, dhi) = if scale == AxisScale::Reciprocal {
                if lo >= 0.0 {
                    return Vec::new();
                }
                let dlo = scale.inverse(lo);
                (dlo, if hi < 0.0 { scale.inverse(hi) } else { dlo * 1e12 })
            } else {
                (scale.inverse(lo), scale.inverse(hi))
            };
            if !(dlo.is_finite() && dhi.is_finite() && dhi > 0.0) {
                return Vec::new();
            }
            let k0 = dlo.max(f64::MIN_POSITIVE).log10().floor() as i32;
            let k1 = (dhi.log10().ceil() as i32).min(k0 + 14);
            let decades = k1 - k0;
            let mults: &[f64] = if decades > 6 {
                &[1.0]
            } else if decades > 2 {
                &[1.0, 3.0]
            } else {
                &[1.0, 2.0, 5.0]
            };
            for k in k0.max(-300)..=k1.min(300) {
                for m in mults {
                    within(&mut out, m * 10f64.powi(k));
                }
            }
            out.sort_by(f64::total_cmp);
        }
        AxisScale::Logit => {
            // Coarse to fine sets of proportions; the finest set that gives at
            // most nine ticks in view.
            let tiers: [&[f64]; 3] = [
                &[0.0001, 0.001, 0.01, 0.1, 0.5, 0.9, 0.99, 0.999, 0.9999],
                &[0.05, 0.25, 0.75, 0.95],
                &[0.02, 0.2, 0.3, 0.4, 0.6, 0.7, 0.8, 0.98],
            ];
            for tier in tiers {
                let mut next = out.clone();
                for &v in tier {
                    within(&mut next, v);
                }
                if out.is_empty() || next.len() <= 9 {
                    out = next;
                } else {
                    break;
                }
            }
            out.sort_by(f64::total_cmp);
        }
        _ => return even_data_ticks(scale, lo, hi),
    }
    // A short range on a log-like scale: fall back to an even step.
    if out.len() < 3 {
        let even = even_data_ticks(scale, lo, hi);
        if even.len() > out.len() {
            return even;
        }
    }
    out
}

/// About six ticks at an even 1-2-5 step in data units, mapped through the
/// scale (the square-root scale, and short ranges on the others).
fn even_data_ticks(scale: AxisScale, lo: f64, hi: f64) -> Vec<f64> {
    let (a, b) = (scale.inverse(lo), scale.inverse(hi));
    if !(a.is_finite() && b.is_finite()) {
        return Vec::new();
    }
    let (dlo, dhi) = (a.min(b), a.max(b));
    let raw = (dhi - dlo) / 6.0;
    if raw.is_nan() || raw <= 0.0 {
        return Vec::new();
    }
    let p = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0].into_iter().map(|m| m * p).find(|s| *s >= raw).unwrap_or(10.0 * p);
    // Allow for rounding in the round trip through the scale.
    let slack = 1e-9 * step;
    let tol = 1e-12 * (hi - lo);
    let mut out = Vec::new();
    let mut k = ((dlo - slack) / step).ceil();
    while k * step <= dhi + slack && out.len() < 50 {
        if let Some(t) = scale.forward(k * step) {
            if t >= lo - tol && t <= hi + tol {
                out.push(t);
            }
        }
        k += 1.0;
    }
    out.sort_by(f64::total_cmp);
    out
}

/// Where the user has put a movable label (see [`crate::layer::Mark::Text`]):
/// panel index, label identifier, and position as a fraction of the panel's
/// view (0–1 across from the left and up from the bottom), so it stays in the
/// same place on the plot when the axes are dragged.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelPosition {
    /// Index of the panel in the frame.
    pub panel: usize,
    /// The label's identifier.
    pub id: String,
    /// Fraction of the view across and up.
    pub at: [f64; 2],
}

/// `layers` of panel `panel` with every moved label placed where the user put
/// it inside `view` (`([x0, x1], [y0, y1])` in drawn units). Labels without a
/// stored place keep their computed position.
#[must_use]
pub fn place_labels(
    layers: &[Layer],
    positions: &[LabelPosition],
    panel: usize,
    view: ([f64; 2], [f64; 2]),
) -> Vec<Layer> {
    let mut out = layers.to_vec();
    if !positions.iter().any(|p| p.panel == panel) {
        return out;
    }
    let ([x0, x1], [y0, y1]) = view;
    for layer in &mut out {
        for mark in &mut layer.marks {
            if let crate::layer::Mark::Text { at, movable: Some(id), .. } = mark {
                if let Some(p) = positions.iter().rev().find(|p| p.panel == panel && p.id == *id) {
                    *at = [x0 + p.at[0] * (x1 - x0), y0 + p.at[1] * (y1 - y0)];
                }
            }
        }
    }
    out
}

/// Where `point` (drawn units) sits as a fraction of `view`.
#[must_use]
pub fn view_fraction(point: [f64; 2], view: ([f64; 2], [f64; 2])) -> [f64; 2] {
    let ([x0, x1], [y0, y1]) = view;
    let f = |v: f64, a: f64, b: f64| if (b - a).abs() > 0.0 { (v - a) / (b - a) } else { 0.5 };
    [f(point[0], x0, x1), f(point[1], y0, y1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_ranges_pad_and_respect_the_origin() {
        let close = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-12 && (a.1 - b.1).abs() < 1e-12;
        // 6% each side.
        assert!(close(auto_range(60.0, 80.0), (58.8, 81.2)));
        // Non-negative data: padding stops at 0.
        assert!(close(auto_range(0.5, 10.0), (0.0, 10.57)));
        assert!(close(auto_range(0.0, 50.0), (0.0, 53.0)));
        // Negative values (residuals): free padding.
        assert!(close(auto_range(-2.0, 3.0), (-2.3, 3.3)));
        assert!(close(auto_range(-5.0, -1.0), (-5.24, -0.76)));
        // Single values.
        assert!(close(auto_range(4.0, 4.0), (0.0, 4.6)));
        assert!(close(auto_range(0.0, 0.0), (0.0, 1.0)));
        assert!(close(auto_range(-2.0, -2.0), (-2.3, 0.0)));
        // Nothing drawn; reversed arguments.
        assert_eq!(auto_range(f64::INFINITY, f64::NEG_INFINITY), (0.0, 1.0));
        assert!(close(auto_range(80.0, 60.0), (58.8, 81.2)));
    }

    #[test]
    fn auto_ranges_stay_finite_at_the_limits_of_binary64() {
        // The span −10³⁰⁸ … 1.5·10³⁰⁸ overflows; the padded range must not.
        let (lo, hi) = auto_range(-1e308, 1.5e308);
        assert!(lo.is_finite() && hi.is_finite() && lo < -1e308 && hi > 1.5e308, "({lo}, {hi})");
        let (lo, hi) = auto_range(-1.7e308, 1.7e308);
        assert!(lo == -f64::MAX && hi == f64::MAX, "({lo}, {hi})");
        assert_eq!(auto_range(1.6e308, 1.6e308), (0.0, f64::MAX));
        assert_eq!(auto_range(-1.6e308, -1.6e308), (-f64::MAX, 0.0));
    }
    use crate::encoding::EncodingPlan;
    use crate::frame::{AxisKind, FrameOptions, build_frame};
    use stat_data::{Column, DataTable, RowStateManager};

    fn table() -> DataTable {
        DataTable::new(
            "t",
            vec![
                Column::numeric("x", vec![1.0, 2.0, 3.0, 4.0]),
                Column::numeric("y", vec![10.0, 20.0, 30.0, 40.0]),
                Column::text("g", vec!["a".into(), "b".into(), "a".into(), "b".into()]),
            ],
        )
        .unwrap()
    }

    #[test]
    fn shared_ranges_link_panels() {
        let t = table();
        let state = RowStateManager::new(4);
        let plan = EncodingPlan {
            x: vec!["x".into()],
            y: vec!["y".into()],
            group_x: Some("g".into()),
            ..EncodingPlan::default()
        };
        let spec = GraphSpec { encoding: plan.clone(), ..GraphSpec::default() };
        let frame = build_frame(&t, &state, &plan, &FrameOptions::default()).unwrap();
        assert_eq!(frame.panels.len(), 2);
        let layers = spec.layers(&frame, &state);
        let r = SharedRanges::of(&frame, &layers);
        let (lo, hi) = r.x["x"];
        assert!(lo < 1.0 && hi > 4.0, "both panels' x values are covered: {lo} {hi}");
        let (lo, hi) = r.y["y"];
        assert!(lo < 10.0 && hi > 40.0);

        // A categorical axis spans its levels.
        let plan = EncodingPlan { x: vec!["g".into()], y: vec!["y".into()], ..EncodingPlan::default() };
        let frame = build_frame(&t, &state, &plan, &FrameOptions::default()).unwrap();
        let r = SharedRanges::of(&frame, &spec.layers(&frame, &state));
        assert_eq!(r.x["g"], (0.5, 2.5));
    }

    #[test]
    fn positions_and_box_selection() {
        let t = table();
        let state = RowStateManager::new(4);
        let plan = EncodingPlan { x: vec!["g".into()], y: vec!["y".into()], ..EncodingPlan::default() };
        let spec = GraphSpec { encoding: plan.clone(), ..GraphSpec::default() };
        let frame = build_frame(&t, &state, &plan, &FrameOptions::default()).unwrap();
        let pos = spec.row_positions(&frame.panels[0]);
        assert_eq!(pos.len(), 4);
        // Jittered, but within the level's slot.
        assert!(pos.iter().all(|(_, p)| (p[0] - p[0].round()).abs() <= 0.4));
        let level_a = rows_in_box(&pos, [0.5, 0.0], [1.5, 100.0]);
        assert_eq!(level_a, [0, 2]);
        assert_eq!(rows_in_box(&pos, [2.5, 100.0], [0.5, 25.0]), [2, 3], "corners in any order");
        assert!(rows_in_box(&pos, [5.0, 0.0], [6.0, 1.0]).is_empty());
    }

    #[test]
    fn lasso_selection() {
        let pos = vec![(0, [0.5, 0.5]), (1, [2.0, 2.5]), (2, [1.5, 0.2]), (3, [0.2, 1.6])];
        // A triangle covering the lower-left half of the unit-ish square.
        let tri = [[0.0, 0.0], [2.0, 0.0], [0.0, 2.0]];
        assert_eq!(rows_in_polygon(&pos, &tri), [0, 2, 3]);
        // A concave "C" shape leaves out its notch.
        let c = [[0.0, 0.0], [3.0, 0.0], [3.0, 1.0], [1.0, 1.0], [1.0, 2.0], [3.0, 2.0], [3.0, 3.0], [0.0, 3.0]];
        assert_eq!(rows_in_polygon(&pos, &c), [0, 1, 2, 3]);
        assert!(!point_in_polygon([2.0, 1.5], &c));
        assert!(rows_in_polygon(&pos, &tri[..2]).is_empty());
    }

    #[test]
    fn tick_labels() {
        let cat = AxisSpec {
            name: "g".into(),
            kind: AxisKind::Categorical { levels: vec!["a".into(), "b".into()] },
            scale: AxisScale::Linear,
            time: None,
        };
        assert_eq!(tick_label(&cat, 1.0), "a");
        assert_eq!(tick_label(&cat, 2.0), "b");
        assert_eq!(tick_label(&cat, 1.5), "");
        assert_eq!(tick_label(&cat, 3.0), "");
        assert_eq!(tick_label(&cat, 0.0), "");
        let log = AxisSpec { name: "x".into(), kind: AxisKind::Continuous, scale: AxisScale::Log10, time: None };
        assert_eq!(tick_label(&log, 2.0), "100");
        assert_eq!(tick_label(&log, -1.0), "0.1");
        assert_eq!(format_value(1234.5678), "1235");
        assert_eq!(format_value(0.012345), "0.01235");
        assert_eq!(format_value(-2.5), "-2.5");
        assert_eq!(format_value(3e9), "3.000e9");
    }

    #[test]
    fn scaled_tick_positions() {
        assert!(scaled_ticks(AxisScale::Linear, 0.0, 1.0).is_empty());
        let t = scaled_ticks(AxisScale::Log10, 0.0, 3.0);
        assert!(t.contains(&0.0) && t.contains(&3.0) && t.len() >= 4, "{t:?}");
        let t = scaled_ticks(AxisScale::Ln, 0.0, 10f64.ln());
        assert!(t.iter().all(|v| (0.0..=10f64.ln() + 1e-12).contains(v)) && t.len() >= 3);
        let t = scaled_ticks(AxisScale::Sqrt, 0.0, 10.0);
        assert!(t.len() >= 4 && t.iter().all(|v| v.is_finite()));
        let t = scaled_ticks(AxisScale::Logit, AxisScale::Logit.forward(0.02).unwrap(), 3.0);
        assert!(t.contains(&0.0) && t.len() >= 3 && t.len() <= 9, "{t:?}");
        let r = AxisScale::Reciprocal;
        let t = scaled_ticks(r, r.forward(0.5).unwrap(), r.forward(100.0).unwrap());
        assert!(t.len() >= 3 && t.windows(2).all(|w| w[0] < w[1]), "{t:?}");
        assert!(!scaled_ticks(r, -1.0, 0.5).is_empty(), "a range reaching past zero still gets ticks");
        assert!(scaled_ticks(r, 0.0, 1.0).is_empty());
        // Less than a decade: an even step instead of one lonely tick.
        let t = scaled_ticks(r, r.forward(17.0).unwrap(), r.forward(30.0).unwrap());
        assert!(t.len() >= 3, "{t:?}");
        let t = scaled_ticks(AxisScale::Log10, 17f64.log10(), 30f64.log10());
        assert!(t.len() >= 3, "{t:?}");
        assert!(scaled_ticks(AxisScale::Log10, f64::NAN, 1.0).is_empty());
        assert!(scaled_ticks(AxisScale::Log10, 2.0, 1.0).is_empty());
    }
}
