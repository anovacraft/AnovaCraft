//! Shared parts of the Time Series menu's forecasting reports (exponential
//! smoothing and seasonal ARIMA): the sections every forecasting model
//! shows — the forecast plot and table, residual plot, residual
//! autocorrelation with white-noise limits, the Ljung–Box test with the
//! fitted parameters taken off its degrees of freedom, and in-sample
//! accuracy — plus the table "Save forecasts" adds to the project. The series
//! itself (rows, time stamps, seasonal period, forecast time stamps) comes
//! from [`crate::time_series_data`].

use crate::format::{format_number, format_p_value};
use crate::project::invalid;
use crate::report::{Annotations, PlotSpec, ReportSection, RowPoint, SectionContent, SectionKind, TextTable};
pub use crate::time_series_data::{Series, SeriesInput};
use stat_core::Result;
use stat_data::{Column, DataTable, JsonValue, RowStateManager};
use stat_timeseries::autocorrelation::{PortmanteauKind, acf, portmanteau};

const DIGITS: usize = 5;

pub(crate) fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

pub(crate) fn num(v: f64) -> String {
    format_number(v, DIGITS)
}

/// Read the series of `input` ([`Series::extract`]), which a model needs at
/// least 3 values of.
///
/// # Errors
/// As [`Series::extract`], or fewer than 3 values.
pub fn read_series(table: &DataTable, rows: &RowStateManager, input: &SeriesInput) -> Result<Series> {
    if input.time == Some(input.column) {
        return Err(invalid("choose a different column for the time"));
    }
    let series = Series::extract(table, rows, input)?;
    if series.len() < 3 {
        return Err(invalid(format!("{} has {} values; at least 3 are needed", series.name, series.len())));
    }
    Ok(series)
}

/// How the seasonal period of `series` was chosen, for the model summary.
#[must_use]
pub fn period_note(series: &Series) -> String {
    match (series.period_from_time, series.period) {
        (false, _) => "as chosen".to_owned(),
        (true, 1) => "no seasonal cycle recognised from the time stamps".to_owned(),
        (true, _) => "from the time column's spacing".to_owned(),
    }
}

/// Forecasts `1..=h` steps ahead with prediction intervals, whatever the model.
#[derive(Debug, Clone, PartialEq)]
pub struct ForecastPath {
    /// Point forecasts.
    pub mean: Vec<f64>,
    /// Forecast standard deviations.
    pub std_errors: Vec<f64>,
    /// Coverage of each interval, ascending.
    pub levels: Vec<f64>,
    /// `lower[k][h−1]` at `levels[k]`.
    pub lower: Vec<Vec<f64>>,
    /// `upper[k][h−1]` at `levels[k]`.
    pub upper: Vec<Vec<f64>>,
    /// How the intervals were obtained, for the report (e.g. "simulated").
    pub method_note: Option<String>,
}

/// Percent label of a level, such as "95%".
#[must_use]
pub fn level_label(level: f64) -> String {
    format!("{}%", format_number(level * 100.0, 4))
}

/// Settings common to every forecasting report.
#[derive(Debug, Clone, PartialEq)]
pub struct ForecastSettings {
    /// Forecasts made (steps ahead).
    pub horizon: usize,
    /// Prediction-interval coverages, each in (0, 1).
    pub levels: Vec<f64>,
}

impl ForecastSettings {
    /// Default horizon.
    pub const DEFAULT_HORIZON: usize = 12;
    /// Default interval levels.
    pub const DEFAULT_LEVELS: [f64; 2] = [0.80, 0.95];

    /// Check the settings.
    ///
    /// # Errors
    /// A zero horizon, more than 10 000 steps, no levels or a level outside (0, 1).
    pub fn validate(&self) -> Result<()> {
        if self.horizon == 0 || self.horizon > 10_000 {
            return Err(invalid("forecast between 1 and 10 000 steps ahead"));
        }
        if self.levels.is_empty() || self.levels.iter().any(|l| !(*l > 0.0 && *l < 1.0)) {
            return Err(invalid("prediction interval levels must lie strictly between 0 and 1"));
        }
        Ok(())
    }

