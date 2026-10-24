//! Wavelet report: a multilevel discrete wavelet decomposition of each
//! signal (drawn as its multiresolution components), the energy and
//! frequency band of each level, wavelet denoising by thresholding the
//! detail coefficients, and a continuous-wavelet scalogram.

use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_signal::wavelet::{
    ContinuousWavelet, Decomposition, DenoiseOptions, Extension, NoiseEstimate, ThresholdKind, ThresholdRule, Wavelet,
    cwt, denoise, log_scales, max_level, wavedec, waverec,
};

use super::{SignalInput, Signals, count, flag, invalid, number, section, summary_table, text};
use crate::format::format_number;
use crate::intensity_map::{IntensityMapSpec, YDisplay};
use crate::project::s;
use crate::report::{PlotSpec, PointGroups, Polyline, ReportSection, SectionContent, SectionKind, TextTable};
use crate::report_columns::ReportColumn;

/// Most scalograms drawn (one per group).
const MAX_SCALOGRAMS: usize = 4;
/// Most time cells across a scalogram (longer signals are averaged in bins).
const MAX_TIME_CELLS: usize = 400;

/// Settings of a wavelet report.
#[derive(Debug, Clone, PartialEq)]
pub struct WaveletSettings {
    /// Discrete wavelet name: `haar`, `db1`–`db10`, `sym2`–`sym10` or
    /// `coif1`–`coif5` (default `db4`).
    pub wavelet: String,
    /// Signal extension at the ends (default symmetric).
    pub extension: Extension,
    /// Decomposition levels (`None`: the most the signal length allows).
    pub levels: Option<usize>,
    /// Denoise by thresholding the detail coefficients (default true).
    pub denoise: bool,
    /// Threshold rule (default universal).
    pub rule: ThresholdRule,
    /// Soft or hard thresholding (default soft).
    pub kind: ThresholdKind,
    /// Where the noise level is estimated (default the finest level).
    pub noise: NoiseEstimate,
    /// Add a continuous-wavelet scalogram (default false).
    pub scalogram: bool,
    /// Mother wavelet of the scalogram (default Morlet).
    pub continuous: ContinuousWavelet,
    /// Scales (rows) of the scalogram (default 48).
    pub scales: usize,
}

impl Default for WaveletSettings {
    fn default() -> Self {
        Self {
            wavelet: "db4".to_owned(),
            extension: Extension::Symmetric,
            levels: None,
            denoise: true,
            rule: ThresholdRule::Universal,
            kind: ThresholdKind::Soft,
            noise: NoiseEstimate::FinestLevel,
            scalogram: false,
            continuous: ContinuousWavelet::Morlet,
            scales: 48,
        }
    }
}

/// Project-file key of an extension.
#[must_use]
pub fn extension_key(e: Extension) -> &'static str {
    match e {
        Extension::Symmetric => "symmetric",
        Extension::Periodic => "periodic",
        Extension::Zero => "zero",
    }
}

/// Menu name of an extension.
#[must_use]
pub fn extension_name(e: Extension) -> &'static str {
    match e {
        Extension::Symmetric => "Symmetric (mirror)",
        Extension::Periodic => "Periodic",
        Extension::Zero => "Zeros",
    }
}

/// Menu name of a threshold rule.
#[must_use]
pub fn rule_name(r: ThresholdRule) -> &'static str {
    match r {
        ThresholdRule::Universal => "Universal (√(2 ln n) σ)",
        ThresholdRule::Sure => "SURE (per level)",
        ThresholdRule::Fixed(_) => "Fixed value",
    }
}

impl WaveletSettings {
    pub(super) fn json_members(&self) -> Vec<(String, JsonValue)> {
        let n = JsonValue::number;
        let mut m: Vec<(String, JsonValue)> =
            vec![("wavelet".into(), s(&self.wavelet)), ("extension".into(), s(extension_key(self.extension)))];
        if let Some(l) = self.levels {
            m.push(("levels".into(), n(l as f64)));
        }
        m.push(("denoise".into(), JsonValue::Bool(self.denoise)));
        let rule = match self.rule {
            ThresholdRule::Universal => "universal",
            ThresholdRule::Sure => "sure",
            ThresholdRule::Fixed(_) => "fixed",
        };
        m.push(("threshold_rule".into(), s(rule)));
        if let ThresholdRule::Fixed(lambda) = self.rule {
            m.push(("threshold".into(), n(lambda)));
        }
        m.extend([
            ("shrinkage".into(), s(if self.kind == ThresholdKind::Soft { "soft" } else { "hard" })),
            ("noise".into(), s(if self.noise == NoiseEstimate::FinestLevel { "finest" } else { "each-level" })),
            ("scalogram".into(), JsonValue::Bool(self.scalogram)),
            (
                "continuous_wavelet".into(),
                s(if self.continuous == ContinuousWavelet::Morlet { "morlet" } else { "mexican-hat" }),
            ),
            ("scales".into(), n(self.scales as f64)),
        ]);
        m
    }

