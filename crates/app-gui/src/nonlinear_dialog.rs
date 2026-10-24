//! Dialog for nonlinear regression: a library form in one predictor, a model
//! formula with named parameters, a differential equation or an implicit
//! equation, with starting values, bounds, data sets with shared
//! parameters, weights, the fitting method and a comparison curve.

use crate::dialog_kit::ComboExt as _;
use app_model::nonlinear_analysis::{FitMethod, NonlinearModel, NonlinearRequest, ParameterSetting, Weighting};
use app_model::{CurveForm, SavedReport};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::tree_dialog::heading;

/// Window title.
pub const TITLE: &str = "Nonlinear regression";

/// One parameter row: name and three number fields as typed.
#[derive(Clone)]
struct ParameterRow {
    name: String,
    start: String,
    lower: String,
    upper: String,
    shared: bool,
}

impl ParameterRow {
    fn new(name: &str, start: &str) -> Self {
        Self {
            name: name.to_owned(),
            start: start.to_owned(),
            lower: String::new(),
            upper: String::new(),
            shared: true,
        }
    }

    /// The row of a saved parameter, its numbers written out.
    fn saved(p: &ParameterSetting) -> Self {
        let text = |v: Option<f64>| v.map(|v| v.to_string()).unwrap_or_default();
        Self {
            name: p.name.clone(),
            start: text(p.start),
            lower: text(p.lower),
            upper: text(p.upper),
            shared: p.shared,
        }
    }
}

/// The kind of model.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Library,
    Formula,
    Differential,
    Implicit,
}

/// Weighting choice (the column is chosen separately).
#[derive(Clone, Copy, PartialEq, Eq)]
enum WeightChoice {
    Equal,
    InverseY2,
    InverseY,
    Column,
}

impl WeightChoice {
    const ALL: [Self; 4] = [Self::Equal, Self::InverseY2, Self::InverseY, Self::Column];
    fn label(self) -> &'static str {
        match self {
            Self::Equal => Weighting::Equal.label(),
            Self::InverseY2 => Weighting::InverseResponseSquared.label(),
            Self::InverseY => Weighting::InverseResponse.label(),
            Self::Column => Weighting::Column(0).label(),
        }
    }
}

/// Fitting-method choice.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MethodChoice {
    LeastSquares,
    Robust,
    Outliers,
}

impl MethodChoice {
    const ALL: [Self; 3] = [Self::LeastSquares, Self::Robust, Self::Outliers];
    fn label(self) -> &'static str {
        match self {
            Self::LeastSquares => "Least squares",
            Self::Robust => "Robust (Cauchy loss)",
            Self::Outliers => "Least squares after removing outliers",
        }
    }
}

#[derive(Clone)]
pub struct NonlinearDialog {
    edit: Option<u64>,
    response: Option<usize>,
    kind: Kind,
    form: CurveForm,
    predictor: Option<usize>,
    /// Rows for the library form (names fixed).
    library_rows: Vec<ParameterRow>,
    formula: String,
    /// Rows for the formula, differential and implicit models (names typed).
    formula_rows: Vec<ParameterRow>,
    /// Rate formula of a differential equation.
    rate: String,
    /// The parameter that is the response's value at the start.
    initial: String,
    /// Predictor value where the solution starts (empty: smallest).
    start_x: String,
    /// Equation of an implicit model.
    equation: String,
    group: Option<usize>,
    weights: WeightChoice,
    weight_column: Option<usize>,
    method: MethodChoice,
    /// Q in percent.
    q_percent: f64,
    compare_with: Option<CurveForm>,
    level: f64,
    profile: bool,
    max_iterations: usize,
}

/// `Ok(None)` for an empty field, `Err` for text that is not a number.
fn parse(text: &str) -> Result<Option<f64>, ()> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(None);
    }
    t.parse::<f64>().ok().filter(|v| v.is_finite()).map(Some).ok_or(())
}

fn library_rows(form: CurveForm) -> Vec<ParameterRow> {
    form.parameter_names().iter().map(|n| ParameterRow::new(n, "")).collect()
}

