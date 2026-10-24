//! Fourier spectrum report: amplitude, power and phase spectra of each
//! signal, its dominant frequencies, Welch's averaged periodogram, a
//! spectrogram and the spectrum values as a table.

use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_signal::fft::fft_shift;
use stat_signal::spectrum::{
    Detrend, Scaling, SegmentOptions, Sides, Spectrum, SpectrumOptions, spectrogram, spectrum, welch,
};
use stat_signal::window::Window;

use super::{
    SignalInput, Signals, count, flag, invalid, number, section, summary_table, text, window_from_json, window_members,
    window_name,
};
use crate::format::format_number;
use crate::intensity_map::IntensityMapSpec;
use crate::project::s;
use crate::report::{Polyline, ReportSection, SectionContent, SectionKind, TextTable};
use crate::report_columns::ReportColumn;

/// Most spectrograms drawn (one per group).
const MAX_SPECTROGRAMS: usize = 4;

/// Settings of a Fourier spectrum report.
#[derive(Debug, Clone, PartialEq)]
pub struct SpectrumSettings {
    /// Window applied before the transform (default Hann).
    pub window: Window,
    /// Trend removed first (default the mean).
    pub detrend: Detrend,
    /// Zero padding: the transform is this many times the signal length
    /// (default 1, none).
    pub padding: usize,
    /// Round the transform length up to a power of two (default false).
    pub power_of_two: bool,
    /// One-sided (default) or two-sided spectra.
    pub sides: Sides,
    /// Power as a density (default) or as power per frequency.
    pub scaling: Scaling,
    /// Plot power in decibels, `10 log₁₀ P` (default true).
    pub decibels: bool,
    /// Show the phase spectrum (default false).
    pub phase: bool,
    /// Dominant frequencies listed (default 5; 0 for none).
    pub peaks: usize,
    /// Add Welch's averaged periodogram (default false).
    pub welch: bool,
    /// Add a spectrogram (default false).
    pub spectrogram: bool,
    /// Segment length of Welch's method and the spectrogram (default 256,
    /// or the signal length when shorter).
    pub segment_length: usize,
    /// Overlap of consecutive segments, percent of the segment (default 50).
    pub overlap_percent: f64,
}

impl Default for SpectrumSettings {
    fn default() -> Self {
        Self {
            window: Window::Hann,
            detrend: Detrend::Constant,
            padding: 1,
            power_of_two: false,
            sides: Sides::One,
            scaling: Scaling::Density,
            decibels: true,
            phase: false,
            peaks: 5,
            welch: false,
            spectrogram: false,
            segment_length: 256,
            overlap_percent: 50.0,
        }
    }
}

/// Project-file key of a detrending choice.
#[must_use]
pub fn detrend_key(d: Detrend) -> &'static str {
    match d {
        Detrend::None => "none",
        Detrend::Constant => "mean",
        Detrend::Linear => "linear",
    }
}

impl SpectrumSettings {
    pub(super) fn json_members(&self) -> Vec<(String, JsonValue)> {
        let n = JsonValue::number;
        let mut m = window_members("window", self.window);
        m.extend([
            ("detrend".into(), s(detrend_key(self.detrend))),
            ("padding".into(), n(self.padding as f64)),
            ("power_of_two".into(), JsonValue::Bool(self.power_of_two)),
            ("sides".into(), s(if self.sides == Sides::One { "one" } else { "two" })),
            ("scaling".into(), s(if self.scaling == Scaling::Density { "density" } else { "spectrum" })),
            ("decibels".into(), JsonValue::Bool(self.decibels)),
            ("phase".into(), JsonValue::Bool(self.phase)),
            ("peaks".into(), n(self.peaks as f64)),
            ("welch".into(), JsonValue::Bool(self.welch)),
            ("spectrogram".into(), JsonValue::Bool(self.spectrogram)),
            ("segment_length".into(), n(self.segment_length as f64)),
            ("overlap_percent".into(), n(self.overlap_percent)),
        ]);
        m
    }

