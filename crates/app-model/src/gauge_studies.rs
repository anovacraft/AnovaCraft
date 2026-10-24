//! Measurement studies besides the ANOVA gauge R&R
//! ([`crate::gauge_analysis`]): run sheets for gauge studies, bias and
//! linearity against reference values, the single-gauge repeatability study
//! (Cg, Cgk) and the intraclass-correlation evaluation of a crossed study
//! ([`stat_quality::gauge_worksheet`], [`stat_quality::gauge_bias`],
//! [`stat_quality::icc`]).
//!
//! The three reports run through [`crate::ComparisonReport`], so front ends
//! edit their settings in place. Every plotted point is a dataset row, so
//! brushing links them to the data.

use crate::format::{format_number, format_optional, format_p_value};
use crate::project::{invalid, member, obj, s};
use crate::report::{Annotations, PlotSpec, ReportSection, RowPoint, SectionContent, SectionKind, TextTable};
use stat_core::Result;
use stat_data::{Column, DataTable, JsonValue, MeasurementLevel, RowStateManager};
use stat_quality::{
    BiasTest, Drift, GaugeLayout, GaugeObservation, GaugeOptions, Hysteresis, IncrementVerdict, LinearityStudy,
    RepeatabilityOptions, RepeatabilityStudy, RunOrder, ShroutFleiss, WorksheetPlan, bias_linearity, decimal_increment,
    drift, evaluate_measurement, gauge_rr_anova, gauge_worksheet, hysteresis, repeatability_study, shrout_fleiss,
};

/// Significant digits in report tables.
const DIGITS: usize = 5;

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

fn column_name(table: &DataTable, i: usize) -> JsonValue {
    s(table.column(i).map_or("", |c| c.name.as_str()))
}

fn find_column(v: &JsonValue, table: &DataTable, key: &str) -> Result<usize> {
    let name = member(v, key)?.as_str().ok_or_else(|| invalid(format!("{key}: expected a column name")))?;
    table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
}

fn numeric(table: &DataTable, i: usize) -> Result<(String, &[f64])> {
    let c = table.column(i).ok_or_else(|| invalid(format!("no column {i}")))?;
    let v = c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))?;
    Ok((c.name.clone(), v))
}

fn percent(v: f64) -> String {
    if v.is_finite() { format!("{v:.2}%") } else { "—".to_owned() }
}

fn interval(lo: f64, hi: f64) -> String {
    format!("[{}, {}]", format_number(lo, DIGITS), format_number(hi, DIGITS))
}

// ---------------------------------------------------------------------------
// Worksheet
// ---------------------------------------------------------------------------

/// Everything needed to create a gauge study run sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct GaugeWorksheetRequest {
    /// Parts: in all (crossed) or per operator (nested).
    pub parts: usize,
    /// Operator names (their number is the number of operators).
    pub operators: Vec<String>,
    /// Measurements of each part by each of its operators.
    pub replicates: usize,
    /// Crossed or nested.
    pub layout: GaugeLayout,
    /// Run order.
    pub order: RunOrder,
    /// Seed of the random order.
    pub seed: u64,
    /// Name of the empty measurement column.
    pub measurement: String,
}

impl Default for GaugeWorksheetRequest {
    /// 10 parts, operators A, B and C, 3 measurements each, crossed, random
    /// within each replicate.
    fn default() -> Self {
        Self {
            parts: 10,
            operators: ["A", "B", "C"].map(String::from).to_vec(),
            replicates: 3,
            layout: GaugeLayout::Crossed,
            order: RunOrder::RandomWithinReplicate,
            seed: 1,
            measurement: "Measurement".to_owned(),
        }
    }
}

/// The run sheet as a data table: `Run`, `Replicate`, `Operator`, `Part`
/// (labels `P01`, `P02`, …; nested parts are numbered across operators) and
/// an empty measurement column. The column names match what the gauge R&R
/// dialog looks for.
///
/// # Errors
/// Errors of [`gauge_worksheet`]; blank or repeated operator names; a blank
/// measurement name or one that clashes with another column.
pub fn create_gauge_worksheet(request: &GaugeWorksheetRequest) -> Result<DataTable> {
    let names: Vec<String> = request.operators.iter().map(|o| o.trim().to_owned()).collect();
    if names.iter().any(String::is_empty) {
        return Err(invalid("every operator needs a name"));
    }
    if names.iter().enumerate().any(|(i, n)| names[..i].contains(n)) {
        return Err(invalid("operator names must be different"));
    }
    let measurement = request.measurement.trim();
    if measurement.is_empty() || ["Run", "Replicate", "Operator", "Part"].contains(&measurement) {
        return Err(invalid("choose a measurement column name other than Run, Replicate, Operator and Part"));
    }
    let plan = WorksheetPlan {
        parts: request.parts,
        operators: names.len(),
        replicates: request.replicates,
        layout: request.layout,
        order: request.order,
        seed: request.seed,
    };
    let runs = gauge_worksheet(&plan)?;
    let total_parts = match request.layout {
        GaugeLayout::Crossed => request.parts,
        GaugeLayout::Nested => request.parts * names.len(),
    };
    let width = total_parts.to_string().len().max(2);
    let mut replicate = Column::numeric("Replicate", runs.iter().map(|r| (r.replicate + 1) as f64).collect());
    replicate.level = MeasurementLevel::Ordinal;
    let columns = vec![
        Column::numeric("Run", (1..=runs.len()).map(|i| i as f64).collect()),
        replicate,
        Column::text("Operator", runs.iter().map(|r| names[r.operator].clone()).collect()),
        Column::text("Part", runs.iter().map(|r| format!("P{:0width$}", r.part + 1)).collect()),
        Column::numeric(measurement, vec![f64::NAN; runs.len()]),
    ];
    let layout = match request.layout {
        GaugeLayout::Crossed => "crossed",
        GaugeLayout::Nested => "nested",
    };
    DataTable::new(format!("Gauge study worksheet ({layout})"), columns)
}

// ---------------------------------------------------------------------------
// Bias and linearity
// ---------------------------------------------------------------------------

/// A bias and linearity study: one row per measurement, with the part's
/// reference value in another column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinearityRequest {
    /// Reference value column.
    pub reference: usize,
    /// Measurement column.
    pub measurement: usize,
    /// Confidence level of the intervals and band.
    pub confidence: f64,
    /// Process variation (usually 6σ of the process) for %Linearity and %Bias.
    pub process_variation: Option<f64>,
}

impl LinearityRequest {
    /// A request with 95% intervals and no process variation.
    #[must_use]
    pub fn new(reference: usize, measurement: usize) -> Self {
        Self { reference, measurement, confidence: 0.95, process_variation: None }
    }

