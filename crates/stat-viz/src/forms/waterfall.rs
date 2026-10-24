//! Waterfall (bridge) chart: how signed contributions build a total. Each
//! category of the horizontal column contributes `vₖ = Σ fᵢyᵢ` (frequency ×
//! the vertical column over its rows); its bar floats from the running total
//! before it, `Sₖ₋₁ = v₁ + … + vₖ₋₁`, to `Sₖ`, and an optional last bar
//! stands from zero to the grand total. Categories keep the order in which
//! they first appear in the table (the order a bridge is usually written
//! in). Increases and decreases differ in colour, decreases are also hatched,
//! and every bar can carry its signed value (`+12`, `−5`), so the direction
//! never rests on colour alone.

use stat_core::Result;
use stat_data::{DataTable, RowStateManager};

use crate::element::{Element, ElementKind};
use crate::encoding::{EncodingPlan, invalid, is_continuous};
use crate::frame::{
    AxisKind, AxisSpec, Frame, FrameOptions, Legend, Panel, PanelExtra, filter_note, row_styles, select_rows,
};
use crate::layer::{KeyGlyph, Layer, LinePattern, Mark, Region, Rgba, TextAnchor};
use crate::palette;
use crate::scale::AxisScale;
use crate::view::format_value;

use super::radial::panel_point;
use super::single_panel_frame;

/// Settings of a waterfall chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaterfallSettings {
    /// End with a bar from zero to the grand total.
    pub total: bool,
    /// Write each bar's signed value.
    pub labels: bool,
}

impl Default for WaterfallSettings {
    fn default() -> Self {
        Self { total: true, labels: true }
    }
}

/// What a waterfall bar shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepKind {
    /// A contribution of zero or more.
    Increase,
    /// A negative contribution.
    Decrease,
    /// The grand total, from zero.
    Total,
}

/// One bar of a waterfall chart.
#[derive(Debug, Clone, PartialEq)]
pub struct WaterfallBar {
    /// Category (or the total's label).
    pub label: String,
    /// Horizontal slot (1, 2, …).
    pub x: f64,
    /// Running total before the bar (0 for the total bar).
    pub start: f64,
    /// Running total after it.
    pub end: f64,
    /// Rows the bar stands for, ascending.
    pub rows: Vec<usize>,
    /// Increase, decrease or total.
    pub kind: StepKind,
}

/// The bars of a waterfall panel.
#[derive(Debug, Clone, PartialEq)]
pub struct WaterfallGeometry {
    /// Bars left to right.
    pub bars: Vec<WaterfallBar>,
    /// The chart's settings.
    pub settings: WaterfallSettings,
}

