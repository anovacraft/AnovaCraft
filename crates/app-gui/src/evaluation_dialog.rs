//! The design evaluation dialog (Design ▸ This table ▸ Evaluate design…):
//! factor columns, the model to evaluate the runs for, the alias terms, the
//! anticipated coefficient and significance level of the power calculation,
//! an optional column whose two levels are two designs to compare, and
//! optional constraints that cut the region the prediction variance is
//! evaluated over ([`app_model::design_evaluation`]).
//!
//! The report runs through [`app_model::ComparisonReport`], so its settings
//! are also edited in place from the report's options menu
//! ([`report_settings`]).

use app_model::{DeclaredFactor, DesignEvaluationRequest, SavedReport};
use eframe::egui::{self, RichText};
use stat_data::DataTable;
use stat_doe::ModelFormula;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::report_view::ReportRequest;

const TITLE: &str = "Evaluate a design";

/// Model presets; `Custom` keeps the typed formula.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Preset {
    MainEffects,
    Interactions,
    Quadratic,
    Custom,
}

impl Preset {
    const ALL: [Self; 4] = [Self::MainEffects, Self::Interactions, Self::Quadratic, Self::Custom];

    fn label(self) -> &'static str {
        match self {
            Self::MainEffects => "Main effects",
            Self::Interactions => "Main effects and two-factor interactions",
            Self::Quadratic => "Full quadratic (response surface)",
            Self::Custom => "Custom formula",
        }
    }

    /// The preset's formula over `names` (`None` for a custom one).
    fn formula(self, names: &[String]) -> Option<String> {
        let k = names.len();
        let f = match self {
            Self::MainEffects => ModelFormula::main_effects(k),
            Self::Interactions => ModelFormula::interactions(k),
            Self::Quadratic => ModelFormula::quadratic(k),
            Self::Custom => return None,
        };
        Some(f.describe(names))
    }
}

/// Dialog for the design evaluation report.
#[derive(Clone)]
pub struct EvaluationDialog {
    edit: Option<u64>,
    /// Opened from the Design menu, which cannot restore remembered settings.
    recall: bool,
    factors: Vec<bool>,
    /// Low and high settings of factors from a report being edited.
    ranges: Vec<DeclaredFactor>,
    preset: Preset,
    model: String,
    alias: String,
    coefficients: Vec<f64>,
    alpha: f64,
    compare: Option<usize>,
    /// Constraints in actual units, one per line (empty: the whole cube).
    constraints: String,
}

impl EvaluationDialog {
    /// A fresh dialog: no factors, the interaction model, coefficient 1, α = 0.05.
    pub fn new(table: &DataTable) -> Self {
        Self {
            edit: None,
            recall: false,
            factors: vec![false; table.n_cols()],
            ranges: Vec::new(),
            preset: Preset::Interactions,
            model: String::new(),
            alias: String::new(),
            coefficients: vec![1.0],
            alpha: 0.05,
            compare: None,
            constraints: String::new(),
        }
    }

    /// A new dialog from the Design menu: the app restores remembered
    /// settings before showing it ([`Self::needs_recall`]).
    pub fn opening(table: &DataTable) -> Self {
        Self { recall: true, ..Self::new(table) }
    }

    /// Whether the app should restore remembered settings (once).
    pub fn needs_recall(&mut self) -> bool {
        std::mem::take(&mut self.recall)
    }

