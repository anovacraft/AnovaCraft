//! Dialog for a Scheffé mixture model.

use app_model::{MixtureRequest, SavedReport, ScheffeModel};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};

#[derive(Clone)]
pub struct MixtureDialog {
    edit: Option<u64>,
    response: Option<usize>,
    /// Indexed by column.
    components: Vec<bool>,
    model: ScheffeModel,
}

impl MixtureDialog {
    pub fn new(table: &DataTable) -> Self {
        Self { edit: None, response: None, components: vec![false; table.n_cols()], model: ScheffeModel::Quadratic }
    }

    /// The dialog filled from a saved mixture model.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::Mixture { request: r, .. } = saved else { return None };
        Some(Self {
            edit: None,
            response: kit::valid(table, r.response),
            components: kit::mask(table.n_cols(), &r.components),
            model: r.model,
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<MixtureRequest> {
        let dialog = Dialog::new("Mixture model").editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<MixtureRequest> {
        let numeric = |i: usize| kit::is_numeric(table, i);
        ui.horizontal(|ui| {
            ui.label("Response:");
            kit::column_choice(ui, "mixture_response", table, &mut self.response, numeric, None);
        });
        ui.label("Components (proportions, percentages or amounts per blend):");
        let response = self.response;
        kit::column_list(ui, "mixture_components", table, &mut self.components, |i| numeric(i) && Some(i) != response);
        ui.horizontal(|ui| {
            ui.label("Scheffé model:");
            for m in [ScheffeModel::Linear, ScheffeModel::Quadratic, ScheffeModel::SpecialCubic] {
                ui.radio_value(&mut self.model, m, m.label());
            }
        });
        kit::note(
            ui,
            "Each row is one blend; components are rescaled to sum to one. Three components also give a ternary \
             contour plot.",
        );
        let chosen: Vec<usize> = kit::chosen_columns(&self.components)
            .into_iter()
            .filter(|&i| i < table.n_cols() && numeric(i) && Some(i) != response)
            .collect();
        let Some(response) = response else { return Err("Choose a response".into()) };
        if chosen.len() < 2 {
            return Err("Choose at least two components".into());
        }
        let summary = format!(
            "{} mixture model of {} in {}",
            self.model.label(),
            kit::name(table, response),
            kit::count(chosen.len(), "component", "components")
        );
        kit::ready(MixtureRequest { response, components: chosen, model: self.model }, summary)
    }
}

impl Remember for MixtureDialog {
    fn memory_key(&self) -> String {
        "Mixture model".into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        let numeric: Vec<usize> = columns.iter().copied().filter(|&c| kit::is_numeric(table, c)).collect();
        self.components = kit::mask(table.n_cols(), &numeric);
    }
}