    /// Columns used.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        vec![self.reference, self.measurement]
    }

    /// The same request with columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(mut self, f: impl Fn(usize) -> usize) -> Self {
        self.reference = f(self.reference);
        self.measurement = f(self.measurement);
        self
    }

    /// The project-file form (`"analysis": "gauge-linearity"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        obj([
            ("analysis", s("gauge-linearity")),
            ("reference", column_name(table, self.reference)),
            ("measurement", column_name(table, self.measurement)),
            ("confidence", JsonValue::number(self.confidence)),
            ("process_variation", self.process_variation.map_or(JsonValue::Null, JsonValue::number)),
        ])
    }

    /// Read the project-file form.
    ///
    /// # Errors
    /// Missing or unknown columns.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        Ok(Self {
            reference: find_column(v, table, "reference")?,
            measurement: find_column(v, table, "measurement")?,
            confidence: v.get("confidence").and_then(JsonValue::as_f64).unwrap_or(0.95),
            process_variation: v.get("process_variation").and_then(JsonValue::as_f64),
        })
    }
}

/// A computed bias and linearity study.
#[derive(Debug, Clone)]
pub struct LinearityAnalysis {
    request: LinearityRequest,
    reference_name: String,
    measurement_name: String,
    study: LinearityStudy,
    /// (row, reference, bias) of each measurement used.
    points: Vec<(usize, f64, f64)>,
    n_excluded: usize,
}

impl LinearityAnalysis {
    /// Run the study on the rows with both values that are not excluded.
    ///
    /// # Errors
    /// The same column twice, non-numeric columns, and errors of
    /// [`bias_linearity`].
    pub fn run(table: &DataTable, states: &RowStateManager, request: &LinearityRequest) -> Result<Self> {
        if request.reference == request.measurement {
            return Err(invalid("the reference and measurement columns must differ"));
        }
        let (reference_name, x) = numeric(table, request.reference)?;
        let (measurement_name, y) = numeric(table, request.measurement)?;
        let mut points = Vec::new();
        let mut n_excluded = 0;
        for r in 0..table.n_rows() {
            if x[r].is_finite() && y[r].is_finite() {
                if states.is_excluded(r) {
                    n_excluded += 1;
                } else {
                    points.push((r, x[r], y[r] - x[r]));
                }
            }
        }
        let refs: Vec<f64> = points.iter().map(|p| p.1).collect();
        let meas: Vec<f64> = points.iter().map(|p| p.1 + p.2).collect();
        let study = bias_linearity(&refs, &meas, request.confidence, request.process_variation)?;
        Ok(Self { request: *request, reference_name, measurement_name, study, points, n_excluded })
    }

    /// The request this analysis answers.
    #[must_use]
    pub fn request(&self) -> &LinearityRequest {
        &self.request
    }

    /// The computed study.
    #[must_use]
    pub fn study(&self) -> &LinearityStudy {
        &self.study
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Bias and Linearity: {}", self.measurement_name)
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let st = &self.study;
        let level = format!("{}%", format_number(100.0 * st.confidence, 4));
        let mut summary = TextTable::new(["Item", "Value"]);
        let mut row = |a: &str, b: String| summary.push([a.to_owned(), b]);
        row("Measurements", (st.df + 2).to_string());
        row(&format!("Reference values ({})", self.reference_name), st.references.len().to_string());
        row("Average bias", format_number(st.average.bias, DIGITS));
        row(&format!("{level} interval for the average bias"), interval(st.average.lower, st.average.upper));
        row("p-value (average bias = 0)", format_optional_p(st.average.p_value));
        row("Linearity slope (change in bias per unit)", format_number(st.slope.estimate, DIGITS));
        row("p-value (slope = 0)", format_optional_p(st.slope.p_value));
        row("R²", format_number(st.r_squared, 4));
        row("%Linearity (100 × |slope|)", percent(st.percent_linearity()));
        if let Some(pv) = st.process_variation {
            row("Process variation", format_number(pv, DIGITS));
            row("Linearity (|slope| × process variation)", format_optional(st.linearity(), DIGITS));
            row("%Bias (100 × |average bias| / process variation)", st.percent_bias().map_or("—".into(), percent));
        }
        if self.n_excluded > 0 {
            row("Rows excluded", self.n_excluded.to_string());
        }
        let verdict = if st.zero_within_band() {
            format!(
                "The {level} band of the bias line contains zero at every reference value: no evidence of bias or \
                 linearity problems."
            )
        } else {
            format!(
                "The {level} band of the bias line excludes zero at some reference values: the gauge is biased \
                 there{}.",
                if st.slope.p_value.is_some_and(|p| p < 1.0 - st.confidence) {
                    ", and the bias changes over the range (linearity)"
                } else {
                    ""
                }
            )
        };

        let mut line = TextTable::new(["Term", "Estimate", "Std Error", "t", "p-value", "Lower", "Upper"]);
        for (name, c) in
            [("Intercept", &st.intercept), (format!("Slope ({})", self.reference_name).as_str(), &st.slope)]
        {
            line.push([
                name.to_owned(),
                format_number(c.estimate, DIGITS),
                format_number(c.std_error, DIGITS),
                format_optional(c.t, 4),
                format_optional_p(c.p_value),
                format_number(c.lower, DIGITS),
                format_number(c.upper, DIGITS),
            ]);
        }
        let mut by_ref = TextTable::new([
            "Reference",
            "N",
            "Mean",
            "Bias",
            "Std Dev",
            "t",
            "p-value",
            &format!("Lower {level}"),
            &format!("Upper {level}"),
        ]);
        for r in &st.references {
            by_ref.push(bias_row(format_number(r.reference, DIGITS), r.mean, &r.test));
        }
        by_ref.push(bias_row("Average".to_owned(), f64::NAN, &st.average));

        // Plot: each bias against its reference, the line and band, zero.
        let (lo, hi) = st
            .references
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), r| (a.min(r.reference), b.max(r.reference)));
        let xs: Vec<f64> = (0..=40).map(|i| lo + (hi - lo) * f64::from(i) / 40.0).collect();
        let band: Vec<(f64, f64, f64)> = xs.iter().map(|&x| (x, st.band(x).0, st.band(x).1)).collect();
        let means: Vec<(f64, f64, String, usize)> =
            st.references.iter().map(|r| (r.reference, r.test.bias, "Mean bias".to_owned(), 0)).collect();
        let plot = PlotSpec {
            x_label: self.reference_name.clone(),
            y_label: format!("Bias ({} − {})", self.measurement_name, self.reference_name),
            points: self.points.iter().map(|&(row, x, y)| RowPoint { row, x, y }).collect(),
            line: Some(band.iter().map(|&(x, f, _)| [x, f]).collect()),
            line_name: Some("Linearity line".to_owned()),
            band: Some((
                band.iter().map(|&(x, f, h)| [x, f - h]).collect(),
                band.iter().map(|&(x, f, h)| [x, f + h]).collect(),
            )),
            segments: vec![vec![[lo, 0.0], [hi, 0.0]]],
            annotations: Some(Annotations {
                groups: vec!["Mean bias at each reference".to_owned()],
                items: means,
                labels: vec![(hi, 0.0, "Bias = 0".to_owned())],
            }),
            ..PlotSpec::default()
        };
        vec![
            section(SectionKind::GaugeLinearitySummary, "Bias and Linearity Summary", SectionContent::Table(summary)),
            section(SectionKind::GaugeLinearityNote, "Conclusion", SectionContent::Note(verdict)),
            section(SectionKind::GaugeLinearityPlot, "Bias against Reference", SectionContent::Plot(Box::new(plot))),
            section(SectionKind::GaugeLinearityLine, "Linearity Line", SectionContent::Table(line)),
            section(SectionKind::GaugeBiasTable, "Bias by Reference Value", SectionContent::Table(by_ref)),
        ]
    }
}

