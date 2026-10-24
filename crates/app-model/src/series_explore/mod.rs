//! Series and Signals ▸ Explore: describing a series and its dependence, splitting
//! it into trend and seasonal components, trend-curve fits with forecasts,
//! and the cross-correlation of two series ([`stat_timeseries`]).
//!
//! Each report analyses one series (a numeric column taken in table order,
//! see [`crate::time_series_data`]); a dialog given several columns opens one
//! report per column.
//!
//! * **Series description** ([`DescribeSettings`]): time series plot, an
//!   optional Box–Cox transformation and regular or seasonal differencing,
//!   ACF and PACF with confidence limits, Ljung–Box tests, augmented
//!   Dickey–Fuller and KPSS tests, and (optionally) the periodogram and
//!   smoothed spectral density.
//! * **Decomposition** ([`DecomposeSettings`]): classical (additive or
//!   multiplicative moving-average) or STL decomposition into trend,
//!   seasonal and remainder, with seasonal indices and the strengths of
//!   trend and seasonality.
//! * **Trend models** ([`TrendSettings`]): linear, quadratic, exponential
//!   growth or logistic S-curve against time, or single moving-average
//!   forecasts, with accuracy measures and forecasts.
//! * **Cross-correlation** ([`CrossSettings`]): correlations of an input
//!   series with an output series at leads and lags, optionally after
//!   prewhitening the input.

mod cross;
mod decompose;
mod describe;
mod trend;

use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, RowStateManager};
pub use stat_timeseries::unit_root::AdfDeterministic;

use crate::format::format_number;
use crate::report::{Annotations, PlotSpec, Polyline, ReportSection, SectionContent, SectionKind};
use crate::report_columns::ReportColumn;
use crate::time_series_data::{Series, SeriesInput, json_column};

pub(crate) fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

pub(crate) fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

pub(crate) fn num(v: f64) -> String {
    format_number(v, 5)
}

pub(crate) fn percent(level: f64) -> String {
    format!("{}%", format_number(level * 100.0, 4))
}

/// A Box–Cox transformation applied before the analysis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BoxCoxChoice {
    /// No transformation.
    None,
    /// `(y^λ − 1)/λ` (`ln y` at λ = 0) with this λ.
    Lambda(f64),
    /// λ chosen by Guerrero's method in [−1, 2].
    Guerrero,
}

/// Settings of a series description.
#[derive(Debug, Clone, PartialEq)]
pub struct DescribeSettings {
    /// Transformation applied first.
    pub box_cox: BoxCoxChoice,
    /// Regular differences `d`.
    pub differences: usize,
    /// Seasonal differences `D` (at the series' period).
    pub seasonal_differences: usize,
    /// Largest lag of the ACF and PACF (`None`: `10 log₁₀ n`, and at least
    /// two seasonal cycles).
    pub max_lag: Option<usize>,
    /// Bartlett limits (for a moving average up to the previous lag) on the
    /// ACF instead of white-noise limits.
    pub bartlett: bool,
    /// Confidence level of limits and intervals.
    pub confidence: f64,
    /// Deterministic terms of the augmented Dickey–Fuller regression.
    pub adf: AdfDeterministic,
    /// Show the unit-root and stationarity tests.
    pub unit_root: bool,
    /// Show the periodogram and smoothed spectral density.
    pub spectrum: bool,
    /// Spans of the modified Daniell kernels smoothing the periodogram.
    pub spans: Vec<usize>,
    /// Plot the spectrum against the period (observations per cycle)
    /// instead of the frequency.
    pub period_axis: bool,
}

impl Default for DescribeSettings {
    fn default() -> Self {
        Self {
            box_cox: BoxCoxChoice::None,
            differences: 0,
            seasonal_differences: 0,
            max_lag: None,
            bartlett: false,
            confidence: 0.95,
            adf: AdfDeterministic::Constant,
            unit_root: true,
            spectrum: false,
            spans: vec![3, 3],
            period_axis: false,
        }
    }
}

/// The decomposition method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecomposeMethod {
    /// Classical moving-average decomposition, `y = T + S + R`.
    ClassicalAdditive,
    /// Classical moving-average decomposition, `y = T · S · R`.
    ClassicalMultiplicative,
    /// STL (seasonal-trend decomposition by LOESS), additive.
    Stl,
}

impl DecomposeMethod {
    /// Every method, in dialog order.
    pub const ALL: [Self; 3] = [Self::ClassicalAdditive, Self::ClassicalMultiplicative, Self::Stl];

