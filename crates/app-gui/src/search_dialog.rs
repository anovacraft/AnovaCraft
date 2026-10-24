//! Dialog for a model search: response, predictors, validation (which tunes
//! and ranks the models), and which kinds of model to try.

use app_model::{ModelKind, ModelSearchRequest, SavedReport};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::tree_dialog::{ValidationChoice, check_roles, heading, is_categorical, roles_ui, used_columns};

#[derive(Clone)]
pub struct SearchDialog {
    edit: Option<u64>,
    response: Option<usize>,
    chosen: Vec<bool>,
    validation: ValidationChoice,
    /// Whether each kind (in [`ModelKind::ALL`] order) is tried.
    models: [bool; 8],
    seed: u64,
    /// Repeats of k-fold cross-validation (1: off) and nested folds.
    repeats: usize,
    nested: bool,
}

impl SearchDialog {
    pub fn new(table: &DataTable) -> Self {
        Self {
            edit: None,
            response: None,
            chosen: vec![false; table.n_cols()],
            validation: ValidationChoice::for_search(),
            models: [true; 8],
            seed: 1,
            repeats: 1,
            nested: false,
        }
    }

    /// The dialog filled from a saved model search.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::ModelSearch { request: r } = saved else { return None };
        Some(Self {
            edit: None,
            response: kit::valid(table, r.response),
            chosen: kit::mask(table.n_cols(), &r.predictors),
            validation: ValidationChoice::for_search().with_method(&r.validation, table),
            models: ModelKind::ALL.map(|k| r.models.contains(&k)),
            seed: r.seed,
            repeats: r.repeats,
            nested: r.nested,
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<ModelSearchRequest> {
        let dialog = Dialog::new("Model search").editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<ModelSearchRequest> {
        kit::note(
            ui,
            "Fits every kind of predictive model that suits the response, each with its default settings and \
             the same validation rows, ranks them by their error on rows they were not fitted to, and opens the \
             best one's full report.",
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
        let categorical = self.response.map(|r| is_categorical(table, r));
        heading(ui, "Models to try");
        egui::Grid::new("search_models").num_columns(2).show(ui, |ui| {
            for (i, kind) in ModelKind::ALL.into_iter().enumerate() {
                let applies = categorical.is_none_or(|c| kind.applies_to(c));
                ui.add_enabled_ui(applies, |ui| ui.checkbox(&mut self.models[i], kind.label()))
                    .response
                    .on_disabled_hover_text(if kind == ModelKind::Mars {
                        "Adaptive regression splines model a continuous response"
                    } else {
                        "Discriminant analysis classifies a categorical response"
                    });
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
        self.validation.ui(ui, table, &used);
        kit::more_options(ui, |ui| {
            heading(ui, "Settings");
            ui.horizontal(|ui| {
                ui.label("Seed:");
                ui.add(crate::dialog_kit::seed_value(&mut self.seed));
            });
            self.validation.more_ui(ui, table, &used);
            heading(ui, "Repeated cross-validation");
            ui.horizontal(|ui| {
                let label = ui.label("Repeat k-fold cross-validation").on_hover_text(
                    "Each repeat uses fresh folds (the next seed); the report shows each model's error across the \
                     repeats with an interval and a forest plot. Needs k-fold roles, or every row training.",
                );
                ui.add(egui::DragValue::new(&mut self.repeats).range(1..=app_model::model_search::MAX_REPEATS))
                    .labelled_by(label.id);
                ui.label("times");
            });
            ui.checkbox(&mut self.nested, "Nested: tune each model within the training folds only").on_hover_text(
                "Each outer fold is held out while the model tunes itself by cross-validation on the other rows, so \
                 the error is not flattered by the tuning. Slower: every model is fitted once per outer fold.",
            );
        });
        let (response, validation) = check_roles(self.response, &predictors, &self.validation)?;
        let categorical = is_categorical(table, response);
        let models: Vec<ModelKind> =
            ModelKind::ALL.into_iter().zip(self.models).filter(|(_, on)| *on).map(|(k, _)| k).collect();
        let applicable = models.iter().filter(|k| k.applies_to(categorical)).count();
        if applicable == 0 {
            return Err("Choose at least one kind of model that suits the response".into());
        }
        let summary = format!(
            "Search of {} for {} on {}",
            kit::count(applicable, "model", "models"),
            kit::name(table, response),
            kit::count(predictors.len(), "predictor", "predictors")
        );
        let summary = match (self.repeats, self.nested) {
            (1, false) => summary,
            (r, nested) => {
                format!("{summary}, {}{}", if nested { "nested " } else { "" }, kit::count(r, "repeat", "repeats"))
            }
        };
        kit::ready(
            ModelSearchRequest {
                response,
                predictors,
                validation,
                models,
                seed: self.seed,
                repeats: self.repeats,
                nested: self.nested,
            },
            summary,
        )
    }
}

impl Remember for SearchDialog {
    fn memory_key(&self) -> String {
        "Model search".into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.chosen = kit::mask(table.n_cols(), columns);
    }
}
