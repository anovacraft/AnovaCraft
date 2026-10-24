//! The distribution of one column: summary statistics and plots.
//!
//! A column is analysed as **continuous** if it is numeric with the continuous
//! measurement level, and as **categorical** otherwise (text columns, or numeric
//! columns marked nominal/ordinal). Excluded rows and missing values are left
//! out; plots keep each bar's / point's dataset rows so brushing links them to
//! the data grid.
//!
//! For continuous data the report can add a **normal fit**: the maximum
//! likelihood mean and the usual sample standard deviation with confidence
//! intervals (`x̄ ± t s/√n`; `s √((n−1)/χ²)` for σ), the fitted density drawn over
//! the histogram (scaled to counts: `n · bin width · φ`), Shapiro–Wilk and
//! Anderson–Darling tests ([`stat_core::normality`]), and a normal quantile
//! plot with the line `x̄ + s·z`.
//!
//! Histogram bins: Sturges' rule `k = ⌈log₂ n⌉ + 1` bins (Sturges 1926,
//! *JASA* 21), with the width rounded up to a "nice" value (1, 2, 2.5 or 5 × 10ᵐ)
//! and the edges aligned to multiples of the width.

use crate::distribution_tests::{
    OutlierSettings, RunsSettings, ToleranceSettings, outlier_sections, runs_section, tolerance_limits,
    tolerance_section,
};
use crate::format::format_p_value;
use crate::format::{format_number, format_optional};
use crate::linear_analysis::normal_quantile_points;
use crate::report::{
    Annotations, BarChartSpec, BarItem, BoxPlotSpec, PlotSpec, Polyline, ReportSection, RowPoint, SectionContent,
    SectionKind, TextTable,
};
use crate::censored_distribution::{CensoredDistribution, Censoring};
use crate::report_columns::ReportColumn;
use stat_core::box_cox::{BoxCoxFit, box_cox_fit, box_cox_log_likelihood, box_cox_transform};
use stat_core::descriptive::{Moments, QuantileMethod, RobustSummary, moments, quantile_sorted, robust_summary};
use stat_core::distributions::{ChiSquared, ContinuousDistribution, Normal};
use stat_core::normality::{SHAPIRO_WILK_MAX_N, anderson_darling, shapiro_wilk};
use stat_core::{Result, StatError};

/// Confidence level of the Box–Cox interval.
const BOX_COX_LEVEL: f64 = 0.95;

/// Plain name of a common power.
fn power_name(lambda: f64) -> &'static str {
    const NAMES: [(f64, &str); 6] = [
        (1.0, "no transformation"),
        (0.0, "logarithm"),
        (0.5, "square root"),
        (-0.5, "reciprocal square root"),
        (-1.0, "reciprocal"),
        (2.0, "square"),
    ];
    NAMES.iter().find(|(l, _)| *l == lambda).map_or("power", |(_, n)| n)
}
use stat_data::{DataTable, MeasurementLevel, RowStateManager, TemporalDisplay, TemporalFormat};

const DIGITS: usize = 6;
/// Proportion trimmed from each tail for the trimmed and winsorized means.
pub const TRIM: f64 = 0.10;
/// Quantiles reported, as (probability, label).
const QUANTILES: [(f64, &str); 11] = [
    (1.0, "100% (maximum)"),
    (0.995, "99.5%"),
    (0.975, "97.5%"),
    (0.90, "90%"),
    (0.75, "75% (upper quartile)"),
    (0.50, "50% (median)"),
    (0.25, "25% (lower quartile)"),
    (0.10, "10%"),
    (0.025, "2.5%"),
    (0.005, "0.5%"),
    (0.0, "0% (minimum)"),
];

/// Which column, and how to compute quantiles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistributionRequest {
    /// Column index.
    pub column: usize,
    /// Quantile definition (default Hyndman–Fan type 8).
    pub quantile_method: QuantileMethod,
    /// Censored values and detection limits (numeric columns; `None`: every
    /// value is exact). See [`crate::censored_distribution`].
    pub censoring: Option<Censoring>,
}

impl DistributionRequest {
    /// The distribution of `column` with the default quantile definition and
    /// no censoring.
    #[must_use]
    pub fn new(column: usize) -> Self {
        Self { column, quantile_method: QuantileMethod::default(), censoring: None }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        std::iter::once(self.column).chain(self.censoring.map(|c| c.column())).collect()
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.column = f(self.column);
        self.censoring = self.censoring.map(|c| c.map_column(f));
    }
}

/// User-toggleable parts of a distribution report.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DistributionOptions {
    /// Quantile table (continuous).
    pub show_quantiles: bool,
    /// Robust estimates (continuous).
    pub show_robust: bool,
    /// Histogram (continuous) or bar chart (categorical).
    pub show_chart: bool,
    /// Box plot (continuous).
    pub show_box_plot: bool,
    /// Normal fit: parameter table, normality tests and the density over the
    /// histogram (continuous).
    pub show_normal_fit: bool,
    /// Normal quantile plot (continuous).
    pub show_quantile_plot: bool,
    /// Box–Cox transformation estimate and profile plot (positive continuous data).
    pub show_box_cox: bool,
    /// Tolerance interval, drawn on the histogram too (continuous; `None`: off).
    pub tolerance: Option<ToleranceSettings>,
    /// Outlier test (continuous; `None`: off).
    pub outliers: Option<OutlierSettings>,
    /// Runs tests for randomness in row order (continuous; `None`: off).
    pub runs: Option<RunsSettings>,
}

