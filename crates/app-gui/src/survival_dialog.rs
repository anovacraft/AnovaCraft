//! Dialog for a survival analysis: time column, optional censoring status and
//! grouping columns.

use app_model::{SavedReport, StatusCoding, SurvivalRequest};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};

#[derive(Clone, Default)]
pub struct SurvivalDialog {
    edit: Option<u64>,
    time: Option<usize>,
    status: Option<usize>,
    coding: StatusCoding,
    group: Option<usize>,
}

impl SurvivalDialog {
    /// The dialog filled from a saved survival report.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::Survival { request, .. } = saved else { return None };
        Some(Self {
            edit: None,
            time: kit::valid(table, request.time),
            status: request.status.and_then(|c| kit::valid(table, c)),
            coding: request.coding,
            group: request.group.and_then(|c| kit::valid(table, c)),
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<SurvivalRequest> {
        let dialog = Dialog::new("Survival analysis").editing(self.edit).default_width(460.0);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::default() };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SurvivalRequest> {
        kit::note(
            ui,
            "Kaplan–Meier survival curves with confidence bands, median survival, and Weibull and lognormal fits. \
             Rows still running when observation stopped are censored.",
        );
        let numeric = |i: usize| kit::is_numeric(table, i);
        egui::Grid::new("survival_columns").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Time to event:");
            kit::column_choice(ui, "survival_time", table, &mut self.time, numeric, None);
            ui.end_row();

            ui.label("Status (optional):");
            let time = self.time;
            kit::column_choice(
                ui,
                "survival_status",
                table,
                &mut self.status,
                |i| numeric(i) && Some(i) != time,
                Some("None: every time is an event"),
            );
            ui.end_row();

            ui.label("");
            ui.add_enabled_ui(self.status.is_some(), |ui| {
                ui.vertical(|ui| {
                    ui.radio_value(
                        &mut self.coding,
                        StatusCoding::NonzeroIsCensored,
                        "Non-zero status = censored (still running)",
                    );
                    ui.radio_value(&mut self.coding, StatusCoding::NonzeroIsEvent, "Non-zero status = event");
                });
            });
            ui.end_row();

            ui.label("Group by (optional):");
            let status = self.status;
            kit::column_choice(
                ui,
                "survival_group",
                table,
                &mut self.group,
                |i| Some(i) != time && Some(i) != status,
                Some("None"),
            );
            ui.end_row();
        });
        // A column chosen as time cannot also be status or group.
        if self.status.is_some() && self.status == self.time {
            self.status = None;
        }
        if self.group.is_some() && (self.group == self.time || self.group == self.status) {
            self.group = None;
        }
        kit::note(ui, "Fitted distributions, bands and the full estimate table are in the report's options.");
        let Some(time) = self.time else { return Err("Choose a time-to-event column".into()) };
        let mut summary = format!("Survival curves of {}", kit::name(table, time));
        if let Some(g) = self.group {
            summary.push_str(&format!(" by {}", kit::name(table, g)));
        }
        if let Some(s) = self.status {
            summary.push_str(&format!(", censoring from {}", kit::name(table, s)));
        }
        kit::ready(SurvivalRequest { time, status: self.status, coding: self.coding, group: self.group }, summary)
    }
}

impl Remember for SurvivalDialog {
    fn memory_key(&self) -> String {
        "Survival analysis".into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.time = columns.iter().copied().find(|&c| kit::is_numeric(table, c));
    }
}
