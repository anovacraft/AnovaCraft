//! Series description: plot, transformation, ACF/PACF, Ljung–Box,
//! unit-root tests and the spectrum.

use stat_core::Result;
use stat_timeseries::autocorrelation::{PortmanteauKind, acf, default_max_lag, pacf, portmanteau};
use stat_timeseries::spectral::{Spectrum, SpectrumOptions, spectral_density};
use stat_timeseries::transform::{box_cox, difference, guerrero_lambda};
use stat_timeseries::unit_root::{AdfDeterministic, AdfOptions, KpssNull, adf, kpss};

use super::{BoxCoxChoice, DescribeSettings, invalid, needle_plot, num, percent, section, series_plot};
use crate::format::{format_number, format_p_value};
use crate::report::{Annotations, PlotSpec, Polyline, ReportSection, SectionContent, SectionKind, TextTable};
use crate::time_series_data::Series;

/// Significance level of the plain-language conclusions.
const ALPHA: f64 = 0.05;

fn mean_sd(v: &[f64]) -> (f64, f64) {
    let n = v.len() as f64;
    let m = v.iter().sum::<f64>() / n;
    let s = if v.len() > 1 { (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n - 1.0)).sqrt() } else { f64::NAN };
    (m, s)
}

/// Lags of the Ljung–Box tests: multiples of the period (or of 5) below `n`.
fn ljung_box_lags(n: usize, period: usize, max_lag: usize) -> Vec<usize> {
    let base = if period > 1 { period } else { 5 };
    let mut lags: Vec<usize> = (1..=4).map(|k| k * base).filter(|&h| h < n.saturating_sub(1)).collect();
    if lags.is_empty() {
        lags.push(max_lag.clamp(1, n.saturating_sub(2).max(1)));
    }
    lags
}

/// The analysed series after the transformation and differencing, with the
/// description of what was done.
struct Transformed {
    values: Vec<f64>,
    /// Observations lost to differencing (at the start).
    lost: usize,
    lambda: Option<f64>,
    description: Vec<String>,
}

fn transform(series: &Series, d: &DescribeSettings) -> Result<Transformed> {
    let m = series.period;
    let mut description = Vec::new();
    let (values, lambda) = match d.box_cox {
        BoxCoxChoice::None => (series.values.clone(), None),
        choice => {
            if series.values.iter().any(|&v| v <= 0.0) {
                return Err(invalid("a Box–Cox transformation needs every value to be positive"));
            }
            let (lambda, how) = match choice {
                BoxCoxChoice::Lambda(l) => (l, "as set"),
                _ => (guerrero_lambda(&series.values, m, -1.0, 2.0)?, "chosen by Guerrero's method"),
            };
            description.push(if lambda == 0.0 {
                format!("Box–Cox λ = 0 ({how}): natural logarithm")
            } else {
                format!("Box–Cox λ = {} ({how}): (y^λ − 1)/λ", format_number(lambda, 4))
            });
            (box_cox(&series.values, lambda)?, Some(lambda))
        }
    };
    if d.seasonal_differences > 0 && m < 2 {
        return Err(invalid("seasonal differencing needs a seasonal period of at least 2"));
    }
    let lost = d.differences + m * d.seasonal_differences;
    if lost + 3 > values.len() {
        return Err(invalid(format!(
            "too few observations ({}) for {} differences",
            values.len(),
            if d.seasonal_differences > 0 { "these regular and seasonal" } else { "these" }
        )));
    }
    let values = difference(&values, d.differences, d.seasonal_differences, m)?;
    if d.differences > 0 {
        description.push(format!("{} regular difference{}", d.differences, if d.differences == 1 { "" } else { "s" }));
    }
    if d.seasonal_differences > 0 {
        description.push(format!(
            "{} seasonal difference{} at lag {m}",
            d.seasonal_differences,
            if d.seasonal_differences == 1 { "" } else { "s" }
        ));
    }
    Ok(Transformed { values, lost, lambda, description })
}