impl Default for DistributionOptions {
    fn default() -> Self {
        Self {
            show_quantiles: true,
            show_robust: true,
            show_chart: true,
            show_box_plot: true,
            show_normal_fit: true,
            show_quantile_plot: false,
            show_box_cox: false,
            tolerance: None,
            outliers: None,
            runs: None,
        }
    }
}

#[derive(Debug, Clone)]
enum Body {
    Continuous {
        /// (row, value) for every value used, in row order.
        data: Vec<(usize, f64)>,
        sorted: Vec<f64>,
        moments: Moments,
        robust: RobustSummary,
    },
    /// Values with censoring (left, right or interval).
    Censored(Box<CensoredDistribution>),
    Categorical {
        /// Levels in natural order (numbers by value, then text), with the
        /// rows at each level, so cumulative proportions follow the scale.
        levels: Vec<(String, Vec<usize>)>,
    },
}

/// The computed distribution of one column.
#[derive(Debug, Clone)]
pub struct DistributionAnalysis {
    request: DistributionRequest,
    name: String,
    body: Body,
    n_missing: usize,
    n_excluded: usize,
    exclusion_revision: u64,
    /// Rows in the table analysed.
    n_rows: usize,
    /// How a date-time column shows its values; `None` for numbers.
    time: Option<stat_data::TemporalDisplay>,
}

