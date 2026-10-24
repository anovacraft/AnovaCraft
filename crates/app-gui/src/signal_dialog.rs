//! Dialogs for Series and Signals ▸ Signals and spectra: smoothing, derivatives and
//! integrals of x–y data, and spectra (baseline correction, peak finding and
//! peak fitting). Baseline correction, peak finding and peak fitting are one
//! report (`SignalRequest::Spectrum`) opened with different defaults.

use app_model::signal_curves::MAX_FIT_PEAKS;
use app_model::{
    CalculusRequest, DerivativeMethod, PeakDetection, PeakFitSettings, ResampleSettings, SavedReport, SignalRequest,
    SignalSmoothingRequest, SpectrumBaseline, SpectrumRequest,
};
use eframe::egui;
use stat_data::DataTable;
use stat_signal::peak_fit::{BaselineTerm, Spacing};
use stat_signal::{BaselineMethod, Integration, Interpolation, MovingAverage, PeakShape, Smoother};

use crate::dialog_kit::ComboExt as _;
use crate::dialog_kit::{self as kit, Dialog, Outcome, Remember};
use crate::report_view::ReportRequest;

/// Which menu item the dialog is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalKind {
    Smoothing,
    Calculus,
    Baseline,
    Peaks,
    PeakFit,
}

impl SignalKind {
    fn title(self) -> &'static str {
        match self {
            Self::Smoothing => "Signal smoothing",
            Self::Calculus => "Derivatives and integrals",
            Self::Baseline => "Baseline correction",
            Self::Peaks => "Peak finding",
            Self::PeakFit => "Peak fitting",
        }
    }
}