    /// Label for dialogs and reports.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::ClassicalAdditive => "Classical, additive",
            Self::ClassicalMultiplicative => "Classical, multiplicative",
            Self::Stl => "STL (LOESS)",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::ClassicalAdditive => "classical-additive",
            Self::ClassicalMultiplicative => "classical-multiplicative",
            Self::Stl => "stl",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }
}

/// Settings of a decomposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecomposeSettings {
    /// The method.
    pub method: DecomposeMethod,
    /// STL seasonal smoothing span (odd, at least 7), or `None` for a
    /// periodic seasonal (every cycle the same).
    pub seasonal_window: Option<usize>,
    /// STL robustness iterations (downweight outliers).
    pub robust: bool,
}

impl Default for DecomposeSettings {
    fn default() -> Self {
        Self { method: DecomposeMethod::ClassicalMultiplicative, seasonal_window: None, robust: false }
    }
}

/// The trend curve or forecasting rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrendChoice {
    /// `y = β₀ + β₁ t`.
    Linear,
    /// `y = β₀ + β₁ t + β₂ t²`.
    Quadratic,
    /// `y = b₀ · b₁ᵗ`.
    Exponential,
    /// `y = c / (1 + a e^{−bt})`.
    Logistic,
    /// Mean of the last `span` observations.
    MovingAverage,
}

impl TrendChoice {
    /// Every choice, in dialog order.
    pub const ALL: [Self; 5] = [Self::Linear, Self::Quadratic, Self::Exponential, Self::Logistic, Self::MovingAverage];

    /// Label for dialogs and reports.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::Quadratic => "Quadratic",
            Self::Exponential => "Exponential growth",
            Self::Logistic => "Logistic S-curve",
            Self::MovingAverage => "Moving average",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::Quadratic => "quadratic",
            Self::Exponential => "exponential",
            Self::Logistic => "logistic",
            Self::MovingAverage => "moving-average",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }
}

/// Settings of a trend model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrendSettings {
    /// The curve.
    pub model: TrendChoice,
    /// Span of the moving average.
    pub span: usize,
    /// Periods forecast.
    pub horizon: usize,
    /// Coverage of the prediction limits.
    pub confidence: f64,
}

impl Default for TrendSettings {
    fn default() -> Self {
        Self { model: TrendChoice::Linear, span: 3, horizon: 12, confidence: 0.95 }
    }
}

/// How the input series is prewhitened before cross-correlation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prewhiten {
    /// Use the series as they are.
    None,
    /// An autoregression of the order with the smallest AIC.
    Aic,
    /// An autoregression of this order.
    Order(usize),
}

/// Settings of a cross-correlation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrossSettings {
    /// The input (explanatory) series column; the request's series is the
    /// output.
    pub input: usize,
    /// Largest lag either way (`None`: `10 log₁₀(n/2)`).
    pub max_lag: Option<usize>,
    /// Prewhitening of the input.
    pub prewhiten: Prewhiten,
    /// Coverage of the limits.
    pub confidence: f64,
}

impl CrossSettings {
    /// Input column `input`, default lags, no prewhitening, 95% limits.
    #[must_use]
    pub fn new(input: usize) -> Self {
        Self { input, max_lag: None, prewhiten: Prewhiten::None, confidence: 0.95 }
    }
}

/// Which exploration, with its settings.
#[derive(Debug, Clone, PartialEq)]
pub enum ExploreMethod {
    /// Series description.
    Describe(DescribeSettings),
    /// Decomposition.
    Decompose(DecomposeSettings),
    /// Trend model.
    Trend(TrendSettings),
    /// Cross-correlation with an input series.
    CrossCorrelation(CrossSettings),
}

impl ExploreMethod {
    /// Report and menu name.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Describe(_) => "Series Description",
            Self::Decompose(_) => "Decomposition",
            Self::Trend(_) => "Trend Model",
            Self::CrossCorrelation(_) => "Cross-Correlation",
        }
    }

    /// Project-file `analysis` name.
    #[must_use]
    pub fn key(&self) -> &'static str {
        match self {
            Self::Describe(_) => "series-description",
            Self::Decompose(_) => "decomposition",
            Self::Trend(_) => "trend-models",
            Self::CrossCorrelation(_) => "cross-correlation",
        }
    }

    /// The analysis names this module reads.
    pub const KEYS: [&'static str; 4] = ["series-description", "decomposition", "trend-models", "cross-correlation"];
}

/// A Series and Signals ▸ Explore request: one series and the exploration.
#[derive(Debug, Clone, PartialEq)]
pub struct SeriesExploreRequest {
    /// The series (for a cross-correlation, the output series).
    pub series: SeriesInput,
    /// What to do with it.
    pub method: ExploreMethod,
}

