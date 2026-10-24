//! Convolution and correlation report: cross- and autocorrelation with the
//! lag of maximum correlation, convolution with a kernel column, Wiener and
//! Richardson–Lucy deconvolution, and the envelope, instantaneous phase and
//! frequency of the analytic signal (Hilbert transform).

use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_signal::convolve::{
    ConvolutionMethod, ConvolutionMode, CorrelationScaling, autocorrelation, convolve, cross_correlation, hilbert,
    richardson_lucy, wiener_deconvolve,
};

use super::{SignalInput, Signals, count, flag, invalid, number, section, summary_table, text};
use crate::format::format_number;
use crate::project::s;
use crate::report::{PlotSpec, PointGroups, Polyline, ReportSection, SectionContent, SectionKind, TextTable};
use crate::report_columns::ReportColumn;
use crate::time_series_data::json_column;

/// How a deconvolution undoes the kernel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DeconvolutionMethod {
    /// Regularized division of the transforms with a noise-to-signal ratio.
    Wiener {
        /// Noise-to-signal power ratio λ ≥ 0.
        noise_ratio: f64,
    },
    /// Richardson–Lucy iteration (non-negative data).
    RichardsonLucy {
        /// Number of iterations.
        iterations: usize,
    },
}

/// What the report computes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CorrelationOperation {
    /// Correlation of the signal with a second column at each lag.
    Cross {
        /// The second signal column (read with the same x and groups).
        other: usize,
        /// Normalisation.
        scaling: CorrelationScaling,
        /// Largest lag shown, in samples (`None`: a quarter of the length).
        max_lag: Option<usize>,
    },
    /// Correlation of the signal with itself.
    Auto {
        /// Normalisation.
        scaling: CorrelationScaling,
        /// Largest lag shown (`None`: a quarter of the length).
        max_lag: Option<usize>,
    },
    /// Convolution with a kernel (the values of a column, in row order).
    Convolution {
        /// Kernel column.
        kernel: usize,
        /// Divide the kernel by its sum (a weighted moving average).
        normalize: bool,
    },
    /// Undo a convolution with a known kernel.
    Deconvolution {
        /// Kernel column.
        kernel: usize,
        /// Method.
        method: DeconvolutionMethod,
    },
    /// Envelope and instantaneous phase and frequency (Hilbert transform).
    Envelope,
}

impl CorrelationOperation {
    /// Menu name.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Cross { .. } => "Cross-correlation",
            Self::Auto { .. } => "Autocorrelation",
            Self::Convolution { .. } => "Convolution",
            Self::Deconvolution { .. } => "Deconvolution",
            Self::Envelope => "Envelope and phase (Hilbert transform)",
        }
    }

    /// The second column read, if any.
    #[must_use]
    pub fn other_column(&self) -> Option<usize> {
        match *self {
            Self::Cross { other, .. } => Some(other),
            Self::Convolution { kernel, .. } | Self::Deconvolution { kernel, .. } => Some(kernel),
            Self::Auto { .. } | Self::Envelope => None,
        }
    }

    pub(super) fn map_column(&mut self, f: &impl Fn(usize) -> usize) {
        match self {
            Self::Cross { other, .. } => *other = f(*other),
            Self::Convolution { kernel, .. } | Self::Deconvolution { kernel, .. } => *kernel = f(*kernel),
            Self::Auto { .. } | Self::Envelope => {}
        }
    }
}

/// Settings of a convolution and correlation report.
#[derive(Debug, Clone, PartialEq)]
pub struct CorrelationSettings {
    /// The operation.
    pub operation: CorrelationOperation,
}

/// Project-file key of a correlation scaling.
#[must_use]
pub fn scaling_key(s: CorrelationScaling) -> &'static str {
    match s {
        CorrelationScaling::None => "none",
        CorrelationScaling::Biased => "biased",
        CorrelationScaling::Unbiased => "unbiased",
        CorrelationScaling::Coefficient => "coefficient",
    }
}

/// Menu name of a correlation scaling.
#[must_use]
pub fn scaling_label(s: CorrelationScaling) -> &'static str {
    match s {
        CorrelationScaling::None => "Raw sums",
        CorrelationScaling::Biased => "Divided by n (biased)",
        CorrelationScaling::Unbiased => "Divided by overlap (unbiased)",
        CorrelationScaling::Coefficient => "Correlation coefficient (−1 to 1)",
    }
}

/// Every scaling, in menu order.
pub const SCALINGS: [CorrelationScaling; 4] = [
    CorrelationScaling::Coefficient,
    CorrelationScaling::Biased,
    CorrelationScaling::Unbiased,
    CorrelationScaling::None,
];