    /// Read the settings from their project-file (JSON) members; members
    /// left out take their defaults.
    ///
    /// # Errors
    /// Unknown option names.
    pub fn from_json(v: &JsonValue) -> Result<Self> {
        let d = Self::default();
        let wavelet = text(v, "wavelet").unwrap_or(&d.wavelet).to_owned();
        Wavelet::new(&wavelet)?;
        let extension = match text(v, "extension") {
            None | Some("symmetric") => Extension::Symmetric,
            Some("periodic") => Extension::Periodic,
            Some("zero") => Extension::Zero,
            Some(other) => return Err(invalid(format!("unknown extension {other:?}"))),
        };
        let rule = match text(v, "threshold_rule") {
            None | Some("universal") => ThresholdRule::Universal,
            Some("sure") => ThresholdRule::Sure,
            Some("fixed") => ThresholdRule::Fixed(number(v, "threshold", 1.0)),
            Some(other) => return Err(invalid(format!("unknown threshold rule {other:?}"))),
        };
        let kind = match text(v, "shrinkage") {
            None | Some("soft") => ThresholdKind::Soft,
            Some("hard") => ThresholdKind::Hard,
            Some(other) => return Err(invalid(format!("unknown shrinkage {other:?}"))),
        };
        let noise = match text(v, "noise") {
            None | Some("finest") => NoiseEstimate::FinestLevel,
            Some("each-level") => NoiseEstimate::EachLevel,
            Some(other) => return Err(invalid(format!("unknown noise estimate {other:?}"))),
        };
        let continuous = match text(v, "continuous_wavelet") {
            None | Some("morlet") => ContinuousWavelet::Morlet,
            Some("mexican-hat") => ContinuousWavelet::MexicanHat,
            Some(other) => return Err(invalid(format!("unknown continuous wavelet {other:?}"))),
        };
        Ok(Self {
            wavelet,
            extension,
            levels: v.get("levels").and_then(JsonValue::as_usize),
            denoise: flag(v, "denoise", d.denoise),
            rule,
            kind,
            noise,
            scalogram: flag(v, "scalogram", d.scalogram),
            continuous,
            scales: count(v, "scales", d.scales).clamp(4, 200),
        })
    }

    fn denoise_options(&self, level: usize) -> DenoiseOptions {
        DenoiseOptions {
            level: Some(level),
            extension: self.extension,
            rule: self.rule,
            kind: self.kind,
            noise: self.noise,
        }
    }
}

/// The decomposition of one trace and, when denoising, its result.
struct Analysed {
    dec: Decomposition,
    thresholds: Option<Vec<f64>>,
    noise_sd: Option<Vec<f64>>,
    denoised: Option<Vec<f64>>,
}

