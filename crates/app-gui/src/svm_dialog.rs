//! Dialog for a support vector machine: response, predictors, validation
//! (which tunes the cost and γ), kernel, and fixed or tuned settings.

use app_model::{SavedReport, SvmKernel, SvmRequest};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::tree_dialog::{ValidationChoice, check_roles, heading, is_categorical, roles_ui, used_columns};

/// Which kernel family the dialog shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KernelChoice {
    Linear,
    Rbf,
    Polynomial,
}

#[derive(Clone)]
pub struct SvmDialog {
    edit: Option<u64>,
    response: Option<usize>,
    chosen: Vec<bool>,
    validation: ValidationChoice,
    kernel: KernelChoice,
    degree: u32,
    fix_cost: bool,
    cost: f64,
    fix_gamma: bool,
    gamma: f64,
    epsilon: f64,
    seed: u64,
}

impl SvmDialog {
    pub fn new(table: &DataTable) -> Self {
        let d = SvmRequest::new(0, Vec::new());
        Self {
            edit: None,
            response: None,
            chosen: vec![false; table.n_cols()],
            validation: ValidationChoice::for_svm(),
            kernel: KernelChoice::Rbf,
            degree: 3,
            fix_cost: false,
            cost: 1.0,
            fix_gamma: false,
            gamma: 0.1,
            epsilon: d.epsilon,
            seed: d.seed,
        }
    }

    /// The dialog filled from a saved support vector machine.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::SupportVectorMachine { request: r, .. } = saved else { return None };
        let fresh = Self::new(table);
        let (kernel, degree) = match r.kernel {
            SvmKernel::Linear => (KernelChoice::Linear, fresh.degree),
            SvmKernel::Rbf => (KernelChoice::Rbf, fresh.degree),
            SvmKernel::Polynomial { degree } => (KernelChoice::Polynomial, degree),
        };
        Some(Self {
            edit: None,
            response: kit::valid(table, r.response),
            chosen: kit::mask(table.n_cols(), &r.predictors),
            validation: ValidationChoice::for_svm().with_method(&r.validation, table),
            kernel,
            degree,
            fix_cost: r.cost.is_some(),
            cost: r.cost.unwrap_or(fresh.cost),
            fix_gamma: r.gamma.is_some(),
            gamma: r.gamma.unwrap_or(fresh.gamma),
            epsilon: r.epsilon,
            seed: r.seed,
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<SvmRequest> {
        let dialog = Dialog::new("Support vector machine").editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SvmRequest> {
        kit::note(
            ui,
            "Separates the classes with the widest possible margin (a categorical response), or fits a curve \
             that ignores errors smaller than ε (a continuous one). A kernel lets the boundary bend. \
             Continuous predictors are standardized; categorical ones become one indicator per level.",
        );
        let predictors = roles_ui(
            ui,
            table,
            &mut self.response,
            &mut self.chosen,
            "Predictors (rows missing a predictor are left out):",
            |_| true,
        );
        let used = used_columns(self.response, &predictors);
        heading(ui, "Kernel");
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.kernel, KernelChoice::Rbf, "Radial basis")
                .on_hover_text("exp(−γ‖u − v‖²): smooth, flexible boundaries");
            ui.radio_value(&mut self.kernel, KernelChoice::Polynomial, "Polynomial").on_hover_text("(γuᵀv + 1)ᵈ");
            ui.radio_value(&mut self.kernel, KernelChoice::Linear, "Linear")
                .on_hover_text("uᵀv: a flat boundary, with weights shown in the report");
        });
        if self.kernel == KernelChoice::Polynomial {
            ui.horizontal(|ui| {
                ui.label("Degree:");
                ui.add(egui::DragValue::new(&mut self.degree).range(2..=5));
            });
        }
        self.validation.ui(ui, table, &used);
        let continuous = self.response.is_some_and(|r| !is_categorical(table, r));
        kit::more_options(ui, |ui| {
            heading(ui, "Settings");
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.fix_cost, "Fix the cost C at")
                    .on_hover_text("Otherwise C is tuned over 0.25, 1, 4, 16 and 64");
                ui.add_enabled(self.fix_cost, egui::DragValue::new(&mut self.cost).speed(0.1).range(0.001..=1e6));
            });
            ui.add_enabled_ui(self.kernel != KernelChoice::Linear, |ui| {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.fix_gamma, "Fix γ at")
                        .on_hover_text("Otherwise γ is tuned over g/16, g/4, g and 4g, with g = 1 / number of inputs");
                    ui.add_enabled(self.fix_gamma, egui::DragValue::new(&mut self.gamma).speed(0.01).range(1e-6..=1e3));
                });
            });
            ui.add_enabled_ui(continuous, |ui| {
                ui.horizontal(|ui| {
                    ui.label("ε (response standard deviations):");
                    ui.add(egui::DragValue::new(&mut self.epsilon).speed(0.01).range(0.0..=10.0));
                });
            })
            .response
            .on_disabled_hover_text("ε applies to a continuous response");
            ui.horizontal(|ui| {
                ui.label("Seed:");
                ui.add(crate::dialog_kit::seed_value(&mut self.seed));
            });
            self.validation.more_ui(ui, table, &used);
        });
        let (response, validation) = check_roles(self.response, &predictors, &self.validation)?;
        let kernel = match self.kernel {
            KernelChoice::Linear => "linear kernel".to_owned(),
            KernelChoice::Rbf => "radial basis kernel".to_owned(),
            KernelChoice::Polynomial => format!("polynomial kernel of degree {}", self.degree),
        };
        let summary = format!(
            "Support vector machine for {} on {}, {kernel}",
            kit::name(table, response),
            kit::count(predictors.len(), "predictor", "predictors")
        );
        kit::ready(
            SvmRequest {
                response,
                predictors,
                validation,
                kernel: match self.kernel {
                    KernelChoice::Linear => SvmKernel::Linear,
                    KernelChoice::Rbf => SvmKernel::Rbf,
                    KernelChoice::Polynomial => SvmKernel::Polynomial { degree: self.degree },
                },
                cost: self.fix_cost.then_some(self.cost),
                gamma: (self.fix_gamma && self.kernel != KernelChoice::Linear).then_some(self.gamma),
                epsilon: self.epsilon,
                seed: self.seed,
            },
            summary,
        )
    }
}

impl Remember for SvmDialog {
    fn memory_key(&self) -> String {
        "Support vector machine".into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.chosen = kit::mask(table.n_cols(), columns);
    }
}
