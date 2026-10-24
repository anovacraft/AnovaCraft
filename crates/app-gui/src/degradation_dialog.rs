//! Dialog for Reliability ▸ Degradation…: repeated measurements of units
//! (unit, time and response columns) or destructive measurements (time and
//! response), a path model, the failure threshold, and how life information
//! is drawn from the paths.

use crate::dialog_kit::ComboExt as _;
use app_model::{
    DegradationLayout, DegradationMethod, DegradationRequest, FailureDirection, PathModel, RegressionFamily,
    SavedReport, ScaleTransform, StressColumn, StressTransform, TemperatureUnit,
};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Readiness};

/// Which data layout the dialog collects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    Repeated,
    Destructive,
}

/// The path form, without the linear form's transforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathForm {
    Linear,
    Asymptotic,
    Power,
}

impl PathForm {
    fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear (on transformed scales)",
            Self::Asymptotic => PathModel::Asymptotic.label(),
            Self::Power => PathModel::Power.label(),
        }
    }
}

/// Columns and settings of a degradation analysis.
#[derive(Clone)]
pub struct DegradationDialog {
    layout: Layout,
    unit: Option<usize>,
    time: Option<usize>,
    response: Option<usize>,
    form: PathForm,
    response_transform: ScaleTransform,
    time_transform: ScaleTransform,
    threshold: f64,
    direction: FailureDirection,
    method: DegradationMethod,
    life_family: RegressionFamily,
    response_family: RegressionFamily,
    use_stress_column: bool,
    stress: Option<usize>,
    transform: StressTransform,
    unit_of_temperature: TemperatureUnit,
    use_stress: f64,
    percentiles: String,
    times: String,
    bootstrap: usize,
    confidence: f64,
}

