//! Dialog of the factorial analysis (Design ▸ This table ▸ Factorial
//! analysis…): two-level factor columns, one response, the model's terms,
//! the kind of response, and display options.
//!
//! The report runs through [`app_model::ComparisonReport`], so its settings
//! (terms, response type, α, means from the data or the model) are also
//! edited in place from the report's options menu ([`report_settings`]).

use app_model::{
    DeclaredFactor, FactorialAnalysisRequest, FactorialResponse, FactorialTerms, MeansSource, SavedReport,
};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::report_view::ReportRequest;

/// Dialog of the factorial analysis.
#[derive(Clone)]
pub struct FactorialDialog {
    edit: Option<u64>,
    /// Opened from the Design menu, which cannot restore remembered
    /// settings; the app restores them before showing it.
    recall: bool,
    /// Factor columns, indexed by column.
    factors: Vec<bool>,
    response: Option<usize>,
    /// Factors' low and high settings from a report being edited.
    ranges: Vec<DeclaredFactor>,
    /// Factors of the cube plot (unset: the first three factors).
    cube: [Option<usize>; 3],
    settings: FactorialAnalysisRequest,
}

impl FactorialDialog {
    /// An empty dialog; a numeric column named like a response is pre-chosen.
    pub fn new(table: &DataTable) -> Self {
        let response = table.columns().iter().enumerate().position(|(i, c)| {
            let name = c.name.to_lowercase();
            ["response", "yield", "rate", "y"].iter().any(|w| name == *w || name.contains("response"))
                && kit::is_numeric(table, i)
        });
        Self {
            edit: None,
            recall: false,
            factors: vec![false; table.n_cols()],
            response,
            ranges: Vec::new(),
            cube: [None; 3],
            settings: FactorialAnalysisRequest::new(&[], 0),
        }
    }

    /// A new dialog from a menu that cannot reach the remembered settings:
    /// the app swaps them in before showing it ([`Self::needs_recall`]).
    pub fn opening(table: &DataTable) -> Self {
        Self { recall: true, ..Self::new(table) }
    }

    /// Whether the app should restore remembered settings into this dialog
    /// (once; this clears the mark).
    pub fn needs_recall(&mut self) -> bool {
        std::mem::take(&mut self.recall)
    }

    /// The dialog filled from a saved factorial analysis.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::FactorialAnalysis { request } = saved else { return None };
        let factors: Vec<usize> = request.factors.iter().map(|f| f.column).collect();
        let mut cube = [None; 3];
        for (slot, &c) in cube.iter_mut().zip(&request.cube) {
            *slot = kit::valid(table, c);
        }
        Some(Self {
            factors: kit::mask(table.n_cols(), &factors),
            response: kit::valid(table, request.response),
            ranges: request.factors.clone(),
            cube,
            settings: request.clone(),
            ..Self::new(table)
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<Box<ReportRequest>> {
        let dialog = Dialog::new("Factorial analysis").editing(self.edit);
        let outcome = dialog.show(ctx, |ui| {
            self.contents(ui, table)
                .map(|r| kit::Ready { output: Box::new(ReportRequest::Comparison(r.output)), summary: r.summary })
        });
        match outcome {
            Outcome::Reset => {
                *self = Self { edit: self.edit, response: None, ..Self::new(table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "One row per run of a two-level factorial or fractional factorial experiment. A numeric factor's \
             low and high settings are the data's smallest and largest values (coded −1 and +1); a categorical \
             factor needs two levels. Centre runs are used in the fit and left out of the means.",
        );
        let response = self.response;
        let width = kit::em(ui) * 17.0;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.strong("Response");
                let factors = self.factors.clone();
                let kind = self.settings.response_type;
                kit::column_choice(
                    ui,
                    "fa_response",
                    table,
                    &mut self.response,
                    |i| {
                        !factors.get(i).copied().unwrap_or(false)
                            && (kind == FactorialResponse::Binary || kit::is_numeric(table, i))
                    },
                    None,
                );
                ui.add_space(6.0);
                response_type(ui, &mut self.settings.response_type);
            });
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.strong("Factors");
                kit::column_list(ui, "fa_factors", table, &mut self.factors, |i| Some(i) != response);
            });
        });
        ui.add_space(4.0);
        terms_choice(ui, &mut self.settings.terms);
        let chosen: Vec<usize> =
            kit::chosen_columns(&self.factors).into_iter().filter(|&c| c < table.n_cols()).collect();
        let factors: Vec<usize> = chosen.iter().copied().filter(|&c| Some(c) != self.response).collect();
        kit::more_options(ui, |ui| {
            display_settings(ui, &mut self.settings);
            if factors.len() > 3 {
                ui.horizontal(|ui| {
                    ui.label("Cube plot factors:");
                    for (k, slot) in self.cube.iter_mut().enumerate() {
                        kit::column_choice(
                            ui,
                            ("fa_cube", k),
                            table,
                            slot,
                            |i| factors.contains(&i),
                            Some("(default)"),
                        );
                    }
                });
            }
        });
        if factors.is_empty() {
            return Err("Choose at least one factor".into());
        }
        let Some(response) = self.response.filter(|&r| r < table.n_cols()) else {
            return Err("Choose a response column".into());
        };
        if let FactorialTerms::Formula(f) = &self.settings.terms
            && f.trim().is_empty()
        {
            return Err("Type the model's terms, or choose another option".into());
        }
        let mut request = FactorialAnalysisRequest {
            factors: factors.iter().map(|&column| DeclaredFactor { column, low: None, high: None }).collect(),
            response,
            cube: self.cube.iter().flatten().copied().filter(|c| factors.contains(c)).collect(),
            ..self.settings.clone()
        };
        // Settings given in the report being edited stay with their factors.
        for f in &mut request.factors {
            if let Some(old) = self.ranges.iter().find(|r| r.column == f.column) {
                (f.low, f.high) = (old.low, old.high);
            }
        }
        let summary = format!(
            "Factorial analysis of {} on {}",
            kit::name(table, response),
            kit::count(factors.len(), "factor", "factors")
        );
        kit::ready(SavedReport::FactorialAnalysis { request }, summary)
    }
}

