//! Series and Signals ▸ ARIMA: a seasonal ARIMA(p,d,q)(P,D,Q)ₘ model
//! ([`stat_timeseries::arima`]) fitted to one numeric column by exact maximum
//! likelihood, with orders chosen by the user or by the stepwise automatic
//! search (differencing chosen by KPSS and seasonal-strength tests, then
//! orders by AICc), optionally on a Box–Cox scale, with forecasts,
//! prediction intervals, residual diagnostics and in-sample accuracy. The
//! series is read and the shared sections drawn by [`crate::forecasting`].
//!
//! **Inputs (transfer functions).** Other columns can enter as input series
//! through rational distributed lags `ω(B)B^b/δ(B)` (Box, Jenkins, Reinsel &
//! Ljung 2015, ch. 11–12; [`stat_timeseries::arima::transfer`]). The rows
//! used are those where the series and every input have values; rows after
//! the series whose inputs are filled in (and the series is empty) supply the
//! future input values the forecasts need, so the horizon is cut to what the
//! inputs cover. A delay left to the data is the first lag of significant
//! prewhitened cross-correlation between the input and the series, both
//! differenced as the model differences the series (for the automatic search,
//! as the KPSS and seasonal-strength tests difference it). The report adds
//! the transfer-function estimates, each input's impulse response (fitted and
//! estimated from the prewhitened cross-correlation) and the cross-correlation
//! of the residuals with the prewhitened input as a diagnostic check.
//!
//! **Box–Cox.** With a λ (given, or Guerrero's choice) the model is fitted
//! to `(y^λ − 1)/λ` (`ln y` at λ = 0). Fitted values, forecasts and limits
//! are transformed back; the back-transformed point forecast is the median
//! of the forecast distribution, not its mean, and the standard errors stay
//! on the transformed scale.

use crate::forecasting::{
    ForecastPath, ForecastSettings, Series, SeriesInput, accuracy, accuracy_table, forecast_data_table, forecast_plot,
    forecast_table, num, period_note, read_series, residual_diagnostics, residual_plot, section,
};
use crate::format::{format_number, format_p_value};
use crate::project::{invalid, s};
use crate::report::{Annotations, PlotSpec, ReportSection, SectionContent, SectionKind, TextTable};
pub use crate::series_explore::BoxCoxChoice;
use crate::time_series_data::json_column;
use stat_core::Result;
use stat_core::distributions::{ChiSquared, ContinuousDistribution, StudentsT};
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_timeseries::arima::{
    ArimaFit, ArimaOptions, ArimaSpec, AutoArimaOptions, CandidateModel, ImpulseResponseEstimate, TransferInput,
    auto_arima_transfer, fit_arima_transfer, impulse_response_estimate, kpss_differencing_order,
    seasonal_differencing_order,
};
use stat_timeseries::autocorrelation::{cross_correlation, yule_walker};
use stat_timeseries::transform::difference;
use stat_timeseries::transform::{box_cox, guerrero_lambda, inverse_box_cox, inverse_box_cox_clipped};

/// How the ARIMA orders are set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArimaOrders {
    /// Differencing by tests, then a search over the orders by AICc.
    Automatic {
        /// Stepwise search (otherwise every model up to the default bounds).
        stepwise: bool,
        /// Allow a mean or drift term.
        allow_constant: bool,
    },
    /// Orders given (the seasonal part uses the request's period).
    Chosen {
        /// (p, d, q).
        regular: (usize, usize, usize),
        /// (P, D, Q); all zero for a non-seasonal model.
        seasonal: (usize, usize, usize),
        /// Include a mean (d + D = 0) or drift (d + D = 1).
        constant: bool,
    },
}

/// An input series of a transfer-function model: the column and its
/// transfer-function orders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferRole {
    /// The input column (numeric).
    pub column: usize,
    /// Delay `b` (`None`: the first lag of significant prewhitened cross-correlation).
    pub delay: Option<usize>,
    /// Numerator order `s`.
    pub numerator: usize,
    /// Denominator order `r`.
    pub denominator: usize,
}

impl TransferRole {
    /// Input `column` with the delay from the data and one weight over a
    /// first-order denominator (a geometrically decaying response).
    #[must_use]
    pub const fn new(column: usize) -> Self {
        Self { column, delay: None, numerator: 0, denominator: 1 }
    }
}

/// Columns and settings of an ARIMA report.
#[derive(Debug, Clone, PartialEq)]
pub struct ArimaRequest {
    /// Series, time column and seasonal period.
    pub series: SeriesInput,
    /// Orders, or the automatic search.
    pub orders: ArimaOrders,
    /// Box–Cox transformation.
    pub box_cox: BoxCoxChoice,
    /// Horizon and interval levels.
    pub forecast: ForecastSettings,
    /// Input series entering through transfer functions (empty: none).
    pub inputs: Vec<TransferRole>,
}

impl ArimaRequest {
    /// Automatic stepwise search for column `response`, with default settings.
    #[must_use]
    pub fn new(response: usize) -> Self {
        Self {
            series: SeriesInput { column: response, time: None, period: None },
            orders: ArimaOrders::Automatic { stepwise: true, allow_constant: true },
            box_cox: BoxCoxChoice::None,
            forecast: ForecastSettings::default(),
            inputs: Vec::new(),
        }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut c = self.series.columns();
        c.extend(self.inputs.iter().map(|i| i.column));
        c
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.series.map_columns(f);
        for i in &mut self.inputs {
            i.column = f(i.column);
        }
    }