fn format_optional_p(p: Option<f64>) -> String {
    p.map_or_else(|| "—".to_owned(), format_p_value)
}

fn bias_row(label: String, mean: f64, t: &BiasTest) -> Vec<String> {
    vec![
        label,
        t.n.to_string(),
        format_number(mean, DIGITS),
        format_number(t.bias, DIGITS),
        format_number(t.std_dev, DIGITS),
        format_optional(t.t, 4),
        format_optional_p(t.p_value),
        format_number(t.lower, DIGITS),
        format_number(t.upper, DIGITS),
    ]
}

// ---------------------------------------------------------------------------
// Single-gauge repeatability (Cg, Cgk)
// ---------------------------------------------------------------------------

/// A single-gauge repeatability study of one reference part.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RepeatabilityRequest {
    /// Measurement column (one row per repeated measurement, in order).
    pub measurement: usize,
    /// Reference value of the part.
    pub reference: f64,
    /// Tolerance `USL − LSL`.
    pub tolerance: f64,
    /// Percentage of the tolerance allowed for the gauge (20 by default).
    pub percent_of_tolerance: f64,
    /// Standard deviations in the gauge's spread (6 by default).
    pub spread: f64,
    /// Confidence level of the bias, hysteresis and drift intervals.
    pub confidence: f64,
    /// Column giving the direction each reading approached from (two
    /// values); adds the hysteresis section.
    pub direction: Option<usize>,
    /// Column of times or run order for the drift; `None`: the measurement
    /// order 1, 2, ….
    pub order: Option<usize>,
    /// Show the drift section (slope of readings over time or order).
    pub show_drift: bool,
    /// Summary statistics shown in the Summary Statistics section.
    pub summary: SummaryStats,
}

/// A set of summary statistics, as a bit mask (see [`SummaryStat`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SummaryStats(pub u16);

/// One statistic of a measurement summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryStat {
    /// Number of readings.
    Count,
    /// Mean.
    Mean,
    /// Standard deviation (n − 1).
    StdDev,
    /// Standard error of the mean.
    StdError,
    /// Smallest reading.
    Minimum,
    /// Median.
    Median,
    /// Largest reading.
    Maximum,
    /// Largest − smallest.
    Range,
    /// 6 × standard deviation (the usual repeatability spread).
    SixSigma,
    /// Coefficient of variation, 100 s / |mean|.
    Cv,
}

impl SummaryStat {
    /// Every statistic, in table order.
    pub const ALL: [Self; 10] = [
        Self::Count,
        Self::Mean,
        Self::StdDev,
        Self::StdError,
        Self::Minimum,
        Self::Median,
        Self::Maximum,
        Self::Range,
        Self::SixSigma,
        Self::Cv,
    ];

    /// Row label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Count => "N",
            Self::Mean => "Mean",
            Self::StdDev => "Standard deviation",
            Self::StdError => "Standard error of the mean",
            Self::Minimum => "Minimum",
            Self::Median => "Median",
            Self::Maximum => "Maximum",
            Self::Range => "Range",
            Self::SixSigma => "6 × standard deviation",
            Self::Cv => "Coefficient of variation (%)",
        }
    }

    /// Key in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Count => "n",
            Self::Mean => "mean",
            Self::StdDev => "sd",
            Self::StdError => "se",
            Self::Minimum => "min",
            Self::Median => "median",
            Self::Maximum => "max",
            Self::Range => "range",
            Self::SixSigma => "six_sd",
            Self::Cv => "cv",
        }
    }

    fn bit(self) -> u16 {
        1 << Self::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }
}

impl SummaryStats {
    /// N, mean, standard deviation, minimum, maximum and range.
    pub const DEFAULT: Self = Self(0b1101_0111);

    /// `true` if `stat` is shown.
    #[must_use]
    pub fn contains(self, stat: SummaryStat) -> bool {
        self.0 & stat.bit() != 0
    }

    /// Show or hide `stat`.
    pub fn set(&mut self, stat: SummaryStat, on: bool) {
        if on {
            self.0 |= stat.bit();
        } else {
            self.0 &= !stat.bit();
        }
    }

    /// The statistics shown, in table order.
    #[must_use]
    pub fn stats(self) -> Vec<SummaryStat> {
        SummaryStat::ALL.into_iter().filter(|s| self.contains(*s)).collect()
    }

    /// The set named by `keys` (unknown keys ignored).
    #[must_use]
    pub fn from_keys<'a>(keys: impl IntoIterator<Item = &'a str>) -> Self {
        let mut out = Self(0);
        for k in keys {
            if let Some(s) = SummaryStat::ALL.into_iter().find(|s| s.key() == k) {
                out.set(s, true);
            }
        }
        out
    }
}

impl Default for SummaryStats {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Cg and Cgk at or above this value are usually accepted.
pub const GAUGE_CAPABLE: f64 = 1.33;

impl RepeatabilityRequest {
    /// A request with the default `K = 20%`, `L = 6` and 95% confidence.
    #[must_use]
    pub fn new(measurement: usize, reference: f64, tolerance: f64) -> Self {
        let d = RepeatabilityOptions::new(reference, tolerance);
        Self {
            measurement,
            reference,
            tolerance,
            percent_of_tolerance: d.percent_of_tolerance,
            spread: d.spread,
            confidence: d.confidence,
            direction: None,
            order: None,
            show_drift: true,
            summary: SummaryStats::DEFAULT,
        }
    }

    /// Columns used.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut c = vec![self.measurement];
        c.extend(self.direction);
        c.extend(self.order);
        c
    }

