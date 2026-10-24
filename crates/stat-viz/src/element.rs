//! Chart elements: independent layers drawn in the same panels and
//! coordinates, so any combination can be active at once (points under a
//! smoother under box plots, …). Each element turns a [`Panel`] into a
//! [`Layer`] of toolkit-independent marks and key entries; with the `egui`
//! feature, `Element::render` draws it straight into an `egui_plot` plot.
//!
//! * [`PointsElement`] — raw rows, jittered on category axes.
//! * [`FitLineElement`] — least-squares polynomials with confidence and
//!   prediction bands and fit statistics.
//! * [`SmootherElement`] — smoothing spline or LOESS, with a confidence band.
//! * [`BoxPlotElement`] (optionally notched) and [`ViolinElement`] —
//!   distributions per category.
//! * [`SummaryElement`] — a statistic per category or x with error bars.
//! * [`EllipseElement`], [`ContourElement`], [`BinElement`] — bivariate
//!   density: normal ellipses, kernel-density contours, binned counts.
//! * [`FunctionElement`] — curves `y = f(x)` typed as formulas.
//! * [`BarElement`], [`LineElement`], [`HeatmapElement`] — aggregates: bars
//!   (side by side, stacked, 100%), line and run charts, category heat maps.
//! * [`HistogramElement`], [`DotPlotElement`], [`ProbabilityPlotElement`] —
//!   one continuous column across: histograms, Wilkinson dot plots and
//!   probability plots against a fitted distribution.
//! * Whole-panel chart forms ([`crate::forms`]): treemaps, parallel
//!   coordinates, pie, radar, polar, wind-rose, ternary, waterfall, ridgeline
//!   and vector-field charts. A scatterplot matrix uses the usual elements, with the
//!   one-variable ones on its diagonal and the others off it.

use stat_data::RowStateManager;

use crate::frame::{Panel, PanelPoint};
use crate::layer::{Layer, Rgba};

mod aggregate;
mod annotation;
mod bivariate;
mod brackets;
mod distribution;
mod fit;
mod function;
mod histogram;
mod hull;
mod points;
pub mod reference;
mod smoother;
mod summaries;

pub use aggregate::{BarElement, BarLayout, HeatmapElement, LineConnect, LineElement};
pub use annotation::{
    ANNOTATION_INK, ANNOTATION_WIDTH, Annotation, AnnotationAnchor, AnnotationShape, PanelPlace, annotation_layer,
};
pub use bivariate::{BinElement, ContourElement, EllipseElement};
pub use brackets::{
    BracketComparison, BracketElement, BracketLabel, BracketTest, STAR_KEY, STAR_THRESHOLDS, bracket_tiers, place_brackets,
    star_key, stars, stars_with,
};
pub use distribution::{BoxPlotElement, ViolinElement};
pub use fit::{FitLabels, FitLineElement};
pub use function::FunctionElement;
pub use histogram::{DotPlotElement, HistogramBins, HistogramElement, ProbabilityPlotElement};
pub use hull::{HullElement, convex_hull};
pub use points::{FADED_ALPHA, PointsElement, SWARM_PANEL};
pub use reference::{
    ReferenceBand, ReferenceLine, ReferenceLook, ReferenceOrientation, ReferenceScope, ReferenceStat, ReferenceValue,
};
pub use smoother::{SmoothMethod, SmootherElement};
pub use summaries::SummaryElement;

/// The kinds of element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ElementKind {
    /// Raw data points.
    Points,
    /// Smoothing spline or LOESS.
    Smoother,
    /// Least-squares polynomial with bands.
    FitLine,
    /// Tukey box plots.
    BoxPlot,
    /// Violin (kernel density) plots.
    Violin,
    /// Summary statistic points with error bars.
    Summary,
    /// Normal density ellipses.
    Ellipse,
    /// Kernel density contours.
    Contour,
    /// Binned counts (rectangles or hexagons).
    Bins,
    /// Formula curves.
    Function,
    /// Bars of a statistic.
    Bars,
    /// Line chart.
    Line,
    /// Heat map of category pairs.
    Heatmap,
    /// Treemap (a whole-panel chart form).
    Treemap,
    /// Parallel coordinates (a whole-panel chart form).
    Parallel,
    /// Multi-vari means and joins (a whole-panel chart form).
    MultiVari,
    /// Histogram of the horizontal column.
    Histogram,
    /// Wilkinson dot plot of the horizontal column.
    DotPlot,
    /// Probability plot of the horizontal column against a fitted distribution.
    ProbabilityPlot,
    /// Convex hulls around subgroups.
    Hull,
    /// Significance brackets between categories.
    Brackets,
    /// Clustered heat map (a whole-panel chart form).
    ClusterMap,
    /// Estimation plot differences (a whole-panel chart form).
    Estimation,
    /// Sankey, chord or network (a whole-panel chart form).
    Flows,
    /// Pie or donut slices (a whole-panel chart form).
    Pie,
    /// Radar spokes and polygons (a whole-panel chart form).
    Radar,
    /// Polar rings and spokes (a whole-panel chart form).
    Polar,
    /// Wind-rose sectors (a whole-panel chart form).
    WindRose,
    /// Ternary triangle and gridlines (a whole-panel chart form).
    Ternary,
    /// Waterfall bars (a whole-panel chart form).
    Waterfall,
    /// Ridgeline densities (a whole-panel chart form).
    Ridgeline,
    /// Vector-field arrows (a whole-panel chart form).
    Vectors,
}

