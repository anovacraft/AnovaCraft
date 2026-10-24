//! Report content: what a result section shows, independent of how it is drawn.
//!
//! The GUI renders [`ReportSection`]s as collapsible sections. Tables arrive
//! already formatted as text; plots arrive as data series in plot coordinates,
//! each point tagged with its dataset row so views can apply shared row state.

/// A table of pre-formatted cells.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextTable {
    /// Column headings.
    pub columns: Vec<String>,
    /// Rows of cells; every row has `columns.len()` cells.
    pub rows: Vec<Vec<String>>,
}

impl TextTable {
    /// A table with the given headings and no rows.
    #[must_use]
    pub fn new<S: Into<String>>(columns: impl IntoIterator<Item = S>) -> Self {
        Self { columns: columns.into_iter().map(Into::into).collect(), rows: Vec::new() }
    }

    /// Append a row.
    ///
    /// # Panics
    /// If the row length differs from the number of columns.
    pub fn push<S: Into<String>>(&mut self, row: impl IntoIterator<Item = S>) {
        let row: Vec<String> = row.into_iter().map(Into::into).collect();
        assert_eq!(row.len(), self.columns.len(), "row length must match the number of columns");
        self.rows.push(row);
    }

    /// The table as tab-separated text with a line of headings, for pasting
    /// into a spreadsheet or document. Cells are written as a spreadsheet
    /// reads them: the typographic minus (−) becomes `-`, a lone dash (—,
    /// "not applicable") an empty cell, and a field holding a tab, line break
    /// or quote is quoted.
    #[must_use]
    pub fn to_tsv(&self) -> String {
        let mut out = String::new();
        for line in std::iter::once(&self.columns).chain(&self.rows) {
            let fields: Vec<String> = line.iter().map(|c| tsv_field(c)).collect();
            out.push_str(&fields.join("\t"));
            out.push('\n');
        }
        out
    }

    /// The table as a data table named `name`: numbers become numeric
    /// columns, anything else text (as when pasting into a new table). An
    /// empty heading becomes `Column n`, and repeated headings get a suffix.
    ///
    /// # Errors
    /// When the table has no rows or no columns.
    pub fn to_data_table(&self, name: &str) -> stat_core::Result<stat_data::DataTable> {
        if self.rows.is_empty() || self.columns.is_empty() {
            return Err(stat_core::StatError::InvalidArgument { message: "the table has no rows".into() });
        }
        let mut names: Vec<String> = Vec::new();
        for (j, heading) in self.columns.iter().enumerate() {
            let base = if heading.trim().is_empty() { format!("Column {}", j + 1) } else { heading.trim().to_owned() };
            let mut candidate = base.clone();
            let mut n = 2;
            while names.contains(&candidate) {
                candidate = format!("{base} {n}");
                n += 1;
            }
            names.push(candidate);
        }
        let rows: Vec<Vec<String>> = self.rows.iter().map(|r| r.iter().map(|c| tsv_field_value(c)).collect()).collect();
        stat_data::table_from_records_with_notes(name, names, rows).map(|(t, _)| t)
    }

    /// Cell text by row index and column heading, if both exist.
    #[must_use]
    pub fn cell(&self, row: usize, column: &str) -> Option<&str> {
        let j = self.columns.iter().position(|c| c == column)?;
        self.rows.get(row).map(|r| r[j].as_str())
    }
}

/// A report cell as a spreadsheet reads it (see [`TextTable::to_tsv`]).
fn tsv_field_value(cell: &str) -> String {
    let t = cell.trim();
    if t == "—" { String::new() } else { t.replace('−', "-") }
}

/// [`tsv_field_value`], quoted when it holds a tab, line break or quote.
fn tsv_field(cell: &str) -> String {
    let v = tsv_field_value(cell);
    if v.contains(['\t', '\n', '\r', '"']) { format!("\"{}\"", v.replace('"', "\"\"")) } else { v }
}

/// One plotted observation, tagged with its dataset row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowPoint {
    /// Dataset row index.
    pub row: usize,
    /// Horizontal coordinate.
    pub x: f64,
    /// Vertical coordinate.
    pub y: f64,
}

/// A polyline in plot coordinates.
pub type Polyline = Vec<[f64; 2]>;

/// Data for a scatter plot with optional overlay curves.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlotSpec {
    /// Horizontal axis label.
    pub x_label: String,
    /// Vertical axis label.
    pub y_label: String,
    /// Observations (rows with complete data, including excluded rows; views
    /// decide how to draw excluded and hidden rows).
    pub points: Vec<RowPoint>,
    /// Fitted curve or reference line, as a polyline.
    pub line: Option<Polyline>,
    /// What `line` is, for the key (`None`: "Fit", or "Means" with groups).
    pub line_name: Option<String>,
    /// Lower and upper confidence curves for the mean, as polylines.
    pub band: Option<(Polyline, Polyline)>,
    /// What `band` is, for the key (`None`: "Confidence band").
    pub band_name: Option<String>,
    /// What the ungrouped points are, for the key (`None`: "Rows").
    pub point_name: Option<String>,
    /// Optional grouping of the points (e.g. by operator), drawn with a distinct
    /// color and marker shape per group.
    pub groups: Option<PointGroups>,
    /// Explicit horizontal-axis ticks `(position, label)`, replacing the numeric
    /// axis (e.g. part numbers in a variability chart). `None` keeps the default.
    pub x_ticks: Option<Vec<(f64, String)>>,
    /// Explicit vertical-axis ticks `(position, label)` (e.g. the percentages
    /// of a probability plot's transformed scale). `None` keeps the default.
    pub y_ticks: Option<Vec<(f64, String)>>,
    /// Side-by-side panes the horizontal axis is divided into (empty for none).
    /// Views draw a divider between consecutive panes and each pane's label.
    pub panes: Vec<PlotPane>,
    /// Labelled markers not tied to dataset rows (e.g. category positions on a
    /// correspondence map).
    pub annotations: Option<Annotations>,
    /// Date-time display of the horizontal axis's column, so views tick it on
    /// the calendar and label it with dates (`None` for numeric axes).
    pub x_time: Option<stat_data::TemporalDisplay>,
    /// Date-time display of the vertical axis's column.
    pub y_time: Option<stat_data::TemporalDisplay>,
    /// Extra line work drawn thinly under the points (e.g. dendrogram branches
    /// or loading arrows); not tied to rows.
    pub segments: Vec<Polyline>,
    /// Thin lines joining points in their row order, drawn under the points
    /// in the points' colour (e.g. the replicates within each cell of a
    /// multi-vari chart, so a trend inside a cell is visible).
    pub point_runs: Vec<Polyline>,
    /// Style index of the first annotation group, past the point groups
    /// (e.g. 1 when ungrouped points share the plot with marked means, so
    /// the means never take the points' colour and marker).
    pub annotation_style_offset: usize,
    /// A ternary (simplex) plot: drawn at equal scale on both axes, without
    /// numeric ticks or grid (the triangle's corner labels name the
    /// components).
    pub simplex: bool,
    /// A contour map drawn under everything else (filled bands, level lines
    /// and their values, as its style asks).
    pub contour: Option<ContourMap>,
}

/// A surface drawn as a contour map under a plot: the sampled surface, the
/// plot's own number of levels, the quantity's name (the colour bar's
/// title) and how it is drawn (from the report's Plot settings).
#[derive(Debug, Clone, PartialEq)]
pub struct ContourMap {
    /// The surface over a triangle mesh.
    pub field: std::sync::Arc<stat_viz::ContourField>,
    /// About this many levels at round values, unless the style sets a number.
    pub levels: usize,
    /// What the surface's values are (e.g. "Predicted Yield").
    pub value_name: String,
    /// Fill, lines, labels, colour scale and levels.
    pub style: stat_viz::ContourStyle,
}

impl ContourMap {
    /// A map of `field` with about `levels` levels in the default style.
    #[must_use]
    pub fn new(field: stat_viz::ContourField, levels: usize, value_name: impl Into<String>) -> Self {
        Self {
            field: std::sync::Arc::new(field),
            levels,
            value_name: value_name.into(),
            style: stat_viz::ContourStyle::default(),
        }
    }

    /// The levels and their spacing in the current style.
    #[must_use]
    pub fn levels(&self) -> (Vec<f64>, f64) {
        let Some(data) = self.field.value_range() else { return (Vec::new(), 0.0) };
        let (lo, hi) = self.style.range.filter(|r| r.1 > r.0).unwrap_or(data);
        stat_viz::contour::nice_levels(lo, hi, self.style.levels.unwrap_or(self.levels))
    }

    /// What the map draws (colours in the dark variant when `dark`), with
    /// level values written to six significant digits.
    #[must_use]
    pub fn drawing(&self, dark: bool) -> stat_viz::ContourDrawing {
        stat_viz::draw_contours(&self.field, &self.style, self.levels, &self.value_name, dark, &|v| {
            crate::format::format_number(v, 6)
        })
    }
}

impl ReportSection {
    /// Draw this section's contour map (if it has one) in `style`, and keep
    /// a "contours every …" title in step with the levels.
    pub fn apply_contour_style(&mut self, style: &stat_viz::ContourStyle) {
        let SectionContent::Plot(p) = &mut self.content else { return };
        let Some(map) = &mut p.contour else { return };
        let (_, before) = map.levels();
        map.style = *style;
        let (_, after) = map.levels();
        if before > 0.0 && after > 0.0 && before != after {
            let fmt = |v: f64| format!("contours every {}", crate::format::format_number(v, 6));
            self.title = self.title.replacen(&fmt(before), &fmt(after), 1);
        }
    }

    /// Whether the section is a plot with a contour map.
    #[must_use]
    pub fn has_contours(&self) -> bool {
        matches!(&self.content, SectionContent::Plot(p) if p.contour.is_some())
    }
}

impl PlotSpec {
    /// Key label of [`line`](Self::line).
    #[must_use]
    pub fn line_label(&self) -> &str {
        self.line_name.as_deref().unwrap_or(if self.groups.is_some() { "Means" } else { "Fit" })
    }

    /// Key label of [`band`](Self::band).
    #[must_use]
    pub fn band_label(&self) -> &str {
        self.band_name.as_deref().unwrap_or("Confidence band")
    }

    /// Key label of the points when they are not grouped.
    #[must_use]
    pub fn point_label(&self) -> &str {
        self.point_name.as_deref().unwrap_or("Rows")
    }
}

/// How a fitted curve is drawn, so curves stay distinguishable without color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinePattern {
    /// Continuous line.
    Solid,
    /// Long dashes.
    Dashed,
    /// Dots.
    Dotted,
    /// Alternating dashes and dots.
    DashDot,
}

impl LinePattern {
    /// The engine's pattern for this one.
    #[must_use]
    pub fn viz(self) -> stat_viz::LinePattern {
        match self {
            Self::Solid => stat_viz::LinePattern::Solid,
            Self::Dashed => stat_viz::LinePattern::Dashed,
            Self::Dotted => stat_viz::LinePattern::Dotted,
            Self::DashDot => stat_viz::LinePattern::DashDot,
        }
    }

    /// This pattern for the engine's.
    #[must_use]
    pub fn from_viz(p: stat_viz::LinePattern) -> Self {
        match p {
            stat_viz::LinePattern::Solid => Self::Solid,
            stat_viz::LinePattern::Dashed => Self::Dashed,
            stat_viz::LinePattern::Dotted => Self::Dotted,
            stat_viz::LinePattern::DashDot => Self::DashDot,
        }
    }
}

/// A survival plot: step-function estimates with optional confidence bands
/// and fitted parametric curves, one curve per group, plus a marker for every
/// analysed row (filled for events, hollow for censored observations) placed
/// on its group's curve so it can be brushed.
#[derive(Debug, Clone, PartialEq)]
pub struct SurvivalPlotSpec {
    /// Horizontal axis label (the time column).
    pub x_label: String,
    /// Vertical axis label.
    pub y_label: String,
    /// One entry per group (a single entry when ungrouped).
    pub curves: Vec<SurvivalCurveSpec>,
    /// Analysed rows at `(time, Ŝ(time))`.
    pub points: Vec<RowPoint>,
    /// `true` where the row is censored, parallel to `points`.
    pub censored: Vec<bool>,
    /// Curve index of each point, parallel to `points`.
    pub membership: Vec<usize>,
    /// Name of the step estimate when it is not a survival probability
    /// (e.g. `"Mean cumulative function"`): views name the curve with it,
    /// call hollow markers ends of observation, and do not tie the vertical
    /// axis to [0, 1]. `None` for Kaplan–Meier curves.
    pub estimate: Option<String>,
    /// Number-at-risk table drawn under the plot, aligned with the time
    /// ticks (`None`: not shown).
    pub risk_table: Option<RiskTable>,
}