    /// The same request with columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(mut self, f: impl Fn(usize) -> usize) -> Self {
        self.measurement = f(self.measurement);
        self.direction = self.direction.map(&f);
        self.order = self.order.map(&f);
        self
    }

    /// The project-file form (`"analysis": "gauge-repeatability"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let n = JsonValue::number;
        obj([
            ("analysis", s("gauge-repeatability")),
            ("measurement", column_name(table, self.measurement)),
            ("reference", n(self.reference)),
            ("tolerance", n(self.tolerance)),
            ("percent_of_tolerance", n(self.percent_of_tolerance)),
            ("spread", n(self.spread)),
            ("confidence", n(self.confidence)),
            ("direction", self.direction.map_or(JsonValue::Null, |c| column_name(table, c))),
            ("order", self.order.map_or(JsonValue::Null, |c| column_name(table, c))),
            ("show_drift", JsonValue::Bool(self.show_drift)),
            ("summary_statistics", JsonValue::Array(self.summary.stats().into_iter().map(|st| s(st.key())).collect())),
        ])
    }

    /// Read the project-file form.
    ///
    /// # Errors
    /// A missing column, reference or tolerance.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let num = |key: &str| member(v, key)?.as_f64().ok_or_else(|| invalid(format!("{key}: expected a number")));
        let d = Self::new(find_column(v, table, "measurement")?, num("reference")?, num("tolerance")?);
        let opt = |key: &str, default: f64| v.get(key).and_then(JsonValue::as_f64).unwrap_or(default);
        let optional_column = |key: &str| match v.get(key) {
            None | Some(JsonValue::Null) => Ok(None),
            Some(_) => find_column(v, table, key).map(Some),
        };
        Ok(Self {
            percent_of_tolerance: opt("percent_of_tolerance", d.percent_of_tolerance),
            spread: opt("spread", d.spread),
            confidence: opt("confidence", d.confidence),
            direction: optional_column("direction")?,
            order: optional_column("order")?,
            show_drift: v.get("show_drift").and_then(JsonValue::as_bool).unwrap_or(true),
            summary: v
                .get("summary_statistics")
                .and_then(JsonValue::as_array)
                .map_or(SummaryStats::DEFAULT, |a| SummaryStats::from_keys(a.iter().filter_map(JsonValue::as_str))),
            ..d
        })
    }
}

/// A computed single-gauge repeatability study.
#[derive(Debug, Clone)]
pub struct RepeatabilityAnalysis {
    request: RepeatabilityRequest,
    name: String,
    study: RepeatabilityStudy,
    /// (row, value) in row order.
    points: Vec<(usize, f64)>,
    n_excluded: usize,
    /// Hysteresis between the two directions (their names), or why not.
    hysteresis: Option<std::result::Result<(String, String, Hysteresis), String>>,
    /// Drift over time or order (with the time column's name), or why not.
    drift: Option<std::result::Result<(String, Drift), String>>,
}

impl RepeatabilityAnalysis {
    /// Run the study on the non-excluded rows with a value.
    ///
    /// # Errors
    /// A non-numeric column and errors of [`repeatability_study`].
    pub fn run(table: &DataTable, states: &RowStateManager, request: &RepeatabilityRequest) -> Result<Self> {
        let (name, y) = numeric(table, request.measurement)?;
        let present: Vec<usize> = (0..table.n_rows()).filter(|&r| y[r].is_finite()).collect();
        let points: Vec<(usize, f64)> =
            present.iter().filter(|&&r| !states.is_excluded(r)).map(|&r| (r, y[r])).collect();
        let n_excluded = present.len() - points.len();
        let options = RepeatabilityOptions {
            reference: request.reference,
            tolerance: request.tolerance,
            percent_of_tolerance: request.percent_of_tolerance,
            spread: request.spread,
            confidence: request.confidence,
        };
        let values: Vec<f64> = points.iter().map(|p| p.1).collect();
        let study = repeatability_study(&values, &options)?;
        let hysteresis = request.direction.map(|c| -> std::result::Result<_, String> {
            let col = table.column(c).ok_or_else(|| format!("no column {c}"))?;
            let mut levels: Vec<String> = Vec::new();
            let mut groups: Vec<Vec<f64>> = Vec::new();
            for &(r, y) in &points {
                let level = col.level_text(r);
                if level.is_empty() {
                    continue;
                }
                match levels.iter().position(|l| *l == level) {
                    Some(k) => groups[k].push(y),
                    None => {
                        levels.push(level);
                        groups.push(vec![y]);
                    }
                }
            }
            if levels.len() != 2 {
                return Err(format!(
                    "the direction column {:?} needs exactly two values (it has {})",
                    col.name,
                    levels.len()
                ));
            }
            let h = hysteresis(&groups[0], &groups[1], request.confidence).map_err(|e| e.to_string())?;
            Ok((levels[0].clone(), levels[1].clone(), h))
        });
        let drift = request.show_drift.then(|| -> std::result::Result<_, String> {
            let (label, t, y): (String, Vec<f64>, Vec<f64>) = match request.order {
                Some(c) => {
                    let (name, t) = numeric(table, c).map_err(|e| e.to_string())?;
                    let pairs: Vec<(f64, f64)> =
                        points.iter().filter(|p| t[p.0].is_finite()).map(|&(r, y)| (t[r], y)).collect();
                    (name, pairs.iter().map(|p| p.0).collect(), pairs.iter().map(|p| p.1).collect())
                }
                None => {
                    ("Measurement order".to_owned(), (1..=points.len()).map(|i| i as f64).collect(), values.clone())
                }
            };
            Ok((label, drift(&t, &y, request.confidence).map_err(|e| e.to_string())?))
        });
        Ok(Self { request: *request, name, study, points, n_excluded, hysteresis, drift })
    }