impl ElementKind {
    /// The layers a graph with panels can draw, in their default drawing
    /// order (back to front; see [`GraphSpec::elements`]).
    pub const PANEL_ORDER: [ElementKind; 18] = [
        Self::Heatmap,
        Self::Histogram,
        Self::Bins,
        Self::Contour,
        Self::Bars,
        Self::Violin,
        Self::BoxPlot,
        Self::Ellipse,
        Self::Hull,
        Self::FitLine,
        Self::Smoother,
        Self::Line,
        Self::Function,
        Self::Summary,
        Self::ProbabilityPlot,
        Self::DotPlot,
        Self::Points,
        Self::Brackets,
    ];

    /// Name used in saved files (the layer's member of a saved graph).
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Points => "points",
            Self::Smoother => "smoother",
            Self::FitLine => "fit",
            Self::BoxPlot => "box_plot",
            Self::Violin => "violin",
            Self::Summary => "summary",
            Self::Ellipse => "ellipse",
            Self::Contour => "contour",
            Self::Bins => "bins",
            Self::Function => "function",
            Self::Bars => "bars",
            Self::Line => "line",
            Self::Heatmap => "heatmap",
            Self::Treemap => "treemap",
            Self::Parallel => "parallel",
            Self::MultiVari => "multi_vari",
            Self::Histogram => "histogram",
            Self::DotPlot => "dot_plot",
            Self::ProbabilityPlot => "probability_plot",
            Self::Hull => "hull",
            Self::Brackets => "brackets",
            Self::ClusterMap => "cluster_map",
            Self::Estimation => "estimation",
            Self::Flows => "flows",
            Self::Pie => "pie",
            Self::Radar => "radar",
            Self::Polar => "polar",
            Self::WindRose => "wind_rose",
            Self::Ternary => "ternary",
            Self::Waterfall => "waterfall",
            Self::Ridgeline => "ridgeline",
            Self::Vectors => "vectors",
        }
    }

    /// The panel layer with this [`ElementKind::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::PANEL_ORDER.into_iter().find(|k| k.key() == key)
    }

    /// Display name of a panel layer (as on its toggle in the layer bar).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Points => "Points",
            Self::Smoother => "Smoother",
            Self::FitLine => "Fit line",
            Self::BoxPlot => "Box plot",
            Self::Violin => "Violin",
            Self::Summary => "Summary",
            Self::Ellipse => "Ellipse",
            Self::Contour => "Contours",
            Self::Bins => "Bins",
            Self::Function => "Formula",
            Self::Bars => "Bars",
            Self::Line => "Line",
            Self::Heatmap => "Heat map",
            Self::Histogram => "Histogram",
            Self::DotPlot => "Dot plot",
            Self::ProbabilityPlot => "Probability plot",
            Self::Hull => "Hulls",
            Self::Brackets => "Brackets",
            _ => "Chart",
        }
    }

    /// `true` for the layers that plot the horizontal column alone
    /// (histograms, dot plots, probability plots): the diagonal of a
    /// scatterplot matrix draws these and only these.
    #[must_use]
    pub fn is_one_variable(self) -> bool {
        matches!(self, Self::Histogram | Self::DotPlot | Self::ProbabilityPlot)
    }
}

/// The histogram a scatterplot matrix's diagonal shows when no one-variable
/// layer is on.
const MATRIX_HISTOGRAM: HistogramElement =
    HistogramElement { bins: HistogramBins::Auto, stacked: false, density: false };

/// A layer of a graph.
pub trait Element {
    /// Which element this is.
    fn kind(&self) -> ElementKind;

    /// The marks this element draws in `panel` (`state` gives selection).
    fn layer(&self, panel: &Panel, state: &RowStateManager) -> Layer;

    /// Draw this element for the whole of `df` as a single plot (every panel
    /// of the encoding's layout overlaid in one coordinate plane), on linear
    /// scales. Graph canvases with several panels call [`Element::layer`] per
    /// panel and draw with [`crate::render::draw_layer`] instead.
    #[cfg(feature = "egui")]
    fn render(
        &self,
        ui: &mut egui_plot::PlotUi,
        df: &stat_data::DataTable,
        state: &RowStateManager,
        encoding: &crate::encoding::EncodingPlan,
    ) {
        if let Ok(frame) = crate::frame::build_frame(df, state, encoding, &crate::frame::FrameOptions::default()) {
            for panel in &frame.panels {
                crate::render::draw_layer(ui, &self.layer(panel, state));
            }
        }
    }
}

