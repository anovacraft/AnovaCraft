//! Dialog for comparing models through their saved prediction columns:
//! response, one column per model, the event class of a categorical
//! response and an optional column of validation roles.

use crate::dialog_kit::ComboExt as _;
use app_model::prediction::Response;
use app_model::{ModelComparisonRequest, SavedReport};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};

#[derive(Clone)]
pub struct ComparisonDialog {
    edit: Option<u64>,
    response: Option<usize>,
    chosen: Vec<bool>,
    event: Option<String>,
    validation: Option<usize>,
}

impl ComparisonDialog {
    pub fn new(table: &DataTable) -> Self {
        Self { edit: None, response: None, chosen: vec![false; table.n_cols()], event: None, validation: None }
    }

    /// The dialog filled from a saved model comparison.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::ModelComparison { request: r } = saved else { return None };
        Some(Self {
            edit: None,
            response: kit::valid(table, r.response),
            chosen: kit::mask(table.n_cols(), &r.models),
            event: r.event.clone(),
            validation: r.validation.and_then(|v| kit::valid(table, v)),
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<ModelComparisonRequest> {
        let dialog = Dialog::new("Compare models").editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    /// Pre-tick the columns a model report saved for `response` ("Predicted
    /// y", "Prob y = …").
    fn suggest_models(&mut self, table: &DataTable, response: usize) {
        let Some(name) = table.column(response).map(|c| c.name.clone()) else { return };
        self.chosen.resize(table.n_cols(), false);
        for (i, chosen) in self.chosen.iter_mut().enumerate() {
            let Some(c) = table.column(i) else { continue };
            *chosen = i != response
                && c.as_numeric().is_some()
                && (c.name.starts_with(&format!("Predicted {name}")) || c.name.starts_with(&format!("Prob {name} =")));
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<ModelComparisonRequest> {
        kit::note(
            ui,
            "Compares models on the same rows through the prediction columns they saved (a report's options \
             menu saves them): predicted values for a continuous response, or the probability of one event \
             class for a categorical one.",
        );
        let before = self.response;
        ui.horizontal(|ui| {
            ui.label("Response:");
            kit::column_choice(ui, "comparison_response", table, &mut self.response, |_| true, None);
        });
        if self.response != before {
            self.event = None;
            if let Some(r) = self.response {
                self.suggest_models(table, r);
            }
        }
        let all_rows: Vec<usize> = (0..table.n_rows()).collect();
        let classes = self
            .response
            .and_then(|r| Response::read(table, r, &all_rows).ok())
            .and_then(|r| r.classes().map(<[String]>::to_vec));
        if let Some(classes) = &classes {
            ui.horizontal(|ui| {
                ui.label("Event class:");
                let shown = self.event.clone().unwrap_or_else(|| format!("{} (last)", classes[classes.len() - 1]));
                egui::ComboBox::from_id_salt("comparison_event").selected_text(shown).show_list(ui, |ui| {
                    for class in classes {
                        ui.selectable_value(&mut self.event, Some(class.clone()), class);
                    }
                });
            })
            .response
            .on_hover_text("Each model column holds the probability of this class; the others count as non-events");
        }
        ui.label("Prediction columns (one per model):");
        let (response, validation) = (self.response, self.validation);
        let allow = |i: usize| Some(i) != response && Some(i) != validation && kit::is_numeric(table, i);
        kit::column_list(ui, "comparison_models", table, &mut self.chosen, allow);
        ui.horizontal(|ui| {
            ui.label("Validation roles:");
            kit::column_choice(
                ui,
                "comparison_validation",
                table,
                &mut self.validation,
                |i| Some(i) != response,
                Some("None (all rows)"),
            );
        })
        .response
        .on_hover_text("A column of 0/1/2 or Training/Validation/Test: measures are given for each role");
        let (response, validation) = (self.response, self.validation);
        let models: Vec<usize> = kit::chosen_columns(&self.chosen)
            .into_iter()
            .filter(|&i| {
                i < table.n_cols() && Some(i) != response && Some(i) != validation && kit::is_numeric(table, i)
            })
            .collect();
        let Some(response) = response else { return Err("Choose a response".into()) };
        if validation == Some(response) {
            return Err("The validation column cannot be the response".into());
        }
        if models.is_empty() {
            return Err("Choose at least one prediction column".into());
        }
        let summary =
            format!("Comparison of {} of {}", kit::count(models.len(), "model", "models"), kit::name(table, response));
        kit::ready(
            ModelComparisonRequest {
                response,
                models,
                event: if classes.is_some() { self.event.clone() } else { None },
                validation,
            },
            summary,
        )
    }
}

impl Remember for ComparisonDialog {
    fn memory_key(&self) -> String {
        "Compare models".into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        let numeric: Vec<usize> = columns.iter().copied().filter(|&c| kit::is_numeric(table, c)).collect();
        self.chosen = kit::mask(table.n_cols(), &numeric);
    }
}