    /// The levels sorted, without repeats.
    #[must_use]
    pub fn sorted_levels(&self) -> Vec<f64> {
        let mut l = self.levels.clone();
        l.sort_by(f64::total_cmp);
        l.dedup();
        l
    }

    pub(crate) fn push_json(&self, m: &mut Vec<(String, JsonValue)>) {
        m.push(("horizon".into(), JsonValue::number(self.horizon as f64)));
        m.push(("levels".into(), JsonValue::Array(self.levels.iter().map(|&l| JsonValue::number(l)).collect())));
    }

    pub(crate) fn from_json(v: &JsonValue) -> Self {
        Self {
            horizon: v.get("horizon").and_then(JsonValue::as_usize).unwrap_or(Self::DEFAULT_HORIZON),
            levels: v
                .get("levels")
                .and_then(JsonValue::as_array)
                .map_or_else(|| Self::DEFAULT_LEVELS.to_vec(), |a| a.iter().filter_map(JsonValue::as_f64).collect()),
        }
    }
}

impl Default for ForecastSettings {
    fn default() -> Self {
        Self { horizon: Self::DEFAULT_HORIZON, levels: Self::DEFAULT_LEVELS.to_vec() }
    }
}

/// In-sample accuracy of one-step fitted values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InSampleAccuracy {
    /// Root mean squared error.
    pub rmse: f64,
    /// Mean absolute error.
    pub mae: f64,
    /// Mean absolute percentage error (`None` when a value is 0).
    pub mape: Option<f64>,
}

/// Accuracy of `fitted` one-step values against `actual`: `RMSE = √(Σd²/n)`,
/// `MAE = Σ|d|/n`, `MAPE = 100 Σ|d/y|/n` with `d = y − ŷ`, over the
/// observations with a fitted value (a differenced model has none for its
/// first `d + mD`).
#[must_use]
pub fn accuracy(actual: &[f64], fitted: &[f64]) -> InSampleAccuracy {
    let (actual, fitted): (Vec<f64>, Vec<f64>) =
        actual.iter().zip(fitted).filter(|(y, f)| y.is_finite() && f.is_finite()).map(|(y, f)| (*y, *f)).unzip();
    let n = actual.len().max(1) as f64;
    let d: Vec<f64> = actual.iter().zip(&fitted).map(|(y, f)| y - f).collect();
    let mape = actual
        .iter()
        .all(|y| *y != 0.0)
        .then(|| 100.0 * actual.iter().zip(&d).map(|(y, e)| (e / y).abs()).sum::<f64>() / n);
    InSampleAccuracy {
        rmse: (d.iter().map(|e| e * e).sum::<f64>() / n).sqrt(),
        mae: d.iter().map(|e| e.abs()).sum::<f64>() / n,
        mape,
    }
}