impl DistributionAnalysis {
    /// Analyse `request.column`, honouring row exclusion.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for an invalid column;
    /// [`StatError::InsufficientData`] if no values remain.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &DistributionRequest) -> Result<Self> {
        let col = table
            .column(request.column)
            .ok_or_else(|| StatError::InvalidArgument { message: format!("no column {}", request.column) })?;
        let (mut n_missing, mut n_excluded) = (0, 0);
        let mut keep = |r: usize, missing: bool| {
            if missing {
                n_missing += 1;
                false
            } else if states.is_excluded(r) {
                n_excluded += 1;
                false
            } else {
                true
            }
        };
        if let Some(censoring) = &request.censoring {
            let read = crate::censored_distribution::read_rows(table, states, request.column, censoring)?;
            let (n_missing, n_excluded) = (read.n_missing, read.n_excluded);
            let censored = CensoredDistribution::new(col.name.clone(), read)?;
            return Ok(Self {
                request: request.clone(),
                name: col.name.clone(),
                body: Body::Censored(Box::new(censored)),
                n_missing,
                n_excluded,
                exclusion_revision: states.exclusion_revision(),
                n_rows: table.n_rows(),
                time: None,
            });
        }
        let body = match (col.as_numeric(), col.level) {
            (Some(values), MeasurementLevel::Continuous) => {
                let data: Vec<(usize, f64)> =
                    (0..table.n_rows()).filter(|&r| keep(r, !values[r].is_finite())).map(|r| (r, values[r])).collect();
                let xs: Vec<f64> = data.iter().map(|d| d.1).collect();
                let moments = moments(&xs)?;
                let robust = robust_summary(&xs, TRIM, request.quantile_method)?;
                let mut sorted = xs;
                sorted.sort_by(f64::total_cmp);
                Body::Continuous { data, sorted, moments, robust }
            }
            _ => {
                let mut levels: Vec<(String, Vec<usize>)> = Vec::new();
                for r in 0..table.n_rows() {
                    let label = col.level_text(r);
                    if !keep(r, label.is_empty()) {
                        continue;
                    }
                    match levels.iter_mut().find(|(l, _)| *l == label) {
                        Some((_, rows)) => rows.push(r),
                        None => levels.push((label, vec![r])),
                    }
                }
                if levels.is_empty() {
                    return Err(StatError::InsufficientData { required: 1, found: 0 });
                }
                levels.sort_by(|a, b| stat_data::natural_order(&a.0, &b.0));
                Body::Categorical { levels }
            }
        };
        Ok(Self {
            request: request.clone(),
            name: col.name.clone(),
            body,
            n_missing,
            n_excluded,
            exclusion_revision: states.exclusion_revision(),
            n_rows: table.n_rows(),
            time: col.temporal(),
        })
    }

    /// `true` if row exclusion changed since the analysis ran.
    #[must_use]
    pub fn is_stale(&self, states: &RowStateManager) -> bool {
        states.exclusion_revision() != self.exclusion_revision
    }

    /// `true` if the column was analysed as continuous.
    #[must_use]
    pub fn is_continuous(&self) -> bool {
        matches!(self.body, Body::Continuous { .. } | Body::Censored(_))
    }

    /// The censored-data analysis, when the request has censoring.
    #[must_use]
    pub fn censored(&self) -> Option<&CensoredDistribution> {
        match &self.body {
            Body::Censored(c) => Some(c),
            _ => None,
        }
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Distribution: {}", self.name)
    }

    /// Report sections, honouring `options`.
    #[must_use]
    pub fn sections(&self, options: &DistributionOptions) -> Vec<ReportSection> {
        match &self.body {
            Body::Continuous { data, sorted, moments, robust } => {
                let mut out = vec![self.summary_table(moments)];
                if options.show_quantiles {
                    out.push(self.quantile_table(sorted));
                }
                if options.show_robust {
                    out.push(robust_table(robust, self.time));
                }
                if options.show_normal_fit {
                    out.push(normal_fit_table(moments, self.time));
                    out.push(normality_tests(sorted));
                }
                let text = |x: f64| location_text(self.time, x);
                let values: Vec<f64> = data.iter().map(|d| d.1).collect();
                if let Some(s) = &options.tolerance {
                    out.push(tolerance_section(&values, s, &text));
                }
                if let Some(s) = &options.outliers {
                    let rows: Vec<usize> = data.iter().map(|d| d.0).collect();
                    out.extend(outlier_sections(&values, &rows, s, &text).0);
                }
                if let Some(s) = &options.runs {
                    out.push(runs_section(&values, s, &text));
                }
                if options.show_chart {
                    let mut histogram = self.histogram(data, options.show_normal_fit.then_some(moments));
                    if let (Some(s), SectionContent::Bars(spec)) = (&options.tolerance, &mut histogram.content) {
                        if let Ok((lo, hi)) = tolerance_limits(&values, s) {
                            spec.references.extend(
                                [(lo, "Lower tolerance limit"), (hi, "Upper tolerance limit")]
                                    .into_iter()
                                    .filter(|(x, _)| x.is_finite())
                                    .map(|(x, l)| (x, l.to_owned())),
                            );
                        }
                    }
                    out.push(histogram);
                }
                if options.show_box_plot {
                    out.push(self.box_plot(data, sorted, moments.mean));
                }
                if options.show_quantile_plot {
                    out.push(self.quantile_plot(data, moments));
                }
                if options.show_box_cox {
                    out.extend(self.box_cox_sections(sorted));
                }
                out
            }
            Body::Censored(c) => c.sections(options.show_chart),
            Body::Categorical { levels } => {
                let mut out = vec![self.frequency_table(levels)];
                if options.show_chart {
                    out.push(self.frequency_chart(levels));
                }
                out
            }
        }
    }

    fn summary_table(&self, m: &Moments) -> ReportSection {
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut row = |a: &str, b: String| t.push([a.to_owned(), b]);
        row("N", m.n.to_string());
        if self.n_missing > 0 {
            row("Missing", self.n_missing.to_string());
        }
        if self.n_excluded > 0 {
            row("Excluded", self.n_excluded.to_string());
        }
        let (loc, span) = (|x| location_text(self.time, x), |x| spread_text(self.time, x));
        row("Mean", loc(m.mean));
        row("Standard deviation", opt(m.std_dev, span));
        row("Standard error of mean", opt(m.std_error, span));
        let ci = m.mean_confidence_interval(0.95).ok().flatten();
        row("Lower 95% CI for mean", opt(ci.map(|c| c.0), loc));
        row("Upper 95% CI for mean", opt(ci.map(|c| c.1), loc));
        row("Skewness (G₁)", format_optional(m.skewness, DIGITS));
        row("Excess kurtosis (G₂)", format_optional(m.excess_kurtosis, DIGITS));
        // A ratio to, or a sum of, instants depends on the arbitrary 1970
        // origin, so both are shown only for numbers and lengths of time.
        let has_origin = self.time.is_some_and(|d| d.format != TemporalFormat::Duration);
        let cv = if has_origin { "—".to_owned() } else { format_optional(m.coefficient_of_variation(), DIGITS) };
        row("Coefficient of variation (%)", cv);
        row("Minimum", loc(m.min));
        row("Maximum", loc(m.max));
        row("Sum", if has_origin { "—".to_owned() } else { loc(m.sum) });
        section(SectionKind::SummaryStatistics, "Summary Statistics", SectionContent::Table(t))
    }

    fn quantile_table(&self, sorted: &[f64]) -> ReportSection {
        let mut t = TextTable::new(["Quantile", "Value"]);
        for (p, label) in QUANTILES {
            let q = quantile_sorted(sorted, p, self.request.quantile_method).unwrap_or(f64::NAN);
            t.push([label.to_owned(), location_text(self.time, q)]);
        }
        let title = format!("Quantiles (Hyndman–Fan type {})", self.request.quantile_method.type_number());
        section(SectionKind::Quantiles, &title, SectionContent::Table(t))
    }

    fn histogram(&self, data: &[(usize, f64)], normal: Option<&Moments>) -> ReportSection {
        let bars = histogram_bars(data);
        let curve = normal.and_then(|m| normal_curve(m, &bars)).map(|c| ("Normal fit".to_owned(), c));
        let spec = BarChartSpec {
            x_time: self.time,
            x_label: self.name.clone(),
            y_label: "Count".to_owned(),
            bars,
            x_ticks: None,
            curve,
            references: Vec::new(),
        };
        section(SectionKind::Histogram, "Histogram", SectionContent::Bars(spec))
    }

    /// Dataset rows the outlier test in `options` declares outliers (empty
    /// when the test is off, finds none, or cannot run).
    #[must_use]
    pub fn flagged_rows(&self, options: &DistributionOptions) -> Vec<usize> {
        match (&self.body, &options.outliers) {
            (Body::Continuous { data, .. }, Some(s)) => {
                let values: Vec<f64> = data.iter().map(|d| d.1).collect();
                let rows: Vec<usize> = data.iter().map(|d| d.0).collect();
                outlier_sections(&values, &rows, s, &|x| location_text(self.time, x)).1
            }
            _ => Vec::new(),
        }
    }

    /// Name of the column analysed.
    #[must_use]
    pub fn column_name(&self) -> &str {
        &self.name
    }

    /// Box–Cox fit of the values used (`None` for categorical data, or data
    /// that are not all positive).
    #[must_use]
    pub fn box_cox(&self) -> Option<BoxCoxFit> {
        match &self.body {
            Body::Continuous { sorted, .. } => box_cox_fit(sorted, BOX_COX_LEVEL).ok(),
            Body::Categorical { .. } | Body::Censored(_) => None,
        }
    }

    /// Columns a report can add to the table: the Box–Cox transformed
    /// column at the convenient λ (positive continuous data only).
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        let Some(fit) = self.box_cox() else { return Vec::new() };
        let lambda = fit.convenient_lambda().unwrap_or(fit.lambda);
        let shown = crate::format::format_number(lambda, 3);
        self.box_cox_column(lambda)
            .map(|values| {
                ReportColumn::continuous(
                    "box_cox",
                    &format!("Save Box–Cox transformed column (λ = {shown})"),
                    format!("{} (Box–Cox λ = {shown})", self.column_name()),
                    values,
                )
            })
            .into_iter()
            .collect()
    }

    /// A new column holding the Box–Cox transform at `lambda` of every value
    /// used (NaN for other rows), one value per table row.
    #[must_use]
    pub fn box_cox_column(&self, lambda: f64) -> Option<Vec<f64>> {
        let n_rows = self.n_rows;
        let Body::Continuous { data, .. } = &self.body else { return None };
        let values: Vec<f64> = data.iter().map(|d| d.1).collect();
        let transformed = box_cox_transform(&values, lambda).ok()?;
        let mut out = vec![f64::NAN; n_rows];
        for (&(row, _), t) in data.iter().zip(transformed) {
            if row < n_rows {
                out[row] = t;
            }
        }
        Some(out)
    }

    fn box_cox_sections(&self, sorted: &[f64]) -> Vec<ReportSection> {
        let fit = match box_cox_fit(sorted, BOX_COX_LEVEL) {
            Ok(f) => f,
            Err(e) => {
                return vec![section(
                    SectionKind::BoxCox,
                    "Box–Cox Transformation",
                    SectionContent::Note(format!("Not available: {e}. (Shift the data to be positive first.)")),
                )];
            }
        };
        let interval = |v: Option<f64>, beyond: &str| v.map_or_else(|| beyond.to_owned(), |x| format_number(x, 4));
        let at_one = box_cox_log_likelihood(sorted, 1.0).unwrap_or(f64::NAN);
        let lr = 2.0 * (fit.log_likelihood - at_one).max(0.0);
        let p = ChiSquared::new(1.0).and_then(|c| c.sf(lr)).unwrap_or(f64::NAN);
        let mut t = TextTable::new(["Statistic", "Value"]);
        t.push(["λ̂ (maximum likelihood)".to_owned(), format_number(fit.lambda, 4)]);
        t.push([
            format!("{:.0}% confidence interval for λ", BOX_COX_LEVEL * 100.0),
            format!("{} to {}", interval(fit.interval.0, "below −5"), interval(fit.interval.1, "above 5")),
        ]);
        let suggestion = fit.convenient_lambda().map_or_else(
            || "none of the usual powers fits; use λ̂".to_owned(),
            |l| format!("{} ({})", format_number(l, 2), power_name(l)),
        );
        t.push(["Suggested power".to_owned(), suggestion]);
        t.push(["Test of λ = 1 (no transformation): χ²₁".to_owned(), format_number(lr, 4)]);
        t.push(["p-value".to_owned(), format_p_value(p)]);

        // Profile plot over a range covering the interval.
        let lo = fit.interval.0.unwrap_or(-5.0).clamp(-5.0, -2.0);
        let hi = fit.interval.1.unwrap_or(5.0).clamp(2.0, 5.0);
        let curve: Vec<[f64; 2]> = (0..=120)
            .map(|i| {
                let l = lo + (hi - lo) * f64::from(i) / 120.0;
                [l, box_cox_log_likelihood(sorted, l).unwrap_or(f64::NAN)]
            })
            .filter(|p| p[1].is_finite())
            .collect();
        let cut =
            fit.log_likelihood - 0.5 * ChiSquared::new(1.0).and_then(|c| c.quantile(BOX_COX_LEVEL)).unwrap_or(f64::NAN);
        let spec = PlotSpec {
            x_time: None,
            y_time: None,
            x_label: "λ".to_owned(),
            y_label: "Profile log-likelihood".to_owned(),
            points: Vec::new(),
            line: Some(curve),
            line_name: Some("Profile log-likelihood".to_owned()),
            band: None,
            groups: None,
            x_ticks: None,
            panes: Vec::new(),
            annotations: Some(Annotations {
                groups: vec!["Maximum".to_owned()],
                items: vec![(fit.lambda, fit.log_likelihood, format!("λ̂ = {}", format_number(fit.lambda, 3)), 0)],
                labels: vec![(hi, cut, format!("{:.0}% limit", BOX_COX_LEVEL * 100.0))],
            }),
            segments: vec![vec![[lo, cut], [hi, cut]]],
            ..PlotSpec::default()
        };
        vec![
            section(SectionKind::BoxCox, "Box–Cox Transformation", SectionContent::Table(t)),
            section(
                SectionKind::BoxCoxProfile,
                "Box–Cox Profile Likelihood (λ values above the limit line form the interval)",
                SectionContent::Plot(Box::new(spec)),
            ),
        ]
    }

    fn quantile_plot(&self, data: &[(usize, f64)], m: &Moments) -> ReportSection {
        let mut sorted = data.to_vec();
        sorted.sort_by(|a, b| a.1.total_cmp(&b.1));
        let points = normal_quantile_points(&sorted);
        let line = m.std_dev.and_then(|s| {
            let (lo, hi) = (points.first()?.x, points.last()?.x);
            Some(vec![[lo, m.mean + s * lo], [hi, m.mean + s * hi]])
        });
        let spec = PlotSpec {
            x_time: None,
            y_time: self.time,
            x_label: "Normal quantile".to_owned(),
            y_label: self.name.clone(),
            points,
            line,
            line_name: Some("Normal reference line".to_owned()),
            band: None,
            groups: None,
            x_ticks: None,
            panes: Vec::new(),
            annotations: None,
            segments: Vec::new(),
            ..PlotSpec::default()
        };
        section(SectionKind::NormalQuantilePlot, "Normal Quantile Plot", SectionContent::Plot(Box::new(spec)))
    }

    fn box_plot(&self, data: &[(usize, f64)], sorted: &[f64], mean: f64) -> ReportSection {
        let q = |p| quantile_sorted(sorted, p, self.request.quantile_method).unwrap_or(f64::NAN);
        let (q1, median, q3) = (q(0.25), q(0.5), q(0.75));
        let iqr = q3 - q1;
        let (lo_fence, hi_fence) = (q1 - 1.5 * iqr, q3 + 1.5 * iqr);
        let inside = sorted.iter().copied().filter(|v| (lo_fence..=hi_fence).contains(v));
        let (whisker_low, whisker_high) =
            inside.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)));
        let points: Vec<RowPoint> = data.iter().map(|&(row, x)| RowPoint { row, x, y: 0.0 }).collect();
        let outliers = (0..points.len()).filter(|&i| !(lo_fence..=hi_fence).contains(&points[i].x)).collect();
        let spec = BoxPlotSpec {
            x_time: self.time,
            x_label: self.name.clone(),
            q1,
            median,
            q3,
            whisker_low: if whisker_low.is_finite() { whisker_low } else { q1 },
            whisker_high: if whisker_high.is_finite() { whisker_high } else { q3 },
            mean,
            points,
            outliers,
        };
        section(SectionKind::BoxPlot, "Box Plot", SectionContent::BoxPlot(spec))
    }

    fn frequency_table(&self, levels: &[(String, Vec<usize>)]) -> ReportSection {
        let total: usize = levels.iter().map(|l| l.1.len()).sum();
        let mut t = TextTable::new(["Level", "Count", "Proportion", "Cumulative"]);
        let mut cumulative = 0;
        for (label, rows) in levels {
            cumulative += rows.len();
            t.push([
                label.clone(),
                rows.len().to_string(),
                format!("{:.4}", rows.len() as f64 / total as f64),
                format!("{:.4}", cumulative as f64 / total as f64),
            ]);
        }
        t.push(["Total".to_owned(), total.to_string(), "1.0000".to_owned(), String::new()]);
        if self.n_missing > 0 {
            t.push(["(missing)".to_owned(), self.n_missing.to_string(), String::new(), String::new()]);
        }
        section(SectionKind::Frequencies, "Frequencies", SectionContent::Table(t))
    }

    fn frequency_chart(&self, levels: &[(String, Vec<usize>)]) -> ReportSection {
        let bars = levels
            .iter()
            .enumerate()
            .map(|(i, (_, rows))| BarItem { center: (i + 1) as f64, width: 0.7, rows: rows.clone(), unlinked: 0 })
            .collect();
        let ticks = levels.iter().enumerate().map(|(i, (l, _))| ((i + 1) as f64, l.clone())).collect();
        let spec = BarChartSpec {
            x_time: None,
            x_label: self.name.clone(),
            y_label: "Count".to_owned(),
            bars,
            x_ticks: Some(ticks),
            curve: None,
            references: Vec::new(),
        };
        section(SectionKind::FrequencyChart, "Frequency Chart", SectionContent::Bars(spec))
    }
}

