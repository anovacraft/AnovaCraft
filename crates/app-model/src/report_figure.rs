//! Report plots drawn as vector figures (SVG, PDF, EPS) with
//! [`stat_viz::export::export_chart`], for saving from scripts.
//!
//! Each plot kind is drawn from the same data the report view draws:
//! scatter plots with their fits, bands, groups and labels; control charts
//! with centre line, limits and flagged points; histograms and frequency
//! charts; box plots; survival curves; and effect Pareto charts. Groups
//! carry a marker shape as well as a color, and excluded rows are drawn as
//! hollow gray markers, as on screen. Figures use the light palette.

use stat_data::RowStateManager;
use stat_viz::export::{ChartPanel, Figure, Label, VectorFormat, export_chart, export_chart_grid};
use stat_viz::palette::{BAR, BOX, DEFAULT_POINT, EXCLUDED, FIT, LINE, SELECTED, level_style};
use stat_viz::{KeyEntry, KeyGlyph, LinePattern as VizPattern, Mark, MarkShape, Rgba, TextAnchor};

use crate::cause_effect::CauseEffectDiagram;
use crate::linear_analysis::{Profile, ProfilePanel};
use crate::pareto_chart::{PARETO_BAR_WIDTH, ParetoChartSpec};
use crate::plot_view::ViewBounds;
use crate::report::{
    BarChartSpec, BoxPlotSpec, ControlChartSpec, EffectPareto, LinePattern, PlotSpec, ReportSection, SectionContent,
    SurvivalPlotSpec, TreeDiagram,
};

/// Centre lines of control charts.
const CENTER: Rgba = Rgba::rgb(5, 150, 105);
/// Points flagged by run rules.
const FLAGGED: Rgba = Rgba::rgb(185, 28, 28);
/// Marker radius in points.
const RADIUS: f32 = 3.5;
/// Level lines over a filled contour map: dark and translucent, so they
/// read on every band colour.
const CONTOUR_INK: Rgba = Rgba([20, 24, 32, 140]);

/// Relative luminance (0–1) of an opaque colour, roughly as the eye sees it.
fn luminance(c: Rgba) -> f64 {
    (0.2126 * f64::from(c.0[0]) + 0.7152 * f64::from(c.0[1]) + 0.0722 * f64::from(c.0[2])) / 255.0
}

/// Whether a section is drawn as a plot (and so can be saved as a figure).
#[must_use]
pub fn is_plot(content: &SectionContent) -> bool {
    !matches!(content, SectionContent::Table(_) | SectionContent::Note(_) | SectionContent::Formula(_))
}

/// Page size and view of a figure.
#[derive(Debug, Clone, PartialEq)]
pub struct FigureLayout {
    /// Page width in points.
    pub width: f64,
    /// Page height in points.
    pub height: f64,
    /// Axis ranges to show (as on screen); `None` fits the data.
    pub view: Option<ViewBounds>,
    /// How category labels along the horizontal axis are turned (as chosen
    /// in Plot settings).
    pub category_labels: stat_viz::CategoryLabels,
    /// The plot's axis settings from Plot settings (ticks, grid lines, tick
    /// marks, offsets, tick labels), drawn as on screen; `None` draws the
    /// standard axes.
    pub axes: Option<stat_viz::axis_settings::PlotAxes>,
}

impl Default for FigureLayout {
    fn default() -> Self {
        Self {
            width: 540.0,
            height: 380.0,
            view: None,
            category_labels: stat_viz::CategoryLabels::Automatic,
            axes: None,
        }
    }
}

impl FigureLayout {
    /// Points around the plot area: margins, axis labels, title and key.
    const AROUND: (f64, f64) = (92.0, 112.0);

    /// Points per screen pixel: a plot's on-screen size read at 96 pixels per
    /// inch, so text keeps about its on-screen size relative to the plot.
    pub const POINTS_PER_PIXEL: f64 = 0.75;

    /// A page whose plot area has the proportions of a plot drawn
    /// `plot_width` × `plot_height` pixels on screen, showing `view`.
    #[must_use]
    pub fn matching_screen(plot_width: f64, plot_height: f64, view: Option<ViewBounds>) -> Self {
        let k = Self::POINTS_PER_PIXEL;
        Self {
            width: plot_width * k + Self::AROUND.0,
            height: plot_height * k + Self::AROUND.1,
            view,
            category_labels: stat_viz::CategoryLabels::Automatic,
            axes: None,
        }
    }

    /// This page drawing the plot's axes with `axes` (as set in Plot
    /// settings), widened by the room tick labels moved out by offsets or
    /// outside tick marks take, so the plot area keeps its size.
    #[must_use]
    pub fn with_axes(mut self, axes: stat_viz::axis_settings::PlotAxes) -> Self {
        use stat_viz::axis_settings::AxisSide;
        let k = Self::POINTS_PER_PIXEL;
        self.width += axes.y.label_gap() * k + if axes.y.side == AxisSide::Opposite { 4.0 } else { 0.0 };
        self.height += axes.x.label_gap() * k + if axes.x.side == AxisSide::Opposite { 6.0 } else { 0.0 };
        self.axes = Some(axes);
        self
    }

    /// A page at a print scale: the plot area `mm_per_unit` millimetres per
    /// horizontal unit of `view` (its height keeping the on-screen
    /// proportions of a frame `plot_width` × `plot_height` pixels), so a
    /// printed figure measures true. `None` when the view is unusable.
    #[must_use]
    pub fn at_print_scale(plot_width: f64, plot_height: f64, view: ViewBounds, mm_per_unit: f64) -> Option<Self> {
        let span = view.x_max - view.x_min;
        if !(span > 0.0 && mm_per_unit > 0.0 && plot_width > 0.0 && plot_height > 0.0) {
            return None;
        }
        let points = span * mm_per_unit * 72.0 / 25.4;
        // Pages beyond about a metre are not useful; keep them sane.
        if !(points.is_finite() && points <= 3000.0) {
            return None;
        }
        let k = points / plot_width;
        Some(Self {
            width: points + Self::AROUND.0,
            height: plot_height * k + Self::AROUND.1,
            view: Some(view),
            category_labels: stat_viz::CategoryLabels::Automatic,
            axes: None,
        })
    }
}

/// The section's plot as a vector figure titled with the section's title,
/// or `None` for tables and notes. `rows` marks excluded rows (all rows
/// drawn as included when `None`).
#[must_use]
pub fn section_figure(
    section: &ReportSection,
    rows: Option<&RowStateManager>,
    format: VectorFormat,
) -> Option<Vec<u8>> {
    section_figure_with(section, rows, format, FigureLayout::default())
}

