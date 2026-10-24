//! Dialog for partial least squares: response, numeric predictors,
//! validation (which chooses the number of factors), and the factor settings.

use app_model::{PlsAlgorithm, PlsRequest, SavedReport};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::tree_dialog::{ValidationChoice, check_roles, heading, roles_ui, used_columns};

#[derive(Clone)]
pub struct PlsDialog {
    edit: Option<u64>,
    response: Option<usize>,
    chosen: Vec<bool>,
    validation: ValidationChoice,
    max_factors: usize,
    fixed: bool,
    factors: usize,
    algorithm: PlsAlgorithm,
    scale: bool,
}

impl PlsDialog {
    pub fn new(table: &DataTable) -> Self {
        let d = PlsRequest::new(0, Vec::new());
        Self {
            edit: None,
            response: None,
            chosen: vec![false; table.n_cols()],
            validation: ValidationChoice::for_pls(),
            max_factors: d.max_factors,
            fixed: false,
            factors: 2,
            algorithm: d.algorithm,
            scale: d.scale,
        }
    }

    /// The dialog filled from a saved partial least squares fit.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::PartialLeastSquares { request: r, .. } = saved else { return None };
        let fresh = Self::new(table);
        Some(Self {
            edit: None,
            response: kit::valid(table, r.response),
            chosen: kit::mask(table.n_cols(), &r.predictors),
            validation: ValidationChoice::for_pls().with_method(&r.validation, table),
            max_factors: if r.factors.is_some() { fresh.max_factors } else { r.max_factors },
            fixed: r.factors.is_some(),
            factors: r.factors.unwrap_or(fresh.factors),
            algorithm: r.algorithm,
            scale: r.scale,
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<PlsRequest> {
        let dialog = Dialog::new("Partial least squares").editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<PlsRequest> {
        kit::note(
            ui,
            "Finds a few factors (weighted sums of the predictors) that explain the predictors and the response \
             together, then regresses the response on them. Suited to many correlated predictors, even more \
             than rows. A categorical response is fitted as one indicator per class (PLS discriminant analysis).",
        );
        let predictors =
            roles_ui(ui, table, &mut self.response, &mut self.chosen, "Predictors (numeric columns):", |i| {
                kit::is_numeric(table, i)
            });
        let used = used_columns(self.response, &predictors);
        heading(ui, "Factors");
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.fixed, false, "Choose the number, up to");
            ui.add_enabled(!self.fixed, egui::DragValue::new(&mut self.max_factors).range(1..=100));
        })
        .response
        .on_hover_text("The fewest factors whose held-out error is within one standard error of the best");
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.fixed, true, "Use exactly");
            ui.add_enabled(self.fixed, egui::DragValue::new(&mut self.factors).range(1..=100));
            ui.label("factors");
        });
        self.validation.ui(ui, table, &used);
        kit::more_options(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Method:");
                ui.radio_value(&mut self.algorithm, PlsAlgorithm::Simpls, "SIMPLS")
                    .on_hover_text("de Jong (1993): deflates the cross-product matrix; fast");
                ui.radio_value(&mut self.algorithm, PlsAlgorithm::Nipals, "NIPALS")
                    .on_hover_text("Wold's iterative algorithm, deflating the predictors after each factor");
            });
            ui.checkbox(&mut self.scale, "Standardize predictors to unit variance")
                .on_hover_text("Without it, predictors measured on larger scales dominate the factors");
            self.validation.more_ui(ui, table, &used);
        });
        let (response, validation) = check_roles(self.response, &predictors, &self.validation)?;
        let factors = if self.fixed {
            kit::count(self.factors.max(1), "factor", "factors")
        } else {
            format!("up to {} factors", self.max_factors)
        };
        let summary = format!(
            "Partial least squares of {} on {}, {factors}",
            kit::name(table, response),
            kit::count(predictors.len(), "predictor", "predictors")
        );
        kit::ready(
            PlsRequest {
                response,
                predictors,
                validation,
                max_factors: if self.fixed { self.factors.max(1) } else { self.max_factors },
                factors: self.fixed.then_some(self.factors.max(1)),
                algorithm: self.algorithm,
                scale: self.scale,
            },
            summary,
        )
    }
}

impl Remember for PlsDialog {
    fn memory_key(&self) -> String {
        "Partial least squares".into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        let numeric: Vec<usize> = columns.iter().copied().filter(|&c| kit::is_numeric(table, c)).collect();
        self.chosen = kit::mask(table.n_cols(), &numeric);
    }
}
