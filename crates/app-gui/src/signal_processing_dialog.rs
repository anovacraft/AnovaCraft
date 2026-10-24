//! Dialogs for Series and Signals ▸ Signals and spectra: Fourier spectrum, digital filter,
//! convolution and correlation, and wavelets. One report per chosen signal
//! column; every dialog takes an optional x (time) column, an optional group
//! column for signals stacked in long format, and the sampling rate when
//! there is no x column.

use app_model::signal_analysis::correlation::{SCALINGS, scaling_label};
use app_model::signal_analysis::{
    BandKind, CorrelationOperation, CorrelationSettings, DeconvolutionMethod, FilterDesign, FilterSettings, Signals,
    SpectrumSettings, WINDOWS, WaveletSettings, window_name,
};
use app_model::{SavedReport, SignalInput, SignalMethod, SignalProcessingRequest};
use eframe::egui;
use stat_data::{DataTable, RowStateManager};
use stat_signal::convolve::CorrelationScaling;
use stat_signal::filter::{FftFilterShape, IirFamily};
use stat_signal::spectrum::{Detrend, Scaling, Sides};
use stat_signal::wavelet::{ContinuousWavelet, Extension, NoiseEstimate, ThresholdKind, ThresholdRule, Wavelet};
use stat_signal::window::Window;

use crate::dialog_kit::{self as kit, ComboExt as _, Dialog, Outcome, Readiness, Remember};

/// Which Signal item the dialog is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalKind {
    Spectrum,
    Filter,
    Correlation,
    Wavelet,
}

impl SignalKind {
    /// Every kind, in menu order.
    pub const ALL: [Self; 4] = [Self::Spectrum, Self::Filter, Self::Correlation, Self::Wavelet];

    /// Dialog title.
    pub fn title(self) -> &'static str {
        match self {
            Self::Spectrum => "Fourier spectrum",
            Self::Filter => "Digital filter",
            Self::Correlation => "Convolution and correlation",
            Self::Wavelet => "Wavelets",
        }
    }

    /// Menu item label.
    pub fn menu_label(self) -> String {
        format!("{}…", self.title())
    }

    /// Menu item hover text.
    pub fn hint(self) -> &'static str {
        match self {
            Self::Spectrum => {
                "Amplitude, power and phase spectra by the fast Fourier transform, with windows, zero padding, \
                 Welch's averaged periodogram and a spectrogram"
            }
            Self::Filter => {
                "Low-, high-, band-pass and band-stop filters (Butterworth, Chebyshev, elliptic, FIR, FFT) and a \
                 notch, applied forwards or with zero phase"
            }
            Self::Correlation => {
                "Cross- and autocorrelation with the lag of maximum, convolution and deconvolution with a kernel, \
                 and the envelope from the Hilbert transform"
            }
            Self::Wavelet => {
                "Multilevel discrete wavelet decomposition, wavelet denoising and a continuous-wavelet scalogram"
            }
        }
    }

    fn of(method: &SignalMethod) -> Self {
        match method {
            SignalMethod::Spectrum(_) => Self::Spectrum,
            SignalMethod::Filter(_) => Self::Filter,
            SignalMethod::Correlation(_) => Self::Correlation,
            SignalMethod::Wavelet(_) => Self::Wavelet,
        }
    }
}

/// Filter families offered, in menu order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DesignChoice {
    Butterworth,
    ChebyshevI,
    ChebyshevII,
    Elliptic,
    Fir,
    FftIdeal,
    FftSmooth,
    Notch,
}

impl DesignChoice {
    const ALL: [Self; 8] = [
        Self::Butterworth,
        Self::ChebyshevI,
        Self::ChebyshevII,
        Self::Elliptic,
        Self::Fir,
        Self::FftIdeal,
        Self::FftSmooth,
        Self::Notch,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Butterworth => "Butterworth (flattest pass band)",
            Self::ChebyshevI => "Chebyshev type I (ripple in the pass band)",
            Self::ChebyshevII => "Chebyshev type II (ripple in the stop band)",
            Self::Elliptic => "Elliptic (sharpest cut-off)",
            Self::Fir => "FIR, windowed sinc (linear phase)",
            Self::FftIdeal => "FFT, ideal cut-off",
            Self::FftSmooth => "FFT, smooth cut-off",
            Self::Notch => "Notch (remove one frequency)",
        }
    }

    fn iir(self) -> bool {
        matches!(self, Self::Butterworth | Self::ChebyshevI | Self::ChebyshevII | Self::Elliptic)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OperationChoice {
    Cross,
    Auto,
    Convolution,
    Deconvolution,
    Envelope,
}

impl OperationChoice {
    const ALL: [Self; 5] = [Self::Cross, Self::Auto, Self::Convolution, Self::Deconvolution, Self::Envelope];

    fn label(self) -> &'static str {
        match self {
            Self::Cross => "Cross-correlation with a second signal",
            Self::Auto => "Autocorrelation",
            Self::Convolution => "Convolution with a kernel",
            Self::Deconvolution => "Deconvolution by a known kernel",
            Self::Envelope => "Envelope and instantaneous phase (Hilbert transform)",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Cross => "How well the two signals match at each shift, and the shift (lag) where they match best",
            Self::Auto => "How well the signal matches itself shifted: repeating patterns show as peaks",
            Self::Convolution => "Smooth or shape the signal with a kernel (a column of weights)",
            Self::Deconvolution => "Undo a blur by a known kernel (instrument response)",
            Self::Envelope => "The signal's slowly changing amplitude, its phase and its instantaneous frequency",
        }
    }
}