/// Group a panel's points that take part in analyses by subgroup.
fn included_by_group(panel: &Panel) -> Vec<Vec<&PanelPoint>> {
    let mut groups: Vec<Vec<&PanelPoint>> = vec![Vec::new(); panel.groups.len().max(1)];
    for p in panel.points.iter().filter(|p| p.in_analysis()) {
        let last = groups.len() - 1;
        groups[p.group.min(last)].push(p);
    }
    groups
}

/// A map key for position `x` that orders like `x` and is equal for values
/// that agree after rounding to `1/resolution` (so `0.1 + 0.2` and `0.3`
/// share a slot). Any finite or infinite `x` gets its own key: rounding is
/// skipped where `x · resolution` overflows, and the key is the rounded
/// value's bit pattern, so it never saturates the way an integer cast does.
pub(crate) fn position_key(x: f64, resolution: f64) -> i64 {
    let scaled = x * resolution;
    let v = if scaled.is_finite() { scaled.round() / resolution } else { x };
    let v = if v == 0.0 { 0.0 } else { v };
    // Bits of an IEEE double, flipped for negatives, order like the values.
    let bits = i64::from_ne_bytes(v.to_bits().to_ne_bytes());
    bits ^ ((bits >> 63) & i64::MAX)
}

/// One horizontal position's points: `((x, subgroup), points)`.
type Slot<'a> = ((f64, usize), Vec<&'a PanelPoint>);

/// A panel's points that take part in analyses, grouped by horizontal
/// position (the dodged category slot, or the exact x) and subgroup, in
/// ascending x.
fn included_by_slot(panel: &Panel) -> Vec<Slot<'_>> {
    let mut slots: std::collections::BTreeMap<(i64, usize), (f64, Vec<&PanelPoint>)> =
        std::collections::BTreeMap::new();
    for p in panel.points.iter().filter(|p| p.in_analysis()) {
        // Keyed on a rounded copy so equal x group together; ordered by x.
        let key = (position_key(p.x, 1e9), p.group);
        slots.entry(key).or_insert_with(|| (p.x, Vec::new())).1.push(p);
    }
    slots.into_iter().map(|((_, g), (x, pts))| ((x, g), pts)).collect()
}

/// The points' vertical values in data units (undoing the axis scale), with
/// their frequencies and weights: summaries are statistics of the data, then
/// drawn on the scale.
fn data_obs(panel: &Panel, pts: &[&PanelPoint]) -> Vec<crate::summary::Obs> {
    pts.iter().map(|p| (panel.y.scale.inverse(p.y), p.freq, p.weight)).collect()
}

/// Color of a subgroup's lines: its own color when the panel is split,
/// otherwise the element's default.
fn group_color(panel: &Panel, g: usize, default: Rgba) -> Rgba {
    if panel.groups.len() > 1 { panel.groups[g].color } else { default }
}

/// Color of a subgroup's markers (the user's or the palette's), so lines
/// drawn for it match its points even when the panel is not split.
fn marker_color(panel: &Panel, g: usize) -> Rgba {
    panel.groups.get(g).or(panel.groups.first()).map_or(crate::palette::DEFAULT_POINT, |s| s.color)
}

/// Color of a key entry for lines that follow their subgroup's markers: the
/// markers' color when there is one subgroup, else a neutral ink (which the
/// dark appearance lightens, see [`crate::palette::for_dark`]).
fn key_color(panel: &Panel) -> Rgba {
    if panel.groups.len() == 1 { marker_color(panel, 0) } else { crate::palette::LINE }
}

/// Color of a key entry for marks drawn in `default` unless the panel is
/// split by color, when each subgroup takes its own color: then the entry is
/// a neutral ink, so it does not claim one color for marks drawn in several.
fn split_key_color(panel: &Panel, default: Rgba) -> Rgba {
    if panel.groups.len() > 1 { crate::palette::LINE } else { default }
}

/// Label prefix of a subgroup (empty for an unsplit panel).
fn group_prefix(panel: &Panel, g: usize) -> String {
    if panel.groups.len() > 1 { format!("{}: ", panel.groups[g].label) } else { String::new() }
}

/// `true` when both axes of the panel are continuous.
fn both_numeric(panel: &Panel) -> bool {
    !panel.x.is_categorical() && !panel.y.is_categorical()
}

/// `true` for categories (or nothing) across and numbers up.
fn categories_across(panel: &Panel) -> bool {
    panel.x.is_categorical() && !panel.y.is_categorical()
}