/// The Signals and spectra menu items: kind, label and hover text.
pub const MENU: [(SignalKind, &str, &str); 5] = [
    (
        SignalKind::Smoothing,
        "Smoothing…",
        "Savitzky–Golay, moving average, exponential, running median or percentile, or Gaussian kernel \
         smoothing, with the raw and smoothed signals overlaid",
    ),
    (
        SignalKind::Calculus,
        "Derivatives and integrals…",
        "Derivative (central differences or Savitzky–Golay), running integral (trapezoid or Simpson) and \
         resampling onto an even grid by linear, spline or shape-preserving interpolation",
    ),
    (
        SignalKind::Baseline,
        "Baseline correction…",
        "Estimate and remove a spectrum's background: asymmetric least squares, arPLS, SNIP, or a known \
         baseline column",
    ),
    (
        SignalKind::Peaks,
        "Peak finding…",
        "Peaks above the noise by height, prominence and separation, after optional Savitzky–Golay smoothing: \
         positions, heights, widths and areas",
    ),
    (
        SignalKind::PeakFit,
        "Peak fitting…",
        "Fit overlapping Gaussian, Lorentzian, Voigt or asymmetric peaks with a baseline: centres, heights, \
         widths and areas with standard errors, and a comparison of peak counts; every curve in one table",
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SmoothKind {
    SavitzkyGolay,
    Centred,
    Trailing,
    Exponential,
    Median,
    Percentile,
    Gaussian,
}

const SMOOTHERS: [(SmoothKind, &str, &str); 7] = [
    (
        SmoothKind::SavitzkyGolay,
        "Savitzky–Golay",
        "A polynomial fitted by least squares around each point: keeps peak heights and widths better than a \
         moving average",
    ),
    (SmoothKind::Centred, "Moving average (centred)", "The mean of the points around each point"),
    (SmoothKind::Trailing, "Moving average (trailing)", "The mean of each point and the ones before it"),
    (SmoothKind::Exponential, "Exponential", "sᵢ = α·yᵢ + (1 − α)·sᵢ₋₁: recent points weigh most"),
    (SmoothKind::Median, "Running median", "The median of the points around each point: ignores isolated spikes"),
    (SmoothKind::Percentile, "Running percentile", "A chosen percentile of the points around each point"),
    (SmoothKind::Gaussian, "Gaussian kernel", "A weighted mean with bell-shaped weights of a chosen width"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BaselineMode {
    None,
    Asls,
    Arpls,
    Snip,
    Column,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpacingMode {
    Free,
    Equal,
    Fixed,
}

/// Columns and settings of a Signals and spectra analysis.
#[derive(Clone)]
pub struct SignalDialog {
    edit: Option<u64>,
    kind: SignalKind,
    x: Option<usize>,
    y: Option<usize>,
    group: Option<usize>,
    // Smoothing.
    smoother: SmoothKind,
    window: usize,
    order: usize,
    derivative: usize,
    alpha: f64,
    percentile: f64,
    sigma: f64,
    // Calculus.
    sg_derivative: bool,
    d_window: usize,
    d_order: usize,
    simpson: bool,
    resample: bool,
    interpolation: Interpolation,
    points: usize,
    // Spectrum.
    baseline: BaselineMode,
    lambda_exp: f64,
    asymmetry: f64,
    half_window: usize,
    decreasing: bool,
    lls: bool,
    baseline_column: Option<usize>,
    detection: PeakDetection,
    pre_smooth: bool,
    ps_window: usize,
    ps_order: usize,
    fit: bool,
    fit_settings: PeakFitSettings,
    count_auto: bool,
    count: usize,
    centres: String,
    spacing: SpacingMode,
    spacing_value: f64,
}

impl SignalDialog {
    /// A fresh dialog for `kind`.
    pub fn new(kind: SignalKind, _table: &DataTable) -> Self {
        Self {
            edit: None,
            kind,
            x: None,
            y: None,
            group: None,
            smoother: SmoothKind::SavitzkyGolay,
            window: 11,
            order: 3,
            derivative: 0,
            alpha: 0.3,
            percentile: 50.0,
            sigma: 2.0,
            sg_derivative: false,
            d_window: 11,
            d_order: 3,
            simpson: false,
            resample: false,
            interpolation: Interpolation::Pchip,
            points: 101,
            baseline: BaselineMode::Arpls,
            lambda_exp: 5.0,
            asymmetry: 0.01,
            half_window: 20,
            decreasing: true,
            lls: false,
            baseline_column: None,
            detection: PeakDetection::default(),
            pre_smooth: true,
            ps_window: 11,
            ps_order: 3,
            fit: kind == SignalKind::PeakFit,
            fit_settings: PeakFitSettings::default(),
            count_auto: true,
            count: 2,
            centres: String::new(),
            spacing: SpacingMode::Free,
            spacing_value: 10.0,
        }
    }

    /// The dialog filled from a saved Signals and spectra report.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::Signal { request } = saved else { return None };
        let valid = |c: usize| kit::valid(table, c);
        Some(match request {
            SignalRequest::Smoothing(r) => {
                let mut d = Self::new(SignalKind::Smoothing, table);
                d.x = r.x.and_then(valid);
                d.y = valid(r.y);
                d.group = r.group.and_then(valid);
                match r.method {
                    Smoother::SavitzkyGolay { window, order, derivative } => {
                        (d.smoother, d.window, d.order, d.derivative) =
                            (SmoothKind::SavitzkyGolay, window, order, derivative);
                    }
                    Smoother::MovingAverage { window, kind } => {
                        d.window = window;
                        d.smoother =
                            if kind == MovingAverage::Centred { SmoothKind::Centred } else { SmoothKind::Trailing };
                    }
                    Smoother::Exponential { alpha } => (d.smoother, d.alpha) = (SmoothKind::Exponential, alpha),
                    Smoother::RunningMedian { window } => (d.smoother, d.window) = (SmoothKind::Median, window),
                    Smoother::RunningPercentile { window, percentile } => {
                        (d.smoother, d.window, d.percentile) = (SmoothKind::Percentile, window, percentile);
                    }
                    Smoother::Gaussian { sigma } => (d.smoother, d.sigma) = (SmoothKind::Gaussian, sigma),
                }
                d
            }
            SignalRequest::Calculus(r) => {
                let mut d = Self::new(SignalKind::Calculus, table);
                d.x = valid(r.x);
                d.y = valid(r.y);
                d.group = r.group.and_then(valid);
                if let DerivativeMethod::SavitzkyGolay { window, order } = r.derivative {
                    (d.sg_derivative, d.d_window, d.d_order) = (true, window, order);
                }
                d.simpson = r.integral == Integration::Simpson;
                if let Some(s) = r.resample {
                    (d.resample, d.interpolation, d.points) = (true, s.method, s.points);
                }
                d
            }
            SignalRequest::Spectrum(r) => {
                let kind = if r.fit.is_some() { SignalKind::PeakFit } else { SignalKind::Peaks };
                let mut d = Self::new(kind, table);
                d.x = valid(r.x);
                d.y = valid(r.y);
                d.group = r.curve.and_then(valid);
                match r.baseline {
                    SpectrumBaseline::None => d.baseline = BaselineMode::None,
                    SpectrumBaseline::Column(c) => (d.baseline, d.baseline_column) = (BaselineMode::Column, valid(c)),
                    SpectrumBaseline::Estimated(BaselineMethod::Asls { lambda, p }) => {
                        (d.baseline, d.lambda_exp, d.asymmetry) = (BaselineMode::Asls, lambda.log10(), p);
                    }
                    SpectrumBaseline::Estimated(BaselineMethod::Arpls { lambda }) => {
                        (d.baseline, d.lambda_exp) = (BaselineMode::Arpls, lambda.log10());
                    }
                    SpectrumBaseline::Estimated(BaselineMethod::Snip { half_window, decreasing, lls }) => {
                        (d.baseline, d.half_window, d.decreasing, d.lls) =
                            (BaselineMode::Snip, half_window, decreasing, lls);
                    }
                }
                d.detection = r.detection;
                d.pre_smooth = r.detection.smoothing.is_some();
                if let Some((w, o)) = r.detection.smoothing {
                    (d.ps_window, d.ps_order) = (w, o);
                }
                if let Some(f) = &r.fit {
                    d.fit = true;
                    d.fit_settings = f.clone();
                    d.count_auto = f.count.is_none();
                    d.count = f.count.unwrap_or(2);
                    d.centres = f.centres.iter().map(|c| format!("{c}")).collect::<Vec<_>>().join(", ");
                    (d.spacing, d.spacing_value) = match f.spacing {
                        Spacing::Free => (SpacingMode::Free, 10.0),
                        Spacing::Equal => (SpacingMode::Equal, 10.0),
                        Spacing::Fixed(v) => (SpacingMode::Fixed, v),
                    };
                }
                d
            }
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<Box<ReportRequest>> {
        let outcome = Dialog::new(self.kind.title()).editing(self.edit).show(ctx, |ui| {
            ui.set_min_width(kit::em(ui) * 34.0);
            kit::note(
                ui,
                match self.kind {
                    SignalKind::Smoothing => {
                        "Smooth a noisy signal recorded at evenly spaced points, and see the raw and smoothed \
                         signals together."
                    }
                    SignalKind::Calculus => {
                        "Rates of change, running totals (areas) and evenly spaced resampling of y against x; \
                         x need not be evenly spaced."
                    }
                    SignalKind::Baseline => {
                        "Remove the slowly varying background under a spectrum or chromatogram, leaving the \
                         peaks on a flat zero line."
                    }
                    SignalKind::Peaks => "Find the peaks that stand out from the noise, after removing the baseline.",
                    SignalKind::PeakFit => {
                        "Separate overlapping peaks by fitting a sum of peak shapes, and compare models with \
                         different numbers of peaks."
                    }
                },
            );
            ui.add_space(4.0);
            let ready = match self.kind {
                SignalKind::Smoothing => self.smoothing(ui, table).map(|r| (SignalRequest::Smoothing(r.0), r.1)),
                SignalKind::Calculus => self.calculus(ui, table).map(|r| (SignalRequest::Calculus(r.0), r.1)),
                _ => self.spectrum(ui, table).map(|r| (SignalRequest::Spectrum(r.0), r.1)),
            };
            ready.map(|(request, summary)| kit::Ready {
                output: Box::new(ReportRequest::Comparison(SavedReport::Signal { request })),
                summary,
            })
        });
        match outcome {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(self.kind, table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    /// x, y and group column choices; `x_label` `None` when x is optional.
    fn columns(&mut self, ui: &mut egui::Ui, table: &DataTable, x_required: bool, group_label: &str) {
        let numeric = |i: usize| kit::is_numeric(table, i);
        egui::Grid::new(("signal_cols", self.kind.title())).num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label(if self.kind == SignalKind::Smoothing { "Signal:" } else { "y (intensity):" });
            kit::column_choice(ui, "sig_y", table, &mut self.y, numeric, None);
            ui.end_row();
            ui.label(if x_required { "x (position):" } else { "Order by (optional):" });
            kit::column_choice(ui, "sig_x", table, &mut self.x, numeric, (!x_required).then_some("None: row order"));
            ui.end_row();
            ui.label(group_label);
            kit::column_choice(ui, "sig_group", table, &mut self.group, |_| true, Some("None: one signal"));
            ui.end_row();
        });
    }

    fn check_columns(&self, x_required: bool) -> Result<usize, String> {
        let Some(y) = self.y else { return Err("Choose the y column".into()) };
        if x_required && self.x.is_none() {
            return Err("Choose the x column".into());
        }
        if self.x == Some(y) {
            return Err("x and y must be different columns".into());
        }
        if self.group.is_some_and(|g| Some(g) == self.x || g == y) {
            return Err("The grouping column must differ from x and y".into());
        }
        Ok(y)
    }

    fn smoother(&self) -> Smoother {
        match self.smoother {
            SmoothKind::SavitzkyGolay => {
                Smoother::SavitzkyGolay { window: self.window, order: self.order, derivative: self.derivative }
            }
            SmoothKind::Centred => Smoother::MovingAverage { window: self.window, kind: MovingAverage::Centred },
            SmoothKind::Trailing => Smoother::MovingAverage { window: self.window, kind: MovingAverage::Trailing },
            SmoothKind::Exponential => Smoother::Exponential { alpha: self.alpha },
            SmoothKind::Median => Smoother::RunningMedian { window: self.window },
            SmoothKind::Percentile => Smoother::RunningPercentile { window: self.window, percentile: self.percentile },
            SmoothKind::Gaussian => Smoother::Gaussian { sigma: self.sigma },
        }
    }

    fn smoothing(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Result<(SignalSmoothingRequest, String), String> {
        self.columns(ui, table, false, "One signal per:");
        ui.separator();
        ui.label("Method:");
        for (k, label, hint) in SMOOTHERS {
            ui.radio_value(&mut self.smoother, k, label).on_hover_text(hint);
        }
        let windowed = !matches!(self.smoother, SmoothKind::Exponential | SmoothKind::Gaussian);
        let odd = !matches!(self.smoother, SmoothKind::Trailing);
        ui.horizontal(|ui| {
            if windowed {
                ui.label("Window (points):");
                ui.add(egui::DragValue::new(&mut self.window).range(if odd { 3..=1001 } else { 1..=1001 }))
                    .on_hover_text(if odd { "Odd: the point itself and as many on each side" } else { "" });
            }
            match self.smoother {
                SmoothKind::SavitzkyGolay => {
                    ui.label("Degree:");
                    ui.add(egui::DragValue::new(&mut self.order).range(0..=10));
                    ui.label("Derivative:");
                    ui.add(egui::DragValue::new(&mut self.derivative).range(0..=self.order.min(4)))
                        .on_hover_text("0 gives the smoothed signal; 1 its slope, 2 its curvature (per x unit)");
                }
                SmoothKind::Exponential => {
                    ui.label("Weight α:");
                    ui.add(egui::DragValue::new(&mut self.alpha).range(0.01..=1.0).speed(0.01));
                }
                SmoothKind::Percentile => {
                    ui.label("Percentile:");
                    ui.add(egui::DragValue::new(&mut self.percentile).range(0.0..=100.0).speed(1.0));
                }
                SmoothKind::Gaussian => {
                    ui.label("Standard deviation (points):");
                    ui.add(egui::DragValue::new(&mut self.sigma).range(0.1..=200.0).speed(0.1));
                }
                _ => {}
            }
        });
        let y = self.check_columns(false)?;
        if windowed && odd && self.window % 2 == 0 {
            return Err("The window must be an odd number of points".into());
        }
        if self.smoother == SmoothKind::SavitzkyGolay && self.order >= self.window {
            return Err("The degree must be smaller than the window".into());
        }
        let method = self.smoother();
        let summary = format!("{} of {}", method.label(), kit::name(table, y));
        Ok((SignalSmoothingRequest { x: self.x, y, group: self.group, method }, summary))
    }

    fn calculus(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Result<(CalculusRequest, String), String> {
        self.columns(ui, table, true, "One curve per:");
        ui.separator();
        ui.label("Derivative:");
        ui.radio_value(&mut self.sg_derivative, false, "Central differences")
            .on_hover_text("From each point's neighbours; any spacing; exact for parabolas");
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.sg_derivative, true, "Savitzky–Golay, window")
                .on_hover_text("Smooths while differentiating; needs evenly spaced x");
            ui.add_enabled(self.sg_derivative, egui::DragValue::new(&mut self.d_window).range(3..=1001));
            ui.label("degree");
            ui.add_enabled(self.sg_derivative, egui::DragValue::new(&mut self.d_order).range(1..=10));
        });
        ui.horizontal(|ui| {
            ui.label("Integral:");
            ui.radio_value(&mut self.simpson, false, "Trapezoid rule");
            ui.radio_value(&mut self.simpson, true, "Simpson's rule")
                .on_hover_text("Parabolas through neighbouring points: more accurate for smooth curves");
        });
        ui.checkbox(&mut self.resample, "Resample onto evenly spaced x");
        ui.add_enabled_ui(self.resample, |ui| {
            ui.horizontal(|ui| {
                ui.label("Points:");
                ui.add(egui::DragValue::new(&mut self.points).range(2..=100_000));
                egui::ComboBox::from_id_salt("sig_interp").selected_text(self.interpolation.label()).show_list(
                    ui,
                    |ui| {
                        for m in Interpolation::ALL {
                            ui.selectable_value(&mut self.interpolation, m, m.label());
                        }
                    },
                );
            });
        });
        let y = self.check_columns(true)?;
        if self.sg_derivative && (self.d_window % 2 == 0 || self.d_order >= self.d_window) {
            return Err("The Savitzky–Golay window must be odd and larger than the degree".into());
        }
        let request = CalculusRequest {
            x: self.x.unwrap_or(0),
            y,
            group: self.group,
            derivative: if self.sg_derivative {
                DerivativeMethod::SavitzkyGolay { window: self.d_window, order: self.d_order }
            } else {
                DerivativeMethod::CentralDifferences
            },
            integral: if self.simpson { Integration::Simpson } else { Integration::Trapezoid },
            resample: self.resample.then_some(ResampleSettings { method: self.interpolation, points: self.points }),
        };
        let summary = format!("Derivative and integral of {} against {}", kit::name(table, y), kit::name(table, request.x));
        Ok((request, summary))
    }

    fn spectrum(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Result<(SpectrumRequest, String), String> {
        self.columns(ui, table, true, "One spectrum per:");
        ui.separator();
        ui.label("Baseline:");
        ui.horizontal_wrapped(|ui| {
            for (m, label, hint) in [
                (
                    BaselineMode::Arpls,
                    "arPLS",
                    "Asymmetrically reweighted penalized least squares (Baek et al. 2015): a smooth curve under \
                     the peaks, with weights set from the noise; one setting (smoothness)",
                ),
                (
                    BaselineMode::Asls,
                    "Asymmetric least squares",
                    "Eilers & Boelens (2005): a smooth curve that points above it pull up only a little \
                     (asymmetry p)",
                ),
                (
                    BaselineMode::Snip,
                    "SNIP",
                    "Statistics-sensitive non-linear iterative peak clipping (Ryan et al. 1988): clips peaks \
                     narrower than the window; suits counting spectra",
                ),
                (BaselineMode::Column, "Known (a column)", "Subtract a baseline measured or computed elsewhere"),
                (BaselineMode::None, "None", "The data already sit on zero"),
            ] {
                ui.radio_value(&mut self.baseline, m, label).on_hover_text(hint);
            }
        });
        ui.horizontal(|ui| match self.baseline {
            BaselineMode::Arpls | BaselineMode::Asls => {
                ui.label("Smoothness log₁₀ λ:");
                ui.add(egui::DragValue::new(&mut self.lambda_exp).range(0.0..=12.0).speed(0.1))
                    .on_hover_text("Larger is stiffer; 5 (λ = 10⁵) suits a few hundred to a few thousand points");
                if self.baseline == BaselineMode::Asls {
                    ui.label("Asymmetry p:");
                    ui.add(egui::DragValue::new(&mut self.asymmetry).range(0.0001..=0.5).speed(0.001));
                }
            }
            BaselineMode::Snip => {
                ui.label("Half-window (points):");
                ui.add(egui::DragValue::new(&mut self.half_window).range(1..=1000))
                    .on_hover_text("A little more than half the widest peak's width in points");
                ui.checkbox(&mut self.decreasing, "Decreasing windows");
                ui.checkbox(&mut self.lls, "Log–log–square-root scale")
                    .on_hover_text("For counts spanning orders of magnitude; needs values ≥ 0");
            }
            BaselineMode::Column => {
                ui.label("Baseline column:");
                kit::column_choice(ui, "sig_base", table, &mut self.baseline_column, |i| kit::is_numeric(table, i), None);
            }
            BaselineMode::None => {}
        });
        ui.separator();
        ui.label("Peak finding (thresholds in multiples of the noise level σ̂):");
        ui.horizontal(|ui| {
            ui.label("Height at least");
            ui.add(egui::DragValue::new(&mut self.detection.min_height).range(0.0..=1000.0).speed(0.1));
            ui.label("Prominence at least");
            ui.add(egui::DragValue::new(&mut self.detection.min_prominence).range(0.0..=1000.0).speed(0.1))
                .on_hover_text("How far a peak rises above the higher of the lowest points on either side");
        });
        ui.horizontal(|ui| {
            ui.label("Peaks at least");
            ui.add(egui::DragValue::new(&mut self.detection.min_separation).range(0.0..=f64::MAX).speed(0.5));
            ui.label("x units apart");
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.pre_smooth, "Smooth before searching (Savitzky–Golay), window");
            ui.add_enabled(self.pre_smooth, egui::DragValue::new(&mut self.ps_window).range(3..=1001));
            ui.label("degree");
            ui.add_enabled(self.pre_smooth, egui::DragValue::new(&mut self.ps_order).range(0..=10));
        });
        ui.separator();
        ui.checkbox(&mut self.fit, "Fit a peak model");
        if self.fit {
            self.fit_options(ui);
        }
        let y = self.check_columns(true)?;
        if self.baseline == BaselineMode::Column && self.baseline_column.is_none() {
            return Err("Choose the baseline column".into());
        }
        if self.pre_smooth && (self.ps_window % 2 == 0 || self.ps_order >= self.ps_window) {
            return Err("The smoothing window must be odd and larger than the degree".into());
        }
        let centres = parse_centres(&self.centres)?;
        if centres.len() > MAX_FIT_PEAKS {
            return Err(format!("At most {MAX_FIT_PEAKS} peaks"));
        }
        let baseline = match self.baseline {
            BaselineMode::None => SpectrumBaseline::None,
            BaselineMode::Column => SpectrumBaseline::Column(self.baseline_column.unwrap_or(0)),
            BaselineMode::Arpls => SpectrumBaseline::Estimated(BaselineMethod::Arpls { lambda: 10f64.powf(self.lambda_exp) }),
            BaselineMode::Asls => SpectrumBaseline::Estimated(BaselineMethod::Asls {
                lambda: 10f64.powf(self.lambda_exp),
                p: self.asymmetry,
            }),
            BaselineMode::Snip => SpectrumBaseline::Estimated(BaselineMethod::Snip {
                half_window: self.half_window,
                decreasing: self.decreasing,
                lls: self.lls,
            }),
        };
        let fit = self.fit.then(|| PeakFitSettings {
            count: (!self.count_auto).then_some(self.count),
            centres,
            spacing: match self.spacing {
                SpacingMode::Free => Spacing::Free,
                SpacingMode::Equal => Spacing::Equal,
                SpacingMode::Fixed => Spacing::Fixed(self.spacing_value),
            },
            ..self.fit_settings.clone()
        });
        let x = self.x.unwrap_or(0);
        let summary = match &fit {
            Some(f) => format!("{} peak fit of {}", f.shape.label(), kit::name(table, y)),
            None => format!("Peaks of {} against {}", kit::name(table, y), kit::name(table, x)),
        };
        let detection = PeakDetection { smoothing: self.pre_smooth.then_some((self.ps_window, self.ps_order)), ..self.detection };
        Ok((SpectrumRequest { x, y, curve: self.group, baseline, detection, fit }, summary))
    }

    fn fit_options(&mut self, ui: &mut egui::Ui) {
        let f = &mut self.fit_settings;
        egui::Grid::new("sig_fit").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Peak shape:");
            shape_choice(ui, &mut f.shape);
            ui.end_row();
            ui.label("Baseline under the peaks:");
            term_choice(ui, &mut f.baseline);
            ui.end_row();
            ui.label("Number of peaks:");
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.count_auto, true, "The peaks found")
                    .on_hover_text(format!("Every peak found, at most {MAX_FIT_PEAKS}, in order of prominence"));
                ui.radio_value(&mut self.count_auto, false, "The most prominent");
                ui.add_enabled(!self.count_auto, egui::DragValue::new(&mut self.count).range(1..=MAX_FIT_PEAKS));
            });
            ui.end_row();
            ui.label("Or start at centres:");
            ui.add(egui::TextEdit::singleline(&mut self.centres).hint_text("e.g. 1200, 1240").desired_width(160.0))
                .on_hover_text("x positions separated by commas; used instead of the peaks found");
            ui.end_row();
        });
        ui.checkbox(&mut f.compare, "Compare fits with 1, 2, … peaks (AICc and F tests)");
        ui.checkbox(&mut f.common_width, "One width for every peak");
        ui.horizontal(|ui| {
            ui.label("Centres:");
            ui.radio_value(&mut self.spacing, SpacingMode::Free, "Free");
            ui.radio_value(&mut self.spacing, SpacingMode::Equal, "Equally spaced");
            ui.radio_value(&mut self.spacing, SpacingMode::Fixed, "Spaced by");
            ui.add_enabled(
                self.spacing == SpacingMode::Fixed,
                egui::DragValue::new(&mut self.spacing_value).speed(0.1),
            );
        });
    }
}

fn parse_centres(text: &str) -> Result<Vec<f64>, String> {
    text.split([',', ';', ' '])
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().parse::<f64>().map_err(|_| format!("{s:?} is not a number (centres)")))
        .collect()
}

fn shape_choice(ui: &mut egui::Ui, shape: &mut PeakShape) {
    egui::ComboBox::from_id_salt("sig_shape").selected_text(shape.label()).show_list(ui, |ui| {
        for s in PeakShape::ALL {
            ui.selectable_value(shape, s, s.label());
        }
    });
}

fn term_choice(ui: &mut egui::Ui, term: &mut BaselineTerm) {
    ui.horizontal(|ui| {
        for t in BaselineTerm::ALL {
            ui.radio_value(term, t, t.label());
        }
    });
}

impl Remember for SignalDialog {
    fn memory_key(&self) -> String {
        self.kind.title().to_owned()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], _table: &DataTable) {
        match columns {
            [y] => self.y = Some(*y),
            [x, y, ..] => (self.x, self.y) = (Some(*x), Some(*y)),
            [] => {}
        }
    }
}