    pub(super) fn from_json(v: &JsonValue) -> Result<Self> {
        let d = Self::default();
        let detrend = match text(v, "detrend") {
            None => d.detrend,
            Some("none") => Detrend::None,
            Some("mean") => Detrend::Constant,
            Some("linear") => Detrend::Linear,
            Some(other) => return Err(invalid(format!("unknown detrending {other:?}"))),
        };
        Ok(Self {
            window: window_from_json(v, "window", d.window)?,
            detrend,
            padding: count(v, "padding", d.padding).max(1),
            power_of_two: flag(v, "power_of_two", d.power_of_two),
            sides: if text(v, "sides") == Some("two") { Sides::Two } else { Sides::One },
            scaling: if text(v, "scaling") == Some("spectrum") { Scaling::Spectrum } else { Scaling::Density },
            decibels: flag(v, "decibels", d.decibels),
            phase: flag(v, "phase", d.phase),
            peaks: count(v, "peaks", d.peaks),
            welch: flag(v, "welch", d.welch),
            spectrogram: flag(v, "spectrogram", d.spectrogram),
            segment_length: count(v, "segment_length", d.segment_length),
            overlap_percent: number(v, "overlap_percent", d.overlap_percent),
        })
    }

    fn fft_length(&self, n: usize) -> usize {
        let len = n * self.padding.max(1);
        if self.power_of_two { len.next_power_of_two() } else { len }
    }

    fn power_label(&self, unit: &str) -> String {
        let base = match self.scaling {
            Scaling::Density => "Power density",
            Scaling::Spectrum => "Power",
        };
        if self.decibels {
            format!("{base} (dB)")
        } else if self.scaling == Scaling::Density {
            format!("{base} (units² per {})", unit.trim_start_matches("cycles per ").trim_start_matches("unit of "))
        } else {
            format!("{base} (units²)")
        }
    }

    fn segments(&self, fs: f64) -> Result<SegmentOptions> {
        if !(0.0..95.0).contains(&self.overlap_percent) {
            return Err(invalid("the segment overlap must be at least 0% and below 95%"));
        }
        if self.segment_length < 8 {
            return Err(invalid("segments need at least 8 samples"));
        }
        let overlap = (self.segment_length as f64 * self.overlap_percent / 100.0).round() as usize;
        Ok(SegmentOptions {
            sampling_rate: fs,
            window: self.window,
            segment_length: self.segment_length,
            overlap: Some(overlap.min(self.segment_length - 1)),
            fft_length: None,
            detrend: self.detrend,
            sides: self.sides,
            scaling: self.scaling,
        })
    }
}

fn db(p: f64) -> f64 {
    if p > 0.0 { 10.0 * p.log10() } else { f64::NAN }
}

/// Points of a spectrum in ascending frequency order (two-sided spectra are
/// shifted so negative frequencies come first).
fn ordered(freq: &[f64], values: &[f64], two_sided: bool) -> Polyline {
    let pts: Polyline = freq.iter().zip(values).filter(|(_, v)| v.is_finite()).map(|(&f, &v)| [f, v]).collect();
    if two_sided { fft_shift(&pts) } else { pts }
}

/// Local maxima of the amplitude away from zero frequency, strongest power
/// first.
fn dominant(s: &Spectrum, k: usize) -> Vec<usize> {
    let n = s.amplitude.len();
    let mut peaks: Vec<usize> = (1..n)
        .filter(|&i| s.frequency[i] > 0.0)
        .filter(|&i| s.amplitude[i] >= s.amplitude[i - 1] && (i + 1 >= n || s.amplitude[i] > s.amplitude[i + 1]))
        .collect();
    peaks.sort_by(|&a, &b| s.power[b].total_cmp(&s.power[a]));
    peaks.truncate(k);
    peaks
}

