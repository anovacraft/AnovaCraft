//! Dialog for Reliability ▸ Life–stress models…: censored failure times
//! (time and censor columns, or failure intervals) with one or two stress
//! columns, each with its relationship and use condition.

use crate::dialog_kit::ComboExt as _;
use app_model::{
    LifeDataLayout, LifeStressRequest, RegressionFamily, SavedReport, StatusCoding, StressColumn, StressTransform,
    TemperatureUnit,
};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Readiness};

/// Which data layout the dialog collects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    Times,
    Intervals,
}

/// One stress row of the dialog.
#[derive(Clone)]
struct StressRow {
    column: Option<usize>,
    transform: StressTransform,
    unit: TemperatureUnit,
    use_value: f64,
}

impl StressRow {
    fn new(transform: StressTransform, use_value: f64) -> Self {
        Self { column: None, transform, unit: TemperatureUnit::Celsius, use_value }
    }
}

/// Columns and settings of a life–stress analysis.
#[derive(Clone)]
pub struct LifeStressDialog {
    layout: Layout,
    time: Option<usize>,
    censor: Option<usize>,
    coding: StatusCoding,
    start: Option<usize>,
    end: Option<usize>,
    frequency: Option<usize>,
    stresses: [StressRow; 2],
    two_stresses: bool,
    interaction: bool,
    family: RegressionFamily,
    stress_dependent_scale: bool,
    percentiles: String,
    times: String,
    confidence: f64,
}

impl Default for LifeStressDialog {
    fn default() -> Self {
        Self {
            layout: Layout::Times,
            time: None,
            censor: None,
            coding: StatusCoding::NonzeroIsCensored,
            start: None,
            end: None,
            frequency: None,
            stresses: [
                StressRow::new(StressTransform::Arrhenius, 25.0),
                StressRow::new(StressTransform::InversePower, 1.0),
            ],
            two_stresses: false,
            interaction: false,
            family: RegressionFamily::Weibull,
            stress_dependent_scale: false,
            percentiles: "1, 10, 50".to_owned(),
            times: String::new(),
            confidence: LifeStressRequest::DEFAULT_CONFIDENCE,
        }
    }
}

/// Numbers separated by commas, semicolons or spaces; `None` if any is not a number.
pub(crate) fn parse_numbers(text: &str) -> Option<Vec<f64>> {
    text.split([',', ';', ' '])
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().parse::<f64>().ok().filter(|v| v.is_finite()))
        .collect()
}

/// Numbers as text, separated by commas ("1, 10, 50").
pub(crate) fn numbers_text(values: impl IntoIterator<Item = f64>) -> String {
    values.into_iter().map(|v| app_model::format::format_number(v, 10)).collect::<Vec<_>>().join(", ")
}