/// Data of a number-at-risk table: every curve's exit times (event or
/// censoring), so a view can count those still at risk at whatever time
/// ticks its axis shows.
#[derive(Debug, Clone, PartialEq)]
pub struct RiskTable {
    /// Also show the number censored since the previous tick.
    pub censored: bool,
    /// Per curve (parallel to [`SurvivalPlotSpec::curves`]): `(time,
    /// censored)` of each analysed row, sorted by time.
    pub exits: Vec<Vec<(f64, bool)>>,
}

impl RiskTable {
    /// Rows of curve `curve` still at risk at time `t` (time ≥ `t`).
    #[must_use]
    pub fn at_risk(&self, curve: usize, t: f64) -> usize {
        self.exits.get(curve).map_or(0, |e| e.len() - e.partition_point(|x| x.0 < t))
    }

    /// Rows of curve `curve` censored at a time in `(after, through]`.
    #[must_use]
    pub fn censored_between(&self, curve: usize, after: f64, through: f64) -> usize {
        self.exits.get(curve).map_or(0, |e| e.iter().filter(|x| x.1 && x.0 > after && x.0 <= through).count())
    }
}

/// One group's curves in a [`SurvivalPlotSpec`].
#[derive(Debug, Clone, PartialEq)]
pub struct SurvivalCurveSpec {
    /// Group label (empty when ungrouped).
    pub label: String,
    /// Vertices of the step function.
    pub steps: Polyline,
    /// Confidence band as constant pieces `[t_start, t_end, lower, upper]`
    /// (empty when not shown).
    pub band: Vec<[f64; 4]>,
    /// Fitted parametric survival curves: `(name, pattern, curve)`.
    pub fitted: Vec<(String, LinePattern, Polyline)>,
}

/// Labelled markers in plot coordinates, in groups with legend names.
#[derive(Debug, Clone, PartialEq)]
pub struct Annotations {
    /// Legend name of each group.
    pub groups: Vec<String>,
    /// `(x, y, label, group)`.
    pub items: Vec<(f64, f64, String, usize)>,
    /// Text drawn without a marker, centred at `(x, y)` (e.g. contour values).
    pub labels: Vec<(f64, f64, String)>,
}

/// One labelled pane of a faceted plot, spanning `[x_min, x_max]`.
#[derive(Debug, Clone, PartialEq)]
pub struct PlotPane {
    /// Pane heading (e.g. "Operator A").
    pub label: String,
    /// Left edge in plot coordinates.
    pub x_min: f64,
    /// Right edge in plot coordinates.
    pub x_max: f64,
}

/// Assignment of plotted points to named groups.
#[derive(Debug, Clone, PartialEq)]
pub struct PointGroups {
    /// What the groups are (e.g. the grouping column's name), for the legend.
    pub title: String,
    /// Group labels, indexed by group number.
    pub labels: Vec<String>,
    /// Group number of each point, parallel to [`PlotSpec::points`].
    pub membership: Vec<usize>,
    /// One optional polyline per group (e.g. that group's level means); an empty
    /// polyline draws nothing.
    pub lines: Vec<Polyline>,
}

impl PointGroups {
    /// Key label of group `g`: a bare number is prefixed with the title
    /// ("Operator 2"); a label that names itself ("Training", "γ = 0.1",
    /// "Instrument 1") is shown as it is.
    #[must_use]
    pub fn key_label(&self, g: usize) -> String {
        let label = self.labels.get(g).map_or_else(|| format!("Group {}", g + 1), Clone::clone);
        if self.title.is_empty() || label.trim().parse::<f64>().is_err() {
            label
        } else {
            format!("{} {label}", self.title)
        }
    }
}

/// One plotted point of a control chart.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartPoint {
    /// Sample number (1-based), the horizontal coordinate.
    pub sample: usize,
    /// Plotted statistic.
    pub value: f64,
    /// Dataset rows that make up this point (the subgroup, or the one or two
    /// observations behind an individual value or moving range).
    pub rows: Vec<usize>,
    /// Numbers of the run rules this point violates (empty if none).
    pub rules: Vec<u8>,
    /// Label of the point (from a label or subgroup column), if any.
    pub label: Option<String>,
    /// Short text written beside the point (e.g. a zone chart's cumulative
    /// score), if any.
    pub note: Option<String>,
    /// Flagged by the chart's own test rather than a numbered run rule (e.g.
    /// a zone chart's cumulative score reaching 8).
    pub signal: bool,
}

impl ChartPoint {
    /// `true` if the point is flagged by a run rule or the chart's own test.
    #[must_use]
    pub fn flagged(&self) -> bool {
        self.signal || !self.rules.is_empty()
    }
}

/// Limits holding over a stretch of samples of a control chart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LimitStep {
    /// Horizontal start (half a sample before the first sample it covers).
    pub from: f64,
    /// Horizontal end (half a sample after the last).
    pub to: f64,
    /// Center line.
    pub center: f64,
    /// Lower control limit.
    pub lower: f64,
    /// Upper control limit.
    pub upper: f64,
    /// Standard deviation of the plotted statistic, for the 1σ/2σ zone lines.
    pub zone_sigma: f64,
}

/// The start of a phase on a control chart.
#[derive(Debug, Clone, PartialEq)]
pub struct PhaseMark {
    /// Horizontal position of the boundary (half a sample before its first sample).
    pub at: f64,
    /// Phase name.
    pub label: String,
}

/// Data for drawing one control chart.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlChartSpec {
    /// Horizontal axis label (`"Sample"` for charts in time order).
    pub x_label: String,
    /// Vertical axis label (the plotted statistic).
    pub y_label: String,
    /// Labelled horizontal-axis ticks `(sample, label)` replacing the sample
    /// numbers (e.g. group names on an analysis-of-means chart); `None` keeps
    /// the numeric axis.
    pub x_ticks: Option<Vec<(f64, String)>>,
    /// Legend text for flagged points when they are flagged for crossing a
    /// limit rather than by numbered run rules; views then do not write rule
    /// numbers beside them. `None` for run-rule charts.
    pub flag_label: Option<String>,
    /// Legend text for the limits when they are not control limits (e.g.
    /// "Decision limits"); `None` for control limits.
    pub limit_label: Option<String>,
    /// Plotted points in sample order (undefined values are omitted).
    pub points: Vec<ChartPoint>,
    /// Center line (`NaN` when the chart has none, as for T²).
    pub center: f64,
    /// Lower control limit (`NaN` when the chart has none).
    pub lower: f64,
    /// Upper control limit.
    pub upper: f64,
    /// Standard deviation of the plotted statistic, for drawing 1σ/2σ zone lines.
    pub zone_sigma: f64,
    /// Limits that change along the chart (by phase or subgroup size), in
    /// order; empty when `center`, `lower` and `upper` hold throughout.
    pub steps: Vec<LimitStep>,
    /// Phase boundaries with their names (empty for a single phase).
    pub phases: Vec<PhaseMark>,
}

impl ControlChartSpec {
    /// Legend text for the limit lines.
    #[must_use]
    pub fn limits_label(&self) -> &str {
        self.limit_label.as_deref().unwrap_or("Control limits")
    }

    /// The limit steps to draw: [`ControlChartSpec::steps`], or one step
    /// spanning every point.
    #[must_use]
    pub fn limit_steps(&self) -> Vec<LimitStep> {
        if !self.steps.is_empty() {
            return self.steps.clone();
        }
        let first = self.points.first().map_or(0.5, |p| p.sample as f64 - 0.5);
        let last = self.points.last().map_or(1.5, |p| p.sample as f64 + 0.5);
        vec![LimitStep {
            from: first,
            to: last,
            center: self.center,
            lower: self.lower,
            upper: self.upper,
            zone_sigma: self.zone_sigma,
        }]
    }
}

/// One bar: an interval on the horizontal axis whose height is the number of
/// dataset rows it counts.
#[derive(Debug, Clone, PartialEq)]
pub struct BarItem {
    /// Center of the bar.
    pub center: f64,
    /// Width of the bar.
    pub width: f64,
    /// Dataset rows counted by the bar.
    pub rows: Vec<usize>,
    /// Values counted that are not dataset rows (e.g. simulated runs); the
    /// bar's height is `rows.len() + unlinked`.
    pub unlinked: usize,
}

impl BarItem {
    /// Bar height (rows and unlinked values).
    #[must_use]
    pub fn count(&self) -> usize {
        self.rows.len() + self.unlinked
    }
}

/// Data for a bar chart: a histogram, or frequencies of categorical levels.
#[derive(Debug, Clone, PartialEq)]
pub struct BarChartSpec {
    /// Horizontal axis label.
    pub x_label: String,
    /// Vertical axis label.
    pub y_label: String,
    /// The bars.
    pub bars: Vec<BarItem>,
    /// Labelled ticks replacing the numeric axis (categorical levels).
    pub x_ticks: Option<Vec<(f64, String)>>,
    /// A curve drawn over the bars, in the same units (e.g. a fitted density
    /// scaled to counts), with its legend name.
    pub curve: Option<(String, Polyline)>,
    /// Labelled vertical reference lines (e.g. specification limits).
    pub references: Vec<(f64, String)>,
    /// Date-time display of the horizontal axis's column (`None` for numeric
    /// axes).
    pub x_time: Option<stat_data::TemporalDisplay>,
}

/// Data for a horizontal box plot of one variable (Tukey 1977): box from the
/// first to the third quartile, median line, whiskers to the most extreme values
/// within 1.5 IQR of the box, and the points beyond plotted individually.
#[derive(Debug, Clone, PartialEq)]
pub struct BoxPlotSpec {
    /// Date-time display of the axis when the variable is a date-time column.
    pub x_time: Option<stat_data::TemporalDisplay>,
    /// Axis label (the variable).
    pub x_label: String,
    /// First quartile.
    pub q1: f64,
    /// Median.
    pub median: f64,
    /// Third quartile.
    pub q3: f64,
    /// End of the lower whisker.
    pub whisker_low: f64,
    /// End of the upper whisker.
    pub whisker_high: f64,
    /// Mean, drawn as a marker.
    pub mean: f64,
    /// Every value with its row (`x` = value, `y` = 0), for brushing.
    pub points: Vec<RowPoint>,
    /// Indices into `points` of the values beyond the whiskers.
    pub outliers: Vec<usize>,
}

/// What a section shows.
#[derive(Debug, Clone, PartialEq)]
pub enum SectionContent {
    /// A table.
    Table(TextTable),
    /// A scatter plot (boxed: plot data is much larger than the other variants).
    Plot(Box<PlotSpec>),
    /// A control chart.
    ControlChart(ControlChartSpec),
    /// A bar chart (histogram or categorical frequencies).
    Bars(BarChartSpec),
    /// A box plot of one variable.
    BoxPlot(BoxPlotSpec),
    /// A survival plot (boxed: large).
    Survival(Box<SurvivalPlotSpec>),
    /// A short note.
    Note(String),
    /// Formula text (see [`crate::formula`]), shown in a fixed-width font
    /// with its line breaks kept.
    Formula(String),
    /// Model effects ranked by significance (a Pareto chart of −log₁₀ p).
    EffectPareto(EffectPareto),
    /// A decision tree as nested nodes.
    Tree(TreeDiagram),
    /// A Pareto chart of categories with its cumulative-percentage line.
    Pareto(crate::pareto_chart::ParetoChartSpec),
    /// A cause-and-effect (fishbone) diagram.
    CauseEffect(crate::cause_effect::CauseEffectDiagram),
    /// A matrix of signed values as coloured cells with their values printed.
    HeatMap(crate::heat_map::HeatMapSpec),
    /// Frequent terms with type sized by their counts.
    WordCloud(crate::word_cloud::WordCloudSpec),
    /// A dense grid of values over time and frequency (a spectrogram or
    /// scalogram) as coloured cells.
    IntensityMap(crate::intensity_map::IntensityMapSpec),
    /// A dense grid of values (an image, a 2-D spectrum) drawn as one
    /// picture with a colour bar (boxed: it holds every cell).
    Raster(Box<stat_viz::RasterSpec>),
}

/// A decision tree for display: nodes with their rule, counts and rows.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeDiagram {
    /// Nodes; node 0 is the root.
    pub nodes: Vec<TreeDiagramNode>,
    /// Class labels, for the class shares of a classification tree (empty
    /// for a regression tree).
    pub classes: Vec<String>,
}