pub(super) fn sections(series: &Series, d: &DescribeSettings) -> Result<Vec<ReportSection>> {
    let t = transform(series, d)?;
    let w = &t.values;
    let n = w.len();
    if w.iter().all(|&v| (v - w[0]).abs() <= 1e-12 * w[0].abs().max(1.0)) {
        return Err(invalid("the (transformed) series is constant, so it has no autocorrelation"));
    }
    let m = series.period;
    let default_lag = || {
        let k = default_max_lag(n, 1);
        if m > 1 { k.max(2 * m) } else { k }.min(n - 1).max(1)
    };
    let max_lag = d.max_lag.map_or_else(default_lag, |k| k.clamp(1, n - 1));
    let mut out = vec![summary(series, &t, d)];
    out.push(section(
        SectionKind::SeriesPlot,
        "Time Series Plot",
        SectionContent::Plot(Box::new(series_plot(series, &series.values, &series.name, "Observations"))),
    ));
    if !t.description.is_empty() {
        let mut shifted = series.clone();
        shifted.rows.drain(..t.lost);
        shifted.x.drain(..t.lost);
        shifted.values.clone_from(w);
        let label = format!("{} (transformed)", series.name);
        out.push(section(
            SectionKind::SeriesTransformedPlot,
            format!("Transformed Series ({})", t.description.join("; ")),
            SectionContent::Plot(Box::new(series_plot(&shifted, w, &label, "Transformed values"))),
        ));
    }
    let r = acf(w, Some(max_lag))?;
    let p = pacf(w, Some(max_lag))?;
    let white = r.white_noise_limit(d.confidence)?;
    let bartlett = r.bartlett_limits(d.confidence)?;
    let lags: Vec<f64> = (1..=max_lag).map(|k| k as f64).collect();
    let level = percent(d.confidence);
    let acf_limits: Vec<f64> = if d.bartlett { bartlett.clone() } else { vec![white; max_lag] };
    let acf_band = if d.bartlett {
        format!("{level} Bartlett limits (for a moving average up to the previous lag)")
    } else {
        format!("{level} white-noise limits ±{}", format_number(white, 3))
    };
    out.push(section(
        SectionKind::SeriesAcfPlot,
        format!("Autocorrelation Function (ACF; dashed: {acf_band})"),
        SectionContent::Plot(Box::new(needle_plot(
            &lags,
            &r.values[1..],
            &acf_limits,
            ("Lag", "Autocorrelation"),
            ("Autocorrelation", &acf_band),
        ))),
    ));
    let pacf_band = format!("{level} white-noise limits ±{}", format_number(white, 3));
    out.push(section(
        SectionKind::SeriesPacfPlot,
        format!("Partial Autocorrelation Function (PACF; dashed: {pacf_band})"),
        SectionContent::Plot(Box::new(needle_plot(
            &lags,
            &p.values,
            &vec![p.white_noise_limit(d.confidence)?; max_lag],
            ("Lag", "Partial autocorrelation"),
            ("Partial autocorrelation", &pacf_band),
        ))),
    ));
    let bartlett_heading = format!("± Bartlett limit ({level})");
    let white_heading = format!("± White-noise limit ({level})");
    let mut columns = vec!["Lag", "Autocorrelation", "Partial autocorrelation", white_heading.as_str()];
    if d.bartlett {
        columns.push(bartlett_heading.as_str());
    }
    let mut table = TextTable::new(columns);
    for k in 1..=max_lag {
        let mut row = vec![k.to_string(), format_number(r.values[k], 4), format_number(p.values[k - 1], 4)];
        row.push(format_number(white, 4));
        if d.bartlett {
            row.push(format_number(bartlett[k - 1], 4));
        }
        table.push(row);
    }
    out.push(section(SectionKind::SeriesAutocorrelationTable, "Autocorrelations", SectionContent::Table(table)));
    let lb_lags = ljung_box_lags(n, m, max_lag);
    let mut lb = TextTable::new(["Up to lag", "Q", "DF", "p-value"]);
    for q in portmanteau(w, &lb_lags, 0, PortmanteauKind::LjungBox)? {
        lb.push([q.lag.to_string(), num(q.statistic), q.df.to_string(), format_p_value(q.p_value)]);
    }
    out.push(section(
        SectionKind::SeriesLjungBox,
        "Ljung–Box Tests (null hypothesis: no autocorrelation up to the lag; a small p-value means the values \
         depend on earlier ones)",
        SectionContent::Table(lb),
    ));
    if d.unit_root {
        out.push(unit_root_table(w, d.adf));
    }
    if d.spectrum {
        out.extend(spectrum_sections(w, m, d)?);
    }
    Ok(out)
}