impl LifeStressDialog {
    /// The dialog filled from a saved life–stress analysis.
    pub fn from_saved(request: &LifeStressRequest, table: &DataTable) -> Self {
        let valid = |c: usize| kit::valid(table, c);
        let mut d = Self {
            frequency: request.frequency.and_then(valid),
            two_stresses: request.stresses.len() > 1,
            interaction: request.interaction,
            family: request.family,
            stress_dependent_scale: request.stress_dependent_scale,
            percentiles: numbers_text(request.probabilities.iter().map(|p| p * 100.0)),
            times: numbers_text(request.times.iter().copied()),
            confidence: request.confidence,
            ..Self::default()
        };
        for (k, s) in request.stresses.iter().take(2).enumerate() {
            d.stresses[k] = StressRow {
                column: valid(s.column),
                transform: s.transform,
                unit: s.unit,
                use_value: request.use_stress.get(k).copied().unwrap_or(d.stresses[k].use_value),
            };
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

    /// Put the first two numeric selected columns in the time and stress roles.
    pub fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        let mut numeric = columns.iter().copied().filter(|&c| kit::is_numeric(table, c));
        self.time = numeric.next();
        self.stresses[0].column = numeric.next();
    }

    fn stress_ui(ui: &mut egui::Ui, k: usize, row: &mut StressRow, table: &DataTable) {
        let numeric = |i: usize| kit::is_numeric(table, i);
        let which = if k == 0 { "Stress" } else { "Second stress" };
        ui.label(format!("{which}:"));
        kit::column_choice(ui, format!("ls_stress_{k}"), table, &mut row.column, numeric, None);
        ui.end_row();
        ui.label("Relationship:");
        egui::ComboBox::from_id_salt(format!("ls_transform_{k}"))
            .selected_text(row.transform.label())
            .width(kit::combo_width(ui))
            .show_list(ui, |ui| {
                for t in StressTransform::ALL {
                    ui.selectable_value(&mut row.transform, t, t.label());
                }
            });
        ui.end_row();
        if row.transform.is_temperature() {
            ui.label("Temperature in:");
            ui.horizontal(|ui| {
                ui.radio_value(&mut row.unit, TemperatureUnit::Celsius, "°C");
                ui.radio_value(&mut row.unit, TemperatureUnit::Kelvin, "K");
            });
            ui.end_row();
        }
        ui.label("Use condition:");
        ui.add(egui::DragValue::new(&mut row.use_value).speed(0.5).max_decimals(4));
        ui.end_row();
    }

    /// Draw the settings; ready when they are complete and valid.
    pub fn ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "A life distribution whose location (log life) depends on one or two stresses, fitted by maximum \
             likelihood to failure times with right, left or interval censoring; results are extrapolated to the \
             use conditions.",
        );
        let numeric = |i: usize| kit::is_numeric(table, i);
        ui.horizontal(|ui| {
            ui.label("Data:");
            ui.radio_value(&mut self.layout, Layout::Times, "Time and censor columns");
            ui.radio_value(&mut self.layout, Layout::Intervals, "Start and end of the failure interval");
        });
        egui::Grid::new("ls_columns").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            match self.layout {
                Layout::Times => {
                    ui.label("Time:");
                    kit::column_choice(ui, "ls_time", table, &mut self.time, numeric, None);
                    ui.end_row();
                    ui.label("Censor (optional):");
                    kit::column_choice(
                        ui,
                        "ls_censor",
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
                    kit::column_choice(ui, "ls_start", table, &mut self.start, numeric, None);
                    ui.end_row();
                    ui.label("End:");
                    kit::column_choice(ui, "ls_end", table, &mut self.end, numeric, None);
                    ui.end_row();
                    ui.label("");
                    kit::note(ui, "Empty start = failed before the first look; empty end = still running.");
                    ui.end_row();
                }
            }
            ui.label("Frequency (optional):");
            kit::column_choice(ui, "ls_frequency", table, &mut self.frequency, numeric, Some("None: one unit per row"));
            ui.end_row();
            Self::stress_ui(ui, 0, &mut self.stresses[0], table);
        });
        ui.checkbox(&mut self.two_stresses, "Add a second stress");
        if self.two_stresses {
            egui::Grid::new("ls_second").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                Self::stress_ui(ui, 1, &mut self.stresses[1], table);
            });
            ui.checkbox(&mut self.interaction, "Include the interaction of the two stresses (generalized Eyring)");
        }
        egui::Grid::new("ls_family").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Life distribution:");
            egui::ComboBox::from_id_salt("ls_family_choice")
                .selected_text(self.family.label())
                .width(kit::combo_width(ui))
                .show_list(ui, |ui| {
                    for f in RegressionFamily::LIFE {
                        ui.selectable_value(&mut self.family, f, f.label());
                    }
                });
            ui.end_row();
        });
        kit::more_options(ui, |ui| {
            ui.add_enabled(
                !self.family.fixed_scale(),
                egui::Checkbox::new(
                    &mut self.stress_dependent_scale,
                    "Scale depends on the stresses (tests constant shape)",
                ),
            );
            egui::Grid::new("ls_settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("Percentiles at use (% failing):");
                ui.add(egui::TextEdit::singleline(&mut self.percentiles).desired_width(kit::combo_width(ui)));
                ui.end_row();
                ui.label("Failure probability at use, at times:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.times)
                        .desired_width(kit::combo_width(ui))
                        .hint_text("e.g. 10000, 50000"),
                );
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
        let used = if self.two_stresses { 2 } else { 1 };
        let mut stresses = Vec::new();
        let mut use_stress = Vec::new();
        for row in &self.stresses[..used] {
            let column = row.column.ok_or("Choose a stress column")?;
            let s = StressColumn { column, transform: row.transform, unit: row.unit };
            if !s.variable().valid(row.use_value) {
                return Err(match row.transform {
                    StressTransform::InversePower => "Inverse-power stresses must be positive".into(),
                    _ => "The use temperature must be above absolute zero".into(),
                });
            }
            stresses.push(s);
            use_stress.push(row.use_value);
        }
        let Some(percentiles) = parse_numbers(&self.percentiles).filter(|v| v.iter().all(|p| *p > 0.0 && *p < 100.0))
        else {
            return Err("Percentiles must be numbers between 0 and 100".into());
        };
        let Some(times) = parse_numbers(&self.times).filter(|v| v.iter().all(|t| *t > 0.0)) else {
            return Err("Times must be positive numbers".into());
        };
        let request = LifeStressRequest {
            layout,
            frequency: self.frequency,
            stresses,
            interaction: self.two_stresses && self.interaction,
            family: self.family,
            stress_dependent_scale: self.stress_dependent_scale && !self.family.fixed_scale(),
            use_stress,
            confidence: self.confidence,
            probabilities: percentiles.iter().map(|p| p / 100.0).collect(),
            times,
        };
        let cols = request.columns();
        if (1..cols.len()).any(|i| cols[..i].contains(&cols[i])) {
            return Err("A column is chosen twice".into());
        }
        let names: Vec<&str> = request.stresses.iter().map(|s| kit::name(table, s.column)).collect();
        let summary = format!(
            "{}–{} model of {} by {}",
            self.family.label(),
            request.stresses.iter().map(|s| s.transform.label()).collect::<Vec<_>>().join(" + "),
            match request.layout {
                LifeDataLayout::Times { time, .. } => kit::name(table, time).to_owned(),
                LifeDataLayout::Intervals { start, end } => {
                    format!("{}–{} intervals", kit::name(table, start), kit::name(table, end))
                }
            },
            names.join(" and ")
        );
        kit::ready(SavedReport::LifeStress { request }, summary)
    }
}