/// [`section_figure`] with a chosen page size and view (the view replaces
/// the fitted axis ranges).
#[must_use]
pub fn section_figure_with(
    section: &ReportSection,
    rows: Option<&RowStateManager>,
    format: VectorFormat,
    layout: FigureLayout,
) -> Option<Vec<u8>> {
    let excluded = |r: usize| rows.is_some_and(|s| r < s.len() && s.is_excluded(r));
    // Broken axes, as on screen (the view is in broken drawn units).
    let mut breaks = [None, None];
    let panels = match &section.content {
        SectionContent::Plot(p) => {
            breaks = layout.axes.as_ref().map_or([None, None], |a| crate::plot_breaks::plot_breaks(p, a));
            let (broken, _) = crate::plot_breaks::broken_plot(p, &breaks);
            vec![scatter(&broken, &excluded)]
        }
        SectionContent::ControlChart(c) => vec![control_chart(c)],
        SectionContent::Bars(b) => vec![bars(b)],
        SectionContent::BoxPlot(b) => vec![box_plot(b, &excluded)],
        SectionContent::Survival(s) => vec![survival(s, &excluded)],
        SectionContent::EffectPareto(p) => vec![pareto(p)],
        SectionContent::Tree(d) => return Some(tree_figure(d, &|_| false, &section.title, format)),
        SectionContent::Pareto(p) => vec![pareto_chart(p)],
        SectionContent::CauseEffect(d) => return Some(fishbone_figure(d, &section.title, format)),
        SectionContent::HeatMap(h) => vec![h.chart_panel()],
        SectionContent::WordCloud(w) => vec![w.chart_panel()],
        SectionContent::IntensityMap(m) => vec![m.chart_panel()],
        SectionContent::Raster(r) => {
            let selected = |row: usize| rows.is_some_and(|s| row < s.len() && s.is_selected(row));
            vec![r.chart_panel(Some((&selected, SELECTED)))]
        }
        SectionContent::Table(_) | SectionContent::Note(_) | SectionContent::Formula(_) => return None,
    };
    let mut panels = panels;
    for panel in &mut panels {
        panel.x_labels = layout.category_labels;
        panel.axes.clone_from(&layout.axes);
        panel.breaks.clone_from(&breaks);
    }
    if let (Some(v), [panel]) = (layout.view, panels.as_mut_slice()) {
        if v.x_max > v.x_min && v.y_max > v.y_min {
            panel.x_range = (v.x_min, v.x_max);
            panel.y_range = (v.y_min, v.y_max);
        }
    }
    let figure = Figure { title: &section.title, format, width: layout.width, height: layout.height };
    Some(export_chart(&panels, figure))
}

/// [`section_figure_with`] with the plot's reference lines and bands
/// (`reference`, from its plot style): bands behind the marks, lines over
/// them, their key entries added, and the fitted ranges widened to show
/// them (a view from `layout` is kept as it is).
#[must_use]
pub fn section_figure_styled(
    section: &ReportSection,
    rows: Option<&RowStateManager>,
    format: VectorFormat,
    layout: FigureLayout,
    reference: &stat_viz::plot_style::ReferenceLook,
) -> Option<Vec<u8>> {
    if reference.is_empty() || !matches!(chart_kind(&section.content), Some(true)) {
        return section_figure_with(section, rows, format, layout);
    }
    let excluded = |r: usize| rows.is_some_and(|s| r < s.len() && s.is_excluded(r));
    let mut panel = match &section.content {
        SectionContent::Plot(p) => scatter(p, &excluded),
        SectionContent::ControlChart(c) => control_chart(c),
        SectionContent::Bars(b) => bars(b),
        SectionContent::BoxPlot(b) => box_plot(b, &excluded),
        SectionContent::Survival(s) => survival(s, &excluded),
        SectionContent::EffectPareto(p) => pareto(p),
        SectionContent::Pareto(p) => pareto_chart(p),
        _ => return section_figure_with(section, rows, format, layout),
    };
    panel.x_labels = layout.category_labels;
    panel.axes.clone_from(&layout.axes);
    let refs = reference_layers(&section.content, rows, reference);
    let [ex, ey] = refs.extent();
    panel.x_range = stat_viz::element::reference::widen_range(panel.x_range, ex);
    panel.y_range = stat_viz::element::reference::widen_range(panel.y_range, ey);
    if let Some(v) = layout.view.filter(|v| v.x_max > v.x_min && v.y_max > v.y_min) {
        panel.x_range = (v.x_min, v.x_max);
        panel.y_range = (v.y_min, v.y_max);
    }
    let front = refs.front.marks;
    panel.marks.splice(0..0, refs.back.marks);
    panel.marks.extend(front);
    panel.key.extend(refs.back.key.into_iter().chain(refs.front.key));
    let figure = Figure { title: &section.title, format, width: layout.width, height: layout.height };
    Some(export_chart(&[panel], figure))
}

/// `Some(true)` for plot sections drawn as one x–y panel, which can carry
/// reference lines; `Some(false)` for other plots; `None` for tables and
/// notes.
#[must_use]
pub fn chart_kind(content: &SectionContent) -> Option<bool> {
    match content {
        SectionContent::Plot(_)
        | SectionContent::ControlChart(_)
        | SectionContent::Bars(_)
        | SectionContent::BoxPlot(_)
        | SectionContent::Survival(_)
        | SectionContent::EffectPareto(_)
        | SectionContent::Pareto(_) => Some(true),
        c if is_plot(c) => Some(false),
        _ => None,
    }
}

/// The plotted values of a report plot that its reference statistics are
/// computed from, rows in order: an x–y plot's points (horizontal and
/// vertical), a control chart's plotted statistics (vertical, in sample
/// order), a box plot's values (horizontal) and a survival plot's rows.
/// Excluded and hidden rows are left out. Other plots (bars, Pareto charts)
/// give no values: their reference lines take typed values only.
#[must_use]
pub fn reference_values(content: &SectionContent, rows: Option<&RowStateManager>) -> stat_viz::element::reference::ValueSet {
    let skip = |r: usize| rows.is_some_and(|s| r < s.len() && (s.is_excluded(r) || s.is_hidden(r)));
    let one = |v: f64| (v, 1.0, 1.0);
    let mut out = stat_viz::element::reference::ValueSet::default();
    let from_points = |points: &[crate::report::RowPoint], x: bool, y: bool| {
        let mut kept: Vec<&crate::report::RowPoint> = points.iter().filter(|p| !skip(p.row)).collect();
        kept.sort_by_key(|p| p.row);
        (
            if x { kept.iter().map(|p| one(p.x)).collect() } else { Vec::new() },
            if y { kept.iter().map(|p| one(p.y)).collect() } else { Vec::new() },
        )
    };
    match content {
        SectionContent::Plot(p) => {
            (out.x, out.y) = from_points(&p.points, p.x_ticks.is_none(), p.y_ticks.is_none());
        }
        SectionContent::ControlChart(c) => {
            out.y = c.points.iter().filter(|p| p.value.is_finite()).map(|p| one(p.value)).collect();
        }
        SectionContent::BoxPlot(b) => (out.x, _) = from_points(&b.points, true, false),
        SectionContent::Survival(s) => (out.x, out.y) = from_points(&s.points, true, true),
        _ => {}
    }
    out
}

/// The reference lines and bands of a report plot showing `content`.
#[must_use]
pub fn reference_layers(
    content: &SectionContent,
    rows: Option<&RowStateManager>,
    reference: &stat_viz::plot_style::ReferenceLook,
) -> stat_viz::element::reference::ReferenceLayers {
    use stat_viz::element::reference::{ReferenceAxes, ReferenceData, ValueSet};
    let data = if reference.uses_data() {
        ReferenceData::single(reference_values(content, rows))
    } else {
        ReferenceData::single(ValueSet::default())
    };
    let (x_categorical, y_categorical) = match content {
        SectionContent::Plot(p) => (p.x_ticks.is_some(), p.y_ticks.is_some()),
        SectionContent::ControlChart(c) => (c.x_ticks.is_some(), false),
        SectionContent::Bars(b) => (b.x_ticks.is_some(), false),
        SectionContent::BoxPlot(_) => (false, true),
        SectionContent::EffectPareto(_) | SectionContent::Pareto(_) => (true, false),
        _ => (false, false),
    };
    stat_viz::element::reference::reference_layers(
        reference,
        &data,
        ReferenceAxes { x_categorical, y_categorical, ..ReferenceAxes::default() },
    )
}

/// Confidence bands of the Response Explorer (the fit colour, lighter).
const PROFILE_BAND: Rgba = Rgba([225, 29, 72, 115]);
/// The current-prediction guide.
const DIVIDER: Rgba = Rgba::rgb(100, 116, 139);