pub(super) fn run(
    table: &DataTable,
    states: &RowStateManager,
    input: &SignalInput,
    settings: &WaveletSettings,
) -> Result<(String, Vec<ReportSection>, Vec<ReportColumn>)> {
    let w = Wavelet::new(&settings.wavelet)?;
    let sig = Signals::extract(table, states, input, 4)?;
    let fs = sig.sampling_rate;
    let shortest = sig.traces.iter().map(|t| t.values.len()).min().unwrap_or(0);
    let most = max_level(shortest, &w).max(1);
    let level = settings.levels.unwrap_or(most).clamp(1, most);
    let mut analysed = Vec::with_capacity(sig.traces.len());
    for t in &sig.traces {
        let a = if settings.denoise {
            let d = denoise(&t.values, &w, &settings.denoise_options(level))?;
            Analysed {
                dec: d.decomposition,
                thresholds: Some(d.thresholds),
                noise_sd: Some(d.noise_sd),
                denoised: Some(d.signal),
            }
        } else {
            Analysed {
                dec: wavedec(&t.values, &w, settings.extension, Some(level))?,
                thresholds: None,
                noise_sd: None,
                denoised: None,
            }
        };
        analysed.push(a);
    }

    let mut summary = sig.summary_rows();
    summary.push(("Wavelet".to_owned(), format!("{} ({} coefficients)", w.name(), w.filter_length())));
    summary.push(("Extension".to_owned(), extension_name(settings.extension).to_owned()));
    let level_note = match settings.levels {
        Some(l) if l > most => format!("{level} (the most this signal length allows; {l} were asked for)"),
        _ => level.to_string(),
    };
    summary.push(("Levels".to_owned(), level_note));
    if settings.denoise {
        let shrink = if settings.kind == ThresholdKind::Soft { "soft" } else { "hard" };
        let rule = match settings.rule {
            ThresholdRule::Fixed(l) => format!("fixed at {}", format_number(l, 6)),
            r => rule_name(r).to_owned(),
        };
        summary.push(("Denoising".to_owned(), format!("{shrink} thresholding, {rule}")));
        summary.push((
            "Noise level estimated".to_owned(),
            if settings.noise == NoiseEstimate::FinestLevel {
                "from the finest level (median absolute deviation / 0.6745)"
            } else {
                "at each level (median absolute deviation / 0.6745)"
            }
            .to_owned(),
        ));
        if let Some(sd) = analysed.first().and_then(|a| a.noise_sd.as_ref()).and_then(|v| v.last()) {
            summary.push(("Noise standard deviation".to_owned(), format_number(*sd, 5)));
        }
    }
    let mut sections =
        vec![section(SectionKind::SignalProcessingSummary, "Summary", SectionContent::Table(summary_table(summary)))];

    // Multiresolution components of the first signal.
    let t0 = &sig.traces[0];
    let a0 = &analysed[0];
    let title = if t0.label.is_empty() {
        "Multilevel Decomposition".to_owned()
    } else {
        format!("Multilevel Decomposition: {}", t0.label)
    };
    sections.push(section(
        SectionKind::WaveletDecomposition,
        &title,
        SectionContent::Plot(Box::new(decomposition_plot(
            &sig.time_label,
            sig.time_display,
            &t0.time,
            &t0.values,
            &a0.dec,
            &w,
            settings.extension,
        )?)),
    ));

    // Energy and bands per level.
    let grouped = sig.group_name.is_some();
    let mut header: Vec<String> = grouped.then(|| sig.group_name.clone().unwrap_or_default()).into_iter().collect();
    header.extend(["Level", "Frequency band", "Coefficients", "Energy (%)"].map(str::to_owned));
    if settings.denoise {
        header.extend(["Noise SD", "Threshold", "Set to zero (%)"].map(str::to_owned));
    }
    let mut energy = TextTable::new(header);
    for (t, a) in sig.traces.iter().zip(&analysed) {
        let levels = a.dec.levels();
        let sq = |v: &[f64]| v.iter().map(|x| x * x).sum::<f64>();
        let total = sq(&a.dec.approximation) + a.dec.details.iter().map(|d| sq(d)).sum::<f64>();
        let pct = |e: f64| if total > 0.0 { format_number(100.0 * e / total, 4) } else { "0".to_owned() };
        let band = |lo: f64, hi: f64| format!("{} to {}", format_number(lo, 4), format_number(hi, 4));
        // Finest level first.
        for j in 1..=levels {
            let d = &a.dec.details[levels - j];
            let mut row: Vec<String> = grouped.then(|| t.label.clone()).into_iter().collect();
            row.extend([
                format!("Detail {j}"),
                band(fs / 2f64.powi(j as i32 + 1), fs / 2f64.powi(j as i32)),
                d.len().to_string(),
                pct(sq(d)),
            ]);
            if let (Some(th), Some(sd)) = (&a.thresholds, &a.noise_sd) {
                let lambda = th[levels - j];
                let zero = d
                    .iter()
                    .filter(|x| if settings.kind == ThresholdKind::Soft { x.abs() <= lambda } else { x.abs() < lambda })
                    .count();
                row.extend([
                    format_number(sd[levels - j], 5),
                    format_number(lambda, 5),
                    format_number(100.0 * zero as f64 / d.len().max(1) as f64, 4),
                ]);
            }
            energy.push(row);
        }
        let mut row: Vec<String> = grouped.then(|| t.label.clone()).into_iter().collect();
        row.extend([
            format!("Approximation {levels}"),
            band(0.0, fs / 2f64.powi(levels as i32 + 1)),
            a.dec.approximation.len().to_string(),
            pct(sq(&a.dec.approximation)),
        ]);
        if settings.denoise {
            row.extend(["".to_owned(), "not thresholded".to_owned(), "".to_owned()]);
        }
        energy.push(row);
    }
    sections.push(section(SectionKind::WaveletEnergy, "Energy by Level", SectionContent::Table(energy)));

    let mut columns = Vec::new();
    if settings.denoise {
        let denoised: Vec<Vec<f64>> = analysed.iter().map(|a| a.denoised.clone().unwrap_or_default()).collect();
        sections.push(section(
            SectionKind::WaveletDenoised,
            "Denoised Signal",
            SectionContent::Plot(Box::new(sig.rows_with_lines(&denoised, "Denoised", &sig.name))),
        ));
        let mut rows = vec![f64::NAN; sig.n_rows];
        let mut removed = vec![f64::NAN; sig.n_rows];
        for (t, y) in sig.traces.iter().zip(&denoised) {
            t.fill_rows(y, &mut rows);
            let r: Vec<f64> = t.values.iter().zip(y).map(|(a, b)| a - b).collect();
            t.fill_rows(&r, &mut removed);
        }
        columns.push(ReportColumn::continuous(
            "denoised",
            "Save denoised signal",
            format!("Denoised {}", sig.name),
            rows,
        ));
        columns.push(ReportColumn::continuous(
            "removed",
            "Save removed noise (signal − denoised)",
            format!("Noise removed from {}", sig.name),
            removed,
        ));
    }
    // The smooth part: the coarsest approximation alone.
    let mut smooth = vec![f64::NAN; sig.n_rows];
    for (t, a) in sig.traces.iter().zip(&analysed) {
        let only = Decomposition {
            approximation: a.dec.approximation.clone(),
            details: a.dec.details.iter().map(|d| vec![0.0; d.len()]).collect(),
            signal_length: a.dec.signal_length,
        };
        let y = waverec(&only, &w, settings.extension)?;
        t.fill_rows(&y[..t.values.len().min(y.len())], &mut smooth);
    }
    columns.push(ReportColumn::continuous(
        "approximation",
        "Save smooth part (approximation)",
        format!("Approximation of {}", sig.name),
        smooth,
    ));

    if settings.scalogram {
        for t in sig.traces.iter().take(MAX_SCALOGRAMS) {
            let map = scalogram(&t.values, &t.time, fs, settings, &sig.time_label, &sig.frequency_label())?;
            let title = if t.label.is_empty() { "Scalogram".to_owned() } else { format!("Scalogram: {}", t.label) };
            sections.push(section(SectionKind::Scalogram, &title, SectionContent::IntensityMap(map)));
        }
    }
    Ok((format!("Wavelet Analysis: {}", sig.name), sections, columns))
}

