//! Dialog for Describe ▸ Pharmacokinetic parameters…: long concentration–time
//! data (subject, time, concentration, optional treatment and dose) and the
//! non-compartmental settings.

use crate::dialog_kit::ComboExt as _;
use app_model::{Administration, AucMethod, LambdaZRule, PkDose, PkRequest, SavedReport};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Readiness};

/// How the dialog collects the dose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DoseMode {
    None,
    Value,
    Column,
}

/// Route of administration as a plain choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    Extravascular,
    Bolus,
    Infusion,
}

impl Route {
    const ALL: [Self; 3] = [Self::Extravascular, Self::Bolus, Self::Infusion];

    fn label(self) -> &'static str {
        match self {
            Self::Extravascular => "Extravascular (oral, subcutaneous…)",
            Self::Bolus => "Intravenous bolus",
            Self::Infusion => "Intravenous infusion",
        }
    }
}

/// Which points the terminal phase uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Terminal {
    BestFit,
    LastPoints,
    TimeRange,
}

/// Columns and settings of a non-compartmental analysis.
#[derive(Clone)]
pub struct PkDialog {
    subject: Option<usize>,
    time: Option<usize>,
    concentration: Option<usize>,
    treatment: Option<usize>,
    dose_mode: DoseMode,
    dose_value: f64,
    dose_column: Option<usize>,
    method: AucMethod,
    route: Route,
    duration: f64,
    terminal: Terminal,
    points: usize,
    range: (f64, f64),
}

impl Default for PkDialog {
    fn default() -> Self {
        Self {
            subject: None,
            time: None,
            concentration: None,
            treatment: None,
            dose_mode: DoseMode::None,
            dose_value: 100.0,
            dose_column: None,
            method: AucMethod::LinearUpLogDown,
            route: Route::Extravascular,
            duration: 1.0,
            terminal: Terminal::BestFit,
            points: 3,
            range: (0.0, 24.0),
        }
    }
}

impl PkDialog {
    /// The dialog filled from a saved analysis.
    pub fn from_saved(request: &PkRequest, table: &DataTable) -> Self {
        let valid = |c: usize| kit::valid(table, c);
        let mut d = Self {
            subject: valid(request.subject),
            time: valid(request.time),
            concentration: valid(request.concentration),
            treatment: request.treatment.and_then(valid),
            method: request.method,
            ..Self::default()
        };
        match request.dose {
            PkDose::None => d.dose_mode = DoseMode::None,
            PkDose::Value(v) => (d.dose_mode, d.dose_value) = (DoseMode::Value, v),
            PkDose::Column(c) => (d.dose_mode, d.dose_column) = (DoseMode::Column, valid(c)),
        }
        match request.administration {
            Administration::Extravascular => d.route = Route::Extravascular,
            Administration::IntravenousBolus => d.route = Route::Bolus,
            Administration::IntravenousInfusion { duration } => (d.route, d.duration) = (Route::Infusion, duration),
        }
        match request.lambda_rule {
            LambdaZRule::BestFit => d.terminal = Terminal::BestFit,
            LambdaZRule::LastPoints(k) => (d.terminal, d.points) = (Terminal::LastPoints, k),
            LambdaZRule::TimeRange(a, b) => (d.terminal, d.range) = (Terminal::TimeRange, (a, b)),
        }
        d
    }