fn summary(series: &Series, t: &Transformed, d: &DescribeSettings) -> ReportSection {
    let (mean, sd) = mean_sd(&series.values);
    let mut table = TextTable::new(["Statistic", "Value"]);
    table.push(["Observations".to_owned(), series.len().to_string()]);
    table.push([
        "From".to_owned(),
        format!("{} to {}", series.x_text(series.x[0]), series.x_text(*series.x.last().unwrap_or(&series.x[0]))),
    ]);
    table.push(["Mean".to_owned(), num(mean)]);
    table.push(["Standard deviation".to_owned(), num(sd)]);
    table.push([
        "Seasonal period".to_owned(),
        if series.period > 1 {
            format!(
                "{} observations per cycle ({})",
                series.period,
                if series.period_from_time { "from the time column" } else { "as set" }
            )
        } else {
            "none (1)".to_owned()
        },
    ]);
    if t.lambda.is_some() || d.differences + d.seasonal_differences > 0 {
        table.push(["Analysed as".to_owned(), t.description.join("; ")]);
        let (wm, ws) = mean_sd(&t.values);
        table.push(["Values after transformation".to_owned(), t.values.len().to_string()]);
        table.push(["Mean after transformation".to_owned(), num(wm)]);
        table.push(["Standard deviation after transformation".to_owned(), num(ws)]);
    }
    let mut title = "Summary".to_owned();
    if series.n_trimmed > 0 {
        title.push_str(&format!(
            " ({} leading or trailing row{} left out: excluded or missing)",
            series.n_trimmed,
            if series.n_trimmed == 1 { "" } else { "s" }
        ));
    }
    section(SectionKind::SeriesSummary, title, SectionContent::Table(table))
}

fn adf_terms(det: AdfDeterministic) -> &'static str {
    match det {
        AdfDeterministic::None => "none",
        AdfDeterministic::Constant => "constant",
        AdfDeterministic::ConstantTrend => "constant and trend",
    }
}

fn unit_root_table(w: &[f64], det: AdfDeterministic) -> ReportSection {
    let mut t =
        TextTable::new(["Test", "Null hypothesis", "Deterministic terms", "Lag", "Statistic", "p-value", "Conclusion"]);
    let around = match det {
        AdfDeterministic::ConstantTrend => "around a linear trend",
        AdfDeterministic::Constant => "around a constant level",
        AdfDeterministic::None => "around zero",
    };
    match adf(w, &AdfOptions { deterministic: det, ..AdfOptions::default() }) {
        Ok(a) => {
            let conclusion = if a.p_value < ALPHA {
                format!("Unit root rejected: the series looks stationary {around}")
            } else {
                "Unit root not rejected: the series may need differencing".to_owned()
            };
            t.push([
                "Augmented Dickey–Fuller (lag by AIC)".to_owned(),
                "Unit root (not stationary)".to_owned(),
                adf_terms(det).to_owned(),
                a.lag.to_string(),
                format_number(a.statistic, 4),
                format_p_value(a.p_value),
                conclusion,
            ]);
        }
        Err(e) => t.push([
            "Augmented Dickey–Fuller".to_owned(),
            "Unit root (not stationary)".to_owned(),
            adf_terms(det).to_owned(),
            "—".to_owned(),
            "—".to_owned(),
            "—".to_owned(),
            format!("Not computed: {e}"),
        ]),
    }
    for (null, what, terms) in [
        (KpssNull::Level, "Stationary around a level", "constant"),
        (KpssNull::Trend, "Stationary around a linear trend", "constant and trend"),
    ] {
        match kpss(w, null, None) {
            Ok(k) => {
                // The p-value is interpolated in a table and clipped to [0.01, 0.10].
                let p = if k.p_value >= 0.10 {
                    "≥ 0.10".to_owned()
                } else if k.p_value <= 0.01 {
                    "≤ 0.01".to_owned()
                } else {
                    format!("{:.3}", k.p_value)
                };
                let conclusion = if k.p_value < ALPHA {
                    format!("{what} rejected: the series may need differencing")
                } else {
                    format!("Consistent with {}", what.to_lowercase())
                };
                t.push([
                    "KPSS".to_owned(),
                    what.to_owned(),
                    terms.to_owned(),
                    k.lag.to_string(),
                    format_number(k.statistic, 4),
                    p,
                    conclusion,
                ]);
            }
            Err(e) => t.push([
                "KPSS".to_owned(),
                what.to_owned(),
                terms.to_owned(),
                "—".to_owned(),
                "—".to_owned(),
                "—".to_owned(),
                format!("Not computed: {e}"),
            ]),
        }
    }
    section(
        SectionKind::SeriesUnitRoot,
        "Unit-Root and Stationarity Tests (conclusions at the 5% level; the two tests have opposite null \
         hypotheses, so agreement between them is the stronger evidence)",
        SectionContent::Table(t),
    )
}

/// Labelled ticks at powers of ten (and 2 and 5 times them when the range
/// is narrow) covering `[lo, hi]` on a log₁₀ scale.
fn log_ticks(lo: f64, hi: f64) -> Vec<(f64, String)> {
    if !(lo.is_finite() && hi.is_finite()) {
        return Vec::new();
    }
    let (a, b) = (lo.floor() as i32, hi.ceil() as i32);
    let mut out = Vec::new();
    let fine = b - a <= 3;
    for e in a..=b {
        for m in if fine { &[1.0, 2.0, 5.0][..] } else { &[1.0][..] } {
            let v = m * 10f64.powi(e);
            let at = v.log10();
            if at >= lo - 0.05 && at <= hi + 0.05 {
                out.push((at, format_number(v, 3)));
            }
        }
    }
    out
}

