//! The graph canvas engine: a grammar-of-graphics pipeline from a data table
//! and a [`EncodingPlan`] (which column drives which position, panel split,
//! color, size, shape or subgroup) to panels of drawable marks.
//!
//! * [`axes`] — user titles, axis breaks and the right-hand vertical axis;
//!   [`axis_breaks`] — several breaks per axis, a scale and share per segment.
//! * [`encoding`] — column assignments to visual channels.
//! * [`export`] — SVG, PDF and EPS files; the summary table.
//! * [`expr`] — formulas in x for function curves.
//! * [`figure_page`] — several figures on one page with panel letters.
//! * [`filter`] — a graph's own row filter (show, analyse, select).
//! * [`scale`] — linear, log and square-root axis scales.
//! * [`forms`] — treemaps and parallel coordinates.
//! * [`cluster_map`], [`estimation`], [`flows`] — clustered heat maps,
//!   estimation plots, Sankey and chord diagrams and networks.
//! * [`forms`] — whole-panel chart forms: treemaps, parallel coordinates,
//!   multi-vari charts, scatterplot matrices, pie, radar, polar, wind-rose,
//!   ternary, waterfall, ridgeline and vector-field charts.
//! * [`frame`] — panels (small multiples), positions, marker styles, key.
//! * [`element`] — composable layers: points, fit lines, smoothers, box and
//!   violin plots, summaries, density ellipses and contours, binned counts,
//!   formula curves; [`GraphSpec`] combines them.
//! * [`density`] — kernel densities, contour lines, density ellipses.
//! * [`interpolate`] — surfaces through scattered points: Delaunay
//!   triangulation with linear interpolation, thin-plate splines.
//! * [`loess`] — locally weighted regression with a confidence band.
//! * [`math_text`] — math in labels: subscripts, superscripts, Greek letters
//!   and fractions, laid out once for the screen and every file format.
//! * [`stats`] — the computations: jitter, Tukey box statistics, polynomial
//!   fits with confidence bands, cubic smoothing splines.
//! * [`summary`] — group statistics and error intervals.
//! * [`axis_settings`] — user settings for plot axes, tick positions and labels.
//! * [`plot_style`] — everything a user sets on one plot, kept with the plot
//!   in projects, dashboards, scripts and graph files.
//! * [`view`] — linked axis ranges, row positions for selection, ticks.
//! * [`time_scale`] — date-time axes: calendar tick steps, positions, labels.
//! * [`point_raster`] — point clouds above the point budget drawn as a
//!   density picture.
//! * [`layer`] and [`palette`] — toolkit-independent marks and colors.
//! * `render` (feature `egui`) — draws layers with `egui_plot`.
//!
//! Nothing here depends on a GUI unless the `egui` feature is enabled, so
//! every computation is unit tested headlessly.

pub mod axes;
pub mod axis_breaks;
pub mod axis_settings;
pub mod bitmap;
pub mod category_labels;
pub mod cluster_map;
pub mod contour;
pub mod density;
pub mod element;
pub mod encoding;
pub mod estimation;
pub mod export;
pub(crate) mod export_axes;
pub mod expr;
pub mod figure_page;
pub mod filter;
pub mod flows;
pub mod forms;
pub mod frame;
pub mod image_plot;
pub mod interpolate;
pub mod layer;
pub mod loess;
pub mod math_text;
pub mod palette;
pub mod plot_style;
pub mod point_raster;
pub mod raster;
#[cfg(feature = "egui")]
pub mod render;
pub mod row_styles;
pub mod scale;
pub mod stats;
pub mod summary;
pub mod time_scale;
pub mod view;

pub use axes::{AxisBreak, GraphAxes, RightAxis};
pub use axis_breaks::{AxisBreaks, AxisSegment, BreakMark, BrokenAxis};
pub use category_labels::{CategoryLabels, CategoryLayout, LabelTurn};
pub use cluster_map::ClusterMapSettings;
pub use contour::{ColorBar, ContourDrawing, ContourField, ContourStyle, Isoband, draw_contours};
pub use element::{
    ANNOTATION_INK, ANNOTATION_WIDTH, Annotation, AnnotationAnchor, AnnotationShape, PanelPlace, annotation_layer,
    BarElement, BarLayout, BinElement, BoxPlotElement, BracketElement, BracketLabel, BracketTest, ContourElement,
    DotPlotElement, Element, ElementKind, EllipseElement, FitLabels, FitLineElement, FunctionElement, GraphSpec,
    HeatmapElement, HistogramBins, HistogramElement, HullElement, LegendPlacement, LineConnect, LineElement,
    PointsElement, ProbabilityPlotElement, ReferenceBand, ReferenceLine, ReferenceLook, ReferenceOrientation,
    ReferenceScope, ReferenceStat, ReferenceValue, SmoothMethod, SmootherElement, SummaryElement, ViolinElement,
};
pub use encoding::{ColumnSwitcher, EncodingPlan, Role, is_continuous};
pub use estimation::{EstimationInterval, EstimationSettings};
pub use filter::{FilterCondition, LocalFilter};
pub use row_styles::{RowStyleScheme, row_style_key};
pub use flows::{FlowSettings, NetworkSettings};
pub use forms::{
    AngleUnit, GraphForm, PieSettings, PolarSettings, RadarSettings, RidgelineSettings, TreemapSettings,
    VectorSettings, WaterfallSettings, WindRoseSettings,
};
pub use frame::{
    AxisKind, AxisSpec, ColorKey, DEFAULT_RADIUS, Frame, FrameOptions, Legend, MarkerStyle, Panel, PanelExtra,
    PanelPoint, ParallelAxis, Tile, build_frame,
};
pub use image_plot::{RasterMark, RasterSpec};
pub use layer::{KeyEntry, KeyGlyph, Layer, LinePattern, Mark, MarkShape, Ramp, Region, Rgba, TextAnchor};
pub use math_text::{MathLayout, MathNode, MathText};
pub use scale::AxisScale;
pub use stats::{
    BoxStats, JitterKind, PolyFit, SmoothingSpline, box_stats, box_stats_weighted, jitter_offset, poly_fit,
    poly_fit_weighted, weighted_quantile,
};
pub use view::{
    LabelPosition, SharedRanges, auto_range, place_labels, region_rows, rows_in_box, rows_in_polygon, scaled_ticks,
    tick_label,
};