/// Columns and settings of one Series and Signals ▸ Signals and spectra analysis.
#[derive(Clone)]
pub struct SignalProcessingDialog {
    edit: Option<u64>,
    kind: SignalKind,
    chosen: Vec<bool>,
    x: Option<usize>,
    group: Option<usize>,
    set_rate: bool,
    rate: f64,
    spectrum: SpectrumSettings,
    design: DesignChoice,
    order: usize,
    ripple: f64,
    attenuation: f64,
    taps: usize,
    fir_window: Window,
    quality: f64,
    band: BandKind,
    /// Edges; NaN until a sampling rate is known, then a fifth and a half of
    /// the Nyquist frequency.
    low: f64,
    high: f64,
    zero_phase: bool,
    operation: OperationChoice,
    other: Option<usize>,
    scaling: CorrelationScaling,
    auto_lag: bool,
    max_lag: usize,
    normalize: bool,
    richardson_lucy: bool,
    noise_ratio: f64,
    iterations: usize,
    wavelet: WaveletSettings,
    auto_levels: bool,
    levels: usize,
    fixed_threshold: f64,
    /// Sampling rate and Nyquist frequency of the first chosen signal, kept
    /// while the columns and rate stay the same.
    sampling: Option<(SamplingKey, Result<f64, String>)>,
}

type SamplingKey = (Option<usize>, Option<usize>, Option<usize>, Option<u64>, usize);

impl SignalProcessingDialog {
    /// A fresh dialog for `kind`.
    pub fn new(kind: SignalKind, table: &DataTable) -> Self {
        // A date-time column, when the table has one, is the likely x axis.
        let x = (0..table.n_cols()).find(|&i| table.column(i).is_some_and(|c| c.temporal().is_some()));
        Self {
            edit: None,
            kind,
            chosen: vec![false; table.n_cols()],
            x,
            group: None,
            set_rate: false,
            rate: 1.0,
            spectrum: SpectrumSettings::default(),
            design: DesignChoice::Butterworth,
            order: 4,
            ripple: 1.0,
            attenuation: 40.0,
            taps: 61,
            fir_window: Window::Hamming,
            quality: 30.0,
            band: BandKind::Lowpass,
            low: f64::NAN,
            high: f64::NAN,
            zero_phase: true,
            operation: OperationChoice::Cross,
            other: None,
            scaling: CorrelationScaling::Coefficient,
            auto_lag: true,
            max_lag: 50,
            normalize: true,
            richardson_lucy: false,
            noise_ratio: 0.01,
            iterations: 30,
            wavelet: WaveletSettings::default(),
            auto_levels: true,
            levels: 4,
            fixed_threshold: 1.0,
            sampling: None,
        }
    }

