//! Dialog for Reliability ▸ Fatigue (stress–life) curves…: cycles to failure
//! or run-out at each stress amplitude, the S–N curve form and the scatter
//! distribution.

use crate::dialog_kit::ComboExt as _;
use crate::life_stress_dialog::{numbers_text, parse_numbers};
use app_model::{FatigueRequest, RegressionFamily, SavedReport, SnForm};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Readiness};

/// Scatter distributions offered (log-life families).
const FAMILIES: [RegressionFamily; 3] =
    [RegressionFamily::Lognormal, RegressionFamily::Weibull, RegressionFamily::LogLogistic];

/// Columns and settings of a fatigue-curve analysis.
#[derive(Clone)]
pub struct FatigueDialog {
    cycles: Option<usize>,
    runout: Option<usize>,
    stress: Option<usize>,
    frequency: Option<usize>,
    form: SnForm,
    family: RegressionFamily,
    stress_dependent_scale: bool,
    percentiles: String,
    lives: String,
    stresses: String,
    confidence: f64,
}

impl Default for FatigueDialog {
    fn default() -> Self {
        Self {
            cycles: None,
            runout: None,
            stress: None,
            frequency: None,
            form: SnForm::Basquin,
            family: RegressionFamily::Lognormal,
            stress_dependent_scale: false,
            percentiles: numbers_text(FatigueRequest::DEFAULT_PROBABILITIES.map(|p| p * 100.0)),
            lives: numbers_text(FatigueRequest::DEFAULT_LIVES),
            stresses: String::new(),
            confidence: FatigueRequest::DEFAULT_CONFIDENCE,
        }
    }
}

impl FatigueDialog {
    /// The dialog filled from a saved analysis.
    pub fn from_saved(request: &FatigueRequest, table: &DataTable) -> Self {
        let valid = |c: usize| kit::valid(table, c);
        Self {
            cycles: valid(request.cycles),
            runout: request.runout.and_then(valid),
            stress: valid(request.stress),
            frequency: request.frequency.and_then(valid),
            form: request.form,
            family: request.family,
            stress_dependent_scale: request.stress_dependent_scale,
            percentiles: numbers_text(request.probabilities.iter().map(|p| p * 100.0)),
            lives: numbers_text(request.lives.iter().copied()),
            stresses: numbers_text(request.stresses.iter().copied()),
            confidence: request.confidence,
        }
    }

    /// The first two numeric selected columns become cycles and stress.
    pub fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        let mut numeric = columns.iter().copied().filter(|&c| kit::is_numeric(table, c));
        self.cycles = numeric.next();
        self.stress = numeric.next();
    }

    /// Draw the settings; ready when they are complete and valid.
    pub fn ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "Cycles to failure against stress amplitude: an S–N curve fitted by maximum likelihood with \
             unbroken specimens (run-outs) as censored, quantile curves such as B10 with a lower bound, and \
             the fatigue strength at chosen lives.",
        );
        let numeric = |i: usize| kit::is_numeric(table, i);
        egui::Grid::new("fatigue_columns").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Cycles:");
            kit::column_choice(ui, "fatigue_cycles", table, &mut self.cycles, numeric, None);
            ui.end_row();
            ui.label("Run-out (optional):");
            kit::column_choice(
                ui,
                "fatigue_runout",
                table,
                &mut self.runout,
                numeric,
                Some("None: every specimen failed"),
            );
            ui.end_row();
            ui.label("");
            kit::note(ui, "Non-zero = the specimen did not fail (stopped at its cycles).");
            ui.end_row();
            ui.label("Stress:");
            kit::column_choice(ui, "fatigue_stress", table, &mut self.stress, numeric, None);
            ui.end_row();
            ui.label("Frequency (optional):");
            kit::column_choice(
                ui,
                "fatigue_frequency",
                table,
                &mut self.frequency,
                numeric,
                Some("None: one specimen per row"),
            );
            ui.end_row();
            ui.label("Curve:");
            egui::ComboBox::from_id_salt("fatigue_form")
                .selected_text(self.form.label())
                .width(kit::combo_width(ui))
                .show_list(ui, |ui| {
                    for f in SnForm::ALL {
                        ui.selectable_value(&mut self.form, f, f.label()).on_hover_text(f.formula());
                    }
                });
            ui.end_row();
            ui.label("Life distribution:");
            egui::ComboBox::from_id_salt("fatigue_family")
                .selected_text(self.family.label())
                .width(kit::combo_width(ui))
                .show_list(ui, |ui| {
                    for f in FAMILIES {
                        ui.selectable_value(&mut self.family, f, f.label());
                    }
                });
            ui.end_row();
        });
        kit::more_options(ui, |ui| {
            ui.checkbox(&mut self.stress_dependent_scale, "Scatter depends on stress (ln σ linear in ln S)");
            egui::Grid::new("fatigue_settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("Quantile curves (% failing):");
                ui.add(egui::TextEdit::singleline(&mut self.percentiles).desired_width(kit::combo_width(ui)));
                ui.end_row();
                ui.label("Fatigue strength at lives (cycles):");
                ui.add(egui::TextEdit::singleline(&mut self.lives).desired_width(kit::combo_width(ui)));
                ui.end_row();
                ui.label("Life quantiles at stresses:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.stresses)
                        .desired_width(kit::combo_width(ui))
                        .hint_text("Empty: the tested stresses"),
                );
                ui.end_row();
                ui.label("Confidence level:");
                ui.add(egui::DragValue::new(&mut self.confidence).range(0.5..=0.999).speed(0.005).max_decimals(3));
                ui.end_row();
            });
        });
        let cycles = self.cycles.ok_or("Choose a cycles column")?;
        let stress = self.stress.ok_or("Choose a stress column")?;
        let Some(percentiles) = parse_numbers(&self.percentiles).filter(|v| v.iter().all(|p| *p > 0.0 && *p < 100.0))
        else {
            return Err("Percentages failing must be numbers between 0 and 100".into());
        };
        let Some(lives) = parse_numbers(&self.lives).filter(|v| v.iter().all(|n| *n > 0.0)) else {
            return Err("Lives must be positive numbers".into());
        };
        let Some(stresses) = parse_numbers(&self.stresses).filter(|v| v.iter().all(|s| *s > 0.0)) else {
            return Err("Stresses must be positive numbers".into());
        };
        let request = FatigueRequest {
            frequency: self.frequency,
            form: self.form,
            family: self.family,
            stress_dependent_scale: self.stress_dependent_scale,
            confidence: self.confidence,
            probabilities: percentiles.iter().map(|p| p / 100.0).collect(),
            lives,
            stresses,
            ..FatigueRequest::new(cycles, self.runout, stress)
        };
        let cols = request.columns();
        if (1..cols.len()).any(|i| cols[..i].contains(&cols[i])) {
            return Err("A column is chosen twice".into());
        }
        let summary = format!(
            "{} S–N curve of {} by {}, {} scatter",
            self.form.label(),
            kit::name(table, cycles),
            kit::name(table, stress),
            self.family.label().to_lowercase()
        );
        kit::ready(SavedReport::Fatigue { request }, summary)
    }
}
