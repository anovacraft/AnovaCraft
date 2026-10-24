//! Digital filter report: design an IIR, FIR, FFT or notch filter, apply it
//! to each signal (forwards or with zero phase), and show the filtered
//! signal, the filter's frequency and phase responses, its coefficients and
//! the spectrum before and after.

use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_signal::filter::{
    Band, FftFilterShape, FrequencyResponse, IirFamily, Sos, design_fir, design_iir, fft_filter, fft_filter_gain,
    fir_filter, fir_filtfilt, fir_frequency_response, notch,
};
use stat_signal::spectrum::{Detrend, Scaling, Sides, SpectrumOptions, spectrum};
use stat_signal::window::Window;

use super::{
    SignalInput, Signals, count, flag, invalid, number, section, summary_table, text, window_from_json, window_members,
};
use crate::format::format_number;
use crate::project::s;
use crate::report::{PlotSpec, PointGroups, Polyline, ReportSection, SectionContent, SectionKind, TextTable};
use crate::report_columns::ReportColumn;

/// The kind of band a filter passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandKind {
    /// Below one cut-off.
    Lowpass,
    /// Above one cut-off.
    Highpass,
    /// Between two edges.
    Bandpass,
    /// Outside two edges.
    Bandstop,
}

impl BandKind {
    /// Every kind, in menu order.
    pub const ALL: [Self; 4] = [Self::Lowpass, Self::Highpass, Self::Bandpass, Self::Bandstop];

    /// Menu name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Lowpass => "Low pass",
            Self::Highpass => "High pass",
            Self::Bandpass => "Band pass",
            Self::Bandstop => "Band stop",
        }
    }

    /// Project-file key.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Lowpass => "lowpass",
            Self::Highpass => "highpass",
            Self::Bandpass => "bandpass",
            Self::Bandstop => "bandstop",
        }
    }

    /// Whether the band has two edges.
    #[must_use]
    pub fn two_edges(self) -> bool {
        matches!(self, Self::Bandpass | Self::Bandstop)
    }

    fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|b| b.key() == k)
    }
}

/// How the filter is designed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterDesign {
    /// Recursive filter of a family and order, in second-order sections.
    Iir {
        /// Family (with its ripple and attenuation).
        family: IirFamily,
        /// Order (1–30; band filters have twice as many poles).
        order: usize,
    },
    /// Windowed-sinc finite impulse response.
    Fir {
        /// Number of coefficients.
        taps: usize,
        /// Window.
        window: Window,
    },
    /// Gain applied to the signal's Fourier transform.
    Fft {
        /// Ideal or smooth cut-off.
        shape: FftFilterShape,
    },
    /// Second-order notch removing one frequency.
    Notch {
        /// Quality factor `f₀ / bandwidth`.
        quality: f64,
    },
}

impl FilterDesign {
    /// Menu name of the design's kind.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Iir { family, order } => format!("{} IIR, order {order}", family.label()),
            Self::Fir { taps, window } => format!("Windowed-sinc FIR, {taps} taps, {} window", window.label()),
            Self::Fft { shape: FftFilterShape::Ideal } => "FFT filter, ideal cut-off".to_owned(),
            Self::Fft { shape: FftFilterShape::Smooth { order } } => {
                format!("FFT filter, smooth cut-off (Butterworth order {order})")
            }
            Self::Notch { quality } => format!("Notch, quality factor {}", format_number(*quality, 4)),
        }
    }
}

/// Settings of a digital filter report.
#[derive(Debug, Clone, PartialEq)]
pub struct FilterSettings {
    /// The design (default fourth-order Butterworth).
    pub design: FilterDesign,
    /// Band kind (ignored by the notch filter).
    pub band: BandKind,
    /// Cut-off, lower edge, or the notch frequency.
    pub low: f64,
    /// Upper edge of a band filter.
    pub high: f64,
    /// Filter forwards and backwards for zero phase (default true; FFT
    /// filters are always zero-phase).
    pub zero_phase: bool,
}