pub(super) fn run(
    table: &DataTable,
    states: &RowStateManager,
    input: &SignalInput,
    settings: &SpectrumSettings,
) -> Result<(String, Vec<ReportSection>, Vec<ReportColumn>)> {
    if settings.padding > 64 {
        return Err(invalid("zero padding of more than 64 times the signal length is not offered"));
    }
    let sig = Signals::extract(table, states, input, 4)?;
    let fs = sig.sampling_rate;
    let two = settings.sides == Sides::Two;
    let mut spectra = Vec::with_capacity(sig.traces.len());
    for t in &sig.traces {
        let opts = SpectrumOptions {
            sampling_rate: fs,
            window: settings.window,
            detrend: settings.detrend,
            fft_length: Some(settings.fft_length(t.values.len())),
            sides: settings.sides,
            scaling: settings.scaling,
        };
        spectra.push(spectrum(&t.values, &opts)?);
    }
    let first = &spectra[0];
    let mut summary = sig.summary_rows();
    summary.extend([
        ("Window".to_owned(), settings.window.label()),
        (
            "Trend removed".to_owned(),
            match settings.detrend {
                Detrend::None => "none",
                Detrend::Constant => "mean",
                Detrend::Linear => "straight line",
            }
            .to_owned(),
        ),
        ("Transform length".to_owned(), first.fft_length.to_string()),
        ("Frequency resolution".to_owned(), format_number(first.resolution, 6)),
        ("Spectrum".to_owned(), if two { "two-sided" } else { "one-sided" }.to_owned()),
    ]);
    let mut sections =
        vec![section(SectionKind::SignalProcessingSummary, "Summary", SectionContent::Table(summary_table(summary)))];
    let raw: Vec<Vec<f64>> = sig.traces.iter().map(|t| t.values.clone()).collect();
    sections.push(section(
        SectionKind::SignalTrace,
        "Signal",
        SectionContent::Plot(Box::new(sig.rows_with_lines(&raw, "Signal", &sig.name))),
    ));
    let freq_label = sig.frequency_label();
    let curves = |f: &dyn Fn(&Spectrum) -> Vec<f64>| -> Vec<Polyline> {
        spectra.iter().map(|sp| ordered(&sp.frequency, &f(sp), two)).collect()
    };
    sections.push(section(
        SectionKind::AmplitudeSpectrum,
        "Amplitude Spectrum",
        SectionContent::Plot(Box::new(sig.curves(
            curves(&|sp| sp.amplitude.clone()),
            &freq_label,
            "Amplitude",
            "Amplitude",
        ))),
    ));
    let power_label = settings.power_label(&sig.frequency_unit);
    let shown_power =
        |p: &[f64]| -> Vec<f64> { if settings.decibels { p.iter().map(|&v| db(v)).collect() } else { p.to_vec() } };
    let power_title = if settings.scaling == Scaling::Density { "Power Spectral Density" } else { "Power Spectrum" };
    sections.push(section(
        SectionKind::PowerSpectrum,
        power_title,
        SectionContent::Plot(Box::new(sig.curves(
            curves(&|sp| shown_power(&sp.power)),
            &freq_label,
            &power_label,
            "Power",
        ))),
    ));
    if settings.phase {
        sections.push(section(
            SectionKind::PhaseSpectrum,
            "Phase Spectrum",
            SectionContent::Plot(Box::new(sig.curves(
                curves(&|sp| sp.phase.iter().map(|p| p.to_degrees()).collect()),
                &freq_label,
                "Phase (degrees)",
                "Phase",
            ))),
        ));
    }
    if settings.peaks > 0 {
        let grouped = sig.group_name.is_some();
        let mut t = TextTable::new(
            grouped
                .then(|| sig.group_name.clone().unwrap_or_default())
                .into_iter()
                .chain(["Rank", "Frequency", "Period", "Amplitude", "Power", "Share of power"].map(str::to_owned)),
        );
        for (trace, sp) in sig.traces.iter().zip(&spectra) {
            let total: f64 = sp.power.iter().sum();
            for (rank, &i) in dominant(sp, settings.peaks).iter().enumerate() {
                let mut row: Vec<String> = Vec::new();
                if grouped {
                    row.push(trace.label.clone());
                }
                let f = sp.frequency[i];
                row.extend([
                    (rank + 1).to_string(),
                    format_number(f, 6),
                    format_number(1.0 / f, 6),
                    format_number(sp.amplitude[i], 5),
                    format_number(sp.power[i], 5),
                    format!("{}%", format_number(100.0 * sp.power[i] / total, 3)),
                ]);
                t.push(row);
            }
        }
        sections.push(section(SectionKind::DominantFrequencies, "Dominant Frequencies", SectionContent::Table(t)));
    }
    if settings.welch {
        let opts = settings.segments(fs)?;
        let mut welch_curves = Vec::new();
        let mut info = None;
        for t in &sig.traces {
            let w = welch(&t.values, &opts)?;
            info.get_or_insert((w.segments, w.segment_length, w.overlap, w.resolution));
            welch_curves.push(ordered(&w.frequency, &shown_power(&w.power), two));
        }
        sections.push(section(
            SectionKind::WelchSpectrum,
            "Welch Averaged Periodogram",
            SectionContent::Plot(Box::new(sig.curves(welch_curves, &freq_label, &power_label, "Welch estimate"))),
        ));
        if let Some((segments, length, overlap, resolution)) = info {
            sections.push(section(
                SectionKind::WelchSpectrum,
                "Welch Segments",
                SectionContent::Note(format!(
                    "{segments} segment{} of {length} samples overlapping by {overlap} ({} window); frequency \
                     resolution {}. Averaging segments lowers the variance of the estimate at the cost of resolution.",
                    if segments == 1 { "" } else { "s" },
                    window_name(settings.window),
                    format_number(resolution, 5)
                )),
            ));
        }
    }
    if settings.spectrogram {
        let opts = settings.segments(fs)?;
        for t in sig.traces.iter().take(MAX_SPECTROGRAMS) {
            let sg = spectrogram(&t.values, &opts)?;
            let t0 = t.time[0];
            let values: Vec<Vec<f64>> = sg
                .power
                .iter()
                .map(|col| {
                    let shown = shown_power(col);
                    if two { fft_shift(&shown) } else { shown }
                })
                .collect();
            let y = if two { fft_shift(&sg.frequency) } else { sg.frequency.clone() };
            let finite = values.iter().flatten().copied().filter(|v| v.is_finite());
            let hi = finite.clone().fold(f64::NEG_INFINITY, f64::max);
            let lo = finite.fold(f64::INFINITY, f64::min);
            let lo = if settings.decibels { lo.max(hi - 100.0) } else { lo };
            let title = if t.label.is_empty() { "Spectrogram".to_owned() } else { format!("Spectrogram: {}", t.label) };
            sections.push(section(
                SectionKind::Spectrogram,
                &title,
                SectionContent::IntensityMap(IntensityMapSpec {
                    x_label: sig.time_label.clone(),
                    y_label: freq_label.clone(),
                    value_label: power_label.clone(),
                    x: sg.time.iter().map(|&tc| t0 + tc).collect(),
                    y,
                    values,
                    y_ticks: None,
                    y_display: None,
                    range: (lo, if hi.is_finite() { hi } else { 1.0 }),
                }),
            ));
        }
    }
    // The spectrum values, which the table menu can turn into a new table.
    let grouped = sig.group_name.is_some();
    let mut t = TextTable::new(
        grouped
            .then(|| sig.group_name.clone().unwrap_or_default())
            .into_iter()
            .chain(["Frequency", "Period", "Amplitude", "Phase (degrees)", "Power"].map(str::to_owned)),
    );
    for (trace, sp) in sig.traces.iter().zip(&spectra) {
        let order: Vec<usize> = if two {
            fft_shift(&(0..sp.frequency.len()).collect::<Vec<_>>())
        } else {
            (0..sp.frequency.len()).collect()
        };
        for i in order {
            let f = sp.frequency[i];
            let mut row: Vec<String> = Vec::new();
            if grouped {
                row.push(trace.label.clone());
            }
            row.extend([
                format_number(f, 8),
                if f == 0.0 { "—".to_owned() } else { format_number(1.0 / f, 6) },
                format_number(sp.amplitude[i], 6),
                format_number(sp.phase[i].to_degrees(), 5),
                format_number(sp.power[i], 6),
            ]);
            t.push(row);
        }
    }
    sections.push(section(SectionKind::FourierValues, "Spectrum Values", SectionContent::Table(t)));
    Ok((format!("Fourier Spectrum: {}", sig.name), sections, Vec::new()))
}