impl Default for NonlinearDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl NonlinearDialog {
    pub fn new() -> Self {
        let form = CurveForm::Logistic3;
        Self {
            edit: None,
            response: None,
            kind: Kind::Library,
            form,
            predictor: None,
            library_rows: library_rows(form),
            formula: String::new(),
            formula_rows: vec![ParameterRow::new("a", ""), ParameterRow::new("b", "")],
            rate: String::new(),
            initial: String::new(),
            start_x: String::new(),
            equation: String::new(),
            group: None,
            weights: WeightChoice::Equal,
            weight_column: None,
            method: MethodChoice::LeastSquares,
            q_percent: 1.0,
            compare_with: None,
            level: 0.95,
            profile: true,
            max_iterations: NonlinearRequest::DEFAULT_ITERATIONS,
        }
    }

    /// The dialog filled from a saved nonlinear regression.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::Nonlinear { request: r, .. } = saved else { return None };
        let mut d = Self::new();
        d.response = kit::valid(table, r.response);
        let rows: Vec<ParameterRow> = r.parameters.iter().map(ParameterRow::saved).collect();
        match &r.model {
            NonlinearModel::Library { form, predictor } => {
                d.form = *form;
                d.predictor = kit::valid(table, *predictor);
                d.library_rows = if rows.len() == form.parameter_names().len() { rows } else { library_rows(*form) };
            }
            NonlinearModel::Formula { formula, .. } => {
                d.kind = Kind::Formula;
                d.formula = formula.clone();
                d.formula_rows = rows;
            }
            NonlinearModel::Differential { rate, predictor, initial, start } => {
                d.kind = Kind::Differential;
                d.rate = rate.clone();
                d.predictor = kit::valid(table, *predictor);
                d.initial = initial.clone();
                d.start_x = start.map(|v| v.to_string()).unwrap_or_default();
                d.formula_rows = rows;
            }
            NonlinearModel::Implicit { equation, .. } => {
                d.kind = Kind::Implicit;
                d.equation = equation.clone();
                d.formula_rows = rows;
            }
        }
        d.group = r.group.and_then(|g| kit::valid(table, g));
        (d.weights, d.weight_column) = match r.weighting {
            Weighting::Equal => (WeightChoice::Equal, None),
            Weighting::InverseResponseSquared => (WeightChoice::InverseY2, None),
            Weighting::InverseResponse => (WeightChoice::InverseY, None),
            Weighting::Column(c) => (WeightChoice::Column, kit::valid(table, c)),
        };
        (d.method, d.q_percent) = match r.method {
            FitMethod::LeastSquares => (MethodChoice::LeastSquares, 1.0),
            FitMethod::Robust => (MethodChoice::Robust, 1.0),
            FitMethod::RemoveOutliers { q } => (MethodChoice::Outliers, 100.0 * q),
        };
        d.compare_with = r.compare_with;
        d.level = r.confidence_level;
        d.profile = r.profile_intervals;
        d.max_iterations = r.max_iterations;
        Some(d)
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<NonlinearRequest> {
        let dialog = Dialog::new(TITLE).editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new() };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<NonlinearRequest> {
        let numeric = |i: usize| kit::is_numeric(table, i);
        egui::Grid::new("nonlinear_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Response:");
            kit::column_choice(ui, "nonlinear_response", table, &mut self.response, numeric, None);
            ui.end_row();
            ui.label("Model:");
            // Two rows of two: a wrapped row inside the grid overlaps the next one.
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.kind, Kind::Library, "Curve from the library");
                    ui.radio_value(&mut self.kind, Kind::Formula, "Formula with parameters");
                });
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.kind, Kind::Differential, "Differential equation");
                    ui.radio_value(&mut self.kind, Kind::Implicit, "Implicit equation");
                });
            });
            ui.end_row();
            if self.kind == Kind::Library {
                ui.label("Curve:");
                let before = self.form;
                egui::ComboBox::from_id_salt("nonlinear_form")
                    .selected_text(self.form.label())
                    .width(kit::combo_width(ui))
                    .height(kit::em(ui) * 30.0)
                    .show_list(ui, |ui| {
                        for form in CurveForm::ALL {
                            ui.selectable_value(&mut self.form, form, form.label());
                        }
                    });
                if self.form != before {
                    self.library_rows = library_rows(self.form);
                }
                ui.end_row();
            }
            if matches!(self.kind, Kind::Library | Kind::Differential) {
                ui.label("Predictor (x):");
                kit::column_choice(ui, "nonlinear_predictor", table, &mut self.predictor, numeric, None);
                ui.end_row();
            }
        });
        let em = kit::em(ui);
        let code = |ui: &mut egui::Ui, text: &mut String, hint: &str| {
            ui.add(
                egui::TextEdit::multiline(text).code_editor().desired_rows(2).desired_width(em * 30.0).hint_text(hint),
            );
        };
        match self.kind {
            Kind::Library => {
                let names: Vec<String> = self.form.parameter_names().iter().map(|s| (*s).to_owned()).collect();
                ui.label(egui::RichText::new(format!("y = {}", self.form.formula("x", &names))).monospace().weak());
            }
            Kind::Formula => {
                ui.add_space(4.0);
                ui.label("Model formula (columns and parameters by name, as in formula columns; two columns make a surface):");
                code(ui, &mut self.formula, "Vmax * Conc / (Km + Conc)");
            }
            Kind::Differential => {
                ui.add_space(4.0);
                let y = self.response.map_or("y", |r| kit::name(table, r)).to_owned();
                let x = self.predictor.map_or("x", |r| kit::name(table, r)).to_owned();
                ui.label(format!("Rate of change d{y}/d{x} (the response, the predictor and the parameters by name):"));
                code(ui, &mut self.rate, "-k * Conc");
                egui::Grid::new("nonlinear_ode").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    ui.label(format!("Parameter for {y} at the start:"));
                    let names: Vec<String> = self.formula_rows.iter().map(|r| r.name.trim().to_owned()).collect();
                    egui::ComboBox::from_id_salt("nonlinear_initial")
                        .selected_text(if self.initial.is_empty() { "Choose a parameter…" } else { &self.initial })
                        .width(kit::combo_width(ui))
                        .show_list(ui, |ui| {
                            for n in names {
                                ui.selectable_value(&mut self.initial, n.clone(), n);
                            }
                        });
                    ui.end_row();
                    ui.label(format!("Starting {x}:"));
                    let bad = parse(&self.start_x).is_err();
                    let edit =
                        egui::TextEdit::singleline(&mut self.start_x).desired_width(em * 7.0).hint_text("smallest");
                    let edit = if bad { edit.text_color(ui.visuals().error_fg_color) } else { edit };
                    ui.add(edit).widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Starting predictor value")
                    });
                    ui.end_row();
                });
            }
            Kind::Implicit => {
                ui.add_space(4.0);
                ui.label(
                    "Equation that is zero on the curve, in the response and one other column (fitted by orthogonal distance):",
                );
                code(ui, &mut self.equation, "(X - a)**2 + (Y - b)**2 - r**2");
            }
        }
        heading(ui, "Parameters");
        let typed = self.kind != Kind::Library;
        let grouped = self.group.is_some();
        let rows = if typed { &mut self.formula_rows } else { &mut self.library_rows };
        let mut remove = None;
        egui::Grid::new("nonlinear_parameters").num_columns(6).spacing([6.0, 4.0]).show(ui, |ui| {
            ui.label("Name");
            ui.label("Start");
            ui.label("Lower bound");
            ui.label("Upper bound");
            if grouped {
                ui.label("Shared by all data sets");
            }
            ui.label("");
            ui.end_row();
            for (k, row) in rows.iter_mut().enumerate() {
                if typed {
                    ui.add(egui::TextEdit::singleline(&mut row.name).desired_width(em * 6.0)).widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, format!("Parameter {} name", k + 1))
                    });
                } else {
                    ui.label(&row.name);
                }
                let hint = if typed { "required" } else { "automatic" };
                let label = row.name.clone();
                for (field, what, hint, width) in [
                    (&mut row.start, "start", hint, 7.0),
                    (&mut row.lower, "lower bound", "none", 5.5),
                    (&mut row.upper, "upper bound", "none", 5.5),
                ] {
                    let bad = parse(field).is_err();
                    let edit = egui::TextEdit::singleline(field).desired_width(em * width).hint_text(hint);
                    let edit = if bad { edit.text_color(ui.visuals().error_fg_color) } else { edit };
                    ui.add(edit).widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, format!("{label} {what}"))
                    });
                }
                if grouped {
                    ui.checkbox(&mut row.shared, "").widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::Checkbox,
                            true,
                            row.shared,
                            format!("{label} shared"),
                        )
                    });
                }
                if typed && ui.small_button("Remove").clicked() {
                    remove = Some(k);
                }
                ui.end_row();
            }
        });
        if let Some(k) = remove {
            rows.remove(k);
        }
        if typed && ui.small_button("Add parameter").clicked() {
            let n = rows.len() + 1;
            rows.push(ParameterRow::new(&format!("p{n}"), ""));
        }
        heading(ui, "Data sets and fitting");
        egui::Grid::new("nonlinear_fitting").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Data sets (optional):");
            kit::column_choice(ui, "nonlinear_group", table, &mut self.group, |_| true, Some("One data set"));
            ui.end_row();
            ui.label("Weights:");
            egui::ComboBox::from_id_salt("nonlinear_weights")
                .selected_text(self.weights.label())
                .width(kit::combo_width(ui))
                .show_list(ui, |ui| {
                    for w in WeightChoice::ALL {
                        ui.selectable_value(&mut self.weights, w, w.label());
                    }
                });
            ui.end_row();
            if self.weights == WeightChoice::Column {
                ui.label("Weight column:");
                kit::column_choice(ui, "nonlinear_weight_column", table, &mut self.weight_column, numeric, None);
                ui.end_row();
            }
            ui.label("Fitting:");
            egui::ComboBox::from_id_salt("nonlinear_method")
                .selected_text(self.method.label())
                .width(kit::combo_width(ui))
                .show_list(ui, |ui| {
                    for m in MethodChoice::ALL {
                        ui.selectable_value(&mut self.method, m, m.label());
                    }
                });
            ui.end_row();
            if self.method == MethodChoice::Outliers {
                ui.label("Q (false discovery rate, %):");
                ui.add(egui::DragValue::new(&mut self.q_percent).range(0.01..=10.0).speed(0.05).max_decimals(2));
                ui.end_row();
            }
            if self.kind == Kind::Library && self.group.is_none() {
                ui.label("Compare with:");
                egui::ComboBox::from_id_salt("nonlinear_compare")
                    .selected_text(self.compare_with.map_or("No comparison", CurveForm::label))
                    .width(kit::combo_width(ui))
                    .height(em * 30.0)
                    .show_list(ui, |ui| {
                        ui.selectable_value(&mut self.compare_with, None, "No comparison");
                        for form in CurveForm::ALL.into_iter().filter(|f| *f != self.form) {
                            ui.selectable_value(&mut self.compare_with, Some(form), form.label());
                        }
                    });
                ui.end_row();
            }
        });
        if self.method == MethodChoice::Outliers {
            kit::note(ui, "A robust fit finds the outliers; they are removed and the rest refitted by least squares.");
        }
        kit::more_options(ui, |ui| {
            egui::Grid::new("nonlinear_options").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("Confidence level:");
                ui.add(egui::DragValue::new(&mut self.level).range(0.5..=0.999).speed(0.005).max_decimals(3));
                ui.end_row();
                ui.label("Iteration limit:");
                ui.add(egui::DragValue::new(&mut self.max_iterations).range(1..=10_000));
                ui.end_row();
            });
            ui.checkbox(&mut self.profile, "Profile intervals (refits the model along each parameter)");
        });
        let request = self.request(table)?;
        let what = match &request.model {
            NonlinearModel::Library { form, predictor } => format!(
                "{} curve of {} on {}",
                form.label(),
                kit::name(table, request.response),
                kit::name(table, *predictor)
            ),
            NonlinearModel::Formula { .. } => format!(
                "Formula model of {} with {}",
                kit::name(table, request.response),
                kit::count(request.parameters.len(), "parameter", "parameters")
            ),
            NonlinearModel::Differential { predictor, .. } => format!(
                "Differential equation for {} in {}",
                kit::name(table, request.response),
                kit::name(table, *predictor)
            ),
            NonlinearModel::Implicit { .. } => format!(
                "Implicit curve in {} with {}",
                kit::name(table, request.response),
                kit::count(request.parameters.len(), "parameter", "parameters")
            ),
        };
        let summary = match request.group {
            Some(g) => format!("{what}, one per data set of {}", kit::name(table, g)),
            None => what,
        };
        kit::ready(request, summary)
    }

    /// The request the settings describe, or the first thing missing.
    fn request(&self, table: &DataTable) -> Result<NonlinearRequest, String> {
        let Some(response) = self.response else { return Err("Choose a response".into()) };
        let need_predictor = || {
            let Some(predictor) = self.predictor else { return Err("Choose a predictor".to_owned()) };
            if predictor == response {
                return Err("Response and predictor must be different columns".into());
            }
            Ok(predictor)
        };
        let typed = self.kind != Kind::Library;
        let mut request = match self.kind {
            Kind::Library => NonlinearRequest::library(response, need_predictor()?, self.form),
            Kind::Formula => {
                if self.formula.trim().is_empty() {
                    return Err("Write the model formula".into());
                }
                NonlinearRequest::formula(table, response, &self.formula, &[])
            }
            Kind::Differential => {
                let predictor = need_predictor()?;
                if self.rate.trim().is_empty() {
                    return Err("Write the rate of change".into());
                }
                if self.initial.is_empty() {
                    return Err("Choose the parameter for the starting value".into());
                }
                let Ok(start) = parse(&self.start_x) else {
                    return Err("Enter a number for the starting value".into());
                };
                let mut r = NonlinearRequest::differential(response, predictor, &self.rate, &self.initial, &[]);
                if let NonlinearModel::Differential { start: s, .. } = &mut r.model {
                    *s = start;
                }
                r
            }
            Kind::Implicit => {
                if self.equation.trim().is_empty() {
                    return Err("Write the equation".into());
                }
                NonlinearRequest::implicit(table, response, &self.equation, &[])
            }
        };
        let rows = if typed { &self.formula_rows } else { &self.library_rows };
        let mut settings = Vec::new();
        for row in rows {
            let name = row.name.trim();
            let (Ok(start), Ok(lower), Ok(upper)) = (parse(&row.start), parse(&row.lower), parse(&row.upper)) else {
                return Err(format!("Enter numbers for the values of {name}"));
            };
            if let (Some(l), Some(u)) = (lower, upper) {
                if l > u {
                    return Err(format!("Give {name} a lower bound below its upper bound"));
                }
            }
            if typed && start.is_none() {
                return Err(format!("Enter a starting value for {name}"));
            }
            settings.push(ParameterSetting { name: name.to_owned(), start, lower, upper, shared: row.shared });
        }
        if typed {
            if settings.is_empty() {
                return Err("Add at least one parameter".into());
            }
            let names: Vec<String> = settings.iter().map(|s| s.name.clone()).collect();
            let text = match self.kind {
                Kind::Formula => &self.formula,
                Kind::Differential => &self.rate,
                _ => &self.equation,
            };
            match app_model::formula::compile_with_parameters(table, text, &names) {
                Ok(compiled) => {
                    let uses_response = compiled.columns(table).contains(&response);
                    match self.kind {
                        Kind::Formula if uses_response => {
                            return Err("Leave the response column out of the formula".into());
                        }
                        Kind::Implicit if !uses_response => {
                            return Err("Use the response column in the equation".into());
                        }
                        Kind::Differential if !names.contains(&self.initial) => {
                            return Err("Choose the parameter for the starting value".into());
                        }
                        _ => {}
                    }
                }
                Err(e) => return Err(format!("Formula: {e}")),
            }
        }
        request.parameters = settings;
        request.group = self.group;
        if self.group == Some(response) {
            return Err("The data-set column must differ from the response".into());
        }
        request.weighting = match self.weights {
            WeightChoice::Equal => Weighting::Equal,
            WeightChoice::InverseY2 => Weighting::InverseResponseSquared,
            WeightChoice::InverseY => Weighting::InverseResponse,
            WeightChoice::Column => {
                Weighting::Column(self.weight_column.ok_or_else(|| "Choose the weight column".to_owned())?)
            }
        };
        request.method = match self.method {
            MethodChoice::LeastSquares => FitMethod::LeastSquares,
            MethodChoice::Robust => FitMethod::Robust,
            MethodChoice::Outliers => FitMethod::RemoveOutliers { q: self.q_percent / 100.0 },
        };
        request.compare_with =
            if self.kind == Kind::Library && self.group.is_none() { self.compare_with } else { None };
        request.confidence_level = self.level;
        request.profile_intervals = self.profile;
        request.max_iterations = self.max_iterations;
        Ok(request)
    }
}

impl Remember for NonlinearDialog {
    fn memory_key(&self) -> String {
        TITLE.into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        let mut numeric = columns.iter().copied().filter(|&c| kit::is_numeric(table, c));
        self.response = numeric.next();
        self.predictor = numeric.next();
    }
}