impl FilterSettings {
    /// A fourth-order Butterworth low-pass filter at `cutoff`.
    #[must_use]
    pub fn lowpass(cutoff: f64) -> Self {
        Self {
            design: FilterDesign::Iir { family: IirFamily::Butterworth, order: 4 },
            band: BandKind::Lowpass,
            low: cutoff,
            high: cutoff,
            zero_phase: true,
        }
    }

    fn band(&self) -> Band {
        match self.band {
            BandKind::Lowpass => Band::Lowpass(self.low),
            BandKind::Highpass => Band::Highpass(self.low),
            BandKind::Bandpass => Band::Bandpass(self.low, self.high),
            BandKind::Bandstop => Band::Bandstop(self.low, self.high),
        }
    }

    fn band_text(&self) -> String {
        match self.design {
            FilterDesign::Notch { .. } => format!("removes {}", format_number(self.low, 6)),
            _ if self.band.two_edges() => {
                format!("{} {} to {}", self.band.label(), format_number(self.low, 6), format_number(self.high, 6))
            }
            _ => format!("{} at {}", self.band.label(), format_number(self.low, 6)),
        }
    }

    pub(super) fn json_members(&self) -> Vec<(String, JsonValue)> {
        let n = JsonValue::number;
        let mut m: Vec<(String, JsonValue)> = Vec::new();
        match self.design {
            FilterDesign::Iir { family, order } => {
                let (key, rp, rs) = match family {
                    IirFamily::Butterworth => ("butterworth", None, None),
                    IirFamily::ChebyshevI { ripple_db } => ("chebyshev1", Some(ripple_db), None),
                    IirFamily::ChebyshevII { attenuation_db } => ("chebyshev2", None, Some(attenuation_db)),
                    IirFamily::Elliptic { ripple_db, attenuation_db } => {
                        ("elliptic", Some(ripple_db), Some(attenuation_db))
                    }
                };
                m.push(("design".into(), s(key)));
                m.push(("order".into(), n(order as f64)));
                if let Some(r) = rp {
                    m.push(("ripple_db".into(), n(r)));
                }
                if let Some(r) = rs {
                    m.push(("attenuation_db".into(), n(r)));
                }
            }
            FilterDesign::Fir { taps, window } => {
                m.push(("design".into(), s("fir")));
                m.push(("taps".into(), n(taps as f64)));
                m.extend(window_members("window", window));
            }
            FilterDesign::Fft { shape } => {
                m.push(("design".into(), s("fft")));
                match shape {
                    FftFilterShape::Ideal => m.push(("cutoff_shape".into(), s("ideal"))),
                    FftFilterShape::Smooth { order } => {
                        m.push(("cutoff_shape".into(), s("smooth")));
                        m.push(("order".into(), n(f64::from(order))));
                    }
                }
            }
            FilterDesign::Notch { quality } => {
                m.push(("design".into(), s("notch")));
                m.push(("quality".into(), n(quality)));
            }
        }
        m.extend([
            ("band".into(), s(self.band.key())),
            ("low".into(), n(self.low)),
            ("high".into(), n(self.high)),
            ("zero_phase".into(), JsonValue::Bool(self.zero_phase)),
        ]);
        m
    }

