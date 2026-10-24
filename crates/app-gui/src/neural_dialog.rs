//! Dialog for a neural network: response, predictors, hidden layers and
//! activation, validation (which tunes the penalty), and the solver
//! settings most people leave alone.

use crate::dialog_kit::ComboExt as _;
use app_model::{NeuralRequest, SavedReport};
use eframe::egui;
use stat_data::DataTable;
use stat_predictive::{Activation, Solver};

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::tree_dialog::{ValidationChoice, check_roles, heading, roles_ui, used_columns};

#[derive(Clone)]
pub struct NeuralDialog {
    edit: Option<u64>,
    response: Option<usize>,
    chosen: Vec<bool>,
    validation: ValidationChoice,
    layers: usize,
    first: usize,
    second: usize,
    activation: Activation,
    fix_penalty: bool,
    penalty: f64,
    solver: Option<Solver>,
    starts: usize,
    max_iter: usize,
    seed: u64,
}

fn activation_label(a: Activation) -> &'static str {
    match a {
        Activation::Tanh => "Hyperbolic tangent",
        Activation::Logistic => "Logistic",
        Activation::Relu => "Rectified linear",
        Activation::Identity => "Identity (linear)",
    }
}

impl NeuralDialog {
    pub fn new(table: &DataTable) -> Self {
        let d = NeuralRequest::new(0, Vec::new());
        Self {
            edit: None,
            response: None,
            chosen: vec![false; table.n_cols()],
            validation: ValidationChoice::for_neural(),
            layers: 1,
            first: d.hidden[0],
            second: 2,
            activation: d.activation,
            fix_penalty: false,
            penalty: 0.01,
            solver: d.solver,
            starts: d.starts,
            max_iter: d.max_iter,
            seed: d.seed,
        }
    }

    /// The dialog filled from a saved neural network.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::NeuralNetwork { request: r, .. } = saved else { return None };
        let fresh = Self::new(table);
        Some(Self {
            edit: None,
            response: kit::valid(table, r.response),
            chosen: kit::mask(table.n_cols(), &r.predictors),
            validation: ValidationChoice::for_neural().with_method(&r.validation, table),
            layers: r.hidden.len().clamp(1, 2),
            first: r.hidden.first().copied().unwrap_or(fresh.first),
            second: r.hidden.get(1).copied().unwrap_or(fresh.second),
            activation: r.activation,
            fix_penalty: r.penalty.is_some(),
            penalty: r.penalty.unwrap_or(fresh.penalty),
            solver: r.solver,
            starts: r.starts,
            max_iter: r.max_iter,
            seed: r.seed,
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<NeuralRequest> {
        let dialog = Dialog::new("Neural network").editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<NeuralRequest> {
        kit::note(
            ui,
            "A neural network passes the predictors through one or two layers of hidden nodes, each a smooth \
             function of a weighted sum of the layer before, so it can follow curved and interacting effects. \
             A penalty on the weights keeps it from chasing noise. Continuous predictors are standardized; \
             categorical ones become one indicator per level.",
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
        heading(ui, "Hidden layers");
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.layers, 1, "One layer");
            ui.radio_value(&mut self.layers, 2, "Two layers");
        });
        ui.horizontal(|ui| {
            ui.label("Nodes in layer 1:");
            ui.add(egui::DragValue::new(&mut self.first).range(1..=100));
            if self.layers == 2 {
                ui.label("Nodes in layer 2:");
                ui.add(egui::DragValue::new(&mut self.second).range(1..=100));
            }
        });
        ui.horizontal(|ui| {
            ui.label("Activation:");
            egui::ComboBox::from_id_salt("neural_activation")
                .selected_text(activation_label(self.activation))
                .show_list(ui, |ui| {
                    for a in Activation::ALL {
                        ui.selectable_value(&mut self.activation, a, activation_label(a)).on_hover_text(a.formula());
                    }
                });
        });
        self.validation.ui(ui, table, &used);
        kit::more_options(ui, |ui| {
            heading(ui, "Settings");
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.fix_penalty, "Fix the penalty α at")
                    .on_hover_text("Otherwise α is tuned over 0.0001, 0.001, 0.01, 0.1, 1 and 10");
                ui.add_enabled(self.fix_penalty, egui::DragValue::new(&mut self.penalty).speed(0.001).range(0.0..=1e4));
            });
            ui.horizontal(|ui| {
                ui.label("Solver:");
                ui.radio_value(&mut self.solver, None, "Automatic")
                    .on_hover_text("L-BFGS up to 2,000 training rows, Adam above");
                ui.radio_value(&mut self.solver, Some(Solver::Lbfgs), "L-BFGS")
                    .on_hover_text("A quasi-Newton method on all training rows: precise, for small and medium data");
                ui.radio_value(&mut self.solver, Some(Solver::Adam), "Adam")
                    .on_hover_text("Steps on small random batches of rows: for large data");
            });
            ui.horizontal(|ui| {
                ui.label("Random starts:");
                ui.add(egui::DragValue::new(&mut self.starts).range(1..=100))
                    .on_hover_text("The network is fitted from this many random starting weights; the best is kept");
                ui.label("Most iterations:");
                ui.add(egui::DragValue::new(&mut self.max_iter).range(10..=100_000));
            });
            ui.horizontal(|ui| {
                ui.label("Seed:");
                ui.add(crate::dialog_kit::seed_value(&mut self.seed));
            });
            self.validation.more_ui(ui, table, &used);
        });
        let (response, validation) = check_roles(self.response, &predictors, &self.validation)?;
        let hidden = if self.layers == 2 { vec![self.first, self.second] } else { vec![self.first] };
        let shape: Vec<String> = hidden.iter().map(ToString::to_string).collect();
        let summary = format!(
            "Neural network for {} on {}, hidden layers {} ({})",
            kit::name(table, response),
            kit::count(predictors.len(), "predictor", "predictors"),
            shape.join("–"),
            activation_label(self.activation).to_lowercase()
        );
        kit::ready(
            NeuralRequest {
                response,
                predictors,
                validation,
                hidden,
                activation: self.activation,
                penalty: self.fix_penalty.then_some(self.penalty),
                solver: self.solver,
                starts: self.starts,
                max_iter: self.max_iter,
                seed: self.seed,
            },
            summary,
        )
    }
}

impl Remember for NeuralDialog {
    fn memory_key(&self) -> String {
        "Neural network".into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.chosen = kit::mask(table.n_cols(), columns);
    }
}