/// The Response Explorer as a figure: one panel per factor, side by side
/// (wrapping after `columns`), each showing the predicted response with its
/// confidence band as that factor varies, the factor's setting (vertical
/// line or diamond) and the current prediction (dashed). All panels share
/// the vertical range, as on screen. `title` heads the page.
#[must_use]
pub fn profile_figure(
    profile: &Profile,
    title: &str,
    columns: usize,
    format: VectorFormat,
    layout: FigureLayout,
) -> Vec<u8> {
    let mut panels = profile_panels(profile);
    if let Some(first) = panels.first_mut() {
        first.key = profile_key(std::slice::from_ref(profile));
    }
    let figure = Figure { title, format, width: layout.width, height: layout.height };
    export_chart_grid(&panels, columns.max(1), figure)
}

/// Linked Response Explorer rows as one figure: a row of panels per
/// profile (every profile over the same factors), each row on its own
/// vertical range, as on screen.
#[must_use]
pub fn linked_profile_figure(profiles: &[Profile], title: &str, format: VectorFormat, layout: FigureLayout) -> Vec<u8> {
    let columns = profiles.first().map_or(1, |p| p.panels.len().max(1));
    let mut panels: Vec<ChartPanel> = profiles.iter().flat_map(profile_panels).collect();
    if let Some(first) = panels.first_mut() {
        first.key = profile_key(profiles);
    }
    let figure = Figure { title, format, width: layout.width, height: layout.height };
    export_chart_grid(&panels, columns, figure)
}

fn profile_key(profiles: &[Profile]) -> Vec<KeyEntry> {
    let mut key = vec![
        key("Prediction", FIT, KeyGlyph::Line(VizPattern::Solid)),
        key("Setting", SELECTED, KeyGlyph::Line(VizPattern::Solid)),
        key("Current prediction", DIVIDER, KeyGlyph::Line(VizPattern::Dotted)),
    ];
    if profiles.iter().flat_map(|p| &p.panels).any(|p| matches!(p, ProfilePanel::Continuous { band: Some(_), .. })) {
        key.push(key_entry_band());
    }
    key
}

/// One chart panel per factor of `profile`.
fn profile_panels(profile: &Profile) -> Vec<ChartPanel> {
    let (y0, y1) = profile.y_range;
    profile
        .panels
        .iter()
        .map(|panel| {
            let mut c = ChartPanel { y_title: profile.response.clone(), y_range: (y0, y1), ..ChartPanel::default() };
            match panel {
                ProfilePanel::Continuous { name, curve, band, range: (lo, hi), value } => {
                    c.x_title = name.clone();
                    let pad = ((hi - lo) * 0.02).max(1e-9);
                    c.x_range = (lo - pad, hi + pad);
                    for edge in band.iter().flat_map(|(lower, upper)| [lower, upper]) {
                        c.marks.push(line(edge.clone(), PROFILE_BAND, 1.0, VizPattern::Dashed, "Confidence band"));
                    }
                    c.marks.push(line(curve.clone(), FIT, 2.0, VizPattern::Solid, "Prediction"));
                    c.marks.push(line(vec![[*value, y0], [*value, y1]], SELECTED, 2.0, VizPattern::Solid, "Setting"));
                }
                ProfilePanel::Categorical { name, levels, predictions, selected } => {
                    c.x_title = name.clone();
                    c.x_range = (0.5, levels.len() as f64 + 0.5);
                    c.x_ticks = Some(levels.iter().enumerate().map(|(i, l)| ((i + 1) as f64, l.clone())).collect());
                    let x = (selected + 1) as f64;
                    c.marks.push(line(vec![[x, y0], [x, y1]], SELECTED, 1.0, VizPattern::Solid, "Setting"));
                    for (i, &(_, limits)) in predictions.iter().enumerate() {
                        if let Some((lo, hi)) = limits {
                            let x = (i + 1) as f64;
                            c.marks.push(line(
                                vec![[x, lo], [x, hi]],
                                PROFILE_BAND,
                                2.0,
                                VizPattern::Solid,
                                "Interval",
                            ));
                        }
                    }
                    let others = predictions
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| i != selected)
                        .map(|(i, p)| [(i + 1) as f64, p.0])
                        .collect();
                    c.marks.push(points(others, FIT, MarkShape::Circle, true, "Prediction"));
                    if let Some(p) = predictions.get(*selected) {
                        c.marks.push(Mark::Points {
                            points: vec![[x, p.0]],
                            color: SELECTED,
                            shape: MarkShape::Diamond,
                            radius: 5.0,
                            filled: true,
                            name: "Setting".into(),
                        });
                    }
                }
            }
            let (x0, x1) = c.x_range;
            c.marks.push(line(
                vec![[x0, profile.predicted], [x1, profile.predicted]],
                DIVIDER,
                1.0,
                VizPattern::Dotted,
                "",
            ));
            c
        })
        .collect()
}

fn key_entry_band() -> KeyEntry {
    key("Confidence band", PROFILE_BAND, KeyGlyph::Line(VizPattern::Dashed))
}

fn pattern(p: LinePattern) -> VizPattern {
    match p {
        LinePattern::Solid => VizPattern::Solid,
        LinePattern::Dashed => VizPattern::Dashed,
        LinePattern::Dotted => VizPattern::Dotted,
        LinePattern::DashDot => VizPattern::DashDot,
    }
}

fn line(points: Vec<[f64; 2]>, color: Rgba, width: f32, pattern: VizPattern, name: &str) -> Mark {
    Mark::Line { points, color, width, pattern, name: name.to_owned() }
}

fn points(points: Vec<[f64; 2]>, color: Rgba, shape: MarkShape, filled: bool, name: &str) -> Mark {
    Mark::Points { points, color, shape, radius: RADIUS, filled, name: name.to_owned() }
}

fn text(at: [f64; 2], text: &str, anchor: TextAnchor) -> Mark {
    Mark::Text { at, text: text.to_owned(), color: stat_viz::palette::LABEL, anchor, movable: None }
}

fn key(label: &str, color: Rgba, glyph: KeyGlyph) -> KeyEntry {
    KeyEntry { label: label.to_owned(), color, glyph }
}

/// Markers for included rows (`style`) and hollow gray ones for excluded rows.
fn split_excluded(
    rows_points: impl Iterator<Item = (usize, [f64; 2])>,
    excluded: &dyn Fn(usize) -> bool,
) -> (Vec<[f64; 2]>, Vec<[f64; 2]>) {
    let (mut kept, mut out) = (Vec::new(), Vec::new());
    for (r, p) in rows_points {
        if excluded(r) { out.push(p) } else { kept.push(p) }
    }
    (kept, out)
}