    /// Read the settings from their project-file (JSON) members; members
    /// left out take their defaults.
    ///
    /// # Errors
    /// Unknown option names.
    pub fn from_json(v: &JsonValue) -> Result<Self> {
        let order = count(v, "order", 4);
        let design = match text(v, "design").unwrap_or("butterworth") {
            "butterworth" => FilterDesign::Iir { family: IirFamily::Butterworth, order },
            "chebyshev1" => {
                FilterDesign::Iir { family: IirFamily::ChebyshevI { ripple_db: number(v, "ripple_db", 1.0) }, order }
            }
            "chebyshev2" => FilterDesign::Iir {
                family: IirFamily::ChebyshevII { attenuation_db: number(v, "attenuation_db", 40.0) },
                order,
            },
            "elliptic" => FilterDesign::Iir {
                family: IirFamily::Elliptic {
                    ripple_db: number(v, "ripple_db", 1.0),
                    attenuation_db: number(v, "attenuation_db", 40.0),
                },
                order,
            },
            "fir" => FilterDesign::Fir {
                taps: count(v, "taps", 61),
                window: window_from_json(v, "window", Window::Hamming)?,
            },
            "fft" => FilterDesign::Fft {
                shape: if text(v, "cutoff_shape") == Some("smooth") {
                    FftFilterShape::Smooth { order: order as u32 }
                } else {
                    FftFilterShape::Ideal
                },
            },
            "notch" => FilterDesign::Notch { quality: number(v, "quality", 30.0) },
            other => return Err(invalid(format!("unknown filter design {other:?}"))),
        };
        let band = match text(v, "band") {
            None => BandKind::Lowpass,
            Some(k) => BandKind::from_key(k).ok_or_else(|| invalid(format!("unknown band {k:?}")))?,
        };
        let low = number(v, "low", f64::NAN);
        Ok(Self { design, band, low, high: number(v, "high", low), zero_phase: flag(v, "zero_phase", true) })
    }
}

/// A designed filter.
#[derive(Debug, Clone, PartialEq)]
pub enum DesignedFilter {
    /// An IIR or notch filter in second-order sections.
    Sections(Sos),
    /// FIR coefficients.
    Taps(Vec<f64>),
    /// A gain applied to the Fourier transform.
    Fft(Band, FftFilterShape),
}

impl DesignedFilter {
    /// Design the filter of `settings` at sampling rate `fs`.
    ///
    /// # Errors
    /// A missing cut-off, edges outside (0, fs/2), an order or tap count out
    /// of range.
    pub fn design(settings: &FilterSettings, fs: f64) -> Result<Self> {
        if !settings.low.is_finite() {
            return Err(invalid("give the cut-off frequency"));
        }
        Ok(match settings.design {
            FilterDesign::Iir { family, order } => Self::Sections(design_iir(family, order, settings.band(), fs)?),
            FilterDesign::Fir { taps, window } => Self::Taps(design_fir(taps, settings.band(), window, fs)?),
            FilterDesign::Fft { shape } => {
                // Validate the band and shape on a short dummy signal.
                fft_filter(&[0.0; 4], settings.band(), shape, fs)?;
                Self::Fft(settings.band(), shape)
            }
            FilterDesign::Notch { quality } => Self::Sections(notch(settings.low, quality, fs)?),
        })
    }

    /// Filter `x` (sampled at `fs`), forwards or with zero phase (FFT
    /// filters are always zero phase).
    ///
    /// # Errors
    /// A signal too short for zero-phase filtering.
    pub fn apply(&self, x: &[f64], zero_phase: bool, fs: f64) -> Result<Vec<f64>> {
        Ok(match self {
            Self::Sections(sos) if zero_phase => sos.filtfilt(x)?,
            Self::Sections(sos) => sos.filter(x),
            Self::Taps(b) if zero_phase => fir_filtfilt(b, x)?,
            Self::Taps(b) => fir_filter(b, x),
            Self::Fft(band, shape) => fft_filter(x, *band, *shape, fs)?,
        })
    }

    /// Frequency response at `points` frequencies from 0 to below fs/2 (of
    /// one forward pass).
    #[must_use]
    pub fn response(&self, points: usize, fs: f64) -> FrequencyResponse {
        match self {
            Self::Sections(sos) => sos.frequency_response(points, fs),
            Self::Taps(b) => fir_frequency_response(b, points, fs),
            Self::Fft(band, shape) => {
                let frequency: Vec<f64> = (0..points).map(|k| fs / 2.0 * k as f64 / points as f64).collect();
                let magnitude: Vec<f64> = frequency.iter().map(|&f| fft_filter_gain(*band, *shape, f)).collect();
                FrequencyResponse {
                    magnitude_db: magnitude.iter().map(|m| 20.0 * m.log10()).collect(),
                    phase: vec![0.0; points],
                    response: magnitude.iter().map(|&m| stat_signal::complex::Complex::real(m)).collect(),
                    frequency,
                    magnitude,
                }
            }
        }
    }
}