    /// The dialog filled from a saved design evaluation.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::DesignEvaluation { request } = saved else { return None };
        let columns: Vec<usize> = request.factors.iter().map(|f| f.column).collect();
        let names: Vec<String> = columns.iter().map(|&c| kit::name(table, c).to_owned()).collect();
        let preset = Preset::ALL
            .into_iter()
            .find(|p| {
                p.formula(&names).is_some_and(|f| f == request.model)
                    || matches!(
                        (p, request.model.trim().to_ascii_lowercase().as_str()),
                        (Preset::MainEffects, "main effects")
                            | (Preset::Interactions, "interactions")
                            | (Preset::Quadratic, "quadratic")
                    )
            })
            .unwrap_or(Preset::Custom);
        Some(Self {
            factors: kit::mask(table.n_cols(), &columns),
            ranges: request.factors.clone(),
            preset,
            model: request.model.clone(),
            alias: request.alias_terms.clone(),
            coefficients: request.coefficients.clone(),
            alpha: request.alpha,
            compare: request.compare_by.and_then(|c| kit::valid(table, c)),
            constraints: request.constraints.clone(),
            ..Self::new(table)
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<Box<ReportRequest>> {
        let dialog = Dialog::new(TITLE).editing(self.edit);
        let outcome = dialog.show(ctx, |ui| {
            self.contents(ui, table)
                .map(|r| kit::Ready { output: Box::new(ReportRequest::Comparison(r.output)), summary: r.summary })
        });
        match outcome {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "Evaluate the runs in this table for a model before collecting responses: power of each term, \
             prediction variance over the region, aliasing and efficiencies. Numeric factors are coded −1 to +1 \
             from their smallest to largest values; a categorical factor needs two levels.",
        );
        let compare = self.compare;
        ui.strong("Factors");
        kit::column_list(ui, "eval_factors", table, &mut self.factors, |i| Some(i) != compare);
        let factors: Vec<usize> = kit::chosen_columns(&self.factors)
            .into_iter()
            .filter(|&c| c < table.n_cols() && Some(c) != compare)
            .collect();
        let names: Vec<String> = factors.iter().map(|&c| kit::name(table, c).to_owned()).collect();

        ui.add_space(4.0);
        ui.strong("Model");
        for p in Preset::ALL {
            ui.radio_value(&mut self.preset, p, p.label());
        }
        if let Some(text) = self.preset.formula(&names) {
            self.model = text;
        }
        let edited = ui
            .add(
                egui::TextEdit::multiline(&mut self.model)
                    .desired_rows(2)
                    .desired_width(kit::em(ui) * 30.0)
                    .hint_text("e.g. A + B + A*B + A^2"),
            )
            .on_hover_text("Terms joined by +; * or : joins factors, ^2 squares a factor; names are column names")
            .changed();
        if edited {
            self.preset = Preset::Custom;
        }
        ui.add_space(4.0);
        self.options(ui, table);

        if factors.is_empty() {
            return Err("Choose at least one factor".into());
        }
        let formula = ModelFormula::parse(&self.model, &names).map_err(|e| format!("Model: {e}"))?;
        if !self.alias.trim().is_empty() {
            ModelFormula::parse(&self.alias, &names).map_err(|e| format!("Alias terms: {e}"))?;
        }
        let p = formula.n_terms() - 1;
        if self.coefficients.len() != 1 && self.coefficients.len() != p {
            self.coefficients = vec![self.coefficients.first().copied().unwrap_or(1.0)];
        }
        let mut request = DesignEvaluationRequest {
            model: self.model.clone(),
            alias_terms: self.alias.trim().to_owned(),
            coefficients: self.coefficients.clone(),
            alpha: self.alpha,
            compare_by: self.compare,
            constraints: self.constraints.trim().to_owned(),
            ..DesignEvaluationRequest::new(&factors)
        };
        // Settings given in the report being edited stay with their factors.
        for f in &mut request.factors {
            if let Some(old) = self.ranges.iter().find(|r| r.column == f.column) {
                (f.low, f.high) = (old.low, old.high);
            }
        }
        let mut summary = format!(
            "Evaluate {} for a {}-term model",
            kit::count(factors.len(), "factor", "factors"),
            formula.n_terms()
        );
        if let Some(c) = self.compare {
            summary.push_str(&format!(", comparing the designs in {}", kit::name(table, c)));
        }
        if !self.constraints.trim().is_empty() {
            summary.push_str(", over the constrained region");
        }
        kit::ready(SavedReport::DesignEvaluation { request }, summary)
    }

    fn options(&mut self, ui: &mut egui::Ui, table: &DataTable) {
        let factors = self.factors.clone();
        egui::Grid::new("eval_options").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Compare designs by:");
            let allow = |i: usize| !factors.get(i).copied().unwrap_or(false);
            kit::column_choice(ui, "eval_compare", table, &mut self.compare, allow, Some("None: one design"));
            ui.end_row();
        });
        test_settings(ui, &mut self.coefficients, &mut self.alpha);
        constraints_field(ui, &mut self.constraints);
        kit::more_options(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Alias terms:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.alias)
                        .desired_width(kit::em(ui) * 22.0)
                        .hint_text("Two-factor interactions not in the model"),
                );
            });
        });
    }
}

