//! Ridgeline plot (joy plot): the distribution of one continuous column in
//! each group as a Gaussian kernel density (the same estimator as the violin
//! layer, [`crate::density::kde_1d`] with Silverman's bandwidth per group),
//! the groups stacked one above another on a shared horizontal axis and
//! overlapping slightly so many groups fit (Wilke, *Fundamentals of Data
//! Visualization*, 2019, ch. 9 "Visualizing many distributions at once").
//!
//! All ridges share one height scale (the tallest peak reaches the chosen
//! height in category slots), so their areas are equal and their heights
//! compare directly. The rows themselves are drawn as a rug of ticks under
//! each ridge.

use stat_core::Result;
use stat_data::{DataTable, RowStateManager};

use crate::density::{kde_1d, silverman_bandwidth};
use crate::element::{Element, ElementKind};
use crate::encoding::{EncodingPlan, invalid, is_continuous};
use crate::frame::{
    AxisKind, AxisSpec, Frame, FrameOptions, Panel, PanelExtra, filter_note, levels_of, row_styles, select_rows,
};
use crate::layer::{KeyGlyph, Layer, LinePattern, Mark, MarkShape, Region, Rgba};
use crate::palette;

use super::radial::panel_point;
use super::single_panel_frame;

/// Points at which each density is evaluated.
const GRID_POINTS: usize = 161;

/// Settings of a ridgeline plot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RidgelineSettings {
    /// Height of the tallest peak, in percent of the spacing between ridges
    /// (100: touching; more overlaps).
    pub height: u16,
    /// Draw each row as a tick under its ridge.
    pub rug: bool,
}

impl Default for RidgelineSettings {
    fn default() -> Self {
        Self { height: 150, rug: true }
    }
}

/// One ridge.
#[derive(Debug, Clone, PartialEq)]
pub struct Ridge {
    /// Group level (empty for a single ridge).
    pub label: String,
    /// Baseline (vertical position of the group).
    pub base: f64,
    /// The density curve, `[x, base + scaled density]`, left to right.
    pub curve: Vec<[f64; 2]>,
    /// Bandwidth used (drawn units).
    pub bandwidth: f64,
    /// Rows in the group, ascending.
    pub rows: Vec<usize>,
    /// Fill colour.
    pub color: Rgba,
    /// The level's marker shape (for the key).
    pub shape: MarkShape,
}

/// The ridges of a ridgeline panel.
#[derive(Debug, Clone, PartialEq)]
pub struct RidgelineGeometry {
    /// Ridges from the top (first level) down.
    pub ridges: Vec<Ridge>,
    /// The chart's settings.
    pub settings: RidgelineSettings,
}