/// Fitted normal parameters with 95% confidence intervals.
fn normal_fit_table(m: &Moments, time: Option<TemporalDisplay>) -> ReportSection {
    let (loc, span) = (|x| location_text(time, x), |x| spread_text(time, x));
    let mut t = TextTable::new(["Parameter", "Estimate", "Lower 95%", "Upper 95%"]);
    let ci = m.mean_confidence_interval(0.95).ok().flatten();
    t.push(["Mean (μ)".to_owned(), loc(m.mean), opt(ci.map(|c| c.0), loc), opt(ci.map(|c| c.1), loc)]);
    let sigma_ci = m.std_dev.and_then(|s| {
        let df = (m.n - 1) as f64;
        let chi = ChiSquared::new(df).ok()?;
        let (lo, hi) = (chi.quantile(0.025).ok()?, chi.quantile(0.975).ok()?);
        Some((s * (df / hi).sqrt(), s * (df / lo).sqrt()))
    });
    t.push([
        "Standard deviation (σ)".to_owned(),
        opt(m.std_dev, span),
        opt(sigma_ci.map(|c| c.0), span),
        opt(sigma_ci.map(|c| c.1), span),
    ]);
    section(SectionKind::NormalFit, "Normal Fit", SectionContent::Table(t))
}

/// Shapiro–Wilk and Anderson–Darling tests, with the reason when one cannot run.
fn normality_tests(sorted: &[f64]) -> ReportSection {
    let mut t = TextTable::new(["Test", "Statistic", "p-value"]);
    let mut add = |name: &str, result: stat_core::Result<stat_core::normality::NormalityTest>| match result {
        Ok(r) => t.push([name.to_owned(), format_number(r.statistic, DIGITS), format_p_value(r.p_value)]),
        Err(e) => t.push([name.to_owned(), "—".to_owned(), e.to_string()]),
    };
    if sorted.len() > SHAPIRO_WILK_MAX_N {
        add(
            "Shapiro–Wilk W",
            Err(StatError::InvalidArgument { message: format!("not computed for n > {SHAPIRO_WILK_MAX_N}") }),
        );
    } else {
        add("Shapiro–Wilk W", shapiro_wilk(sorted));
    }
    add("Anderson–Darling A²", anderson_darling(sorted));
    section(
        SectionKind::NormalityTests,
        "Tests of Normality (small p-values indicate non-normal data)",
        SectionContent::Table(t),
    )
}