/// The report's in-place settings (the report recomputes as they change).
pub fn report_settings(ui: &mut egui::Ui, request: &mut SignalRequest) {
    match request {
        SignalRequest::Smoothing(r) => match &mut r.method {
            Smoother::SavitzkyGolay { window, order, derivative } => {
                ui.horizontal(|ui| {
                    ui.label("Window:");
                    odd_drag(ui, window);
                    ui.label("Degree:");
                    ui.add(egui::DragValue::new(order).range(0..=(*window - 1).min(10)));
                    ui.label("Derivative:");
                    ui.add(egui::DragValue::new(derivative).range(0..=(*order).min(4)));
                });
            }
            Smoother::MovingAverage { window, kind } => {
                ui.horizontal(|ui| {
                    ui.label("Window:");
                    if *kind == MovingAverage::Centred {
                        odd_drag(ui, window);
                    } else {
                        ui.add(egui::DragValue::new(window).range(1..=1001));
                    }
                });
            }
            Smoother::RunningMedian { window } => {
                ui.horizontal(|ui| {
                    ui.label("Window:");
                    odd_drag(ui, window);
                });
            }
            Smoother::RunningPercentile { window, percentile } => {
                ui.horizontal(|ui| {
                    ui.label("Window:");
                    odd_drag(ui, window);
                    ui.label("Percentile:");
                    ui.add(egui::DragValue::new(percentile).range(0.0..=100.0));
                });
            }
            Smoother::Exponential { alpha } => {
                ui.horizontal(|ui| {
                    ui.label("Weight α:");
                    ui.add(egui::DragValue::new(alpha).range(0.01..=1.0).speed(0.01));
                });
            }
            Smoother::Gaussian { sigma } => {
                ui.horizontal(|ui| {
                    ui.label("Standard deviation (points):");
                    ui.add(egui::DragValue::new(sigma).range(0.1..=200.0).speed(0.1));
                });
            }
        },
        SignalRequest::Calculus(r) => {
            ui.horizontal(|ui| {
                ui.label("Integral:");
                ui.radio_value(&mut r.integral, Integration::Trapezoid, "Trapezoid rule");
                ui.radio_value(&mut r.integral, Integration::Simpson, "Simpson's rule");
            });
            if let DerivativeMethod::SavitzkyGolay { window, order } = &mut r.derivative {
                ui.horizontal(|ui| {
                    ui.label("Derivative window:");
                    odd_drag(ui, window);
                    ui.label("degree");
                    ui.add(egui::DragValue::new(order).range(1..=(*window - 1).min(10)));
                });
            }
            if let Some(s) = &mut r.resample {
                ui.horizontal(|ui| {
                    ui.label("Resampled points:");
                    ui.add(egui::DragValue::new(&mut s.points).range(2..=100_000));
                    egui::ComboBox::from_id_salt("sig_interp_r").selected_text(s.method.label()).show_list(ui, |ui| {
                        for m in Interpolation::ALL {
                            ui.selectable_value(&mut s.method, m, m.label());
                        }
                    });
                });
            }
        }
        SignalRequest::Spectrum(r) => {
            ui.horizontal(|ui| {
                ui.label("Peaks: height ≥");
                ui.add(egui::DragValue::new(&mut r.detection.min_height).range(0.0..=1000.0).speed(0.1));
                ui.label("σ̂, prominence ≥");
                ui.add(egui::DragValue::new(&mut r.detection.min_prominence).range(0.0..=1000.0).speed(0.1));
                ui.label("σ̂");
            });
            match &mut r.baseline {
                SpectrumBaseline::Estimated(BaselineMethod::Arpls { lambda } | BaselineMethod::Asls { lambda, .. }) => {
                    let mut e = lambda.log10();
                    ui.horizontal(|ui| {
                        ui.label("Baseline smoothness log₁₀ λ:");
                        if ui.add(egui::DragValue::new(&mut e).range(0.0..=12.0).speed(0.1)).changed() {
                            *lambda = 10f64.powf(e);
                        }
                    });
                }
                SpectrumBaseline::Estimated(BaselineMethod::Snip { half_window, .. }) => {
                    ui.horizontal(|ui| {
                        ui.label("SNIP half-window:");
                        ui.add(egui::DragValue::new(half_window).range(1..=1000));
                    });
                }
                _ => {}
            }
            if let Some(f) = &mut r.fit {
                ui.horizontal(|ui| {
                    ui.label("Peak shape:");
                    shape_choice(ui, &mut f.shape);
                });
                ui.horizontal(|ui| {
                    ui.label("Baseline under the peaks:");
                    term_choice(ui, &mut f.baseline);
                });
                ui.checkbox(&mut f.compare, "Compare fits with 1, 2, … peaks");
                ui.checkbox(&mut f.common_width, "One width for every peak");
            }
        }
    }
}

/// A drag value for an odd window of at least 3 points.
fn odd_drag(ui: &mut egui::Ui, window: &mut usize) {
    let r = ui.add(egui::DragValue::new(window).range(3..=1001));
    if r.changed() && *window % 2 == 0 {
        *window += 1;
    }
}