/// The frame of a ridgeline plot: the first horizontal column (continuous,
/// on the horizontal scale) for each level of the grouping column — the first
/// vertical column if categorical, else the subgroup column, else a
/// categorical colour column, else one ridge for all rows. The first level is
/// at the top. The vertical axis is categorical with a blank slot above the
/// top ridge so it has room to rise. Densities count each row's frequency ×
/// weight; excluded rows and rows outside an analysing filter are left out of
/// them but drawn in the rug.
///
/// # Errors
/// [`stat_core::StatError::InvalidArgument`] without a continuous horizontal
/// column, with a continuous vertical column, or when the plan does not fit
/// the table.
pub fn build_ridgeline_frame(
    table: &DataTable,
    state: &RowStateManager,
    plan: &EncodingPlan,
    options: &FrameOptions,
    settings: RidgelineSettings,
) -> Result<Frame> {
    plan.validate(table)?;
    let need = || {
        invalid(
            "a ridgeline plot needs a continuous column across and a categorical column up (or a colour or subgroup column)",
        )
    };
    let x_col = plan.x.first().filter(|c| is_continuous(table, c)).ok_or_else(need)?;
    if plan.y.first().is_some_and(|c| is_continuous(table, c)) {
        return Err(need());
    }
    let group_col =
        plan.y.first().or(plan.overlay.as_ref()).or(plan.color.as_ref().filter(|c| !is_continuous(table, c))).cloned();
    let values = table.find(x_col).and_then(|i| table.column(i)).and_then(|c| c.as_numeric()).unwrap_or(&[]);
    let scale = options.x_scale;
    let position = |r: usize| values.get(r).copied().filter(|v| v.is_finite()).and_then(|v| scale.forward(v));
    let rs = select_rows(table, state, plan, options);
    let style = options.style.for_panel(0);
    let styles = row_styles(table, state, plan, &rs.rows, &style);
    let (codes, levels) = match &group_col {
        Some(c) => levels_of(table, c, &rs.rows),
        None => (vec![Some(0); table.n_rows()], vec![String::new()]),
    };
    let n = levels.len().max(1);
    let base = |k: usize| (n - k) as f64;
    let excluded = |r: usize| r < state.len() && state.is_excluded(r);
    // Values and weights per group, for the densities.
    let mut data: Vec<(Vec<f64>, Vec<f64>, Vec<usize>)> = vec![(Vec::new(), Vec::new(), Vec::new()); levels.len()];
    let mut points = Vec::new();
    let mut dropped = 0;
    for &r in &rs.page_rows {
        match (position(r), codes[r]) {
            (Some(x), Some(k)) => {
                points.push(panel_point(r, [x, base(k)], &styles, &rs, state));
                let w = rs.freq[r] * rs.weight[r];
                data[k].2.push(r);
                if !rs.filtered[r] && !excluded(r) && w > 0.0 {
                    data[k].0.push(x);
                    data[k].1.push(w);
                }
            }
            _ => dropped += 1,
        }
    }
    let bandwidths: Vec<f64> = data.iter().map(|(v, w, _)| silverman_bandwidth(v, w)).collect();
    let widest = data.iter().zip(&bandwidths).filter(|(d, _)| !d.0.is_empty()).map(|(_, h)| *h).fold(0.0_f64, f64::max);
    let (lo, hi) = data
        .iter()
        .flat_map(|d| d.0.iter().copied())
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(x), b.max(x)));
    let densities: Vec<Vec<[f64; 2]>> = data
        .iter()
        .zip(&bandwidths)
        .map(|((v, w, _), &h)| {
            if v.is_empty() || !lo.is_finite() {
                Vec::new()
            } else {
                kde_1d(v, w, h, lo - 3.0 * widest, hi + 3.0 * widest, GRID_POINTS)
            }
        })
        .collect();
    let peak = densities.iter().flatten().map(|p| p[1]).fold(0.0_f64, f64::max);
    let factor =
        if peak > 0.0 && peak.is_finite() { f64::from(settings.height.clamp(20, 400)) / 100.0 / peak } else { 0.0 };
    let column = group_col.clone().unwrap_or_default();
    let ridges = levels
        .iter()
        .enumerate()
        .zip(densities)
        .zip(data)
        .map(|(((k, label), density), (_, _, rows))| {
            let (color, shape) = if group_col.is_some() {
                style.level(&column, label, k)
            } else {
                (style.color.unwrap_or(palette::DEFAULT_POINT), MarkShape::Circle)
            };
            Ridge {
                label: label.clone(),
                base: base(k),
                curve: density.iter().map(|p| [p[0], base(k) + factor * p[1]]).collect(),
                bandwidth: bandwidths[k],
                rows,
                color,
                shape,
            }
        })
        .collect();
    let mut notes = Vec::new();
    if dropped > 0 {
        notes.push(format!("{dropped} row(s) without a value or a group are left out."));
    }
    notes.extend(filter_note(state, options, &rs.passes));
    // Bottom to top: the last level first, then a blank slot for the top ridge.
    let mut slots: Vec<String> = levels.iter().rev().cloned().collect();
    slots.push(String::new());
    let panel = Panel {
        grid_row: 0,
        grid_col: 0,
        title: None,
        x: AxisSpec { name: x_col.clone(), kind: AxisKind::Continuous, scale, time: None },
        y: AxisSpec {
            name: column,
            kind: AxisKind::Categorical { levels: slots },
            scale: Default::default(),
            time: None,
        },
        points,
        groups: styles.groups,
        slot_half_width: 0.0,
        extra: PanelExtra::Ridgeline(RidgelineGeometry { ridges, settings }),
        labels: std::collections::BTreeMap::new(),
        data_extent: [None; 2],
    };
    Ok(single_panel_frame(panel, styles.legend, notes, rs.pages, rs.page))
}