/// The frame of a waterfall chart: a categorical horizontal axis with one
/// slot per category of the first horizontal column (in order of first
/// appearance), then a "Total" slot, and the running total up. Excluded
/// rows, rows outside an analysing filter and rows with a missing category or
/// value are left out of the sums (and noted). Each row's point sits in the
/// middle of its category's bar for box selection; clicking a bar selects
/// its rows.
///
/// # Errors
/// [`stat_core::StatError::InvalidArgument`] without a horizontal column and
/// a numeric vertical column, or when the plan does not fit the table.
pub fn build_waterfall_frame(
    table: &DataTable,
    state: &RowStateManager,
    plan: &EncodingPlan,
    options: &FrameOptions,
    settings: WaterfallSettings,
) -> Result<Frame> {
    plan.validate(table)?;
    let need = || invalid("a waterfall chart needs a category column across and a numeric contribution column up");
    let category = plan.x.first().ok_or_else(need)?;
    let value_col = plan.y.first().filter(|c| is_continuous(table, c)).ok_or_else(need)?;
    let cat = table.find(category).and_then(|i| table.column(i)).ok_or_else(need)?;
    let values = table.find(value_col).and_then(|i| table.column(i)).and_then(|c| c.as_numeric()).unwrap_or(&[]);
    let rs = select_rows(table, state, plan, options);
    let style = options.style.for_panel(0);
    let styles = row_styles(table, state, plan, &rs.rows, &style);
    // Categories in order of first appearance among the rows shown.
    let mut levels: Vec<String> = Vec::new();
    for &r in &rs.rows {
        let l = cat.level_text(r).trim().to_owned();
        if !l.is_empty() && !levels.contains(&l) {
            levels.push(l);
        }
    }
    let excluded = |r: usize| r < state.len() && state.is_excluded(r);
    let mut sums: Vec<(f64, Vec<usize>)> = vec![(0.0, Vec::new()); levels.len()];
    let mut dropped = 0;
    for &r in rs.page_rows.iter().filter(|&&r| !rs.filtered[r] && !excluded(r) && rs.freq[r] > 0.0) {
        let label = cat.level_text(r);
        let v = values.get(r).copied().unwrap_or(f64::NAN);
        match levels.iter().position(|l| *l == label.trim()) {
            Some(k) if v.is_finite() => {
                sums[k].0 += rs.freq[r] * v;
                sums[k].1.push(r);
            }
            _ => dropped += 1,
        }
    }
    let mut bars = Vec::new();
    let mut running = 0.0;
    let mut all_rows = Vec::new();
    for (k, (v, rows)) in sums.into_iter().enumerate() {
        let start = running;
        running += v;
        all_rows.extend(rows.iter().copied());
        let kind = if v < 0.0 { StepKind::Decrease } else { StepKind::Increase };
        bars.push(WaterfallBar { label: levels[k].clone(), x: (k + 1) as f64, start, end: running, rows, kind });
    }
    if !running.is_finite() || bars.iter().any(|b| !b.end.is_finite()) {
        return Err(invalid("the running total is too large to draw (it overflows)"));
    }
    let mut slots = levels.clone();
    if settings.total {
        let label = if levels.iter().any(|l| l == "Total") { "Total (all)" } else { "Total" }.to_owned();
        all_rows.sort_unstable();
        bars.push(WaterfallBar {
            label: label.clone(),
            x: (levels.len() + 1) as f64,
            start: 0.0,
            end: running,
            rows: all_rows,
            kind: StepKind::Total,
        });
        slots.push(label);
    }
    let points = bars
        .iter()
        .filter(|b| b.kind != StepKind::Total)
        .flat_map(|b| b.rows.iter().map(move |&r| (r, [b.x, 0.5 * (b.start + b.end)])))
        .map(|(r, at)| panel_point(r, at, &styles, &rs, state))
        .collect();
    let mut notes = Vec::new();
    if dropped > 0 {
        notes.push(format!("{dropped} row(s) with a missing category or value are left out."));
    }
    if options.y_scale != AxisScale::Linear {
        notes.push("A waterfall chart's running total is drawn on a linear scale.".to_owned());
    }
    notes.extend(filter_note(state, options, &rs.passes));
    let panel = Panel {
        grid_row: 0,
        grid_col: 0,
        title: None,
        x: AxisSpec {
            name: category.clone(),
            kind: AxisKind::Categorical { levels: slots },
            scale: AxisScale::Linear,
            time: None,
        },
        y: AxisSpec { name: value_col.clone(), kind: AxisKind::Continuous, scale: AxisScale::Linear, time: None },
        points,
        groups: styles.groups,
        slot_half_width: 0.0,
        extra: PanelExtra::Waterfall(WaterfallGeometry { bars, settings }),
        labels: std::collections::BTreeMap::new(),
        data_extent: [None; 2],
    };
    Ok(single_panel_frame(panel, Legend::default(), notes, rs.pages, rs.page))
}

/// Half-width of a waterfall bar (in category slots).
const HALF: f64 = 0.35;

/// Colour of a bar of `kind`.
fn kind_color(kind: StepKind) -> Rgba {
    match kind {
        StepKind::Increase => palette::INCREASE,
        StepKind::Decrease => palette::DECREASE,
        StepKind::Total => palette::BAR,
    }
}

/// Diagonal hatching inside the box `x0..x1 × y0..y1` (parallel lines in the
/// box's own proportions).
fn hatching(x0: f64, x1: f64, y0: f64, y1: f64) -> Vec<[[f64; 2]; 2]> {
    let (w, h) = (x1 - x0, y1 - y0);
    (1..8)
        .map(|j| {
            let u = j as f64 / 4.0;
            let (p0, q0) = ((u - 1.0).max(0.0), u.min(1.0));
            let (p1, q1) = (u.min(1.0), (u - 1.0).max(0.0));
            [[x0 + p0 * w, y0 + q0 * h], [x0 + p1 * w, y0 + q1 * h]]
        })
        .collect()
}

/// A contribution with its sign: `+12`, `−5` (a true minus sign).
fn signed(v: f64) -> String {
    if v < 0.0 { format!("−{}", format_value(-v)) } else { format!("+{}", format_value(v)) }
}

/// Draws the bars of a waterfall panel with dashed connectors from each
/// bar's end to the next bar's start; the selected share of a bar is shaded
/// dark with the selection outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WaterfallElement;

impl Element for WaterfallElement {
    fn kind(&self) -> ElementKind {
        ElementKind::Waterfall
    }