/// The forecast plot: observations (tied to rows), one-step fitted values
/// continued by the forecasts, forecast markers, the widest interval as a
/// band and narrower ones as thin lines, and a rule at the forecast origin.
#[must_use]
pub fn forecast_plot(series: &Series, fitted: &[f64], f: &ForecastPath, model: &str) -> ReportSection {
    let future = series.future_x(f.mean.len());
    let mut line: Vec<[f64; 2]> =
        series.x.iter().zip(fitted).filter(|(_, v)| v.is_finite()).map(|(&x, &v)| [x, v]).collect();
    let anchor = [*series.x.last().unwrap_or(&0.0), *series.values.last().unwrap_or(&0.0)];
    line.extend(future.iter().zip(&f.mean).map(|(&x, &v)| [x, v]));
    let all = series.values.iter().chain(f.lower.iter().flatten()).chain(f.upper.iter().flatten()).chain(&f.mean);
    let (lo, hi) =
        all.filter(|v| v.is_finite()).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
    // An unbounded limit (a Box–Cox back-transform beyond the transform's
    // range) is drawn at the edge of the finite values.
    let finite = |v: f64| {
        if v.is_finite() {
            v
        } else if v > 0.0 {
            hi
        } else {
            lo
        }
    };
    let limits = |k: usize| -> (Vec<[f64; 2]>, Vec<[f64; 2]>) {
        let lo = std::iter::once(anchor).chain(future.iter().zip(&f.lower[k]).map(|(&x, &v)| [x, finite(v)])).collect();
        let hi = std::iter::once(anchor).chain(future.iter().zip(&f.upper[k]).map(|(&x, &v)| [x, finite(v)])).collect();
        (lo, hi)
    };
    let widest = f.levels.len().checked_sub(1);
    let band = widest.map(limits);
    let mut segments = Vec::new();
    for k in 0..widest.unwrap_or(0) {
        let (lo, hi) = limits(k);
        segments.push(lo);
        segments.push(hi);
    }
    // The forecast origin: half a step after the last observation.
    let origin = match future.first() {
        Some(&next) => 0.5 * (anchor[0] + next),
        None => anchor[0],
    };
    if lo.is_finite() && hi.is_finite() {
        segments.push(vec![[origin, lo], [origin, hi]]);
    }
    let annotations = Annotations {
        groups: vec!["Forecast".to_owned()],
        items: future.iter().zip(&f.mean).map(|(&x, &v)| (x, v, String::new(), 0)).collect(),
        labels: Vec::new(),
    };
    let widest_label = widest.map(|k| level_label(f.levels[k]));
    let spec = PlotSpec {
        x_label: series.x_label.clone(),
        y_label: series.name.clone(),
        points: series.points(&series.values),
        point_name: Some("Observed".to_owned()),
        line: Some(line),
        line_name: Some("One-step fitted, then forecast".to_owned()),
        band: band.clone(),
        band_name: widest_label.as_ref().map(|l| format!("{l} prediction limits")),
        annotations: Some(annotations),
        x_time: series.x_time,
        segments,
        ..PlotSpec::default()
    };
    let mut title = format!(
        "Forecast ({model}; circles: observed; solid line: one-step fitted values, continuing as the forecast \
         (diamonds) after the vertical rule"
    );
    if let Some(l) = &widest_label {
        title.push_str(&format!("; dashed: {l} prediction limits"));
    }
    if f.levels.len() > 1 {
        let inner: Vec<String> = f.levels[..f.levels.len() - 1].iter().map(|&l| level_label(l)).collect();
        title.push_str(&format!("; thin lines: {} limits", inner.join(", ")));
    }
    title.push(')');
    section(SectionKind::ForecastPlot, title, SectionContent::Plot(Box::new(spec)))
}

/// The forecast table: one row per step ahead.
#[must_use]
pub fn forecast_table(series: &Series, f: &ForecastPath) -> ReportSection {
    let future = series.future_x(f.mean.len());
    let mut header = vec!["Step".to_owned(), series.x_label.clone(), "Forecast".to_owned(), "Std error".to_owned()];
    for &l in &f.levels {
        header.push(format!("Lower {}", level_label(l)));
        header.push(format!("Upper {}", level_label(l)));
    }
    let mut t = TextTable::new(header);
    for h in 0..f.mean.len() {
        let mut row = vec![(h + 1).to_string(), series.x_text(future[h]), num(f.mean[h]), num(f.std_errors[h])];
        for k in 0..f.levels.len() {
            row.push(num(f.lower[k][h]));
            row.push(num(f.upper[k][h]));
        }
        t.push(row);
    }
    let mut title = format!("Forecasts ({} steps ahead", f.mean.len());
    if let Some(note) = &f.method_note {
        title.push_str("; ");
        title.push_str(note);
    }
    title.push(')');
    section(SectionKind::ForecastTable, title, SectionContent::Table(t))
}

/// Residuals over time, tied to rows, with a zero line.
#[must_use]
pub fn residual_plot(series: &Series, residuals: &[f64], label: &str) -> ReportSection {
    let points = series
        .rows
        .iter()
        .zip(&series.x)
        .zip(residuals)
        .filter(|(_, e)| e.is_finite())
        .map(|((&row, &x), &y)| RowPoint { row, x, y })
        .collect();
    let (x0, x1) = (series.x.first().copied().unwrap_or(0.0), series.x.last().copied().unwrap_or(1.0));
    let spec = PlotSpec {
        x_label: series.x_label.clone(),
        y_label: label.to_owned(),
        points,
        point_name: Some("Residual".to_owned()),
        line: Some(vec![[x0, 0.0], [x1, 0.0]]),
        x_time: series.x_time,
        ..PlotSpec::default()
    };
    section(SectionKind::ForecastResidualPlot, format!("{label} over Time"), SectionContent::Plot(Box::new(spec)))
}

