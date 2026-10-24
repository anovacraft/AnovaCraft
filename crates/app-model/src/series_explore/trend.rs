//! Trend models against time and moving-average forecasts.

use stat_core::Result;
use stat_timeseries::trend::{AccuracyMeasures, TrendModel, fit_trend, moving_average_forecast};

use super::{TrendChoice, TrendSettings, invalid, num, percent, section};
use crate::format::{format_number, format_optional};
use crate::report::{PlotSpec, Polyline, ReportSection, SectionContent, SectionKind, TextTable};
use crate::report_columns::ReportColumn;
use crate::time_series_data::Series;

/// Fitted values (NaN where there is none), forecasts and limits.
struct Fit {
    fitted: Vec<f64>,
    forecast: Vec<f64>,
    limits: Option<(Vec<f64>, Vec<f64>)>,
    accuracy: AccuracyMeasures,
    coefficients: TextTable,
    equation: String,
}

fn signed(v: f64) -> String {
    if v < 0.0 { format!("− {}", num(-v)) } else { format!("+ {}", num(v)) }
}

fn fit(series: &Series, s: &TrendSettings) -> Result<Fit> {
    let y = &series.values;
    if s.model == TrendChoice::MovingAverage {
        if s.span == 0 {
            return Err(invalid("the moving-average span must be at least 1"));
        }
        let ma = moving_average_forecast(y, s.span)?;
        let mut coefficients = TextTable::new(["Term", "Value"]);
        coefficients.push(["Span (observations averaged)".to_owned(), s.span.to_string()]);
        coefficients.push(["Forecast of every future period".to_owned(), num(ma.forecast)]);
        return Ok(Fit {
            fitted: ma.fitted.iter().map(|v| v.unwrap_or(f64::NAN)).collect(),
            forecast: vec![ma.forecast; s.horizon],
            limits: None,
            accuracy: ma.accuracy,
            coefficients,
            equation: format!(
                "Each period's forecast is the mean of the {} observations before it; every future period is \
                 forecast by the mean of the last {}: {}.",
                s.span,
                s.span,
                num(ma.forecast)
            ),
        });
    }
    let model = match s.model {
        TrendChoice::Linear => TrendModel::Linear,
        TrendChoice::Quadratic => TrendModel::Quadratic,
        TrendChoice::Exponential => TrendModel::Exponential,
        _ => TrendModel::Logistic,
    };
    let f = fit_trend(y, model)?;
    let fc = f.forecast(s.horizon, s.confidence)?;
    let c = &f.coefficients;
    let se = &f.std_errors;
    let mut coefficients = TextTable::new(["Term", "Estimate", "Std error", "Meaning"]);
    let mut row = |term: &str, estimate: f64, error: f64, meaning: String| {
        coefficients.push([term.to_owned(), num(estimate), format_number(error, 4), meaning]);
    };
    let equation = match model {
        TrendModel::Linear => {
            row("β₀", c[0], se[0], "Level at t = 0 (one period before the first observation)".to_owned());
            row("β₁", c[1], se[1], "Change per period".to_owned());
            format!("ŷ = {} {}·t", num(c[0]), signed(c[1]))
        }
        TrendModel::Quadratic => {
            row("β₀", c[0], se[0], "Level at t = 0".to_owned());
            row("β₁", c[1], se[1], "Slope at t = 0".to_owned());
            row("β₂", c[2], se[2], "Curvature (half the change in slope per period)".to_owned());
            format!("ŷ = {} {}·t {}·t²", num(c[0]), signed(c[1]), signed(c[2]))
        }
        TrendModel::Exponential => {
            row("b₀", c[0], se[0], "Level at t = 0 (std error of ln b₀)".to_owned());
            row(
                "b₁",
                c[1],
                se[1],
                format!(
                    "Growth factor per period: {}% per period (std error of ln b₁)",
                    format_number((c[1] - 1.0) * 100.0, 4)
                ),
            );
            format!("ŷ = {} × {}ᵗ (fitted by least squares on ln y)", num(c[0]), num(c[1]))
        }
        TrendModel::Logistic => {
            row("c", c[0], se[0], "Upper asymptote (saturation level)".to_owned());
            row("a", c[1], se[1], "Position: the curve is at c/(1 + a) at t = 0".to_owned());
            row("b", c[2], se[2], "Rate: 1/b periods is the curve's time scale".to_owned());
            let mid = c[1].ln() / c[2];
            let mid_text = if mid.is_finite() && mid >= 0.5 && mid <= series.len() as f64 + 0.5 {
                format!(" ({})", series.x_text(series.x[(mid.round() as usize).clamp(1, series.len()) - 1]))
            } else {
                String::new()
            };
            coefficients.push([
                "Midpoint t = ln(a)/b".to_owned(),
                num(mid),
                String::new(),
                format!("Period of fastest growth, where the curve reaches c/2{mid_text}"),
            ]);
            format!("ŷ = {} / (1 + {}·e^(−{}·t))", num(c[0]), num(c[1]), num(c[2]))
        }
    };
    Ok(Fit {
        fitted: f.fitted.clone(),
        forecast: fc.mean,
        limits: fc.lower.zip(fc.upper),
        accuracy: f.accuracy,
        coefficients,
        equation: format!("{equation}, with t = 1 at the first observation ({})", series.x_text(series.x[0])),
    })
}

