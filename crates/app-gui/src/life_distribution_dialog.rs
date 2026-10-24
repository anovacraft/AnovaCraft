//! Dialog for Reliability ▸ Life distributions…: censored failure times in
//! one of two layouts, the families to fit and the estimates to report.

use crate::dialog_kit::ComboExt as _;
use app_model::{LifeDataLayout, LifeDistributionRequest, SavedReport, StatusCoding};
use eframe::egui;
use stat_core::distribution_fit::FitFamily;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Readiness};

/// Which data layout the dialog collects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    Times,
    Intervals,
}

/// Columns and settings of a life-distribution analysis.
#[derive(Clone)]
pub struct LifeDistributionDialog {
    layout: Layout,
    time: Option<usize>,
    censor: Option<usize>,
    coding: StatusCoding,
    start: Option<usize>,
    end: Option<usize>,
    frequency: Option<usize>,
    group: Option<usize>,
    families: Vec<bool>,
    detail: Option<FitFamily>,
    percentiles: String,
    times: String,
    known_shape: bool,
    shape: f64,
    confidence: f64,
}

impl Default for LifeDistributionDialog {
    fn default() -> Self {
        Self {
            layout: Layout::Times,
            time: None,
            censor: None,
            coding: StatusCoding::NonzeroIsCensored,
            start: None,
            end: None,
            frequency: None,
            group: None,
            families: FitFamily::ALL.iter().map(|f| LifeDistributionRequest::DEFAULT_FAMILIES.contains(f)).collect(),
            detail: None,
            percentiles: "1, 10, 50".to_owned(),
            times: String::new(),
            known_shape: false,
            shape: 2.0,
            confidence: LifeDistributionRequest::DEFAULT_CONFIDENCE,
        }
    }
}

/// Numbers separated by commas, semicolons or spaces; `None` if any is not a number.
fn parse_numbers(text: &str) -> Option<Vec<f64>> {
    text.split([',', ';', ' '])
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().parse::<f64>().ok().filter(|v| v.is_finite()))
        .collect()
}

/// Numbers as text, separated by commas ("1, 10, 50").
fn numbers_text(values: impl IntoIterator<Item = f64>) -> String {
    values.into_iter().map(|v| app_model::format::format_number(v, 10)).collect::<Vec<_>>().join(", ")
}

impl LifeDistributionDialog {
    /// The dialog filled from a saved life-distribution analysis.
    pub fn from_saved(request: &LifeDistributionRequest, table: &DataTable) -> Self {
        let valid = |c: usize| kit::valid(table, c);
        let mut d = Self {
            frequency: request.frequency.and_then(valid),
            group: request.group.and_then(valid),
            families: FitFamily::ALL.iter().map(|f| request.families.contains(f)).collect(),
            detail: request.detail,
            percentiles: numbers_text(request.probabilities.iter().map(|p| p * 100.0)),
            times: numbers_text(request.times.iter().copied()),
            known_shape: request.known_shape.is_some(),
            confidence: request.confidence,
            ..Self::default()
        };
        if let Some(shape) = request.known_shape {
            d.shape = shape;
        }
        match request.layout {
            LifeDataLayout::Times { time, censor, coding } => {
                d.layout = Layout::Times;
                d.time = valid(time);
                d.censor = censor.and_then(valid);
                d.coding = coding;
            }
            LifeDataLayout::Intervals { start, end } => {
                d.layout = Layout::Intervals;
                d.start = valid(start);
                d.end = valid(end);
            }
        }
        d
    }

