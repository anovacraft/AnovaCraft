//! Dialog for Reliability ▸ Competing risks…: a time column, a cause (failure
//! mode) column with the value meaning "censored", optional group, frequency
//! and covariate columns, and the display and model settings.

use crate::dialog_kit::ComboExt as _;
use app_model::{CompetingRisksRequest, SavedReport};
use eframe::egui;
use stat_core::distribution_fit::FitFamily;
use stat_data::DataTable;
use stat_data::natural_order;

use crate::dialog_kit::{self as kit, Readiness};

/// Values that usually mean "still running".
const CENSOR_WORDS: [&str; 8] = ["0", "censored", "running", "survived", "alive", "none", "suspended", "in service"];

/// Columns and settings of a competing-risks analysis.
#[derive(Clone)]
pub struct CompetingRisksDialog {
    time: Option<usize>,
    cause: Option<usize>,
    /// The cause value meaning "censored" (`None`: nothing censored).
    censor: Option<String>,
    /// The cause column the censor value was guessed for.
    guessed_for: Option<usize>,
    group: Option<usize>,
    frequency: Option<usize>,
    covariates: Vec<bool>,
    regression_cause: Option<String>,
    profiles: String,
    stacked: bool,
    mode_fits: bool,
    family: FitFamily,
    times: String,
    confidence: f64,
}

impl Default for CompetingRisksDialog {
    fn default() -> Self {
        Self {
            time: None,
            cause: None,
            censor: None,
            guessed_for: None,
            group: None,
            frequency: None,
            covariates: Vec::new(),
            regression_cause: None,
            profiles: String::new(),
            stacked: false,
            mode_fits: false,
            family: FitFamily::Weibull,
            times: String::new(),
            confidence: CompetingRisksRequest::DEFAULT_CONFIDENCE,
        }
    }
}

/// The distinct non-empty values of a column, in natural order (at most 60).
fn distinct_values(table: &DataTable, column: usize) -> Vec<String> {
    let Some(c) = table.column(column) else { return Vec::new() };
    let mut values: Vec<String> =
        (0..table.n_rows()).map(|r| c.level_text(r).trim().to_owned()).filter(|v| !v.is_empty()).collect();
    values.sort_by(|a, b| natural_order(a, b));
    values.dedup();
    values.truncate(60);
    values
}

fn parse_numbers(text: &str) -> Option<Vec<f64>> {
    text.split([',', ';', ' '])
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().parse::<f64>().ok().filter(|v| v.is_finite()))
        .collect()
}

impl CompetingRisksDialog {
    /// The dialog filled from a saved request.
    pub fn from_request(r: &CompetingRisksRequest, table: &DataTable) -> Self {
        let valid = |c: usize| kit::valid(table, c);
        Self {
            time: valid(r.time),
            cause: valid(r.cause),
            censor: r.censor_value.clone(),
            guessed_for: valid(r.cause),
            group: r.group.and_then(valid),
            frequency: r.frequency.and_then(valid),
            covariates: kit::mask(table.n_cols(), &r.covariates),
            regression_cause: r.regression_cause.clone(),
            profiles: r.profiles.iter().map(|p| p.join(", ")).collect::<Vec<_>>().join("\n"),
            stacked: r.stacked,
            mode_fits: r.mode_family.is_some(),
            family: r.mode_family.unwrap_or(FitFamily::Weibull),
            times: r.times.iter().map(|t| app_model::format::format_number(*t, 10)).collect::<Vec<_>>().join(", "),
            confidence: r.confidence,
        }
    }