    /// The computed study.
    #[must_use]
    pub fn study(&self) -> &RepeatabilityStudy {
        &self.study
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Gauge Repeatability (Cg, Cgk): {}", self.name)
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let st = &self.study;
        let o = st.options;
        let (k, l) = (format_number(o.percent_of_tolerance, 4), format_number(o.spread, 4));
        let verdict = |c: Option<f64>| match c {
            Some(c) if c >= GAUGE_CAPABLE => format!("{} (≥ {GAUGE_CAPABLE}: capable)", format_number(c, 4)),
            Some(c) => format!("{} (< {GAUGE_CAPABLE}: not capable)", format_number(c, 4)),
            None => "— (no variation)".to_owned(),
        };
        let mut cap = TextTable::new(["Item", "Value"]);
        let mut row = |a: &str, b: String| cap.push([a.to_owned(), b]);
        row(&format!("Cg = ({k}% × T) / ({l} s)"), verdict(st.cg));
        row(&format!("Cgk = ({k}%/2 × T − |bias|) / ({l}/2 × s)"), verdict(st.cgk));
        row(&format!("%Repeatability = 100 × {l} s / T"), percent(st.percent_repeatability));
        row(&format!("%Repeatability and bias = 100 × ({l} s + 2|bias|) / T"), percent(st.percent_repeatability_bias));
        row("Tolerance T", format_number(o.tolerance, DIGITS));
        row("Measurements", st.bias.n.to_string());
        if self.n_excluded > 0 {
            row("Rows excluded", self.n_excluded.to_string());
        }
        let level = format!("{}%", format_number(100.0 * o.confidence, 4));
        let mut bias = TextTable::new(["Item", "Value"]);
        let mut row = |a: &str, b: String| bias.push([a.to_owned(), b]);
        row("Reference value", format_number(o.reference, DIGITS));
        row("Mean", format_number(st.mean, DIGITS));
        row("Bias (mean − reference)", format_number(st.bias.bias, DIGITS));
        row("Standard deviation s", format_number(st.bias.std_dev, DIGITS));
        row(&format!("{level} interval for the bias"), interval(st.bias.lower, st.bias.upper));
        row("t", format_optional(st.bias.t, 4));
        row("p-value (bias = 0)", format_optional_p(st.bias.p_value));

        // Run chart with the reference, the allowed band and the mean.
        let n = self.points.len() as f64;
        let half = o.percent_of_tolerance / 200.0 * o.tolerance;
        let (x0, x1) = (1.0, n.max(2.0));
        let hline = |y: f64| vec![[x0, y], [x1, y]];
        let plot = PlotSpec {
            x_label: "Measurement order".to_owned(),
            y_label: self.name.clone(),
            points: self
                .points
                .iter()
                .enumerate()
                .map(|(i, &(row, y))| RowPoint { row, x: (i + 1) as f64, y })
                .collect(),
            line: Some(hline(st.mean)),
            line_name: Some("Mean".to_owned()),
            point_name: Some("Measurements".to_owned()),
            segments: vec![hline(o.reference), hline(o.reference - half), hline(o.reference + half)],
            annotations: Some(Annotations {
                groups: Vec::new(),
                items: Vec::new(),
                labels: vec![
                    (x1, o.reference, "Reference".to_owned()),
                    (x1, o.reference + half, format!("Reference + {k}%/2 of T")),
                    (x1, o.reference - half, format!("Reference − {k}%/2 of T")),
                    (x0, st.mean, "Mean".to_owned()),
                ],
            }),
            ..PlotSpec::default()
        };
        let mut out =
            vec![section(SectionKind::GaugeCapabilityIndices, "Gauge Capability", SectionContent::Table(cap))];
        if let Some(t) = self.summary_table() {
            out.push(section(SectionKind::GaugeSummaryStatistics, "Summary Statistics", SectionContent::Table(t)));
        }
        out.push(section(SectionKind::GaugeBiasTest, "Bias", SectionContent::Table(bias)));
        if let Some(h) = &self.hysteresis {
            out.push(self.hysteresis_section(h, &level));
        }
        if let Some(d) = &self.drift {
            out.push(self.drift_section(d, &level));
        }
        out.push(section(SectionKind::GaugeRunChart, "Measurements in Order", SectionContent::Plot(Box::new(plot))));
        out
    }

    /// The chosen summary statistics of the readings (`None` when none is chosen).
    fn summary_table(&self) -> Option<TextTable> {
        let stats = self.request.summary.stats();
        if stats.is_empty() {
            return None;
        }
        let mut y: Vec<f64> = self.points.iter().map(|p| p.1).collect();
        y.sort_by(f64::total_cmp);
        let n = y.len();
        let st = &self.study;
        let sd = st.bias.std_dev;
        let median = if n % 2 == 1 { y[n / 2] } else { 0.5 * (y[n / 2 - 1] + y[n / 2]) };
        let mut t = TextTable::new(["Statistic", "Value"]);
        for stat in stats {
            let value = match stat {
                SummaryStat::Count => n.to_string(),
                SummaryStat::Mean => format_number(st.mean, DIGITS),
                SummaryStat::StdDev => format_number(sd, DIGITS),
                SummaryStat::StdError => format_number(st.bias.std_error, DIGITS),
                SummaryStat::Minimum => format_number(y[0], DIGITS),
                SummaryStat::Median => format_number(median, DIGITS),
                SummaryStat::Maximum => format_number(y[n - 1], DIGITS),
                SummaryStat::Range => format_number(y[n - 1] - y[0], DIGITS),
                SummaryStat::SixSigma => format_number(6.0 * sd, DIGITS),
                SummaryStat::Cv => {
                    if st.mean == 0.0 {
                        "—".to_owned()
                    } else {
                        format_number(100.0 * sd / st.mean.abs(), DIGITS)
                    }
                }
            };
            t.push([stat.label().to_owned(), value]);
        }
        Some(t)
    }

    fn hysteresis_section(
        &self,
        h: &std::result::Result<(String, String, Hysteresis), String>,
        level: &str,
    ) -> ReportSection {
        let title = "Hysteresis (by direction of approach)";
        let (a, b, h) = match h {
            Ok(x) => x,
            Err(e) => {
                return section(
                    SectionKind::GaugeHysteresis,
                    title,
                    SectionContent::Note(format!("Not available: {e}")),
                );
            }
        };
        let mut t = TextTable::new(["Item", "Value"]);
        let mut row = |x: &str, y: String| t.push([x.to_owned(), y]);
        row(&format!("Mean approached from {a} (n = {})", h.n.0), format_number(h.means.0, DIGITS));
        row(&format!("Mean approached from {b} (n = {})", h.n.1), format_number(h.means.1, DIGITS));
        row(&format!("Hysteresis ({a} − {b})"), format_number(h.difference, DIGITS));
        row("Standard error (Welch)", format_number(h.std_error, DIGITS));
        row(&format!("{level} interval"), interval(h.lower, h.upper));
        row("t", format_optional(h.t, 4));
        row("Degrees of freedom", format_number(h.df, 4));
        row("p-value (hysteresis = 0)", format_optional_p(h.p_value));
        row("Hysteresis as % of the tolerance", percent(100.0 * h.difference.abs() / self.study.options.tolerance));
        section(SectionKind::GaugeHysteresis, title, SectionContent::Table(t))
    }

    fn drift_section(&self, d: &std::result::Result<(String, Drift), String>, level: &str) -> ReportSection {
        let (label, d) = match d {
            Ok(x) => x,
            Err(e) => {
                return section(SectionKind::GaugeDrift, "Drift", SectionContent::Note(format!("Not available: {e}")));
            }
        };
        let mut t = TextTable::new(["Item", "Value"]);
        let mut row = |x: &str, y: String| t.push([x.to_owned(), y]);
        row(&format!("Slope (change per unit of {label})"), format_number(d.slope, DIGITS));
        row("Standard error", format_number(d.std_error, DIGITS));
        row(&format!("{level} interval"), interval(d.lower, d.upper));
        row("t", format_optional(d.t, 4));
        row("p-value (no drift)", format_optional_p(d.p_value));
        row("Fitted change over the study", format_number(d.total_change(), DIGITS));
        row("Change as % of the tolerance", percent(100.0 * d.total_change().abs() / self.study.options.tolerance));
        row("Readings", d.n.to_string());
        section(SectionKind::GaugeDrift, format!("Drift over {label}"), SectionContent::Table(t))
    }
}

// ---------------------------------------------------------------------------
// Intraclass correlation evaluation
// ---------------------------------------------------------------------------

/// An intraclass-correlation evaluation of a balanced crossed study (the same
/// layout as gauge R&R; one or more measurements per part and operator).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IccRequest {
    /// Part column.
    pub part: usize,
    /// Operator column.
    pub operator: usize,
    /// Measurement column.
    pub measurement: usize,
    /// Confidence level of the Shrout & Fleiss intervals.
    pub confidence: f64,
    /// The increment in which measurements are recorded (`None`: detected
    /// from the decimals of the values).
    pub increment: Option<f64>,
}