/// Draws the ridges of a ridgeline panel from the top down (so lower ridges
/// overlap the ones behind them), each filled in its level's colour with its
/// outline, and the rug: a tick per row, the selected rows' ticks in the
/// selection colour on top. Clicking a ridge selects its group's rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RidgelineElement;

impl Element for RidgelineElement {
    fn kind(&self) -> ElementKind {
        ElementKind::Ridgeline
    }

    fn layer(&self, panel: &Panel, state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        let PanelExtra::Ridgeline(g) = &panel.extra else { return layer };
        for ridge in &g.ridges {
            let (Some(first), Some(last)) = (ridge.curve.first(), ridge.curve.last()) else { continue };
            let lower: Vec<[f64; 2]> = ridge.curve.iter().map(|p| [p[0], ridge.base]).collect();
            layer.marks.push(Mark::Band {
                lower,
                upper: ridge.curve.clone(),
                color: ridge.color.with_alpha(0.75),
                name: ridge.label.clone(),
            });
            layer.marks.push(Mark::Line {
                points: ridge.curve.clone(),
                color: ridge.color,
                width: 1.4,
                pattern: LinePattern::Solid,
                name: ridge.label.clone(),
            });
            layer.marks.push(Mark::Line {
                points: vec![[first[0], ridge.base], [last[0], ridge.base]],
                color: palette::LINE.with_alpha(0.4),
                width: 0.8,
                pattern: LinePattern::Solid,
                name: String::new(),
            });
            let mut outline = ridge.curve.clone();
            outline.push([last[0], ridge.base]);
            outline.push([first[0], ridge.base]);
            layer.regions.push(Region { outline, rows: ridge.rows.clone() });
        }
        if g.settings.rug {
            let tick = |p: &&crate::frame::PanelPoint| [[p.x, p.y], [p.x, p.y + 0.12]];
            let selected = |p: &&crate::frame::PanelPoint| p.row < state.len() && state.is_selected(p.row);
            let (sel, rest): (Vec<&crate::frame::PanelPoint>, Vec<_>) = panel.points.iter().partition(|p| selected(p));
            let (excluded, plain): (Vec<&crate::frame::PanelPoint>, Vec<_>) =
                rest.into_iter().partition(|p: &&crate::frame::PanelPoint| p.excluded);
            for (pts, color, width, name) in [
                (plain, palette::LINE.with_alpha(0.55), 1.0, "Rows"),
                (excluded, palette::EXCLUDED, 1.0, "Excluded"),
                (sel, palette::SELECTED, 2.2, "Selected rows"),
            ] {
                if !pts.is_empty() {
                    layer.marks.push(Mark::Segments {
                        segments: pts.iter().map(tick).collect(),
                        color,
                        width,
                        name: name.to_owned(),
                    });
                }
            }
            if panel.points.iter().any(|p| p.row < state.len() && state.is_selected(p.row)) {
                layer.key_entry("Selected rows (rug)", palette::SELECTED, KeyGlyph::Line(LinePattern::Solid));
            }
        }
        layer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::{Column, SelectionMode};

    #[test]
    fn ridges_stack_with_one_height_scale() {
        let g: Vec<String> = ["a", "a", "a", "b", "b", "b", "b", "c"].iter().map(|s| (*s).to_owned()).collect();
        let t = DataTable::new(
            "r",
            vec![Column::numeric("x", vec![1.0, 2.0, 3.0, 5.0, 5.5, 6.0, 6.5, f64::NAN]), Column::text("g", g)],
        )
        .unwrap();
        let mut state = RowStateManager::new(8);
        let plan = EncodingPlan { x: vec!["x".into()], y: vec!["g".into()], ..Default::default() };
        let f =
            build_ridgeline_frame(&t, &state, &plan, &FrameOptions::default(), RidgelineSettings::default()).unwrap();
        let p = &f.panels[0];
        assert_eq!(p.y.levels(), ["c", "b", "a", ""], "first level at the top, a blank slot above it");
        let PanelExtra::Ridgeline(geo) = &p.extra else { panic!() };
        assert_eq!(geo.ridges.iter().map(|r| r.base).collect::<Vec<_>>(), [3.0, 2.0, 1.0]);
        assert!(geo.ridges[2].curve.is_empty(), "c has no value");
        // The tallest peak is 1.5 slots above its base.
        let top = geo.ridges.iter().flat_map(|r| r.curve.iter().map(move |q| q[1] - r.base)).fold(0.0, f64::max);
        assert!((top - 1.5).abs() < 1e-12, "{top}");
        // Each scaled curve is the kernel density at Silverman's bandwidth.
        let h = silverman_bandwidth(&[1.0, 2.0, 3.0], &[1.0; 3]);
        assert!((geo.ridges[0].bandwidth - h).abs() < 1e-15);
        // Independently: the drawn heights are proportional to the Gaussian
        // kernel density (1/nh) Σ φ((x − xᵢ)/h), written out here.
        let density = |x: f64| {
            [1.0, 2.0, 3.0_f64]
                .iter()
                .map(|xi| (-0.5 * ((x - xi) / h).powi(2)).exp() / (2.0 * std::f64::consts::PI).sqrt())
                .sum::<f64>()
                / (3.0 * h)
        };
        let ridge = &geo.ridges[0];
        let ratios: Vec<f64> =
            ridge.curve.iter().filter(|q| density(q[0]) > 1e-3).map(|q| (q[1] - ridge.base) / density(q[0])).collect();
        assert!(ratios.len() > 20);
        assert!(ratios.iter().all(|r| (r / ratios[0] - 1.0).abs() < 1e-10), "{ratios:?}");
        assert_eq!(geo.ridges[1].rows, [3, 4, 5, 6]);
        assert!(f.notes.iter().any(|n| n.contains("1 row")));
        state.select([4], SelectionMode::Replace);
        let layer = RidgelineElement.layer(p, &state);
        assert!(layer.all_finite());
        assert_eq!(layer.regions.len(), 2);
        assert!(layer.marks.iter().any(
            |m| matches!(m, Mark::Segments { name, segments, .. } if name == "Selected rows" && segments.len() == 1)
        ));
        assert_eq!(crate::view::region_rows(&[layer], [5.75, 2.05]), Some(vec![3, 4, 5, 6]));
        // A continuous vertical column is refused; no group column gives one ridge.
        let bad = EncodingPlan { x: vec!["x".into()], y: vec!["x".into()], ..Default::default() };
        assert!(
            build_ridgeline_frame(&t, &state, &bad, &FrameOptions::default(), RidgelineSettings::default()).is_err()
        );
        let one = EncodingPlan { x: vec!["x".into()], ..Default::default() };
        let f =
            build_ridgeline_frame(&t, &state, &one, &FrameOptions::default(), RidgelineSettings::default()).unwrap();
        let PanelExtra::Ridgeline(geo) = &f.panels[0].extra else { panic!() };
        assert_eq!(geo.ridges.len(), 1);
    }
}