/// A probability as a percentage label (`95%`, `99.5%`).
fn percent(level: f64) -> String {
    format!("{}%", crate::view::format_value(level * 100.0))
}

// ---------------------------------------------------------------------------

/// A whole graph: encodings, axis options and the active elements with their
/// settings (what a graph canvas saves).
#[derive(Debug, Clone, PartialEq)]
pub struct GraphSpec {
    /// Column assignments.
    pub encoding: crate::encoding::EncodingPlan,
    /// Axis scales, page and row filter.
    pub options: crate::frame::FrameOptions,
    /// Column pickers shown on the graph.
    pub switchers: Vec<crate::encoding::ColumnSwitcher>,
    /// Points layer, if shown.
    pub points: Option<PointsElement>,
    /// Smoother layer, if shown.
    pub smoother: Option<SmootherElement>,
    /// Fit-line layer, if shown.
    pub fit: Option<FitLineElement>,
    /// Box-plot layer, if shown.
    pub box_plot: Option<BoxPlotElement>,
    /// Violin layer, if shown.
    pub violin: Option<ViolinElement>,
    /// Summary-statistic layer, if shown.
    pub summary: Option<SummaryElement>,
    /// Density-ellipse layer, if shown.
    pub ellipse: Option<EllipseElement>,
    /// Density-contour layer, if shown.
    pub contour: Option<ContourElement>,
    /// Binned-count layer, if shown.
    pub bins: Option<BinElement>,
    /// Formula-curve layer, if shown.
    pub function: Option<FunctionElement>,
    /// Bar layer, if shown.
    pub bars: Option<BarElement>,
    /// Line-chart layer, if shown.
    pub line: Option<LineElement>,
    /// Heat-map layer, if shown.
    pub heatmap: Option<HeatmapElement>,
    /// Histogram layer, if shown.
    pub histogram: Option<HistogramElement>,
    /// Dot-plot layer, if shown.
    pub dot_plot: Option<DotPlotElement>,
    /// Probability-plot layer, if shown.
    pub probability_plot: Option<ProbabilityPlotElement>,
    /// Panels with layers, or a whole-panel form (treemap, parallel coordinates).
    pub form: crate::forms::GraphForm,
    /// Where the key goes.
    pub legend: LegendPlacement,
    /// Statistic labels the user has dragged to a new place.
    pub label_positions: Vec<crate::view::LabelPosition>,
    /// Titles typed by the user, axis breaks and markup ([`crate::axes`]).
    pub axes: crate::axes::GraphAxes,
    /// Convex-hull layer, if shown.
    pub hull: Option<HullElement>,
    /// Significance-bracket layer, if shown.
    pub brackets: Option<BracketElement>,
    /// Settings of the estimation-plot form.
    pub estimation: crate::estimation::EstimationSettings,
    /// Drawing order set by the user, back to front: the listed layers are
    /// drawn in this order, in the places such layers take in the default
    /// order; others keep their default places (empty: the default order).
    pub layer_order: Vec<ElementKind>,
}

/// Where a graph's key is placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum LegendPlacement {
    /// Below the panels.
    #[default]
    Below,
    /// To the right of the panels (on screen; exports put it below).
    Right,
    /// Not shown.
    Hidden,
}

impl LegendPlacement {
    /// Every placement.
    pub const ALL: [LegendPlacement; 3] = [LegendPlacement::Below, LegendPlacement::Right, LegendPlacement::Hidden];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Below => "Below the graph",
            Self::Right => "Right of the graph",
            Self::Hidden => "Hidden",
        }
    }

    /// Short name for saving.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Below => "below",
            Self::Right => "right",
            Self::Hidden => "hidden",
        }
    }

    /// The placement with this [`LegendPlacement::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.key() == key)
    }
}

impl Default for GraphSpec {
    fn default() -> Self {
        Self {
            encoding: crate::encoding::EncodingPlan::default(),
            options: crate::frame::FrameOptions::default(),
            switchers: Vec::new(),
            points: Some(PointsElement::default()),
            smoother: None,
            fit: None,
            box_plot: None,
            violin: None,
            summary: None,
            ellipse: None,
            contour: None,
            bins: None,
            function: None,
            bars: None,
            line: None,
            heatmap: None,
            histogram: None,
            dot_plot: None,
            probability_plot: None,
            form: crate::forms::GraphForm::Panels,
            legend: LegendPlacement::Below,
            label_positions: Vec::new(),
            axes: crate::axes::GraphAxes::default(),
            hull: None,
            brackets: None,
            estimation: crate::estimation::EstimationSettings::default(),
            layer_order: Vec::new(),
        }
    }
}