fn scatter(p: &PlotSpec, excluded: &dyn Fn(usize) -> bool) -> ChartPanel {
    let mut c = ChartPanel {
        x_title: p.x_label.clone(),
        y_title: p.y_label.clone(),
        x_ticks: p.x_ticks.clone(),
        y_ticks: p.y_ticks.clone(),
        x_time: p.x_time,
        y_time: p.y_time,
        ..ChartPanel::default()
    };
    let contour = p.contour.as_ref().map(|m| (m, m.drawing(false)));
    if let Some((map, d)) = &contour {
        for (color, band) in &d.bands {
            c.marks.push(Mark::Area {
                rings: band.rings.clone(),
                pieces: Vec::new(),
                color: *color,
                name: String::new(),
            });
        }
        let ink = if map.style.fill { CONTOUR_INK } else { LINE.with_alpha(0.7) };
        for (_, lines) in &d.lines {
            for l in lines {
                c.marks.push(line(l.clone(), ink, 0.8, VizPattern::Solid, ""));
            }
        }
        c.color_bar.clone_from(&d.bar);
    }
    if let Some((lo, hi)) = &p.band {
        c.marks.push(Mark::Band {
            lower: lo.clone(),
            upper: hi.clone(),
            color: FIT.with_alpha(0.18),
            name: String::new(),
        });
        c.key.push(key(p.band_label(), FIT, KeyGlyph::Area(FIT.with_alpha(0.18))));
    }
    for s in &p.segments {
        c.marks.push(line(s.clone(), LINE.with_alpha(0.7), 0.8, VizPattern::Solid, ""));
    }
    for r in &p.point_runs {
        c.marks.push(line(r.clone(), DEFAULT_POINT.with_alpha(0.55), 0.7, VizPattern::Solid, ""));
    }
    let mut hollow = Vec::new();
    match &p.groups {
        Some(g) => {
            for (k, label) in g.labels.iter().enumerate() {
                let (color, shape) = level_style(k);
                let members =
                    p.points.iter().zip(&g.membership).filter(|(_, m)| **m == k).map(|(pt, _)| (pt.row, [pt.x, pt.y]));
                let (kept, out) = split_excluded(members, excluded);
                hollow.extend(out);
                if let Some(l) = g.lines.get(k).filter(|l| !l.is_empty()) {
                    c.marks.push(line(l.clone(), color, 1.2, VizPattern::Solid, label));
                }
                c.marks.push(points(kept, color, shape, true, label));
                let name = g.key_label(k);
                c.key.push(key(&name, color, KeyGlyph::Marker(shape, true)));
            }
        }
        None => {
            let (kept, out) = split_excluded(p.points.iter().map(|pt| (pt.row, [pt.x, pt.y])), excluded);
            hollow.extend(out);
            if contour.as_ref().is_some_and(|(m, _)| m.style.fill) {
                // A white rim keeps points visible on any band colour.
                let rim = Mark::Points {
                    points: kept.clone(),
                    color: Rgba::rgb(255, 255, 255),
                    shape: MarkShape::Circle,
                    radius: RADIUS + 1.2,
                    filled: true,
                    name: String::new(),
                };
                c.marks.push(rim);
            }
            c.marks.push(points(kept, DEFAULT_POINT, MarkShape::Circle, true, ""));
            // Values whose style the annotations step past name their mark.
            if !p.points.is_empty() && p.annotation_style_offset > 0 {
                c.key.push(key(p.point_label(), DEFAULT_POINT, KeyGlyph::Marker(MarkShape::Circle, true)));
            }
        }
    }
    if !hollow.is_empty() {
        c.marks.push(points(hollow, EXCLUDED, MarkShape::Circle, false, "Excluded"));
        c.key.push(key("Excluded rows", EXCLUDED, KeyGlyph::Marker(MarkShape::Circle, false)));
    }
    if let Some(l) = &p.line {
        c.marks.push(line(l.clone(), FIT, 1.6, VizPattern::Solid, p.line_label()));
    }
    if let Some(a) = &p.annotations {
        for (k, name) in a.groups.iter().enumerate() {
            // Offset past the point groups so the two never share a style.
            let (color, shape) =
                level_style(k + p.groups.as_ref().map_or(0, |g| g.labels.len()) + p.annotation_style_offset);
            let items: Vec<_> = a.items.iter().filter(|i| i.3 == k).collect();
            c.marks.push(points(items.iter().map(|i| [i.0, i.1]).collect(), color, shape, true, name));
            for i in items {
                c.marks.push(text([i.0, i.1], &i.2, TextAnchor::Above));
            }
            // A name already given to a point group is in the key once.
            if !p.groups.as_ref().is_some_and(|g| (0..g.labels.len()).any(|k| g.key_label(k) == *name)) {
                c.key.push(key(name, color, KeyGlyph::Marker(shape, true)));
            }
        }
        for (x, y, label) in &a.labels {
            c.marks.push(text([*x, *y], label, TextAnchor::Centre));
        }
    }
    if let Some((map, d)) = &contour {
        let (lo, hi) = map.style.range.filter(|r| r.1 > r.0).or(map.field.value_range()).unwrap_or((0.0, 1.0));
        for (at, level, label) in &d.labels {
            // Level values in white where the scale is dark there.
            let level = *level;
            let under = map.style.scale.color(map.style.scale_position(level, lo, hi), false);
            let color = if map.style.fill && luminance(under) < 0.42 {
                Rgba::rgb(255, 255, 255)
            } else {
                stat_viz::palette::LABEL
            };
            c.marks.push(Mark::Text { at: *at, text: label.clone(), color, anchor: TextAnchor::Centre, movable: None });
        }
    }
    c.fit_ranges();
    if let Some(e) = contour.as_ref().and_then(|(m, _)| m.field.extent()) {
        // A map shows its own region (no stretch down to zero), plus any
        // point outside it.
        let mut r = [e[0], e[1], e[2], e[3]];
        for pt in &p.points {
            r = [r[0].min(pt.x), r[1].min(pt.y), r[2].max(pt.x), r[3].max(pt.y)];
        }
        let pad = |a: f64, b: f64| {
            let m = if b > a { 0.06 * (b - a) } else { 1.0 };
            (a - m, b + m)
        };
        c.x_range = pad(r[0], r[2]);
        c.y_range = pad(r[1], r[3]);
    }
    if !p.panes.is_empty() {
        // As on screen: headings stand on the highest data value, with 12 %
        // headroom above the fitted range so they sit inside the frame.
        let top = if p.points.is_empty() {
            data_top(&c)
        } else {
            p.points.iter().map(|pt| pt.y).filter(|y| y.is_finite()).fold(f64::NEG_INFINITY, f64::max)
        };
        let (lo, hi) = c.y_range;
        if top > lo {
            c.y_range.1 = hi + PANE_HEADROOM * (hi - lo);
        }
        pane_headings(&mut c, &p.panes, top);
    }
    c
}

/// Headroom (fraction of the fitted vertical range) added above a paned
/// scatter plot for its pane headings, as the report view adds.
pub const PANE_HEADROOM: f64 = 0.12;

/// The highest finite vertical coordinate of the panel's marks.
fn data_top(c: &ChartPanel) -> f64 {
    c.marks.iter().flat_map(Mark::coordinates).map(|p| p[1]).filter(|y| y.is_finite()).fold(f64::NEG_INFINITY, f64::max)
}

/// Pane dividers (the full height of the frame) and each pane's heading,
/// standing on `heading_y` (bottom-anchored, so it never reaches past the
/// headroom above the data). The dividers reach well past the fitted range,
/// so a figure drawn with the on-screen view still has them top to bottom;
/// the frame clips them.
fn pane_headings(c: &mut ChartPanel, panes: &[crate::report::PlotPane], heading_y: f64) {
    let (lo, hi) = c.y_range;
    let span = (hi - lo).abs().max(1.0);
    for pair in panes.windows(2) {
        let x = (pair[0].x_max + pair[1].x_min) / 2.0;
        c.marks.push(line(vec![[x, lo - 100.0 * span], [x, hi + 100.0 * span]], LINE, 0.8, VizPattern::Dashed, ""));
    }
    if heading_y.is_finite() {
        for pane in panes {
            let mid = (pane.x_min + pane.x_max) / 2.0;
            c.marks.push(text([mid, heading_y], &pane.label, TextAnchor::Above));
        }
    }
}

