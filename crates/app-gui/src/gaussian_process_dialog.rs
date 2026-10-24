//! Dialog for a Gaussian-process model: response, predictors, covariance,
//! nugget, validation, and the likelihood search's starts and seed.

use crate::dialog_kit::ComboExt as _;
use app_model::{GaussianProcessRequest, SavedReport};
use eframe::egui;
use stat_data::DataTable;
use stat_predictive::CovarianceKernel;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::tree_dialog::{ValidationChoice, check_roles, heading, roles_ui, used_columns};

#[derive(Clone)]
pub struct GaussianProcessDialog {
    edit: Option<u64>,
    response: Option<usize>,
    chosen: Vec<bool>,
    validation: ValidationChoice,
    kernel: CovarianceKernel,
    fix_nugget: bool,
    nugget: f64,
    starts: usize,
    seed: u64,
}

fn kernel_hint(k: CovarianceKernel) -> &'static str {
    match k {
        CovarianceKernel::Matern52 => "Twice-differentiable surfaces; a good default that rarely overshoots",
        CovarianceKernel::SquaredExponential => "Very smooth surfaces; suits simulators with gentle responses",
    }
}

impl GaussianProcessDialog {
    pub fn new(table: &DataTable) -> Self {
        let d = GaussianProcessRequest::new(0, Vec::new());
        Self {
            edit: None,
            response: None,
            chosen: vec![false; table.n_cols()],
            validation: ValidationChoice::for_gaussian_process(),
            kernel: d.kernel,
            fix_nugget: false,
            nugget: 1e-6,
            starts: d.starts,
            seed: d.seed,
        }
    }

    /// The dialog filled from a saved Gaussian process.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::GaussianProcess { request: r, .. } = saved else { return None };
        let fresh = Self::new(table);
        Some(Self {
            edit: None,
            response: kit::valid(table, r.response),
            chosen: kit::mask(table.n_cols(), &r.predictors),
            validation: ValidationChoice::for_gaussian_process().with_method(&r.validation, table),
            kernel: r.kernel,
            fix_nugget: r.nugget.is_some(),
            nugget: r.nugget.unwrap_or(fresh.nugget),
            starts: r.starts,
            seed: r.seed,
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<GaussianProcessRequest> {
        let dialog = Dialog::new("Gaussian process").editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<GaussianProcessRequest> {
        kit::note(
            ui,
            "A Gaussian process fits a smooth surface through a continuous response and says how uncertain \
             it is everywhere: certain near the runs, less so between and beyond them. One length scale per \
             predictor says how quickly the response changes with it. It suits computer experiments and small \
             designed experiments.",
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
        heading(ui, "Covariance");
        ui.horizontal(|ui| {
            ui.label("Kernel:");
            egui::ComboBox::from_id_salt("gp_kernel").selected_text(self.kernel.label()).show_list(ui, |ui| {
                for k in CovarianceKernel::ALL {
                    ui.selectable_value(&mut self.kernel, k, k.label()).on_hover_text(kernel_hint(k));
                }
            });
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.fix_nugget, "Fix the nugget at").on_hover_text(
                "The noise variance as a fraction of the response's variance. A tiny value (10⁻⁶) makes the \
                     surface pass through every run, for a deterministic simulator; otherwise it is estimated",
            );
            ui.add_enabled(
                self.fix_nugget,
                egui::DragValue::new(&mut self.nugget).speed(1e-6).range(0.0..=10.0).max_decimals(8),
            );
        });
        self.validation.ui(ui, table, &used);
        kit::more_options(ui, |ui| {
            heading(ui, "Settings");
            ui.horizontal(|ui| {
                ui.label("Starts:");
                ui.add(egui::DragValue::new(&mut self.starts).range(1..=100))
                    .on_hover_text("The likelihood is maximized from this many starting points; the best is kept");
                ui.label("Seed:");
                ui.add(crate::dialog_kit::seed_value(&mut self.seed));
            });
            self.validation.more_ui(ui, table, &used);
        });
        let (response, validation) = check_roles(self.response, &predictors, &self.validation)?;
        if !kit::is_numeric(table, response) {
            return Err("Choose a numeric (continuous) response".into());
        }
        let summary = format!(
            "Gaussian process ({}) for {} on {}",
            self.kernel.label(),
            kit::name(table, response),
            kit::count(predictors.len(), "predictor", "predictors"),
        );
        kit::ready(
            GaussianProcessRequest {
                response,
                predictors,
                validation,
                kernel: self.kernel,
                nugget: self.fix_nugget.then_some(self.nugget),
                starts: self.starts,
                seed: self.seed,
            },
            summary,
        )
    }
}

impl Remember for GaussianProcessDialog {
    fn memory_key(&self) -> String {
        "Gaussian process".into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.chosen = kit::mask(table.n_cols(), columns);
    }
}