impl GraphSpec {
    /// The active elements in drawing order: area fills at the back (heat
    /// maps, bins, contours, bars, violins, boxes), then bands and lines, then
    /// summaries and points on top. A treemap or parallel-coordinates form
    /// draws its own element with points (when on) over it.
    #[must_use]
    pub fn elements(&self) -> Vec<&dyn Element> {
        let mut out: Vec<&dyn Element> = Vec::new();
        match self.form {
            crate::forms::GraphForm::Panels => {}
            crate::forms::GraphForm::Treemap(settings) => {
                out.push(match settings.labels {
                    true => &crate::forms::TreemapElement { labels: true },
                    false => &crate::forms::TreemapElement { labels: false },
                });
                if let Some(p) = &self.points {
                    out.push(p);
                }
                return out;
            }
            crate::forms::GraphForm::Parallel => {
                out.push(&crate::forms::ParallelElement);
                if let Some(p) = &self.points {
                    out.push(p);
                }
                return out;
            }
            crate::forms::GraphForm::MultiVari => {
                if let Some(p) = &self.points {
                    out.push(p);
                }
                out.push(&crate::forms::MultiVariElement);
                return out;
            }
            crate::forms::GraphForm::ScatterMatrix => {
                if self.histogram.is_none() && self.dot_plot.is_none() && self.probability_plot.is_none() {
                    out.push(&MATRIX_HISTOGRAM);
                }
            }
            crate::forms::GraphForm::ClusterMap(_) => {
                out.push(&crate::cluster_map::ClusterMapElement);
                return out;
            }
            crate::forms::GraphForm::Estimation => {
                if let Some(p) = &self.points {
                    out.push(p);
                }
                out.push(&crate::estimation::EstimationElement);
                return out;
            }
            crate::forms::GraphForm::Sankey(_)
            | crate::forms::GraphForm::Chord(_)
            | crate::forms::GraphForm::Network(_) => {
                out.push(&crate::flows::FlowElement);
                return out;
            }
            crate::forms::GraphForm::Pie(_) => return vec![&crate::forms::PieElement],
            crate::forms::GraphForm::Radar(_) => return vec![&crate::forms::RadarElement],
            crate::forms::GraphForm::WindRose(_) => return vec![&crate::forms::WindRoseElement],
            crate::forms::GraphForm::Waterfall(_) => return vec![&crate::forms::WaterfallElement],
            crate::forms::GraphForm::Ridgeline(_) => return vec![&crate::forms::RidgelineElement],
            crate::forms::GraphForm::Polar(_)
            | crate::forms::GraphForm::Ternary
            | crate::forms::GraphForm::VectorField(_) => {
                out.push(match self.form {
                    crate::forms::GraphForm::Polar(_) => &crate::forms::PolarElement,
                    crate::forms::GraphForm::Ternary => &crate::forms::TernaryElement,
                    _ => &crate::forms::VectorElement,
                });
                if let Some(p) = &self.points {
                    out.push(p);
                }
                return out;
            }
        }
        macro_rules! push {
            ($($field:ident),*) => {$(
                if let Some(e) = &self.$field {
                    out.push(e);
                }
            )*};
        }
        push!(
            heatmap,
            histogram,
            bins,
            contour,
            bars,
            violin,
            box_plot,
            ellipse,
            hull,
            fit,
            smoother,
            line,
            function,
            summary,
            probability_plot,
            dot_plot,
            points,
            brackets
        );
        crate::plot_style::reorder_slots(out, |e| self.layer_order.iter().position(|k| *k == e.kind()))
    }

    /// The active layers of a graph with panels, back to front, as drawn
    /// (empty for whole-panel chart forms, which draw in a fixed order).
    #[must_use]
    pub fn layer_kinds(&self) -> Vec<ElementKind> {
        if self.form != crate::forms::GraphForm::Panels {
            return Vec::new();
        }
        self.elements().iter().map(|e| e.kind()).filter(|k| Self::is_panel_layer(*k)).collect()
    }

    fn is_panel_layer(kind: ElementKind) -> bool {
        ElementKind::PANEL_ORDER.contains(&kind)
    }

    /// Draw layer `kind` one place nearer the front (`forward`) or the back,
    /// among the active layers. Returns `false` when it cannot move.
    pub fn move_layer(&mut self, kind: ElementKind, forward: bool) -> bool {
        let mut kinds = self.layer_kinds();
        let Some(i) = kinds.iter().position(|k| *k == kind) else { return false };
        let j = if forward { Some(i + 1).filter(|j| *j < kinds.len()) } else { i.checked_sub(1) };
        let Some(j) = j else { return false };
        kinds.swap(i, j);
        // Keep the places of layers that are off now, so turning one back on
        // returns it where it was.
        let mut order: Vec<ElementKind> = self.layer_order.iter().copied().filter(|k| !kinds.contains(k)).collect();
        order.splice(0..0, kinds);
        self.layer_order = order;
        true
    }

    /// The annotations the user placed on this graph.
    #[must_use]
    pub fn annotations(&self) -> &[Annotation] {
        self.axes.style.as_ref().map_or(&[], |s| s.annotations.items.as_slice())
    }

