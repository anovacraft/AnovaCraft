//! Dialog for an EWMA or CUSUM control chart.

use app_model::{SavedReport, TimeWeightedKind, TimeWeightedRequest};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};

#[derive(Clone)]
pub struct TimeWeightedDialog {
    edit: Option<u64>,
    column: Option<usize>,
    kind: TimeWeightedKind,
    known_target: bool,
    mean: f64,
    sigma: f64,
}

impl TimeWeightedDialog {
    /// A dialog starting on the EWMA (`ewma = true`) or CUSUM chart.
    pub fn new(ewma: bool) -> Self {
        Self {
            edit: None,
            column: None,
            kind: if ewma { TimeWeightedKind::EWMA_DEFAULT } else { TimeWeightedKind::CUSUM_DEFAULT },
            known_target: false,
            mean: 0.0,
            sigma: 1.0,
        }
    }

    /// The dialog filled from a saved EWMA or CUSUM chart.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::TimeWeighted { request } = saved else { return None };
        let mut d = Self { column: kit::valid(table, request.column), kind: request.kind, ..Self::new(true) };
        if let Some((mean, sigma)) = request.target {
            (d.known_target, d.mean, d.sigma) = (true, mean, sigma);
        }
        Some(d)
    }

    fn is_ewma(&self) -> bool {
        matches!(self.kind, TimeWeightedKind::Ewma { .. })
    }

    fn title(&self) -> &'static str {
        if self.is_ewma() { "EWMA chart" } else { "CUSUM chart" }
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<TimeWeightedRequest> {
        // The title follows the chart type, so the window is kept under one id.
        let dialog = Dialog::new(self.title()).id_salt("time_weighted_dialog").editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(self.is_ewma()) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<TimeWeightedRequest> {
        ui.label("Measurement (numeric, in time order down the rows):");
        kit::column_choice(ui, "time_weighted_column", table, &mut self.column, |i| kit::is_numeric(table, i), None);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let is_ewma = self.is_ewma();
            if ui.radio(is_ewma, "EWMA").clicked() && !is_ewma {
                self.kind = TimeWeightedKind::EWMA_DEFAULT;
            }
            if ui.radio(!is_ewma, "CUSUM").clicked() && is_ewma {
                self.kind = TimeWeightedKind::CUSUM_DEFAULT;
            }
        });
        kit::more_options(ui, |ui| {
            match &mut self.kind {
                TimeWeightedKind::Ewma { lambda, width } => {
                    ui.horizontal(|ui| {
                        ui.label("Weight λ:");
                        ui.add(egui::DragValue::new(lambda).range(0.01..=1.0).speed(0.01));
                        ui.label("Limit width L:");
                        ui.add(egui::DragValue::new(width).range(0.5..=5.0).speed(0.05));
                    });
                    kit::note(ui, "Smaller λ reacts to smaller shifts; λ = 1 is an individuals chart.");
                }
                TimeWeightedKind::Cusum { k, h } => {
                    ui.horizontal(|ui| {
                        ui.label("Reference k:");
                        ui.add(egui::DragValue::new(k).range(0.0..=3.0).speed(0.05));
                        ui.label("Decision h:");
                        ui.add(egui::DragValue::new(h).range(0.5..=20.0).speed(0.1));
                    });
                    kit::note(ui, "In units of σ. k is usually half the shift to detect (0.5 for 1σ).");
                }
            }
            ui.add_space(4.0);
            ui.checkbox(&mut self.known_target, "Use a known target mean and σ");
            ui.add_enabled_ui(self.known_target, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Mean:");
                    ui.add(egui::DragValue::new(&mut self.mean).speed(0.1));
                    ui.label("σ:");
                    ui.add(egui::DragValue::new(&mut self.sigma).range(1e-9..=f64::MAX).speed(0.01));
                });
            });
            if !self.known_target {
                kit::note(ui, "Otherwise both are estimated from the data.");
            }
        });
        let Some(column) = self.column else { return Err("Choose a measurement column".into()) };
        let target = self.known_target.then_some((self.mean, self.sigma));
        let settings = match self.kind {
            TimeWeightedKind::Ewma { lambda, width } => format!("λ = {lambda}, L = {width}"),
            TimeWeightedKind::Cusum { k, h } => format!("k = {k}, h = {h}"),
        };
        let mut summary = format!("{} of {} ({settings}", self.title(), kit::name(table, column));
        if let Some((mean, sigma)) = target {
            summary.push_str(&format!(", target {mean} and σ {sigma}"));
        }
        summary.push(')');
        kit::ready(TimeWeightedRequest { column, kind: self.kind, target }, summary)
    }
}

impl Remember for TimeWeightedDialog {
    fn memory_key(&self) -> String {
        self.title().into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.column = columns.iter().copied().find(|&c| kit::is_numeric(table, c));
    }
}