impl CorrelationSettings {
    pub(super) fn json_members(&self, table: &DataTable) -> Vec<(String, JsonValue)> {
        let n = JsonValue::number;
        let name = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        let lag = |l: Option<usize>| l.map_or(JsonValue::Null, |k| n(k as f64));
        match self.operation {
            CorrelationOperation::Cross { other, scaling, max_lag } => vec![
                ("operation".into(), s("cross-correlation")),
                ("other".into(), name(other)),
                ("scaling".into(), s(scaling_key(scaling))),
                ("max_lag".into(), lag(max_lag)),
            ],
            CorrelationOperation::Auto { scaling, max_lag } => vec![
                ("operation".into(), s("autocorrelation")),
                ("scaling".into(), s(scaling_key(scaling))),
                ("max_lag".into(), lag(max_lag)),
            ],
            CorrelationOperation::Convolution { kernel, normalize } => vec![
                ("operation".into(), s("convolution")),
                ("kernel".into(), name(kernel)),
                ("normalize".into(), JsonValue::Bool(normalize)),
            ],
            CorrelationOperation::Deconvolution { kernel, method } => {
                let mut m = vec![("operation".into(), s("deconvolution")), ("kernel".into(), name(kernel))];
                match method {
                    DeconvolutionMethod::Wiener { noise_ratio } => {
                        m.push(("method".into(), s("wiener")));
                        m.push(("noise_ratio".into(), n(noise_ratio)));
                    }
                    DeconvolutionMethod::RichardsonLucy { iterations } => {
                        m.push(("method".into(), s("richardson-lucy")));
                        m.push(("iterations".into(), n(iterations as f64)));
                    }
                }
                m
            }
            CorrelationOperation::Envelope => vec![("operation".into(), s("envelope"))],
        }
    }

    pub(super) fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let scaling = match text(v, "scaling").unwrap_or("coefficient") {
            "none" => CorrelationScaling::None,
            "biased" => CorrelationScaling::Biased,
            "unbiased" => CorrelationScaling::Unbiased,
            "coefficient" => CorrelationScaling::Coefficient,
            other => return Err(invalid(format!("unknown correlation scaling {other:?}"))),
        };
        let max_lag = v.get("max_lag").and_then(JsonValue::as_usize);
        let column = |key: &str| -> Result<usize> {
            json_column(v, key, table)?.ok_or_else(|| invalid(format!("the signal report needs \"{key}\"")))
        };
        let operation = match text(v, "operation").unwrap_or("cross-correlation") {
            "cross-correlation" => CorrelationOperation::Cross { other: column("other")?, scaling, max_lag },
            "autocorrelation" => CorrelationOperation::Auto { scaling, max_lag },
            "convolution" => {
                CorrelationOperation::Convolution { kernel: column("kernel")?, normalize: flag(v, "normalize", true) }
            }
            "deconvolution" => CorrelationOperation::Deconvolution {
                kernel: column("kernel")?,
                method: if text(v, "method") == Some("richardson-lucy") {
                    DeconvolutionMethod::RichardsonLucy { iterations: count(v, "iterations", 30) }
                } else {
                    DeconvolutionMethod::Wiener { noise_ratio: number(v, "noise_ratio", 0.01) }
                },
            },
            "envelope" => CorrelationOperation::Envelope,
            other => return Err(invalid(format!("unknown signal operation {other:?}"))),
        };
        Ok(Self { operation })
    }
}

/// The kernel: the column's values on rows that are not excluded, in row
/// order, missing values skipped.
fn kernel_values(table: &DataTable, states: &RowStateManager, column: usize) -> Result<(String, Vec<f64>)> {
    let col = table.column(column).ok_or_else(|| invalid(format!("no column {column}")))?;
    let v = col.as_numeric().ok_or_else(|| invalid(format!("the kernel column {} must be numeric", col.name)))?;
    let k: Vec<f64> =
        (0..table.n_rows()).filter(|&r| !states.is_excluded(r)).map(|r| v[r]).filter(|x| x.is_finite()).collect();
    if k.is_empty() {
        return Err(invalid(format!("the kernel column {} has no values", col.name)));
    }
    Ok((col.name.clone(), k))
}

