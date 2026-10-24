//! Dialog for functional data analysis: the curve, time and value columns
//! of long data, curve-level factors for the score model, the basis and
//! smoothing, registration (landmarks and continuous alignment) and the
//! number of principal components.

use crate::dialog_kit::ComboExt as _;
use app_model::{FunctionalBasis, FunctionalLandmark, FunctionalRequest, FunctionalSmoothing, SavedReport};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::tree_dialog::heading;

#[derive(Clone, Copy, PartialEq, Eq)]
enum BasisKind {
    BSpline,
    Fourier,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SmoothKind {
    Gcv,
    Fixed,
    None,
}

#[derive(Clone, PartialEq)]
struct LandmarkRow {
    maximum: bool,
    whole_range: bool,
    from: f64,
    to: f64,
}

#[derive(Clone)]
pub struct FunctionalDialog {
    edit: Option<u64>,
    curve: Option<usize>,
    time: Option<usize>,
    value: Option<usize>,
    factors: Vec<bool>,
    basis: BasisKind,
    auto_functions: bool,
    functions: usize,
    order: usize,
    auto_period: bool,
    period: f64,
    smoothing: SmoothKind,
    log_lambda: f64,
    landmarks: Vec<LandmarkRow>,
    continuous: bool,
    warp_functions: usize,
    warp_penalty: f64,
    auto_components: bool,
    components: usize,
    interactions: bool,
    confidence: f64,
}

impl FunctionalDialog {
    pub fn new(table: &DataTable) -> Self {
        let d = FunctionalRequest::new(0, 0, 0);
        Self {
            edit: None,
            curve: None,
            time: None,
            value: None,
            factors: vec![false; table.n_cols()],
            basis: BasisKind::BSpline,
            auto_functions: true,
            functions: 12,
            order: 4,
            auto_period: true,
            period: 1.0,
            smoothing: SmoothKind::Gcv,
            log_lambda: 0.0,
            landmarks: Vec::new(),
            continuous: false,
            warp_functions: d.warp_functions,
            warp_penalty: d.warp_penalty,
            auto_components: true,
            components: 2,
            interactions: false,
            confidence: d.confidence,
        }
    }

