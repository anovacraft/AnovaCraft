//! Dialog for the assay curve templates: choose the kind of experiment and a
//! template, then the columns it reads, any constants it needs, and
//! optionally starting values, bounds, sharing, weights and the
//! standard-curve acceptance limits. The fit is a nonlinear regression with
//! the template's formula and data-driven starting values.

use crate::dialog_kit::ComboExt as _;
use app_model::SavedReport;
use app_model::curve_templates::{CurveTemplate, DataSets, Extra, SecondKind, TemplateGroup, TemplateSettings};
use app_model::nonlinear_analysis::{NonlinearRequest, Weighting};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::tree_dialog::heading;

/// Window title.
pub const TITLE: &str = "Curve templates";

/// One parameter row: the template's name, three number fields as typed
/// (start empty: computed from the data) and the sharing choice.
#[derive(Clone)]
struct ParameterRow {
    name: &'static str,
    meaning: &'static str,
    start: String,
    lower: String,
    upper: String,
    shared: bool,
}

fn bound_text(v: f64) -> String {
    if v.is_finite() { v.to_string() } else { String::new() }
}

fn rows_of(template: CurveTemplate) -> Vec<ParameterRow> {
    template
        .parameters()
        .iter()
        .map(|p| ParameterRow {
            name: p.name,
            meaning: p.meaning,
            start: String::new(),
            lower: bound_text(p.lower),
            upper: bound_text(p.upper),
            shared: p.shared,
        })
        .collect()
}

fn constants_of(template: CurveTemplate) -> Vec<String> {
    template.all_constants().iter().map(|c| bound_text(c.default)).collect()
}

/// `Ok(None)` for an empty field, `Err` for text that is not a number.
fn parse(text: &str) -> Result<Option<f64>, ()> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(None);
    }
    t.parse::<f64>().ok().filter(|v| v.is_finite()).map(Some).ok_or(())
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

    fn of(w: Weighting) -> Self {
        match w {
            Weighting::Equal => Self::Equal,
            Weighting::InverseResponseSquared => Self::InverseY2,
            Weighting::InverseResponse => Self::InverseY,
            Weighting::Column(_) => Self::Column,
        }
    }
}

#[derive(Clone)]
pub struct CurveTemplateDialog {
    edit: Option<u64>,
    group: TemplateGroup,
    template: CurveTemplate,
    response: Option<usize>,
    x: Option<usize>,
    second: Option<usize>,
    data_sets: Option<usize>,
    dilution: Option<usize>,
    /// Standard preparation (relative potency); `None`: the first data set.
    standard: Option<String>,
    /// Constants as typed.
    constants: Vec<String>,
    rows: Vec<ParameterRow>,
    weights: WeightChoice,
    weight_column: Option<usize>,
    cv_limit: f64,
    tolerance: f64,
    end_tolerance: f64,
    min_percent: f64,
    min_levels: usize,
    level: f64,
    profile: bool,
}

impl Default for CurveTemplateDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl CurveTemplateDialog {
    pub fn new() -> Self {
        let template = CurveTemplate::FourPl;
        let defaults = TemplateSettings::new(template, 0);
        Self {
            edit: None,
            group: template.group(),
            template,
            response: None,
            x: None,
            second: None,
            data_sets: None,
            dilution: None,
            standard: None,
            constants: constants_of(template),
            rows: rows_of(template),
            weights: WeightChoice::of(template.default_weighting()),
            weight_column: None,
            cv_limit: defaults.cv_limit,
            tolerance: defaults.acceptance.tolerance_percent,
            end_tolerance: defaults.acceptance.end_tolerance_percent,
            min_percent: 100.0 * defaults.acceptance.min_fraction,
            min_levels: defaults.acceptance.min_levels,
            level: 0.95,
            profile: true,
        }
    }

    /// Switch to `template`, resetting what depends on it.
    fn choose(&mut self, template: CurveTemplate) {
        if template == self.template {
            return;
        }
        self.template = template;
        self.group = template.group();
        self.constants = constants_of(template);
        self.rows = rows_of(template);
        self.weights = WeightChoice::of(template.default_weighting());
        if template.second().is_none() {
            self.second = None;
        }
        if template.extra() != Extra::StandardCurve {
            self.dilution = None;
        }
    }