impl TreeDiagram {
    /// Where each node is drawn when the nodes for which `collapsed` is true
    /// hide their branches: `places[i] = Some([x, depth])` with `x` the
    /// node's centre in leaf slots (each shown leaf, or collapsed node, takes
    /// one slot of width 1, the first centred at 0.5; a split node is centred
    /// over its children), `None` for a hidden node. Also returns the number
    /// of slots and the deepest depth shown.
    #[must_use]
    pub fn layout(&self, collapsed: &dyn Fn(usize) -> bool) -> (Vec<Option<[f64; 2]>>, usize, usize) {
        fn place(
            d: &TreeDiagram,
            i: usize,
            depth: usize,
            collapsed: &dyn Fn(usize) -> bool,
            next: &mut usize,
            deepest: &mut usize,
            out: &mut [Option<[f64; 2]>],
        ) -> f64 {
            *deepest = (*deepest).max(depth);
            let node = &d.nodes[i];
            let x = if node.children.is_empty() || collapsed(i) {
                *next += 1;
                *next as f64 - 0.5
            } else {
                let xs: Vec<f64> =
                    node.children.iter().map(|&c| place(d, c, depth + 1, collapsed, next, deepest, out)).collect();
                xs.iter().sum::<f64>() / xs.len() as f64
            };
            out[i] = Some([x, depth as f64]);
            x
        }
        let mut out = vec![None; self.nodes.len()];
        let (mut next, mut deepest) = (0, 0);
        if !self.nodes.is_empty() {
            place(self, 0, 0, collapsed, &mut next, &mut deepest, &mut out);
        }
        (out, next, deepest)
    }
}

/// One node of a [`TreeDiagram`].
#[derive(Debug, Clone, PartialEq)]
pub struct TreeDiagramNode {
    /// The rule that leads to the node from its parent (`"All rows"` at the
    /// root), e.g. `"Tenure < 12.5"`.
    pub rule: String,
    /// Count and prediction, e.g. `"n = 40 · mean 12.3"`.
    pub summary: String,
    /// Training rows in the node (table rows), for brushing.
    pub rows: Vec<usize>,
    /// Children (two for a split node, none for a leaf).
    pub children: Vec<usize>,
    /// Class shares in the node (classification), in class order.
    pub shares: Vec<f64>,
    /// Leaf number (from 1, left to right) for a leaf.
    pub leaf: Option<usize>,
}

/// One model effect and its test's p-value.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectSource {
    /// Effect name (e.g. `"Temperature"`, `"A*B"`).
    pub name: String,
    /// p-value of the effect's test (NaN when it could not be tested).
    pub p_value: f64,
}

impl EffectSource {
    /// `−log₁₀ p`: 1.3 at p = 0.05, 2 at p = 0.01 (NaN untested, capped at 300).
    #[must_use]
    pub fn neg_log10_p(&self) -> f64 {
        if self.p_value.is_nan() { f64::NAN } else { -self.p_value.max(1e-300).log10() }
    }

    /// Conventional significance stars: `***` p < 0.001, `**` < 0.01, `*` < 0.05,
    /// `.` < 0.1, otherwise empty.
    #[must_use]
    pub fn stars(&self) -> &'static str {
        match self.p_value {
            p if p < 0.001 => "***",
            p if p < 0.01 => "**",
            p if p < 0.05 => "*",
            p if p < 0.1 => ".",
            _ => "",
        }
    }
}

/// Model effects sorted from most to least significant, for a Pareto chart
/// of `−log₁₀ p` (Juran's Pareto ordering applied to significance; the
/// `−log₁₀ p` scale is the one of genome-wide association "Manhattan"
/// plots) with the p-values alongside.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectPareto {
    /// Effects, smallest p-value first (untested ones last).
    pub effects: Vec<EffectSource>,
    /// The test behind the p-values (e.g. `"partial F tests"`).
    pub test: String,
}

impl EffectPareto {
    /// A summary of `effects` (in any order) tested by `test`.
    #[must_use]
    pub fn new(effects: impl IntoIterator<Item = (String, f64)>, test: impl Into<String>) -> Self {
        let mut effects: Vec<EffectSource> =
            effects.into_iter().map(|(name, p_value)| EffectSource { name, p_value }).collect();
        // Stable: ties keep model order; NaN last.
        effects.sort_by(|a, b| match (a.p_value.is_nan(), b.p_value.is_nan()) {
            (false, false) => a.p_value.total_cmp(&b.p_value),
            (x, y) => x.cmp(&y),
        });
        Self { effects, test: test.into() }
    }

    /// The summary as a report section (`None` without effects).
    #[must_use]
    pub fn section(self) -> Option<ReportSection> {
        (!self.effects.is_empty()).then(|| ReportSection {
            kind: SectionKind::EffectPareto,
            title: format!("Effect Pareto: −log₁₀ p ({})", self.test),
            content: SectionContent::EffectPareto(self),
        })
    }

    /// The summary as a table (Effect, −log₁₀ p, p-value, stars).
    #[must_use]
    pub fn table(&self) -> TextTable {
        let mut t = TextTable::new(["Effect", "−log₁₀ p", "p-value", "Significance"]);
        for e in &self.effects {
            t.push([
                e.name.clone(),
                crate::format::format_number(e.neg_log10_p(), 4),
                crate::format::format_p_value(e.p_value),
                e.stars().to_owned(),
            ]);
        }
        t
    }
}