impl Remember for FactorialDialog {
    fn memory_key(&self) -> String {
        "Factorial analysis".into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.factors = kit::mask(table.n_cols(), columns);
    }
}

fn response_type(ui: &mut egui::Ui, kind: &mut FactorialResponse) {
    ui.label("Response type:");
    for (k, label) in [
        (FactorialResponse::Continuous, "Continuous"),
        (FactorialResponse::Binary, "Binary (0/1 or two levels)"),
        (FactorialResponse::Count, "Count"),
    ] {
        ui.radio_value(kind, k, label).on_hover_text(k.label());
    }
}

/// The model's terms: every estimable interaction, interactions up to an
/// order, or a formula.
fn terms_choice(ui: &mut egui::Ui, terms: &mut FactorialTerms) {
    ui.label("Model terms:");
    let all = matches!(terms, FactorialTerms::All);
    if ui.radio(all, "Every interaction the runs can estimate (up to the saturated model)").clicked() {
        *terms = FactorialTerms::All;
    }
    ui.horizontal(|ui| {
        let mut order = match terms {
            FactorialTerms::Order(n) => *n,
            _ => 2,
        };
        let on = matches!(terms, FactorialTerms::Order(_));
        if ui.radio(on, "Interactions of up to").clicked() {
            *terms = FactorialTerms::Order(order);
        }
        if ui.add_enabled(on, egui::DragValue::new(&mut order).range(1..=9)).changed() {
            *terms = FactorialTerms::Order(order);
        }
        ui.label("factors");
    });
    ui.horizontal(|ui| {
        let mut text = match terms {
            FactorialTerms::Formula(f) => f.clone(),
            _ => String::new(),
        };
        let on = matches!(terms, FactorialTerms::Formula(_));
        if ui.radio(on, "Formula:").clicked() {
            *terms = FactorialTerms::Formula(text.clone());
        }
        let edit = ui.add_enabled(
            on,
            egui::TextEdit::singleline(&mut text).hint_text("A + B + C + A*B").desired_width(kit::em(ui) * 18.0),
        );
        if edit.changed() {
            *terms = FactorialTerms::Formula(text);
        }
    });
}

/// α and the source of the plotted means.
fn display_settings(ui: &mut egui::Ui, r: &mut FactorialAnalysisRequest) {
    ui.horizontal(|ui| {
        ui.label("Significance level α for active effects:");
        ui.add(egui::DragValue::new(&mut r.alpha).speed(0.005).range(0.001..=0.4));
    });
    ui.horizontal(|ui| {
        ui.label("Main-effect, interaction and cube plots show:");
        ui.radio_value(&mut r.means, MeansSource::Data, "Data means");
        ui.radio_value(&mut r.means, MeansSource::Model, "Model predictions");
    });
}

/// In-report settings of a factorial analysis; `false` for any other report.
pub fn report_settings(ui: &mut egui::Ui, saved: &mut SavedReport) -> bool {
    let SavedReport::FactorialAnalysis { request } = saved else { return false };
    terms_choice(ui, &mut request.terms);
    ui.horizontal(|ui| response_type(ui, &mut request.response_type));
    display_settings(ui, request);
    true
}