    /// The project-file (JSON) form, columns by name.
    pub(crate) fn to_json(&self, table: &DataTable) -> JsonValue {
        let n = |v: usize| JsonValue::number(v as f64);
        let mut m: Vec<(String, JsonValue)> = vec![("analysis".into(), s("arima"))];
        m.extend(self.series.json_members(table));
        match self.orders {
            ArimaOrders::Automatic { stepwise, allow_constant } => {
                m.push(("orders".into(), s("automatic")));
                m.push(("stepwise".into(), JsonValue::Bool(stepwise)));
                m.push(("allow_constant".into(), JsonValue::Bool(allow_constant)));
            }
            ArimaOrders::Chosen { regular: (p, d, q), seasonal: (sp, sd, sq), constant } => {
                m.push(("orders".into(), JsonValue::Array(vec![n(p), n(d), n(q)])));
                m.push(("seasonal_orders".into(), JsonValue::Array(vec![n(sp), n(sd), n(sq)])));
                m.push(("constant".into(), JsonValue::Bool(constant)));
            }
        }
        match self.box_cox {
            BoxCoxChoice::None => {}
            BoxCoxChoice::Lambda(l) => m.push(("box_cox".into(), JsonValue::number(l))),
            BoxCoxChoice::Guerrero => m.push(("box_cox".into(), s("guerrero"))),
        }
        self.forecast.push_json(&mut m);
        if !self.inputs.is_empty() {
            let inputs = self
                .inputs
                .iter()
                .map(|i| {
                    JsonValue::Object(vec![
                        ("column".into(), s(table.column(i.column).map_or("", |c| c.name.as_str()))),
                        ("delay".into(), i.delay.map_or_else(|| s("automatic"), n)),
                        ("numerator".into(), n(i.numerator)),
                        ("denominator".into(), n(i.denominator)),
                    ])
                })
                .collect();
            m.push(("inputs".into(), JsonValue::Array(inputs)));
        }
        JsonValue::Object(m)
    }

    /// Read the JSON form (see [`to_json`](Self::to_json)).
    pub(crate) fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let series = SeriesInput::from_json(v, table)?;
        let flag = |key: &str, default: bool| v.get(key).and_then(JsonValue::as_bool).unwrap_or(default);
        let triple = |key: &str| -> Result<(usize, usize, usize)> {
            match v.get(key).and_then(JsonValue::as_array) {
                None => Ok((0, 0, 0)),
                Some([a, b, c]) => match (a.as_usize(), b.as_usize(), c.as_usize()) {
                    (Some(a), Some(b), Some(c)) => Ok((a, b, c)),
                    _ => Err(invalid(format!("ARIMA {key}: expected three whole numbers"))),
                },
                Some(_) => Err(invalid(format!("ARIMA {key}: expected three whole numbers"))),
            }
        };
        let orders = match v.get("orders") {
            None => ArimaOrders::Automatic { stepwise: true, allow_constant: true },
            Some(JsonValue::String(t)) if t == "automatic" => ArimaOrders::Automatic {
                stepwise: flag("stepwise", true),
                allow_constant: flag("allow_constant", true),
            },
            Some(_) => ArimaOrders::Chosen {
                regular: triple("orders")?,
                seasonal: triple("seasonal_orders")?,
                constant: flag("constant", false),
            },
        };
        let box_cox = match v.get("box_cox") {
            None | Some(JsonValue::Null) => BoxCoxChoice::None,
            Some(JsonValue::String(t)) if t == "none" => BoxCoxChoice::None,
            Some(JsonValue::String(t)) if t == "guerrero" => BoxCoxChoice::Guerrero,
            Some(x) => BoxCoxChoice::Lambda(
                x.as_f64().ok_or_else(|| invalid("ARIMA box_cox: expected \"none\", \"guerrero\" or a number"))?,
            ),
        };
        let mut inputs = Vec::new();
        for i in v.get("inputs").and_then(JsonValue::as_array).unwrap_or_default() {
            let column = json_column(i, "column", table)?.ok_or_else(|| invalid("ARIMA input: missing \"column\""))?;
            let order = |key: &str| i.get(key).and_then(JsonValue::as_usize).unwrap_or(0);
            let delay = match i.get("delay") {
                None | Some(JsonValue::Null) => None,
                Some(JsonValue::String(t)) if t == "automatic" => None,
                Some(d) => Some(d.as_usize().ok_or_else(|| invalid("ARIMA input delay: expected a whole number"))?),
            };
            inputs.push(TransferRole {
                column,
                delay,
                numerator: order("numerator"),
                denominator: order("denominator"),
            });
        }
        Ok(Self { series, orders, box_cox, forecast: ForecastSettings::from_json(v), inputs })
    }
}

/// A computed ARIMA report.
#[derive(Debug, Clone)]
pub struct ArimaAnalysis {
    request: ArimaRequest,
    series: Series,
    period: usize,
    period_note: String,
    lambda: Option<f64>,
    fit: ArimaFit,
    /// Fitted values on the data's scale.
    fitted: Vec<f64>,
    /// Automatic search: (d, D, candidates).
    search: Option<(usize, usize, Vec<CandidateModel>)>,
    forecast: ForecastPath,
    inputs: Vec<InputReport>,
    /// Forecasts cut short because the inputs' future values ran out.
    horizon_note: Option<String>,
}

/// What the report shows about one input.
#[derive(Debug, Clone)]
struct InputReport {
    name: String,
    /// Delay used, and whether it came from the data.
    delay: usize,
    delay_from_data: bool,
    numerator: usize,
    denominator: usize,
    /// Prewhitened identification (on the differenced series).
    identification: Option<ImpulseResponseEstimate>,
    identification_error: Option<String>,
    /// Differencing applied before the identification: (d, D, period).
    differencing: (usize, usize, usize),
    /// The input's values on the series rows (for the residual check).
    values: Vec<f64>,
}