/// Lags pooled by the Ljung–Box test: `2m` (at most `n/5`) for a seasonal
/// series, otherwise 10 (at most `n/5`), and always more than the fitted
/// parameters (Hyndman & Athanasopoulos, *Forecasting: Principles and
/// Practice*, 3rd ed., §5.4).
#[must_use]
pub fn ljung_box_lag(n: usize, period: usize, fitted_parameters: usize) -> usize {
    let base = if period > 1 { 2 * period } else { 10 };
    base.min(n / 5).max(fitted_parameters + 1).min(n.saturating_sub(1))
}

/// Residual autocorrelation plot (spikes with markers, ±z/√n white-noise
/// limits) and the Ljung–Box test with `fitted_parameters` taken off its
/// degrees of freedom.
#[must_use]
pub fn residual_diagnostics(residuals: &[f64], period: usize, fitted_parameters: usize) -> Vec<ReportSection> {
    let e: Vec<f64> = residuals.iter().copied().filter(|v| v.is_finite()).collect();
    let n = e.len();
    let max_lag = if period > 1 { (2 * period).max(10) } else { 20 }.min(n.saturating_sub(1));
    let mut out = Vec::new();
    match acf(&e, Some(max_lag.max(1))) {
        Ok(r) => {
            let limit = r.white_noise_limit(0.95).unwrap_or(f64::NAN);
            let k_max = r.max_lag() as f64;
            let spikes: Vec<Vec<[f64; 2]>> =
                (1..=r.max_lag()).map(|k| vec![[k as f64, 0.0], [k as f64, r.values[k]]]).collect();
            let annotations = Annotations {
                groups: vec!["Autocorrelation".to_owned()],
                items: (1..=r.max_lag()).map(|k| (k as f64, r.values[k], String::new(), 0)).collect(),
                labels: Vec::new(),
            };
            let mut segments = spikes;
            segments.push(vec![[0.5, 0.0], [k_max + 0.5, 0.0]]);
            let spec = PlotSpec {
                x_label: "Lag".to_owned(),
                y_label: "Residual autocorrelation".to_owned(),
                band: Some((vec![[0.5, -limit], [k_max + 0.5, -limit]], vec![[0.5, limit], [k_max + 0.5, limit]])),
                band_name: Some("95% white-noise limits (±1.96/√n)".to_owned()),
                annotations: Some(annotations),
                segments,
                ..PlotSpec::default()
            };
            out.push(section(
                SectionKind::ForecastResidualAcf,
                format!(
                    "Residual Autocorrelation (lags 1–{}; dashed: 95% limits ±{} for white noise)",
                    r.max_lag(),
                    format_number(limit, 3)
                ),
                SectionContent::Plot(Box::new(spec)),
            ));
        }
        Err(err) => out.push(section(
            SectionKind::ForecastResidualAcf,
            "Residual Autocorrelation",
            SectionContent::Note(format!("Not computed: {err}")),
        )),
    }
    let h = ljung_box_lag(n, period, fitted_parameters);
    let mut lags = vec![h];
    if period > 1 && period > fitted_parameters && period < h {
        lags.insert(0, period);
    }
    match portmanteau(&e, &lags, fitted_parameters, PortmanteauKind::LjungBox) {
        Ok(tests) => {
            let mut t = TextTable::new(["Lags pooled", "Q (Ljung–Box)", "DF", "p-value"]);
            for test in tests {
                t.push([test.lag.to_string(), num(test.statistic), test.df.to_string(), format_p_value(test.p_value)]);
            }
            out.push(section(
                SectionKind::ForecastLjungBox,
                format!(
                    "Ljung–Box Test of the Residuals (H₀: no autocorrelation up to the lag; degrees of freedom \
                     reduced by the {fitted_parameters} fitted model parameters; small p-values mean structure \
                     is left in the residuals)"
                ),
                SectionContent::Table(t),
            ));
        }
        Err(err) => out.push(section(
            SectionKind::ForecastLjungBox,
            "Ljung–Box Test of the Residuals",
            SectionContent::Note(format!("Not computed: {err}")),
        )),
    }
    out
}