/// The fitted normal density scaled to histogram counts, across the bars and
/// a little beyond.
fn normal_curve(m: &Moments, bars: &[BarItem]) -> Option<Polyline> {
    let s = m.std_dev.filter(|s| *s > 0.0)?;
    let width = bars.first()?.width;
    let lo = (bars.first()?.center - width / 2.0).min(m.mean - 3.0 * s);
    let hi = (bars.last()?.center + width / 2.0).max(m.mean + 3.0 * s);
    let normal = Normal::new(m.mean, s).ok()?;
    let scale = m.n as f64 * width;
    const POINTS: usize = 120;
    Some(
        (0..=POINTS)
            .map(|i| {
                let x = lo + (hi - lo) * i as f64 / POINTS as f64;
                [x, scale * normal.pdf(x)]
            })
            .collect(),
    )
}

fn robust_table(r: &RobustSummary, time: Option<TemporalDisplay>) -> ReportSection {
    let pct = (r.trim * 100.0).round();
    let mut t = TextTable::new(["Estimate", "Value"]);
    let mut row = |a: String, b: String| t.push([a, b]);
    let (loc, span) = (|x| location_text(time, x), |x| spread_text(time, x));
    row("Median".to_owned(), loc(r.median));
    row("Median absolute deviation (MAD)".to_owned(), span(r.mad));
    row("MAD scaled to σ (× 1.4826)".to_owned(), span(r.mad_scaled));
    row("Interquartile range".to_owned(), span(r.iqr));
    row(format!("{pct}% trimmed mean"), loc(r.trimmed_mean));
    row(format!("{pct}% winsorized mean"), loc(r.winsorized_mean));
    section(SectionKind::RobustEstimates, "Robust Estimates", SectionContent::Table(t))
}