impl Remember for EvaluationDialog {
    fn memory_key(&self) -> String {
        TITLE.into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.factors = kit::mask(table.n_cols(), columns);
    }
}

/// The anticipated coefficient (common, or one per term) and the
/// significance level.
fn test_settings(ui: &mut egui::Ui, coefficients: &mut [f64], alpha: &mut f64) {
    egui::Grid::new("eval_test").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        if let [one] = coefficients {
            ui.label("Anticipated coefficient (in σ):")
                .on_hover_text("The effect each term is expected to have, in units of the error standard deviation");
            ui.add(egui::DragValue::new(one).speed(0.05).range(0.0..=100.0));
            ui.end_row();
        } else {
            for (j, b) in coefficients.iter_mut().enumerate() {
                ui.label(format!("Coefficient of term {} (in σ):", j + 1));
                ui.add(egui::DragValue::new(b).speed(0.05).range(0.0..=100.0));
                ui.end_row();
            }
        }
        ui.label("Significance level α:");
        ui.add(egui::DragValue::new(alpha).speed(0.005).range(0.001..=0.5));
        ui.end_row();
    });
}

/// Constraints that cut the region (optional), in actual units.
fn constraints_field(ui: &mut egui::Ui, constraints: &mut String) {
    ui.label("Constraints on the region (optional, one per line):").on_hover_text(
        "Limits in the factors' actual units, such as Temp + 4*Time <= 300, or combinations to leave out, such as \
         exclude Temp > 180 and Time > 25. The largest and average prediction variance and the fraction of design \
         space are then over the constrained region. Optimal designs made with constraints fill this in.",
    );
    ui.add(
        egui::TextEdit::multiline(constraints)
            .desired_rows(2)
            .desired_width(kit::em(ui) * 30.0)
            .hint_text("None: the whole cube of factor settings"),
    )
    .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Region constraints"));
}

/// In-report settings of the design evaluation; `false` for any other report.
pub fn report_settings(ui: &mut egui::Ui, saved: &mut SavedReport) -> bool {
    let SavedReport::DesignEvaluation { request } = saved else { return false };
    ui.label("Model (terms joined by +; presets: main effects, interactions, quadratic):");
    ui.add(egui::TextEdit::multiline(&mut request.model).desired_rows(2).desired_width(f32::INFINITY));
    ui.horizontal(|ui| {
        for (label, text) in
            [("Main effects", "main effects"), ("Interactions", "interactions"), ("Quadratic", "quadratic")]
        {
            if ui.small_button(label).clicked() {
                request.model = text.to_owned();
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label("Alias terms:");
        ui.add(
            egui::TextEdit::singleline(&mut request.alias_terms).hint_text("Two-factor interactions not in the model"),
        );
    });
    test_settings(ui, &mut request.coefficients, &mut request.alpha);
    constraints_field(ui, &mut request.constraints);
    ui.label(RichText::new("Factor settings coded −1 and +1 (unchecked: the data's smallest and largest values):"));
    egui::Grid::new("eval_ranges").num_columns(3).show(ui, |ui| {
        for (i, f) in request.factors.iter_mut().enumerate() {
            let DeclaredFactor { low, high, .. } = f;
            ui.label(format!("Factor {}", i + 1));
            for (label, value) in [("Low", low), ("High", high)] {
                ui.horizontal(|ui| {
                    let mut used = value.is_some();
                    ui.checkbox(&mut used, label);
                    let mut v = value.unwrap_or(if label == "Low" { -1.0 } else { 1.0 });
                    ui.add_enabled(used, egui::DragValue::new(&mut v).speed(0.01));
                    *value = used.then_some(v);
                });
            }
            ui.end_row();
        }
    });
    true
}