    /// The dialog filled from a saved functional data report.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::FunctionalData { request: r, .. } = saved else { return None };
        let mut d = Self::new(table);
        d.curve = kit::valid(table, r.curve);
        d.time = kit::valid(table, r.time);
        d.value = kit::valid(table, r.value);
        d.factors = kit::mask(table.n_cols(), &r.factors);
        match r.basis {
            FunctionalBasis::BSpline { functions, order } => {
                d.basis = BasisKind::BSpline;
                d.order = order;
                d.auto_functions = functions.is_none();
                d.functions = functions.unwrap_or(d.functions);
            }
            FunctionalBasis::Fourier { functions, period } => {
                d.basis = BasisKind::Fourier;
                d.auto_functions = functions.is_none();
                d.functions = functions.unwrap_or(d.functions);
                d.auto_period = period.is_none();
                d.period = period.unwrap_or(d.period);
            }
        }
        match r.smoothing {
            FunctionalSmoothing::Gcv => d.smoothing = SmoothKind::Gcv,
            FunctionalSmoothing::None => d.smoothing = SmoothKind::None,
            FunctionalSmoothing::Fixed(l) => {
                d.smoothing = SmoothKind::Fixed;
                d.log_lambda = if l > 0.0 { l.log10() } else { -12.0 };
            }
        }
        d.landmarks = r
            .landmarks
            .iter()
            .map(|l| LandmarkRow {
                maximum: l.maximum,
                whole_range: l.window.is_none(),
                from: l.window.map_or(0.0, |w| w.0),
                to: l.window.map_or(1.0, |w| w.1),
            })
            .collect();
        d.continuous = r.continuous;
        d.warp_functions = r.warp_functions;
        d.warp_penalty = r.warp_penalty;
        d.auto_components = r.components.is_none();
        d.components = r.components.unwrap_or(d.components);
        d.interactions = r.interactions;
        d.confidence = r.confidence;
        Some(d)
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<FunctionalRequest> {
        let dialog = Dialog::new("Functional data").editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    /// The time range of the chosen time column, for landmark windows.
    fn time_range(&self, table: &DataTable) -> Option<(f64, f64)> {
        let v = table.column(self.time?)?.as_numeric()?;
        let lo = v.iter().copied().filter(|x| x.is_finite()).fold(f64::INFINITY, f64::min);
        let hi = v.iter().copied().filter(|x| x.is_finite()).fold(f64::NEG_INFINITY, f64::max);
        (lo < hi).then_some((lo, hi))
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<FunctionalRequest> {
        kit::note(
            ui,
            "Treats each curve, the rows sharing a curve identifier, as one observation: smooths it in a basis, \
             optionally aligns the curves in time, and finds the main ways the curves differ (functional \
             principal components). Curve-level factors model the component scores, and the profiler then \
             shows the predicted curve as the factors change.",
        );
        egui::Grid::new("functional_roles").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            ui.label("Curve identifier:").on_hover_text("Rows with the same value form one curve");
            kit::column_choice(ui, "curve", table, &mut self.curve, |_| true, None);
            ui.end_row();
            ui.label("Time:").on_hover_text("Where along the curve each value was measured (numeric)");
            kit::column_choice(ui, "time", table, &mut self.time, |i| kit::is_numeric(table, i), None);
            ui.end_row();
            ui.label("Value:").on_hover_text("The measured value (numeric)");
            kit::column_choice(ui, "value", table, &mut self.value, |i| kit::is_numeric(table, i), None);
            ui.end_row();
        });
        let roles = [self.curve, self.time, self.value];
        ui.label("Factors for the scores (optional, constant within each curve):");
        kit::column_list(ui, "functional_factors", table, &mut self.factors, |i| !roles.contains(&Some(i)));

        heading(ui, "Basis and smoothing");
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.basis, BasisKind::BSpline, "B-splines")
                .on_hover_text("Piecewise polynomials joined smoothly at equally spaced knots");
            ui.radio_value(&mut self.basis, BasisKind::Fourier, "Fourier series")
                .on_hover_text("Sines and cosines: for curves that repeat with a period, such as a year");
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.auto_functions, "Automatic number of basis functions").on_hover_text(
                "With smoothing: about one per distinct time (at most 40 B-splines or 25 Fourier functions); \
                 without: at most the fewest readings of any curve",
            );
            ui.add_enabled(!self.auto_functions, egui::DragValue::new(&mut self.functions).range(1..=200));
        });
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.smoothing, SmoothKind::Gcv, "Smooth by GCV").on_hover_text(
                "Penalize roughness, with the amount chosen by generalized cross-validation (one amount for \
                 all curves)",
            );
            ui.radio_value(&mut self.smoothing, SmoothKind::Fixed, "Fixed smoothing");
            ui.radio_value(&mut self.smoothing, SmoothKind::None, "No smoothing")
                .on_hover_text("Least squares in the basis: the number of basis functions sets the smoothness");
        });
        if self.smoothing == SmoothKind::Fixed {
            ui.horizontal(|ui| {
                ui.label("log₁₀ λ:");
                ui.add(egui::DragValue::new(&mut self.log_lambda).speed(0.05).range(-12.0..=12.0));
            });
        }

        heading(ui, "Registration (align the curves in time)");
        let range = self.time_range(table);
        let mut remove = None;
        for (k, l) in self.landmarks.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("Landmark {}:", k + 1));
                egui::ComboBox::from_id_salt(("landmark_kind", k))
                    .selected_text(if l.maximum { "Maximum" } else { "Minimum" })
                    .show_list(ui, |ui| {
                        ui.selectable_value(&mut l.maximum, true, "Maximum");
                        ui.selectable_value(&mut l.maximum, false, "Minimum");
                    });
                ui.checkbox(&mut l.whole_range, "anywhere");
                if !l.whole_range {
                    ui.label("between");
                    ui.add(egui::DragValue::new(&mut l.from).speed(0.1));
                    ui.label("and");
                    ui.add(egui::DragValue::new(&mut l.to).speed(0.1));
                }
                if ui.small_button("Remove").clicked() {
                    remove = Some(k);
                }
            });
        }
        if let Some(k) = remove {
            self.landmarks.remove(k);
        }
        if ui
            .button("Add landmark")
            .on_hover_text("Line the curves up on the time of each curve's peak or trough in a window")
            .clicked()
        {
            let (a, b) = range.unwrap_or((0.0, 1.0));
            self.landmarks.push(LandmarkRow { maximum: true, whole_range: true, from: a, to: b });
        }
        ui.checkbox(&mut self.continuous, "Continuous registration to the mean curve").on_hover_text(
            "After any landmarks, warp each curve smoothly so its shape best matches the mean curve (minimum \
             eigenvalue criterion of Ramsay & Li)",
        );

        heading(ui, "Principal components and score model");
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.auto_components, "Automatic number of components")
                .on_hover_text("Enough to explain 90% of the variation, at most six");
            ui.add_enabled(!self.auto_components, egui::DragValue::new(&mut self.components).range(1..=50));
        });
        ui.add_enabled(
            self.factors.iter().filter(|f| **f).count() >= 2,
            egui::Checkbox::new(&mut self.interactions, "Two-factor interactions in the score model"),
        );
        kit::more_options(ui, |ui| {
            if self.basis == BasisKind::BSpline {
                ui.horizontal(|ui| {
                    ui.label("B-spline order:").on_hover_text("4 is cubic; the penalty is on the second derivative");
                    ui.add(egui::DragValue::new(&mut self.order).range(2..=6));
                });
            } else {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.auto_period, "Period is the length of the time range");
                    ui.add_enabled(!self.auto_period, egui::DragValue::new(&mut self.period).speed(0.1));
                });
            }
            ui.horizontal(|ui| {
                ui.label("Warping basis functions:");
                ui.add(egui::DragValue::new(&mut self.warp_functions).range(2..=20));
                ui.label("Warping roughness penalty:");
                ui.add(egui::DragValue::new(&mut self.warp_penalty).speed(0.0001).range(0.0..=10.0));
            });
            ui.horizontal(|ui| {
                ui.label("Confidence level of the predicted curve:");
                ui.add(egui::DragValue::new(&mut self.confidence).speed(0.005).range(0.5..=0.999));
            });
        });

        let (Some(curve), Some(time), Some(value)) = (self.curve, self.time, self.value) else {
            return Err("Choose the curve identifier, time and value columns".into());
        };
        if !kit::all_different(&roles) {
            return Err("The curve, time and value columns must be different".into());
        }
        let factors: Vec<usize> =
            kit::chosen_columns(&self.factors).into_iter().filter(|i| !roles.contains(&Some(*i))).collect();
        let functions = (!self.auto_functions).then_some(self.functions);
        let basis = match self.basis {
            BasisKind::BSpline => FunctionalBasis::BSpline { functions, order: self.order },
            BasisKind::Fourier => {
                FunctionalBasis::Fourier { functions, period: (!self.auto_period).then_some(self.period) }
            }
        };
        let smoothing = match self.smoothing {
            SmoothKind::Gcv => FunctionalSmoothing::Gcv,
            SmoothKind::Fixed => FunctionalSmoothing::Fixed(10f64.powf(self.log_lambda)),
            SmoothKind::None => FunctionalSmoothing::None,
        };
        for l in &self.landmarks {
            if !l.whole_range && l.to <= l.from {
                return Err("Each landmark window must end after it starts".into());
            }
        }
        let landmarks = self
            .landmarks
            .iter()
            .map(|l| FunctionalLandmark { maximum: l.maximum, window: (!l.whole_range).then_some((l.from, l.to)) })
            .collect::<Vec<_>>();
        let mut summary = format!(
            "Curves of {} by {} for each {}",
            kit::name(table, value),
            kit::name(table, time),
            kit::name(table, curve)
        );
        if !landmarks.is_empty() || self.continuous {
            summary.push_str(", registered");
        }
        if !factors.is_empty() {
            summary.push_str(&format!(", scores modelled on {}", kit::count(factors.len(), "factor", "factors")));
        }
        kit::ready(
            FunctionalRequest {
                factors,
                basis,
                smoothing,
                landmarks,
                continuous: self.continuous,
                warp_functions: self.warp_functions,
                warp_penalty: self.warp_penalty,
                components: (!self.auto_components).then_some(self.components),
                interactions: self.interactions,
                confidence: self.confidence,
                ..FunctionalRequest::new(curve, time, value)
            },
            summary,
        )
    }
}

impl Remember for FunctionalDialog {
    fn memory_key(&self) -> String {
        "Functional data".into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        // Selected columns in the grid fill the roles in order.
        let mut it = columns.iter().copied().filter(|&c| c < table.n_cols());
        self.curve = it.next();
        self.time = it.next().filter(|&c| kit::is_numeric(table, c));
        self.value = it.next().filter(|&c| kit::is_numeric(table, c));
    }
}