/// The signal and its multiresolution components (each detail level and the
/// approximation reconstructed alone), stacked in rows: the signal at the
/// top, then the details from finest to coarsest, then the approximation.
/// Each row is centred and scaled to fill its band.
fn decomposition_plot(
    time_label: &str,
    time_display: Option<stat_data::TemporalDisplay>,
    time: &[f64],
    signal: &[f64],
    dec: &Decomposition,
    w: &Wavelet,
    extension: Extension,
) -> Result<PlotSpec> {
    let levels = dec.levels();
    let zeros = |v: &[f64]| vec![0.0; v.len()];
    let mut components: Vec<(String, Vec<f64>)> = vec![("Signal".to_owned(), signal.to_vec())];
    for j in 1..=levels {
        let i = levels - j;
        let part = Decomposition {
            approximation: zeros(&dec.approximation),
            details: dec.details.iter().enumerate().map(|(k, d)| if k == i { d.clone() } else { zeros(d) }).collect(),
            signal_length: dec.signal_length,
        };
        components.push((format!("Detail {j}"), waverec(&part, w, extension)?));
    }
    let approx = Decomposition {
        approximation: dec.approximation.clone(),
        details: dec.details.iter().map(|d| zeros(d)).collect(),
        signal_length: dec.signal_length,
    };
    components.push((format!("Approximation {levels}"), waverec(&approx, w, extension)?));
    let rows = components.len();
    let mut lines: Vec<Polyline> = Vec::with_capacity(rows);
    let mut ticks = Vec::with_capacity(rows);
    let mut labels = Vec::with_capacity(rows);
    for (k, (label, v)) in components.into_iter().enumerate() {
        let centre = (rows - 1 - k) as f64;
        let n = v.len().min(time.len());
        let mean = v[..n].iter().sum::<f64>() / n.max(1) as f64;
        let spread = v[..n].iter().map(|x| (x - mean).abs()).fold(0.0, f64::max);
        let scale = if spread > 0.0 { 0.42 / spread } else { 0.0 };
        lines.push(time[..n].iter().zip(&v[..n]).map(|(&t, &y)| [t, centre + (y - mean) * scale]).collect());
        ticks.push((centre, label.clone()));
        labels.push(label);
    }
    Ok(PlotSpec {
        x_label: time_label.to_owned(),
        y_label: "Component (each centred and scaled to its row)".to_owned(),
        groups: Some(PointGroups { title: "Component".to_owned(), labels, membership: Vec::new(), lines }),
        y_ticks: Some(ticks),
        x_time: time_display,
        ..PlotSpec::default()
    })
}