const RESPONSE_POINTS: usize = 512;

pub(super) fn run(
    table: &DataTable,
    states: &RowStateManager,
    input: &SignalInput,
    settings: &FilterSettings,
) -> Result<(String, Vec<ReportSection>, Vec<ReportColumn>)> {
    let sig = Signals::extract(table, states, input, 4)?;
    let fs = sig.sampling_rate;
    let designed = DesignedFilter::design(settings, fs)?;
    let zero_phase = settings.zero_phase || matches!(designed, DesignedFilter::Fft(..));
    let mut filtered = Vec::with_capacity(sig.traces.len());
    for t in &sig.traces {
        filtered.push(designed.apply(&t.values, zero_phase, fs).map_err(|e| too_short(&e, &t.label))?);
    }
    let response = designed.response(RESPONSE_POINTS, fs);
    // Applied forwards and backwards, the effective magnitude is squared.
    let applied: Vec<f64> = response
        .magnitude
        .iter()
        .map(|&m| if zero_phase && !matches!(designed, DesignedFilter::Fft(..)) { m * m } else { m })
        .collect();
    let mut summary = sig.summary_rows();
    summary.push(("Filter".to_owned(), settings.design.label()));
    summary.push(("Band".to_owned(), settings.band_text()));
    summary.push((
        "Applied".to_owned(),
        if matches!(designed, DesignedFilter::Fft(..)) {
            "to the Fourier transform (zero phase; the signal is treated as periodic)".to_owned()
        } else if zero_phase {
            "forwards and backwards (zero phase, squared magnitude)".to_owned()
        } else {
            "forwards (causal; output delayed by the filter's phase)".to_owned()
        },
    ));
    if let DesignedFilter::Sections(sos) = &designed {
        summary.push(("Second-order sections".to_owned(), sos.len().to_string()));
        let stable = sos.sections.iter().all(|s| s[5].abs() < 1.0 && s[4].abs() < 1.0 + s[5]);
        summary.push((
            "Stable".to_owned(),
            if stable { "yes (all poles inside the unit circle)" } else { "no" }.to_owned(),
        ));
    }
    let mut sections =
        vec![section(SectionKind::SignalProcessingSummary, "Summary", SectionContent::Table(summary_table(summary)))];
    sections.push(section(
        SectionKind::FilteredSignal,
        "Filtered Signal",
        SectionContent::Plot(Box::new(sig.rows_with_lines(&filtered, "Filtered", &sig.name))),
    ));
    let freq_label = sig.frequency_label();
    let pts = |v: &[f64]| -> Polyline {
        response.frequency.iter().zip(v).filter(|(_, y)| y.is_finite()).map(|(&f, &y)| [f, y]).collect()
    };
    // Deep stop bands are clipped at −150 dB so the pass band stays readable.
    let gain_db: Vec<f64> = applied.iter().map(|m| (20.0 * m.log10()).max(-150.0)).collect();
    let lowest = gain_db.iter().copied().fold(0.0, f64::min);
    sections.push(section(
        SectionKind::FilterResponse,
        "Frequency Response",
        SectionContent::Plot(Box::new(PlotSpec {
            x_label: freq_label.clone(),
            y_label: "Gain (dB)".to_owned(),
            line: Some(pts(&gain_db)),
            line_name: Some(if zero_phase && !matches!(designed, DesignedFilter::Fft(..)) {
                "Gain as applied (squared)".to_owned()
            } else {
                "Gain".to_owned()
            }),
            segments: edge_marks(settings, lowest, 3.0),
            ..PlotSpec::default()
        })),
    ));
    if !zero_phase {
        sections.push(section(
            SectionKind::FilterPhase,
            "Phase Response",
            SectionContent::Plot(Box::new(PlotSpec {
                x_label: freq_label.clone(),
                y_label: "Phase (degrees)".to_owned(),
                line: Some(pts(&response.phase.iter().map(|p| p.to_degrees()).collect::<Vec<_>>())),
                line_name: Some("Phase".to_owned()),
                ..PlotSpec::default()
            })),
        ));
    }
    // Amplitude spectrum of the first signal before and after.
    let t0 = &sig.traces[0];
    let opts = SpectrumOptions {
        sampling_rate: fs,
        window: Window::Hann,
        detrend: Detrend::Constant,
        fft_length: None,
        sides: Sides::One,
        scaling: Scaling::Spectrum,
    };
    let before = spectrum(&t0.values, &opts)?;
    let after = spectrum(&filtered[0], &opts)?;
    let line = |sp: &stat_signal::spectrum::Spectrum| -> Polyline {
        sp.frequency.iter().zip(&sp.amplitude).map(|(&f, &a)| [f, a]).collect()
    };
    let title = if t0.label.is_empty() {
        "Spectrum Before and After".to_owned()
    } else {
        format!("Spectrum Before and After: {}", t0.label)
    };
    sections.push(section(
        SectionKind::FilterSpectrumComparison,
        &title,
        SectionContent::Plot(Box::new(PlotSpec {
            x_label: freq_label,
            y_label: "Amplitude".to_owned(),
            groups: Some(PointGroups {
                title: String::new(),
                labels: vec!["Before filtering".to_owned(), "After filtering".to_owned()],
                membership: Vec::new(),
                lines: vec![line(&before), line(&after)],
            }),
            ..PlotSpec::default()
        })),
    ));
    match &designed {
        DesignedFilter::Sections(sos) => {
            let mut t = TextTable::new(["Section", "b₀", "b₁", "b₂", "a₁", "a₂"]);
            for (i, sec) in sos.sections.iter().enumerate() {
                t.push([
                    (i + 1).to_string(),
                    format_number(sec[0], 8),
                    format_number(sec[1], 8),
                    format_number(sec[2], 8),
                    format_number(sec[4], 8),
                    format_number(sec[5], 8),
                ]);
            }
            sections.push(section(SectionKind::FilterCoefficients, "Second-Order Sections", SectionContent::Table(t)));
        }
        DesignedFilter::Taps(b) => {
            let mut t = TextTable::new(["Tap", "Coefficient"]);
            for (i, v) in b.iter().enumerate() {
                t.push([i.to_string(), format_number(*v, 8)]);
            }
            sections.push(section(SectionKind::FilterCoefficients, "FIR Coefficients", SectionContent::Table(t)));
        }
        DesignedFilter::Fft(..) => {}
    }
    let mut filtered_rows = vec![f64::NAN; sig.n_rows];
    let mut removed_rows = vec![f64::NAN; sig.n_rows];
    for (t, y) in sig.traces.iter().zip(&filtered) {
        t.fill_rows(y, &mut filtered_rows);
        let resid: Vec<f64> = t.values.iter().zip(y).map(|(a, b)| a - b).collect();
        t.fill_rows(&resid, &mut removed_rows);
    }
    let columns = vec![
        ReportColumn::continuous("filtered", "Save filtered signal", format!("Filtered {}", sig.name), filtered_rows),
        ReportColumn::continuous(
            "removed",
            "Save removed part (signal − filtered)",
            format!("Removed from {}", sig.name),
            removed_rows,
        ),
    ];
    Ok((format!("Digital Filter: {}", sig.name), sections, columns))
}

fn too_short(e: &stat_core::StatError, label: &str) -> stat_core::StatError {
    let which = if label.is_empty() { String::new() } else { format!(" (group {label})") };
    invalid(format!("the signal{which} is too short for zero-phase filtering with this filter: {e}"))
}

/// Vertical marks at the band edges from `bottom` to `top` dB.
fn edge_marks(settings: &FilterSettings, bottom: f64, top: f64) -> Vec<Polyline> {
    let mut out = Vec::new();
    let edges: Vec<f64> = match settings.design {
        FilterDesign::Notch { .. } => vec![settings.low],
        _ if settings.band.two_edges() => vec![settings.low, settings.high],
        _ => vec![settings.low],
    };
    for f in edges {
        out.push(vec![[f, bottom], [f, top]]);
    }
    out
}
