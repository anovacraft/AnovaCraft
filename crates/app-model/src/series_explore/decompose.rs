//! Decomposition into trend, seasonal and remainder (classical or STL).

use stat_core::Result;
use stat_timeseries::decompose::{
    Decomposition, DecompositionType, StlOptions, classical_decomposition, seasonal_strength, stl_fit,
};

use super::{DecomposeMethod, DecomposeSettings, invalid, num, section, series_plot};
use crate::format::format_number;
use crate::report::{PlotSpec, ReportSection, SectionContent, SectionKind, TextTable};
use crate::report_columns::ReportColumn;
use crate::time_series_data::Series;

fn variance(x: &[f64]) -> f64 {
    let n = x.len() as f64;
    let m = x.iter().sum::<f64>() / n;
    x.iter().map(|v| (v - m).powi(2)).sum::<f64>() / (n - 1.0)
}

/// Strength of trend `max(0, 1 − var(R)/var(T + R))` (Wang, Smith &
/// Hyndman 2006) over the points where both are defined.
fn trend_strength(d: &Decomposition) -> f64 {
    let (r, tr): (Vec<f64>, Vec<f64>) = d
        .remainder
        .iter()
        .zip(&d.trend)
        .filter(|(r, t)| r.is_finite() && t.is_finite())
        .map(|(r, t)| (*r, r + t))
        .unzip();
    if r.len() < 2 {
        return f64::NAN;
    }
    let denominator = variance(&tr);
    if denominator <= 0.0 { 0.0 } else { (1.0 - variance(&r) / denominator).max(0.0) }
}

fn strength_words(s: f64) -> &'static str {
    if !s.is_finite() {
        "—"
    } else if s >= 0.9 {
        "very strong"
    } else if s >= 0.64 {
        "strong"
    } else if s >= 0.3 {
        "moderate"
    } else {
        "weak"
    }
}