fn lag_plot(
    lags: &[i64],
    values: &[f64],
    fs: f64,
    time_axis: bool,
    x_label: &str,
    y_label: &str,
    name: &str,
) -> PlotSpec {
    let x = |l: i64| if time_axis { l as f64 / fs } else { l as f64 };
    let line: Polyline = lags.iter().zip(values).map(|(&l, &v)| [x(l), v]).collect();
    let segments: Vec<Polyline> = lags.iter().zip(values).map(|(&l, &v)| vec![[x(l), 0.0], [x(l), v]]).collect();
    let (lo, hi) = (lags.first().map_or(0.0, |&l| x(l)), lags.last().map_or(0.0, |&l| x(l)));
    PlotSpec {
        x_label: x_label.to_owned(),
        y_label: y_label.to_owned(),
        line: Some(line),
        line_name: Some(name.to_owned()),
        segments: if lags.len() <= 200 { segments } else { vec![vec![[lo, 0.0], [hi, 0.0]]] },
        ..PlotSpec::default()
    }
}

fn kernel_plot(name: &str, k: &[f64]) -> PlotSpec {
    let c = (k.len() - 1) / 2;
    PlotSpec {
        x_label: "Offset from the kernel's centre (samples)".to_owned(),
        y_label: name.to_owned(),
        line: Some(k.iter().enumerate().map(|(j, &v)| [j as f64 - c as f64, v]).collect()),
        line_name: Some("Kernel".to_owned()),
        ..PlotSpec::default()
    }
}