impl IccRequest {
    /// A request with 95% intervals and a detected increment.
    #[must_use]
    pub fn new(part: usize, operator: usize, measurement: usize) -> Self {
        Self { part, operator, measurement, confidence: 0.95, increment: None }
    }

    /// Columns used.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        vec![self.part, self.operator, self.measurement]
    }

    /// The same request with columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(mut self, f: impl Fn(usize) -> usize) -> Self {
        self.part = f(self.part);
        self.operator = f(self.operator);
        self.measurement = f(self.measurement);
        self
    }

    /// The project-file form (`"analysis": "intraclass-correlation"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        obj([
            ("analysis", s("intraclass-correlation")),
            ("part", column_name(table, self.part)),
            ("operator", column_name(table, self.operator)),
            ("measurement", column_name(table, self.measurement)),
            ("confidence", JsonValue::number(self.confidence)),
            ("increment", self.increment.map_or(JsonValue::Null, JsonValue::number)),
        ])
    }

    /// Read the project-file form.
    ///
    /// # Errors
    /// Missing or unknown columns.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        Ok(Self {
            part: find_column(v, table, "part")?,
            operator: find_column(v, table, "operator")?,
            measurement: find_column(v, table, "measurement")?,
            confidence: v.get("confidence").and_then(JsonValue::as_f64).unwrap_or(0.95),
            increment: v.get("increment").and_then(JsonValue::as_f64),
        })
    }
}

/// One estimate of the product and error variances.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ErrorSource {
    label: &'static str,
    product: f64,
    error: f64,
}

/// A computed intraclass-correlation evaluation.
#[derive(Debug, Clone)]
pub struct IccAnalysis {
    request: IccRequest,
    measurement_name: String,
    parts: usize,
    operators: usize,
    replicates: usize,
    sources: Vec<ErrorSource>,
    shrout_fleiss: ShroutFleiss,
    increment: Option<(f64, bool)>,
    n_excluded: usize,
}