fn spectrum_sections(w: &[f64], m: usize, d: &DescribeSettings) -> Result<Vec<ReportSection>> {
    let raw = spectral_density(w, &SpectrumOptions { level: d.confidence, ..SpectrumOptions::default() })?;
    let spans: Vec<usize> = d.spans.iter().copied().filter(|&s| s >= 2).collect();
    let smooth: Option<Spectrum> = if spans.is_empty() {
        None
    } else {
        Some(spectral_density(
            w,
            &SpectrumOptions { spans: spans.clone(), level: d.confidence, ..SpectrumOptions::default() },
        )?)
    };
    let x = |f: f64| if d.period_axis { 1.0 / f } else { f };
    let log = |v: f64| if v > 0.0 { v.log10() } else { f64::NAN };
    let shown = smooth.as_ref().unwrap_or(&raw);
    let line: Polyline = shown
        .frequency
        .iter()
        .zip(&shown.density)
        .map(|(&f, &v)| [x(f), log(v)])
        .filter(|p| p[1].is_finite())
        .collect();
    let curve = |lim: &[f64]| -> Polyline {
        shown.frequency.iter().zip(lim).map(|(&f, &v)| [x(f), log(v)]).filter(|p| p[1].is_finite()).collect()
    };
    let band = (curve(&shown.lower), curve(&shown.upper));
    let items: Vec<(f64, f64, String, usize)> = raw
        .frequency
        .iter()
        .zip(&raw.density)
        .filter(|(_, v)| **v > 0.0)
        .map(|(&f, &v)| (x(f), v.log10(), String::new(), 0))
        .collect();
    let all_y: Vec<f64> = items.iter().map(|i| i.1).chain(band.0.iter().chain(&band.1).map(|p| p[1])).collect();
    let lo = all_y.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = all_y.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    // Thin rules at the seasonal frequency and its harmonics.
    let mut segments = Vec::new();
    if m > 1 && lo.is_finite() {
        for k in 1..=m / 2 {
            let f = k as f64 / m as f64;
            segments.push(vec![[x(f), lo], [x(f), hi]]);
        }
    }
    let smoothing = smooth.as_ref().map(|s| {
        format!(
            "modified Daniell spans {}, bandwidth {}, {} equivalent degrees of freedom",
            spans.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "),
            format_number(s.bandwidth, 3),
            format_number(s.df, 3)
        )
    });
    let spec = PlotSpec {
        x_label: if d.period_axis {
            "Period (observations per cycle)".to_owned()
        } else {
            "Frequency (cycles per observation)".to_owned()
        },
        y_label: "Spectral density (log scale)".to_owned(),
        line: Some(line),
        line_name: Some(if smooth.is_some() { "Smoothed spectrum" } else { "Periodogram" }.to_owned()),
        band: Some(band),
        band_name: Some(format!("{} confidence limits", percent(d.confidence))),
        annotations: Some(Annotations { groups: vec!["Periodogram".to_owned()], items, labels: Vec::new() }),
        y_ticks: Some(log_ticks(lo, hi)),
        segments,
        ..PlotSpec::default()
    };
    let mut title = "Spectral Density (markers: periodogram".to_owned();
    if let Some(s) = &smoothing {
        title.push_str(&format!("; line: smoothed, {s}"));
    }
    if m > 1 {
        title.push_str(&format!("; thin rules: the seasonal frequency 1/{m} and its multiples"));
    }
    title.push_str("; linear trend removed, 10% cosine taper)");
    let mut out = vec![section(SectionKind::SeriesSpectrumPlot, title, SectionContent::Plot(Box::new(spec)))];
    // The largest local peaks of the shown estimate.
    let dens = &shown.density;
    let mut peaks: Vec<usize> = (0..dens.len())
        .filter(|&i| (i == 0 || dens[i] > dens[i - 1]) && (i + 1 == dens.len() || dens[i] >= dens[i + 1]))
        .collect();
    peaks.sort_by(|&a, &b| dens[b].total_cmp(&dens[a]));
    peaks.truncate(5);
    let mut t = TextTable::new(["Frequency (cycles per observation)", "Period (observations per cycle)", "Density"]);
    for i in peaks {
        let f = shown.frequency[i];
        t.push([format_number(f, 4), format_number(1.0 / f, 4), num(dens[i])]);
    }
    out.push(section(
        SectionKind::SeriesSpectrumPeaks,
        format!("Largest Spectral Peaks ({})", if smooth.is_some() { "smoothed spectrum" } else { "periodogram" }),
        SectionContent::Table(t),
    ));
    Ok(out)
}