/// Stable identifier of a section within a report, used for view state such as
/// which sections are open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SectionKind {
    /// Fit statistics of several responses' models.
    ResponseFits,
    /// Several responses: the terms each response's stepwise search selected.
    TermSelection,
    /// Several responses: the step history of response `i`'s search.
    ResponseStepHistory(usize),
    /// Factor settings of a profiler.
    CurrentSettings,
    /// Desirability of each response and overall.
    DesirabilityTable,
    /// Settings found by maximizing the overall desirability.
    BestSettings,
    /// Simulated defect rates.
    SimulationSummary,
    /// Probability of meeting every specification at the corners of a box
    /// of factor settings.
    SpecRegion,
    /// Specification-limit contours with the box of settings drawn on them.
    SpecRegionContours,
    /// Fit measures of a predictive model by validation role.
    FitByRole,
    /// Confusion matrix of a classifier.
    ConfusionMatrix,
    /// Settings and size of a decision tree.
    TreeSummary,
    /// Held-out error along a tree's pruning sequence (plot).
    PruningPlot,
    /// A tree's pruning sequence (table).
    PruningSequence,
    /// A tree drawn as collapsible nodes.
    TreeDiagram,
    /// Importance of each predictor.
    VariableImportance,
    /// A tree's leaves.
    TreeLeaves,
    /// A random forest's settings and out-of-bag error.
    ForestSummary,
    /// Out-of-bag error as trees are added.
    OobError,
    /// A boosted-tree model's settings and rounds kept.
    BoostSummary,
    /// Training and held-out loss by boosting round.
    BoostLoss,
    /// A PLS model's settings and factors used.
    PlsSummary,
    /// Variance explained and held-out error by number of PLS factors.
    PlsFactors,
    /// Held-out error against the number of PLS factors.
    PlsPress,
    /// PLS coefficients in original units.
    PlsCoefficients,
    /// PLS weights and loadings.
    PlsLoadings,
    /// A support vector machine's kernel, cost and support vectors.
    SvmSummary,
    /// Held-out error over the SVM tuning grid.
    SvmTuning,
    /// Held-out error against the cost, one line per γ.
    SvmTuningPlot,
    /// Weights of a linear SVM.
    SvmWeights,
    /// A MARS model's size, fit and settings.
    MarsSummary,
    /// Functional data: curves, basis, smoothing, registration, components.
    FunctionalSummary,
    /// Functional data: observations and smoothed curves.
    FunctionalCurves,
    /// Functional data: GCV against the smoothing parameter.
    FunctionalGcv,
    /// Functional data: landmark times of each curve.
    FunctionalLandmarks,
    /// Functional data: registered curves.
    FunctionalRegistered,
    /// Functional data: warping functions.
    FunctionalWarps,
    /// Functional data: mean curve with a ±2 SD band.
    FunctionalMean,
    /// Functional data: eigenvalues of the principal components.
    FunctionalEigenvalues,
    /// Functional data: eigenfunctions.
    FunctionalEigenfunctions,
    /// Functional data: effect of one component on the mean curve.
    FunctionalComponent(usize),
    /// Functional data: plot of the component scores.
    FunctionalScorePlot,
    /// Functional data: table of the component scores.
    FunctionalScores,
    /// Functional data: curves reconstructed from the retained components.
    FunctionalReconstruction,
    /// Functional data: fit statistics of each curve.
    FunctionalCurveFit,
    /// Functional data: tests of the score model's effects.
    FunctionalScoreModel,
    /// Functional data: fit of each modelled score.
    FunctionalScoreFits,
    /// A MARS model's basis functions and coefficients.
    MarsBasis,
    /// GCV (and held-out error) against the number of MARS terms.
    MarsSelection,
    /// The steps of the MARS forward pass.
    MarsForward,
    /// A neural network's structure, penalty, solver and starts.
    NeuralSummary,
    /// A neural network's layers and connections.
    NeuralDiagram,
    /// Held-out error of each neural network penalty.
    NeuralTuning,
    /// Held-out error against the neural network penalty.
    NeuralTuningPlot,
    /// One layer of neural network weights.
    NeuralWeights,
    /// A Gaussian process's kernel, hyperparameters and likelihood.
    GaussianProcessSummary,
    /// A Gaussian process's length scale per input.
    GaussianProcessLengthScales,
    /// Actual response against held-out Gaussian-process predictions.
    GaussianProcessActualByPredicted,
    /// Contour plot of a Gaussian process's prediction or its standard error.
    GaussianProcessContours,
    /// Responses, goals, factors and settings of a sequential optimization.
    SequentialSummary,
    /// The Gaussian-process model of each response in a sequential optimization.
    SequentialModels,
    /// The best run of a sequential optimization so far.
    SequentialBestRun,
    /// The next runs a sequential optimization proposes.
    SequentialProposals,
    /// Best value so far by round or run.
    SequentialProgress,
    /// Whether a sequential optimization's stop rule is met.
    SequentialStopRule,
    /// A model search's roles, measure and best model.
    ModelSearchSummary,
    /// Models of a search ranked by held-out error.
    ModelSearchRanking,
    /// Held-out error of each model of a search.
    ModelSearchPlot,
    /// Fit measures of several models.
    ModelComparison,
    /// ROC curves of several models.
    RocCurves,
    /// Lift by depth of several models.
    LiftChart,
    /// Cumulative gains of several models.
    GainsChart,
    /// Predicted probability against observed rate.
    CalibrationPlot,
    /// Observed response against each model's prediction.
    ActualByPredicted,
    /// Residuals against each model's prediction.
    ResidualByPredicted,
    /// Histogram of one response's simulated values (by response position).
    SimulatedResponse(usize),
    /// A model's prediction equation as a formula.
    PredictionFormula,
    /// A note that the model breaks the hierarchy principle.
    HierarchyNote,
    /// Effects ranked by p-value (Pareto chart of −log₁₀ p).
    EffectPareto,
    /// R², RMSE, information criteria, row counts.
    FitStatistics,
    /// Regression ANOVA table.
    AnalysisOfVariance,
    /// Coefficient table.
    ParameterEstimates,
    /// Scatter of the data with the fitted line (one predictor).
    FittedLine,
    /// Observed against predicted response (several predictors).
    ObservedVsPredicted,
    /// Residuals against predicted values.
    ResidualsVsPredicted,
    /// Center lines, control limits and the process sigma estimate.
    ControlLimits,
    /// Chart of subgroup means or individual values.
    LocationChart,
    /// Chart of ranges, standard deviations or moving ranges.
    DispersionChart,
    /// Within-subgroup range or standard deviation chart of a three-way chart.
    WithinChart,
    /// Mean and σ of each part number on a short-run chart.
    PartEstimates,
    /// Points flagged by the run rules.
    RuleViolations,
    /// Gauge study variance components and summary percentages.
    VarianceComponents,
    /// Gauge study ANOVA table.
    GaugeAnova,
    /// Gauge study design and number of distinct categories.
    GaugeSummary,
    /// Measurements plotted by part.
    MeasurementsByPart,
    /// Measurements plotted by operator.
    MeasurementsByOperator,
    /// Gauge R&R run chart: each part's measurements in run order, by operator.
    GaugeRunChartByPart,
    /// Expanded gauge study: variance components with standard errors and intervals.
    GaugeComponentIntervals,
    /// Expanded gauge study: tests of the fixed factors.
    GaugeFixedEffects,
    /// Moments and related statistics of one variable.
    SummaryStatistics,
    /// Censored values: counts of exact and censored values, detection limits.
    CensoredSummary,
    /// Censored values: nonparametric (Kaplan–Meier / Turnbull) percentiles and mean.
    CensoredNonparametric,
    /// Censored values: maximum-likelihood fits ranked by AIC.
    CensoredFits,
    /// Censored values: fitted parameters, median and mean with intervals.
    CensoredParameters,
    /// Censored values: distribution-function plot with fitted curves.
    CensoredPlot,
    /// Censored values: Poisson and negative binomial fits to counts.
    CensoredCountFits,
    /// Quantiles of one variable.
    Quantiles,
    /// Robust location and scale estimates.
    RobustEstimates,
    /// Histogram of a continuous variable.
    Histogram,
    /// Box plot of a continuous variable.
    BoxPlot,
    /// Frequency table of a categorical variable.
    Frequencies,
    /// Bar chart of a categorical variable's frequencies.
    FrequencyChart,
    /// Correlation coefficients of two continuous variables.
    Correlations,
    /// Scatter plot of two continuous variables.
    BivariateScatter,
    /// Two-way table of counts.
    ContingencyCounts,
    /// Tests of independence for a two-way table.
    IndependenceTests,
    /// Means and spreads of a continuous variable within each group.
    GroupMeans,
    /// One-way ANOVA comparing group means.
    OneWayAnova,
    /// A continuous variable plotted by group.
    ValuesByGroup,
    /// Partial F-test for each model term.
    EffectTests,
    /// Rows with high leverage, large studentized residuals or large Cook's distance.
    InfluenceDiagnostics,
    /// Cook's distance by row.
    CooksDistance,
    /// Normal quantile plot of studentized residuals.
    ResidualQuantiles,
    /// Normal distribution fitted to one variable.
    NormalFit,
    /// Shapiro–Wilk and Anderson–Darling tests.
    NormalityTests,
    /// Normal quantile plot of one variable.
    NormalQuantilePlot,
    /// Pooled and Welch two-sample t-tests.
    MeanDifference,
    /// Welch's test of equal means without equal variances.
    WelchAnova,
    /// Levene, Brown–Forsythe and Bartlett tests.
    EqualVariances,
    /// Wilcoxon rank-sum and Kruskal–Wallis tests.
    RankTests,
    /// Principal component variances.
    Eigenvalues,
    /// Eigenvalues by component number.
    ScreePlot,
    /// Eigenvectors and variable–component correlations.
    Loadings,
    /// Observations on the first two components.
    ScorePlot,
    /// Cluster sizes and spreads.
    ClusterSummary,
    /// K-means starting centers given by the user.
    ClusterStartingCenters,
    /// Variable means by cluster.
    ClusterMeans,
    /// Observations coloured by cluster.
    ClusterPlot,
    /// Hierarchical merge tree.
    Dendrogram,
    /// Number at risk (and censored) at round times, by group.
    NumberAtRisk,
    /// Gap statistic and average silhouette width for each number of clusters.
    ClusterNumber,
    /// Gap statistic against the number of clusters.
    GapPlot,
    /// Average silhouette width against the number of clusters.
    SilhouettePlot,
    /// GLM deviance, information criteria and whole-model test.
    ModelSummary,
    /// GLM coefficients with Wald tests and ratios.
    GlmEstimates,
    /// Likelihood-ratio test for each term.
    LikelihoodRatioTests,
    /// Observed against fitted values (or a fitted curve).
    FittedValues,
    /// Correlations between random effects of one grouping.
    RandomCorrelations,
    /// Predicted random effects for each level.
    ConditionalModes,
    /// Thresholds (cut points) of an ordinal model.
    Thresholds,
    /// Observed and predicted category proportions of an ordinal model.
    CategoryProbabilities,
    /// Likelihood-ratio test of equal residual variances across groups.
    ResidualVarianceTest,
    /// Penalty chosen by cross-validation.
    PenaltySummary,
    /// Coefficients at the chosen penalties.
    PenalizedCoefficients,
    /// Cross-validated error by penalty.
    CrossValidation,
    /// Coefficients along the penalty path.
    CoefficientPath,
    /// Correspondence analysis inertia by dimension.
    Inertia,
    /// Row category coordinates.
    RowCoordinates,
    /// Column category coordinates.
    ColumnCoordinates,
    /// Correspondence analysis map.
    CorrespondenceMap,
    /// Columns, counts, means and SDs of a correlation report.
    CorrelationSummary,
    /// Pearson correlation matrix.
    CorrelationMatrix,
    /// Heat map of the correlation matrix.
    CorrelationHeatMap,
    /// Every pair's Pearson r with interval and p-value.
    PairwiseCorrelations,
    /// Every pair's Spearman ρ with interval and p-value.
    SpearmanCorrelations,
    /// Partial correlations given the other columns.
    PartialCorrelations,
    /// Covariance matrix.
    CovarianceMatrix,
    /// Settings and reference limits of outlier distances.
    OutlierSummary,
    /// A distance by row number with its limit.
    OutlierPlot,
    /// Rows above the limit.
    OutlierRows,
    /// Mean vector and covariance matrix behind the distances.
    OutlierEstimates,
    /// Responses, effects and rows of a MANOVA.
    ManovaSummary,
    /// Wilks, Pillai, Hotelling–Lawley and Roy tests of each effect.
    ManovaTests,
    /// Univariate F-test of each response for each effect.
    ManovaUnivariate,
    /// Contrast tests and estimates.
    ManovaContrasts,
    /// Eigenvalues, canonical correlations and coefficients of each effect.
    ManovaCanonical,
    /// Rows' canonical scores for a categorical effect.
    ManovaCanonicalPlot,
    /// Error SDs and partial correlations of the responses.
    ManovaResiduals,
    /// Box's M test of equal covariance matrices.
    ManovaBoxM,
    /// Cronbach's alpha with its interval.
    ReliabilitySummary,
    /// Per-item statistics of an item analysis.
    ItemStatistics,
    /// Heat map of the inter-item correlations.
    InterItemCorrelations,
    /// Settings, rows and Heywood cases of a factor analysis.
    FactorSummary,
    /// Eigenvalues of the analysed correlation or covariance matrix.
    FactorEigenvalues,
    /// Scree plot of those eigenvalues.
    FactorScree,
    /// Maximum-likelihood chi-square test that the factors suffice.
    FactorFitTest,
    /// Unrotated factor loadings.
    FactorLoadings,
    /// Rotated (pattern) factor loadings.
    FactorRotated,
    /// Structure loadings of an oblique rotation.
    FactorStructure,
    /// Correlations of oblique factors.
    FactorCorrelations,
    /// Communalities and uniquenesses.
    FactorCommunalities,
    /// Variance accounted for by each factor.
    FactorVariance,
    /// Variables' loadings on factors 1 and 2.
    FactorLoadingPlot,
    /// Settings and rows of a discriminant analysis.
    DiscriminantSummary,
    /// Class means and priors.
    DiscriminantMeans,
    /// Rows assigned another class than their own.
    DiscriminantMisclassified,
    /// Eigenvalues, canonical correlations and Wilks' Λ tests.
    DiscriminantCanonical,
    /// Canonical scores of the rows with class means.
    DiscriminantPlot,
    /// Standardized and raw canonical coefficients.
    DiscriminantCoefficients,
    /// Structure correlations of the variables with the canonical functions.
    DiscriminantStructure,
    /// Chosen Gaussian mixture model and fit.
    GaussianMixtureSummary,
    /// BIC of every covariance structure and number of components.
    GaussianMixtureBic,
    /// BIC against the number of components, one line per structure.
    GaussianMixtureBicPlot,
    /// Mixing proportions and means of the components.
    GaussianMixtureComponents,
    /// Covariance matrices of the components.
    GaussianMixtureCovariances,
    /// Rows by most likely component with component ellipses.
    GaussianMixtureClusterPlot,
    /// Corpus and term settings of a text exploration.
    TextSummary,
    /// Counts and document frequencies of terms.
    TermFrequencies,
    /// Bars of the most frequent terms.
    WordFrequencyChart,
    /// Word cloud of the most frequent terms.
    WordCloud,
    /// Frequent phrases (n-grams).
    Phrases,
    /// Singular values and shares of the semantic dimensions.
    SemanticDimensions,
    /// Highest and lowest term loadings per semantic dimension.
    SemanticTerms,
    /// Documents on the first two semantic dimensions.
    DocumentMap,
    /// Sizes and characteristic terms of document clusters.
    DocumentClusters,
    /// Corpus, priors and fit of a topic model.
    TopicSummary,
    /// Top terms of each topic.
    TopicTerms,
    /// Documents by most likely topic.
    TopicShares,
    /// Log-likelihood of the topic sampler by sweep.
    TopicTrace,
    /// Most representative documents of each topic.
    TopicDocuments,
    /// Corpus, lexicon and score summary of a sentiment analysis.
    SentimentSummary,
    /// Histogram of sentiment scores.
    SentimentDistribution,
    /// Lexicon words that drive the scores.
    SentimentWords,
    /// Documents with the highest scores.
    MostPositive,
    /// Documents with the lowest scores.
    MostNegative,
    /// Chosen latent class model and its fit.
    LatentClassSummary,
    /// Fit and information criteria of every number of classes.
    LatentClassSelection,
    /// Information criteria against the number of classes.
    LatentClassCriteriaPlot,
    /// Class sizes with standard errors.
    LatentClassSizes,
    /// Item-response probabilities by class with standard errors.
    LatentClassProbabilities,
    /// Response probabilities by item, level and class.
    LatentClassProfilePlot,
    /// Average posterior probabilities by most likely class.
    LatentClassClassification,
    /// Method, settings and fit of an embedding.
    EmbeddingSummary,
    /// Eigenvalues of classical scaling.
    EmbeddingEigenvalues,
    /// The map of the rows.
    EmbeddingMap,
    /// Dissimilarities against map distances.
    ShepardDiagram,
    /// Events, censoring and median survival per group.
    SurvivalSummary,
    /// Log-rank comparison of groups.
    LogRank,
    /// Survival curves with confidence bands and fitted distributions.
    SurvivalCurves,
    /// Weibull maximum-likelihood estimates.
    WeibullFit,
    /// Lognormal maximum-likelihood estimates.
    LognormalFit,
    /// Log-likelihood and information criteria of the fitted distributions.
    FitComparison,
    /// The product-limit table, one row per distinct time.
    ProductLimitTable,
    /// Power-law (Crow–AMSAA) estimates of a growth test, per phase.
    GrowthEstimates,
    /// Cumulative failures against time with the fitted mean.
    GrowthCumulative,
    /// Failure intensity against time.
    GrowthIntensity,
    /// Instantaneous MTBF against time.
    GrowthMtbf,
    /// Cramér–von Mises goodness of fit of the growth model.
    GrowthGoodnessOfFit,
    /// Systems, repairs and observation of a recurrent-events analysis.
    RecurrenceSummary,
    /// Mean cumulative function plot.
    McfPlot,
    /// Mean cumulative function estimates with limits.
    McfTable,
    /// Difference of two populations' mean cumulative functions (plot).
    McfDifferencePlot,
    /// Difference of two mean cumulative functions (table).
    McfDifferenceTable,
    /// Constant-rate and power-law process fits.
    RecurrenceFits,
    /// Laplace and military handbook trend tests.
    TrendTests,
    /// Units and kinds of censoring of a life-distribution analysis.
    LifeDataSummary,
    /// Log-likelihood, AIC, BIC and Anderson–Darling of each fitted life distribution.
    LifeModelComparison,
    /// Likelihood-ratio tests of nested life distributions.
    LifeNestedTests,
    /// Probability plot of the nonparametric estimate with the fitted line and limits.
    LifeProbabilityPlot,
    /// Parameter estimates with Wald and likelihood-ratio intervals.
    LifeParameters,
    /// Percentiles (times by which given fractions fail) with intervals.
    LifePercentiles,
    /// Failure probabilities at given times with intervals.
    LifeProbabilities,
    /// Nonparametric and fitted survival curves.
    LifeSurvivalPlot,
    /// Fitted hazard function with limits.
    LifeHazardPlot,
    /// Turnbull's nonparametric estimate.
    LifeNonparametric,
    /// Weibull with a known shape.
    LifeKnownShape,
    /// Units, failures per cause and censoring of a competing-risks analysis.
    CompetingRisksSummary,
    /// Cumulative incidence curves per cause (stacked or overlaid).
    CumulativeIncidencePlot,
    /// Aalen–Johansen estimates with limits.
    CumulativeIncidenceTable,
    /// Gray's K-sample tests of each cause's cumulative incidence.
    GrayTests,
    /// Fine–Gray subdistribution hazard ratios.
    SubdistributionHazards,
    /// Fit statistics of a Fine–Gray model.
    SubdistributionFit,
    /// Predicted cumulative incidence at chosen covariate values.
    PredictedIncidencePlot,
    /// A life distribution fitted to each failure mode.
    FailureModeFits,
    /// Each failure mode's and the series system's failure probability.
    SeriesSystemPlot,
    /// The series system's failure probabilities with limits.
    SeriesSystemProbabilities,
    /// Shipments, returns and units in service per shipment period.
    WarrantySummary,
    /// The censored life data built from the warranty counts.
    WarrantyLifeData,
    /// Observed and forecast returns per calendar period.
    WarrantyForecastPlot,
    /// Forecast returns per future period with prediction limits.
    WarrantyForecastTable,
    /// Cumulative forecast returns with prediction limits.
    WarrantyCumulativeForecast,
    /// Life–stress model: data and model summary.
    LifeStressSummary,
    /// Life–stress model: fit statistics.
    LifeStressFitStatistics,
    /// Life–stress model: parameter estimates with Wald and likelihood-ratio limits.
    LifeStressParameters,
    /// Life–stress model: activation energy, power exponent or other derived values.
    LifeStressDerived,
    /// Life–stress model: acceleration factors relative to use conditions.
    LifeStressAcceleration,
    /// Life–stress model: percentiles of life at use conditions.
    LifeStressUsePercentiles,
    /// Life–stress model: failure probabilities at use conditions.
    LifeStressUseProbabilities,
    /// Life–stress model: life against stress plot.
    LifeStressPlot,
    /// Life–stress model: probability plot by stress level with fitted lines.
    LifeStressProbabilityPlot,
    /// Life–stress model: likelihood-ratio checks (constant scale, relationship).
    LifeStressModelChecks,
    /// Life–stress model: separate fits at each stress level.
    LifeStressLevelFits,
    /// Principal coordinates: distance, objects and correction.
    PcoaSummary,
    /// Principal coordinates: eigenvalues with relative and broken-stick shares.
    PcoaEigenvalues,
    /// Principal coordinates: ordination plot of the first two axes.
    PcoaPlot,
    /// Principal coordinates: correlations of variables with the axes.
    PcoaCorrelations,
    /// Pharmacokinetics: dose, area method and terminal-phase rule.
    PkSettings,
    /// Pharmacokinetics: parameters per subject profile.
    PkParameters,
    /// Pharmacokinetics: summary by treatment (geometric means and CVs).
    PkSummary,
    /// Pharmacokinetics: terminal-phase regressions.
    PkTerminalPhase,
    /// Pharmacokinetics: concentration–time profiles.
    PkProfilePlot,
    /// Pharmacokinetics: semi-logarithmic profiles with terminal lines.
    PkSemiLogPlot,
    /// Fatigue: failures and run-outs at each stress level.
    FatigueLevels,
    /// Fatigue: log-likelihood, AIC and endurance limit.
    FatigueFitStatistics,
    /// Fatigue: S–N curve coefficients and scale with intervals.
    FatigueParameters,
    /// Fatigue: fatigue strength at chosen lives.
    FatigueStrength,
    /// Fatigue: life quantiles at chosen stresses.
    FatigueLives,
    /// Fatigue: S–N plot with quantile curves and bounds.
    FatigueSnPlot,
    /// Fatigue: standardized residuals against stress.
    FatigueResiduals,
    /// Fatigue: probability plot of standardized residuals across stress levels.
    FatigueProbabilityPlot,
    /// Degradation: data summary.
    DegradationSummary,
    /// Degradation: measured paths with the failure threshold.
    DegradationPathPlot,
    /// Degradation: per-unit path fits and pseudo failure times.
    DegradationUnitPaths,
    /// Degradation: life distribution or regression parameters.
    DegradationLifeParameters,
    /// Degradation: probability plot of pseudo failure times.
    DegradationProbabilityPlot,
    /// Degradation: percentiles of the time to reach the threshold.
    DegradationPercentiles,
    /// Degradation: probabilities of reaching the threshold by given times.
    DegradationProbabilities,
    /// Degradation: random-effects path model estimates.
    DegradationRandomEffects,
    /// Degradation: time-to-threshold distribution curve.
    DegradationLifeCurve,
    /// Likelihood-ratio test of each effect of a proportional hazards model.
    HazardEffectTests,
    /// Coefficients and hazard ratios of a proportional hazards model.
    HazardEstimates,
    /// Grambsch–Therneau tests of proportional hazards.
    HazardProportionality,
    /// Scaled Schoenfeld residuals against time.
    HazardSchoenfeldPlot,
    /// Predicted survival curves of a proportional hazards model.
    HazardSurvivalPlot,
    /// Predicted survival at chosen times.
    HazardSurvivalTable,
    /// Deviance residuals of a proportional hazards model.
    HazardResiduals,
    /// Breslow baseline survival.
    HazardBaseline,
    /// AIC comparison of accelerated failure time distributions.
    SurvivalRegressionComparison,
    /// Likelihood-ratio test of each effect of a survival regression.
    SurvivalRegressionEffectTests,
    /// Parameter estimates of a survival regression.
    SurvivalRegressionEstimates,
    /// Time ratios (acceleration factors).
    SurvivalRegressionTimeRatios,
    /// Predicted quantiles at chosen settings.
    SurvivalRegressionPredictions,
    /// Predicted failure probabilities at chosen settings.
    SurvivalRegressionProbabilities,
    /// Fitted survival curves with nonparametric estimates.
    SurvivalRegressionSurvivalPlot,
    /// Cox–Snell residual check.
    SurvivalRegressionResiduals,
    /// Inputs of a reliability test plan.
    TestPlanInputs,
    /// The planned units, test time or levels.
    TestPlanResult,
    /// Alternatives: plans for other failure counts, precisions or plan types.
    TestPlanAlternatives,
    /// Operating characteristic or sample-size curve of a test plan.
    TestPlanCurve,
    /// Sample sizes, means and spreads of the samples compared.
    ComparisonSummary,
    /// One-sample, two-sample or paired tests.
    ComparisonTests,
    /// Two one-sided tests of equivalence.
    EquivalenceTests,
    /// Standardized effect sizes or measures of association.
    EffectSizes,
    /// Comparisons of every pair of groups.
    PairwiseComparisons,
    /// Effect sizes of ANOVA effects with noncentral-F intervals.
    AnovaEffectSizes,
    /// Linear-trend contrast across ordered groups.
    LinearTrend,
    /// p-values adjusted for multiple testing.
    PValueAdjustments,
    /// Methods, error rates and counts of rejections of a p-value adjustment.
    PValueSummary,
    /// Combined tests (Fisher, Stouffer) of several p-values.
    CombinedPValues,
    /// Ranked raw and adjusted p-values against their rank.
    PValuePlot,
    /// One test per variable between two groups.
    ManyTestsResults,
    /// Treatment, method, rows used and balance verdict of a treatment-effect analysis.
    TreatmentSummary,
    /// Treatment effect estimates (difference, ratios) with intervals.
    TreatmentEffect,
    /// Estimated potential-outcome means per treatment group.
    TreatmentMeans,
    /// Standardized differences and variance ratios before and after adjustment.
    CovariateBalance,
    /// |Standardized difference| per covariate before and after adjustment.
    BalancePlot,
    /// Propensity scores by treatment group.
    PropensityOverlap,
    /// Logistic regression of the treatment on the covariates.
    PropensityModel,
    /// Outcome regressions of a doubly robust estimate.
    OutcomeModels,
    /// Treatment effect within each subgroup, with a test of equal effects.
    SubgroupEffects,
    /// Subgroup effects with intervals (forest plot).
    SubgroupForest,
    /// Difference against −log₁₀ p, one point per variable.
    VolcanoPlot,
    /// Area under a curve, by curve.
    AreaUnderCurve,
    /// Regions of a curve above its baseline.
    AreaPeaks,
    /// A curve of means with its observations and baseline.
    AreaPlot,
    /// Paired differences against pair means.
    PairedDifferences,
    /// Within-subject tests of several conditions.
    RepeatedMeasures,
    /// Factors of a factorial repeated-measures design.
    RepeatedDesign,
    /// Cell means of a factorial repeated-measures design.
    RepeatedCellMeans,
    /// Univariate tests of a factorial repeated-measures design.
    RepeatedTests,
    /// Mauchly's tests and ε estimates per within-subject effect.
    Sphericity,
    /// Tests of one factor at each level of another.
    SimpleEffects,
    /// Cell means across a within-subject factor.
    RepeatedProfile,
    /// Groups, subjects and model-based means of a nested comparison.
    NestedSummary,
    /// Tests of a nested comparison (groups, pairs of groups).
    NestedTests,
    /// Classical nested analysis of variance.
    NestedAnova,
    /// Measurements by group and subject.
    NestedPlot,
    /// Event counts and proportions.
    ProportionSummary,
    /// Tests of proportions.
    ProportionTests,
    /// Proportions with confidence intervals.
    ProportionPlot,
    /// Event counts, exposure and rates.
    RateSummary,
    /// Tests of rates.
    RateTests,
    /// Rates with confidence intervals.
    RatePlot,
    /// Chi-square tests of a count distribution.
    GoodnessOfFitTests,
    /// Observed and expected counts per category.
    GoodnessOfFitCells,
    /// Observed against expected counts.
    GoodnessOfFitPlot,
    /// Item counts with percent of cases and of responses.
    MultipleResponseFrequencies,
    /// Respondents, responses and mean responses per respondent.
    MultipleResponseSummary,
    /// Bar chart of item percentages of cases.
    MultipleResponseChart,
    /// Item counts and percentages by group.
    MultipleResponseCrossTab,
    /// Per-item chi-square tests with Bonferroni adjustment.
    MultipleResponseTests,
    /// Subjects and means per sequence and period of a crossover.
    CrossoverSummary,
    /// Treatment, period and carryover estimates of a crossover.
    CrossoverEffects,
    /// Analysis of variance of a crossover.
    CrossoverAnova,
    /// Equivalence of the treatments in a crossover.
    CrossoverEquivalence,
    /// Subject profiles across the periods of a crossover.
    CrossoverPlot,
    /// The distribution, its parameters and moments.
    CalculatorDistribution,
    /// Probabilities and quantiles from a distribution.
    CalculatorProbabilities,
    /// Density or mass with the region of interest marked.
    CalculatorPlot,
    /// Assumptions of a power calculation.
    PowerInputs,
    /// Sample size, power or detectable effect.
    PowerResult,
    /// Power against sample size.
    PowerCurve,
    /// Power against the size of the effect.
    EffectCurve,
    /// Fit statistics and factor coding of a response-surface model.
    SurfaceSummary,
    /// Coefficients of a response-surface model in coded units.
    SurfaceCoefficients,
    /// Stationary point of a response surface.
    StationaryPoint,
    /// Eigenvalues and eigenvectors of a response surface.
    CanonicalAnalysis,
    /// Path of greatest predicted response.
    RidgeMaximum,
    /// Path of least predicted response.
    RidgeMinimum,
    /// Contour plot of a response surface.
    SurfaceContours,
    /// Settings and summary of an EWMA or CUSUM chart.
    TimeWeightedSummary,
    /// The EWMA or CUSUM chart.
    TimeWeightedChart,
    /// Samples where an EWMA or CUSUM chart signals.
    TimeWeightedSignals,
    /// Box–Cox estimate of the best power transformation.
    BoxCox,
    /// Box–Cox profile log-likelihood.
    BoxCoxProfile,
    /// Capability indices with confidence intervals.
    CapabilityIndices,
    /// Limits, mean and standard deviations of a capability study.
    CapabilitySummary,
    /// Observed and expected nonconforming rates.
    CapabilityNonconforming,
    /// Histogram with specification limits.
    CapabilityHistogram,
    /// The distribution a non-normal capability study fitted, with its
    /// reference percentiles (and the families compared by AIC).
    CapabilityDistribution,
    /// Normal-theory indices shown beside a non-normal study for comparison.
    CapabilityNormalComparison,
    /// Location chart of a capability summary.
    CapabilityLocationChart,
    /// Dispersion chart of a capability summary.
    CapabilityDispersionChart,
    /// The last subgroups' individual values in a capability summary.
    CapabilityLastSubgroups,
    /// Normal (or fitted-model) probability plot in a capability summary.
    CapabilityProbabilityPlot,
    /// Run chart of a control-chart report: every value with the subgroup
    /// means or medians joined.
    RunChart,
    /// Capability indices of each group of a by column.
    CapabilityByGroup,
    /// Ppk of each group of a by column.
    CapabilityByGroupChart,
    /// Summary of an attribute (p, np, c, u) chart.
    AttributeSummary,
    /// The attribute chart.
    AttributeChart,
    /// Subgroups outside an attribute chart's limits.
    AttributeSignals,
    /// Settings and outcome of a stepwise search.
    StepwiseSettings,
    /// Steps of a stepwise search.
    StepHistory,
    /// Fit summary of a mixture model.
    MixtureSummary,
    /// Scheffé coefficients.
    MixtureCoefficients,
    /// Tests of nonlinear blending.
    MixtureBlending,
    /// Ternary contour plot.
    MixtureContours,
    /// Tolerance interval of one column.
    ToleranceInterval,
    /// Outlier test result.
    OutlierTests,
    /// Steps of the generalized ESD outlier test.
    OutlierSteps,
    /// Runs tests for randomness.
    RunsTests,
    /// Mood's median test.
    MedianTest,
    /// Comparisons of each group with a control (Dunnett).
    ControlComparisons,
    /// Interval plot of the differences from the control.
    ControlComparisonPlot,
    /// Comparisons of each group with the best of the others (Hsu).
    BestComparisons,
    /// Interval plot of the differences from the best of the others.
    BestComparisonPlot,
    /// Settings of a best-subsets search.
    BestSubsetsSettings,
    /// Best models of each size.
    BestSubsetsModels,
    /// A selection criterion against model size.
    BestSubsetsPlot,
    /// Goodness of fit of each fitted distribution.
    DistributionFitSummary,
    /// Parameter estimates of each fitted distribution.
    DistributionFitParameters,
    /// Histogram with the best-fitting density.
    DistributionFitHistogram,
    /// Quantile plot of the data against a fitted distribution.
    ProbabilityPlot,
    /// Estimate and exact interval of an attribute capability study.
    AttributeCapabilitySummary,
    /// Cumulative fraction defective or defects per unit over the samples.
    AttributeCapabilityCumulative,
    /// Rate of each sample against its size.
    AttributeCapabilityBySize,
    /// Counts and rates of each sample.
    AttributeCapabilitySamples,
    /// Within, between/within and overall capability indices.
    BetweenWithinIndices,
    /// Between- and within-subgroup variance components.
    BetweenWithinComponents,
    /// Limits, mean and σ estimators of a between/within study.
    BetweenWithinSummary,
    /// Values by subgroup with the subgroup means.
    BetweenWithinPlot,
    /// Risk points and lot of an acceptance sampling report.
    AcceptanceRequirements,
    /// The designed sampling plan.
    AcceptancePlan,
    /// Sampling plans side by side.
    AcceptanceComparison,
    /// Operating-characteristic curves.
    AcceptanceOc,
    /// Average-outgoing-quality curves.
    AcceptanceAoq,
    /// Average-total-inspection curves.
    AcceptanceAti,
    /// Average-sample-number curves.
    AcceptanceAsn,
    /// Pareto chart of categories.
    ParetoChart,
    /// Counts and cumulative percentages of a Pareto chart's categories.
    ParetoTable,
    /// Which categories make up most of the total.
    ParetoSummary,
    /// Cause-and-effect diagram.
    CauseEffectDiagram,
    /// Outline of a cause-and-effect diagram.
    CauseEffectOutline,
    /// Multi-vari chart of means and values.
    MultiVariChart,
    /// Spread of each factor's means in a multi-vari study.
    MultiVariVariation,
    /// Ranges within the cells of a multi-vari study.
    MultiVariRanges,
    /// Statistics of each cell of a multi-vari study.
    MultiVariCells,
    /// Settings and limits of a multivariate control chart.
    MultivariateSummary,
    /// The multivariate (T², |S| or MEWMA) chart.
    MultivariateChart,
    /// Points outside a multivariate chart's limits.
    MultivariateSignals,
    /// Each variable's contribution to the out-of-control points.
    MultivariateContributions,
    /// How contributions are computed and read.
    MultivariateContributionNote,
    /// Contributions plotted across the variables.
    MultivariateContributionPlot,
    /// Reference mean vector and covariance matrix.
    MultivariateEstimates,
    /// Settings, model and limits of a principal-components control chart.
    PcaChartSummary,
    /// Cross-validated PRESS by number of components.
    PcaChartCrossValidation,
    /// T² chart of the retained component scores.
    PcaChartT2,
    /// Squared prediction error chart.
    PcaChartSpe,
    /// Points above either limit.
    PcaChartSignals,
    /// Variable contributions to the T² signals.
    PcaChartT2Contributions,
    /// T² contributions plotted across the variables.
    PcaChartT2ContributionPlot,
    /// How T² contributions are computed.
    PcaChartT2ContributionNote,
    /// Variable contributions to the SPE signals.
    PcaChartSpeContributions,
    /// SPE contributions plotted across the variables.
    PcaChartSpeContributionPlot,
    /// How SPE contributions are computed.
    PcaChartSpeContributionNote,
    /// Signalling rows against the reference mean (parallel coordinates).
    PcaChartParallel,
    /// Change-point estimate of the mean.
    PcaChartChangePoint,
    /// How the change point is estimated.
    PcaChartChangePointNote,
    /// Likelihood-ratio statistic of every split.
    PcaChartChangePointPlot,
    /// Reference means, scales, loadings and eigenvalues.
    PcaChartLoadings,
    /// Settings and totals of a monitoring-limits study.
    MonitoringSummary,
    /// The fitted distribution of each limit group.
    MonitoringFits,
    /// Every candidate family's fit and AIC.
    MonitoringCandidates,
    /// Alert and action levels of each limit group.
    MonitoringLevels,
    /// Readings, excursions and status by location.
    MonitoringByLocation,
    /// Every reading that breaks a rule.
    MonitoringExcursions,
    /// The early-warning rules.
    MonitoringRules,
    /// One location's readings against its levels.
    MonitoringCharts,
    /// Settings and critical values of an analysis of means.
    AnomSummary,
    /// Analysis-of-means chart (first or only factor).
    AnomChart,
    /// Groups of the first or only factor with their decision limits.
    AnomGroups,
    /// Analysis-of-means chart of the second factor.
    AnomSecondChart,
    /// Groups of the second factor.
    AnomSecondGroups,
    /// Interaction effects chart of a two-way analysis of means.
    AnomInteractionChart,
    /// Interaction effects with their decision limits.
    AnomInteractionGroups,
    /// Caution about the normal approximation for counts.
    AnomNote,
    /// Bias and linearity summary.
    GaugeLinearitySummary,
    /// Whether the bias line's band contains zero.
    GaugeLinearityNote,
    /// Biases against the reference values with the fitted line and band.
    GaugeLinearityPlot,
    /// Coefficients of the bias line.
    GaugeLinearityLine,
    /// Bias at each reference value.
    GaugeBiasTable,
    /// Cg and Cgk of a single-gauge repeatability study.
    GaugeCapabilityIndices,
    /// Chosen summary statistics of a gauge repeatability study.
    GaugeSummaryStatistics,
    /// Hysteresis: mean difference between directions of approach.
    GaugeHysteresis,
    /// Drift: slope of readings over time or order.
    GaugeDrift,
    /// Bias test of a single-gauge repeatability study.
    GaugeBiasTest,
    /// Repeated measurements in order with the reference and its band.
    GaugeRunChart,
    /// Wheeler's evaluation: intraclass correlation, class, attenuation,
    /// probable error and effective increment.
    IccEvaluation,
    /// Intraclass correlation for each definition of measurement error.
    IccBySource,
    /// The six Shrout & Fleiss forms.
    IccForms,
    /// How the intraclass correlations are computed.
    IccNote,
    /// Signal-to-noise settings and best settings.
    SnSummary,
    /// Main effects of the factors on the signal-to-noise ratio.
    SnEffectsPlot,
    /// Factors ranked by the range of their level means.
    SnRanking,
    /// Mean ratio and response at each factor level.
    SnResponseTable,
    /// Main effects of the factors on the mean response.
    SnMeanEffectsPlot,
    /// Ratio of each run.
    SnRuns,
    /// Factor and response roles of a declared design.
    DeclaredRoles,
    /// Runs, replicates and centre runs of a declared design.
    DeclaredRuns,
    /// Correlations of the coded factor columns.
    DeclaredCorrelations,
    /// Orthogonality note of a declared design.
    DeclaredNote,
    /// Models a declared design can estimate.
    DeclaredModels,
    /// Whole plots (and subplots) of a declared split-plot design and the
    /// factors held fixed within them.
    DeclaredPlots,
    /// Runs, terms, error df and efficiencies of an evaluated design.
    EvaluationSummary,
    /// Relative standard error, variance inflation and power of each term.
    EvaluationPower,
    /// Relative prediction variance along each factor.
    EvaluationProfile,
    /// Fraction-of-design-space plot.
    EvaluationFds,
    /// Quantiles of the relative prediction variance over the region.
    EvaluationFdsQuantiles,
    /// Alias matrix of terms left out of the model.
    EvaluationAlias,
    /// Absolute correlations of model and alias terms.
    EvaluationCorrelations,
    /// The same for the second of two compared designs.
    EvaluationCorrelationsSecond,
    /// Two designs' criteria side by side with relative efficiencies.
    EvaluationComparison,
    /// How the design evaluation measures are defined.
    EvaluationNote,
    /// Settings and fit statistics of a dose–response analysis.
    DoseResponseSummary,
    /// Intercept and slope of the dose–response line.
    DoseResponseEstimates,
    /// Effective doses with fiducial limits.
    EffectiveDoses,
    /// Observed and expected responses per dose group.
    DoseGroups,
    /// Proportion responding against dose with the fitted curve.
    DoseResponsePlot,
    /// Data summary of a method comparison.
    MethodComparisonSummary,
    /// Deming regression estimates.
    DemingEstimates,
    /// Passing–Bablok regression estimates.
    PassingBablokEstimates,
    /// Passing–Bablok cumulative-sum linearity test.
    LinearityTest,
    /// Why a Passing–Bablok slope limit is undefined.
    PassingBablokNote,
    /// Scatter plot of the two methods with the fitted lines.
    MethodComparisonPlot,
    /// Differences between the methods against their mean.
    MethodDifferencePlot,
    /// Summary of a shelf-life analysis.
    ShelfLifeSummary,
    /// Batch poolability tests of a stability study.
    PoolabilityTests,
    /// Fitted line and shelf life of each batch.
    ShelfLifeEstimates,
    /// Stability data with fitted lines, confidence bound and limits.
    ShelfLifePlot,
    /// Notes on the chosen model and extrapolation.
    ShelfLifeNote,
    /// Odds ratios with confidence limits of an ordinal or nominal logistic
    /// regression.
    OddsRatios,
    /// Score test of the proportional-odds (parallel lines) assumption.
    ParallelLinesTest,
    /// Predicted category probabilities against a continuous predictor.
    ClassProbabilityPlot,
    /// Model, sums of squares and convergence of a nonlinear regression.
    NonlinearSummary,
    /// Convergence, bound and profile-interval warnings of a nonlinear regression.
    NonlinearWarnings,
    /// Parameter estimates with asymptotic and profile intervals.
    NonlinearEstimates,
    /// Correlation of the nonlinear parameter estimates.
    NonlinearCorrelation,
    /// Data with the fitted curve and its bands (or observed by predicted).
    NonlinearCurve,
    /// Residuals against predicted values of a nonlinear regression.
    NonlinearResiduals,
    /// Extra sum-of-squares F tests and AICc of alternative nonlinear models.
    NonlinearComparison,
    /// Rows a robust nonlinear fit identified as outliers.
    NonlinearOutliers,
    /// Contour plot of a fitted nonlinear surface.
    NonlinearSurface,
    /// Predictor values read off a fitted curve.
    NonlinearInverse,
    /// The assay template a nonlinear regression was made from: model,
    /// parameters' meanings and constants.
    TemplateModel,
    /// Quantities derived from an assay template's parameters.
    TemplateDerived,
    /// Fitted curves of an assay template (by the second column, or a
    /// standard curve on a log axis).
    TemplateCurves,
    /// Calibration standards read back off a standard curve.
    StandardBackCalculation,
    /// Acceptance of a standard curve and its quantitation range.
    StandardAcceptance,
    /// Precision profile of a standard curve.
    PrecisionProfile,
    /// Unknown samples read off a standard curve.
    SampleConcentrations,
    /// Reversible inhibition mechanisms compared by AICc.
    InhibitionMechanisms,
    /// Schild plot of an antagonist-shift experiment.
    SchildPlot,
    /// Relative potency of preparations from parallel curves.
    RelativePotency,
    /// Parts, appraisers, trials and categories of an attribute agreement study.
    AgreementSummary,
    /// Each appraiser's agreement with themselves over repeat trials.
    AgreementWithin,
    /// Each appraiser against the standard.
    AgreementVersusStandard,
    /// Agreement of all appraisers with each other.
    AgreementBetween,
    /// Cohen's kappa of every pair of appraisers.
    AgreementPairs,
    /// All appraisers against the standard.
    AgreementAllVersusStandard,
    /// Weighted kappas and Kendall's τ_b against the standard (ordered categories).
    AgreementOrdinal,
    /// Kappa of each category in every comparison.
    AgreementByCategory,
    /// Counts of ratings by standard category.
    AgreementConfusion,
    /// Percent agreement with intervals per appraiser.
    AgreementPlot,
    /// How the agreement statistics are computed.
    AgreementNote,
    /// Bias, repeatability and the bias test of a go/no-go gauge.
    AttributeGaugeSummary,
    /// Acceptances and adjusted probabilities at each reference value.
    AttributeGaugePoints,
    /// Probability of acceptance against the reference value with the fitted curve.
    AttributeGaugeCurve,
    /// How the attribute gauge study is computed.
    AttributeGaugeNote,
    /// Counts, percentages and statistics by nested row and column classifications.
    MultiwayTable,
    /// What a multi-way table holds.
    MultiwayNote,
    /// Row × column counts in each stratum.
    StratifiedCounts,
    /// Pearson and exact tests in each stratum and pooled.
    StratifiedTests,
    /// Generalized Cochran–Mantel–Haenszel statistics.
    CmhTests,
    /// Mantel–Haenszel common odds ratio with its interval.
    CommonOddsRatio,
    /// Breslow–Day tests of homogeneous odds ratios.
    OddsRatioHomogeneity,
    /// Notes on the stratified analysis.
    StratifiedNote,
    /// What each column of a 3-D scatter plot shows.
    Scatter3dColumns,
    /// Rows plotted in a 3-D scatter plot.
    Scatter3dNote,
    /// How a surface of data was made.
    DataSurfaceFit,
    /// Contour plot of a surface of data.
    DataSurfaceContours,
    /// Bootstrap estimates, bias, standard errors and intervals.
    BootstrapEstimates,
    /// Histogram of one statistic's bootstrap replicates.
    BootstrapDistribution,
    /// How the bootstrap was run.
    BootstrapNote,
    /// Permutation test result.
    PermutationTest,
    /// Histogram of the permutation null distribution.
    PermutationDistribution,
    /// How the permutation test was run.
    PermutationNote,
    /// Settings and fit of a factorial analysis.
    FactorialSummary,
    /// Effects of a factorial analysis with their tests or Lenth's method.
    FactorialEffects,
    /// Normal plot of factorial effects.
    FactorialNormalPlot,
    /// Half-normal plot of factorial effects.
    FactorialHalfNormalPlot,
    /// Main-effects plot of a factorial analysis.
    FactorialMainEffects,
    /// Interaction plot of two factors.
    FactorialInteraction,
    /// Cube (or square) plot of corner means or predictions.
    FactorialCube,
    /// Dispersion effects of a factorial analysis.
    FactorialDispersion,
    /// Normal plot of dispersion effects.
    FactorialDispersionPlot,
    /// Notes of a factorial analysis (off-target settings, method).
    FactorialNote,
    /// Missing values per column.
    MissingSummary,
    /// Distinct missing-data patterns.
    MissingPatterns,
    /// Map of the missing-data patterns.
    MissingPatternMap,
    /// Values filled in, or the fitted mean and covariance.
    ImputationEstimates,
    /// Iterations and convergence of an imputation.
    ImputationConvergence,
    /// Plot of an imputation's convergence.
    ImputationConvergencePlot,
    /// Held-out error by rank of a low-rank imputation.
    ImputationRank,
    /// The imputed values.
    ImputedValues,
    /// Scatter plot with imputed values marked.
    ImputedScatter,
    /// Notes on an imputation.
    ImputationNote,
    /// Warning that an iterative imputation did not converge.
    ImputationWarning,
    /// The imputation model of each column of a multiple imputation.
    ImputationModels,
    /// Means of the imputed columns pooled across imputations.
    ImputationPooledMeans,
    /// Mean or spread of the imputed values by iteration, one line per chain.
    ImputationTrace,
    /// Observed and imputed values by imputation number.
    ImputationStrip,
    /// Densities of the observed and imputed values.
    ImputationDensity,
    /// Shares of each category among observed and imputed values.
    ImputationCategories,
    /// How a report was pooled across imputations, and what follows it.
    PooledNote,
    /// Estimates pooled across imputations by Rubin's rules.
    PooledEstimates,
    /// Within-, between- and total variance of the pooled estimates.
    PooledVariance,
    /// Tests of effects (several parameters) pooled across imputations.
    PooledTests,
    /// Fit statistics (R²) pooled across imputations.
    PooledFit,
    /// Estimates of each imputation.
    PooledByImputation,
    /// Summary of a described time series.
    SeriesSummary,
    /// Time series plot.
    SeriesPlot,
    /// Plot of the transformed (Box–Cox, differenced) series.
    SeriesTransformedPlot,
    /// Autocorrelation function plot.
    SeriesAcfPlot,
    /// Partial autocorrelation function plot.
    SeriesPacfPlot,
    /// Table of autocorrelations and partial autocorrelations.
    SeriesAutocorrelationTable,
    /// Ljung–Box portmanteau tests.
    SeriesLjungBox,
    /// Augmented Dickey–Fuller and KPSS tests.
    SeriesUnitRoot,
    /// Periodogram and smoothed spectral density.
    SeriesSpectrumPlot,
    /// Largest peaks of the spectrum.
    SeriesSpectrumPeaks,
    /// Method and strengths of a decomposition.
    DecompositionSummary,
    /// Observed series panel of a decomposition.
    DecompositionObserved,
    /// Trend panel of a decomposition.
    DecompositionTrend,
    /// Seasonal panel of a decomposition.
    DecompositionSeasonal,
    /// Remainder panel of a decomposition.
    DecompositionRemainder,
    /// Seasonal indices of a classical decomposition.
    DecompositionIndices,
    /// Data, fitted trend curve and forecasts.
    TrendModelPlot,
    /// The fitted trend equation.
    TrendModelEquation,
    /// Trend model coefficients.
    TrendModelCoefficients,
    /// MAPE, MAD and MSD of a trend model.
    TrendModelAccuracy,
    /// Trend model forecasts.
    TrendModelForecast,
    /// How to read the lags of a cross-correlation.
    CrossCorrelationNote,
    /// Cross-correlation plot.
    CrossCorrelationPlot,
    /// Table of cross-correlations.
    CrossCorrelationTable,
    /// Autoregression used to prewhiten the input series.
    CrossCorrelationFilter,
    /// Forecasting model: model, likelihood, information criteria, σ.
    ForecastModelSummary,
    /// Forecasting model: candidates of an automatic selection.
    ForecastModelComparison,
    /// Forecasting model: smoothing parameters or coefficients.
    ForecastParameters,
    /// Exponential smoothing: initial level, slope and seasonal states.
    ForecastInitialStates,
    /// Observations, fitted values and forecasts with interval bands.
    ForecastPlot,
    /// Forecasts with standard errors and prediction limits.
    ForecastTable,
    /// Residuals over time.
    ForecastResidualPlot,
    /// Residual autocorrelation with white-noise limits.
    ForecastResidualAcf,
    /// Ljung–Box test of the residuals.
    ForecastLjungBox,
    /// In-sample accuracy (RMSE, MAE, MAPE).
    ForecastAccuracy,
    /// ARIMA with inputs: each input's fitted transfer function and gain.
    ForecastTransferFunction,
    /// ARIMA with inputs: fitted and prewhitened impulse response weights.
    ForecastImpulseResponse,
    /// ARIMA with inputs: residual cross-correlation with the prewhitened input.
    ForecastInputResidualCcf,
    /// Bayesian analysis: the data summarized.
    BayesData,
    /// Bayesian analysis: the prior.
    BayesPrior,
    /// Why a contrast's posterior mean or SD is not shown.
    BayesMomentNote,
    /// Bayesian analysis: posterior means, SDs and credible intervals.
    BayesPosterior,
    /// Posterior probabilities about a reference value and an equivalence region.
    BayesProbabilities,
    /// Prior and posterior densities (plot).
    BayesDensity,
    /// Each group's posterior (plot).
    BayesGroupPosteriors,
    /// Bayes factor table.
    BayesFactor,
    /// Bayes factor against the prior scale (plot).
    BayesRobustness,
    /// Bayesian linear model: models with Bayes factors and probabilities.
    BayesModels,
    /// Bayesian linear model: inclusion probability of each effect.
    BayesInclusion,
    /// Bayesian linear model: inclusion probabilities (plot).
    BayesInclusionPlot,
    /// Bayesian linear model: model-averaged estimates.
    BayesEstimates,
    /// Bayesian linear model: estimates in the most probable model.
    BayesBestEstimates,
    /// Bayesian model: group (random) effects.
    BayesGroupEffects,
    /// Sampler diagnostics per chain.
    BayesSampler,
    /// Sampler warnings.
    BayesWarnings,
    /// PSIS-LOO and WAIC.
    BayesLoo,
    /// Coefficients with credible intervals (plot).
    BayesIntervals,
    /// Trace plot of one parameter.
    BayesTrace,
    /// Rank histograms of one parameter.
    BayesRank,
    /// Autocorrelation of one parameter's draws.
    BayesAutocorrelation,
    /// Posterior predictive check (plot).
    BayesPredictive,
    /// Tests of one factor within each level of another (slices of an interaction).
    SliceTests,
    /// Least-squares means of the cells of an interaction.
    SliceMeans,
    /// Pairwise differences of least-squares means within slices.
    SlicePairs,
    /// Each group's slope of a continuous predictor.
    SlopeComparisons,
    /// Test of equal slopes across groups.
    EqualSlopes,
    /// Pairwise differences of group slopes.
    SlopePairs,
    /// Knots of the spline terms of a model.
    SplineKnots,
    /// One row per response of a many-response screen.
    ScreeningResults,
    /// Effect size against −log₁₀ p across responses (plot).
    ScreeningVolcano,
    /// Two-level differences per response with intervals.
    ScreeningDifferences,
    /// Forest plot of the two-level differences.
    ScreeningForest,
    /// Counts of significant responses per effect.
    ScreeningSummary,
    /// Ratio of two means with Fieller's interval.
    RatioOfMeans,
    /// Differences of proportions between groups with Newcombe intervals.
    ProportionDifferences,
    /// Forest plot of the differences of proportions.
    ProportionForest,
    /// Adjusted residuals of a contingency table.
    AdjustedResiduals,
    /// Multiplicity-adjusted p-values across several tables.
    AdjustedTableTests,
    /// Held-out error of each model over repeated cross-validation.
    RepeatedCrossValidation,
    /// Forest plot of repeated cross-validation errors.
    RepeatedCrossValidationPlot,
    /// Signal processing: summary per signal.
    SignalSummary,
    /// Signal processing: a signal with its smoothed values, derivative or integral (plot).
    SignalPlot,
    /// Signal processing: the signals resampled onto an even grid.
    SignalResampled,
    /// Signal processing: notes (averaged rows, uneven spacing, plots left out).
    SignalNotes,
    /// Spectrum: peaks found.
    SpectrumPeaks,
    /// Spectrum: fitted peak parameters, one row per curve.
    SpectrumBatch,
    /// Spectrum: the spectrum and its baseline (plot).
    SpectrumBaselinePlot,
    /// Spectrum: corrected spectrum with the peaks found (plot).
    SpectrumPeakPlot,
    /// Spectrum: fit statistics (or why the fit failed).
    SpectrumFit,
    /// Spectrum: data, fitted sum and components (plot).
    SpectrumFitPlot,
    /// Spectrum: fitted centres, heights, widths and areas.
    SpectrumFitPeaks,
    /// Spectrum: every fitted parameter.
    SpectrumFitParameters,
    /// Spectrum: residuals of the peak fit (plot).
    SpectrumResiduals,
    /// Spectrum: fits with different numbers of peaks compared.
    SpectrumComparison,
    /// Signal report summary.
    SignalProcessingSummary,
    /// The signals with their rows (plot).
    SignalTrace,
    /// Amplitude spectrum (plot).
    AmplitudeSpectrum,
    /// Power spectrum or spectral density (plot).
    PowerSpectrum,
    /// Phase spectrum (plot).
    PhaseSpectrum,
    /// Largest spectral peaks.
    DominantFrequencies,
    /// Welch's averaged periodogram (plot) and its segments.
    WelchSpectrum,
    /// Spectrogram (time–frequency map).
    Spectrogram,
    /// Spectrum values by frequency.
    FourierValues,
    /// Filtered signal (plot).
    FilteredSignal,
    /// Filter gain against frequency (plot).
    FilterResponse,
    /// Filter phase against frequency (plot).
    FilterPhase,
    /// Spectrum before and after filtering (plot).
    FilterSpectrumComparison,
    /// Filter coefficients.
    FilterCoefficients,
    /// Cross- or autocorrelation against lag (plot).
    CorrelationPlot,
    /// Lags of the largest correlations.
    CorrelationPeaks,
    /// Convolved signal (plot).
    ConvolutionResult,
    /// Convolution kernel (plot).
    ConvolutionKernel,
    /// Deconvolved signal (plot).
    Deconvolved,
    /// Signal with its Hilbert envelope (plot).
    SignalEnvelope,
    /// Instantaneous frequency (plot).
    InstantaneousFrequency,
    /// Instantaneous phase (plot).
    InstantaneousPhase,
    /// Multiresolution components of a wavelet decomposition (plot).
    WaveletDecomposition,
    /// Energy, noise and thresholds by wavelet level.
    WaveletEnergy,
    /// Wavelet-denoised signal (plot).
    WaveletDenoised,
    /// Continuous-wavelet scalogram (time–frequency map).
    Scalogram,
    /// An image plot of a grid of values.
    ImagePlot,
    /// Size, spacing and value range of an image plot's grid.
    ImageSummary,
    /// Image spectrum: centred log-magnitude (or power) spectrum.
    ImageSpectrumMagnitude,
    /// Image spectrum: phase spectrum.
    ImageSpectrumPhase,
    /// Image spectrum: radially averaged power spectrum (plot).
    ImageRadialSpectrum,
    /// Image spectrum: power-law fit of the radial spectrum.
    ImagePowerLaw,
    /// Image spectrum: power by direction (plot).
    ImageAngularSpectrum,
    /// Image spectrum: radial profiles along chosen directions (plot).
    ImageRadialProfile,
    /// Image spectrum: the radial profiles' values.
    ImageRadialProfileValues,
    /// Image spectrum: dominant spatial frequencies.
    ImageDominantFrequencies,
    /// Image filter: the filtered image.
    ImageFiltered,
    /// Image filter: original minus filtered.
    ImageRemoved,
    /// Image filter: the frequency-domain mask.
    ImageFilterMask,
    /// Image filter: notch frequencies.
    ImageNotches,
    /// Image deblurring: the restored image.
    ImageRestored,
    /// Image registration: the image to align.
    ImageMoving,
    /// Image registration: the aligned image.
    ImageAligned,
    /// Image registration: difference before aligning.
    ImageDifferenceBefore,
    /// Image registration: difference after aligning.
    ImageDifferenceAfter,
    /// Image registration: the shift found.
    ImageShift,
    /// Image autocorrelation surface.
    ImageAutocorrelation,
}

