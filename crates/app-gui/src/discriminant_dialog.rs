//! Dialog for discriminant analysis: the class column, numeric predictors,
//! the linear or quadratic rule, prior probabilities and validation roles.

use app_model::{DiscriminantRequest, DiscriminantRule, PriorChoice, SavedReport};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::tree_dialog::{ValidationChoice, check_roles, heading, is_categorical, roles_ui, used_columns};

#[derive(Clone)]
pub struct DiscriminantDialog {
    edit: Option<u64>,
    response: Option<usize>,
    chosen: Vec<bool>,
    validation: ValidationChoice,
    rule: DiscriminantRule,
    priors: PriorChoice,
}

impl DiscriminantDialog {
    pub fn new(table: &DataTable) -> Self {
        Self {
            edit: None,
            response: None,
            chosen: vec![false; table.n_cols()],
            validation: ValidationChoice::for_discriminant(),
            rule: DiscriminantRule::Linear,
            priors: PriorChoice::Proportional,
        }
    }

    /// The dialog filled from a saved discriminant analysis.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::DiscriminantAnalysis { request: r } = saved else { return None };
        Some(Self {
            edit: None,
            response: kit::valid(table, r.response),
            chosen: kit::mask(table.n_cols(), &r.predictors),
            validation: ValidationChoice::for_discriminant().with_method(&r.validation, table),
            rule: r.rule,
            priors: r.priors,
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<DiscriminantRequest> {
        let dialog = Dialog::new("Discriminant analysis").editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<DiscriminantRequest> {
        ui.set_max_width(kit::em(ui) * 32.0);
        kit::note(
            ui,
            "Classifies rows into the classes of a categorical response from numeric measurements, assuming \
             normal distributions within classes, and finds the canonical functions that best separate the \
             classes.",
        );
        let predictors =
            roles_ui(ui, table, &mut self.response, &mut self.chosen, "Predictors (numeric columns):", |i| {
                kit::is_numeric(table, i)
            });
        let used = used_columns(self.response, &predictors);
        heading(ui, "Rule");
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.rule, DiscriminantRule::Linear, "Linear")
                .on_hover_text("One covariance matrix pooled over the classes: straight boundaries");
            ui.radio_value(&mut self.rule, DiscriminantRule::Quadratic, "Quadratic")
                .on_hover_text("A covariance matrix per class: curved boundaries; needs more rows per class");
        });
        ui.horizontal(|ui| {
            ui.label("Prior probabilities:");
            ui.radio_value(&mut self.priors, PriorChoice::Proportional, "Proportional")
                .on_hover_text("In proportion to the training rows of each class");
            ui.radio_value(&mut self.priors, PriorChoice::Equal, "Equal");
        });
        self.validation.ui(ui, table, &used);
        kit::more_options(ui, |ui| self.validation.more_ui(ui, table, &used));
        let (response, validation) = check_roles(self.response, &predictors, &self.validation)?;
        if !is_categorical(table, response) {
            return Err("The response must be categorical (text, or numbers marked nominal or ordinal)".into());
        }
        let rule = match self.rule {
            DiscriminantRule::Linear => "Linear",
            DiscriminantRule::Quadratic => "Quadratic",
        };
        let summary = format!(
            "{rule} discriminant analysis of {} on {}",
            kit::name(table, response),
            kit::count(predictors.len(), "predictor", "predictors")
        );
        kit::ready(
            DiscriminantRequest { response, predictors, rule: self.rule, priors: self.priors, validation },
            summary,
        )
    }
}

impl Remember for DiscriminantDialog {
    fn memory_key(&self) -> String {
        "Discriminant analysis".into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        let numeric: Vec<usize> = columns.iter().copied().filter(|&c| kit::is_numeric(table, c)).collect();
        self.chosen = kit::mask(table.n_cols(), &numeric);
    }
}
