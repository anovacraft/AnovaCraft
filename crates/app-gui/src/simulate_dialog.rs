//! Dialog for simulating responses for the design in the active table:
//! choose the factor columns, a model and the prior on its coefficients
//! (effect sparsity, hierarchy and heredity); Add puts the simulated
//! response columns in the table and the true coefficients in a new table
//! of the same project ([`app_model::design_simulation`]).

use app_model::DeclaredFactor;
use app_model::design_simulation::{SimulateOutcome, SimulateRequest, simulate_design_responses};
use eframe::egui::{self, RichText};
use stat_data::DataTable;
use stat_doe::ModelFormula;
use stat_doe::simulate::MAX_SIMULATIONS;

use crate::design_dialog::sentence_fragment;
use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};

/// The dialog's title.
pub const TITLE: &str = "Simulate responses";

/// How the activity of a higher-order term depends on its parents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Heredity {
    /// Only when every parent is active.
    Strong,
    /// More likely with active parents (the default probabilities).
    Weak,
    /// As likely as a main effect, whatever the parents.
    None,
}

impl Heredity {
    /// The probabilities (all parents, some parents, no parents) for main
    /// effects active with probability `main`.
    fn probabilities(self, main: f64) -> (f64, f64, f64) {
        match self {
            Self::Strong => (0.25, 0.0, 0.0),
            Self::Weak => (0.25, 0.10, 0.01),
            Self::None => (main, main, main),
        }
    }
}

/// A preview: the notes and the active terms of each simulated response.
type Preview = Result<(Vec<String>, Vec<String>), String>;

#[derive(Clone)]
pub struct SimulateDialog {
    /// Opened from a menu that cannot restore remembered settings.
    recall: bool,
    factors: Vec<bool>,
    formula: String,
    request: SimulateRequest,
    heredity: Heredity,
    /// The request and table revision the preview was computed for.
    preview: Option<((SimulateRequest, String, u64), Preview)>,
}

impl SimulateDialog {
    /// A new dialog for `table`, with no columns chosen.
    pub fn new(table: &DataTable) -> Self {
        Self {
            recall: false,
            factors: vec![false; table.n_cols()],
            formula: String::new(),
            request: SimulateRequest::new(Vec::new()),
            heredity: Heredity::Weak,
            preview: None,
        }
    }

    /// A new dialog from the Design menu, which cannot reach remembered
    /// settings: the app swaps them in before showing it.
    pub fn opening(table: &DataTable) -> Self {
        Self { recall: true, ..Self::new(table) }
    }

    /// Whether the app should restore remembered settings (once).
    pub fn needs_recall(&mut self) -> bool {
        std::mem::take(&mut self.recall)
    }

    fn request(&self, table: &DataTable) -> Result<SimulateRequest, String> {
        let factors: Vec<DeclaredFactor> = kit::chosen_columns(&self.factors)
            .into_iter()
            .filter(|&c| c < table.n_cols())
            .map(|column| DeclaredFactor { column, low: None, high: None })
            .collect();
        if factors.is_empty() {
            return Err("Choose the factor columns".into());
        }
        Ok(SimulateRequest { factors, formula: self.formula.clone(), ..self.request.clone() })
    }

    fn refresh_preview(&mut self, table: &DataTable, request: &SimulateRequest) {
        let key = (request.clone(), table.name().to_owned(), table.revision());
        if self.preview.as_ref().is_some_and(|(k, _)| *k == key) {
            return;
        }
        let result = simulate_design_responses(table, request)
            .map(|o| {
                let active = o.coefficients.column(2).map_or_else(Vec::new, |c| {
                    (0..o.coefficients.n_rows())
                        .map(|r| {
                            let name = o.coefficients.column(0).map(|n| n.cell_text(r)).unwrap_or_default();
                            format!("{name}: {}", c.cell_text(r))
                        })
                        .collect()
                });
                (o.notes, active)
            })
            .map_err(|e| e.to_string());
        self.preview = Some((key, result));
    }