impl SectionKind {
    /// Whether the section starts collapsed: long listings of values that
    /// a plot above already shows.
    #[must_use]
    pub fn starts_closed(self) -> bool {
        matches!(self, Self::ImageRadialProfileValues | Self::ResponseStepHistory(_))
    }
}

/// One collapsible section of a report.
#[derive(Debug, Clone, PartialEq)]
pub struct ReportSection {
    /// Identifier.
    pub kind: SectionKind,
    /// Heading.
    pub title: String,
    /// Body.
    pub content: SectionContent,
}

/// The "Prediction Formula" section of a model report: `formula`, or why it
/// cannot be written.
pub(crate) fn prediction_formula_section(formula: stat_core::Result<String>) -> ReportSection {
    ReportSection {
        kind: SectionKind::PredictionFormula,
        title: "Prediction Formula".to_owned(),
        content: match formula {
            Ok(f) => SectionContent::Formula(f),
            Err(e) => SectionContent::Note(format!("No prediction formula: {e}")),
        },
    }
}

/// User-toggleable parts of a linear-model report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportOptions {
    /// Show the analysis-of-variance table.
    pub show_anova: bool,
    /// Show confidence limits in the parameter table.
    pub show_confidence_limits: bool,
    /// Show the fitted-line / observed-vs-predicted plot.
    pub show_fit_plot: bool,
    /// Draw the confidence band around the fitted line.
    pub show_confidence_band: bool,
    /// Show the residual plot.
    pub show_residual_plot: bool,
    /// Show influence diagnostics (table, Cook's distance, residual quantile plot).
    pub show_diagnostics: bool,
    /// Show the Response Explorer.
    pub show_profiler: bool,
    /// Show the 3-D surface profiler (needs two continuous terms).
    pub show_surface_profiler: bool,
    /// Show observed data in the 3-D profiler.
    pub show_surface_observed: bool,
    /// Comparisons of every pair of levels, by this method (`None`: off).
    /// One-way models (a single categorical factor) only; rank-based Dunn is
    /// not offered, as it ignores the fitted model.
    pub pairwise: Option<stat_inference::multiple::ComparisonMethod>,
    /// Tests of equal variances across the levels (one-way models only).
    pub show_equal_variances: bool,
    /// Compare every level with this one, by position in the factor's
    /// levels (Dunnett 1955; one-way models only; `None`: off).
    pub control_level: Option<usize>,
    /// Compare every level with the best of the others, larger or smaller
    /// being better (Hsu 1996; one-way models only; `None`: off).
    pub best: Option<stat_inference::best::BestDirection>,
    /// Effect sizes of each effect (η², partial η², ω², Cohen's f) with
    /// noncentral-F intervals (Steiger 2004).
    pub show_effect_sizes: bool,
    /// Test for a linear trend across the levels in their order (one-way
    /// models with three or more levels only).
    pub linear_trend: bool,
    /// Comparisons within the slices of each two-factor interaction of
    /// categorical predictors, pairs adjusted by this method (`None`: off;
    /// [`stat_linear::marginal`]).
    pub slices: Option<stat_linear::marginal::PairAdjust>,
    /// Slopes of each `factor × continuous` effect by level, pairs adjusted
    /// by this method (`None`: off).
    pub slopes: Option<stat_linear::marginal::PairAdjust>,
}