/// Continuous-wavelet scalogram of one signal: `|W(s, t)|` over time and
/// frequency (log₂ axis), scales from the Nyquist frequency down to two
/// cycles over the signal; long signals averaged into at most
/// [`MAX_TIME_CELLS`] time bins.
fn scalogram(
    x: &[f64],
    time: &[f64],
    fs: f64,
    settings: &WaveletSettings,
    time_label: &str,
    freq_label: &str,
) -> Result<IntensityMapSpec> {
    let n = x.len();
    let fc = settings.continuous.peak_frequency();
    // Scales in samples: frequency = fc·fs / s.
    let f_hi = fs / 2.0;
    let f_lo = (2.0 * fs / n as f64).min(f_hi / 4.0);
    let mut scales = log_scales(fc * fs / f_hi, fc * fs / f_lo, settings.scales);
    // Largest scale first, so frequency (and the log₂ axis) increases.
    scales.reverse();
    let mean = x.iter().sum::<f64>() / n as f64;
    let centred: Vec<f64> = x.iter().map(|v| v - mean).collect();
    let sc = cwt(&centred, &scales, settings.continuous, 1.0 / fs)?;
    let bin = n.div_ceil(MAX_TIME_CELLS).max(1);
    let cells = (n / bin).max(1);
    let dt = 1.0 / fs;
    let xs: Vec<f64> = (0..cells).map(|i| time[0] + (i * bin) as f64 * dt + (bin - 1) as f64 * dt / 2.0).collect();
    let ys: Vec<f64> = sc.frequency.iter().map(|f| f.log2()).collect();
    let values: Vec<Vec<f64>> = (0..cells)
        .map(|i| {
            sc.coefficients
                .iter()
                .map(|row| {
                    let seg = &row[i * bin..((i + 1) * bin).min(row.len())];
                    seg.iter().map(|v| v.abs()).sum::<f64>() / seg.len().max(1) as f64
                })
                .collect()
        })
        .collect();
    let hi = values.iter().flatten().copied().filter(|v| v.is_finite()).fold(0.0, f64::max);
    Ok(IntensityMapSpec {
        x_label: time_label.to_owned(),
        y_label: freq_label.to_owned(),
        value_label: format!("|Coefficient| ({})", settings.continuous.label()),
        x: xs,
        y: ys.clone(),
        values,
        y_ticks: Some(log2_ticks(ys[0], ys[ys.len() - 1])),
        y_display: Some(YDisplay::Log2),
        range: (0.0, if hi > 0.0 { hi } else { 1.0 }),
    })
}

/// Ticks at 1, 2 and 5 × 10ᵏ between `2^lo` and `2^hi`, placed at their log₂.
fn log2_ticks(lo: f64, hi: f64) -> Vec<(f64, String)> {
    let (a, b) = (lo.min(hi).exp2(), lo.max(hi).exp2());
    let mut out = Vec::new();
    let mut k = a.log10().floor() as i32;
    while 10f64.powi(k) <= b * 1.0001 {
        for m in [1.0, 2.0, 5.0] {
            let v = m * 10f64.powi(k);
            if v >= a * 0.9999 && v <= b * 1.0001 {
                out.push((v.log2(), format_number(v, 4)));
            }
        }
        k += 1;
    }
    out
}
