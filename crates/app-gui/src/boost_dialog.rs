//! Dialog for gradient-boosted trees: response, predictors, validation (which
//! chooses the number of rounds), and the boosting settings.

use app_model::{BoostRequest, RegressionLoss, SavedReport};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::tree_dialog::{ValidationChoice, check_roles, heading, is_categorical, roles_ui, used_columns};

#[derive(Clone)]
pub struct BoostDialog {
    edit: Option<u64>,
    response: Option<usize>,
    chosen: Vec<bool>,
    validation: ValidationChoice,
    rounds: usize,
    learning_rate: f64,
    max_depth: usize,
    min_leaf: usize,
    lambda: f64,
    subsample: f64,
    early_stopping: usize,
    regression_loss: RegressionLoss,
    seed: u64,
}

impl BoostDialog {
    pub fn new(table: &DataTable) -> Self {
        let d = BoostRequest::new(0, Vec::new());
        Self {
            edit: None,
            response: None,
            chosen: vec![false; table.n_cols()],
            validation: ValidationChoice::for_boosting(),
            rounds: d.rounds,
            learning_rate: d.learning_rate,
            max_depth: d.max_depth,
            min_leaf: d.min_leaf,
            lambda: d.lambda,
            subsample: d.subsample,
            early_stopping: d.early_stopping,
            regression_loss: d.regression_loss,
            seed: d.seed,
        }
    }

    /// The dialog filled from saved boosted trees.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::BoostedTrees { request: r, .. } = saved else { return None };
        Some(Self {
            edit: None,
            response: kit::valid(table, r.response),
            chosen: kit::mask(table.n_cols(), &r.predictors),
            validation: ValidationChoice::for_boosting().with_method(&r.validation, table),
            rounds: r.rounds,
            learning_rate: r.learning_rate,
            max_depth: r.max_depth,
            min_leaf: r.min_leaf,
            lambda: r.lambda,
            subsample: r.subsample,
            early_stopping: r.early_stopping,
            regression_loss: r.regression_loss,
            seed: r.seed,
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<BoostRequest> {
        let dialog = Dialog::new("Boosted trees").editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<BoostRequest> {
        kit::note(
            ui,
            "Adds small trees one after another, each fitted to what the model so far gets wrong and shrunk \
             by the learning rate. Validation rows choose how many rounds to keep.",
        );
        let predictors = roles_ui(
            ui,
            table,
            &mut self.response,
            &mut self.chosen,
            "Predictors (missing values go the way each split learned):",
            |_| true,
        );
        let used = used_columns(self.response, &predictors);
        self.validation.ui(ui, table, &used);
        let continuous = self.response.is_some_and(|r| !is_categorical(table, r));
        kit::more_options(ui, |ui| {
            heading(ui, "Boosting");
            ui.horizontal(|ui| {
                ui.label("Rounds up to");
                ui.add(egui::DragValue::new(&mut self.rounds).range(1..=10_000));
                ui.label("; learning rate");
                ui.add(egui::DragValue::new(&mut self.learning_rate).speed(0.005).range(0.001..=1.0));
            });
            ui.horizontal(|ui| {
                ui.label("Stop after");
                ui.add(egui::DragValue::new(&mut self.early_stopping).range(0..=1_000));
                ui.label("rounds without improvement on validation rows (0: never)");
            });
            ui.horizontal(|ui| {
                ui.label("Tree depth up to");
                ui.add(egui::DragValue::new(&mut self.max_depth).range(1..=12));
                ui.label("; leaves of at least");
                ui.add(egui::DragValue::new(&mut self.min_leaf).range(1..=5_000));
                ui.label("rows");
            });
            ui.horizontal(|ui| {
                ui.label("L2 penalty on leaf values:");
                ui.add(egui::DragValue::new(&mut self.lambda).speed(0.05).range(0.0..=1_000.0));
                ui.label("Rows per round (share):");
                ui.add(egui::DragValue::new(&mut self.subsample).speed(0.01).range(0.05..=1.0));
            });
            ui.add_enabled_ui(continuous, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Loss:");
                    ui.radio_value(&mut self.regression_loss, RegressionLoss::Squared, "Squared error");
                    ui.radio_value(&mut self.regression_loss, RegressionLoss::Absolute, "Absolute error (robust)");
                });
            })
            .response
            .on_disabled_hover_text("A categorical response uses the logistic (two classes) or multinomial loss");
            ui.horizontal(|ui| {
                ui.label("Seed:");
                ui.add(crate::dialog_kit::seed_value(&mut self.seed));
            });
            self.validation.more_ui(ui, table, &used);
        });
        let (response, validation) = check_roles(self.response, &predictors, &self.validation)?;
        let summary = format!(
            "Boosted trees for {} on {}, up to {} rounds",
            kit::name(table, response),
            kit::count(predictors.len(), "predictor", "predictors"),
            self.rounds
        );
        kit::ready(
            BoostRequest {
                response,
                predictors,
                validation,
                rounds: self.rounds,
                learning_rate: self.learning_rate,
                max_depth: self.max_depth,
                min_leaf: self.min_leaf,
                lambda: self.lambda,
                subsample: self.subsample,
                early_stopping: self.early_stopping,
                regression_loss: self.regression_loss,
                seed: self.seed,
            },
            summary,
        )
    }
}

impl Remember for BoostDialog {
    fn memory_key(&self) -> String {
        "Boosted trees".into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.chosen = kit::mask(table.n_cols(), columns);
    }
}