pub(super) fn run(
    table: &DataTable,
    states: &RowStateManager,
    input: &SignalInput,
    settings: &CorrelationSettings,
) -> Result<(String, Vec<ReportSection>, Vec<ReportColumn>)> {
    let sig = Signals::extract(table, states, input, 4)?;
    let fs = sig.sampling_rate;
    let time_lags = input.x.is_some() || input.sampling_rate.is_some();
    let lag_label = if time_lags { format!("Lag ({})", sig.time_label) } else { "Lag (samples)".to_owned() };
    let mut summary = sig.summary_rows();
    summary.push(("Operation".to_owned(), settings.operation.label().to_owned()));
    let mut sections = Vec::new();
    let mut columns = Vec::new();
    let grouped = sig.group_name.is_some();
    let group_head = || grouped.then(|| sig.group_name.clone().unwrap_or_default()).into_iter();
    let title = match settings.operation {
        CorrelationOperation::Cross { other, scaling, max_lag } => {
            if other == input.column {
                return Err(invalid("choose a second signal different from the first"));
            }
            let second = Signals::extract_column(table, states, input, other, 4)?;
            let mut table_rows = TextTable::new(group_head().chain(
                ["Lag of maximum (samples)", "Lag of maximum (time)", "Correlation at that lag"].map(str::to_owned),
            ));
            let mut first_plot = None;
            for t in &sig.traces {
                let Some(u) = second.traces.iter().find(|u| u.label == t.label) else { continue };
                let n = t.values.len().min(u.values.len());
                let ml = max_lag.unwrap_or((n / 4).max(1)).min(n - 1);
                let r = cross_correlation(&t.values[..n], &u.values[..n], scaling, Some(ml))?;
                let mut row: Vec<String> = Vec::new();
                if grouped {
                    row.push(t.label.clone());
                }
                row.extend([
                    r.lag_of_max.to_string(),
                    format_number(r.lag_of_max as f64 / fs, 6),
                    format_number(r.max_value, 5),
                ]);
                table_rows.push(row);
                if first_plot.is_none() {
                    summary.push(("Second signal".to_owned(), second.name.clone()));
                    summary.push(("Normalisation".to_owned(), scaling_label(scaling).to_owned()));
                    summary.push((
                        "Largest correlation".to_owned(),
                        format!(
                            "{} at lag {} ({} samples){}",
                            format_number(r.max_value, 4),
                            format_number(r.lag_of_max as f64 / fs, 5),
                            r.lag_of_max,
                            if t.label.is_empty() { String::new() } else { format!(", group {}", t.label) }
                        ),
                    ));
                    let reading = match r.lag_of_max.cmp(&0) {
                        std::cmp::Ordering::Greater => format!("{} follows {} by this lag", sig.name, second.name),
                        std::cmp::Ordering::Less => format!("{} leads {} by this lag", sig.name, second.name),
                        std::cmp::Ordering::Equal => "the signals move together without delay".to_owned(),
                    };
                    summary.push(("Reading".to_owned(), reading));
                    first_plot = Some(lag_plot(
                        &r.lag,
                        &r.value,
                        fs,
                        time_lags,
                        &lag_label,
                        "Correlation",
                        &format!("{} with {}", sig.name, second.name),
                    ));
                    let both = PlotSpec {
                        x_label: sig.time_label.clone(),
                        y_label: "Value".to_owned(),
                        x_time: sig.time_display,
                        groups: Some(PointGroups {
                            title: String::new(),
                            labels: vec![sig.name.clone(), second.name.clone()],
                            membership: Vec::new(),
                            lines: vec![
                                t.time.iter().zip(&t.values).map(|(&a, &b)| [a, b]).collect(),
                                u.time.iter().zip(&u.values).map(|(&a, &b)| [a, b]).collect(),
                            ],
                        }),
                        ..PlotSpec::default()
                    };
                    sections.push(section(
                        SectionKind::SignalTrace,
                        "Both Signals",
                        SectionContent::Plot(Box::new(both)),
                    ));
                }
            }
            if let Some(p) = first_plot {
                sections.push(section(
                    SectionKind::CorrelationPlot,
                    "Cross-Correlation",
                    SectionContent::Plot(Box::new(p)),
                ));
            }
            sections.push(section(
                SectionKind::CorrelationPeaks,
                "Lag of Maximum Correlation",
                SectionContent::Table(table_rows),
            ));
            format!("Cross-Correlation: {} with {}", sig.name, second.name)
        }
        CorrelationOperation::Auto { scaling, max_lag } => {
            summary.push(("Normalisation".to_owned(), scaling_label(scaling).to_owned()));
            let mut peaks = TextTable::new(
                group_head().chain(["Rank", "Lag (samples)", "Period", "Correlation"].map(str::to_owned)),
            );
            let mut curves = Vec::new();
            for t in &sig.traces {
                let n = t.values.len();
                let ml = max_lag.unwrap_or((n / 4).max(1)).min(n - 1);
                let r = autocorrelation(&t.values, scaling, Some(ml))?;
                let x = |l: i64| if time_lags { l as f64 / fs } else { l as f64 };
                curves.push(r.lag.iter().zip(&r.value).map(|(&l, &v)| [x(l), v]).collect::<Polyline>());
                // Local maxima after lag 0, strongest first: candidate periods.
                let mut idx: Vec<usize> = (1..r.value.len().saturating_sub(1))
                    .filter(|&i| r.value[i] > r.value[i - 1] && r.value[i] >= r.value[i + 1] && r.value[i] > 0.0)
                    .collect();
                idx.sort_by(|&a, &b| r.value[b].total_cmp(&r.value[a]));
                for (rank, &i) in idx.iter().take(3).enumerate() {
                    let mut row: Vec<String> = Vec::new();
                    if grouped {
                        row.push(t.label.clone());
                    }
                    row.extend([
                        (rank + 1).to_string(),
                        r.lag[i].to_string(),
                        format_number(r.lag[i] as f64 / fs, 6),
                        format_number(r.value[i], 5),
                    ]);
                    peaks.push(row);
                }
            }
            sections.push(section(
                SectionKind::CorrelationPlot,
                "Autocorrelation",
                SectionContent::Plot(Box::new(sig.curves(curves, &lag_label, "Autocorrelation", "Autocorrelation"))),
            ));
            sections.push(section(
                SectionKind::CorrelationPeaks,
                "Repeating Patterns (Peaks after Lag 0)",
                SectionContent::Table(peaks),
            ));
            format!("Autocorrelation: {}", sig.name)
        }
        CorrelationOperation::Convolution { kernel, normalize } => {
            let (kname, mut k) = kernel_values(table, states, kernel)?;
            if normalize {
                let total: f64 = k.iter().sum();
                if total.abs() < f64::MIN_POSITIVE {
                    return Err(invalid("the kernel sums to zero, so it cannot be normalised"));
                }
                k.iter_mut().for_each(|v| *v /= total);
            }
            summary.push((
                "Kernel".to_owned(),
                format!("{kname} ({} values{})", k.len(), if normalize { ", divided by their sum" } else { "" }),
            ));
            let mut out = Vec::new();
            for t in &sig.traces {
                out.push(convolve(&t.values, &k, ConvolutionMode::Same, ConvolutionMethod::Auto)?);
            }
            sections.push(section(
                SectionKind::ConvolutionResult,
                "Signal and Convolution",
                SectionContent::Plot(Box::new(sig.rows_with_lines(&out, "Convolution", &sig.name))),
            ));
            sections.push(section(
                SectionKind::ConvolutionKernel,
                "Kernel",
                SectionContent::Plot(Box::new(kernel_plot(&kname, &k))),
            ));
            columns.push(per_rows(&sig, &out, "convolved", "Save convolution", format!("{} ∗ {kname}", sig.name)));
            format!("Convolution: {} with {kname}", sig.name)
        }
        CorrelationOperation::Deconvolution { kernel, method } => {
            let (kname, k) = kernel_values(table, states, kernel)?;
            let mut out = Vec::new();
            for t in &sig.traces {
                out.push(match method {
                    DeconvolutionMethod::Wiener { noise_ratio } => wiener_deconvolve(&t.values, &k, noise_ratio)?,
                    DeconvolutionMethod::RichardsonLucy { iterations } => richardson_lucy(&t.values, &k, iterations)?,
                });
            }
            summary.push(("Kernel".to_owned(), format!("{kname} ({} values)", k.len())));
            summary.push((
                "Method".to_owned(),
                match method {
                    DeconvolutionMethod::Wiener { noise_ratio } => {
                        format!("Wiener division, noise-to-signal ratio {}", format_number(noise_ratio, 4))
                    }
                    DeconvolutionMethod::RichardsonLucy { iterations } => {
                        format!("Richardson–Lucy, {iterations} iterations (kernel scaled to sum 1)")
                    }
                },
            ));
            sections.push(section(
                SectionKind::Deconvolved,
                "Observed and Restored Signal",
                SectionContent::Plot(Box::new(sig.rows_with_lines(&out, "Restored", &sig.name))),
            ));
            sections.push(section(
                SectionKind::ConvolutionKernel,
                "Kernel",
                SectionContent::Plot(Box::new(kernel_plot(&kname, &k))),
            ));
            columns.push(per_rows(&sig, &out, "deconvolved", "Save restored signal", format!("Restored {}", sig.name)));
            format!("Deconvolution: {} by {kname}", sig.name)
        }
        CorrelationOperation::Envelope => {
            let mut env = Vec::new();
            let mut phase = Vec::new();
            let mut freq = Vec::new();
            for t in &sig.traces {
                let a = hilbert(&t.values, fs)?;
                let mut f = a.frequency.clone();
                // One frequency per sample: the last repeats the one before.
                f.push(*a.frequency.last().unwrap_or(&f64::NAN));
                env.push(a.envelope);
                phase.push(a.phase);
                freq.push(f);
            }
            let mean_freq: Vec<String> = sig
                .traces
                .iter()
                .zip(&freq)
                .map(|(t, f)| {
                    let m = f.iter().sum::<f64>() / f.len() as f64;
                    if t.label.is_empty() {
                        format_number(m, 5)
                    } else {
                        format!("{} {}", t.label, format_number(m, 5))
                    }
                })
                .collect();
            summary.push(("Mean instantaneous frequency".to_owned(), mean_freq.join("; ")));
            sections.push(section(
                SectionKind::SignalEnvelope,
                "Signal and Envelope",
                SectionContent::Plot(Box::new(sig.rows_with_lines(&env, "Envelope", &sig.name))),
            ));
            let t_curves = |vals: &[Vec<f64>]| -> Vec<Polyline> {
                sig.traces
                    .iter()
                    .zip(vals)
                    .map(|(t, v)| t.time.iter().zip(v).map(|(&a, &b)| [a, b]).collect())
                    .collect()
            };
            let mut fp =
                sig.curves(t_curves(&freq), &sig.time_label, &sig.frequency_label(), "Instantaneous frequency");
            fp.x_time = sig.time_display;
            sections.push(section(
                SectionKind::InstantaneousFrequency,
                "Instantaneous Frequency",
                SectionContent::Plot(Box::new(fp)),
            ));
            let mut pp =
                sig.curves(t_curves(&phase), &sig.time_label, "Phase (radians, unwrapped)", "Instantaneous phase");
            pp.x_time = sig.time_display;
            sections.push(section(
                SectionKind::InstantaneousPhase,
                "Instantaneous Phase",
                SectionContent::Plot(Box::new(pp)),
            ));
            columns.push(per_rows(&sig, &env, "envelope", "Save envelope", format!("Envelope of {}", sig.name)));
            columns.push(per_rows(&sig, &phase, "phase", "Save instantaneous phase", format!("Phase of {}", sig.name)));
            columns.push(per_rows(
                &sig,
                &freq,
                "frequency",
                "Save instantaneous frequency",
                format!("Frequency of {}", sig.name),
            ));
            format!("Envelope and Phase: {}", sig.name)
        }
    };
    sections.insert(
        0,
        section(SectionKind::SignalProcessingSummary, "Summary", SectionContent::Table(summary_table(summary))),
    );
    Ok((title, sections, columns))
}

fn per_rows(sig: &Signals, grid: &[Vec<f64>], key: &str, label: &str, name: String) -> ReportColumn {
    let mut out = vec![f64::NAN; sig.n_rows];
    for (t, v) in sig.traces.iter().zip(grid) {
        t.fill_rows(v, &mut out);
    }
    ReportColumn::continuous(key, label, name, out)
}