fn adf_key(d: AdfDeterministic) -> &'static str {
    match d {
        AdfDeterministic::None => "none",
        AdfDeterministic::Constant => "constant",
        AdfDeterministic::ConstantTrend => "trend",
    }
}

fn adf_from_key(key: &str) -> Option<AdfDeterministic> {
    match key {
        "none" => Some(AdfDeterministic::None),
        "constant" => Some(AdfDeterministic::Constant),
        "trend" => Some(AdfDeterministic::ConstantTrend),
        _ => None,
    }
}

impl SeriesExploreRequest {
    /// A series description of `column` with default settings.
    #[must_use]
    pub fn describe(column: usize) -> Self {
        Self { series: SeriesInput::new(column), method: ExploreMethod::Describe(DescribeSettings::default()) }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut out = self.series.columns();
        if let ExploreMethod::CrossCorrelation(c) = &self.method {
            out.push(c.input);
        }
        out
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.series.map_columns(f);
        if let ExploreMethod::CrossCorrelation(c) = &mut self.method {
            c.input = f(c.input);
        }
    }

    /// The project-file (JSON) form, columns by name.
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let n = JsonValue::number;
        let mut m: Vec<(String, JsonValue)> = vec![("analysis".into(), crate::project::s(self.method.key()))];
        m.extend(self.series.json_members(table));
        let lag = |l: Option<usize>| l.map_or(JsonValue::Null, |k| n(k as f64));
        match &self.method {
            ExploreMethod::Describe(d) => {
                let box_cox = match d.box_cox {
                    BoxCoxChoice::None => crate::project::s("none"),
                    BoxCoxChoice::Guerrero => crate::project::s("guerrero"),
                    BoxCoxChoice::Lambda(l) => n(l),
                };
                m.extend([
                    ("box_cox".into(), box_cox),
                    ("differences".into(), n(d.differences as f64)),
                    ("seasonal_differences".into(), n(d.seasonal_differences as f64)),
                    ("max_lag".into(), lag(d.max_lag)),
                    ("bartlett".into(), JsonValue::Bool(d.bartlett)),
                    ("confidence".into(), n(d.confidence)),
                    ("adf".into(), crate::project::s(adf_key(d.adf))),
                    ("unit_root".into(), JsonValue::Bool(d.unit_root)),
                    ("spectrum".into(), JsonValue::Bool(d.spectrum)),
                    ("spans".into(), JsonValue::Array(d.spans.iter().map(|&s| n(s as f64)).collect())),
                    ("spectrum_axis".into(), crate::project::s(if d.period_axis { "period" } else { "frequency" })),
                ]);
            }
            ExploreMethod::Decompose(d) => {
                m.push(("method".into(), crate::project::s(d.method.key())));
                m.push((
                    "seasonal_window".into(),
                    d.seasonal_window.map_or_else(|| crate::project::s("periodic"), |w| n(w as f64)),
                ));
                m.push(("robust".into(), JsonValue::Bool(d.robust)));
            }
            ExploreMethod::Trend(t) => {
                m.extend([
                    ("model".into(), crate::project::s(t.model.key())),
                    ("span".into(), n(t.span as f64)),
                    ("horizon".into(), n(t.horizon as f64)),
                    ("confidence".into(), n(t.confidence)),
                ]);
            }
            ExploreMethod::CrossCorrelation(c) => {
                m.push(("input".into(), crate::project::s(table.column(c.input).map_or("", |x| x.name.as_str()))));
                m.push(("max_lag".into(), lag(c.max_lag)));
                m.push((
                    "prewhiten".into(),
                    match c.prewhiten {
                        Prewhiten::None => crate::project::s("none"),
                        Prewhiten::Aic => crate::project::s("aic"),
                        Prewhiten::Order(p) => n(p as f64),
                    },
                ));
                m.push(("confidence".into(), n(c.confidence)));
            }
        }
        JsonValue::Object(m)
    }

    /// Read the JSON form of analysis `analysis` (one of
    /// [`ExploreMethod::KEYS`]); settings left out take their defaults.
    ///
    /// # Errors
    /// Missing or unknown columns, unknown option names.
    pub fn from_json(analysis: &str, v: &JsonValue, table: &DataTable) -> Result<Self> {
        let series = SeriesInput::from_json(v, table)?;
        let text = |key: &str| v.get(key).and_then(JsonValue::as_str);
        let count = |key: &str| v.get(key).and_then(JsonValue::as_usize);
        let flag = |key: &str, d: bool| v.get(key).and_then(JsonValue::as_bool).unwrap_or(d);
        let level = |d: f64| v.get("confidence").and_then(JsonValue::as_f64).unwrap_or(d);
        let method = match analysis {
            "series-description" => {
                let d = DescribeSettings::default();
                let box_cox = match v.get("box_cox") {
                    None | Some(JsonValue::Null) => BoxCoxChoice::None,
                    Some(JsonValue::String(s)) if s == "none" => BoxCoxChoice::None,
                    Some(JsonValue::String(s)) if s == "guerrero" => BoxCoxChoice::Guerrero,
                    Some(x) => BoxCoxChoice::Lambda(
                        x.as_f64().ok_or_else(|| invalid("box_cox must be \"none\", \"guerrero\" or a number"))?,
                    ),
                };
                let adf = match text("adf") {
                    None => d.adf,
                    Some(k) => adf_from_key(k).ok_or_else(|| invalid(format!("unknown ADF terms {k:?}")))?,
                };
                let spans = match v.get("spans").and_then(JsonValue::as_array) {
                    None => d.spans.clone(),
                    Some(items) => items.iter().filter_map(JsonValue::as_usize).collect(),
                };
                ExploreMethod::Describe(DescribeSettings {
                    box_cox,
                    differences: count("differences").unwrap_or(0),
                    seasonal_differences: count("seasonal_differences").unwrap_or(0),
                    max_lag: count("max_lag"),
                    bartlett: flag("bartlett", d.bartlett),
                    confidence: level(d.confidence),
                    adf,
                    unit_root: flag("unit_root", d.unit_root),
                    spectrum: flag("spectrum", d.spectrum),
                    spans,
                    period_axis: text("spectrum_axis") == Some("period"),
                })
            }
            "decomposition" => {
                let d = DecomposeSettings::default();
                let method = match text("method") {
                    None => d.method,
                    Some(k) => DecomposeMethod::from_key(k)
                        .ok_or_else(|| invalid(format!("unknown decomposition method {k:?}")))?,
                };
                ExploreMethod::Decompose(DecomposeSettings {
                    method,
                    seasonal_window: count("seasonal_window"),
                    robust: flag("robust", d.robust),
                })
            }
            "trend-models" => {
                let d = TrendSettings::default();
                let model = match text("model") {
                    None => d.model,
                    Some(k) => TrendChoice::from_key(k).ok_or_else(|| invalid(format!("unknown trend model {k:?}")))?,
                };
                ExploreMethod::Trend(TrendSettings {
                    model,
                    span: count("span").unwrap_or(d.span),
                    horizon: count("horizon").unwrap_or(d.horizon),
                    confidence: level(d.confidence),
                })
            }
            "cross-correlation" => {
                let input =
                    json_column(v, "input", table)?.ok_or_else(|| invalid("cross-correlation: missing \"input\""))?;
                let prewhiten = match v.get("prewhiten") {
                    None | Some(JsonValue::Null) => Prewhiten::None,
                    Some(JsonValue::String(s)) if s == "none" => Prewhiten::None,
                    Some(JsonValue::String(s)) if s == "aic" => Prewhiten::Aic,
                    Some(x) => Prewhiten::Order(
                        x.as_usize().ok_or_else(|| invalid("prewhiten must be \"none\", \"aic\" or an order"))?,
                    ),
                };
                ExploreMethod::CrossCorrelation(CrossSettings {
                    input,
                    max_lag: count("max_lag"),
                    prewhiten,
                    confidence: level(0.95),
                })
            }
            other => return Err(invalid(format!("unknown time series analysis {other:?}"))),
        };
        Ok(Self { series, method })
    }
}