impl ArimaAnalysis {
    /// Read the series, transform it, fit (or search for) the model and forecast.
    ///
    /// # Errors
    /// As [`read_series`]; invalid forecast settings; Box–Cox of a series
    /// with a value ≤ 0; a seasonal part without a seasonal period of at
    /// least 2; a constant with d + D ≥ 2; too few observations to fit.
    pub fn run(table: &DataTable, rows: &RowStateManager, request: &ArimaRequest) -> Result<Self> {
        request.forecast.validate()?;
        let (series, input_values, future_inputs) = if request.inputs.is_empty() {
            (read_series(table, rows, &request.series)?, Vec::new(), Vec::new())
        } else {
            read_with_inputs(table, rows, request)?
        };
        let (period, period_note) = (series.period, period_note(&series));
        let lambda = match request.box_cox {
            BoxCoxChoice::None => None,
            BoxCoxChoice::Lambda(l) => Some(l),
            BoxCoxChoice::Guerrero => Some(guerrero_lambda(&series.values, period, -1.0, 2.0)?),
        };
        let y = match lambda {
            Some(l) => box_cox(&series.values, l)?,
            None => series.values.clone(),
        };
        // Differencing used to identify the inputs' delays: the model's, or the tests'.
        let differencing = match request.orders {
            _ if request.inputs.is_empty() => (0, 0, period),
            ArimaOrders::Chosen { regular: (_, d, _), seasonal: (_, sd, _), .. } => (d, sd, period),
            ArimaOrders::Automatic { .. } => {
                let sd = seasonal_differencing_order(&y, period, 1)?;
                let ys = if sd > 0 { difference(&y, 0, sd, period)? } else { y.clone() };
                (kpss_differencing_order(&ys, 2)?, sd, period)
            }
        };
        let mut inputs = Vec::new();
        let mut transfer_inputs = Vec::new();
        for (role, values) in request.inputs.iter().zip(&input_values) {
            let name = table.column(role.column).map_or_else(String::new, |c| c.name.clone());
            let (d, sd, m) = differencing;
            let ident = difference(values, d, sd, m).and_then(|dx| {
                let dy = difference(&y, d, sd, m)?;
                let max_lag = (dx.len() / 4).clamp(1, 24);
                impulse_response_estimate(&dx, &dy, Some(max_lag), None)
            });
            let (identification, identification_error) = match ident {
                Ok(e) => (Some(e), None),
                Err(e) => (None, Some(e.to_string())),
            };
            let delay =
                role.delay.unwrap_or_else(|| identification.as_ref().and_then(|e| e.suggested_delay).unwrap_or(0));
            transfer_inputs.push(TransferInput::new(name.clone(), values.clone()).with_orders(
                delay,
                role.numerator,
                role.denominator,
            ));
            inputs.push(InputReport {
                name,
                delay,
                delay_from_data: role.delay.is_none(),
                numerator: role.numerator,
                denominator: role.denominator,
                identification,
                identification_error,
                differencing,
                values: values.clone(),
            });
        }
        let (fit, search) = match request.orders {
            ArimaOrders::Automatic { stepwise, allow_constant } => {
                let options = AutoArimaOptions { stepwise, allow_constant, ..AutoArimaOptions::new(period) };
                let r = auto_arima_transfer(&y, &[], &transfer_inputs, &options)?;
                (r.fit, Some((r.d, r.seasonal_d, r.candidates)))
            }
            ArimaOrders::Chosen { regular: (p, d, q), seasonal: (sp, sd, sq), constant } => {
                let mut spec = ArimaSpec::new(p, d, q).with_constant(constant);
                if sp + sd + sq > 0 {
                    if period < 2 {
                        return Err(invalid(
                            "the seasonal part (P,D,Q) needs a seasonal period of at least 2 (observations per cycle)",
                        ));
                    }
                    spec = spec.seasonal(sp, sd, sq, period);
                }
                (fit_arima_transfer(&y, &spec, &[], &transfer_inputs, &ArimaOptions::default())?, None)
            }
        };
        let levels = request.forecast.sorted_levels();
        let requested = request.forecast.horizon;
        let horizon = inputs
            .iter()
            .zip(&future_inputs)
            .map(|(i, f): (&InputReport, &Vec<f64>)| f.len() + i.delay)
            .fold(requested, usize::min);
        let horizon_note = (horizon < requested).then(|| {
            format!(
                "{} of the {requested} forecasts requested: the inputs' future values (rows after the series \
                 with the inputs filled in) cover {horizon} steps ahead",
                horizon
            )
        });
        let f = fit.forecast_with_inputs(horizon, &[], &future_inputs, &levels)?;
        let back = |v: &[f64]| match lambda {
            Some(l) => inverse_box_cox(v, l),
            None => v.to_vec(),
        };
        // Limits beyond the range of the Box–Cox transform are clipped to its
        // edge (0 for λ > 0, ∞ for λ < 0) instead of turning into NaN.
        let mut clipped = 0;
        let mut back_limits = |v: &[f64]| match lambda {
            Some(l) => {
                let (out, k) = inverse_box_cox_clipped(v, l);
                clipped += k;
                out
            }
            None => v.to_vec(),
        };
        let lower: Vec<Vec<f64>> = f.intervals.iter().map(|i| back_limits(&i.lower)).collect();
        let upper: Vec<Vec<f64>> = f.intervals.iter().map(|i| back_limits(&i.upper)).collect();
        let method_note = match lambda {
            Some(l) => {
                let mut note = format!(
                    "fitted to the Box–Cox transform with λ = {}; forecasts and limits transformed back, so the \
                     point forecast is the median of the forecast distribution; std errors are on the transformed \
                     scale",
                    format_number(l, 4)
                );
                if clipped > 0 {
                    note.push_str(if l > 0.0 {
                        "; lower limits that reach below the range of the transform are truncated at 0"
                    } else {
                        "; upper limits that reach beyond the range of the transform are unbounded (∞)"
                    });
                }
                note
            }
            None => "limits are forecast ± z × std error".to_owned(),
        };
        let forecast = ForecastPath {
            mean: back(&f.mean),
            std_errors: f.std_error.clone(),
            levels: f.intervals.iter().map(|i| i.level).collect(),
            lower,
            upper,
            method_note: Some(method_note),
        };
        let fitted = back(&fit.fitted);
        Ok(Self {
            request: request.clone(),
            series,
            period,
            period_note,
            lambda,
            fit,
            fitted,
            search,
            forecast,
            inputs,
            horizon_note,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &ArimaRequest {
        &self.request
    }

    /// The fitted model (on the transformed scale when Box–Cox is used).
    #[must_use]
    pub fn fit(&self) -> &ArimaFit {
        &self.fit
    }

    /// The Box–Cox λ used, if any.
    #[must_use]
    pub fn lambda(&self) -> Option<f64> {
        self.lambda
    }

    /// The forecasts on the data's scale.
    #[must_use]
    pub fn forecast(&self) -> &ForecastPath {
        &self.forecast
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let mut t = format!("ARIMA: {} — {}", self.series.name, self.fit.spec.label());
        if !self.inputs.is_empty() {
            let names: Vec<&str> = self.inputs.iter().map(|i| i.name.as_str()).collect();
            t.push_str(&format!(" with input{} {}", if names.len() == 1 { "" } else { "s" }, names.join(", ")));
        }
        t
    }

    /// The table "Save forecasts" adds.
    ///
    /// # Errors
    /// Only if the table cannot be built.
    pub fn forecast_table(&self, table: &DataTable) -> Result<DataTable> {
        let time = self.request.series.time.and_then(|t| table.column(t));
        forecast_data_table(&self.series, &self.fitted, &self.forecast, &self.fit.spec.label(), time)
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary()];
        if self.search.is_some() {
            out.push(self.search_table());
        }
        out.push(self.coefficients());
        if !self.inputs.is_empty() {
            out.push(self.transfer_table());
            for (k, i) in self.inputs.iter().enumerate() {
                out.push(self.impulse_plot(k, i));
            }
        }
        let label = self.fit.spec.label();
        out.push(forecast_plot(&self.series, &self.fitted, &self.forecast, &label));
        out.push(forecast_table(&self.series, &self.forecast));
        let residual_label =
            if self.lambda.is_some() { "Residual (Box–Cox scale)" } else { "Residual (standardized innovation)" };
        out.push(residual_plot(&self.series, &self.fit.residuals, residual_label));
        out.extend(residual_diagnostics(&self.fit.residuals, self.period, self.fit.spec.arma_count()));
        for i in &self.inputs {
            out.push(self.residual_input_check(i));
        }
        out.push(accuracy_table(&accuracy(&self.series.values, &self.fitted)));
        out
    }

    fn summary(&self) -> ReportSection {
        let f = &self.fit;
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut row = |k: &str, v: String| t.push([k.to_owned(), v]);
        row("Model", f.spec.label());
        row("Observations", self.series.len().to_string());
        row("Used in the likelihood (after differencing)", f.n_used.to_string());
        row("Seasonal period", format!("{} ({})", self.period, self.period_note));
        row(
            "Box–Cox λ",
            match (self.lambda, self.request.box_cox) {
                (None, _) => "none (data analysed as they are)".to_owned(),
                (Some(l), BoxCoxChoice::Guerrero) => format!("{} (Guerrero's method)", format_number(l, 4)),
                (Some(l), _) => format_number(l, 4),
            },
        );
        row("σ² (innovation variance)", num(f.sigma2));
        row("σ", num(f.sigma2.sqrt()));
        row("Log-likelihood", num(f.log_likelihood));
        row("AIC", num(f.aic));
        row("AICc", if f.aicc.is_finite() { num(f.aicc) } else { "— (too few observations)".to_owned() });
        row("BIC", num(f.bic));
        row("Smallest AR/MA root modulus", {
            let m = f.min_root_modulus();
            if m.is_finite() { format_number(m, 4) } else { "— (no AR or MA terms)".to_owned() }
        });
        for i in &self.inputs {
            row(
                &format!("Input {}", i.name),
                format!(
                    "delay {}{}, numerator order {}, denominator order {}",
                    i.delay,
                    if i.delay_from_data { " (from the prewhitened cross-correlation)" } else { "" },
                    i.numerator,
                    i.denominator
                ),
            );
        }
        if let Some(note) = &self.horizon_note {
            row("Forecasts", note.clone());
        }
        if !f.converged {
            row("Note", "the optimiser did not meet its convergence criterion; treat the estimates with care".into());
        }
        let mut title = format!("Model Summary ({}", self.series.name);
        if self.series.n_trimmed > 0 {
            title.push_str(&format!("; {} rows before or after the series excluded or empty", self.series.n_trimmed));
        }
        title.push(')');
        section(SectionKind::ForecastModelSummary, title, SectionContent::Table(t))
    }

    fn search_table(&self) -> ReportSection {
        let Some((d, sd, candidates)) = &self.search else {
            return section(SectionKind::ForecastModelComparison, "Model Search", SectionContent::Note(String::new()));
        };
        let best = candidates.iter().map(|c| c.aicc).fold(f64::INFINITY, f64::min);
        let mut sorted: Vec<(usize, &CandidateModel)> = candidates.iter().enumerate().collect();
        sorted.sort_by(|a, b| a.1.aicc.total_cmp(&b.1.aicc).then(a.0.cmp(&b.0)));
        let mut t = TextTable::new(["Model", "AICc", "ΔAICc", "Order tried", "Chosen"]);
        for (k, c) in sorted {
            let ok = c.aicc.is_finite();
            t.push([
                c.spec.label(),
                if ok { num(c.aicc) } else { "rejected (failed, or a root near the unit circle)".to_owned() },
                if ok { format_number(c.aicc - best, 4) } else { String::new() },
                (k + 1).to_string(),
                if c.spec == self.fit.spec { "✓ chosen".to_owned() } else { String::new() },
            ]);
        }
        let stepwise = matches!(self.request.orders, ArimaOrders::Automatic { stepwise: true, .. });
        section(
            SectionKind::ForecastModelComparison,
            format!(
                "Model Search ({} models fitted by {} search, ranked by AICc; differencing d = {d} chosen by KPSS \
                 tests, seasonal D = {sd} by the strength of seasonality)",
                candidates.len(),
                if stepwise { "stepwise" } else { "exhaustive" }
            ),
            SectionContent::Table(t),
        )
    }

    fn coefficients(&self) -> ReportSection {
        let f = &self.fit;
        let df = f.n_used.saturating_sub(f.coefficients.len()).max(1) as f64;
        let t_dist = StudentsT::new(df).ok();
        let mut t = TextTable::new(["Term", "Meaning", "Estimate", "Std error", "t ratio", "p-value"]);
        for ((name, &b), &se) in f.coefficient_names.iter().zip(&f.coefficients).zip(&f.std_errors) {
            let ratio = b / se;
            let p = match (&t_dist, ratio.is_finite()) {
                (Some(d), true) => d.sf(ratio.abs()).map_or(f64::NAN, |q| 2.0 * q),
                _ => f64::NAN,
            };
            t.push([name.clone(), meaning(name), num(b), num(se), format_number(ratio, 4), format_p_value(p)]);
        }
        if f.coefficients.is_empty() {
            t.push([
                "(none)".to_owned(),
                "the model has no coefficients beyond σ²".to_owned(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
            ]);
        }
        section(
            SectionKind::ForecastParameters,
            format!(
                "Coefficients (exact maximum likelihood; std errors from the inverse Hessian; t ratios with {} \
                 degrees of freedom; MA terms enter with a plus sign)",
                format_number(df, 6)
            ),
            SectionContent::Table(t),
        )
    }
}

/// The series, each input's values on its rows, and each input's future values.
type SeriesWithInputs = (Series, Vec<Vec<f64>>, Vec<Vec<f64>>);

/// The series and input values on the rows where all are present, and each
/// input's values on the following rows (up to the first row that is excluded
/// or lacks an input value), which the forecasts use.
fn read_with_inputs(table: &DataTable, rows: &RowStateManager, request: &ArimaRequest) -> Result<SeriesWithInputs> {
    let series_in = &request.series;
    if series_in.time == Some(series_in.column) {
        return Err(invalid("choose a different column for the time"));
    }
    let mut columns = vec![series_in.column];
    for i in &request.inputs {
        if i.column == series_in.column {
            return Err(invalid("an input must be a different column from the series"));
        }
        if Some(i.column) == series_in.time {
            return Err(invalid("an input must be a different column from the time"));
        }
        if columns.contains(&i.column) {
            return Err(invalid("each input column can be used once"));
        }
        columns.push(i.column);
    }
    let mut aligned = Series::extract_aligned(table, rows, &columns, series_in.time, series_in.period)?;
    let series = aligned.remove(0);
    if series.len() < 3 {
        return Err(invalid(format!("{} has {} values; at least 3 are needed", series.name, series.len())));
    }
    let inputs: Vec<Vec<f64>> = aligned.into_iter().map(|s| s.values).collect();
    let last = series.rows.last().copied().unwrap_or(0);
    let numeric: Vec<&[f64]> =
        request.inputs.iter().filter_map(|i| table.column(i.column).and_then(|c| c.as_numeric())).collect();
    let mut future = vec![Vec::new(); numeric.len()];
    for r in last + 1..table.n_rows() {
        if rows.is_excluded(r) || numeric.iter().any(|v| !v[r].is_finite()) {
            break;
        }
        for (f, v) in future.iter_mut().zip(&numeric) {
            f.push(v[r]);
        }
    }
    Ok((series, inputs, future))
}

/// Stems at `(k, v)` with a zero line, as plot segments.
fn stems(points: &[(f64, f64)], x_max: f64) -> Vec<Vec<[f64; 2]>> {
    let mut out: Vec<Vec<[f64; 2]>> = points.iter().map(|&(k, v)| vec![[k, 0.0], [k, v]]).collect();
    out.push(vec![[-0.5, 0.0], [x_max + 0.5, 0.0]]);
    out
}

impl ArimaAnalysis {
    fn transfer_table(&self) -> ReportSection {
        let mut t = TextTable::new(["Input", "Transfer function", "Delay b", "Steady-state gain", "Input mean"]);
        for (i, e) in self.inputs.iter().zip(&self.fit.transfer) {
            let mut numer = num(e.omega[0]);
            for (j, w) in e.omega.iter().enumerate().skip(1) {
                let sign = if *w >= 0.0 { "−" } else { "+" };
                numer.push_str(&format!(" {sign} {}·B{}", num_abs(*w), power(j)));
            }
            let mut den = "1".to_owned();
            for (j, d) in e.delta.iter().enumerate() {
                let sign = if *d >= 0.0 { "−" } else { "+" };
                den.push_str(&format!(" {sign} {}·B{}", num_abs(*d), power(j + 1)));
            }
            let lag = if i.delay > 0 { format!("B{} ", power(i.delay)) } else { String::new() };
            let form = if e.delta.is_empty() {
                if e.omega.len() > 1 { format!("({numer}) {lag}x") } else { format!("{numer} {lag}x") }
            } else {
                format!("({numer}) / ({den}) {lag}x")
            };
            let gain = e.steady_state_gain();
            t.push([
                i.name.clone(),
                form,
                format!("{}{}", i.delay, if i.delay_from_data { " (from the data)" } else { "" }),
                if gain.is_finite() { num(gain) } else { "— (no steady state)".to_owned() },
                num(e.input_mean),
            ]);
        }
        section(
            SectionKind::ForecastTransferFunction,
            "Transfer Functions (x is the input minus its mean; B shifts back one observation; the gain is the \
             eventual change in the series per unit step in the input)",
            SectionContent::Table(t),
        )
    }

    fn impulse_plot(&self, k: usize, i: &InputReport) -> ReportSection {
        let e = &self.fit.transfer[k];
        let lags = i.identification.as_ref().map_or(12.max(i.delay + 8), |r| r.lags.len().saturating_sub(1));
        let fitted = e.impulse_response(lags);
        let fitted_pts: Vec<(f64, f64)> = fitted.iter().enumerate().map(|(j, &v)| (j as f64, v)).collect();
        let mut items: Vec<(f64, f64, String, usize)> =
            fitted_pts.iter().map(|&(x, y)| (x, y, String::new(), 0)).collect();
        let mut groups = vec!["Fitted weight".to_owned()];
        let mut band = None;
        let mut title = format!("Impulse Response: {} to {} (stems: fitted weights", self.series.name, i.name);
        if let Some(r) = &i.identification {
            groups.push("Prewhitened estimate".to_owned());
            items.extend(r.lags.iter().zip(&r.weights).map(|(&j, &w)| (j as f64, w, String::new(), 1)));
            let ratio = if r.correlations[0] != 0.0 { r.weights[0] / r.correlations[0] } else { f64::NAN };
            if ratio.is_finite() {
                let lim = r.limit * ratio.abs();
                let x1 = lags as f64 + 0.5;
                band = Some((vec![[-0.5, -lim], [x1, -lim]], vec![[-0.5, lim], [x1, lim]]));
            }
            let (d, sd, m) = i.differencing;
            let diff = match (d, sd) {
                (0, 0) => "undifferenced".to_owned(),
                (d, 0) => format!("both differenced {d}×"),
                (0, sd) => format!("both seasonally differenced {sd}× (period {m})"),
                (d, sd) => format!("both differenced {d}× and seasonally {sd}× (period {m})"),
            };
            title.push_str(&format!(
                "; points: estimates from the cross-correlation after prewhitening the input by an AR({}) \
                 ({diff}); shaded: 95% limits; ",
                r.prewhitened.filter.order
            ));
            match r.suggested_delay {
                Some(b) => title.push_str(&format!("first significant lag {b}")),
                None => title.push_str("no lag significant"),
            }
        } else if let Some(err) = &i.identification_error {
            title.push_str(&format!("; prewhitened estimates not computed: {err}"));
        }
        title.push(')');
        let spec = PlotSpec {
            x_label: "Lag (observations after the input)".to_owned(),
            y_label: format!("Weight ({} per unit of {})", self.series.name, i.name),
            band,
            band_name: Some("95% limits for a zero weight".to_owned()),
            annotations: Some(Annotations { groups, items, labels: Vec::new() }),
            segments: stems(&fitted_pts, lags as f64),
            ..PlotSpec::default()
        };
        section(SectionKind::ForecastImpulseResponse, title, SectionContent::Plot(Box::new(spec)))
    }

    /// Cross-correlation of the residuals with the prewhitened input at lags
    /// 0…K (input leading), with BJR's check `S = m Σ r²` on `K + 1 − (r + s + 1)` df.
    fn residual_input_check(&self, i: &InputReport) -> ReportSection {
        let kind = SectionKind::ForecastInputResidualCcf;
        let title = format!("Residual Cross-Correlation with Prewhitened {}", i.name);
        // The input differenced as the fitted model differences the series, prewhitened by an autoregression.
        let spec = self.fit.spec;
        let prewhitened = difference(&i.values, spec.d, spec.seasonal_d, spec.period.max(1))
            .and_then(|dx| yule_walker(&dx, None, None).map(|ar| ar.filter(&dx)));
        let alpha = match prewhitened {
            Ok(a) => a,
            Err(e) => return section(kind, title, SectionContent::Note(format!("Not computed: {e}"))),
        };
        let n = self.fit.residuals.len();
        let start = n - alpha.len();
        let resid = &self.fit.residuals[start..];
        // Residuals start after the model's differencing, which can exceed the identification's.
        let ok = resid.iter().position(|v| v.is_finite()).unwrap_or(resid.len());
        let (alpha, resid) = (&alpha[ok..], &resid[ok..]);
        let max_lag = (alpha.len() / 4).clamp(1, 24);
        let ccf = match cross_correlation(alpha, resid, Some(max_lag)) {
            Ok(c) => c,
            Err(e) => return section(kind, title, SectionContent::Note(format!("Not computed: {e}"))),
        };
        let pts: Vec<(f64, f64)> =
            ccf.lags.iter().zip(&ccf.values).filter(|(l, _)| **l <= 0).map(|(&l, &v)| ((-l) as f64, v)).collect();
        let limit = ccf.white_noise_limit(0.95).unwrap_or(f64::NAN);
        let s_stat = ccf.n as f64 * pts.iter().map(|(_, v)| v * v).sum::<f64>();
        let df = (max_lag + 1).saturating_sub(i.numerator + 1 + i.denominator);
        let p =
            if df > 0 { ChiSquared::new(df as f64).and_then(|c| c.sf(s_stat)).unwrap_or(f64::NAN) } else { f64::NAN };
        let x1 = max_lag as f64 + 0.5;
        let spec = PlotSpec {
            x_label: "Lag (observations after the input)".to_owned(),
            y_label: "Cross-correlation".to_owned(),
            band: Some((vec![[-0.5, -limit], [x1, -limit]], vec![[-0.5, limit], [x1, limit]])),
            band_name: Some("95% white-noise limits (±1.96/√n)".to_owned()),
            annotations: Some(Annotations {
                groups: vec!["Cross-correlation".to_owned()],
                items: pts.iter().map(|&(x, y)| (x, y, String::new(), 0)).collect(),
                labels: Vec::new(),
            }),
            segments: stems(&pts, max_lag as f64),
            ..PlotSpec::default()
        };
        let verdict = if df == 0 {
            String::new()
        } else {
            format!(
                "; S = n Σ r² = {} on {df} DF, p = {}{}",
                format_number(s_stat, 4),
                format_p_value(p),
                if p < 0.05 { ": the transfer function misses part of the input's effect" } else { "" }
            )
        };
        section(
            kind,
            format!("{title} (lags 0–{max_lag}; should stay within the limits{verdict})"),
            SectionContent::Plot(Box::new(spec)),
        )
    }
}

fn num_abs(v: f64) -> String {
    num(v.abs())
}

/// Superscript exponent for `B`, omitted for 1.
fn power(k: usize) -> String {
    if k == 1 {
        return String::new();
    }
    const SUP: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
    k.to_string().chars().map(|c| SUP[c.to_digit(10).unwrap_or(0) as usize]).collect()
}

/// Plain meaning of a coefficient name such as `ar1`, `sma2` or `drift`.
fn meaning(name: &str) -> String {
    if let Some((input, coef)) = name.rsplit_once(' ') {
        if let Some(j) = coef.strip_prefix('ω').and_then(|k| k.parse::<usize>().ok()) {
            return if j == 0 {
                format!("transfer numerator: immediate weight of {input}")
            } else {
                format!("transfer numerator, lag {j} of {input} (enters with a minus sign)")
            };
        }
        if let Some(j) = coef.strip_prefix('δ').and_then(|k| k.parse::<usize>().ok()) {
            return format!("transfer denominator, lag {j} (decay of {input}'s effect)");
        }
    }
    let lag = |prefix: &str| name.strip_prefix(prefix).and_then(|k| k.parse::<usize>().ok());
    if let Some(k) = lag("sar") {
        format!("seasonal autoregressive, lag {k} cycles")
    } else if let Some(k) = lag("sma") {
        format!("seasonal moving average, lag {k} cycles")
    } else if let Some(k) = lag("ar") {
        format!("autoregressive, lag {k}")
    } else if let Some(k) = lag("ma") {
        format!("moving average, lag {k}")
    } else if name == "intercept" || name == "mean" {
        "mean of the series".to_owned()
    } else if name == "drift" {
        "drift: change per observation".to_owned()
    } else {
        "regression".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::reactor_temperature;

    fn request(t: &DataTable) -> ArimaRequest {
        ArimaRequest {
            series: SeriesInput { column: t.find("Temperature (°C)").unwrap(), time: t.find("Hour"), period: Some(1) },
            ..ArimaRequest::new(0)
        }
    }

    #[test]
    fn automatic_search_differences_once() {
        let t = reactor_temperature();
        let rows = RowStateManager::new(t.n_rows());
        let a = ArimaAnalysis::run(&t, &rows, &request(&t)).unwrap();
        assert_eq!((a.fit().spec.p, a.fit().spec.d, a.fit().spec.q), (1, 1, 1));
        let kinds: Vec<SectionKind> = a.sections().iter().map(|s| s.kind).collect();
        for k in [
            SectionKind::ForecastModelSummary,
            SectionKind::ForecastModelComparison,
            SectionKind::ForecastParameters,
            SectionKind::ForecastPlot,
            SectionKind::ForecastTable,
            SectionKind::ForecastResidualPlot,
            SectionKind::ForecastResidualAcf,
            SectionKind::ForecastLjungBox,
            SectionKind::ForecastAccuracy,
        ] {
            assert!(kinds.contains(&k), "{k:?} missing");
        }
        let f = a.forecast();
        assert_eq!(f.mean.len(), ForecastSettings::DEFAULT_HORIZON);
        assert!(f.lower[1][0] < f.lower[0][0] && f.upper[0][0] < f.upper[1][0]);
    }

    #[test]
    fn chosen_orders_with_box_cox_back_transform() {
        let t = reactor_temperature();
        let rows = RowStateManager::new(t.n_rows());
        let r = ArimaRequest {
            orders: ArimaOrders::Chosen { regular: (1, 1, 1), seasonal: (0, 0, 0), constant: false },
            box_cox: BoxCoxChoice::Lambda(0.0),
            ..request(&t)
        };
        let a = ArimaAnalysis::run(&t, &rows, &r).unwrap();
        assert_eq!(a.fit().coefficient_names, vec!["ar1".to_owned(), "ma1".to_owned()]);
        assert_eq!(a.lambda(), Some(0.0));
        // Forecasts are on the data's scale (hundreds of degrees, not logs).
        assert!(a.forecast().mean[0] > 300.0);
        let saved = a.forecast_table(&t).unwrap();
        assert_eq!(saved.n_rows(), t.n_rows() + ForecastSettings::DEFAULT_HORIZON);
        // A seasonal part without a period is an error.
        let seasonal = ArimaRequest {
            orders: ArimaOrders::Chosen { regular: (0, 1, 1), seasonal: (0, 1, 1), constant: false },
            ..request(&t)
        };
        assert!(ArimaAnalysis::run(&t, &rows, &seasonal).is_err());
    }

    /// A small positive series at λ = 0.5 whose prediction limits reach below
    /// the transform's range (−1/λ = −2): the lower limits are truncated at
    /// 0, never NaN, and the note says so.
    #[test]
    fn box_cox_limits_beyond_the_transform_range_are_clipped() {
        let y: Vec<f64> = (0..12).map(|i| if i % 3 == 2 { 2.0 } else { 1.0 }).collect();
        let t = DataTable::new("s", vec![stat_data::Column::numeric("y", y)]).unwrap();
        let rows = RowStateManager::new(t.n_rows());
        let mut r = ArimaRequest::new(0);
        r.series.period = Some(1);
        r.orders = ArimaOrders::Chosen { regular: (0, 1, 0), seasonal: (0, 0, 0), constant: false };
        r.box_cox = BoxCoxChoice::Lambda(0.5);
        r.forecast.horizon = 12;
        let a = ArimaAnalysis::run(&t, &rows, &r).unwrap();
        let f = a.forecast();
        for (lower, upper) in f.lower.iter().zip(&f.upper) {
            for h in 0..f.mean.len() {
                assert!(lower[h].is_finite() && lower[h] >= -1e-9, "{lower:?}");
                assert!(upper[h].is_finite() && upper[h] >= f.mean[h] - 1e-9, "{upper:?}");
            }
        }
        assert!(f.lower.iter().flatten().any(|v| v.abs() < 1e-9), "some lower limit is clipped to 0");
        assert!(f.method_note.as_deref().unwrap().contains("truncated at 0"));
        let plot = crate::forecasting::forecast_plot(&a.series, &a.fitted, f, "ARIMA");
        let SectionContent::Plot(spec) = plot.content else { panic!("a plot") };
        let (lo, hi) = spec.band.unwrap();
        assert!(lo.iter().chain(&hi).all(|p| p[1].is_finite()));
    }

    fn kiln() -> (DataTable, ArimaRequest) {
        let t = crate::sample::kiln_fuel_feed();
        let mut r = ArimaRequest::new(t.find("Burning-zone temperature (°C)").unwrap());
        r.series.time = t.find("Hour");
        r.series.period = Some(1);
        r.forecast.horizon = 24;
        r.inputs = vec![TransferRole::new(t.find("Fuel feed (t/h)").unwrap())];
        (t, r)
    }

    #[test]
    fn transfer_function_input_recovers_the_simulated_lag() {
        let (t, r) = kiln();
        let rows = RowStateManager::new(t.n_rows());
        let a = ArimaAnalysis::run(&t, &rows, &r).unwrap();
        let e = &a.fit().transfer[0];
        assert_eq!(e.delay, 2, "delay from the prewhitened cross-correlation");
        assert!((e.omega[0] - 4.0).abs() < 0.8, "ω0 = {}", e.omega[0]);
        assert!((e.delta[0] - 0.8).abs() < 0.08, "δ1 = {}", e.delta[0]);
        assert!((e.steady_state_gain() - 20.0).abs() < 4.0, "gain {}", e.steady_state_gain());
        assert_eq!(a.fit().spec.d, 0, "stationary noise");
        // 300 observed hours; the planned feed covers the 24-hour horizon.
        assert_eq!(a.series.len(), 300);
        assert_eq!(a.forecast().mean.len(), 24);
        assert!(a.horizon_note.is_none());
        assert!(a.title().contains("with input Fuel feed"));
        let kinds: Vec<SectionKind> = a.sections().iter().map(|s| s.kind).collect();
        for k in [
            SectionKind::ForecastTransferFunction,
            SectionKind::ForecastImpulseResponse,
            SectionKind::ForecastInputResidualCcf,
        ] {
            assert!(kinds.contains(&k), "{k:?} missing");
        }
        let check = a.sections().into_iter().find(|s| s.kind == SectionKind::ForecastInputResidualCcf).unwrap();
        assert!(check.title.contains("S = "), "{}", check.title);
        assert!(!check.title.contains("misses"), "{}", check.title);
        assert!(meaning("Fuel feed (t/h) ω0").starts_with("transfer numerator"));
        assert!(meaning("Fuel feed (t/h) δ1").starts_with("transfer denominator"));
    }

    #[test]
    fn transfer_function_horizon_follows_the_future_inputs() {
        let (t, mut r) = kiln();
        // Exclude the last 20 planned rows: 4 future values plus the delay of 2 cover 6 steps.
        let mut rows = RowStateManager::new(t.n_rows());
        rows.set_excluded(304..t.n_rows(), true);
        r.inputs[0].delay = Some(2);
        r.orders = ArimaOrders::Chosen { regular: (1, 0, 0), seasonal: (0, 0, 0), constant: true };
        let a = ArimaAnalysis::run(&t, &rows, &r).unwrap();
        assert_eq!(a.forecast().mean.len(), 6);
        assert!(a.horizon_note.as_deref().unwrap().starts_with("6 of the 24"));
        assert_eq!(a.fit().coefficient_names.last().unwrap(), "Fuel feed (t/h) δ1");
        // The series itself cannot be an input.
        r.inputs[0].column = r.series.column;
        assert!(ArimaAnalysis::run(&t, &rows, &r).is_err());
    }

    #[test]
    fn json_round_trip() {
        let t = reactor_temperature();
        let mut r = request(&t);
        assert_eq!(ArimaRequest::from_json(&r.to_json(&t), &t).unwrap(), r);
        r.orders = ArimaOrders::Chosen { regular: (2, 1, 0), seasonal: (0, 1, 1), constant: true };
        r.box_cox = BoxCoxChoice::Guerrero;
        r.series.period = Some(24);
        assert_eq!(ArimaRequest::from_json(&r.to_json(&t), &t).unwrap(), r);
        r.box_cox = BoxCoxChoice::Lambda(0.5);
        r.orders = ArimaOrders::Automatic { stepwise: false, allow_constant: false };
        assert_eq!(ArimaRequest::from_json(&r.to_json(&t), &t).unwrap(), r);
    }

    #[test]
    fn json_round_trip_with_inputs() {
        let (t, mut r) = kiln();
        assert_eq!(ArimaRequest::from_json(&r.to_json(&t), &t).unwrap(), r);
        r.inputs[0] = TransferRole { delay: Some(3), numerator: 2, denominator: 0, ..r.inputs[0] };
        assert_eq!(ArimaRequest::from_json(&r.to_json(&t), &t).unwrap(), r);
        assert_eq!(r.columns().len(), 3);
    }
}