    /// Show the dialog; Add returns the simulated columns and coefficients.
    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<SimulateOutcome> {
        let dialog = Dialog::new(TITLE).action("Add").default_width(560.0);
        match dialog.show(ctx, |ui| {
            let height = (ctx.screen_rect().height() * 0.8 - kit::em(ui) * 6.0).max(kit::em(ui) * 8.0);
            egui::ScrollArea::vertical().max_height(height).show(ui, |ui| self.contents(ui, table)).inner
        }) {
            Outcome::Reset => {
                *self = Self::new(table);
                Outcome::Open
            }
            Outcome::Run(request, place) => match simulate_design_responses(table, &request) {
                Ok(out) => Outcome::Run(out, place),
                Err(e) => {
                    let key = (request, table.name().to_owned(), table.revision());
                    self.preview = Some((key, Err(e.to_string())));
                    Outcome::Open
                }
            },
            Outcome::Open => Outcome::Open,
            Outcome::Cancel => Outcome::Cancel,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SimulateRequest> {
        kit::note(
            ui,
            "Make up responses for the design in this table from random true coefficients, to see how well an \
             analysis finds the effects that matter before running the experiment. Few effects are active \
             (sparsity), main effects more often than interactions (hierarchy), and an interaction mostly when its \
             factors are active too (heredity; Chipman, Hamada & Wu 1997).",
        );
        ui.strong("Factors");
        kit::column_list(ui, "simulate_factors", table, &mut self.factors, |_| true);
        let names: Vec<String> =
            kit::chosen_columns(&self.factors).into_iter().map(|c| kit::name(table, c).to_owned()).collect();
        ui.separator();
        ui.label("Model (terms joined by +; use * for interactions and ^2 for squares):");
        if self.formula.trim().is_empty() && !names.is_empty() {
            self.formula = ModelFormula::interactions(names.len()).describe(&names);
        }
        ui.add(egui::TextEdit::singleline(&mut self.formula).desired_width(f32::INFINITY))
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Model"));
        ui.horizontal(|ui| {
            ui.label("Presets:");
            let k = names.len();
            if ui.small_button("Main effects").clicked() {
                self.formula = ModelFormula::main_effects(k).describe(&names);
            }
            if ui.small_button("+ Interactions").clicked() {
                self.formula = ModelFormula::interactions(k).describe(&names);
            }
            if ui.small_button("Quadratic").clicked() {
                self.formula = ModelFormula::quadratic(k).describe(&names);
            }
        });
        ui.separator();
        let p = &mut self.request.prior;
        ui.horizontal(|ui| {
            ui.label("Main effect active with probability:");
            ui.add(egui::DragValue::new(&mut p.main).speed(0.01).range(0.0..=1.0)).widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Main effect probability")
            });
        });
        ui.horizontal(|ui| {
            ui.label("Heredity:");
            for (h, label, hint) in [
                (Heredity::Strong, "Strong", "An interaction can be active only when all its factors are"),
                (
                    Heredity::Weak,
                    "Weak",
                    "An interaction is most likely when all its factors are active, less with some",
                ),
                (Heredity::None, "None", "Every term is active with the main-effect probability"),
            ] {
                if ui.radio_value(&mut self.heredity, h, label).on_hover_text(hint).changed() {
                    (p.all_parents, p.some_parents, p.no_parents) = h.probabilities(p.main);
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Higher-order term active with all its factors active:");
            ui.add(egui::DragValue::new(&mut p.all_parents).speed(0.01).range(0.0..=1.0));
            ui.label("some:");
            ui.add(egui::DragValue::new(&mut p.some_parents).speed(0.01).range(0.0..=1.0));
            ui.label("none:");
            ui.add(egui::DragValue::new(&mut p.no_parents).speed(0.01).range(0.0..=1.0));
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("SD of an active coefficient:");
            ui.add(egui::DragValue::new(&mut p.effect_sd).speed(0.1).range(0.0..=1e6));
            ui.label("Noise SD:");
            ui.add(egui::DragValue::new(&mut p.noise_sd).speed(0.1).range(0.0..=1e6));
        });
        kit::note(
            ui,
            "Coefficients are in coded units: half the change in the response from a factor's low to high setting.",
        );
        ui.horizontal(|ui| {
            ui.label("Responses to simulate:");
            ui.add(egui::DragValue::new(&mut self.request.count).range(1..=MAX_SIMULATIONS))
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Responses to simulate"));
            ui.label("Seed:");
            ui.add(kit::seed_value(&mut self.request.seed));
        });
        kit::more_options(ui, |ui| {
            let p = &mut self.request.prior;
            ui.horizontal(|ui| {
                ui.label("Intercept:");
                ui.add(egui::DragValue::new(&mut p.intercept).speed(0.1));
                ui.label("SD of an inactive coefficient:");
                ui.add(egui::DragValue::new(&mut p.inactive_sd).speed(0.01).range(0.0..=1e6));
            });
            ui.horizontal(|ui| {
                ui.label("Column name:");
                ui.add(egui::TextEdit::singleline(&mut self.request.name).desired_width(kit::em(ui) * 10.0));
            });
        });

        let request = self.request(table)?;
        self.refresh_preview(table, &request);
        match &self.preview {
            Some((_, Ok((notes, active)))) => {
                ui.separator();
                egui::ScrollArea::vertical().id_salt("simulate_notes").max_height(kit::em(ui) * 9.0).show(ui, |ui| {
                    for note in notes {
                        ui.label(RichText::new(note).small().monospace());
                    }
                    for line in active.iter().take(20) {
                        ui.label(RichText::new(format!("Active in {line}")).small().monospace());
                    }
                    if active.len() > 20 {
                        ui.label(RichText::new(format!("… and {} more", active.len() - 20)).small().weak());
                    }
                });
                let summary = format!(
                    "Adds {} to {}, with the true coefficients in a new table",
                    kit::count(request.count, "simulated response", "simulated responses"),
                    table.name()
                );
                kit::ready(request, summary)
            }
            Some((_, Err(e))) => Err(sentence_fragment(e)),
            None => Err("Set up the simulation".into()),
        }
    }
}

impl Remember for SimulateDialog {
    fn memory_key(&self) -> String {
        TITLE.into()
    }

    fn set_edit(&mut self, _report: Option<u64>) {}

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.factors = kit::mask(table.n_cols(), columns);
    }
}