    /// The dialog filled from a saved Signal report.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::SignalProcessing { request } = saved else { return None };
        let mut d = Self::new(SignalKind::of(&request.method), table);
        let input = &request.input;
        d.chosen = kit::mask(table.n_cols(), &[input.column]);
        d.x = input.x.and_then(|c| kit::valid(table, c));
        d.group = input.group.and_then(|c| kit::valid(table, c));
        d.set_rate = input.sampling_rate.is_some();
        d.rate = input.sampling_rate.unwrap_or(1.0);
        match &request.method {
            SignalMethod::Spectrum(s) => d.spectrum = s.clone(),
            SignalMethod::Filter(f) => {
                d.band = f.band;
                d.low = f.low;
                d.high = f.high;
                d.zero_phase = f.zero_phase;
                match f.design {
                    FilterDesign::Iir { family, order } => {
                        d.order = order;
                        d.design = match family {
                            IirFamily::Butterworth => DesignChoice::Butterworth,
                            IirFamily::ChebyshevI { ripple_db } => {
                                d.ripple = ripple_db;
                                DesignChoice::ChebyshevI
                            }
                            IirFamily::ChebyshevII { attenuation_db } => {
                                d.attenuation = attenuation_db;
                                DesignChoice::ChebyshevII
                            }
                            IirFamily::Elliptic { ripple_db, attenuation_db } => {
                                (d.ripple, d.attenuation) = (ripple_db, attenuation_db);
                                DesignChoice::Elliptic
                            }
                        };
                    }
                    FilterDesign::Fir { taps, window } => {
                        (d.design, d.taps, d.fir_window) = (DesignChoice::Fir, taps, window);
                    }
                    FilterDesign::Fft { shape: FftFilterShape::Ideal } => d.design = DesignChoice::FftIdeal,
                    FilterDesign::Fft { shape: FftFilterShape::Smooth { order } } => {
                        (d.design, d.order) = (DesignChoice::FftSmooth, order as usize);
                    }
                    FilterDesign::Notch { quality } => (d.design, d.quality) = (DesignChoice::Notch, quality),
                }
            }
            SignalMethod::Correlation(c) => match c.operation {
                CorrelationOperation::Cross { other, scaling, max_lag } => {
                    d.operation = OperationChoice::Cross;
                    d.other = kit::valid(table, other);
                    d.scaling = scaling;
                    d.auto_lag = max_lag.is_none();
                    d.max_lag = max_lag.unwrap_or(50);
                }
                CorrelationOperation::Auto { scaling, max_lag } => {
                    d.operation = OperationChoice::Auto;
                    d.scaling = scaling;
                    d.auto_lag = max_lag.is_none();
                    d.max_lag = max_lag.unwrap_or(50);
                }
                CorrelationOperation::Convolution { kernel, normalize } => {
                    d.operation = OperationChoice::Convolution;
                    d.other = kit::valid(table, kernel);
                    d.normalize = normalize;
                }
                CorrelationOperation::Deconvolution { kernel, method } => {
                    d.operation = OperationChoice::Deconvolution;
                    d.other = kit::valid(table, kernel);
                    match method {
                        DeconvolutionMethod::Wiener { noise_ratio } => d.noise_ratio = noise_ratio,
                        DeconvolutionMethod::RichardsonLucy { iterations } => {
                            (d.richardson_lucy, d.iterations) = (true, iterations);
                        }
                    }
                }
                CorrelationOperation::Envelope => d.operation = OperationChoice::Envelope,
            },
            SignalMethod::Wavelet(w) => {
                d.wavelet = w.clone();
                d.auto_levels = w.levels.is_none();
                d.levels = w.levels.unwrap_or(4);
                if let ThresholdRule::Fixed(l) = w.rule {
                    d.fixed_threshold = l;
                }
            }
        }
        Some(d)
    }

    /// One request per chosen signal column, in table order. Editing a
    /// report replaces it with the first and adds the others.
    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<Vec<SignalProcessingRequest>> {
        let outcome = Dialog::new(self.kind.title()).editing(self.edit).show(ctx, |ui| {
            ui.set_max_width(kit::em(ui) * 34.0);
            kit::note(
                ui,
                match self.kind {
                    SignalKind::Spectrum => {
                        "Find the frequencies a signal is made of: its amplitude, power and phase at each frequency, \
                         the strongest frequencies, and how the spectrum changes over time."
                    }
                    SignalKind::Filter => {
                        "Keep or remove a range of frequencies: smooth out fast noise, take away slow drift, or cut a \
                         single interfering frequency."
                    }
                    SignalKind::Correlation => {
                        "Compare a signal with a delayed copy of another (or itself), apply or undo a kernel, or \
                         trace its envelope."
                    }
                    SignalKind::Wavelet => {
                        "Split a signal into detail at each scale, remove noise by shrinking small wavelet \
                         coefficients, and see which frequencies are present at each moment."
                    }
                },
            );
            ui.add_space(4.0);
            self.contents(ui, table)
        });
        match outcome {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(self.kind, table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    /// Sampling rate of the first chosen signal (as the report will read it,
    /// with every row included).
    fn sampling_rate(&mut self, table: &DataTable, column: Option<usize>) -> Result<f64, String> {
        let rate = self.set_rate.then_some(self.rate);
        let key: SamplingKey = (column, self.x, self.group, rate.map(f64::to_bits), table.n_rows());
        if let Some((k, r)) = &self.sampling {
            if *k == key {
                return r.clone();
            }
        }
        let r = match column {
            None => Err("Choose a signal column".to_owned()),
            Some(column) => {
                let input = SignalInput { column, x: self.x, group: self.group, sampling_rate: rate };
                Signals::extract(table, &RowStateManager::new(table.n_rows()), &input, 2)
                    .map(|s| s.sampling_rate)
                    .map_err(|e| e.to_string())
            }
        };
        self.sampling = Some((key, r.clone()));
        r
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<Vec<SignalProcessingRequest>> {
        let signal_ok = |i: usize| kit::is_numeric(table, i) && table.column(i).is_some_and(|c| c.temporal().is_none());
        ui.label("Signal columns (one report each):");
        kit::column_list(ui, ("signal_columns", self.kind.title()), table, &mut self.chosen, signal_ok);
        ui.label("X column (time or position of each sample; optional):");
        let numeric = |i: usize| kit::is_numeric(table, i);
        kit::column_choice(ui, "signal_x", table, &mut self.x, numeric, Some("None: rows in table order"));
        if self.x.is_none() {
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.set_rate, "Samples per unit of time:")
                    .on_hover_text("Without it, frequencies are in cycles per sample (row)");
                ui.add_enabled(
                    self.set_rate,
                    egui::DragValue::new(&mut self.rate).range(1e-9..=1e12).speed(1.0).max_decimals(6),
                );
            });
        }
        ui.label("Group column (one signal per level, stacked rows; optional):");
        kit::column_choice(ui, "signal_group", table, &mut self.group, |_| true, Some("None: one signal"));
        let columns: Vec<usize> =
            kit::chosen_columns(&self.chosen).into_iter().filter(|&c| c < table.n_cols() && signal_ok(c)).collect();
        let fs = self.sampling_rate(table, columns.first().copied());
        if let Ok(fs) = &fs {
            ui.label(
                egui::RichText::new(format!(
                    "Sampling rate {}; frequencies up to {} (the Nyquist frequency)",
                    app_model::format::format_number(*fs, 6),
                    app_model::format::format_number(fs / 2.0, 6)
                ))
                .weak()
                .small(),
            );
        }
        ui.separator();
        let nyquist = fs.as_ref().map(|f| f / 2.0).ok();
        match self.kind {
            SignalKind::Spectrum => self.spectrum_options(ui),
            SignalKind::Filter => self.filter_options(ui, nyquist),
            SignalKind::Correlation => self.correlation_options(ui, table, signal_ok),
            SignalKind::Wavelet => self.wavelet_options(ui),
        }
        if columns.is_empty() {
            return Err("Choose at least one signal column".into());
        }
        if self.x.is_some_and(|x| columns.contains(&x)) {
            return Err("The x column cannot also be a signal".into());
        }
        if self.group.is_some_and(|g| columns.contains(&g) || self.x == Some(g)) {
            return Err("The group column must differ from the signal and x columns".into());
        }
        let fs = fs?;
        if self.kind == SignalKind::Filter {
            let nyquist = fs / 2.0;
            let edges = if self.design != DesignChoice::Notch && self.band.two_edges() {
                vec![self.low, self.high]
            } else {
                vec![self.low]
            };
            if edges.iter().any(|&f| !(f > 0.0 && f < nyquist)) {
                return Err(format!(
                    "Frequencies must lie between 0 and the Nyquist frequency {}",
                    app_model::format::format_number(nyquist, 6)
                ));
            }
            if edges.len() == 2 && self.low >= self.high {
                return Err("The lower edge must be below the upper edge".into());
            }
        }
        if self.kind == SignalKind::Correlation && self.operation != OperationChoice::Auto {
            if self.operation != OperationChoice::Envelope && self.other.is_none() {
                return Err(if self.operation == OperationChoice::Cross {
                    "Choose the second signal".into()
                } else {
                    "Choose the kernel column".into()
                });
            }
            if self.operation == OperationChoice::Cross && self.other.is_some_and(|o| columns.contains(&o)) {
                return Err("The second signal cannot also be a chosen signal".into());
            }
        }
        let method = self.method();
        let input = |column: usize| SignalInput {
            column,
            x: self.x,
            group: self.group,
            sampling_rate: (self.x.is_none() && self.set_rate).then_some(self.rate),
        };
        let requests: Vec<SignalProcessingRequest> =
            columns.iter().map(|&c| SignalProcessingRequest { input: input(c), method: method.clone() }).collect();
        let summary = match columns.as_slice() {
            [c] => format!("{} of {}", self.kind.title(), kit::name(table, *c)),
            _ => format!("{} reports ({})", columns.len(), self.kind.title().to_lowercase()),
        };
        kit::ready(requests, summary)
    }

    fn method(&self) -> SignalMethod {
        match self.kind {
            SignalKind::Spectrum => SignalMethod::Spectrum(self.spectrum.clone()),
            SignalKind::Filter => {
                let design = match self.design {
                    DesignChoice::Butterworth => {
                        FilterDesign::Iir { family: IirFamily::Butterworth, order: self.order }
                    }
                    DesignChoice::ChebyshevI => FilterDesign::Iir {
                        family: IirFamily::ChebyshevI { ripple_db: self.ripple },
                        order: self.order,
                    },
                    DesignChoice::ChebyshevII => FilterDesign::Iir {
                        family: IirFamily::ChebyshevII { attenuation_db: self.attenuation },
                        order: self.order,
                    },
                    DesignChoice::Elliptic => FilterDesign::Iir {
                        family: IirFamily::Elliptic { ripple_db: self.ripple, attenuation_db: self.attenuation },
                        order: self.order,
                    },
                    DesignChoice::Fir => FilterDesign::Fir { taps: self.taps, window: self.fir_window },
                    DesignChoice::FftIdeal => FilterDesign::Fft { shape: FftFilterShape::Ideal },
                    DesignChoice::FftSmooth => {
                        FilterDesign::Fft { shape: FftFilterShape::Smooth { order: self.order as u32 } }
                    }
                    DesignChoice::Notch => FilterDesign::Notch { quality: self.quality },
                };
                let two = self.design != DesignChoice::Notch && self.band.two_edges();
                SignalMethod::Filter(FilterSettings {
                    design,
                    band: self.band,
                    low: self.low,
                    high: if two { self.high } else { self.low },
                    zero_phase: self.zero_phase,
                })
            }
            SignalKind::Correlation => {
                let max_lag = (!self.auto_lag).then_some(self.max_lag);
                let other = self.other.unwrap_or(0);
                let operation = match self.operation {
                    OperationChoice::Cross => CorrelationOperation::Cross { other, scaling: self.scaling, max_lag },
                    OperationChoice::Auto => CorrelationOperation::Auto { scaling: self.scaling, max_lag },
                    OperationChoice::Convolution => {
                        CorrelationOperation::Convolution { kernel: other, normalize: self.normalize }
                    }
                    OperationChoice::Deconvolution => CorrelationOperation::Deconvolution {
                        kernel: other,
                        method: if self.richardson_lucy {
                            DeconvolutionMethod::RichardsonLucy { iterations: self.iterations }
                        } else {
                            DeconvolutionMethod::Wiener { noise_ratio: self.noise_ratio }
                        },
                    },
                    OperationChoice::Envelope => CorrelationOperation::Envelope,
                };
                SignalMethod::Correlation(CorrelationSettings { operation })
            }
            SignalKind::Wavelet => {
                let rule = match self.wavelet.rule {
                    ThresholdRule::Fixed(_) => ThresholdRule::Fixed(self.fixed_threshold),
                    r => r,
                };
                SignalMethod::Wavelet(WaveletSettings {
                    levels: (!self.auto_levels).then_some(self.levels),
                    rule,
                    ..self.wavelet.clone()
                })
            }
        }
    }

    fn spectrum_options(&mut self, ui: &mut egui::Ui) {
        let s = &mut self.spectrum;
        window_choice(ui, "spectrum_window", "Window:", &mut s.window);
        ui.horizontal(|ui| {
            ui.label("Remove first:");
            ui.radio_value(&mut s.detrend, Detrend::None, "Nothing");
            ui.radio_value(&mut s.detrend, Detrend::Constant, "The mean");
            ui.radio_value(&mut s.detrend, Detrend::Linear, "A straight-line trend");
        });
        ui.horizontal(|ui| {
            ui.label("Zero padding:");
            egui::ComboBox::from_id_salt("spectrum_padding")
                .selected_text(if s.padding <= 1 { "None".to_owned() } else { format!("{}× the length", s.padding) })
                .show_list(ui, |ui| {
                    for p in [1, 2, 4, 8] {
                        let label = if p == 1 { "None".to_owned() } else { format!("{p}× the length") };
                        ui.selectable_value(&mut s.padding, p, label);
                    }
                })
                .response
                .on_hover_text("Pad with zeros to draw the spectrum at finer frequency steps (it does not add detail)");
            ui.checkbox(&mut s.power_of_two, "Round up to a power of two");
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut s.decibels, "Power in decibels");
            ui.checkbox(&mut s.phase, "Phase spectrum");
        });
        ui.horizontal(|ui| {
            ui.label("Dominant frequencies listed:");
            ui.add(egui::DragValue::new(&mut s.peaks).range(0..=50));
        });
        ui.checkbox(&mut s.welch, "Welch's averaged spectrum (less noisy, coarser)")
            .on_hover_text("Average the spectra of overlapping windowed segments");
        ui.checkbox(&mut s.spectrogram, "Spectrogram (spectrum over time)");
        ui.add_enabled_ui(s.welch || s.spectrogram, |ui| {
            ui.horizontal(|ui| {
                ui.label("Segment length:");
                ui.add(egui::DragValue::new(&mut s.segment_length).range(8..=1_048_576));
                ui.label("Overlap (%):");
                ui.add(egui::DragValue::new(&mut s.overlap_percent).range(0.0..=95.0).speed(1.0));
            });
        });
        kit::more_options(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Spectrum:");
                ui.radio_value(&mut s.sides, Sides::One, "One-sided");
                ui.radio_value(&mut s.sides, Sides::Two, "Two-sided (negative frequencies too)");
            });
            ui.horizontal(|ui| {
                ui.label("Power as:");
                ui.radio_value(&mut s.scaling, Scaling::Density, "Density (per unit of frequency)");
                ui.radio_value(&mut s.scaling, Scaling::Spectrum, "Power at each frequency");
            });
        });
    }

    fn filter_options(&mut self, ui: &mut egui::Ui, nyquist: Option<f64>) {
        if let Some(n) = nyquist {
            if !self.low.is_finite() {
                self.low = n / 5.0;
            }
            if !self.high.is_finite() {
                self.high = n / 2.0;
            }
        }
        ui.horizontal(|ui| {
            ui.label("Filter:");
            egui::ComboBox::from_id_salt("filter_design").selected_text(self.design.label()).show_list(ui, |ui| {
                for d in DesignChoice::ALL {
                    ui.selectable_value(&mut self.design, d, d.label());
                }
            });
        });
        let speed = nyquist.map_or(0.01, |n| n / 200.0);
        let top = nyquist.unwrap_or(f64::INFINITY);
        if self.design == DesignChoice::Notch {
            ui.horizontal(|ui| {
                ui.label("Frequency to remove:");
                edge(ui, &mut self.low, top, speed);
                ui.label("Quality factor:");
                ui.add(egui::DragValue::new(&mut self.quality).range(0.5..=1000.0).speed(0.5))
                    .on_hover_text("Frequency ÷ width of the notch: larger removes a narrower band");
            });
        } else {
            ui.horizontal(|ui| {
                ui.label("Band:");
                for b in BandKind::ALL {
                    ui.radio_value(&mut self.band, b, b.label());
                }
            });
            ui.horizontal(|ui| {
                if self.band.two_edges() {
                    ui.label("From");
                    edge(ui, &mut self.low, top, speed);
                    ui.label("to");
                    edge(ui, &mut self.high, top, speed);
                } else {
                    ui.label("Cut-off frequency:");
                    edge(ui, &mut self.low, top, speed);
                }
            });
        }
        if self.design.iir() || self.design == DesignChoice::FftSmooth {
            ui.horizontal(|ui| {
                ui.label("Order:");
                ui.add(egui::DragValue::new(&mut self.order).range(1..=30))
                    .on_hover_text("Higher orders cut off more sharply");
            });
        }
        if matches!(self.design, DesignChoice::ChebyshevI | DesignChoice::Elliptic) {
            ui.horizontal(|ui| {
                ui.label("Pass-band ripple (dB):");
                ui.add(egui::DragValue::new(&mut self.ripple).range(0.01..=20.0).speed(0.05));
            });
        }
        if matches!(self.design, DesignChoice::ChebyshevII | DesignChoice::Elliptic) {
            ui.horizontal(|ui| {
                ui.label("Stop-band attenuation (dB):");
                ui.add(egui::DragValue::new(&mut self.attenuation).range(1.0..=200.0).speed(0.5));
            });
        }
        if self.design == DesignChoice::Fir {
            ui.horizontal(|ui| {
                ui.label("Taps:");
                ui.add(egui::DragValue::new(&mut self.taps).range(3..=4001)).on_hover_text(
                    "More taps cut off more sharply; band-stop and high-pass filters need an odd number",
                );
            });
            window_choice(ui, "filter_window", "Window:", &mut self.fir_window);
        }
        let fft = matches!(self.design, DesignChoice::FftIdeal | DesignChoice::FftSmooth);
        ui.add_enabled(!fft, egui::Checkbox::new(&mut self.zero_phase, "Zero phase (filter forwards and backwards)"))
            .on_hover_text("No delay, and the gain is squared; turn off for a causal filter")
            .on_disabled_hover_text("FFT filters never shift the signal");
    }

    fn correlation_options(&mut self, ui: &mut egui::Ui, table: &DataTable, signal_ok: impl Fn(usize) -> bool) {
        for op in OperationChoice::ALL {
            ui.radio_value(&mut self.operation, op, op.label()).on_hover_text(op.hint());
        }
        match self.operation {
            OperationChoice::Cross => {
                ui.label("Second signal (read with the same x and groups):");
                kit::column_choice(ui, "signal_other", table, &mut self.other, &signal_ok, None);
            }
            OperationChoice::Convolution | OperationChoice::Deconvolution => {
                ui.label("Kernel column (its values in row order):");
                kit::column_choice(ui, "signal_kernel", table, &mut self.other, &signal_ok, None);
            }
            _ => {}
        }
        match self.operation {
            OperationChoice::Cross | OperationChoice::Auto => {
                ui.horizontal(|ui| {
                    ui.label("Normalisation:");
                    egui::ComboBox::from_id_salt("signal_scaling")
                        .selected_text(scaling_label(self.scaling))
                        .show_list(ui, |ui| {
                            for s in SCALINGS {
                                ui.selectable_value(&mut self.scaling, s, scaling_label(s));
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.label("Lags:");
                    ui.radio_value(&mut self.auto_lag, true, "A quarter of the length");
                    ui.radio_value(&mut self.auto_lag, false, "Up to");
                    ui.add_enabled(!self.auto_lag, egui::DragValue::new(&mut self.max_lag).range(1..=1_000_000));
                    ui.label("samples");
                });
            }
            OperationChoice::Convolution => {
                ui.checkbox(&mut self.normalize, "Divide the kernel by its sum (a weighted moving average)");
            }
            OperationChoice::Deconvolution => {
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.richardson_lucy, false, "Wiener, noise-to-signal ratio")
                        .on_hover_text("Larger values give a smoother, less sharpened result");
                    ui.add_enabled(
                        !self.richardson_lucy,
                        egui::DragValue::new(&mut self.noise_ratio).range(0.0..=100.0).speed(0.001).max_decimals(6),
                    );
                });
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.richardson_lucy, true, "Richardson–Lucy, iterations")
                        .on_hover_text("For non-negative data such as counts or intensities");
                    ui.add_enabled(self.richardson_lucy, egui::DragValue::new(&mut self.iterations).range(1..=10_000));
                });
            }
            OperationChoice::Envelope => {}
        }
    }

    fn wavelet_options(&mut self, ui: &mut egui::Ui) {
        let w = &mut self.wavelet;
        ui.horizontal(|ui| {
            ui.label("Wavelet:");
            egui::ComboBox::from_id_salt("wavelet_name")
                .selected_text(w.wavelet.clone())
                .show_list(ui, |ui| {
                    for name in Wavelet::names() {
                        ui.selectable_value(&mut w.wavelet, name.to_owned(), name);
                    }
                })
                .response
                .on_hover_text(
                    "haar: steps; db (Daubechies): compact; sym (symlets): nearly symmetric; coif (coiflets)",
                );
        });
        ui.horizontal(|ui| {
            ui.label("At the ends:");
            for e in [Extension::Symmetric, Extension::Periodic, Extension::Zero] {
                ui.radio_value(&mut w.extension, e, app_model::signal_analysis::wavelets::extension_name(e));
            }
        });
        ui.horizontal(|ui| {
            ui.label("Levels:");
            ui.radio_value(&mut self.auto_levels, true, "As many as the length allows");
            ui.radio_value(&mut self.auto_levels, false, "Set:");
            ui.add_enabled(!self.auto_levels, egui::DragValue::new(&mut self.levels).range(1..=20));
        });
        ui.checkbox(&mut w.denoise, "Denoise (shrink small detail coefficients)");
        ui.add_enabled_ui(w.denoise, |ui| {
            ui.horizontal(|ui| {
                ui.label("Threshold:");
                ui.radio_value(&mut w.rule, ThresholdRule::Universal, "Universal")
                    .on_hover_text("σ√(2 ln n): removes nearly all noise; may flatten small features");
                ui.radio_value(&mut w.rule, ThresholdRule::Sure, "SURE")
                    .on_hover_text("Stein's unbiased risk estimate per level: keeps more detail");
                let fixed = matches!(w.rule, ThresholdRule::Fixed(_));
                if ui.radio(fixed, "Fixed:").clicked() {
                    w.rule = ThresholdRule::Fixed(self.fixed_threshold);
                }
                ui.add_enabled(
                    fixed,
                    egui::DragValue::new(&mut self.fixed_threshold).range(0.0..=1e12).speed(0.01).max_decimals(6),
                );
            });
            ui.horizontal(|ui| {
                ui.label("Shrinkage:");
                ui.radio_value(&mut w.kind, ThresholdKind::Soft, "Soft").on_hover_text("Shrink every coefficient by λ");
                ui.radio_value(&mut w.kind, ThresholdKind::Hard, "Hard")
                    .on_hover_text("Keep coefficients above λ, zero the rest");
            });
            ui.horizontal(|ui| {
                ui.label("Noise level from:");
                ui.radio_value(&mut w.noise, NoiseEstimate::FinestLevel, "The finest level");
                ui.radio_value(&mut w.noise, NoiseEstimate::EachLevel, "Each level (coloured noise)");
            });
        });
        ui.checkbox(&mut w.scalogram, "Scalogram (continuous wavelet transform)");
        ui.add_enabled_ui(w.scalogram, |ui| {
            ui.horizontal(|ui| {
                ui.label("Mother wavelet:");
                ui.radio_value(&mut w.continuous, ContinuousWavelet::Morlet, "Morlet");
                ui.radio_value(&mut w.continuous, ContinuousWavelet::MexicanHat, "Mexican hat");
                ui.label("Scales:");
                ui.add(egui::DragValue::new(&mut w.scales).range(4..=200));
            });
        });
    }
}