impl Default for ReportOptions {
    fn default() -> Self {
        Self {
            show_anova: true,
            show_confidence_limits: true,
            show_fit_plot: true,
            show_confidence_band: true,
            show_residual_plot: false,
            show_diagnostics: false,
            show_profiler: true,
            show_surface_profiler: false,
            show_surface_observed: false,
            pairwise: Some(stat_inference::multiple::ComparisonMethod::TukeyKramer),
            show_equal_variances: true,
            control_level: None,
            best: None,
            show_effect_sizes: false,
            linear_trend: false,
            slices: None,
            slopes: None,
        }
    }
}

/// A note when `effects` (products of predictors, with `names`) break the
/// hierarchy principle: an interaction or power without the lower-order
/// effects it is built from ([`stat_linear::hierarchy`]).
#[must_use]
pub fn hierarchy_note(names: &[String], effects: &[Vec<(usize, u8)>]) -> Option<ReportSection> {
    let violations = stat_linear::hierarchy_violations(effects);
    if violations.is_empty() {
        return None;
    }
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let name = |e: &[(usize, u8)]| stat_linear::effect_name(&refs, e);
    let parts: Vec<String> = violations
        .iter()
        .map(|(e, missing)| {
            format!("{} without {}", name(e), missing.iter().map(|m| name(m)).collect::<Vec<_>>().join(", "))
        })
        .collect();
    Some(ReportSection {
        kind: SectionKind::HierarchyNote,
        title: "Model Hierarchy".to_owned(),
        content: SectionContent::Note(format!(
            "This model is not hierarchical: {}. An interaction or power without the effects it is built from \
             changes with the centring and coding of those effects; consider adding them.",
            parts.join("; ")
        )),
    })
}