/// A computed Series and Signals ▸ Explore report.
#[derive(Debug, Clone)]
pub struct SeriesExploreAnalysis {
    request: SeriesExploreRequest,
    title: String,
    sections: Vec<ReportSection>,
    columns: Vec<ReportColumn>,
}

impl SeriesExploreAnalysis {
    /// Extract the series and run the exploration.
    ///
    /// # Errors
    /// A missing or non-numeric column, a gap in the series, too short a
    /// series for the method, or invalid settings (a confidence level outside
    /// (0.5, 1), a period below 2 for a decomposition, non-positive values
    /// for a Box–Cox, multiplicative, exponential or logistic fit).
    pub fn run(table: &DataTable, states: &RowStateManager, request: &SeriesExploreRequest) -> Result<Self> {
        let level = match &request.method {
            ExploreMethod::Describe(d) => d.confidence,
            ExploreMethod::Trend(t) => t.confidence,
            ExploreMethod::CrossCorrelation(c) => c.confidence,
            ExploreMethod::Decompose(_) => 0.95,
        };
        if !(level > 0.5 && level < 1.0) {
            return Err(invalid("the confidence level must be between 0.5 and 1"));
        }
        let (title, sections, columns) = match &request.method {
            ExploreMethod::CrossCorrelation(c) => {
                if c.input == request.series.column {
                    return Err(invalid("choose different input and output columns"));
                }
                let mut both = Series::extract_aligned(
                    table,
                    states,
                    &[c.input, request.series.column],
                    request.series.time,
                    request.series.period,
                )?;
                let output = both.pop().ok_or_else(|| invalid("no output series"))?;
                let input = both.pop().ok_or_else(|| invalid("no input series"))?;
                let sections = cross::sections(&input, &output, c)?;
                (format!("Cross-Correlation: {} → {}", input.name, output.name), sections, Vec::new())
            }
            method => {
                let series = Series::extract(table, states, &request.series)?;
                match method {
                    ExploreMethod::Describe(d) => {
                        (format!("Series Description: {}", series.name), describe::sections(&series, d)?, Vec::new())
                    }
                    ExploreMethod::Decompose(d) => {
                        let (sections, columns) = decompose::run(&series, d, table.n_rows())?;
                        (format!("Decomposition ({}): {}", d.method.label(), series.name), sections, columns)
                    }
                    ExploreMethod::Trend(t) => {
                        let (sections, columns) = trend::run(&series, t, table.n_rows())?;
                        (format!("Trend Model ({}): {}", t.model.label(), series.name), sections, columns)
                    }
                    ExploreMethod::CrossCorrelation(_) => unreachable!("handled above"),
                }
            }
        };
        Ok(Self { request: request.clone(), title, sections, columns })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &SeriesExploreRequest {
        &self.request
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        self.title.clone()
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        self.sections.clone()
    }

    /// Columns the report can add (decomposition components; a trend model's
    /// fitted values and residuals).
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        self.columns.clone()
    }
}