pub(super) fn run(
    series: &Series,
    s: &TrendSettings,
    n_rows: usize,
) -> Result<(Vec<ReportSection>, Vec<ReportColumn>)> {
    if s.horizon == 0 {
        return Err(invalid("forecast at least one period"));
    }
    let f = fit(series, s)?;
    let future = series.future_x(s.horizon);
    let level = percent(s.confidence);
    let mut line: Polyline = series.polyline(&f.fitted);
    line.extend(future.iter().zip(&f.forecast).map(|(&x, &v)| [x, v]));
    let band = f.limits.as_ref().map(|(lo, hi)| {
        let anchor = line.iter().rev().find(|p| p[0] <= *series.x.last().unwrap_or(&0.0)).copied();
        let mut lower: Polyline = anchor.into_iter().collect();
        let mut upper: Polyline = anchor.into_iter().collect();
        lower.extend(future.iter().zip(lo).map(|(&x, &v)| [x, v]));
        upper.extend(future.iter().zip(hi).map(|(&x, &v)| [x, v]));
        (lower, upper)
    });
    let ys = series
        .values
        .iter()
        .chain(&f.forecast)
        .chain(f.limits.iter().flat_map(|(a, b)| a.iter().chain(b)))
        .copied()
        .filter(|v| v.is_finite());
    let (lo, hi) = ys.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)));
    let last = *series.x.last().unwrap_or(&0.0);
    let boundary = future.first().map_or(last, |&x| 0.5 * (last + x));
    let what = if s.model == TrendChoice::MovingAverage { "one-step moving-average forecasts" } else { "fitted curve" };
    let spec = PlotSpec {
        x_label: series.x_label.clone(),
        y_label: series.name.clone(),
        points: series.points(&series.values),
        point_name: Some("Observations".to_owned()),
        line: Some(line),
        line_name: Some(format!("{} ({what}, then forecasts)", s.model.label())),
        band_name: band.as_ref().map(|_| format!("{level} prediction limits")),
        band,
        x_time: series.x_time,
        segments: if lo.is_finite() { vec![vec![[boundary, lo], [boundary, hi]]] } else { Vec::new() },
        ..PlotSpec::default()
    };
    let limits_note = if f.limits.is_some() {
        format!("; dashed: {level} prediction limits")
    } else if s.model == TrendChoice::Logistic {
        "; no prediction limits for the logistic curve".to_owned()
    } else {
        "; no prediction limits for moving-average forecasts".to_owned()
    };
    let mut out = vec![section(
        SectionKind::TrendModelPlot,
        format!(
            "{} (markers: observations; line: {what}; forecasts after the thin vertical rule{limits_note})",
            series.name
        ),
        SectionContent::Plot(Box::new(spec)),
    )];
    out.push(section(SectionKind::TrendModelEquation, "Fitted Equation", SectionContent::Note(f.equation.clone())));
    out.push(section(
        SectionKind::TrendModelCoefficients,
        if s.model == TrendChoice::MovingAverage { "Moving Average" } else { "Coefficients" },
        SectionContent::Table(f.coefficients.clone()),
    ));
    let a = &f.accuracy;
    let mut t = TextTable::new(["Measure", "Value"]);
    t.push(["MAPE (mean absolute percentage error, %)".to_owned(), format_optional(a.mape, 5)]);
    t.push(["MAD (mean absolute deviation)".to_owned(), num(a.mad)]);
    t.push(["MSD (mean squared deviation)".to_owned(), num(a.msd)]);
    t.push(["RMSE (root mean squared error)".to_owned(), num(a.rmse)]);
    t.push(["Mean error (bias)".to_owned(), num(a.mean_error)]);
    t.push(["Periods compared".to_owned(), a.n.to_string()]);
    out.push(section(
        SectionKind::TrendModelAccuracy,
        if s.model == TrendChoice::MovingAverage {
            "Accuracy of the One-Step Forecasts (smaller is better; compare models on the same series)"
        } else {
            "Accuracy of the Fitted Values (smaller is better; compare models on the same series)"
        },
        SectionContent::Table(t),
    ));
    let lower = format!("Lower {level}");
    let upper = format!("Upper {level}");
    let mut columns = vec!["Periods ahead", "Time", "Forecast"];
    if f.limits.is_some() {
        columns.push(&lower);
        columns.push(&upper);
    }
    let mut t = TextTable::new(columns);
    for (k, (&x, &v)) in future.iter().zip(&f.forecast).enumerate() {
        let mut row = vec![(k + 1).to_string(), series.x_text(x), num(v)];
        if let Some((lo, hi)) = &f.limits {
            row.push(num(lo[k]));
            row.push(num(hi[k]));
        }
        t.push(row);
    }
    out.push(section(SectionKind::TrendModelForecast, "Forecasts", SectionContent::Table(t)));
    let residuals: Vec<f64> = series.values.iter().zip(&f.fitted).map(|(y, f)| y - f).collect();
    let name = &series.name;
    let columns = vec![
        ReportColumn::continuous(
            "fitted",
            "Save fitted values",
            format!("{} fit {name}", s.model.label()),
            series.to_column(&f.fitted, n_rows),
        ),
        ReportColumn::continuous(
            "residual",
            "Save residuals",
            format!("{} residual {name}", s.model.label()),
            series.to_column(&residuals, n_rows),
        ),
    ];
    Ok((out, columns))
}
