//! Series and Signals ▸ Exponential smoothing: an ETS model
//! ([`stat_timeseries::smoothing`]) fitted to one numeric column by maximum
//! likelihood, chosen by the user or automatically by AICc, with forecasts,
//! prediction intervals, residual diagnostics and in-sample accuracy. The
//! series is read and the shared sections drawn by [`crate::forecasting`].

use crate::forecasting::{
    ForecastPath, ForecastSettings, Series, SeriesInput, accuracy, accuracy_table, forecast_data_table, forecast_plot,
    forecast_table, level_label, num, period_note, read_series, residual_diagnostics, residual_plot, section,
};
use crate::format::format_number;
use crate::project::{invalid, s};
use crate::report::{ReportSection, SectionContent, SectionKind, TextTable};
use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_timeseries::smoothing::{
    ErrorType, EtsCandidate, EtsFit, EtsOptions, EtsSpec, ForecastOptions, SeasonalType, SelectionOptions, TrendType,
    fit_ets, select_ets,
};

/// Which exponential smoothing model to fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmoothingModel {
    /// Every allowed model fitted; the smallest AICc kept.
    Automatic,
    /// Simple exponential smoothing, ETS(A,N,N).
    Simple,
    /// Holt's linear trend, ETS(A,A,N).
    HoltLinear,
    /// Damped trend, ETS(A,Ad,N).
    DampedTrend,
    /// Additive Holt–Winters, ETS(A,A,A).
    HoltWintersAdditive,
    /// Multiplicative Holt–Winters, ETS(M,A,M).
    HoltWintersMultiplicative,
    /// Any error, trend and seasonal combination.
    Custom(EtsSpec),
}

impl SmoothingModel {
    /// The named choices, in menu order (custom last).
    pub const NAMED: [Self; 6] = [
        Self::Automatic,
        Self::Simple,
        Self::HoltLinear,
        Self::DampedTrend,
        Self::HoltWintersAdditive,
        Self::HoltWintersMultiplicative,
    ];

    /// Menu label.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Automatic => "Automatic (smallest AICc)".into(),
            Self::Simple => "Simple exponential smoothing".into(),
            Self::HoltLinear => "Holt linear trend".into(),
            Self::DampedTrend => "Damped trend".into(),
            Self::HoltWintersAdditive => "Holt–Winters, additive seasonality".into(),
            Self::HoltWintersMultiplicative => "Holt–Winters, multiplicative seasonality".into(),
            Self::Custom(spec) => format!("Custom: {}", spec.label()),
        }
    }

    /// The model, or `None` for automatic selection.
    #[must_use]
    pub fn spec(self) -> Option<EtsSpec> {
        match self {
            Self::Automatic => None,
            Self::Simple => Some(EtsSpec::simple()),
            Self::HoltLinear => Some(EtsSpec::holt_linear()),
            Self::DampedTrend => Some(EtsSpec::damped_trend()),
            Self::HoltWintersAdditive => Some(EtsSpec::holt_winters_additive()),
            Self::HoltWintersMultiplicative => Some(EtsSpec::holt_winters_multiplicative()),
            Self::Custom(spec) => Some(spec),
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::Simple => "simple",
            Self::HoltLinear => "holt-linear",
            Self::DampedTrend => "damped-trend",
            Self::HoltWintersAdditive => "holt-winters-additive",
            Self::HoltWintersMultiplicative => "holt-winters-multiplicative",
            Self::Custom(_) => "custom",
        }
    }
}

fn error_key(e: ErrorType) -> &'static str {
    match e {
        ErrorType::Additive => "additive",
        ErrorType::Multiplicative => "multiplicative",
    }
}

fn trend_key(t: TrendType) -> &'static str {
    match t {
        TrendType::None => "none",
        TrendType::Additive => "additive",
        TrendType::DampedAdditive => "damped",
    }
}

fn seasonal_key(t: SeasonalType) -> &'static str {
    match t {
        SeasonalType::None => "none",
        SeasonalType::Additive => "additive",
        SeasonalType::Multiplicative => "multiplicative",
    }
}