/// A needle (stem) plot of correlations at `lags` with limits: a vertical
/// segment from zero to each value with a marker at its tip, the zero line,
/// and the limits as a dashed band (`limits`: one half-width per lag).
pub(crate) fn needle_plot(
    lags: &[f64],
    values: &[f64],
    limits: &[f64],
    labels: (&str, &str),
    names: (&str, &str),
) -> PlotSpec {
    let segments: Vec<Polyline> = lags.iter().zip(values).map(|(&k, &r)| vec![[k, 0.0], [k, r]]).collect();
    let lo = lags.iter().copied().fold(f64::INFINITY, f64::min) - 0.5;
    let hi = lags.iter().copied().fold(f64::NEG_INFINITY, f64::max) + 0.5;
    let (mut lower, mut upper): (Polyline, Polyline) = (Vec::new(), Vec::new());
    let constant = limits.windows(2).all(|w| (w[0] - w[1]).abs() < 1e-12);
    if constant {
        let l = limits.first().copied().unwrap_or(0.0);
        lower = vec![[lo, -l], [hi, -l]];
        upper = vec![[lo, l], [hi, l]];
    } else {
        for (&k, &l) in lags.iter().zip(limits) {
            lower.extend([[k - 0.5, -l], [k + 0.5, -l]]);
            upper.extend([[k - 0.5, l], [k + 0.5, l]]);
        }
    }
    PlotSpec {
        x_label: labels.0.to_owned(),
        y_label: labels.1.to_owned(),
        line: Some(vec![[lo, 0.0], [hi, 0.0]]),
        band: Some((lower, upper)),
        band_name: Some(names.1.to_owned()),
        annotations: Some(Annotations {
            groups: vec![names.0.to_owned()],
            items: lags.iter().zip(values).map(|(&k, &r)| (k, r, String::new(), 0)).collect(),
            labels: Vec::new(),
        }),
        segments,
        ..PlotSpec::default()
    }
}

/// The time series plot of `values` (parallel to the series' observations):
/// brushable points joined by a line.
pub(crate) fn series_plot(series: &Series, values: &[f64], y_label: &str, name: &str) -> PlotSpec {
    PlotSpec {
        x_label: series.x_label.clone(),
        y_label: y_label.to_owned(),
        points: series.points(values),
        point_name: Some(name.to_owned()),
        line: Some(series.polyline(values)),
        line_name: Some(format!("{name} (joined)")),
        x_time: series.x_time,
        ..PlotSpec::default()
    }
}

#[cfg(test)]
mod tests;