/// A drop-down choosing a window.
fn window_choice(ui: &mut egui::Ui, id: &str, label: &str, window: &mut Window) {
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(id).selected_text(window_name(*window)).show_list(ui, |ui| {
            for w in WINDOWS {
                let hint = match w {
                    Window::Rectangular => "No taper: sharpest peaks, most leakage between frequencies",
                    Window::Hann => "A good general choice",
                    Window::Hamming => "Like Hann, with lower nearest side lobes",
                    Window::Blackman => "Low leakage, wider peaks",
                    Window::FlatTop => "Accurate peak amplitudes, very wide peaks",
                    Window::Kaiser { .. } => "Kaiser, β = 8.6",
                    Window::Tukey { .. } => "Flat middle with tapered ends (α = 0.25)",
                };
                let same = std::mem::discriminant(window) == std::mem::discriminant(&w);
                if ui.selectable_label(same, window_name(w)).on_hover_text(hint).clicked() {
                    *window = w;
                }
            }
        });
    });
}

impl Remember for SignalProcessingDialog {
    fn memory_key(&self) -> String {
        self.kind.title().to_owned()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.chosen = kit::mask(table.n_cols(), columns);
    }
}

/// The report's options menu items: display choices that need no new
/// columns. Edits the request in place; the report recomputes.
pub fn options_menu(ui: &mut egui::Ui, request: &mut SignalProcessingRequest) {
    match &mut request.method {
        SignalMethod::Spectrum(s) => {
            ui.checkbox(&mut s.decibels, "Power in decibels");
            ui.checkbox(&mut s.phase, "Phase spectrum");
            ui.checkbox(&mut s.welch, "Welch's averaged spectrum");
            ui.checkbox(&mut s.spectrogram, "Spectrogram");
            ui.menu_button("Window", |ui| {
                for w in WINDOWS {
                    let same = std::mem::discriminant(&s.window) == std::mem::discriminant(&w);
                    if ui.radio(same, window_name(w)).clicked() {
                        s.window = w;
                        ui.close_menu();
                    }
                }
            });
        }
        SignalMethod::Filter(f) => {
            let fft = matches!(f.design, FilterDesign::Fft { .. });
            ui.add_enabled(!fft, egui::Checkbox::new(&mut f.zero_phase, "Zero phase (forwards and backwards)"));
        }
        SignalMethod::Correlation(c) => {
            if let CorrelationOperation::Cross { scaling, .. } | CorrelationOperation::Auto { scaling, .. } =
                &mut c.operation
            {
                ui.menu_button("Normalisation", |ui| {
                    for s in SCALINGS {
                        if ui.radio(*scaling == s, scaling_label(s)).clicked() {
                            *scaling = s;
                            ui.close_menu();
                        }
                    }
                });
            }
        }
        SignalMethod::Wavelet(w) => {
            ui.checkbox(&mut w.denoise, "Denoise");
            ui.checkbox(&mut w.scalogram, "Scalogram");
            ui.add_enabled_ui(w.denoise, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Shrinkage:");
                    ui.radio_value(&mut w.kind, ThresholdKind::Soft, "Soft");
                    ui.radio_value(&mut w.kind, ThresholdKind::Hard, "Hard");
                });
            });
        }
    }
}

/// A frequency edge; until a signal sets the sampling rate it has no value
/// yet, shown as a dash rather than an editable number.
fn edge(ui: &mut egui::Ui, value: &mut f64, top: f64, speed: f64) {
    if value.is_finite() {
        ui.add(egui::DragValue::new(value).range(0.0..=top).speed(speed).max_decimals(6));
    } else {
        ui.weak("— (choose a signal first)");
    }
}