/// Smoothing parameters held at given values instead of estimated.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FixedSmoothing {
    /// Level smoothing α.
    pub alpha: Option<f64>,
    /// Slope smoothing β.
    pub beta: Option<f64>,
    /// Seasonal smoothing γ.
    pub gamma: Option<f64>,
    /// Damping φ.
    pub phi: Option<f64>,
}

/// Columns and settings of an exponential smoothing report.
#[derive(Debug, Clone, PartialEq)]
pub struct SmoothingRequest {
    /// Series, time column and seasonal period.
    pub series: SeriesInput,
    /// Model, or automatic selection.
    pub model: SmoothingModel,
    /// Automatic selection: consider multiplicative errors and seasonality
    /// (used only for a strictly positive series).
    pub allow_multiplicative: bool,
    /// Automatic selection: consider damped trends.
    pub allow_damped: bool,
    /// Parameters held fixed (a chosen model only; ignored for components
    /// the model lacks).
    pub fixed: FixedSmoothing,
    /// Horizon and interval levels.
    pub forecast: ForecastSettings,
}

impl SmoothingRequest {
    /// Automatic selection for column `response`, with default settings.
    #[must_use]
    pub fn new(response: usize) -> Self {
        Self {
            series: SeriesInput { column: response, time: None, period: None },
            model: SmoothingModel::Automatic,
            allow_multiplicative: true,
            allow_damped: true,
            fixed: FixedSmoothing::default(),
            forecast: ForecastSettings::default(),
        }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        self.series.columns()
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.series.map_columns(f);
    }

    /// The project-file (JSON) form, columns by name.
    pub(crate) fn to_json(&self, table: &DataTable) -> JsonValue {
        let n = JsonValue::number;
        let mut m: Vec<(String, JsonValue)> = vec![("analysis".into(), s("exponential-smoothing"))];
        m.extend(self.series.json_members(table));
        m.push(("model".into(), s(self.model.key())));
        if let SmoothingModel::Custom(spec) = self.model {
            m.push(("error".into(), s(error_key(spec.error))));
            m.push(("trend".into(), s(trend_key(spec.trend))));
            m.push(("seasonal".into(), s(seasonal_key(spec.seasonal))));
        }
        if self.model == SmoothingModel::Automatic {
            m.push(("allow_multiplicative".into(), JsonValue::Bool(self.allow_multiplicative)));
            m.push(("allow_damped".into(), JsonValue::Bool(self.allow_damped)));
        }
        for (key, v) in [
            ("alpha", self.fixed.alpha),
            ("beta", self.fixed.beta),
            ("gamma", self.fixed.gamma),
            ("phi", self.fixed.phi),
        ] {
            if let Some(v) = v {
                m.push((key.into(), n(v)));
            }
        }
        self.forecast.push_json(&mut m);
        JsonValue::Object(m)
    }

    /// Read the JSON form (see [`to_json`](Self::to_json)).
    pub(crate) fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let series = SeriesInput::from_json(v, table)?;
        let text = |key: &str| v.get(key).and_then(JsonValue::as_str);
        let bad = |key: &str, value: &str| invalid(format!("exponential smoothing {key}: unknown value {value:?}"));
        let model = match text("model").unwrap_or("automatic") {
            "automatic" => SmoothingModel::Automatic,
            "simple" => SmoothingModel::Simple,
            "holt-linear" => SmoothingModel::HoltLinear,
            "damped-trend" => SmoothingModel::DampedTrend,
            "holt-winters-additive" => SmoothingModel::HoltWintersAdditive,
            "holt-winters-multiplicative" => SmoothingModel::HoltWintersMultiplicative,
            "custom" => {
                let error = match text("error").unwrap_or("additive") {
                    "additive" => ErrorType::Additive,
                    "multiplicative" => ErrorType::Multiplicative,
                    other => return Err(bad("error", other)),
                };
                let trend = match text("trend").unwrap_or("none") {
                    "none" => TrendType::None,
                    "additive" => TrendType::Additive,
                    "damped" => TrendType::DampedAdditive,
                    other => return Err(bad("trend", other)),
                };
                let seasonal = match text("seasonal").unwrap_or("none") {
                    "none" => SeasonalType::None,
                    "additive" => SeasonalType::Additive,
                    "multiplicative" => SeasonalType::Multiplicative,
                    other => return Err(bad("seasonal", other)),
                };
                SmoothingModel::Custom(EtsSpec::new(error, trend, seasonal))
            }
            other => return Err(bad("model", other)),
        };
        let number = |key: &str| v.get(key).and_then(JsonValue::as_f64);
        let flag = |key: &str| v.get(key).and_then(JsonValue::as_bool).unwrap_or(true);
        Ok(Self {
            series,
            model,
            allow_multiplicative: flag("allow_multiplicative"),
            allow_damped: flag("allow_damped"),
            fixed: FixedSmoothing {
                alpha: number("alpha"),
                beta: number("beta"),
                gamma: number("gamma"),
                phi: number("phi"),
            },
            forecast: ForecastSettings::from_json(v),
        })
    }
}