impl Default for DegradationDialog {
    fn default() -> Self {
        Self {
            layout: Layout::Repeated,
            unit: None,
            time: None,
            response: None,
            form: PathForm::Linear,
            response_transform: ScaleTransform::Identity,
            time_transform: ScaleTransform::Identity,
            threshold: 10.0,
            direction: FailureDirection::Rising,
            method: DegradationMethod::PseudoFailureTimes,
            life_family: RegressionFamily::Weibull,
            response_family: RegressionFamily::Normal,
            use_stress_column: false,
            stress: None,
            transform: StressTransform::Arrhenius,
            unit_of_temperature: TemperatureUnit::Celsius,
            use_stress: 25.0,
            percentiles: "10, 50, 90".to_owned(),
            times: String::new(),
            bootstrap: DegradationRequest::DEFAULT_BOOTSTRAP,
            confidence: DegradationRequest::DEFAULT_CONFIDENCE,
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

/// Numbers as text, separated by commas ("10, 50, 90").
fn numbers_text(values: impl IntoIterator<Item = f64>) -> String {
    values.into_iter().map(|v| app_model::format::format_number(v, 10)).collect::<Vec<_>>().join(", ")
}

fn transform_choice(ui: &mut egui::Ui, id: &str, value: &mut ScaleTransform) {
    egui::ComboBox::from_id_salt(id).selected_text(value.label()).width(kit::combo_width(ui)).show_list(ui, |ui| {
        for t in ScaleTransform::ALL {
            ui.selectable_value(value, t, t.label());
        }
    });
}

fn family_choice(ui: &mut egui::Ui, id: &str, value: &mut RegressionFamily, families: &[RegressionFamily]) {
    egui::ComboBox::from_id_salt(id).selected_text(value.label()).width(kit::combo_width(ui)).show_list(ui, |ui| {
        for &f in families {
            ui.selectable_value(value, f, f.label());
        }
    });
}

impl DegradationDialog {
    /// The dialog filled from a saved degradation analysis.
    pub fn from_saved(request: &DegradationRequest, table: &DataTable) -> Self {
        let valid = |c: usize| kit::valid(table, c);
        let mut d = Self {
            threshold: request.threshold,
            direction: request.direction,
            method: request.method,
            life_family: request.life_family,
            response_family: request.response_family,
            percentiles: numbers_text(request.probabilities.iter().map(|p| p * 100.0)),
            times: numbers_text(request.times.iter().copied()),
            bootstrap: request.bootstrap,
            confidence: request.confidence,
            ..Self::default()
        };
        match request.path {
            PathModel::Linear { response, time } => {
                d.form = PathForm::Linear;
                d.response_transform = response;
                d.time_transform = time;
            }
            PathModel::Asymptotic => d.form = PathForm::Asymptotic,
            PathModel::Power => d.form = PathForm::Power,
        }
        if let Some(s) = request.stress {
            d.use_stress_column = true;
            d.stress = valid(s.column);
            d.transform = s.transform;
            d.unit_of_temperature = s.unit;
        }
        if let Some(u) = request.use_stress {
            d.use_stress = u;
        }
        match request.layout {
            DegradationLayout::RepeatedMeasures { unit, time, response } => {
                d.layout = Layout::Repeated;
                d.unit = valid(unit);
                d.time = valid(time);
                d.response = valid(response);
            }
            DegradationLayout::Destructive { time, response } => {
                d.layout = Layout::Destructive;
                d.time = valid(time);
                d.response = valid(response);
            }
        }
        d
    }

    /// Selected columns fill the unit, time and response roles in order.
    pub fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        let first = columns.first().copied();
        if first.is_some_and(|c| !kit::is_numeric(table, c)) {
            self.unit = first;
        }
        let mut numeric = columns.iter().copied().filter(|&c| kit::is_numeric(table, c));
        self.time = numeric.next();
        self.response = numeric.next();
    }

    /// Draw the settings; ready when they are complete and valid.
    pub fn ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "Life information from a measured characteristic that drifts towards a failure threshold: each unit's \
             path gives a pseudo failure time, or a random-effects path model gives the distribution of the time \
             to reach the threshold; destructive tests measure each unit once.",
        );
        let numeric = |i: usize| kit::is_numeric(table, i);
        ui.horizontal(|ui| {
            ui.label("Data:");
            ui.radio_value(&mut self.layout, Layout::Repeated, "Repeated measurements of each unit");
            ui.radio_value(&mut self.layout, Layout::Destructive, "Destructive: one measurement per unit");
        });
        if self.layout == Layout::Destructive {
            self.form = PathForm::Linear;
        }
        egui::Grid::new("deg_columns").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            if self.layout == Layout::Repeated {
                ui.label("Unit:");
                kit::column_choice(ui, "deg_unit", table, &mut self.unit, |_| true, None);
                ui.end_row();
            }
            ui.label("Time:");
            kit::column_choice(ui, "deg_time", table, &mut self.time, numeric, None);
            ui.end_row();
            ui.label("Response:");
            kit::column_choice(ui, "deg_response", table, &mut self.response, numeric, None);
            ui.end_row();
            ui.label("Failure threshold:");
            ui.add(egui::DragValue::new(&mut self.threshold).speed(0.1).max_decimals(6));
            ui.end_row();
            ui.label("Failure when the response:");
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.direction, FailureDirection::Rising, "Rises to the threshold");
                ui.radio_value(&mut self.direction, FailureDirection::Falling, "Falls to the threshold");
            });
            ui.end_row();
            if self.layout == Layout::Repeated {
                ui.label("Path model:");
                egui::ComboBox::from_id_salt("deg_form")
                    .selected_text(self.form.label())
                    .width(kit::combo_width(ui))
                    .show_list(ui, |ui| {
                        for f in [PathForm::Linear, PathForm::Asymptotic, PathForm::Power] {
                            ui.selectable_value(&mut self.form, f, f.label());
                        }
                    });
                ui.end_row();
            }
            if self.form == PathForm::Linear {
                ui.label("Response scale:");
                transform_choice(ui, "deg_response_transform", &mut self.response_transform);
                ui.end_row();
                ui.label("Time scale:");
                transform_choice(ui, "deg_time_transform", &mut self.time_transform);
                ui.end_row();
            }
            match self.layout {
                Layout::Repeated => {
                    ui.label("Method:");
                    ui.vertical(|ui| {
                        ui.radio_value(
                            &mut self.method,
                            DegradationMethod::PseudoFailureTimes,
                            "Pseudo failure times, then a life distribution",
                        );
                        ui.add_enabled_ui(self.form == PathForm::Linear && !self.use_stress_column, |ui| {
                            ui.radio_value(
                                &mut self.method,
                                DegradationMethod::RandomEffects,
                                "Random-effects paths (implied life distribution)",
                            );
                        });
                    });
                    ui.end_row();
                    if self.method == DegradationMethod::PseudoFailureTimes {
                        ui.label("Life distribution:");
                        family_choice(ui, "deg_life_family", &mut self.life_family, &RegressionFamily::LIFE);
                        ui.end_row();
                    }
                }
                Layout::Destructive => {
                    ui.label("Response distribution:");
                    family_choice(ui, "deg_response_family", &mut self.response_family, &RegressionFamily::REAL_LINE);
                    ui.end_row();
                }
            }
        });
        let random = self.layout == Layout::Repeated && self.method == DegradationMethod::RandomEffects;
        if !random {
            ui.checkbox(&mut self.use_stress_column, "Accelerating stress (accelerated degradation)");
            if self.use_stress_column {
                egui::Grid::new("deg_stress").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label("Stress:");
                    kit::column_choice(ui, "deg_stress_column", table, &mut self.stress, numeric, None);
                    ui.end_row();
                    ui.label("Relationship:");
                    egui::ComboBox::from_id_salt("deg_stress_transform")
                        .selected_text(self.transform.label())
                        .width(kit::combo_width(ui))
                        .show_list(ui, |ui| {
                            for t in StressTransform::ALL {
                                ui.selectable_value(&mut self.transform, t, t.label());
                            }
                        });
                    ui.end_row();
                    if self.transform.is_temperature() {
                        ui.label("Temperature in:");
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut self.unit_of_temperature, TemperatureUnit::Celsius, "°C");
                            ui.radio_value(&mut self.unit_of_temperature, TemperatureUnit::Kelvin, "K");
                        });
                        ui.end_row();
                    }
                    ui.label("Use condition:");
                    ui.add(egui::DragValue::new(&mut self.use_stress).speed(0.5).max_decimals(4));
                    ui.end_row();
                });
            }
        }
        kit::more_options(ui, |ui| {
            egui::Grid::new("deg_settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("Percentiles (% failing):");
                ui.add(egui::TextEdit::singleline(&mut self.percentiles).desired_width(kit::combo_width(ui)));
                ui.end_row();
                ui.label("Failure probability at times:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.times)
                        .desired_width(kit::combo_width(ui))
                        .hint_text("e.g. 4000, 8000"),
                );
                ui.end_row();
                if random {
                    ui.label("Bootstrap replicates:");
                    ui.add(egui::DragValue::new(&mut self.bootstrap).range(0..=2000).speed(5.0));
                    ui.end_row();
                }
                ui.label("Confidence level:");
                ui.add(egui::DragValue::new(&mut self.confidence).range(0.5..=0.999).speed(0.005).max_decimals(3));
                ui.end_row();
            });
        });
        let time = self.time.ok_or("Choose a time column")?;
        let response = self.response.ok_or("Choose a response column")?;
        let layout = match self.layout {
            Layout::Repeated => {
                DegradationLayout::RepeatedMeasures { unit: self.unit.ok_or("Choose a unit column")?, time, response }
            }
            Layout::Destructive => DegradationLayout::Destructive { time, response },
        };
        let path = match self.form {
            PathForm::Linear => PathModel::Linear { response: self.response_transform, time: self.time_transform },
            PathForm::Asymptotic => PathModel::Asymptotic,
            PathForm::Power => PathModel::Power,
        };
        if self.response_transform.apply(self.threshold).is_none() && self.form == PathForm::Linear {
            return Err("The threshold is outside the response scale's range".into());
        }
        let stress = if self.use_stress_column && !random {
            let column = self.stress.ok_or("Choose a stress column")?;
            let s = StressColumn { column, transform: self.transform, unit: self.unit_of_temperature };
            if !s.variable().valid(self.use_stress) {
                return Err("The use condition does not suit the stress relationship".into());
            }
            Some(s)
        } else {
            None
        };
        let method = if self.layout == Layout::Repeated && self.form == PathForm::Linear && stress.is_none() {
            self.method
        } else {
            DegradationMethod::PseudoFailureTimes
        };
        let Some(percentiles) = parse_numbers(&self.percentiles).filter(|v| v.iter().all(|p| *p > 0.0 && *p < 100.0))
        else {
            return Err("Percentiles must be numbers between 0 and 100".into());
        };
        let Some(times) = parse_numbers(&self.times).filter(|v| v.iter().all(|t| *t > 0.0)) else {
            return Err("Times must be positive numbers".into());
        };
        let request = DegradationRequest {
            layout,
            path,
            threshold: self.threshold,
            direction: self.direction,
            method,
            life_family: self.life_family,
            response_family: self.response_family,
            stress,
            use_stress: stress.map(|_| self.use_stress),
            confidence: self.confidence,
            probabilities: percentiles.iter().map(|p| p / 100.0).collect(),
            times,
            bootstrap: self.bootstrap,
        };
        let cols = request.columns();
        if (1..cols.len()).any(|i| cols[..i].contains(&cols[i])) {
            return Err("A column is chosen twice".into());
        }
        let how = match (self.layout, method) {
            (Layout::Destructive, _) => "destructive degradation",
            (_, DegradationMethod::RandomEffects) => "random-effects degradation paths",
            _ => "degradation paths and pseudo failure times",
        };
        let summary = format!(
            "{how} of {} (threshold {})",
            kit::name(table, response),
            app_model::format::format_number(self.threshold, 6)
        );
        kit::ready(SavedReport::Degradation { request }, summary)
    }
}