/// A location statistic (mean, quantile, bound) as a date or time for a
/// date-time column, otherwise as a number.
fn location_text(time: Option<TemporalDisplay>, x: f64) -> String {
    match time {
        Some(d) if x.is_finite() => d.text(x),
        _ => format_number(x, DIGITS),
    }
}

/// A spread statistic (standard deviation, MAD, range) as a length of time
/// for a date-time column, otherwise as a number.
fn spread_text(time: Option<TemporalDisplay>, x: f64) -> String {
    match time {
        Some(d) if x.is_finite() => d.span_text(x),
        _ => format_number(x, DIGITS),
    }
}

/// An optional statistic through `text`, or "—" when it is not defined.
fn opt(x: Option<f64>, text: impl Fn(f64) -> String) -> String {
    x.map_or_else(|| format_optional(None, DIGITS), text)
}

/// Sturges-rule histogram with a "nice" bin width; each bar lists its rows.
#[must_use]
pub fn histogram_bars(data: &[(usize, f64)]) -> Vec<BarItem> {
    let (lo, hi) = data.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), d| (a.min(d.1), b.max(d.1)));
    if !(lo.is_finite() && hi.is_finite()) {
        return Vec::new();
    }
    if hi == lo {
        return vec![BarItem { center: lo, width: 1.0, rows: data.iter().map(|d| d.0).collect(), unlinked: 0 }];
    }
    let k = (data.len() as f64).log2().ceil() + 1.0;
    let width = nice_width((hi - lo) / k);
    let start = (lo / width).floor() * width;
    let n_bins = (((hi - start) / width).floor() as usize + 1).max(1);
    let mut bars: Vec<BarItem> = (0..n_bins)
        .map(|i| BarItem { center: start + (i as f64 + 0.5) * width, width, rows: Vec::new(), unlinked: 0 })
        .collect();
    for &(row, x) in data {
        let i = (((x - start) / width).floor() as usize).min(n_bins - 1);
        bars[i].rows.push(row);
    }
    bars
}

/// Smallest of 1, 2, 2.5, 5 × 10ᵐ that is at least `raw`.
fn nice_width(raw: f64) -> f64 {
    let magnitude = 10f64.powf(raw.log10().floor());
    [1.0, 2.0, 2.5, 5.0, 10.0].iter().map(|m| m * magnitude).find(|&w| w >= raw * (1.0 - 1e-12)).unwrap_or(raw)
}