    /// Put the first numeric selected column in the time role.
    pub fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.time = columns.iter().copied().find(|&c| kit::is_numeric(table, c));
    }

    /// Draw the settings; ready when they are complete and valid.
    pub fn ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "Maximum-likelihood life distributions for failure times with right, left or interval censoring, \
             compared with Turnbull's nonparametric estimate.",
        );
        let numeric = |i: usize| kit::is_numeric(table, i);
        ui.horizontal(|ui| {
            ui.label("Data:");
            ui.radio_value(&mut self.layout, Layout::Times, "Time and censor columns");
            ui.radio_value(&mut self.layout, Layout::Intervals, "Start and end of the failure interval");
        });
        egui::Grid::new("life_columns").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            match self.layout {
                Layout::Times => {
                    ui.label("Time:");
                    kit::column_choice(ui, "life_time", table, &mut self.time, numeric, None);
                    ui.end_row();
                    ui.label("Censor (optional):");
                    kit::column_choice(
                        ui,
                        "life_censor",
                        table,
                        &mut self.censor,
                        numeric,
                        Some("None: every time is a failure"),
                    );
                    ui.end_row();
                    ui.label("");
                    ui.vertical(|ui| {
                        ui.radio_value(&mut self.coding, StatusCoding::NonzeroIsCensored, "Non-zero = still running");
                        ui.radio_value(&mut self.coding, StatusCoding::NonzeroIsEvent, "Non-zero = failed");
                    });
                    ui.end_row();
                }
                Layout::Intervals => {
                    ui.label("Start:");
                    kit::column_choice(ui, "life_start", table, &mut self.start, numeric, None);
                    ui.end_row();
                    ui.label("");
                    kit::note(ui, "Last time seen working; empty = failed before the first look.");
                    ui.end_row();
                    ui.label("End:");
                    kit::column_choice(ui, "life_end", table, &mut self.end, numeric, None);
                    ui.end_row();
                    ui.label("");
                    kit::note(ui, "Time found failed; empty = still running at the start time; equal = exact failure.");
                    ui.end_row();
                }
            }
            ui.label("Frequency (optional):");
            kit::column_choice(
                ui,
                "life_frequency",
                table,
                &mut self.frequency,
                numeric,
                Some("None: one unit per row"),
            );
            ui.end_row();
            ui.label("Group (optional):");
            kit::column_choice(ui, "life_group", table, &mut self.group, |_| true, Some("None"));
            ui.end_row();
        });
        ui.add_space(4.0);
        ui.label("Distributions to fit:");
        egui::Grid::new("life_families").num_columns(3).spacing([12.0, 2.0]).show(ui, |ui| {
            for (i, family) in FitFamily::ALL.iter().enumerate() {
                ui.checkbox(&mut self.families[i], family.label());
                if i % 3 == 2 {
                    ui.end_row();
                }
            }
        });
        let chosen: Vec<FitFamily> =
            FitFamily::ALL.iter().zip(&self.families).filter(|(_, on)| **on).map(|(f, _)| *f).collect();
        if self.detail.is_some_and(|d| !chosen.contains(&d)) {
            self.detail = None;
        }
        kit::more_options(ui, |ui| {
            egui::Grid::new("life_settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("Show in detail:");
                let label = |d: Option<FitFamily>| d.map_or("The best by AIC".to_owned(), |f| f.label().to_owned());
                egui::ComboBox::from_id_salt("life_detail")
                    .selected_text(label(self.detail))
                    .width(kit::combo_width(ui))
                    .show_list(ui, |ui| {
                        ui.selectable_value(&mut self.detail, None, label(None));
                        for &f in &chosen {
                            ui.selectable_value(&mut self.detail, Some(f), f.label());
                        }
                    });
                ui.end_row();
                ui.label("Percentiles (% failing):");
                ui.add(egui::TextEdit::singleline(&mut self.percentiles).desired_width(kit::combo_width(ui)));
                ui.end_row();
                ui.label("Failure probability at times:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.times)
                        .desired_width(kit::combo_width(ui))
                        .hint_text("e.g. 500, 1000"),
                );
                ui.end_row();
                ui.label("Few or no failures:");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.known_shape, "Weibull with known shape β =");
                    ui.add_enabled(
                        self.known_shape,
                        egui::DragValue::new(&mut self.shape).range(0.05..=50.0).speed(0.05),
                    );
                });
                ui.end_row();
                ui.label("Confidence level:");
                ui.add(egui::DragValue::new(&mut self.confidence).range(0.5..=0.999).speed(0.005).max_decimals(3));
                ui.end_row();
            });
        });
        let layout = match self.layout {
            Layout::Times => {
                let time = self.time.ok_or("Choose a time column")?;
                LifeDataLayout::Times { time, censor: self.censor, coding: self.coding }
            }
            Layout::Intervals => {
                let start = self.start.ok_or("Choose a start column")?;
                let end = self.end.ok_or("Choose an end column")?;
                LifeDataLayout::Intervals { start, end }
            }
        };
        if chosen.is_empty() {
            return Err("Choose at least one distribution".into());
        }
        let Some(percentiles) = parse_numbers(&self.percentiles).filter(|v| v.iter().all(|p| *p > 0.0 && *p < 100.0))
        else {
            return Err("Percentiles must be numbers between 0 and 100".into());
        };
        let Some(times) = parse_numbers(&self.times) else { return Err("Times must be numbers".into()) };
        let request = LifeDistributionRequest {
            frequency: self.frequency,
            group: self.group,
            families: chosen.clone(),
            detail: self.detail,
            confidence: self.confidence,
            probabilities: percentiles.iter().map(|p| p / 100.0).collect(),
            times,
            known_shape: self.known_shape.then_some(self.shape),
            ..LifeDistributionRequest::new(layout)
        };
        let cols = request.columns();
        if (1..cols.len()).any(|i| cols[..i].contains(&cols[i])) {
            return Err("A column is chosen twice".into());
        }
        let data = match layout {
            LifeDataLayout::Times { time, .. } => kit::name(table, time).to_owned(),
            LifeDataLayout::Intervals { start, end } => {
                format!("{}–{} intervals", kit::name(table, start), kit::name(table, end))
            }
        };
        let mut summary = format!("{} fitted to {data}", kit::count(chosen.len(), "distribution", "distributions"));
        if let Some(g) = self.group {
            summary.push_str(&format!(" by {}", kit::name(table, g)));
        }
        kit::ready(SavedReport::LifeDistribution { request }, summary)
    }
}