/// The level below which report tables can mark a p-value as significant.
pub const SIGNIFICANCE_LEVEL: f64 = 0.05;

/// Whether a report table heading names a p-value column ("p", "p-value",
/// "p-value (…)").
#[must_use]
pub fn is_p_value_heading(heading: &str) -> bool {
    let h = heading.trim().to_lowercase();
    h == "p" || h.starts_with("p-value")
}

/// Whether a p-value cell, as reports format it ("0.0123", "3.1e-6", "<0.0001"),
/// is below `level`. Blank, "—" and unreadable cells are not.
#[must_use]
pub fn is_significant(cell: &str, level: f64) -> bool {
    let t = cell.trim().replace('−', "-");
    let (bound, number) = match t.strip_prefix('<') {
        Some(rest) => (true, rest.trim()),
        None => (false, t.as_str()),
    };
    number.parse::<f64>().is_ok_and(|p| if bound { p <= level } else { p < level })
}

#[cfg(test)]
mod significance_tests {
    use super::*;

    #[test]
    fn p_value_cells_and_headings() {
        assert!(is_p_value_heading("p-value"));
        assert!(is_p_value_heading("P"));
        assert!(is_p_value_heading("p-value (enter < 0.25, remove > 0.1)"));
        assert!(!is_p_value_heading("Prob Yes = 1"));
        assert!(!is_p_value_heading("Estimate"));
        assert!(is_significant("0.0123", SIGNIFICANCE_LEVEL));
        assert!(is_significant("3.1e-6", SIGNIFICANCE_LEVEL));
        assert!(is_significant("<0.0001", SIGNIFICANCE_LEVEL));
        assert!(!is_significant("0.0500", SIGNIFICANCE_LEVEL));
        assert!(!is_significant("0.2", SIGNIFICANCE_LEVEL));
        assert!(!is_significant("—", SIGNIFICANCE_LEVEL));
        assert!(!is_significant("", SIGNIFICANCE_LEVEL));
    }
}