fn section(kind: SectionKind, title: &str, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.to_owned(), content }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn table() -> DataTable {
        DataTable::new(
            "t",
            vec![
                Column::numeric("x", vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 100.0, f64::NAN]),
                Column::text(
                    "g",
                    ["a", "b", "a", "c", "a", "b", "", "a", "c", "b", "a"].iter().map(|s| (*s).to_owned()).collect(),
                ),
            ],
        )
        .unwrap()
    }

    fn req(column: usize) -> DistributionRequest {
        DistributionRequest::new(column)
    }

    #[test]
    fn continuous_report() {
        let t = table();
        let a = DistributionAnalysis::run(&t, &RowStateManager::new(11), &req(0)).unwrap();
        assert!(a.is_continuous());
        let s = a.sections(&DistributionOptions::default());
        let kinds: Vec<_> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::SummaryStatistics,
                SectionKind::Quantiles,
                SectionKind::RobustEstimates,
                SectionKind::NormalFit,
                SectionKind::NormalityTests,
                SectionKind::Histogram,
                SectionKind::BoxPlot
            ]
        );
        let SectionContent::Table(summary) = &s[0].content else { panic!() };
        assert_eq!(summary.cell(0, "Value"), Some("10"));
        assert_eq!(summary.cell(1, "Value"), Some("1"), "one missing value");
        assert_eq!(summary.cell(2, "Value"), Some("14.5"));
        let SectionContent::Table(q) = &s[1].content else { panic!() };
        assert!(s[1].title.contains("type 8"));
        assert_eq!(q.cell(0, "Value"), Some("100"));
        assert_eq!(q.cell(5, "Value"), Some("5.5"));
        // The 100 is far beyond the upper fence.
        let SectionContent::BoxPlot(b) = &s[6].content else { panic!() };
        assert_eq!(b.outliers.len(), 1);
        assert_eq!(b.points[b.outliers[0]].row, 9);
        assert_eq!(b.whisker_high, 9.0);
        assert_eq!(b.whisker_low, 1.0);
    }

    #[test]
    fn normal_fit_tests_curve_and_quantile_plot() {
        let t = table();
        let a = DistributionAnalysis::run(&t, &RowStateManager::new(11), &req(0)).unwrap();
        let opts = DistributionOptions { show_quantile_plot: true, ..DistributionOptions::default() };
        let s = a.sections(&opts);
        let SectionContent::Table(fit) = &s[3].content else { panic!() };
        assert_eq!(fit.cell(0, "Estimate"), Some("14.5"));
        let SectionContent::Table(tests) = &s[4].content else { panic!() };
        assert_eq!(tests.cell(0, "Test"), Some("Shapiro–Wilk W"));
        // The 100 makes these data clearly non-normal.
        let w = shapiro_wilk(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 100.0]).unwrap();
        assert!(w.p_value < 1e-4);
        assert_eq!(tests.cell(0, "p-value").unwrap(), format_p_value(w.p_value));
        // The density curve integrates to about n over the bars (it is scaled to counts).
        let SectionContent::Bars(h) = &s[5].content else { panic!() };
        let (_, curve) = h.curve.as_ref().unwrap();
        let area: f64 = curve.windows(2).map(|w| (w[1][0] - w[0][0]) * (w[0][1] + w[1][1]) / 2.0).sum();
        assert!((area / h.bars[0].width - 10.0).abs() < 0.1, "area {area}");
        let SectionContent::Plot(qq) = &s[7].content else { panic!() };
        assert_eq!(qq.points.len(), 10);
        assert_eq!(qq.points.last().unwrap().row, 9);
        // Without the normal fit, no curve and no fit sections.
        let plain = a.sections(&DistributionOptions { show_normal_fit: false, ..DistributionOptions::default() });
        assert!(plain.iter().all(|x| x.kind != SectionKind::NormalFit));
        let SectionContent::Bars(h) = &plain[3].content else { panic!() };
        assert!(h.curve.is_none());
    }

    #[test]
    fn date_columns_report_dates_and_lengths_of_time() {
        use stat_data::temporal::seconds_from_civil;
        let day = |d| seconds_from_civil(2026, 10, d, 0, 0, 0.0);
        let display = TemporalDisplay::new(TemporalFormat::Date);
        let t = DataTable::new("t", vec![Column::date_time("when", vec![day(11), day(12), day(13)], display)]).unwrap();
        let a = DistributionAnalysis::run(&t, &RowStateManager::new(3), &req(0)).unwrap();
        assert!(a.is_continuous());
        let opts = DistributionOptions { show_robust: true, ..DistributionOptions::default() };
        let s = a.sections(&opts);
        let SectionContent::Table(sum) = &s[0].content else { panic!() };
        let value = |name: &str| (0..).map_while(|r| sum.cell(r, "Statistic").map(|n| (r, n))).find(|x| x.1 == name);
        let cell = |name: &str| sum.cell(value(name).unwrap().0, "Value").unwrap().to_owned();
        assert_eq!(cell("Mean"), "2026-10-12");
        assert_eq!(cell("Minimum"), "2026-10-11");
        assert_eq!(cell("Standard deviation"), display.span_text(86_400.0));
        // Sums and ratios of instants depend on the 1970 origin.
        assert_eq!(cell("Sum"), "—");
        assert_eq!(cell("Coefficient of variation (%)"), "—");
        let SectionContent::Table(q) = &s[1].content else { panic!() };
        assert_eq!(q.cell(0, "Value"), Some("2026-10-13"));
        let robust = s.iter().find(|x| x.kind == SectionKind::RobustEstimates).unwrap();
        let SectionContent::Table(r) = &robust.content else { panic!() };
        assert_eq!(r.cell(0, "Value"), Some("2026-10-12"));
        assert_eq!(r.cell(1, "Value").unwrap(), display.span_text(86_400.0));
        let hist = s.iter().find(|x| x.kind == SectionKind::Histogram).unwrap();
        let SectionContent::Bars(h) = &hist.content else { panic!() };
        assert_eq!(h.x_time, Some(display));
    }

    #[test]
    fn histogram_bins_cover_every_value_once() {
        let data: Vec<(usize, f64)> = (0..37).map(|i| (i, f64::from(i as u32) * 0.37 - 3.0)).collect();
        let bars = histogram_bars(&data);
        let mut rows: Vec<usize> = bars.iter().flat_map(|b| b.rows.clone()).collect();
        rows.sort_unstable();
        assert_eq!(rows, (0..37).collect::<Vec<_>>());
        let w = bars[0].width;
        assert!(bars.iter().all(|b| (b.width - w).abs() < 1e-12));
        // Edges are multiples of the width, and every value lies inside its bar.
        assert!(((bars[0].center - w / 2.0) / w - ((bars[0].center - w / 2.0) / w).round()).abs() < 1e-9);
        for b in &bars {
            for &r in &b.rows {
                let x = data[r].1;
                assert!(x >= b.center - w / 2.0 - 1e-9 && x <= b.center + w / 2.0 + 1e-9);
            }
        }
        assert_eq!(nice_width(0.37), 0.5);
        assert_eq!(nice_width(2.2), 2.5);
        assert_eq!(nice_width(7.0), 10.0);
        assert_eq!(histogram_bars(&[(0, 3.0), (1, 3.0)]).len(), 1, "constant data: one bar");
    }

    /// An ordinal score listed in data order (5, 1, 3, …) made the
    /// cumulative column meaningless; levels follow the scale.
    #[test]
    fn levels_are_listed_in_natural_order() {
        let t = DataTable::new(
            "t",
            vec![stat_data::Column::text("Score", ["5", "1", "3", "10", "2", "4", "1"].map(str::to_owned).to_vec())],
        )
        .unwrap();
        let a = DistributionAnalysis::run(&t, &RowStateManager::new(7), &req(0)).unwrap();
        let s = a.sections(&DistributionOptions::default());
        let SectionContent::Table(f) = &s[0].content else { panic!() };
        let levels: Vec<&str> = (0..6).map(|r| f.cell(r, "Level").unwrap()).collect();
        assert_eq!(levels, ["1", "2", "3", "4", "5", "10"]);
        assert_eq!(f.cell(0, "Count"), Some("2"));
    }

    #[test]
    fn categorical_report_counts_levels_and_missing() {
        let t = table();
        let a = DistributionAnalysis::run(&t, &RowStateManager::new(11), &req(1)).unwrap();
        assert!(!a.is_continuous());
        let s = a.sections(&DistributionOptions::default());
        let SectionContent::Table(f) = &s[0].content else { panic!() };
        assert_eq!(f.cell(0, "Level"), Some("a"));
        assert_eq!(f.cell(0, "Count"), Some("5"));
        assert_eq!(f.cell(1, "Count"), Some("3"));
        assert_eq!(f.cell(2, "Cumulative"), Some("1.0000"));
        assert_eq!(f.cell(3, "Count"), Some("10"), "total");
        assert_eq!(f.cell(4, "Count"), Some("1"), "missing");
        let SectionContent::Bars(chart) = &s[1].content else { panic!() };
        assert_eq!(chart.bars.iter().map(BarItem::count).collect::<Vec<_>>(), vec![5, 3, 2]);
        assert_eq!(chart.x_ticks.as_ref().unwrap()[2], (3.0, "c".to_owned()));
        assert_eq!(chart.bars[2].rows, vec![3, 8]);
    }

    #[test]
    fn exclusion_is_honoured() {
        let t = table();
        let mut states = RowStateManager::new(11);
        let a = DistributionAnalysis::run(&t, &states, &req(0)).unwrap();
        states.set_excluded([9], true);
        assert!(a.is_stale(&states));
        let b = DistributionAnalysis::run(&t, &states, &req(0)).unwrap();
        let s = b.sections(&DistributionOptions::default());
        let SectionContent::Table(summary) = &s[0].content else { panic!() };
        assert_eq!(summary.cell(0, "Value"), Some("9"));
        assert_eq!(summary.cell(3, "Value"), Some("5"), "mean of 1..9 after excluding 100");
    }

    #[test]
    fn box_cox_sections_and_saved_column() {
        // Right-skewed positive data (the Box–Cox test set in stat-core).
        let v = [
            0.15, 0.09, 0.18, 0.10, 0.05, 0.12, 0.08, 0.05, 0.08, 0.10, 0.07, 0.02, 0.01, 0.10, 0.10, 0.10, 0.02, 0.10,
            0.01, 0.40, 0.10, 0.05, 0.03, 0.05, 0.15, 0.10, 0.15, 0.09, 0.08, 0.18, 0.10, 0.20, 0.11, 0.30, 0.02, 0.20,
            0.20, 0.30, 0.30, 0.40, 0.30, 0.05,
        ];
        let mut values = v.to_vec();
        values.push(f64::NAN);
        let table = DataTable::new("t", vec![stat_data::Column::numeric("y", values)]).unwrap();
        let rows = RowStateManager::new(table.n_rows());
        let request = DistributionRequest::new(0);
        let a = DistributionAnalysis::run(&table, &rows, &request).unwrap();
        let opts = DistributionOptions { show_box_cox: true, ..DistributionOptions::default() };
        let s = a.sections(&opts);
        let bc = s.iter().find(|s| s.kind == SectionKind::BoxCox).unwrap();
        let SectionContent::Table(t) = &bc.content else { panic!() };
        assert_eq!(t.cell(0, "Value"), Some("0.2759"));
        assert_eq!(t.cell(2, "Value"), Some("0.5 (square root)"));
        assert!(s.iter().any(|s| s.kind == SectionKind::BoxCoxProfile));
        assert!((a.box_cox().unwrap().lambda - 0.275_937_3).abs() < 1e-6);
        let col = a.box_cox_column(0.5).unwrap();
        assert!((col[0] - (0.15_f64.sqrt() - 1.0) / 0.5).abs() < 1e-12);
        assert!(col[42].is_nan(), "row without a value");
        // Non-positive data give a note instead.
        let t2 = DataTable::new("t", vec![stat_data::Column::numeric("y", vec![1.0, -2.0, 3.0])]).unwrap();
        let b = DistributionAnalysis::run(&t2, &RowStateManager::new(3), &request).unwrap();
        let s2 = b.sections(&opts);
        assert!(matches!(s2.iter().find(|s| s.kind == SectionKind::BoxCox).unwrap().content, SectionContent::Note(_)));
        assert!(b.box_cox().is_none());
    }
}