fn control_chart(s: &ControlChartSpec) -> ChartPanel {
    let mut c = ChartPanel {
        x_title: s.x_label.clone(),
        y_title: s.y_label.clone(),
        x_ticks: s.x_ticks.clone(),
        ..ChartPanel::default()
    };
    let has_center = s.limit_steps().iter().any(|step| step.center.is_finite());
    for step in s.limit_steps() {
        let across = |y: f64| vec![[step.from, y], [step.to, y]];
        if step.center.is_finite() {
            c.marks.push(line(across(step.center), CENTER, 1.2, VizPattern::Solid, "Center"));
        }
        for limit in [step.lower, step.upper].into_iter().filter(|v| v.is_finite()) {
            c.marks.push(line(across(limit), FIT, 1.2, VizPattern::Dashed, "Control limits"));
        }
    }
    // The connecting line breaks at each phase boundary.
    let mut segment: Vec<[f64; 2]> = Vec::new();
    for p in &s.points {
        let x = p.sample as f64;
        if segment.last().is_some_and(|last| s.phases.iter().any(|m| last[0] < m.at && m.at < x)) {
            c.marks.push(line(std::mem::take(&mut segment), LINE, 1.0, VizPattern::Solid, ""));
        }
        segment.push([x, p.value]);
    }
    c.marks.push(line(segment, LINE, 1.0, VizPattern::Solid, ""));
    let pts: Vec<[f64; 2]> = s.points.iter().map(|p| [p.sample as f64, p.value]).collect();
    c.marks.push(points(pts, LINE, MarkShape::Circle, true, ""));
    let flagged: Vec<[f64; 2]> = s.points.iter().filter(|p| p.flagged()).map(|p| [p.sample as f64, p.value]).collect();
    if has_center {
        c.key.push(key("Center line", CENTER, KeyGlyph::Line(VizPattern::Solid)));
    }
    c.key.push(key(s.limits_label(), FIT, KeyGlyph::Line(VizPattern::Dashed)));
    if !flagged.is_empty() {
        c.marks.push(points(flagged, FLAGGED, MarkShape::Diamond, true, "Flagged"));
        let label = s.flag_label.as_deref().unwrap_or(if s.points.iter().any(|p| p.signal) {
            "Flagged (zone score or run rules)"
        } else {
            "Flagged by run rules"
        });
        c.key.push(key(label, FLAGGED, KeyGlyph::Marker(MarkShape::Diamond, true)));
    }
    // Point notes (zone scores), or the numbers of the rules a point breaks
    // (not for charts with their own flag meaning, such as multivariate charts).
    for p in &s.points {
        let label = match &p.note {
            Some(n) => n.clone(),
            None if !p.rules.is_empty() && s.flag_label.is_none() => {
                p.rules.iter().map(u8::to_string).collect::<Vec<_>>().join(",")
            }
            None => continue,
        };
        c.marks.push(text([p.sample as f64, p.value], &label, TextAnchor::Above));
    }
    c.fit_ranges();
    if s.phases.len() > 1 {
        // Room along the top for the phase names; every other phase shaded
        // behind the rest, a solid rule between phases, and each phase named
        // centred over it (as on screen).
        let (y0, y1) = c.y_range;
        let y1 = y1 + 0.12 * (y1 - y0);
        c.y_range.1 = y1;
        let end = s.points.iter().map(|p| p.sample as f64 + 0.5).fold(f64::NEG_INFINITY, f64::max);
        let mut shading = Vec::new();
        for (k, m) in s.phases.iter().enumerate() {
            let to = s.phases.get(k + 1).map_or(end, |next| next.at);
            if k % 2 == 1 && to > m.at {
                shading.push(Mark::Polygon {
                    points: vec![[m.at, y0], [to, y0], [to, y1], [m.at, y1]],
                    fill: LINE.with_alpha(0.08),
                    stroke: LINE.with_alpha(0.0),
                    name: String::new(),
                });
            }
            if k > 0 {
                c.marks.push(line(vec![[m.at, y0], [m.at, y1]], LINE, 1.2, VizPattern::Solid, ""));
            }
            let mid = if to > m.at { (m.at + to) / 2.0 } else { m.at };
            c.marks.push(text([mid, y1 - 0.05 * (y1 - y0)], &m.label, TextAnchor::Centre));
        }
        c.marks.splice(0..0, shading);
        c.key.push(key("Phase boundary", LINE, KeyGlyph::Line(VizPattern::Solid)));
    }
    c
}

fn bars(b: &BarChartSpec) -> ChartPanel {
    let mut c = ChartPanel {
        x_title: b.x_label.clone(),
        y_title: b.y_label.clone(),
        x_ticks: b.x_ticks.clone(),
        x_time: b.x_time,
        ..ChartPanel::default()
    };
    for bar in &b.bars {
        let (l, r, h) = (bar.center - bar.width / 2.0, bar.center + bar.width / 2.0, bar.count() as f64);
        c.marks.push(Mark::Polygon {
            points: vec![[l, 0.0], [r, 0.0], [r, h], [l, h]],
            fill: BAR.with_alpha(0.35),
            stroke: BAR,
            name: String::new(),
        });
    }
    if let Some((name, curve)) = &b.curve {
        c.marks.push(line(curve.clone(), FIT, 1.4, VizPattern::Dashed, name));
        c.key.push(key(name, FIT, KeyGlyph::Line(VizPattern::Dashed)));
    }
    c.fit_ranges();
    for (x, label) in &b.references {
        c.marks.push(line(vec![[*x, c.y_range.0], [*x, c.y_range.1]], LINE, 1.0, VizPattern::Dotted, label));
        c.marks.push(text([*x, c.y_range.1], label, TextAnchor::RightOfText));
    }
    c
}

fn box_plot(b: &BoxPlotSpec, excluded: &dyn Fn(usize) -> bool) -> ChartPanel {
    let mut c =
        ChartPanel { x_title: b.x_label.clone(), x_time: b.x_time, y_ticks: Some(Vec::new()), ..ChartPanel::default() };
    let (lo, hi) = (-0.25, 0.25);
    c.marks.push(Mark::Polygon {
        points: vec![[b.q1, lo], [b.q3, lo], [b.q3, hi], [b.q1, hi]],
        fill: BOX.with_alpha(0.15),
        stroke: BOX,
        name: String::new(),
    });
    c.marks.push(line(vec![[b.median, lo], [b.median, hi]], BOX, 1.6, VizPattern::Solid, "Median"));
    c.marks.push(line(vec![[b.whisker_low, 0.0], [b.q1, 0.0]], BOX, 1.0, VizPattern::Solid, ""));
    c.marks.push(line(vec![[b.q3, 0.0], [b.whisker_high, 0.0]], BOX, 1.0, VizPattern::Solid, ""));
    for w in [b.whisker_low, b.whisker_high] {
        c.marks.push(line(vec![[w, -0.1], [w, 0.1]], BOX, 1.0, VizPattern::Solid, ""));
    }
    let outliers = b.outliers.iter().filter_map(|&i| b.points.get(i)).map(|p| (p.row, [p.x, 0.0]));
    let (kept, out) = split_excluded(outliers, excluded);
    c.marks.push(points(kept, DEFAULT_POINT, MarkShape::Circle, true, "Beyond the whiskers"));
    c.marks.push(points(out, EXCLUDED, MarkShape::Circle, false, "Excluded"));
    c.marks.push(points(vec![[b.mean, 0.0]], FIT, MarkShape::Diamond, true, "Mean"));
    c.key.push(key("Mean", FIT, KeyGlyph::Marker(MarkShape::Diamond, true)));
    c.key.push(key("Beyond 1.5 IQR", DEFAULT_POINT, KeyGlyph::Marker(MarkShape::Circle, true)));
    c.fit_ranges();
    c.y_range = (-1.0, 1.0);
    c
}

