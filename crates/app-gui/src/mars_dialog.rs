//! Dialog for multivariate adaptive regression splines: response,
//! predictors (some entering only linearly), validation (which chooses the
//! number of terms), interaction degree and pruning settings.

use app_model::{MarsRequest, SavedReport};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::tree_dialog::{ValidationChoice, check_roles, heading, is_categorical, roles_ui, used_columns};

#[derive(Clone)]
pub struct MarsDialog {
    edit: Option<u64>,
    response: Option<usize>,
    chosen: Vec<bool>,
    linear: Vec<bool>,
    validation: ValidationChoice,
    degree: usize,
    auto_terms: bool,
    max_terms: usize,
    auto_penalty: bool,
    penalty: f64,
    threshold: f64,
    auto_spans: bool,
    min_span: usize,
    end_span: usize,
}

impl MarsDialog {
    pub fn new(table: &DataTable) -> Self {
        let d = MarsRequest::new(0, Vec::new());
        Self {
            edit: None,
            response: None,
            chosen: vec![false; table.n_cols()],
            linear: vec![false; table.n_cols()],
            validation: ValidationChoice::for_mars(),
            degree: 2,
            auto_terms: true,
            max_terms: 21,
            auto_penalty: true,
            penalty: 3.0,
            threshold: d.threshold,
            auto_spans: true,
            min_span: 1,
            end_span: 1,
        }
    }

    /// The dialog filled from a saved MARS report.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::Mars { request: r, .. } = saved else { return None };
        let fresh = Self::new(table);
        Some(Self {
            edit: None,
            response: kit::valid(table, r.response),
            chosen: kit::mask(table.n_cols(), &r.predictors),
            linear: kit::mask(table.n_cols(), &r.linear),
            validation: ValidationChoice::for_mars().with_method(&r.validation, table),
            degree: r.max_degree,
            auto_terms: r.max_terms == 0,
            max_terms: if r.max_terms == 0 { fresh.max_terms } else { r.max_terms },
            auto_penalty: r.penalty.is_none(),
            penalty: r.penalty.unwrap_or(fresh.penalty),
            threshold: r.threshold,
            auto_spans: r.min_span == 0 && r.end_span == 0,
            min_span: r.min_span.max(1),
            end_span: r.end_span.max(1),
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<MarsRequest> {
        let dialog = Dialog::new("Multivariate adaptive regression splines").editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<MarsRequest> {
        kit::note(
            ui,
            "Builds a continuous response from hinge functions max(0, x − t) and max(0, t − x) with knots t \
             found in the data, and their products: a forward pass adds the pairs that most reduce the error, \
             then a backward pass removes terms until GCV (or the validation rows) is smallest. A categorical \
             predictor enters through indicators of its levels.",
        );
        let predictors = roles_ui(
            ui,
            table,
            &mut self.response,
            &mut self.chosen,
            "Predictors (training rows missing a predictor are left out):",
            |_| true,
        );
        let continuous: Vec<usize> = predictors.iter().copied().filter(|&i| !is_categorical(table, i)).collect();
        if !continuous.is_empty() {
            ui.label("Enter linearly (no knots):")
                .on_hover_text("These continuous predictors enter as x times a parent term, never through hinges");
            if self.linear.len() < table.n_cols() {
                self.linear.resize(table.n_cols(), false);
            }
            kit::column_list(ui, "linear_predictors", table, &mut self.linear, |i| continuous.contains(&i));
        }
        let used = used_columns(self.response, &predictors);
        heading(ui, "Interactions");
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.degree, 1, "None (additive)").on_hover_text("Each term involves one predictor");
            ui.radio_value(&mut self.degree, 2, "Pairs").on_hover_text("Terms may multiply hinges of two predictors");
            ui.radio_value(&mut self.degree, 3, "Up to three predictors");
        });
        self.validation.ui(ui, table, &used);
        kit::more_options(ui, |ui| {
            heading(ui, "Forward pass");
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.auto_terms, "Automatic maximum number of terms")
                    .on_hover_text("min(200, max(20, 2 × number of variables)) + 1, the intercept included");
                ui.add_enabled(!self.auto_terms, egui::DragValue::new(&mut self.max_terms).range(3..=401));
            });
            ui.horizontal(|ui| {
                ui.label("Threshold (smallest gain in R²):");
                ui.add(egui::DragValue::new(&mut self.threshold).speed(0.0001).range(0.0..=0.5));
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.auto_spans, "Friedman's minimum and end spans").on_hover_text(
                    "Knots at least L cases apart and L_e cases from the ends, from Friedman's rules with α = 0.05",
                );
            });
            if !self.auto_spans {
                ui.horizontal(|ui| {
                    ui.label("Cases between knots:");
                    ui.add(egui::DragValue::new(&mut self.min_span).range(1..=1000));
                    ui.label("Cases outside the end knots:");
                    ui.add(egui::DragValue::new(&mut self.end_span).range(1..=1000));
                });
            }
            heading(ui, "Pruning");
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.auto_penalty, "Default GCV penalty per knot")
                    .on_hover_text("2 for an additive model, 3 with interactions");
                ui.add_enabled(
                    !self.auto_penalty,
                    egui::DragValue::new(&mut self.penalty).speed(0.1).range(0.0..=20.0),
                );
            });
            self.validation.more_ui(ui, table, &used);
        });
        let (response, validation) = check_roles(self.response, &predictors, &self.validation)?;
        if is_categorical(table, response) {
            return Err("Adaptive regression splines model a continuous response; choose a continuous column".into());
        }
        let linear: Vec<usize> = continuous.iter().copied().filter(|&i| self.linear.get(i) == Some(&true)).collect();
        let summary = format!(
            "Adaptive splines for {} on {}, {}",
            kit::name(table, response),
            kit::count(predictors.len(), "predictor", "predictors"),
            match self.degree {
                1 => "additive".to_owned(),
                d => format!("interactions of up to {d} predictors"),
            }
        );
        let d = MarsRequest::new(response, predictors.clone());
        kit::ready(
            MarsRequest {
                linear,
                validation,
                max_degree: self.degree,
                max_terms: if self.auto_terms { 0 } else { self.max_terms },
                penalty: (!self.auto_penalty).then_some(self.penalty),
                threshold: self.threshold,
                min_span: if self.auto_spans { 0 } else { self.min_span },
                end_span: if self.auto_spans { 0 } else { self.end_span },
                ..d
            },
            summary,
        )
    }
}

impl Remember for MarsDialog {
    fn memory_key(&self) -> String {
        "Multivariate adaptive regression splines".into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.chosen = kit::mask(table.n_cols(), columns);
    }
}