    /// Put the first numeric selected column in the time role and the next
    /// selected column in the cause role.
    pub fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.time = columns.iter().copied().find(|&c| kit::is_numeric(table, c));
        self.cause = columns.iter().copied().find(|&c| Some(c) != self.time);
    }

    /// Draw the settings; ready when they are complete and valid.
    pub fn ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "Units that fail from one of several causes (failure modes), or are still running: the cumulative \
             incidence of each cause, Gray's test across groups, subdistribution hazards regression, and a life \
             distribution per mode combined into the system's.",
        );
        let numeric = |i: usize| kit::is_numeric(table, i);
        if self.cause != self.guessed_for {
            self.guessed_for = self.cause;
            self.censor = self.cause.and_then(|c| {
                distinct_values(table, c).into_iter().find(|v| CENSOR_WORDS.contains(&v.to_lowercase().as_str()))
            });
            self.regression_cause = None;
        }
        let values = self.cause.map(|c| distinct_values(table, c)).unwrap_or_default();
        egui::Grid::new("competing_columns").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Time:");
            kit::column_choice(ui, "competing_time", table, &mut self.time, numeric, None);
            ui.end_row();
            ui.label("Cause or failure mode:");
            kit::column_choice(ui, "competing_cause", table, &mut self.cause, |_| true, None);
            ui.end_row();
            ui.label("Censored value:");
            let label = |v: &Option<String>| v.clone().unwrap_or_else(|| "None: every row failed".to_owned());
            egui::ComboBox::from_id_salt("competing_censor")
                .selected_text(label(&self.censor))
                .width(kit::combo_width(ui))
                .show_list(ui, |ui| {
                    ui.selectable_value(&mut self.censor, None, label(&None));
                    for v in &values {
                        ui.selectable_value(&mut self.censor, Some(v.clone()), v);
                    }
                });
            ui.end_row();
            ui.label("");
            kit::note(ui, "Rows with this value were still running at their time; every other value is a cause.");
            ui.end_row();
            ui.label("Group (optional):");
            kit::column_choice(ui, "competing_group", table, &mut self.group, |_| true, Some("None"));
            ui.end_row();
            ui.label("Frequency (optional):");
            kit::column_choice(
                ui,
                "competing_frequency",
                table,
                &mut self.frequency,
                numeric,
                Some("None: one unit per row"),
            );
            ui.end_row();
        });
        ui.add_space(4.0);
        ui.checkbox(&mut self.stacked, "Stack the incidence curves (each band between curves is one cause)");
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.mode_fits, "Fit a life distribution to each failure mode:");
            ui.add_enabled_ui(self.mode_fits, |ui| {
                egui::ComboBox::from_id_salt("competing_family").selected_text(self.family.label()).show_list(
                    ui,
                    |ui| {
                        for f in FitFamily::ALL.iter().filter(|f| !f.has_threshold() && f.needs_positive()) {
                            ui.selectable_value(&mut self.family, *f, f.label());
                        }
                    },
                );
            });
        });
        let causes: Vec<String> = values.iter().filter(|v| Some(*v) != self.censor.as_ref()).cloned().collect();
        kit::more_options(ui, |ui| {
            ui.label("Covariates for subdistribution hazards regression (optional):");
            let exclude = [self.time, self.cause, self.frequency];
            kit::column_list(ui, "competing_covariates", table, &mut self.covariates, |i| !exclude.contains(&Some(i)));
            egui::Grid::new("competing_settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("Cause modelled:");
                let label = |v: &Option<String>| {
                    v.clone().unwrap_or_else(|| format!("{} (first)", causes.first().cloned().unwrap_or_default()))
                };
                egui::ComboBox::from_id_salt("competing_regression_cause")
                    .selected_text(label(&self.regression_cause))
                    .width(kit::combo_width(ui))
                    .show_list(ui, |ui| {
                        ui.selectable_value(&mut self.regression_cause, None, label(&None));
                        for c in &causes {
                            ui.selectable_value(&mut self.regression_cause, Some(c.clone()), c);
                        }
                    });
                ui.end_row();
                ui.label("Predict at:");
                ui.add(
                    egui::TextEdit::multiline(&mut self.profiles)
                        .desired_rows(2)
                        .desired_width(kit::combo_width(ui))
                        .hint_text("One setting per line, values in covariate order, e.g. 10, North"),
                );
                ui.end_row();
                ui.label("System probability at times:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.times)
                        .desired_width(kit::combo_width(ui))
                        .hint_text("Empty: the system's 1%, 10%, 50% points"),
                );
                ui.end_row();
                ui.label("Confidence level:");
                ui.add(egui::DragValue::new(&mut self.confidence).range(0.5..=0.999).speed(0.005).max_decimals(3));
                ui.end_row();
            });
        });
        let time = self.time.ok_or("Choose a time column")?;
        let cause = self.cause.ok_or("Choose the cause or failure-mode column")?;
        if causes.is_empty() {
            return Err("The cause column has no failure values".into());
        }
        let Some(times) = parse_numbers(&self.times) else { return Err("Times must be numbers".into()) };
        let covariates = kit::chosen_columns(&self.covariates);
        let profiles: Vec<Vec<String>> = self
            .profiles
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.split([',', ';']).map(|v| v.trim().to_owned()).collect())
            .collect();
        if profiles.iter().any(|p| p.len() != covariates.len()) {
            return Err(format!("Each prediction setting needs {} values, one per covariate", covariates.len()));
        }
        let request = CompetingRisksRequest {
            censor_value: self.censor.clone(),
            group: self.group,
            frequency: self.frequency,
            covariates,
            regression_cause: self.regression_cause.clone(),
            profiles,
            stacked: self.stacked,
            mode_family: self.mode_fits.then_some(self.family),
            times,
            confidence: self.confidence,
            ..CompetingRisksRequest::new(time, cause)
        };
        if request.has_role_conflict() {
            return Err("Choose a different column for each role".into());
        }
        let mut summary = format!(
            "Cumulative incidence of {} causes of {} in {}",
            causes.len(),
            kit::name(table, time),
            kit::name(table, cause)
        );
        if let Some(g) = self.group {
            summary.push_str(&format!(" by {}", kit::name(table, g)));
        }
        if !request.covariates.is_empty() {
            summary.push_str(&format!(
                ", regression on {}",
                kit::count(request.covariates.len(), "covariate", "covariates")
            ));
        }
        kit::ready(SavedReport::CompetingRisks { request }, summary)
    }
}