fn survival(s: &SurvivalPlotSpec, excluded: &dyn Fn(usize) -> bool) -> ChartPanel {
    let mut c = ChartPanel { x_title: s.x_label.clone(), y_title: s.y_label.clone(), ..ChartPanel::default() };
    for (k, curve) in s.curves.iter().enumerate() {
        let (color, shape) = level_style(k);
        for [t0, t1, lower, upper] in &curve.band {
            c.marks.push(Mark::Band {
                lower: vec![[*t0, *lower], [*t1, *lower]],
                upper: vec![[*t0, *upper], [*t1, *upper]],
                color: color.with_alpha(0.15),
                name: String::new(),
            });
        }
        c.marks.push(line(curve.steps.clone(), color, 1.5, VizPattern::Solid, &curve.label));
        for (name, p, poly) in &curve.fitted {
            c.marks.push(line(poly.clone(), color, 1.2, pattern(*p), name));
            let label = if curve.label.is_empty() { name.clone() } else { format!("{name} ({})", curve.label) };
            c.key.push(key(&label, color, KeyGlyph::Line(pattern(*p))));
        }
        let members = s.points.iter().zip(&s.censored).zip(&s.membership).filter(|(_, m)| **m == k);
        let (mut events, mut censored) = (Vec::new(), Vec::new());
        for ((pt, cens), _) in members {
            if excluded(pt.row) {
                continue;
            }
            if *cens { censored.push([pt.x, pt.y]) } else { events.push([pt.x, pt.y]) }
        }
        let hollow = if s.estimate.is_some() { "End of observation" } else { "Censored" };
        c.marks.push(points(events, color, shape, true, "Event"));
        c.marks.push(points(censored, color, shape, false, hollow));
        if !curve.label.is_empty() {
            c.key.push(key(&curve.label, color, KeyGlyph::Marker(shape, true)));
        }
    }
    let hollow = if s.estimate.is_some() { "End of observation (hollow)" } else { "Censored (hollow)" };
    c.key.push(key(hollow, LINE, KeyGlyph::Marker(MarkShape::Circle, false)));
    c.fit_ranges();
    if s.estimate.is_none() {
        c.y_range = (0.0, 1.05);
    }
    c
}

fn pareto(p: &EffectPareto) -> ChartPanel {
    let mut c = ChartPanel { y_title: "−log₁₀ p".into(), ..ChartPanel::default() };
    let mut ticks = Vec::new();
    for (i, e) in p.effects.iter().enumerate() {
        let x = i as f64 + 1.0;
        ticks.push((x, e.name.clone()));
        let h = e.neg_log10_p();
        if h.is_finite() {
            c.marks.push(Mark::Polygon {
                points: vec![[x - 0.35, 0.0], [x + 0.35, 0.0], [x + 0.35, h], [x - 0.35, h]],
                fill: BAR.with_alpha(0.35),
                stroke: BAR,
                name: e.name.clone(),
            });
            if !e.stars().is_empty() {
                c.marks.push(text([x, h], e.stars(), TextAnchor::Above));
            }
        }
    }
    c.x_ticks = Some(ticks);
    let threshold = -(0.05f64.log10());
    c.marks.push(Mark::Points {
        points: vec![[0.5, threshold]],
        color: Rgba([0, 0, 0, 0]),
        shape: MarkShape::Circle,
        radius: 0.0,
        filled: false,
        name: String::new(),
    });
    c.fit_ranges();
    c.marks.push(line(vec![[c.x_range.0, threshold], [c.x_range.1, threshold]], FIT, 1.0, VizPattern::Dashed, ""));
    c.key.push(key("p = 0.05", FIT, KeyGlyph::Line(VizPattern::Dashed)));
    c
}

/// A Pareto chart: bars ranked largest first ("Other" lighter, last),
/// the cumulative line with its percentages, and a pane per group.
fn pareto_chart(p: &ParetoChartSpec) -> ChartPanel {
    let mut c = ChartPanel { x_title: p.category.clone(), y_title: p.y_label.clone(), ..ChartPanel::default() };
    let half = PARETO_BAR_WIDTH / 2.0;
    let xs = p.bar_x();
    for (panel, xs) in p.panels.iter().zip(&xs) {
        for (bar, &x) in panel.bars.iter().zip(xs) {
            let h = p.height(bar);
            let alpha = if bar.pooled.is_empty() { 0.35 } else { 0.12 };
            c.marks.push(Mark::Polygon {
                points: vec![[x - half, 0.0], [x + half, 0.0], [x + half, h], [x - half, h]],
                fill: BAR.with_alpha(alpha),
                stroke: BAR,
                name: bar.label.clone(),
            });
        }
        let line: Vec<[f64; 2]> = panel.bars.iter().zip(xs).map(|(b, &x)| [x, p.cumulative_height(panel, b)]).collect();
        c.marks.push(self::line(line.clone(), FIT, 1.4, VizPattern::Solid, "Cumulative %"));
        c.marks.push(points(line.clone(), FIT, MarkShape::Square, true, "Cumulative %"));
        for (b, at) in panel.bars.iter().zip(&line) {
            c.marks.push(text(
                *at,
                &format!("{}%", crate::format::format_number(100.0 * b.cumulative, 3)),
                TextAnchor::Above,
            ));
        }
    }
    c.x_ticks = Some(p.ticks());
    c.key.push(key(&p.y_label, BAR, KeyGlyph::Area(BAR.with_alpha(0.35))));
    if p.panels.iter().flat_map(|q| &q.bars).any(|b| !b.pooled.is_empty()) {
        c.key.push(key("Other (pooled)", BAR, KeyGlyph::Area(BAR.with_alpha(0.12))));
    }
    c.key.push(key("Cumulative %", FIT, KeyGlyph::Marker(MarkShape::Square, true)));
    c.fit_ranges();
    c.y_range.0 = 0.0;
    let panes = p.panes();
    // As on screen: 8 % headroom, or 16 % with pane headings standing just
    // above the tallest bar or cumulative point.
    let extra = if panes.is_empty() { 0.08 } else { 0.16 };
    c.y_range.1 += extra * (c.y_range.1 - c.y_range.0);
    pane_headings(&mut c, &panes, p.top() * 1.07);
    c
}

/// A cause-and-effect diagram as a figure, laid out as on screen
/// ([`CauseEffectDiagram::layout`]).
#[must_use]
pub fn fishbone_figure(d: &CauseEffectDiagram, title: &str, format: VectorFormat) -> Vec<u8> {
    let l = d.layout();
    let mut panel =
        ChartPanel { bare: true, x_range: (0.0, l.width), y_range: (0.0, l.height), ..ChartPanel::default() };
    for line in &l.lines {
        panel.marks.push(self::line(line.points.clone(), TREE_INK, line.width as f32, VizPattern::Solid, ""));
    }
    panel.marks.push(Mark::Polygon { points: l.arrow.clone(), fill: TREE_INK, stroke: TREE_INK, name: String::new() });
    let [x0, y0, x1, y1] = l.head;
    panel.marks.push(Mark::Polygon {
        points: vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]],
        fill: TREE_FILL,
        stroke: TREE_INK,
        name: String::new(),
    });
    for label in &l.labels {
        panel.labels.push(Label {
            at: label.at,
            text: label.text.clone(),
            size: label.size,
            color: stat_viz::palette::LABEL,
            align: label.align,
            valign: label.valign,
            bold: label.bold,
        });
    }
    let figure = Figure { title, format, width: l.width + 44.0, height: l.height + 66.0 };
    export_chart(&[panel], figure)
}

/// Width of a leaf slot of a tree figure, in points.
const TREE_SLOT: f64 = 150.0;
/// Height of a tree level, in points.
const TREE_LEVEL: f64 = 74.0;
/// Node box size, in points.
const TREE_BOX: (f64, f64) = (138.0, 46.0);
/// Node box outline and connector lines.
const TREE_INK: Rgba = Rgba::rgb(100, 116, 139);
/// Node box fill.
const TREE_FILL: Rgba = Rgba::rgb(248, 250, 252);