    /// Selected columns fill subject, time and concentration in order (the
    /// first text or categorical one as subject).
    pub fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        let mut numeric = columns.iter().copied().filter(|&c| kit::is_continuous(table, c));
        self.subject = columns.iter().copied().find(|&c| !kit::is_continuous(table, c));
        self.time = numeric.next();
        self.concentration = numeric.next();
    }

    /// Draw the settings; ready when they are complete and valid.
    pub fn ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "Peak, areas under the curve, terminal half-life, clearance and volume of each concentration–time \
             profile, without a compartmental model, and their geometric means by treatment. One row per sample; \
             enter values below the quantification limit as 0.",
        );
        let numeric = |i: usize| kit::is_numeric(table, i);
        egui::Grid::new("pk_columns").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Subject:");
            kit::column_choice(ui, "pk_subject", table, &mut self.subject, |_| true, None);
            ui.end_row();
            ui.label("Time:");
            kit::column_choice(ui, "pk_time", table, &mut self.time, numeric, None);
            ui.end_row();
            ui.label("Concentration:");
            kit::column_choice(ui, "pk_concentration", table, &mut self.concentration, numeric, None);
            ui.end_row();
            ui.label("Treatment or period (optional):");
            kit::column_choice(
                ui,
                "pk_treatment",
                table,
                &mut self.treatment,
                |_| true,
                Some("None: one profile per subject"),
            );
            ui.end_row();
            ui.label("Dose:");
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.dose_mode, DoseMode::None, "None");
                ui.radio_value(&mut self.dose_mode, DoseMode::Value, "Same for all");
                ui.radio_value(&mut self.dose_mode, DoseMode::Column, "From a column");
            });
            ui.end_row();
            match self.dose_mode {
                DoseMode::None => {}
                DoseMode::Value => {
                    ui.label("Dose amount:");
                    ui.add(egui::DragValue::new(&mut self.dose_value).range(0.0..=f64::MAX).speed(1.0));
                    ui.end_row();
                }
                DoseMode::Column => {
                    ui.label("Dose column:");
                    kit::column_choice(ui, "pk_dose", table, &mut self.dose_column, numeric, None);
                    ui.end_row();
                }
            }
            ui.label("Administration:");
            egui::ComboBox::from_id_salt("pk_route")
                .selected_text(self.route.label())
                .width(kit::combo_width(ui))
                .show_list(ui, |ui| {
                    for r in Route::ALL {
                        ui.selectable_value(&mut self.route, r, r.label());
                    }
                });
            ui.end_row();
            if self.route == Route::Infusion {
                ui.label("Infusion duration:");
                ui.add(egui::DragValue::new(&mut self.duration).range(0.0..=f64::MAX).speed(0.1));
                ui.end_row();
            }
        });
        kit::more_options(ui, |ui| {
            egui::Grid::new("pk_settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("Area rule:");
                egui::ComboBox::from_id_salt("pk_method")
                    .selected_text(self.method.label())
                    .width(kit::combo_width(ui))
                    .show_list(ui, |ui| {
                        for m in AucMethod::ALL {
                            ui.selectable_value(&mut self.method, m, m.label());
                        }
                    });
                ui.end_row();
                ui.label("Terminal phase:");
                ui.vertical(|ui| {
                    ui.radio_value(&mut self.terminal, Terminal::BestFit, "Best fit (largest adjusted R²)");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.terminal, Terminal::LastPoints, "Last");
                        ui.add_enabled(
                            self.terminal == Terminal::LastPoints,
                            egui::DragValue::new(&mut self.points).range(3..=50),
                        );
                        ui.label("points");
                    });
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.terminal, Terminal::TimeRange, "Times from");
                        let on = self.terminal == Terminal::TimeRange;
                        ui.add_enabled(on, egui::DragValue::new(&mut self.range.0).speed(0.5));
                        ui.label("to");
                        ui.add_enabled(on, egui::DragValue::new(&mut self.range.1).speed(0.5));
                    });
                });
                ui.end_row();
            });
        });
        let subject = self.subject.ok_or("Choose a subject column")?;
        let time = self.time.ok_or("Choose a time column")?;
        let concentration = self.concentration.ok_or("Choose a concentration column")?;
        let dose = match self.dose_mode {
            DoseMode::None => PkDose::None,
            DoseMode::Value if self.dose_value > 0.0 => PkDose::Value(self.dose_value),
            DoseMode::Value => return Err("The dose must be positive".into()),
            DoseMode::Column => PkDose::Column(self.dose_column.ok_or("Choose a dose column")?),
        };
        let administration = match self.route {
            Route::Extravascular => Administration::Extravascular,
            Route::Bolus => Administration::IntravenousBolus,
            Route::Infusion if self.duration > 0.0 => Administration::IntravenousInfusion { duration: self.duration },
            Route::Infusion => return Err("The infusion duration must be positive".into()),
        };
        let lambda_rule = match self.terminal {
            Terminal::BestFit => LambdaZRule::BestFit,
            Terminal::LastPoints => LambdaZRule::LastPoints(self.points.max(3)),
            Terminal::TimeRange if self.range.0 < self.range.1 => LambdaZRule::TimeRange(self.range.0, self.range.1),
            Terminal::TimeRange => {
                return Err("The terminal time range must run from a smaller to a larger time".into());
            }
        };
        let request = PkRequest {
            treatment: self.treatment,
            dose,
            method: self.method,
            administration,
            lambda_rule,
            ..PkRequest::new(subject, time, concentration)
        };
        let cols = request.columns();
        if (1..cols.len()).any(|i| cols[..i].contains(&cols[i])) {
            return Err("A column is chosen twice".into());
        }
        let by = request.treatment.map_or_else(String::new, |t| format!(" and {}", kit::name(table, t)));
        let summary = format!(
            "Non-compartmental parameters of {} over {}, by {}{by}",
            kit::name(table, concentration),
            kit::name(table, time),
            kit::name(table, subject)
        );
        kit::ready(SavedReport::Pharmacokinetics { request }, summary)
    }
}
