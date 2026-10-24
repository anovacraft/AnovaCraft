//! Dialog for exploratory factor analysis: numeric columns, extraction
//! method, number of factors, rotation, and (under More options) the matrix
//! analysed, the factor-score method and the promax power.

use app_model::{Extraction, FactorRequest, Rotation, SavedReport, ScoreMethod};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::tree_dialog::heading;

#[derive(Clone)]
pub struct FactorDialog {
    edit: Option<u64>,
    chosen: Vec<bool>,
    extraction: Extraction,
    /// Use a fixed number of factors instead of the eigenvalue guideline.
    fixed: bool,
    factors: usize,
    rotation: Rotation,
    covariance: bool,
    scores: ScoreMethod,
    power: f64,
}

impl FactorDialog {
    pub fn new(table: &DataTable) -> Self {
        let d = FactorRequest::new(Vec::new());
        Self {
            edit: None,
            chosen: vec![false; table.n_cols()],
            extraction: d.extraction,
            fixed: false,
            factors: 2,
            rotation: d.rotation,
            covariance: d.covariance,
            scores: d.scores,
            power: d.promax_power,
        }
    }

    /// The dialog filled from a saved factor analysis.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::FactorAnalysis { request: r } = saved else { return None };
        let fresh = Self::new(table);
        Some(Self {
            edit: None,
            chosen: kit::mask(table.n_cols(), &r.columns),
            extraction: r.extraction,
            fixed: r.factors.is_some(),
            factors: r.factors.unwrap_or(fresh.factors),
            rotation: r.rotation,
            covariance: r.covariance,
            scores: r.scores,
            power: r.promax_power,
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<FactorRequest> {
        let dialog = Dialog::new("Factor analysis").editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<FactorRequest> {
        ui.set_max_width(kit::em(ui) * 30.0);
        kit::note(
            ui,
            "Explains the correlations among several columns by a few unobserved common factors: loadings \
             show how strongly each column reflects each factor, and rotation makes them easier to read.",
        );
        ui.label("Columns (numeric, three or more):");
        kit::column_list(ui, "factor_columns", table, &mut self.chosen, |i| kit::is_numeric(table, i));
        let columns: Vec<usize> =
            kit::chosen_columns(&self.chosen).into_iter().filter(|&i| kit::is_numeric(table, i)).collect();
        heading(ui, "Extraction");
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.extraction, Extraction::MaximumLikelihood, "Maximum likelihood")
                .on_hover_text("Also tests whether the number of factors is enough (chi-square)");
            ui.radio_value(&mut self.extraction, Extraction::PrincipalAxis, "Principal axis")
                .on_hover_text("Iterated communalities; no distributional assumption");
        });
        heading(ui, "Number of factors");
        ui.radio_value(&mut self.fixed, false, "Eigenvalues above 1")
            .on_hover_text("Kaiser's guideline: as many factors as eigenvalues of the correlation matrix above 1");
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.fixed, true, "Exactly");
            ui.add_enabled(self.fixed, egui::DragValue::new(&mut self.factors).range(1..=50));
            ui.label("factors");
        });
        heading(ui, "Rotation");
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.rotation, Rotation::Varimax, "Varimax")
                .on_hover_text("Orthogonal: factors stay uncorrelated");
            ui.radio_value(&mut self.rotation, Rotation::Promax, "Promax")
                .on_hover_text("Oblique: factors may correlate; reports pattern and structure loadings");
            ui.radio_value(&mut self.rotation, Rotation::None, "None");
        });
        kit::more_options(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Matrix:");
                ui.radio_value(&mut self.covariance, false, "Correlations");
                ui.radio_value(&mut self.covariance, true, "Covariances")
                    .on_hover_text("Keeps the columns' units; use when they share a scale");
            });
            ui.horizontal(|ui| {
                ui.label("Saved scores:");
                ui.radio_value(&mut self.scores, ScoreMethod::Regression, "Regression");
                ui.radio_value(&mut self.scores, ScoreMethod::Bartlett, "Bartlett");
            });
            ui.add_enabled_ui(self.rotation == Rotation::Promax, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Promax power:");
                    ui.add(egui::DragValue::new(&mut self.power).range(1.5..=8.0).speed(0.1));
                });
            });
        });
        if columns.len() < 3 {
            return Err("Choose at least three columns".into());
        }
        if self.fixed && self.factors >= columns.len() {
            return Err(format!("Choose fewer factors than columns (at most {})", columns.len() - 1));
        }
        let factors = if self.fixed {
            kit::count(self.factors, "factor", "factors")
        } else {
            "factors with eigenvalue above 1".to_owned()
        };
        let summary = format!("Factor analysis of {}, {factors}", kit::count(columns.len(), "column", "columns"));
        kit::ready(
            FactorRequest {
                columns,
                factors: self.fixed.then_some(self.factors),
                extraction: self.extraction,
                rotation: self.rotation,
                covariance: self.covariance,
                scores: self.scores,
                promax_power: self.power,
            },
            summary,
        )
    }
}

impl Remember for FactorDialog {
    fn memory_key(&self) -> String {
        "Factor analysis".into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        let numeric: Vec<usize> = columns.iter().copied().filter(|&c| kit::is_numeric(table, c)).collect();
        self.chosen = kit::mask(table.n_cols(), &numeric);
    }
}
