//! Cross-correlation of an input and an output series.
//!
//! The engine's `r_xy(k)` correlates `x_{t+k}` with `y_t`. With the input
//! as `x` and the output as `y`, the report's lag `ℓ = −k` pairs the input
//! at time `t` with the output at time `t + ℓ`, so a peak at a positive lag
//! means the output follows the input.

use stat_core::Result;
use stat_timeseries::autocorrelation::{cross_correlation, default_max_lag, prewhitened_cross_correlation};

use super::{CrossSettings, Prewhiten, invalid, needle_plot, percent, section};
use crate::format::format_number;
use crate::report::{ReportSection, SectionContent, SectionKind, TextTable};
use crate::time_series_data::Series;

pub(super) fn sections(input: &Series, output: &Series, c: &CrossSettings) -> Result<Vec<ReportSection>> {
    let n = input.len();
    if n < 4 {
        return Err(invalid(format!("too few observations ({n}) for a cross-correlation")));
    }
    let x = &input.values;
    let y = &output.values;
    let (ccf, filter) = match c.prewhiten {
        Prewhiten::None => {
            let lag = c.max_lag.map(|k| k.clamp(1, n - 1));
            (cross_correlation(x, y, lag)?, None)
        }
        Prewhiten::Aic | Prewhiten::Order(_) => {
            let order = match c.prewhiten {
                Prewhiten::Order(p) => Some(p),
                _ => None,
            };
            let left = n.saturating_sub(order.unwrap_or(0));
            let lag = c.max_lag.map(|k| k.clamp(1, left.saturating_sub(1).max(1)));
            let p = prewhitened_cross_correlation(x, y, lag, order)?;
            (p.ccf, Some(p.filter))
        }
    };
    let limit = ccf.white_noise_limit(c.confidence)?;
    // Report lag ℓ = −k (input at t, output at t + ℓ), in increasing order.
    let mut pairs: Vec<(f64, f64)> = ccf.lags.iter().zip(&ccf.values).map(|(&k, &r)| (-(k as f64), r)).collect();
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let lags: Vec<f64> = pairs.iter().map(|p| p.0).collect();
    let values: Vec<f64> = pairs.iter().map(|p| p.1).collect();
    let level = percent(c.confidence);
    let band = format!("{level} limits ±{} for unrelated white-noise series", format_number(limit, 3));
    let mut out = Vec::new();
    let (best_lag, best) =
        pairs.iter().copied().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap_or((0.0, f64::NAN));
    let direction = if best_lag > 0.0 {
        format!(
            "{} follows {} by {} period{}",
            output.name,
            input.name,
            best_lag,
            if best_lag == 1.0 { "" } else { "s" }
        )
    } else if best_lag < 0.0 {
        format!(
            "{} leads {} by {} period{}",
            output.name,
            input.name,
            -best_lag,
            if best_lag == -1.0 { "" } else { "s" }
        )
    } else {
        "the two move together in the same period".to_owned()
    };
    let mut note = format!(
        "Lag k pairs {input} at time t with {output} at time t + k. A correlation at a positive lag means changes in \
         {input} are followed k periods later by changes in {output} ({input} leads); at a negative lag {output} \
         moves first. The largest correlation, {r}, is at lag {lag}: {direction}{beyond}.",
        input = input.name,
        output = output.name,
        r = format_number(best, 3),
        lag = best_lag,
        beyond = if best.abs() > limit { "" } else { " (but it lies within the limits)" },
    );
    if let Some(f) = &filter {
        note.push_str(&format!(
            " Both series were filtered by an autoregression of order {} fitted to {} (Yule–Walker{}), which turns \
             {} into nearly white noise, so the cross-correlations are not smeared by its own autocorrelation and \
             show the delay more clearly.",
            f.order,
            input.name,
            if c.prewhiten == Prewhiten::Aic { ", order chosen by AIC" } else { "" },
            input.name
        ));
    } else {
        note.push_str(
            " When the input is itself autocorrelated, the peak is smeared over neighbouring lags and the limits \
             are too narrow; prewhitening (in the report's options) corrects for this.",
        );
    }
    out.push(section(SectionKind::CrossCorrelationNote, "How to Read the Lags", SectionContent::Note(note)));
    out.push(section(
        SectionKind::CrossCorrelationPlot,
        format!(
            "Cross-Correlation of {} (time t) with {} (time t + lag){} (dashed: {band})",
            input.name,
            output.name,
            if filter.is_some() { ", prewhitened" } else { "" }
        ),
        SectionContent::Plot(Box::new(needle_plot(
            &lags,
            &values,
            &vec![limit; lags.len()],
            ("Lag (periods by which the output follows the input)", "Cross-correlation"),
            ("Cross-correlation", &band),
        ))),
    ));
    let mut t = TextTable::new(["Lag", "Correlation", "Beyond the limits"]);
    for (lag, r) in &pairs {
        t.push([format_number(*lag, 6), format_number(*r, 4), if r.abs() > limit { "yes" } else { "" }.to_owned()]);
    }
    let default_k = default_max_lag(n, 2);
    let mut title = format!("Cross-Correlations ({} pairs; limit ±{}", ccf.n, format_number(limit, 4));
    if c.max_lag.is_none() {
        title.push_str(&format!("; lags up to {default_k} either way, the default 10·log₁₀(n/2)"));
    }
    title.push(')');
    out.push(section(SectionKind::CrossCorrelationTable, title, SectionContent::Table(t)));
    if let Some(f) = filter {
        let mut t = TextTable::new(["Term", "Value"]);
        t.push(["Order".to_owned(), f.order.to_string()]);
        for (i, phi) in f.coefficients.iter().enumerate() {
            t.push([format!("φ{}", i + 1), format_number(*phi, 5)]);
        }
        t.push(["Innovation variance".to_owned(), format_number(f.innovation_variance, 5)]);
        out.push(section(
            SectionKind::CrossCorrelationFilter,
            format!("Prewhitening Filter (autoregression fitted to {})", input.name),
            SectionContent::Table(t),
        ));
    }
    Ok(out)
}