#[cfg(test)]
mod text_table_tests {
    use super::*;

    fn anova() -> TextTable {
        let mut t = TextTable::new(["", "DF", "Estimate", "Note"]);
        t.push(["Model", "2", "−0.52", "a\tb"]);
        t.push(["Error", "10", "—", ""]);
        t
    }

    #[test]
    fn tsv_reads_as_spreadsheet_cells() {
        assert_eq!(anova().to_tsv(), "\tDF\tEstimate\tNote\nModel\t2\t-0.52\t\"a\tb\"\nError\t10\t\t\n");
    }

    #[test]
    fn data_table_types_numbers_and_names_blank_headings() {
        let t = anova().to_data_table("ANOVA").unwrap();
        let names: Vec<&str> = t.columns().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Column 1", "DF", "Estimate", "Note"]);
        assert_eq!(t.column(1).unwrap().as_numeric().map(<[f64]>::to_vec), Some(vec![2.0, 10.0]));
        let estimate = t.column(2).unwrap().as_numeric().unwrap();
        assert!((estimate[0] + 0.52).abs() < 1e-12 && estimate[1].is_nan());
        assert!(TextTable::new(["a"]).to_data_table("x").is_err());
    }
}

#[cfg(test)]
mod effect_pareto_tests {
    use super::*;

    #[test]
    fn effects_sort_by_p_value_with_untested_last() {
        let e = EffectPareto::new(
            [
                ("a".to_owned(), 0.2),
                ("b".to_owned(), f64::NAN),
                ("c".to_owned(), 1e-5),
                ("d".to_owned(), 0.04),
                ("e".to_owned(), 0.0),
            ],
            "test",
        );
        let names: Vec<&str> = e.effects.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["e", "c", "d", "a", "b"]);
        assert!((e.effects[1].neg_log10_p() - 5.0).abs() < 1e-12);
        assert_eq!(e.effects[0].neg_log10_p(), 300.0, "p = 0 is capped");
        assert!(e.effects[4].neg_log10_p().is_nan());
        let stars: Vec<&str> = e.effects.iter().map(EffectSource::stars).collect();
        assert_eq!(stars, ["***", "***", "*", "", ""]);
        let t = e.table();
        assert_eq!(t.cell(2, "−log₁₀ p"), Some("1.398"));
        let s = e.clone().section().unwrap();
        assert_eq!((s.kind, s.title.as_str()), (SectionKind::EffectPareto, "Effect Pareto: −log₁₀ p (test)"));
        assert!(EffectPareto::new(Vec::<(String, f64)>::new(), "x").section().is_none());
    }

    #[test]
    fn hierarchy_notes_name_missing_parents() {
        let names = vec!["A".to_owned(), "x".to_owned()];
        assert!(hierarchy_note(&names, &[vec![(0, 1)], vec![(1, 1)], vec![(0, 1), (1, 1)]]).is_none());
        let s = hierarchy_note(&names, &[vec![(0, 1)], vec![(0, 1), (1, 1)], vec![(1, 2)]]).unwrap();
        let SectionContent::Note(text) = &s.content else { panic!() };
        assert!(text.contains("A × x without x; x² without x"), "{text}");
    }
}