/// A computed exponential smoothing report.
#[derive(Debug, Clone)]
pub struct SmoothingAnalysis {
    request: SmoothingRequest,
    series: Series,
    period: usize,
    period_note: String,
    fit: EtsFit,
    /// Candidates of an automatic selection (empty for a chosen model).
    candidates: Vec<EtsCandidate>,
    /// Which of α, β, γ, φ were estimated.
    estimated: [bool; 4],
    forecast: ForecastPath,
    /// Means of simulated sample paths, when the intervals were simulated.
    path_means: Option<Vec<f64>>,
}

impl SmoothingAnalysis {
    /// Read the series, fit (or select) the model and forecast.
    ///
    /// # Errors
    /// As [`read_series`]; invalid forecast settings; the model needing more
    /// data (two full cycles for a seasonal model), a positive series
    /// (multiplicative components) or a seasonal period of at least 2; no
    /// candidate of an automatic selection fitting.
    pub fn run(table: &DataTable, rows: &RowStateManager, request: &SmoothingRequest) -> Result<Self> {
        request.forecast.validate()?;
        let series = read_series(table, rows, &request.series)?;
        let (period, period_note) = (series.period, period_note(&series));
        let (fit, candidates, estimated) = match request.model.spec() {
            None => {
                let options = SelectionOptions {
                    allow_multiplicative: request.allow_multiplicative,
                    allow_damped: request.allow_damped,
                    ..SelectionOptions::new(period)
                };
                let selection = select_ets(&series.values, &options)?;
                let spec = selection.best.spec;
                let estimated = [
                    true,
                    spec.trend != TrendType::None,
                    spec.seasonal != SeasonalType::None,
                    spec.trend == TrendType::DampedAdditive,
                ];
                (selection.best, selection.candidates, estimated)
            }
            Some(spec) => {
                if spec.seasonal != SeasonalType::None && period < 2 {
                    return Err(invalid(format!(
                        "{} is seasonal: set a seasonal period of at least 2 (observations per cycle)",
                        spec.name()
                    )));
                }
                let has = [
                    true,
                    spec.trend != TrendType::None,
                    spec.seasonal != SeasonalType::None,
                    spec.trend == TrendType::DampedAdditive,
                ];
                let f = request.fixed;
                let keep = |on: bool, v: Option<f64>| if on { v } else { None };
                let options = EtsOptions {
                    alpha: keep(has[0], f.alpha),
                    beta: keep(has[1], f.beta),
                    gamma: keep(has[2], f.gamma),
                    phi: keep(has[3], f.phi),
                    ..EtsOptions::new(period)
                };
                let estimated = [
                    options.alpha.is_none(),
                    has[1] && options.beta.is_none(),
                    has[2] && options.gamma.is_none(),
                    has[3] && options.phi.is_none(),
                ];
                (fit_ets(&series.values, spec, &options)?, Vec::new(), estimated)
            }
        };
        let levels = request.forecast.sorted_levels();
        let options = ForecastOptions { levels: levels.clone(), ..ForecastOptions::default() };
        let f = fit.forecast(request.forecast.horizon, &options)?;
        let method_note = if f.simulated {
            Some(format!(
                "multiplicative seasonality has no closed-form forecast variance, so the limits are quantiles of \
                 {} simulated sample paths (seed {}); point forecasts follow the recursion with zero future errors",
                options.paths, options.seed
            ))
        } else {
            Some("analytic forecast variances; limits are forecast ± z × std error".to_owned())
        };
        let forecast = ForecastPath {
            mean: f.mean,
            std_errors: f.standard_errors,
            levels: f.levels,
            lower: f.lower,
            upper: f.upper,
            method_note,
        };
        Ok(Self {
            request: request.clone(),
            series,
            period,
            period_note,
            fit,
            candidates,
            estimated,
            forecast,
            path_means: f.path_means,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &SmoothingRequest {
        &self.request
    }

    /// The fitted model.
    #[must_use]
    pub fn fit(&self) -> &EtsFit {
        &self.fit
    }

    /// The forecasts.
    #[must_use]
    pub fn forecast(&self) -> &ForecastPath {
        &self.forecast
    }

    /// The series analysed.
    #[must_use]
    pub fn series(&self) -> &Series {
        &self.series
    }

    /// Seasonal period used.
    #[must_use]
    pub fn period(&self) -> usize {
        self.period
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Exponential Smoothing: {} — {}", self.series.name, model_text(&self.fit.spec))
    }

    /// Number of smoothing parameters (α, β, γ, φ) estimated, taken off the
    /// Ljung–Box degrees of freedom.
    #[must_use]
    pub fn fitted_smoothing_parameters(&self) -> usize {
        self.estimated.iter().filter(|e| **e).count()
    }

    /// The table "Save forecasts" adds: time, actual, one-step fitted, then
    /// forecasts with their limits.
    ///
    /// # Errors
    /// Only if the table cannot be built.
    pub fn forecast_table(&self, table: &DataTable) -> Result<DataTable> {
        let time = self.request.series.time.and_then(|t| table.column(t));
        forecast_data_table(&self.series, &self.fit.fitted, &self.forecast, &self.fit.spec.label(), time)
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary()];
        if !self.candidates.is_empty() {
            out.push(self.comparison());
        }
        out.push(self.parameters());
        out.push(self.initial_states());
        let label = self.fit.spec.label();
        out.push(forecast_plot(&self.series, &self.fit.fitted, &self.forecast, &label));
        let mut table = forecast_table(&self.series, &self.forecast);
        if let (Some(means), SectionContent::Table(t)) = (&self.path_means, &mut table.content) {
            t.columns.push("Mean of simulated paths".to_owned());
            for (row, m) in t.rows.iter_mut().zip(means) {
                row.push(num(*m));
            }
        }
        out.push(table);
        let residual_label = match self.fit.spec.error {
            ErrorType::Additive => "Residual (y − ŷ)",
            ErrorType::Multiplicative => "Relative residual ((y − ŷ)/ŷ)",
        };
        out.push(residual_plot(&self.series, &self.fit.residuals, residual_label));
        out.extend(residual_diagnostics(&self.fit.residuals, self.period, self.fitted_smoothing_parameters()));
        out.push(accuracy_table(&accuracy(&self.series.values, &self.fit.fitted)));
        out
    }

    fn summary(&self) -> ReportSection {
        let f = &self.fit;
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut row = |k: &str, v: String| t.push([k.to_owned(), v]);
        row("Model", f.spec.label());
        row("Method", f.spec.name());
        row(
            "Errors",
            match f.spec.error {
                ErrorType::Additive => "additive".to_owned(),
                ErrorType::Multiplicative => "multiplicative (relative to the one-step forecast)".to_owned(),
            },
        );
        row("Observations", f.n.to_string());
        row("Seasonal period", format!("{} ({})", self.period, self.period_note));
        row("Parameters estimated (smoothing and initial states)", f.estimated_parameters.to_string());
        row("Log-likelihood", num(f.log_likelihood));
        row("AIC", num(f.aic));
        row("AICc", f.aicc.map_or_else(|| "— (too few observations)".to_owned(), num));
        row("BIC", num(f.bic));
        row("σ (innovation std deviation)", num(f.sigma()));
        let mut title = format!("Model Summary ({})", self.series.name);
        if self.series.n_trimmed > 0 {
            title = format!(
                "Model Summary ({}; {} rows before or after the series excluded or empty)",
                self.series.name, self.series.n_trimmed
            );
        }
        section(SectionKind::ForecastModelSummary, title, SectionContent::Table(t))
    }

    fn comparison(&self) -> ReportSection {
        let best = self.candidates.iter().filter_map(|c| c.aicc).fold(f64::INFINITY, f64::min);
        let mut sorted: Vec<&EtsCandidate> = self.candidates.iter().collect();
        sorted.sort_by(|a, b| match (a.aicc, b.aicc) {
            (Some(x), Some(y)) => x.total_cmp(&y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        });
        let mut t = TextTable::new(["Model", "Method", "AICc", "ΔAICc", "Chosen"]);
        for c in sorted {
            let chosen = c.spec == self.fit.spec;
            t.push([
                c.spec.label(),
                c.spec.name(),
                c.aicc.map_or_else(|| "could not be fitted".to_owned(), num),
                c.aicc.map_or_else(String::new, |a| format_number(a - best, 4)),
                if chosen { "✓ chosen".to_owned() } else { String::new() },
            ]);
        }
        let n = self.candidates.len();
        section(
            SectionKind::ForecastModelComparison,
            format!(
                "Model Selection ({n} candidate models fitted by maximum likelihood, ranked by AICc; the smallest \
                 is chosen{})",
                if self.series.values.iter().all(|v| *v > 0.0) {
                    ""
                } else {
                    "; multiplicative forms need a strictly positive series"
                }
            ),
            SectionContent::Table(t),
        )
    }

    fn parameters(&self) -> ReportSection {
        let p = &self.fit.parameters;
        let mut t = TextTable::new(["Parameter", "Smooths", "Value", "How"]);
        let how = |e: bool| if e { "estimated" } else { "fixed" }.to_owned();
        t.push(["α".to_owned(), "level".to_owned(), num(p.alpha), how(self.estimated[0])]);
        if let Some(b) = p.beta {
            t.push(["β".to_owned(), "slope (trend)".to_owned(), num(b), how(self.estimated[1])]);
        }
        if let Some(g) = p.gamma {
            t.push(["γ".to_owned(), "seasonal effects".to_owned(), num(g), how(self.estimated[2])]);
        }
        if let Some(phi) = p.phi {
            t.push(["φ".to_owned(), "damping of the trend".to_owned(), num(phi), how(self.estimated[3])]);
        }
        section(
            SectionKind::ForecastParameters,
            "Smoothing Parameters (each between 0 and 1: values near 1 follow recent observations closely, values \
             near 0 change slowly)",
            SectionContent::Table(t),
        )
    }

    fn initial_states(&self) -> ReportSection {
        let init = &self.fit.initial;
        let mut t = TextTable::new(["State", "Initial value"]);
        t.push(["Level ℓ₀".to_owned(), num(init.level)]);
        if self.fit.spec.trend != TrendType::None {
            t.push(["Slope b₀".to_owned(), num(init.slope)]);
        }
        let kind = match self.fit.spec.seasonal {
            SeasonalType::Additive => "seasonal effect",
            _ => "seasonal factor",
        };
        for (j, v) in init.seasonal.iter().enumerate() {
            let when = self.series.x.get(j).map_or_else(String::new, |&x| format!(" ({})", self.series.x_text(x)));
            t.push([format!("Season {}{when} {kind}", j + 1), num(*v)]);
        }
        let note = match self.fit.spec.seasonal {
            SeasonalType::None => "",
            SeasonalType::Additive => "; seasonal effects sum to 0",
            SeasonalType::Multiplicative => "; seasonal factors average 1",
        };
        section(
            SectionKind::ForecastInitialStates,
            format!("Initial States (estimated with the smoothing parameters; seasons of the first cycle{note})"),
            SectionContent::Table(t),
        )
    }
}

fn model_text(spec: &EtsSpec) -> String {
    let name = spec.name();
    if name == spec.label() { name } else { format!("{} {name}", spec.label()) }
}

/// Levels as "80%, 95%".
#[must_use]
pub fn levels_text(levels: &[f64]) -> String {
    levels.iter().map(|&l| level_label(l)).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::hotel_occupancy;

    fn request(t: &DataTable) -> SmoothingRequest {
        SmoothingRequest {
            series: SeriesInput { column: t.find("Occupied rooms").unwrap(), time: t.find("Month"), period: None },
            ..SmoothingRequest::new(0)
        }
    }

    #[test]
    fn automatic_selection_finds_multiplicative_seasonality() {
        let t = hotel_occupancy();
        let rows = RowStateManager::new(t.n_rows());
        let a = SmoothingAnalysis::run(&t, &rows, &request(&t)).unwrap();
        assert_eq!(a.period(), 12);
        assert_eq!(a.fit().spec.seasonal, SeasonalType::Multiplicative);
        let kinds: Vec<SectionKind> = a.sections().iter().map(|s| s.kind).collect();
        for k in [
            SectionKind::ForecastModelSummary,
            SectionKind::ForecastModelComparison,
            SectionKind::ForecastParameters,
            SectionKind::ForecastInitialStates,
            SectionKind::ForecastPlot,
            SectionKind::ForecastTable,
            SectionKind::ForecastResidualPlot,
            SectionKind::ForecastResidualAcf,
            SectionKind::ForecastLjungBox,
            SectionKind::ForecastAccuracy,
        ] {
            assert!(kinds.contains(&k), "{k:?} missing");
        }
        assert_eq!(a.forecast().mean.len(), ForecastSettings::DEFAULT_HORIZON);
        // The intervals nest and contain the point forecast.
        let f = a.forecast();
        for h in 0..f.mean.len() {
            assert!(f.lower[1][h] <= f.lower[0][h] && f.lower[0][h] <= f.mean[h]);
            assert!(f.mean[h] <= f.upper[0][h] && f.upper[0][h] <= f.upper[1][h]);
        }
        // Points of the forecast plot are the dataset's rows.
        let s = a.sections();
        let plot = s.iter().find(|x| x.kind == SectionKind::ForecastPlot).unwrap();
        let SectionContent::Plot(p) = &plot.content else { panic!() };
        assert_eq!(p.points.len(), t.n_rows());
        assert!(p.x_time.is_some());
    }

    #[test]
    fn chosen_model_with_fixed_parameter_and_saved_table() {
        let t = hotel_occupancy();
        let rows = RowStateManager::new(t.n_rows());
        let r = SmoothingRequest {
            model: SmoothingModel::HoltWintersAdditive,
            fixed: FixedSmoothing { alpha: Some(0.3), phi: Some(0.9), ..FixedSmoothing::default() },
            forecast: ForecastSettings { horizon: 6, levels: vec![0.9] },
            ..request(&t)
        };
        let a = SmoothingAnalysis::run(&t, &rows, &r).unwrap();
        assert_eq!(a.fit().parameters.alpha, 0.3);
        // α fixed; β and γ estimated; φ ignored (no damping).
        assert_eq!(a.fitted_smoothing_parameters(), 2);
        let saved = a.forecast_table(&t).unwrap();
        assert_eq!(saved.n_rows(), t.n_rows() + 6);
        assert!(saved.find("Lower 90%").is_some() && saved.find("Forecast").is_some());
        assert!(saved.column(0).unwrap().temporal().is_some());
    }

    #[test]
    fn json_round_trip_and_errors() {
        let t = hotel_occupancy();
        let mut r = request(&t);
        assert_eq!(SmoothingRequest::from_json(&r.to_json(&t), &t).unwrap(), r);
        r.model = SmoothingModel::Custom(EtsSpec::new(
            ErrorType::Multiplicative,
            TrendType::DampedAdditive,
            SeasonalType::None,
        ));
        r.fixed.phi = Some(0.95);
        r.series.period = Some(4);
        r.forecast.levels = vec![0.5];
        assert_eq!(SmoothingRequest::from_json(&r.to_json(&t), &t).unwrap(), r);
        // A seasonal model needs a period.
        let mut rows = RowStateManager::new(t.n_rows());
        let seasonal = SmoothingRequest {
            model: SmoothingModel::HoltWintersAdditive,
            series: SeriesInput { period: Some(1), ..r.series },
            ..r.clone()
        };
        assert!(SmoothingAnalysis::run(&t, &rows, &seasonal).is_err());
        // An excluded row inside the series is a gap.
        rows.set_excluded([10], true);
        let err = SmoothingAnalysis::run(&t, &rows, &request(&t)).unwrap_err().to_string();
        assert!(err.contains("gap at row 11"), "{err}");
    }
}