pub(super) fn run(
    series: &Series,
    d: &DecomposeSettings,
    n_rows: usize,
) -> Result<(Vec<ReportSection>, Vec<ReportColumn>)> {
    let m = series.period;
    if m < 2 {
        return Err(invalid(
            "a decomposition needs a seasonal period of at least 2: choose a time column with regular dates or set \
             the observations per cycle",
        ));
    }
    let y = &series.values;
    let multiplicative = d.method == DecomposeMethod::ClassicalMultiplicative;
    let (components, indices, stl) = match d.method {
        DecomposeMethod::ClassicalAdditive | DecomposeMethod::ClassicalMultiplicative => {
            let kind = if multiplicative { DecompositionType::Multiplicative } else { DecompositionType::Additive };
            let c = classical_decomposition(y, m, kind)?;
            (c.components, Some(c.indices), None)
        }
        DecomposeMethod::Stl => {
            let options = StlOptions { seasonal_window: d.seasonal_window, robust: d.robust, ..StlOptions::new(m) };
            let fit = stl_fit(y, &options)?;
            let spans = (fit.seasonal_window, fit.trend_window, fit.lowpass_window);
            (fit.decomposition, None, Some(spans))
        }
    };
    // Strengths from the additive form (logarithms of a multiplicative one).
    let additive = if multiplicative {
        let ln = |v: &[f64]| v.iter().map(|x| x.ln()).collect::<Vec<f64>>();
        Decomposition {
            period: m,
            trend: ln(&components.trend),
            seasonal: ln(&components.seasonal),
            remainder: ln(&components.remainder),
        }
    } else {
        components.clone()
    };
    let (fs, ft) = (seasonal_strength(&additive), trend_strength(&additive));
    let mut out = Vec::new();
    let mut summary = TextTable::new(["Measure", "Value", "Reading"]);
    summary.push(["Strength of trend".to_owned(), format_number(ft, 3), strength_words(ft).to_owned()]);
    summary.push(["Strength of seasonality".to_owned(), format_number(fs, 3), strength_words(fs).to_owned()]);
    summary.push(["Observations per cycle".to_owned(), m.to_string(), String::new()]);
    summary.push(["Complete cycles".to_owned(), (y.len() / m).to_string(), String::new()]);
    if let Some((seasonal, trend, lowpass)) = stl {
        let s = seasonal.map_or_else(|| "periodic (every cycle the same)".to_owned(), |w| w.to_string());
        summary.push(["Seasonal smoothing span".to_owned(), s, String::new()]);
        summary.push(["Trend smoothing span".to_owned(), trend.to_string(), String::new()]);
        summary.push(["Low-pass span".to_owned(), lowpass.to_string(), String::new()]);
        summary.push([
            "Robustness iterations".to_owned(),
            if d.robust { "yes (outliers downweighted)" } else { "no" }.to_owned(),
            String::new(),
        ]);
    }
    let model = if multiplicative {
        "observed = trend × seasonal × remainder"
    } else {
        "observed = trend + seasonal + remainder"
    };
    out.push(section(
        SectionKind::DecompositionSummary,
        format!(
            "Decomposition Summary ({model}; strengths are 1 − var(remainder)/var(component + remainder){}, from 0 \
             to 1)",
            if multiplicative { " on the log scale" } else { "" }
        ),
        SectionContent::Table(summary),
    ));
    out.push(section(
        SectionKind::DecompositionObserved,
        "Observed",
        SectionContent::Plot(Box::new(series_plot(series, y, &series.name, "Observations"))),
    ));
    let trend_plot = PlotSpec {
        x_label: series.x_label.clone(),
        y_label: "Trend".to_owned(),
        points: series.points(y),
        point_name: Some("Observations".to_owned()),
        line: Some(series.polyline(&components.trend)),
        line_name: Some("Trend".to_owned()),
        x_time: series.x_time,
        ..PlotSpec::default()
    };
    let classical_note = if indices.is_some() {
        format!(" (centred moving average of {m}; undefined for the first and last {} observations)", m / 2)
    } else {
        String::new()
    };
    out.push(section(
        SectionKind::DecompositionTrend,
        format!("Trend{classical_note}: line, with the observations as markers"),
        SectionContent::Plot(Box::new(trend_plot)),
    ));
    let seasonal_plot = PlotSpec {
        x_label: series.x_label.clone(),
        y_label: "Seasonal".to_owned(),
        line: Some(series.polyline(&components.seasonal)),
        line_name: Some("Seasonal component".to_owned()),
        x_time: series.x_time,
        segments: vec![series.polyline(&vec![if multiplicative { 1.0 } else { 0.0 }; y.len()])],
        ..PlotSpec::default()
    };
    out.push(section(
        SectionKind::DecompositionSeasonal,
        format!("Seasonal (thin rule: {})", if multiplicative { "factor 1, no seasonal effect" } else { "zero" }),
        SectionContent::Plot(Box::new(seasonal_plot)),
    ));
    let centre = if multiplicative { 1.0 } else { 0.0 };
    let remainder_plot = PlotSpec {
        x_label: series.x_label.clone(),
        y_label: "Remainder".to_owned(),
        points: series.points(&components.remainder),
        point_name: Some("Remainder".to_owned()),
        line: Some(vec![[series.x[0], centre], [*series.x.last().unwrap_or(&series.x[0]), centre]]),
        line_name: Some(if multiplicative { "No remainder (factor 1)" } else { "Zero" }.to_owned()),
        x_time: series.x_time,
        ..PlotSpec::default()
    };
    out.push(section(
        SectionKind::DecompositionRemainder,
        "Remainder (what trend and season leave unexplained; look for outliers and leftover pattern)",
        SectionContent::Plot(Box::new(remainder_plot)),
    ));
    if let Some(ix) = &indices {
        let mut t = TextTable::new([
            "Position in cycle",
            "Season",
            if multiplicative { "Seasonal index (factor)" } else { "Seasonal index (added)" },
        ]);
        for (j, v) in ix.iter().enumerate() {
            t.push([(j + 1).to_string(), series.season_label(j), num(*v)]);
        }
        out.push(section(
            SectionKind::DecompositionIndices,
            if multiplicative {
                "Seasonal Indices (average 1; 1.10 means 10% above the trend in that season)"
            } else {
                "Seasonal Indices (sum to 0; the amount each season lies above or below the trend)"
            },
            SectionContent::Table(t),
        ));
    }
    let adjusted: Vec<f64> =
        y.iter().zip(&components.seasonal).map(|(v, s)| if multiplicative { v / s } else { v - s }).collect();
    let name = &series.name;
    let columns = vec![
        ReportColumn::continuous(
            "trend",
            "Save trend component",
            format!("Trend {name}"),
            series.to_column(&components.trend, n_rows),
        ),
        ReportColumn::continuous(
            "seasonal",
            "Save seasonal component",
            format!("Seasonal {name}"),
            series.to_column(&components.seasonal, n_rows),
        ),
        ReportColumn::continuous(
            "remainder",
            "Save remainder",
            format!("Remainder {name}"),
            series.to_column(&components.remainder, n_rows),
        ),
        ReportColumn::continuous(
            "adjusted",
            "Save seasonally adjusted series",
            format!("Seasonally adjusted {name}"),
            series.to_column(&adjusted, n_rows),
        ),
    ];
    Ok((out, columns))
}