    /// The dialog filled from a saved template report (`None` for any other
    /// report, including a nonlinear regression without a template).
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::Nonlinear { request: r, .. } = saved else { return None };
        let t = r.template.as_ref()?;
        let mut d = Self::new();
        d.choose(t.template);
        d.response = kit::valid(table, r.response);
        d.x = kit::valid(table, t.x);
        d.second = t.second.and_then(|c| kit::valid(table, c));
        d.dilution = t.dilution.and_then(|c| kit::valid(table, c));
        d.data_sets = r.group.and_then(|c| kit::valid(table, c));
        d.standard.clone_from(&t.standard);
        d.constants = t.constants.iter().map(|&v| bound_text(v)).collect();
        // Starts stay automatic (recomputed from the data); bounds and
        // sharing are kept.
        for row in &mut d.rows {
            if let Some(p) = r.parameters.iter().find(|p| p.name == row.name) {
                row.lower = p.lower.map(|v| v.to_string()).unwrap_or_default();
                row.upper = p.upper.map(|v| v.to_string()).unwrap_or_default();
                row.shared = p.shared;
            }
        }
        (d.weights, d.weight_column) = match r.weighting {
            Weighting::Column(c) => (WeightChoice::Column, kit::valid(table, c)),
            w => (WeightChoice::of(w), None),
        };
        d.cv_limit = t.cv_limit;
        d.tolerance = t.acceptance.tolerance_percent;
        d.end_tolerance = t.acceptance.end_tolerance_percent;
        d.min_percent = 100.0 * t.acceptance.min_fraction;
        d.min_levels = t.acceptance.min_levels;
        d.level = r.confidence_level;
        d.profile = r.profile_intervals;
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
        let em = kit::em(ui);
        egui::Grid::new("template_choice").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Experiment:");
            let mut group = self.group;
            egui::ComboBox::from_id_salt("template_group")
                .selected_text(group.label())
                .width(kit::combo_width(ui))
                .show_list(ui, |ui| {
                    for g in TemplateGroup::ALL {
                        ui.selectable_value(&mut group, g, g.label());
                    }
                });
            if group != self.group {
                if let Some(first) = CurveTemplate::of_group(group).next() {
                    self.choose(first);
                }
            }
            ui.end_row();
            ui.label("Template:");
            let mut template = self.template;
            egui::ComboBox::from_id_salt("template_template")
                .selected_text(template.label())
                .width(kit::combo_width(ui))
                .height(em * 30.0)
                .show_list(ui, |ui| {
                    for t in CurveTemplate::of_group(self.group) {
                        ui.selectable_value(&mut template, t, t.label());
                    }
                });
            self.choose(template);
            ui.end_row();
        });
        let t = self.template;
        ui.label(egui::RichText::new(t.equation()).monospace().weak());
        kit::note(ui, t.description());
        heading(ui, "Columns");
        egui::Grid::new("template_columns").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label(format!("Response ({}):", t.y_label()));
            kit::column_choice(ui, "template_response", table, &mut self.response, numeric, None);
            ui.end_row();
            ui.label(format!("{}:", t.x_label()));
            kit::column_choice(ui, "template_x", table, &mut self.x, numeric, None);
            ui.end_row();
            if let Some(role) = t.second() {
                ui.label(format!("{}:", role.label));
                match role.kind {
                    SecondKind::Numeric => {
                        kit::column_choice(ui, "template_second", table, &mut self.second, numeric, None)
                    }
                    SecondKind::Indicator => {
                        kit::column_choice(ui, "template_second", table, &mut self.second, |_| true, None)
                    }
                };
                ui.end_row();
            }
            match t.data_sets() {
                DataSets::Required(label) => {
                    ui.label(format!("{label}:"));
                    kit::column_choice(ui, "template_sets", table, &mut self.data_sets, |_| true, None);
                }
                DataSets::Optional => {
                    ui.label("Data sets (optional):");
                    kit::column_choice(ui, "template_sets", table, &mut self.data_sets, |_| true, Some("One data set"));
                }
            }
            ui.end_row();
            if t.extra() == Extra::Potency {
                ui.label("Standard preparation:");
                let levels = self.data_sets.map(|g| distinct(table, g)).unwrap_or_default();
                if self.standard.as_ref().is_some_and(|s| !levels.contains(s)) {
                    self.standard = None;
                }
                egui::ComboBox::from_id_salt("template_standard")
                    .selected_text(self.standard.as_deref().unwrap_or("First data set"))
                    .width(kit::combo_width(ui))
                    .show_list(ui, |ui| {
                        ui.selectable_value(&mut self.standard, None, "First data set");
                        for l in levels {
                            ui.selectable_value(&mut self.standard, Some(l.clone()), l);
                        }
                    });
                ui.end_row();
            }
            if t.extra() == Extra::StandardCurve {
                ui.label("Dilution factor (optional):");
                kit::column_choice(ui, "template_dilution", table, &mut self.dilution, numeric, Some("No dilution"));
                ui.end_row();
            }
        });
        if t.extra() == Extra::StandardCurve {
            kit::note(
                ui,
                "Rows with a concentration are standards; rows with only a response are samples read off the curve.",
            );
        }
        let specs = t.all_constants();
        if !specs.is_empty() {
            heading(ui, "Constants");
            egui::Grid::new("template_constants").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                for (spec, text) in specs.iter().zip(self.constants.iter_mut()) {
                    ui.label(format!("{}:", spec.label));
                    let bad = parse(text).is_err();
                    let hint = if spec.required { "required" } else { "optional" };
                    let edit = egui::TextEdit::singleline(text).desired_width(em * 8.0).hint_text(hint);
                    let edit = if bad { edit.text_color(ui.visuals().error_fg_color) } else { edit };
                    let label = spec.label;
                    ui.add(edit).widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, label));
                    ui.end_row();
                }
            });
        }
        let grouped = self.data_sets.is_some();
        egui::CollapsingHeader::new("Parameters").id_salt("template_parameters").show(ui, |ui| {
            egui::Grid::new("template_parameter_grid").num_columns(6).spacing([6.0, 4.0]).show(ui, |ui| {
                ui.label("Name");
                ui.label("Start");
                ui.label("Lower bound");
                ui.label("Upper bound");
                if grouped {
                    ui.label("Shared by all data sets");
                }
                ui.label("Meaning");
                ui.end_row();
                for row in &mut self.rows {
                    ui.label(row.name);
                    let name = row.name;
                    for (field, what, hint, width) in [
                        (&mut row.start, "start", "automatic", 7.0),
                        (&mut row.lower, "lower bound", "none", 5.5),
                        (&mut row.upper, "upper bound", "none", 5.5),
                    ] {
                        let bad = parse(field).is_err();
                        let edit = egui::TextEdit::singleline(field).desired_width(em * width).hint_text(hint);
                        let edit = if bad { edit.text_color(ui.visuals().error_fg_color) } else { edit };
                        ui.add(edit).widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, format!("{name} {what}"))
                        });
                    }
                    if grouped {
                        let shared = row.shared;
                        ui.checkbox(&mut row.shared, "").widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::Checkbox,
                                true,
                                shared,
                                format!("{name} shared"),
                            )
                        });
                    }
                    ui.label(egui::RichText::new(row.meaning).weak());
                    ui.end_row();
                }
            });
        });
        kit::more_options(ui, |ui| {
            egui::Grid::new("template_options").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("Weights:");
                egui::ComboBox::from_id_salt("template_weights")
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
                    kit::column_choice(ui, "template_weight_column", table, &mut self.weight_column, numeric, None);
                    ui.end_row();
                }
                ui.label("Confidence level:");
                ui.add(egui::DragValue::new(&mut self.level).range(0.5..=0.999).speed(0.005).max_decimals(3));
                ui.end_row();
                if t.extra() == Extra::StandardCurve {
                    ui.label("Recovery limit (±%):");
                    ui.add(egui::DragValue::new(&mut self.tolerance).range(1.0..=100.0).speed(0.5).max_decimals(1));
                    ui.end_row();
                    ui.label("At the lowest and highest standard (±%):");
                    ui.add(egui::DragValue::new(&mut self.end_tolerance).range(1.0..=100.0).speed(0.5).max_decimals(1));
                    ui.end_row();
                    ui.label("Standards that must pass (%):");
                    ui.add(egui::DragValue::new(&mut self.min_percent).range(1.0..=100.0).speed(0.5).max_decimals(1));
                    ui.end_row();
                    ui.label("Concentration levels that must pass:");
                    ui.add(egui::DragValue::new(&mut self.min_levels).range(1..=50));
                    ui.end_row();
                    ui.label("Quantitation limit (CV %):");
                    ui.add(egui::DragValue::new(&mut self.cv_limit).range(1.0..=100.0).speed(0.5).max_decimals(1));
                    ui.end_row();
                }
            });
            ui.checkbox(&mut self.profile, "Profile intervals (refits the model along each parameter)");
        });
        let request = self.request(table)?;
        let mut summary = format!(
            "{} of {} on {}",
            t.label(),
            kit::name(table, request.response),
            kit::name(table, self.x.unwrap_or(0))
        );
        if let Some(g) = request.group {
            summary.push_str(&format!(", by {}", kit::name(table, g)));
        }
        kit::ready(request, summary)
    }

    /// The request the settings describe, or the first thing missing.
    fn request(&self, table: &DataTable) -> Result<NonlinearRequest, String> {
        let t = self.template;
        let Some(response) = self.response else { return Err("Choose a response".into()) };
        let Some(x) = self.x else { return Err(format!("Choose the {} column", t.x_label().to_lowercase())) };
        if x == response {
            return Err("Response and x must be different columns".into());
        }
        let mut settings = TemplateSettings::new(t, x);
        if let Some(role) = t.second() {
            let Some(s) = self.second else { return Err(format!("Choose the {} column", role.label.to_lowercase())) };
            if s == x || s == response {
                return Err(format!("The {} column must differ from the response and x", role.label.to_lowercase()));
            }
            settings.second = Some(s);
        }
        if let DataSets::Required(label) = t.data_sets() {
            if self.data_sets.is_none() {
                return Err(format!("Choose the {} column", label.to_lowercase()));
            }
        }
        if self.data_sets.is_some_and(|g| g == response || g == x) {
            return Err("The data-set column must differ from the response and x".into());
        }
        settings.dilution = self.dilution.filter(|_| t.extra() == Extra::StandardCurve);
        for (spec, (text, slot)) in
            t.all_constants().iter().zip(self.constants.iter().zip(settings.constants.iter_mut()))
        {
            match parse(text) {
                Ok(Some(v)) => *slot = v,
                Ok(None) if spec.required => return Err(format!("Enter the {}", spec.label.to_lowercase())),
                Ok(None) => *slot = f64::NAN,
                Err(()) => return Err(format!("Enter a number for the {}", spec.label.to_lowercase())),
            }
        }
        settings.standard = self.standard.clone().filter(|_| t.extra() == Extra::Potency);
        settings.cv_limit = self.cv_limit;
        settings.acceptance.tolerance_percent = self.tolerance;
        settings.acceptance.end_tolerance_percent = self.end_tolerance;
        settings.acceptance.min_fraction = self.min_percent / 100.0;
        settings.acceptance.min_levels = self.min_levels;
        let mut request = settings.request(table, response, self.data_sets).map_err(|e| capitalise(&e.to_string()))?;
        for row in &self.rows {
            let (Ok(start), Ok(lower), Ok(upper)) = (parse(&row.start), parse(&row.lower), parse(&row.upper)) else {
                return Err(format!("Enter numbers for the values of {}", row.name));
            };
            if let (Some(l), Some(u)) = (lower, upper) {
                if l > u {
                    return Err(format!("Give {} a lower bound below its upper bound", row.name));
                }
            }
            if let Some(p) = request.parameters.iter_mut().find(|p| p.name == row.name) {
                if start.is_some() {
                    p.start = start;
                }
                p.lower = lower;
                p.upper = upper;
                p.shared = row.shared;
                // An automatic start outside the bounds typed here moves inside.
                if let Some(s) = p.start {
                    let s = lower.map_or(s, |l| s.max(l));
                    p.start = Some(upper.map_or(s, |u| s.min(u)));
                }
            }
        }
        request.weighting = match self.weights {
            WeightChoice::Equal => Weighting::Equal,
            WeightChoice::InverseY2 => Weighting::InverseResponseSquared,
            WeightChoice::InverseY => Weighting::InverseResponse,
            WeightChoice::Column => {
                Weighting::Column(self.weight_column.ok_or_else(|| "Choose the weight column".to_owned())?)
            }
        };
        request.confidence_level = self.level;
        request.profile_intervals = self.profile;
        Ok(request)
    }
}

/// The distinct non-empty values of column `j`, in order of appearance.
fn distinct(table: &DataTable, j: usize) -> Vec<String> {
    let Some(col) = table.column(j) else { return Vec::new() };
    let mut out: Vec<String> = Vec::new();
    for r in 0..table.n_rows() {
        let v = col.level_text(r);
        if !v.is_empty() && !out.contains(&v) {
            out.push(v);
        }
    }
    out
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().collect::<String>() + c.as_str())
}

impl Remember for CurveTemplateDialog {
    fn memory_key(&self) -> String {
        TITLE.into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        let mut numeric = columns.iter().copied().filter(|&c| kit::is_numeric(table, c));
        self.response = numeric.next();
        self.x = numeric.next();
    }
}