/// In-sample accuracy table.
#[must_use]
pub fn accuracy_table(a: &InSampleAccuracy) -> ReportSection {
    let mut t = TextTable::new(["Measure", "Value"]);
    t.push(["RMSE (root mean squared error)".to_owned(), num(a.rmse)]);
    t.push(["MAE (mean absolute error)".to_owned(), num(a.mae)]);
    t.push([
        "MAPE (mean absolute percentage error, %)".to_owned(),
        a.mape.map_or_else(|| "— (a value is 0)".to_owned(), num),
    ]);
    section(
        SectionKind::ForecastAccuracy,
        "In-Sample Accuracy (one-step-ahead errors y − ŷ over the fitted series)",
        SectionContent::Table(t),
    )
}

/// The table "Save forecasts" adds to the project: the time, the observed
/// value and one-step fitted value for each observation, then one row per
/// forecast with its limits at each level.
///
/// # Errors
/// Only if the table cannot be built (columns of unequal length cannot occur).
pub fn forecast_data_table(
    series: &Series,
    fitted: &[f64],
    f: &ForecastPath,
    model: &str,
    time_column: Option<&Column>,
) -> Result<DataTable> {
    let n = series.len();
    let h = f.mean.len();
    let future = series.future_x(h);
    let mut time: Vec<f64> = series.x.clone();
    time.extend(&future);
    let nan = |k: usize| std::iter::repeat_n(f64::NAN, k);
    let time_name = series.x_label.clone();
    let time_col = match (series.x_time, time_column) {
        (Some(display), _) => Column::date_time(time_name, time, display),
        _ => Column::numeric(time_name, time),
    };
    let actual: Vec<f64> = series.values.iter().copied().chain(nan(h)).collect();
    let fit: Vec<f64> = fitted.iter().copied().chain(nan(h)).collect();
    let forecast: Vec<f64> = nan(n).chain(f.mean.iter().copied()).collect();
    let mut columns = vec![
        time_col,
        Column::numeric(format!("{} (actual)", series.name), actual),
        Column::numeric("Fitted (one-step)", fit),
        Column::numeric("Forecast", forecast),
        Column::numeric("Forecast std error", nan(n).chain(f.std_errors.iter().copied()).collect()),
    ];
    for (k, &l) in f.levels.iter().enumerate() {
        let label = level_label(l);
        columns.push(Column::numeric(format!("Lower {label}"), nan(n).chain(f.lower[k].iter().copied()).collect()));
        columns.push(Column::numeric(format!("Upper {label}"), nan(n).chain(f.upper[k].iter().copied()).collect()));
    }
    DataTable::new(format!("{} forecasts ({model})", series.name), columns)
}

/// The forecast table of a forecasting report (exponential smoothing or
/// ARIMA), computed again from its request: what "Save forecasts as data
/// table" adds to the project. `None` for other reports.
#[must_use]
pub fn saved_forecast_table(
    saved: &crate::SavedReport,
    table: &DataTable,
    rows: &RowStateManager,
) -> Option<Result<DataTable>> {
    match saved {
        crate::SavedReport::ExponentialSmoothing { request } => {
            Some(crate::SmoothingAnalysis::run(table, rows, request).and_then(|a| a.forecast_table(table)))
        }
        crate::SavedReport::Arima { request } => {
            Some(crate::ArimaAnalysis::run(table, rows, request).and_then(|a| a.forecast_table(table)))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accuracy_measures() {
        let a = accuracy(&[2.0, 4.0], &[1.0, 5.0]);
        assert!((a.rmse - 1.0).abs() < 1e-12 && (a.mae - 1.0).abs() < 1e-12);
        assert!((a.mape.unwrap() - 37.5).abs() < 1e-12);
        assert!(accuracy(&[0.0, 1.0], &[0.5, 1.0]).mape.is_none());
        // Observations without a fitted value are left out.
        assert_eq!(accuracy(&[5.0, 2.0], &[f64::NAN, 2.0]).rmse, 0.0);
    }
}