impl IccAnalysis {
    /// Run the evaluation on the non-excluded rows with all three values.
    ///
    /// # Errors
    /// Repeated or non-numeric columns, an unbalanced study (every part must
    /// be measured equally often by every operator), fewer than two parts or
    /// operators, or no variation within parts.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &IccRequest) -> Result<Self> {
        let (p, o, m) = (request.part, request.operator, request.measurement);
        if p == o || p == m || o == m {
            return Err(invalid("part, operator and measurement must be three different columns"));
        }
        let col = |i: usize| table.column(i).ok_or_else(|| invalid(format!("no column {i}")));
        let (part_col, oper_col) = (col(p)?, col(o)?);
        let (measurement_name, values) = numeric(table, m)?;
        let mut parts: Vec<String> = Vec::new();
        let mut operators: Vec<String> = Vec::new();
        let level = |levels: &mut Vec<String>, label: String| {
            levels.iter().position(|l| *l == label).unwrap_or_else(|| {
                levels.push(label);
                levels.len() - 1
            })
        };
        let mut obs = Vec::new();
        let mut n_excluded = 0;
        for r in 0..table.n_rows() {
            let (pl, ol, v) = (part_col.level_text(r), oper_col.level_text(r), values[r]);
            if pl.is_empty() || ol.is_empty() || !v.is_finite() {
                continue;
            }
            if states.is_excluded(r) {
                n_excluded += 1;
                continue;
            }
            let part = level(&mut parts, pl);
            let operator = level(&mut operators, ol);
            obs.push(GaugeObservation { part, operator, value: v });
        }
        let (np, no) = (parts.len(), operators.len());
        if np < 2 || no < 2 {
            return Err(invalid("the study needs at least two parts and two operators"));
        }
        let mut cells: Vec<Vec<f64>> = vec![Vec::new(); np * no];
        for ob in &obs {
            cells[ob.part * no + ob.operator].push(ob.value);
        }
        let replicates = cells[0].len();
        if replicates == 0 || cells.iter().any(|c| c.len() != replicates) {
            return Err(invalid(
                "the study must be balanced: every part measured the same number of times by every operator \
                 (excluded rows and missing values count against this)",
            ));
        }
        let means: Vec<Vec<f64>> = (0..np)
            .map(|i| (0..no).map(|j| cells[i * no + j].iter().sum::<f64>() / replicates as f64).collect())
            .collect();
        let sf = shrout_fleiss(&means, request.confidence)?;
        let sources = if replicates >= 2 {
            let g = gauge_rr_anova(&obs, &GaugeOptions::default())?.components;
            vec![
                ErrorSource {
                    label: "Repeatability and reproducibility",
                    product: g.part.variance,
                    error: g.gauge_rr.variance,
                },
                ErrorSource {
                    label: "Repeatability only (a single operator)",
                    product: g.part.variance,
                    error: g.repeatability.variance,
                },
            ]
        } else {
            // One measurement per cell: the two-way ANOVA without interaction.
            let (n, k) = (np as f64, no as f64);
            vec![ErrorSource {
                label: "Repeatability and reproducibility",
                product: ((sf.bms - sf.ems) / k).max(0.0),
                error: sf.ems + ((sf.jms - sf.ems) / n).max(0.0),
            }]
        };
        let increment = match request.increment {
            Some(u) => Some((u, false)),
            None => decimal_increment(&obs.iter().map(|o| o.value).collect::<Vec<_>>()).map(|u| (u, true)),
        };
        // Validate once so the report cannot fail later.
        evaluate_measurement(sources[0].product, sources[0].error, increment.map(|u| u.0))?;
        for s in &sources[1..] {
            evaluate_measurement(s.product, s.error, None)?;
        }
        Ok(Self {
            request: *request,
            measurement_name,
            parts: np,
            operators: no,
            replicates,
            sources,
            shrout_fleiss: sf,
            increment,
            n_excluded,
        })
    }

    /// The Shrout & Fleiss correlations of the part × operator means.
    #[must_use]
    pub fn shrout_fleiss(&self) -> &ShroutFleiss {
        &self.shrout_fleiss
    }

    /// Intraclass correlation with repeatability and reproducibility as error.
    #[must_use]
    pub fn intraclass_correlation(&self) -> f64 {
        let s = self.sources[0];
        s.product / (s.product + s.error)
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Intraclass Correlation: {}", self.measurement_name)
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let main = self.sources[0];
        let e = evaluate_measurement(main.product, main.error, self.increment.map(|u| u.0))
            .expect("validated when the analysis ran");
        let mut eval = TextTable::new(["Item", "Value"]);
        let mut row = |a: &str, b: String| eval.push([a.to_owned(), b]);
        row("Intraclass correlation ρ (product share of the variance)", format_number(e.intraclass_correlation, 4));
        row("Class as a process monitor", format!("{}: {}", e.class.label(), e.class.meaning()));
        row("Attenuation of process signals (1 − √ρ)", percent(100.0 * e.attenuation));
        row("Measurement error σₑ", format_number(e.error_variance.sqrt(), DIGITS));
        row("Probable error (0.675 σₑ; half of all measurements err by less)", format_number(e.probable_error, DIGITS));
        row(
            "Effective measurement increment (0.2 to 2 probable errors)",
            interval(e.smallest_effective_increment, e.largest_effective_increment),
        );
        if let Some((u, detected)) = self.increment {
            row(
                if detected { "Recorded increment (from the decimals of the data)" } else { "Recorded increment" },
                format_number(u, DIGITS),
            );
            row(
                "Increment",
                match e.increment_verdict {
                    Some(IncrementVerdict::TooFine) => "finer than needed: the last digit records noise".to_owned(),
                    Some(IncrementVerdict::Effective) => "within the effective range".to_owned(),
                    Some(IncrementVerdict::TooCoarse) => {
                        "coarser than the measurement error: another digit would carry information".to_owned()
                    }
                    None => "—".to_owned(),
                },
            );
        }
        row(
            "Parts × operators × measurements each",
            format!("{} × {} × {}", self.parts, self.operators, self.replicates),
        );
        if self.n_excluded > 0 {
            row("Rows excluded", self.n_excluded.to_string());
        }

        let mut by_source =
            TextTable::new(["Measurement error", "Product variance", "Error variance", "ρ", "Attenuation", "Class"]);
        for s in &self.sources {
            let ev = evaluate_measurement(s.product, s.error, None).expect("validated variances");
            by_source.push([
                s.label.to_owned(),
                format_number(s.product, DIGITS),
                format_number(s.error, DIGITS),
                format_number(ev.intraclass_correlation, 4),
                percent(100.0 * ev.attenuation),
                ev.class.label().to_owned(),
            ]);
        }

        let sf = &self.shrout_fleiss;
        let level = format!("{}%", format_number(100.0 * sf.confidence, 4));
        let mut forms = TextTable::new([
            "Form",
            "Model",
            "ICC",
            &format!("Lower {level}"),
            &format!("Upper {level}"),
            "F",
            "DF num",
            "DF den",
            "p-value",
        ]);
        for est in &sf.estimates {
            forms.push([
                est.form.notation(sf.judges),
                est.form.description().to_owned(),
                format_number(est.value, 4),
                format_number(est.lower, 4),
                format_number(est.upper, 4),
                format_number(est.f, 4),
                format_number(est.df1, 4),
                format_number(est.df2, 4),
                format_p_value(est.p_value),
            ]);
        }
        let mut notes = format!(
            "Variance components are the ANOVA (method-of-moments) estimates of the crossed study, truncated at \
             zero. Classes follow Wheeler: first ρ ≥ 0.8, second ≥ 0.5, third ≥ 0.2, fourth below. The {} \
             Shrout & Fleiss forms treat parts as targets and operators as judges.",
            sf.estimates.len()
        );
        if self.replicates > 1 {
            notes.push_str(&format!(
                " They are computed from each part's mean of the {} measurements by each operator.",
                self.replicates
            ));
        }
        vec![
            section(SectionKind::IccEvaluation, "Measurement Process Evaluation", SectionContent::Table(eval)),
            section(
                SectionKind::IccBySource,
                "Intraclass Correlation by Error Source",
                SectionContent::Table(by_source),
            ),
            section(SectionKind::IccForms, "Shrout & Fleiss Intraclass Correlations", SectionContent::Table(forms)),
            section(SectionKind::IccNote, "About These Measures", SectionContent::Note(notes)),
        ]
    }

    /// The request this analysis answers.
    #[must_use]
    pub fn request(&self) -> &IccRequest {
        &self.request
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample;

    #[test]
    fn worksheet_table_matches_the_gauge_dialog_names() {
        let t = create_gauge_worksheet(&GaugeWorksheetRequest::default()).unwrap();
        assert_eq!(t.n_rows(), 90);
        let names: Vec<&str> = t.columns().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Run", "Replicate", "Operator", "Part", "Measurement"]);
        let parts: std::collections::HashSet<String> = (0..90).map(|r| t.column(3).unwrap().level_text(r)).collect();
        assert_eq!(parts.len(), 10);
        assert!(parts.contains("P01") && parts.contains("P10"));
        // Random within each replicate: the first 30 runs are replicate 1.
        assert!((0..30).all(|r| t.column(1).unwrap().as_numeric().unwrap()[r] == 1.0));
        let nested = create_gauge_worksheet(&GaugeWorksheetRequest {
            layout: GaugeLayout::Nested,
            parts: 5,
            replicates: 2,
            ..GaugeWorksheetRequest::default()
        })
        .unwrap();
        assert_eq!(nested.n_rows(), 30);
        let parts: std::collections::HashSet<String> =
            (0..30).map(|r| nested.column(3).unwrap().level_text(r)).collect();
        assert_eq!(parts.len(), 15);
        let bad = GaugeWorksheetRequest { operators: vec!["A".into(), "A".into()], ..GaugeWorksheetRequest::default() };
        assert!(create_gauge_worksheet(&bad).is_err());
        let bad = GaugeWorksheetRequest { measurement: "Part".into(), ..GaugeWorksheetRequest::default() };
        assert!(create_gauge_worksheet(&bad).is_err());
    }

    #[test]
    fn linearity_report_on_the_sample() {
        let t = sample::gauge_linearity();
        let states = RowStateManager::new(t.n_rows());
        let req = LinearityRequest::new(t.find("Reference").unwrap(), t.find("Measurement").unwrap());
        let a = LinearityAnalysis::run(&t, &states, &req).unwrap();
        assert_eq!(a.study().references.len(), 5);
        let s = a.sections();
        assert_eq!(s.len(), 5);
        let SectionContent::Plot(p) = &s[2].content else { panic!() };
        assert_eq!(p.points.len(), t.n_rows());
        assert!(p.band.is_some());
        let SectionContent::Table(by_ref) = &s[4].content else { panic!() };
        assert_eq!(by_ref.rows.len(), 6);
        assert_eq!(by_ref.rows[5][0], "Average");
        // The simulated gauge reads high at small values and low at large ones.
        assert!(a.study().slope.estimate < 0.0 && a.study().slope.p_value.unwrap() < 0.01);
        let back = LinearityRequest::from_json(&req.to_json(&t), &t).unwrap();
        assert_eq!(back, req);
        let mut states = states;
        states.set_excluded(0..12, true);
        let b = LinearityAnalysis::run(&t, &states, &req).unwrap();
        assert_eq!(b.study().references.len(), 4, "excluding a reference's rows drops it");
        assert!(LinearityAnalysis::run(&t, &states, &LinearityRequest::new(0, 0)).is_err());
    }

    #[test]
    fn repeatability_report_on_the_sample() {
        let t = sample::gauge_repeatability();
        let states = RowStateManager::new(t.n_rows());
        let req = RepeatabilityRequest::new(t.find("Measurement").unwrap(), 25.0, 0.1);
        let a = RepeatabilityAnalysis::run(&t, &states, &req).unwrap();
        assert_eq!(a.study().bias.n, 50);
        let cg = a.study().cg.unwrap();
        assert!(cg > GAUGE_CAPABLE, "the sample gauge is capable: Cg = {cg}");
        let s = a.sections();
        let kinds: Vec<SectionKind> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::GaugeCapabilityIndices,
                SectionKind::GaugeSummaryStatistics,
                SectionKind::GaugeBiasTest,
                SectionKind::GaugeDrift,
                SectionKind::GaugeRunChart
            ]
        );
        let SectionContent::Plot(p) = &s[4].content else { panic!() };
        assert_eq!(p.points.len(), 50);
        assert_eq!(p.points[9].x, 10.0);
        assert_eq!(p.segments.len(), 3);
        let SectionContent::Table(summary) = &s[1].content else { panic!() };
        let labels: Vec<&str> = summary.rows.iter().map(|r| r[0].as_str()).collect();
        assert_eq!(labels, ["N", "Mean", "Standard deviation", "Minimum", "Maximum", "Range"]);
        let back = RepeatabilityRequest::from_json(&req.to_json(&t), &t).unwrap();
        assert_eq!(back, req);

        // The sample's own report: hysteresis by approach, drift over Order.
        let full = RepeatabilityRequest {
            direction: t.find("Approach"),
            order: t.find("Order"),
            summary: SummaryStats::from_keys(["median", "cv"]),
            ..req
        };
        let a = RepeatabilityAnalysis::run(&t, &states, &full).unwrap();
        let s = a.sections();
        let h = s.iter().find(|x| x.kind == SectionKind::GaugeHysteresis).unwrap();
        let SectionContent::Table(h) = &h.content else { panic!() };
        assert_eq!(h.rows[2][0], "Hysteresis (below − above)");
        let SectionContent::Table(summary) = &s[1].content else { panic!() };
        assert_eq!(summary.rows.len(), 2);
        let d = s.iter().find(|x| x.kind == SectionKind::GaugeDrift).unwrap();
        assert_eq!(d.title, "Drift over Order");
        // Engine values on the same rows (checked against R in stat-quality).
        let y = t.column(t.find("Measurement").unwrap()).unwrap().as_numeric().unwrap();
        let (below, above): (Vec<f64>, Vec<f64>) =
            (y.iter().step_by(2).copied().collect(), y.iter().skip(1).step_by(2).copied().collect());
        let direct = stat_quality::hysteresis(&below, &above, 0.95).unwrap();
        assert_eq!(h.rows[2][1], format_number(direct.difference, DIGITS));
        let back = RepeatabilityRequest::from_json(&full.to_json(&t), &t).unwrap();
        assert_eq!(back, full);
        // A direction column with more than two values says so.
        let bad = RepeatabilityRequest { direction: t.find("Order"), ..full };
        let s = RepeatabilityAnalysis::run(&t, &states, &bad).unwrap().sections();
        let h = s.iter().find(|x| x.kind == SectionKind::GaugeHysteresis).unwrap();
        assert!(matches!(&h.content, SectionContent::Note(n) if n.contains("exactly two values")));
    }

    #[test]
    fn icc_report_on_the_gauge_sample() {
        let t = sample::gauge_study();
        let states = RowStateManager::new(t.n_rows());
        let req = IccRequest::new(t.find("Part").unwrap(), t.find("Operator").unwrap(), t.find("Measurement").unwrap());
        let a = IccAnalysis::run(&t, &states, &req).unwrap();
        // ρ agrees with the gauge R&R components.
        let g = crate::GaugeAnalysis::run(
            &t,
            &states,
            &crate::GaugeStudyRequest {
                part: req.part,
                operator: req.operator,
                measurement: req.measurement,
                tolerance: None,
                pool_interaction: false,
                study_multiplier: 6.0,
                design: crate::gauge_design::GaugeStudyDesign::default(),
            },
        )
        .unwrap();
        let c = g.study().unwrap().components;
        let rho = c.part.variance / (c.part.variance + c.gauge_rr.variance);
        assert!((a.intraclass_correlation() - rho).abs() < 1e-12);
        assert_eq!(a.shrout_fleiss().estimates.len(), 6);
        let s = a.sections();
        assert_eq!(s.len(), 4);
        let SectionContent::Table(by_source) = &s[1].content else { panic!() };
        assert_eq!(by_source.rows.len(), 2);
        let SectionContent::Table(forms) = &s[2].content else { panic!() };
        assert_eq!(forms.rows[1][0], "ICC(2,1)");
        assert_eq!(forms.rows[4][0], "ICC(2,3)");
        let back = IccRequest::from_json(&req.to_json(&t), &t).unwrap();
        assert_eq!(back, req);
        let mut states = states;
        states.set_excluded([0], true);
        assert!(IccAnalysis::run(&t, &states, &req).unwrap_err().to_string().contains("balanced"));
    }

    /// One measurement per cell: the Shrout & Fleiss Table 2 data laid out
    /// long; ρ = σ²_p/(σ²_p + σ²_e) uses the two-way ANOVA without
    /// interaction, which here equals ICC(2,1) = 0.2898 (Shrout & Fleiss 1979).
    #[test]
    fn single_measurement_study() {
        let data = [[9, 2, 5, 8], [6, 1, 3, 2], [8, 4, 6, 8], [7, 1, 2, 6], [10, 5, 6, 9], [6, 2, 4, 7]];
        let mut part = Vec::new();
        let mut oper = Vec::new();
        let mut y = Vec::new();
        for (i, r) in data.iter().enumerate() {
            for (j, &v) in r.iter().enumerate() {
                part.push(format!("T{i}"));
                oper.push(format!("J{j}"));
                y.push(f64::from(v));
            }
        }
        let t = DataTable::new(
            "sf",
            vec![Column::text("Part", part), Column::text("Operator", oper), Column::numeric("Y", y)],
        )
        .unwrap();
        let a = IccAnalysis::run(&t, &RowStateManager::new(24), &IccRequest::new(0, 1, 2)).unwrap();
        assert!((a.intraclass_correlation() - 0.289_763_779_527_559_05).abs() < 1e-12);
        assert_eq!(a.sources.len(), 1);
    }

    #[test]
    fn icc_rejects_a_study_whose_repeatability_source_cannot_be_evaluated() {
        // Every reading repeats within its cell (repeatability 0) and parts
        // differ less than the interaction (part variance truncated to 0):
        // the "repeatability only" source has no variation at all.
        let t = DataTable::new(
            "coarse",
            vec![
                Column::text("Part", ["P1", "P1", "P2", "P2", "P1", "P1", "P2", "P2"].map(String::from).to_vec()),
                Column::text("Operator", ["A", "A", "A", "A", "B", "B", "B", "B"].map(String::from).to_vec()),
                Column::numeric("Y", vec![1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 2.0, 2.0]),
            ],
        )
        .unwrap();
        if let Ok(a) = IccAnalysis::run(&t, &RowStateManager::new(8), &IccRequest::new(0, 1, 2)) {
            let _ = a.sections();
        }
    }
}