    /// The key changes the user made.
    #[must_use]
    pub fn key_look(&self) -> crate::plot_style::KeyLook {
        self.axes.style.as_ref().map(|s| s.key.clone()).unwrap_or_default()
    }

    /// Change the graph's plot style with `edit` (creating it when the graph
    /// has none, dropping it when it ends up at its defaults).
    pub fn edit_style(&mut self, edit: impl FnOnce(&mut crate::plot_style::PlotStyle)) {
        let mut style = self.axes.style.take().unwrap_or_default();
        edit(&mut style);
        self.axes.style = (style != crate::plot_style::PlotStyle::default()).then_some(style);
    }

    /// How positions on panel `panel` map to plot coordinates when it shows
    /// `view` (drawn units, after breaks).
    #[must_use]
    pub fn panel_place<'a>(&self, panel: &'a Panel, view: ([f64; 2], [f64; 2])) -> PanelPlace<'a> {
        let (x_break, y_break) = self.panel_breaks(panel);
        PanelPlace { x: &panel.x, y: &panel.y, x_break, y_break, view }
    }

    /// The layer drawing the annotations of panel `index` (`panel`) when it
    /// shows `view`.
    #[must_use]
    pub fn annotation_layer(&self, index: usize, panel: &Panel, view: ([f64; 2], [f64; 2])) -> Layer {
        annotation_layer(self.annotations(), index, &self.panel_place(panel, view))
    }

    /// The key's entries before the user's changes, each with an identifier
    /// that stays the same while the data change: right-axis series, color
    /// levels, shapes and subgroups (`color/…`, `shape/…`, `group/…`, by
    /// column title and level), then each layer's entries once each in
    /// drawing order (`<layer key>/<text>`). `layers` are
    /// `[panel][element]` as from [`GraphSpec::layers`].
    #[must_use]
    pub fn key_items(
        &self,
        frame: &crate::frame::Frame,
        layers: &[Vec<Layer>],
        right: Option<&crate::axes::RightAxis>,
    ) -> Vec<(String, String, crate::layer::Rgba, crate::layer::KeyGlyph)> {
        use crate::layer::KeyGlyph;
        let mut out: Vec<(String, String, crate::layer::Rgba, KeyGlyph)> = Vec::new();
        if let Some(r) = right {
            for e in &r.series {
                out.push((format!("axis/{}", e.label), e.label.clone(), e.color, e.glyph));
            }
        }
        let legend = &frame.legend;
        if let Some(crate::frame::ColorKey::Levels { title, entries }) = &legend.color {
            for (level, color, shape) in entries {
                out.push((format!("color/{title}/{level}"), format!("{title} {level}"), *color, KeyGlyph::Marker(*shape, true)));
            }
        }
        if let Some((title, entries)) = &legend.shape {
            for (level, shape) in entries {
                out.push((
                    format!("shape/{title}/{level}"),
                    format!("{title} {level}"),
                    crate::palette::LABEL,
                    KeyGlyph::Marker(*shape, false),
                ));
            }
        }
        if let Some((title, groups)) = &legend.groups {
            for g in groups {
                out.push((
                    format!("group/{title}/{}", g.label),
                    format!("{title} {}", g.label),
                    g.color,
                    KeyGlyph::Marker(g.shape, true),
                ));
            }
        }
        let kinds: Vec<ElementKind> = self.elements().iter().map(|e| e.kind()).collect();
        let kind_of = |k: usize| kinds.get(k).map_or("layer", |k| k.key());
        let main = layers.iter().flat_map(|per_panel| per_panel.iter().enumerate());
        let right_layers = right
            .into_iter()
            .flat_map(|r| r.layers.iter().flatten().flat_map(|per_panel| per_panel.iter().enumerate()));
        for (k, layer) in main.chain(right_layers) {
            for e in &layer.key {
                if !out.iter().any(|x| x.1 == e.label) {
                    out.push((format!("{}/{}", kind_of(k), e.label), e.label.clone(), e.color, e.glyph));
                }
            }
        }
        out
    }

    /// The key as shown: [`GraphSpec::key_items`] with the user's changes
    /// (hidden entries left out, renamed ones with the user's text, in the
    /// user's order).
    #[must_use]
    pub fn key_shown(
        &self,
        frame: &crate::frame::Frame,
        layers: &[Vec<Layer>],
        right: Option<&crate::axes::RightAxis>,
    ) -> Vec<(String, String, crate::layer::Rgba, crate::layer::KeyGlyph)> {
        let items = self.key_items(frame, layers, right).into_iter().map(|(id, t, c, g)| (id, t, (c, g))).collect();
        self.key_look().apply(items).into_iter().map(|(id, t, (c, g))| (id, t, c, g)).collect()
    }

    /// Title of a panel's vertical axis: the column's (with its scale), or,
    /// with no vertical column, what a one-variable layer plots there
    /// (standard quantiles, density or counts).
    #[must_use]
    pub fn y_title(&self, axis: &crate::frame::AxisSpec) -> String {
        if !axis.name.is_empty() || self.form != crate::forms::GraphForm::Panels {
            return axis.title();
        }
        if let Some(p) = &self.probability_plot {
            format!("Standard quantile ({} fit)", p.family.label())
        } else if let Some(h) = &self.histogram {
            if h.density { "Density" } else { "Count" }.to_owned()
        } else if self.dot_plot.is_some() {
            "Rows".to_owned()
        } else {
            axis.title()
        }
    }

    /// The frame of this graph for `table` ([`crate::frame::build_frame`] with
    /// the graph's encodings and options).
    ///
    /// # Errors
    /// As [`crate::frame::build_frame`].
    pub fn frame(
        &self,
        table: &stat_data::DataTable,
        state: &RowStateManager,
    ) -> stat_core::Result<crate::frame::Frame> {
        match self.form {
            crate::forms::GraphForm::Panels => {
                let mut frame = crate::frame::build_frame(table, state, &self.encoding, &self.options)?;
                // Say so when a broken axis mixes scales (its tick labels alone
                // would not tell).
                let notes = [
                    self.axes.x_breaks.scales_note("horizontal", self.options.x_scale),
                    self.axes.y_breaks.scales_note("vertical", self.options.y_scale),
                ];
                frame.notes.extend(notes.into_iter().flatten());
                Ok(frame)
            }
            crate::forms::GraphForm::Treemap(settings) => {
                crate::forms::build_treemap_frame(table, state, &self.encoding, &self.options, settings)
            }
            crate::forms::GraphForm::Parallel => {
                crate::forms::build_parallel_frame(table, state, &self.encoding, &self.options)
            }
            crate::forms::GraphForm::MultiVari => {
                crate::forms::build_multivari_frame(table, state, &self.encoding, &self.options)
            }
            crate::forms::GraphForm::ScatterMatrix => {
                crate::forms::build_scatter_matrix_frame(table, state, &self.encoding, &self.options)
            }
            crate::forms::GraphForm::ClusterMap(settings) => {
                crate::cluster_map::build_cluster_map_frame(table, state, &self.encoding, &self.options, settings)
            }
            crate::forms::GraphForm::Estimation => {
                crate::estimation::build_estimation_frame(table, state, &self.encoding, &self.options, &self.estimation)
            }
            crate::forms::GraphForm::Sankey(settings) => {
                crate::flows::build_sankey_frame(table, state, &self.encoding, &self.options, settings)
            }
            crate::forms::GraphForm::Chord(settings) => {
                crate::flows::build_chord_frame(table, state, &self.encoding, &self.options, settings)
            }
            crate::forms::GraphForm::Network(settings) => {
                crate::flows::build_network_frame(table, state, &self.encoding, &self.options, settings)
            }
            crate::forms::GraphForm::Pie(s) => {
                crate::forms::build_pie_frame(table, state, &self.encoding, &self.options, s)
            }
            crate::forms::GraphForm::Radar(s) => {
                crate::forms::build_radar_frame(table, state, &self.encoding, &self.options, s)
            }
            crate::forms::GraphForm::Polar(s) => {
                crate::forms::build_polar_frame(table, state, &self.encoding, &self.options, s)
            }
            crate::forms::GraphForm::WindRose(s) => {
                crate::forms::build_wind_rose_frame(table, state, &self.encoding, &self.options, s)
            }
            crate::forms::GraphForm::Ternary => {
                crate::forms::build_ternary_frame(table, state, &self.encoding, &self.options)
            }
            crate::forms::GraphForm::Waterfall(s) => {
                crate::forms::build_waterfall_frame(table, state, &self.encoding, &self.options, s)
            }
            crate::forms::GraphForm::Ridgeline(s) => {
                crate::forms::build_ridgeline_frame(table, state, &self.encoding, &self.options, s)
            }
            crate::forms::GraphForm::VectorField(s) => {
                crate::forms::build_vector_frame(table, state, &self.encoding, &self.options, s)
            }
        }
    }

    /// Every column the graph uses: encodings, filter conditions and switcher
    /// choices.
    #[must_use]
    pub fn columns(&self) -> Vec<&str> {
        let mut out = self.encoding.columns();
        let extra = self
            .options
            .filter
            .columns()
            .into_iter()
            .chain(self.switchers.iter().flat_map(|s| s.choices.iter().map(String::as_str)));
        for c in extra {
            if !out.contains(&c) {
                out.push(c);
            }
        }
        out
    }

    /// Follow a column renamed from `old` to `new` everywhere in the graph.
    pub fn rename(&mut self, old: &str, new: &str) {
        self.encoding.rename(old, new);
        self.options.filter.rename(old, new);
        for s in &mut self.switchers {
            s.rename(old, new);
        }
    }

    /// Every element's layer for every panel of `frame` (`[panel][element]`).
    /// In a scatterplot matrix the diagonal panels draw only the
    /// one-variable elements and the other panels only the rest (the
    /// skipped ones give empty layers, so indices stay aligned). Axis breaks
    /// ([`crate::axes::GraphAxes`]) are applied to the drawn coordinates.
    #[must_use]
    pub fn layers(&self, frame: &crate::frame::Frame, state: &RowStateManager) -> Vec<Vec<Layer>> {
        let elements = self.elements();
        let matrix = self.form == crate::forms::GraphForm::ScatterMatrix;
        frame
            .panels
            .iter()
            .map(|p| {
                let diagonal = matches!(p.extra, crate::frame::PanelExtra::MatrixDiagonal);
                let (bx, by) = self.panel_breaks(p);
                let mut out: Vec<Layer> = elements
                    .iter()
                    .map(|e| {
                        if matrix && diagonal != e.kind().is_one_variable() {
                            Layer::default()
                        } else {
                            let mut layer = e.layer(p, state);
                            let hidden = crate::axes::apply_breaks(&mut layer, bx.as_ref(), by.as_ref());
                            if hidden > 0 {
                                layer.notes.push(crate::axes::break_note(hidden));
                            }
                            layer
                        }
                    })
                    .collect();
                if let Some(mut refs) = self.panel_reference(frame, p) {
                    crate::axes::apply_breaks(&mut refs.back, bx.as_ref(), by.as_ref());
                    crate::axes::apply_breaks(&mut refs.front, bx.as_ref(), by.as_ref());
                    out.insert(0, refs.back);
                    out.push(refs.front);
                }
                out
            })
            .collect()
    }

    /// The user's reference lines and bands for `panel` of `frame` (only
    /// panels of the ordinary form have them), or `None` when there are
    /// none: bands go behind every element, lines over them
    /// ([`reference::reference_layers`]).
    #[must_use]
    pub fn panel_reference(
        &self,
        frame: &crate::frame::Frame,
        panel: &crate::frame::Panel,
    ) -> Option<reference::ReferenceLayers> {
        let look = &self.axes.style.as_ref()?.reference;
        if look.is_empty() || self.form != crate::forms::GraphForm::Panels {
            return None;
        }
        let data = if look.uses_data() {
            reference::panel_reference_data(frame, panel)
        } else {
            reference::ReferenceData::default()
        };
        let axes = reference::ReferenceAxes {
            x: panel.x.scale,
            y: panel.y.scale,
            x_categorical: panel.x.is_categorical(),
            y_categorical: panel.y.is_categorical(),
        };
        Some(reference::reference_layers(look, &data, axes))
    }

    /// The breaks of `panel`'s horizontal and vertical axes in drawn units
    /// (only panels of the ordinary form have breaks).
    #[must_use]
    pub fn panel_breaks(&self, panel: &crate::frame::Panel) -> (Option<crate::axes::Gap>, Option<crate::axes::Gap>) {
        if self.form != crate::forms::GraphForm::Panels {
            return (None, None);
        }
        let [ex, ey] = panel.data_extent;
        (self.axes.break_of(&panel.x, false, ex), self.axes.break_of(&panel.y, true, ey))
    }

    /// Remember that the movable label `id` of panel `panel` sits at
    /// `fraction` of the panel's view (0–1 across and up), replacing any
    /// earlier place.
    pub fn place_label(&mut self, panel: usize, id: &str, fraction: [f64; 2]) {
        let at = [fraction[0].clamp(0.0, 1.0), fraction[1].clamp(0.0, 1.0)];
        self.label_positions.retain(|l| !(l.panel == panel && l.id == id));
        self.label_positions.push(crate::view::LabelPosition { panel, id: id.to_owned(), at });
    }
}

#[cfg(test)]
mod tests {
    use super::position_key;

    /// The slot key orders like the value, groups values equal to nine
    /// decimals, and keeps large positions apart.
    #[test]
    fn position_keys_order_and_group() {
        let k = |x: f64| position_key(x, 1e9);
        assert_eq!(k(0.1 + 0.2), k(0.3));
        assert_eq!(k(-0.0), k(0.0));
        assert_ne!(k(1e10), k(2e10));
        assert_ne!(k(1e300), k(f64::MAX));
        let xs = [f64::NEG_INFINITY, -1e300, -2e10, -1.0, -1e-6, 0.0, 1e-6, 1.0, 9.3e9, 1e10, 1e300, f64::INFINITY];
        for w in xs.windows(2) {
            assert!(k(w[0]) < k(w[1]), "{} < {}", w[0], w[1]);
        }
        assert_ne!(position_key(1e12 / 0.1, 1e6), position_key((1e12 + 1.0) / 0.1, 1e6));
    }
}