/// A decision tree as a figure: one box per node shown (nodes for which
/// `collapsed` is true hide their branches, as on screen) holding the rule
/// leading to it, its count and prediction and, for a classification tree,
/// a bar of class shares whose colours the key names. Boxes join their
/// children with elbow connectors.
#[must_use]
pub fn tree_figure(d: &TreeDiagram, collapsed: &dyn Fn(usize) -> bool, title: &str, format: VectorFormat) -> Vec<u8> {
    let (places, slots, deepest) = d.layout(collapsed);
    let (w, h) = (slots.max(1) as f64 * TREE_SLOT, (deepest + 1) as f64 * TREE_LEVEL);
    // Page coordinates, y up: depth 0 at the top.
    let centre = |p: [f64; 2]| [p[0] * TREE_SLOT, h - (p[1] + 0.5) * TREE_LEVEL];
    let mut panel = ChartPanel { bare: true, x_range: (0.0, w), y_range: (0.0, h), ..ChartPanel::default() };
    let (bw, bh) = TREE_BOX;
    for (i, node) in d.nodes.iter().enumerate() {
        let Some(p) = places[i] else { continue };
        let [cx, cy] = centre(p);
        if !collapsed(i) {
            for &c in &node.children {
                let Some(q) = places[c] else { continue };
                let [qx, qy] = centre(q);
                let mid = (cy - bh / 2.0 + qy + bh / 2.0) / 2.0;
                panel.marks.push(line(
                    vec![[cx, cy - bh / 2.0], [cx, mid], [qx, mid], [qx, qy + bh / 2.0]],
                    TREE_INK,
                    1.0,
                    VizPattern::Solid,
                    "",
                ));
            }
        }
        let (x0, x1, y0, y1) = (cx - bw / 2.0, cx + bw / 2.0, cy - bh / 2.0, cy + bh / 2.0);
        panel.marks.push(Mark::Polygon {
            points: vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]],
            fill: TREE_FILL,
            stroke: TREE_INK,
            name: String::new(),
        });
        let label = |at: [f64; 2], text: &str, size: f64, bold: bool| Label {
            at,
            text: text.to_owned(),
            size,
            color: stat_viz::palette::LABEL,
            align: 0.5,
            valign: 1.0,
            bold,
        };
        panel.labels.push(label([cx, y1 - 13.0], &node.rule, 8.0, true));
        panel.labels.push(label([cx, y1 - 25.0], &node.summary, 7.5, false));
        let note = if collapsed(i) && !node.children.is_empty() { "(branches hidden)" } else { "" };
        if !node.shares.is_empty() {
            let mut x = x0 + 6.0;
            let span = bw - 12.0;
            for (k, &share) in node.shares.iter().enumerate() {
                let (color, _) = level_style(k);
                let dx = share * span;
                if dx > 0.0 {
                    panel.marks.push(Mark::Polygon {
                        points: vec![[x, y0 + 6.0], [x + dx, y0 + 6.0], [x + dx, y0 + 13.0], [x, y0 + 13.0]],
                        fill: color,
                        stroke: color,
                        name: String::new(),
                    });
                }
                x += dx;
            }
        } else if !note.is_empty() {
            panel.labels.push(label([cx, y0 + 6.0], note, 7.0, false));
        }
    }
    for (k, class) in d.classes.iter().enumerate() {
        panel.key.push(key(class, level_style(k).0, KeyGlyph::Area(level_style(k).0)));
    }
    let figure =
        Figure { title, format, width: w + 44.0, height: h + 66.0 + if d.classes.is_empty() { 0.0 } else { 16.0 } };
    export_chart(&[panel], figure)
}

#[cfg(test)]
mod tests {
    #[test]
    fn print_scale_sets_the_plot_width_in_millimetres() {
        let view = ViewBounds { x_min: 0.0, x_max: 10.0, y_min: 0.0, y_max: 5.0 };
        // 10 units at 5 mm each: a plot 50 mm (about 141.7 points) wide.
        let layout = super::FigureLayout::at_print_scale(400.0, 200.0, view, 5.0).unwrap();
        let plot_w = layout.width - 92.0;
        assert!((plot_w - 50.0 * 72.0 / 25.4).abs() < 1e-9);
        assert!((layout.height - 112.0 - plot_w / 2.0).abs() < 1e-9, "the screen's proportions");
        assert!(super::FigureLayout::at_print_scale(400.0, 200.0, view, 1e6).is_none(), "absurdly large");
        // Offsets widen the page so the plot keeps its width.
        let mut axes = stat_viz::axis_settings::PlotAxes::default();
        axes.y.offset = 8.0;
        let wider = layout.clone().with_axes(axes);
        assert!((wider.width - layout.width - 6.0).abs() < 1e-9);
    }

    use super::*;
    use crate::project::SavedReport;
    use crate::run_report;
    use crate::sample::catalog;