    fn layer(&self, panel: &Panel, state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        let PanelExtra::Waterfall(g) = &panel.extra else { return layer };
        let rect = |x0: f64, x1: f64, y0: f64, y1: f64| vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
        let mut any_selected = false;
        for (i, b) in g.bars.iter().enumerate() {
            let (lo, hi) = (b.start.min(b.end), b.start.max(b.end));
            let (x0, x1) = (b.x - HALF, b.x + HALF);
            let color = kind_color(b.kind);
            let outline = rect(x0, x1, lo, hi);
            layer.marks.push(Mark::Polygon {
                points: outline.clone(),
                fill: color.with_alpha(0.7),
                stroke: color,
                name: b.label.clone(),
            });
            if b.kind == StepKind::Decrease && hi > lo {
                layer.marks.push(Mark::Segments {
                    segments: hatching(x0, x1, lo, hi),
                    color: palette::BAR_TEXT.with_alpha(0.45),
                    width: 0.8,
                    name: String::new(),
                });
            }
            let selected = b.rows.iter().filter(|&&r| r < state.len() && state.is_selected(r)).count();
            if selected > 0 && !b.rows.is_empty() {
                any_selected = true;
                let top = lo + (hi - lo) * selected as f64 / b.rows.len() as f64;
                layer.marks.push(Mark::Polygon {
                    points: rect(x0, x1, lo, top),
                    fill: Rgba::rgb(20, 20, 30).with_alpha(0.4),
                    stroke: palette::SELECTED,
                    name: "Selected rows".to_owned(),
                });
            }
            layer.regions.push(Region { outline, rows: b.rows.clone() });
            if g.settings.labels {
                let text = match b.kind {
                    StepKind::Total => format_value(b.end),
                    _ => signed(b.end - b.start),
                };
                layer.marks.push(Mark::Text {
                    at: [b.x, hi],
                    text,
                    color: palette::BAR_TEXT,
                    anchor: TextAnchor::Above,
                    movable: None,
                });
            }
            if let Some(next) = g.bars.get(i + 1) {
                layer.marks.push(Mark::Line {
                    points: vec![[x1, b.end], [next.x - HALF, b.end]],
                    color: palette::LINE.with_alpha(0.7),
                    width: 1.0,
                    pattern: LinePattern::Dashed,
                    name: "Running total".to_owned(),
                });
            }
        }
        for (kind, label) in
            [(StepKind::Increase, "Increase"), (StepKind::Decrease, "Decrease (hatched)"), (StepKind::Total, "Total")]
        {
            if g.bars.iter().any(|b| b.kind == kind) {
                let c = kind_color(kind);
                layer.key_entry(label, c, KeyGlyph::Area(c.with_alpha(0.7)));
            }
        }
        if any_selected {
            layer.key_entry(
                "Share of rows selected",
                palette::SELECTED,
                KeyGlyph::Area(palette::SELECTED.with_alpha(0.65)),
            );
        }
        layer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::{Column, SelectionMode};

    #[test]
    fn waterfall_runs_a_total_in_table_order() {
        let text = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let t = DataTable::new(
            "w",
            vec![
                Column::text("item", text(&["Sales", "Costs", "Sales", "Tax", "Costs", ""])),
                Column::numeric("amount", vec![100.0, -30.0, 20.0, -15.0, -5.0, 7.0]),
            ],
        )
        .unwrap();
        let mut state = RowStateManager::new(6);
        let plan = EncodingPlan { x: vec!["item".into()], y: vec!["amount".into()], ..Default::default() };
        let f =
            build_waterfall_frame(&t, &state, &plan, &FrameOptions::default(), WaterfallSettings::default()).unwrap();
        let p = &f.panels[0];
        assert_eq!(p.x.levels(), ["Sales", "Costs", "Tax", "Total"], "first appearance, then the total");
        let PanelExtra::Waterfall(g) = &p.extra else { panic!() };
        let spans: Vec<(f64, f64, StepKind)> = g.bars.iter().map(|b| (b.start, b.end, b.kind)).collect();
        assert_eq!(
            spans,
            [
                (0.0, 120.0, StepKind::Increase),
                (120.0, 85.0, StepKind::Decrease),
                (85.0, 70.0, StepKind::Decrease),
                (0.0, 70.0, StepKind::Total)
            ]
        );
        assert_eq!(g.bars[3].rows, [0, 1, 2, 3, 4]);
        assert!(f.notes.iter().any(|n| n.contains("1 row")));
        assert_eq!(p.points.len(), 5);
        state.select([1], SelectionMode::Replace);
        let layer = WaterfallElement.layer(p, &state);
        assert!(layer.all_finite());
        let texts: Vec<&str> = layer
            .marks
            .iter()
            .filter_map(|m| if let Mark::Text { text, .. } = m { Some(text.as_str()) } else { None })
            .collect();
        assert_eq!(texts, ["+120", "−35", "−15", "70"]);
        assert_eq!(layer.marks.iter().filter(|m| matches!(m, Mark::Segments { .. })).count(), 2, "decreases hatched");
        assert_eq!(layer.key.len(), 4);
        assert_eq!(crate::view::region_rows(&[layer], [2.0, 100.0]), Some(vec![1, 4]));
        // Without the total bar.
        let no_total = WaterfallSettings { total: false, labels: false };
        let f = build_waterfall_frame(&t, &state, &plan, &FrameOptions::default(), no_total).unwrap();
        assert_eq!(f.panels[0].x.levels().len(), 3);
        let bad = EncodingPlan { x: vec!["item".into()], y: vec!["item".into()], ..Default::default() };
        assert!(build_waterfall_frame(&t, &state, &bad, &FrameOptions::default(), no_total).is_err());
    }

    #[test]
    fn hatching_stays_inside_its_box() {
        for [a, b] in hatching(1.0, 2.0, -3.0, 5.0) {
            for p in [a, b] {
                assert!((1.0..=2.0).contains(&p[0]) && (-3.0..=5.0).contains(&p[1]), "{p:?}");
            }
        }
    }
}