    /// An exported control chart with phases shades every other phase
    /// behind the data, draws solid boundaries and names each phase centred
    /// over it inside the frame.
    #[test]
    fn exported_phases_are_shaded_and_named() {
        let points = (1..=20)
            .map(|i| crate::report::ChartPoint {
                sample: i,
                value: if i <= 10 { 10.0 } else { 20.0 } + (i % 2) as f64,
                rows: vec![i - 1],
                rules: Vec::new(),
                label: None,
                note: None,
                signal: false,
            })
            .collect();
        let phase = |at: f64, label: &str| crate::report::PhaseMark { at, label: label.to_owned() };
        let spec = ControlChartSpec {
            x_label: "Sample".to_owned(),
            y_label: "Value".to_owned(),
            x_ticks: None,
            flag_label: None,
            limit_label: None,
            points,
            center: 15.0,
            lower: 5.0,
            upper: 25.0,
            zone_sigma: 0.0,
            steps: Vec::new(),
            phases: vec![phase(0.5, "Before"), phase(10.5, "After")],
        };
        let c = control_chart(&spec);
        let Mark::Polygon { points, .. } = &c.marks[0] else { panic!("the shading is drawn first: {:?}", c.marks[0]) };
        assert_eq!((points[0][0], points[1][0]), (10.5, 20.5), "the second phase is shaded");
        assert!(c.marks.iter().any(|m| matches!(m, Mark::Line { points, pattern: VizPattern::Solid, .. }
            if points[0][0] == 10.5 && points[1][0] == 10.5)));
        let names: Vec<(f64, f64, &str)> = c
            .marks
            .iter()
            .filter_map(|m| match m {
                Mark::Text { at, text, anchor: TextAnchor::Centre, .. } => Some((at[0], at[1], text.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(names.iter().map(|n| (n.0, n.2)).collect::<Vec<_>>(), [(5.5, "Before"), (15.5, "After")]);
        assert!(names.iter().all(|n| n.1 < c.y_range.1 && n.1 > 25.0), "inside the frame, above the limits");
        assert!(c.key.iter().any(|k| k.label == "Phase boundary"));
    }

    /// A tuning plot marks its chosen setting with an annotation named like
    /// the point group it belongs to; the key listed that name twice.
    #[test]
    fn annotation_named_like_a_group_is_keyed_once() {
        let spec = PlotSpec {
            points: vec![crate::report::RowPoint { row: 0, x: 1.0, y: 1.0 }],
            groups: Some(crate::report::PointGroups {
                title: "Loss".to_owned(),
                labels: vec!["Training".to_owned()],
                membership: vec![0],
                lines: Vec::new(),
            }),
            annotations: Some(crate::report::Annotations {
                groups: vec!["Training".to_owned(), "Chosen".to_owned()],
                items: vec![(1.0, 1.0, String::new(), 0), (1.0, 1.0, String::new(), 1)],
                labels: Vec::new(),
            }),
            ..PlotSpec::default()
        };
        let c = scatter(&spec, &|_| false);
        let labels: Vec<&str> = c.key.iter().map(|k| k.label.as_str()).collect();
        assert_eq!(labels.iter().filter(|l| **l == "Training").count(), 1, "{labels:?}");
        assert!(labels.contains(&"Chosen"), "{labels:?}");
    }

    /// Pane headings of an exported multi-vari chart sit inside the frame,
    /// above the highest value, as on screen (they were cut in half at the
    /// frame's top edge).
    #[test]
    fn pane_headings_stay_inside_the_frame() {
        let t = crate::sample::shaft_diameters();
        let rows = RowStateManager::new(t.n_rows());
        let col = |n: &str| t.find(n).unwrap();
        let request =
            crate::MultiVariRequest { response: col("Diameter (mm)"), factors: vec![col("Batch"), col("Shaft")] };
        let a = crate::MultiVariAnalysis::run(&t, &rows, &request).unwrap();
        let sections = a.sections(&crate::MultiVariOptions::default());
        let SectionContent::Plot(p) = &sections[0].content else { panic!() };
        let c = scatter(p, &|_| false);
        let top = p.points.iter().map(|q| q.y).fold(f64::NEG_INFINITY, f64::max);
        let headings: Vec<[f64; 2]> = c
            .marks
            .iter()
            .filter_map(|m| match m {
                Mark::Text { at, text, anchor: TextAnchor::Above, .. } if text.starts_with("Batch: ") => Some(*at),
                _ => None,
            })
            .collect();
        assert_eq!(headings.len(), 3);
        // Default page: about 268 points of plot height; a heading needs
        // its 7.5-point text plus the 3-point gap above its anchor.
        let (lo, hi) = c.y_range;
        let text_height = (7.5 + 3.0) / 268.0 * (hi - lo);
        for at in &headings {
            assert!((at[1] - top).abs() < 1e-9, "the heading stands on the highest value");
            assert!(hi - at[1] >= text_height, "room above the heading: {} < {text_height}", hi - at[1]);
        }
        // The SVG draws each heading below the top of the plot's clip rect.
        let svg = String::from_utf8(section_figure(&sections[0], Some(&rows), VectorFormat::Svg).unwrap()).unwrap();
        let attr = |line: &str, name: &str| -> f64 {
            let start = line.find(&format!(" {name}=\"")).unwrap() + name.len() + 3;
            line[start..].split('"').next().unwrap().parse().unwrap()
        };
        let clip = svg.lines().find(|l| l.contains("<clipPath")).unwrap();
        let frame_top = attr(clip, "y");
        for batch in ["Friday", "Monday", "Wednesday"] {
            let line = svg.lines().find(|l| l.contains(&format!(">Batch: {batch}<"))).unwrap();
            let size = attr(line, "font-size");
            assert!(attr(line, "y") - size >= frame_top, "{batch}: {line} vs frame top {frame_top}");
        }
    }

    /// A figure laid out from the screen keeps the plot's proportions and
    /// shows the view on screen rather than the fitted ranges.
    #[test]
    fn figures_follow_the_screen_size_and_view() {
        let s = catalog().into_iter().find(|i| i.name.starts_with("Norris")).map(|i| (i.build)()).unwrap();
        let rows = RowStateManager::new(s.table.n_rows());
        let saved = s.reports.iter().find(|r| matches!(r, SavedReport::Linear { .. })).unwrap();
        let r = run_report(saved, &s.table, &rows).unwrap();
        let section = r.sections.iter().find(|s| s.title == "Fitted Line").unwrap();
        let svg =
            |layout| String::from_utf8(section_figure_with(section, None, VectorFormat::Svg, layout).unwrap()).unwrap();
        let fitted = svg(FigureLayout::default());
        assert!(fitted.contains(">1000<"), "the fitted view reaches 1000");
        let view = ViewBounds { x_min: 0.0, x_max: 100.0, y_min: 0.0, y_max: 100.0 };
        let wide = svg(FigureLayout::matching_screen(800.0, 300.0, Some(view)));
        assert!(wide.contains("viewBox=\"0 0 692 337\""), "{}", &wide[..200]);
        assert!(!wide.contains(">1000<") && wide.contains(">100<"));
    }

    /// The Response Explorer figure has a panel per factor in the screen's
    /// layout, and draws as PNG too.
    #[test]
    fn response_explorer_figure() {
        use crate::{SurfaceAnalysis, SurfaceModel, SurfaceRequest};
        let t = crate::sample::reaction_yield();
        let rows = RowStateManager::new(t.n_rows());
        let a = SurfaceAnalysis::run(&t, &rows, &SurfaceRequest { response: 2, factors: vec![0, 1] }).unwrap();
        let p = crate::surface_profiler::profile(&a, &a.default_profile_settings()).unwrap();
        let layout = FigureLayout::matching_screen(600.0, 200.0, None);
        let svg = String::from_utf8(profile_figure(&p, "Response Explorer", 2, VectorFormat::Svg, layout.clone())).unwrap();
        for name in ["Response Explorer", "Time (min)", "Confidence band", "Current prediction"] {
            assert!(svg.contains(name), "{name}");
        }
        assert!(svg.contains("viewBox=\"0 0 542 262\""), "{}", &svg[..200]);
        let png = profile_figure(&p, "Response Explorer", 2, VectorFormat::Png, layout);
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    }

    /// Every plot of every sample report draws as SVG, PDF, EPS and PNG.
    #[test]
    fn every_sample_plot_draws() {
        let mut kinds = std::collections::HashSet::new();
        for info in catalog() {
            let s = (info.build)();
            let rows = RowStateManager::new(s.table.n_rows());
            for saved in s.reports.iter().filter(|r| !matches!(r, SavedReport::Graph { .. })) {
                let r = run_report(saved, &s.table, &rows).unwrap();
                for section in &r.sections {
                    let Some(svg) = section_figure(section, Some(&rows), VectorFormat::Svg) else {
                        assert!(!is_plot(&section.content));
                        continue;
                    };
                    kinds.insert(std::mem::discriminant(&section.content));
                    let svg = String::from_utf8(svg).unwrap();
                    assert!(svg.starts_with("<svg") || svg.contains("<svg"), "{}", section.title);
                    assert!(
                        svg.contains("<path") || svg.contains("<polyline") || svg.contains("<polygon"),
                        "{}",
                        section.title
                    );
                    let pdf = section_figure(section, None, VectorFormat::Pdf).unwrap();
                    assert!(pdf.starts_with(b"%PDF"));
                    let eps = section_figure(section, None, VectorFormat::Eps).unwrap();
                    assert!(eps.starts_with(b"%!PS"));
                    let png = section_figure(section, None, VectorFormat::Png).unwrap();
                    assert!(png.starts_with(b"\x89PNG"), "{}", section.title);
                }
            }
        }
        assert!(kinds.len() >= 6, "plot kinds drawn: {}", kinds.len());
    }

    #[test]
    fn excluded_rows_are_drawn_hollow_and_listed_in_the_key() {
        let s = crate::sample::norris();
        let mut rows = RowStateManager::new(s.n_rows());
        rows.set_excluded([0], true);
        let spec =
            crate::LinearModelSpec { response: 0, predictors: vec![1], effects: Vec::new(), splines: Vec::new() };
        let a = crate::LinearModelAnalysis::run(&s, &rows, &spec).unwrap();
        let options = crate::ReportOptions { show_fit_plot: true, ..crate::ReportOptions::default() };
        let plot = a.sections(&options).into_iter().find(|x| matches!(x.content, SectionContent::Plot(_))).unwrap();
        let with = String::from_utf8(section_figure(&plot, Some(&rows), VectorFormat::Svg).unwrap()).unwrap();
        let without = String::from_utf8(section_figure(&plot, None, VectorFormat::Svg).unwrap()).unwrap();
        assert!(with.contains("Excluded rows") && !without.contains("Excluded rows"));
    }
}
