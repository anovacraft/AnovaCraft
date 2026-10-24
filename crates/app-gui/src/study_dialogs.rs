//! Dialogs for the measurement studies (gauge study worksheet, bias and
//! linearity, single-gauge repeatability, intraclass correlation) and the
//! design tools (orthogonal-array robust designs, signal-to-noise analysis,
//! declaring a table as a design).
//!
//! The reports run through [`app_model::ComparisonReport`], so their settings
//! are also edited in place from the report's options menu
//! ([`report_settings`]). The worksheet and robust-design dialogs create new
//! data tables.

use crate::dialog_kit::ComboExt as _;
use app_model::{
    ArrayFactor, DeclaredFactor, DesignDeclarationRequest, GaugeWorksheetRequest, IccRequest, LinearityRequest,
    RepeatabilityRequest, RobustDesignRequest, SavedReport, SignalToNoiseRequest, SummaryStat, create_gauge_worksheet,
    create_robust_design,
};
use eframe::egui::{self, RichText};
use stat_data::DataTable;
use stat_doe::{SignalToNoise, StandardArray, default_factor_names};
use stat_quality::{GaugeLayout, RunOrder};

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::report_view::ReportRequest;

/// Which report the study dialog sets up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StudyTool {
    Linearity,
    Repeatability,
    Icc,
    SignalToNoise,
    DeclareDesign,
}

/// Dialog for the table-based study reports.
#[derive(Clone)]
pub struct StudyDialog {
    edit: Option<u64>,
    /// Opened from a menu that cannot restore remembered settings; the app
    /// restores them before showing it ([`StudyDialog::opening`]).
    recall: bool,
    tool: StudyTool,
    first: Option<usize>,
    second: Option<usize>,
    third: Option<usize>,
    linearity: LinearityRequest,
    repeatability: RepeatabilityRequest,
    icc: IccRequest,
    /// Factor and response columns, indexed by column.
    factors: Vec<bool>,
    responses: Vec<bool>,
    /// Declared factors' low and high settings from a report being edited.
    ranges: Vec<DeclaredFactor>,
    /// A declared split-plot design's recorded model (kept when editing).
    model: String,
    ratio: SignalToNoise,
}

impl StudyDialog {
    /// A dialog for `tool`, pre-selecting columns whose names suggest their role.
    pub fn new(tool: StudyTool, table: &DataTable) -> Self {
        let guess = |words: &[&str], numeric: bool| {
            table.columns().iter().enumerate().position(|(i, c)| {
                let name = c.name.to_lowercase();
                words.iter().any(|w| name.contains(w)) && (!numeric || kit::is_numeric(table, i))
            })
        };
        let measurement = guess(&["measure", "reading", "value"], true);
        let (first, second, third) = match tool {
            StudyTool::Linearity => (guess(&["reference", "master", "standard"], true), measurement, None),
            StudyTool::Repeatability => (measurement, None, None),
            StudyTool::Icc => {
                (guess(&["part", "item", "sample"], false), guess(&["operator", "appraiser"], false), measurement)
            }
            _ => (None, None, None),
        };
        Self {
            edit: None,
            recall: false,
            tool,
            first,
            second,
            third,
            linearity: LinearityRequest::new(0, 0),
            repeatability: RepeatabilityRequest::new(0, 0.0, 1.0),
            icc: IccRequest::new(0, 0, 0),
            factors: vec![false; table.n_cols()],
            responses: vec![false; table.n_cols()],
            ranges: Vec::new(),
            model: String::new(),
            ratio: SignalToNoise::SmallerTheBetter,
        }
    }

    /// A new dialog for `tool` from a menu that cannot reach the remembered
    /// settings: the app swaps them in before showing it ([`Self::needs_recall`]).
    pub fn opening(tool: StudyTool, table: &DataTable) -> Self {
        Self { recall: true, ..Self::new(tool, table) }
    }

    /// Whether the app should restore remembered settings into this dialog
    /// (once; this clears the mark).
    pub fn needs_recall(&mut self) -> bool {
        std::mem::take(&mut self.recall)
    }

    /// The dialog filled from a saved study report.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let valid = |c: usize| kit::valid(table, c);
        let d = match saved {
            SavedReport::GaugeLinearity { request } => Self {
                first: valid(request.reference),
                second: valid(request.measurement),
                linearity: *request,
                ..Self::new(StudyTool::Linearity, table)
            },
            SavedReport::GaugeRepeatability { request } => Self {
                first: valid(request.measurement),
                second: request.direction.and_then(valid),
                third: request.order.and_then(valid),
                repeatability: *request,
                ..Self::new(StudyTool::Repeatability, table)
            },
            SavedReport::IntraclassCorrelation { request } => Self {
                first: valid(request.part),
                second: valid(request.operator),
                third: valid(request.measurement),
                icc: *request,
                ..Self::new(StudyTool::Icc, table)
            },
            SavedReport::SignalToNoise { request } => Self {
                factors: kit::mask(table.n_cols(), &request.factors),
                responses: kit::mask(table.n_cols(), &request.responses),
                ratio: request.ratio,
                ..Self::new(StudyTool::SignalToNoise, table)
            },
            SavedReport::DeclaredDesign { request } => {
                let factors: Vec<usize> = request.factors.iter().map(|f| f.column).collect();
                Self {
                    factors: kit::mask(table.n_cols(), &factors),
                    responses: kit::mask(table.n_cols(), &request.responses),
                    ranges: request.factors.clone(),
                    first: request.whole_plot.and_then(valid),
                    second: request.subplot.and_then(valid),
                    model: request.model.clone(),
                    ..Self::new(StudyTool::DeclareDesign, table)
                }
            }
            _ => return None,
        };
        Some(d)
    }

    fn title(&self) -> &'static str {
        match self.tool {
            StudyTool::Linearity => "Gauge bias and linearity",
            StudyTool::Repeatability => "Single-gauge repeatability (Cg, Cgk)",
            StudyTool::Icc => "Intraclass correlation evaluation",
            StudyTool::SignalToNoise => "Signal-to-noise analysis",
            StudyTool::DeclareDesign => "Declare a table as a design",
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<Box<ReportRequest>> {
        let dialog = Dialog::new(self.title()).editing(self.edit);
        let outcome = dialog.show(ctx, |ui| {
            self.contents(ui, table)
                .map(|r| kit::Ready { output: Box::new(ReportRequest::Comparison(r.output)), summary: r.summary })
        });
        match outcome {
            Outcome::Reset => {
                *self = Self { edit: self.edit, first: None, second: None, third: None, ..Self::new(self.tool, table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        let numeric = |i: usize| kit::is_numeric(table, i);
        let any = |_: usize| true;
        let name = |i: usize| kit::name(table, i);
        match self.tool {
            StudyTool::Linearity => {
                kit::note(ui, "One row per measurement of a part whose reference (true) value is in another column.");
                egui::Grid::new("lin_cols").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label("Reference value:");
                    kit::column_choice(ui, "lin_ref", table, &mut self.first, numeric, None);
                    ui.end_row();
                    ui.label("Measurement:");
                    kit::column_choice(ui, "lin_meas", table, &mut self.second, numeric, None);
                    ui.end_row();
                });
                kit::more_options(ui, |ui| linearity_settings(ui, &mut self.linearity));
                let Some(reference) = self.first else { return Err("Choose the reference-value column".into()) };
                let Some(measurement) = self.second else { return Err("Choose a measurement column".into()) };
                if reference == measurement {
                    return Err("Reference and measurement must be different columns".into());
                }
                kit::ready(
                    SavedReport::GaugeLinearity {
                        request: LinearityRequest { reference, measurement, ..self.linearity },
                    },
                    format!("Bias and linearity of {} against {}", name(measurement), name(reference)),
                )
            }
            StudyTool::Repeatability => {
                kit::note(ui, "Repeated measurements of one reference part on one gauge, one row each, in order.");
                egui::Grid::new("rep_cols").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label("Measurement:");
                    kit::column_choice(ui, "rep_meas", table, &mut self.first, numeric, None);
                    ui.end_row();
                    repeatability_part(ui, &mut self.repeatability);
                    ui.label("Direction of approach:");
                    kit::column_choice(ui, "rep_dir", table, &mut self.second, any, Some("(none)"));
                    ui.end_row();
                    ui.label("Time or order:");
                    kit::column_choice(ui, "rep_order", table, &mut self.third, numeric, Some("(row order)"));
                    ui.end_row();
                });
                kit::more_options(ui, |ui| {
                    repeatability_indices(ui, &mut self.repeatability);
                    repeatability_sections(ui, &mut self.repeatability);
                });
                let Some(measurement) = self.first else { return Err("Choose a measurement column".into()) };
                if self.second == Some(measurement) || self.third == Some(measurement) {
                    return Err("Direction and time must be columns other than the measurement".into());
                }
                let r = RepeatabilityRequest {
                    measurement,
                    direction: self.second,
                    order: self.third,
                    ..self.repeatability
                };
                kit::ready(
                    SavedReport::GaugeRepeatability { request: r },
                    format!(
                        "Cg and Cgk of {}, reference {}, tolerance {}",
                        name(measurement),
                        r.reference,
                        r.tolerance
                    ),
                )
            }
            StudyTool::Icc => {
                kit::note(
                    ui,
                    "A crossed study, one row per measurement: every part measured equally often by every operator.",
                );
                egui::Grid::new("icc_cols").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label("Part:");
                    kit::column_choice(ui, "icc_part", table, &mut self.first, any, None);
                    ui.end_row();
                    ui.label("Operator:");
                    kit::column_choice(ui, "icc_oper", table, &mut self.second, any, None);
                    ui.end_row();
                    ui.label("Measurement:");
                    kit::column_choice(ui, "icc_meas", table, &mut self.third, numeric, None);
                    ui.end_row();
                });
                kit::more_options(ui, |ui| icc_settings(ui, &mut self.icc));
                let Some(part) = self.first else { return Err("Choose a part column".into()) };
                let Some(operator) = self.second else { return Err("Choose an operator column".into()) };
                let Some(measurement) = self.third else { return Err("Choose a measurement column".into()) };
                if !kit::all_different(&[Some(part), Some(operator), Some(measurement)]) {
                    return Err("Part, operator and measurement must be different columns".into());
                }
                kit::ready(
                    SavedReport::IntraclassCorrelation {
                        request: IccRequest { part, operator, measurement, ..self.icc },
                    },
                    format!("Intraclass correlation of {} by {} and {}", name(measurement), name(part), name(operator)),
                )
            }
            StudyTool::SignalToNoise => {
                kit::note(
                    ui,
                    "One row per run of an orthogonal-array experiment: control factor columns, and one response \
                     column per noise condition or replicate.",
                );
                self.columns_picker(ui, table, "Control factors", "Responses");
                ratio_choice(ui, &mut self.ratio);
                let (factors, responses) = self.chosen(table);
                if factors.is_empty() {
                    return Err("Choose at least one control factor".into());
                }
                if responses.is_empty() {
                    return Err("Choose at least one response".into());
                }
                let summary = format!(
                    "Signal-to-noise ({}) of {} on {}",
                    self.ratio.label(),
                    kit::count(responses.len(), "response", "responses"),
                    kit::count(factors.len(), "factor", "factors")
                );
                kit::ready(
                    SavedReport::SignalToNoise {
                        request: SignalToNoiseRequest { factors, responses, ratio: self.ratio },
                    },
                    summary,
                )
            }
            StudyTool::DeclareDesign => {
                kit::note(
                    ui,
                    "Mark the columns that are factors and the numeric columns that are responses. A numeric \
                     factor's low and high settings start at the data's range (change them from the report's \
                     options menu); a categorical factor needs two levels, coded −1 and +1 in sorted order.",
                );
                self.columns_picker(ui, table, "Factors", "Responses");
                // Restricted randomization: the plots of a split-plot design.
                egui::Grid::new("declare_plots").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label("Whole plots (split-plot):");
                    kit::column_choice(ui, "declare_wp", table, &mut self.first, any, Some("None"));
                    ui.end_row();
                    ui.label("Subplots (split-split-plot):");
                    kit::column_choice(ui, "declare_sp", table, &mut self.second, any, Some("None"));
                    ui.end_row();
                });
                let (factors, responses) = self.chosen(table);
                if factors.is_empty() {
                    return Err("Choose at least one factor".into());
                }
                if self.second.is_some() && self.first.is_none() {
                    return Err("Choose the whole-plot column for the subplots".into());
                }
                let roles: Vec<usize> = factors.iter().chain(&responses).copied().collect();
                if self.first.into_iter().chain(self.second).any(|c| roles.contains(&c))
                    || self.first == self.second && self.first.is_some()
                {
                    return Err("A plot column cannot also be a factor or response".into());
                }
                let mut request = DesignDeclarationRequest::new(&factors, &responses);
                request.whole_plot = self.first;
                request.subplot = self.second;
                request.model = self.model.clone();
                // Settings given in the report being edited stay with their factors.
                for f in &mut request.factors {
                    if let Some(old) = self.ranges.iter().find(|r| r.column == f.column) {
                        (f.low, f.high) = (old.low, old.high);
                    }
                }
                let summary = format!(
                    "Design of {} and {}",
                    kit::count(factors.len(), "factor", "factors"),
                    kit::count(responses.len(), "response", "responses")
                );
                kit::ready(SavedReport::DeclaredDesign { request }, summary)
            }
        }
    }

    /// Two lists of columns: responses (numeric) on the left, titled
    /// `responses_title`, and factors (any column) on the right. A column
    /// can have only one role.
    fn columns_picker(&mut self, ui: &mut egui::Ui, table: &DataTable, factors_title: &str, responses_title: &str) {
        let (responses, factors) = (self.responses.clone(), self.factors.clone());
        // Fixed widths: a list otherwise takes all the width it is offered.
        let width = kit::em(ui) * 17.0;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.strong(responses_title);
                let allow = |i: usize| kit::is_numeric(table, i) && !factors.get(i).copied().unwrap_or(false);
                kit::column_list(ui, "pick_responses", table, &mut self.responses, allow);
            });
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.strong(factors_title);
                let allow = |i: usize| !responses.get(i).copied().unwrap_or(false);
                kit::column_list(ui, "pick_factors", table, &mut self.factors, allow);
            });
        });
    }

    /// Chosen factor and response columns, in table order.
    fn chosen(&self, table: &DataTable) -> (Vec<usize>, Vec<usize>) {
        let keep = |m: &[bool]| kit::chosen_columns(m).into_iter().filter(|&c| c < table.n_cols()).collect();
        let factors: Vec<usize> = keep(&self.factors);
        let responses: Vec<usize> =
            keep(&self.responses).into_iter().filter(|c| !factors.contains(c) && kit::is_numeric(table, *c)).collect();
        (factors, responses)
    }
}

impl Remember for StudyDialog {
    fn memory_key(&self) -> String {
        self.title().into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        match self.tool {
            StudyTool::Repeatability => {
                if let Some(c) = columns.iter().copied().find(|&c| kit::is_numeric(table, c)) {
                    self.first = Some(c);
                }
            }
            StudyTool::DeclareDesign => self.factors = kit::mask(table.n_cols(), columns),
            _ => {}
        }
    }
}

fn hint(ui: &mut egui::Ui, text: &str) {
    kit::note(ui, text);
    ui.add_space(4.0);
}

fn ratio_choice(ui: &mut egui::Ui, ratio: &mut SignalToNoise) {
    ui.label("Signal-to-noise ratio:");
    for r in SignalToNoise::ALL {
        ui.radio_value(ratio, r, format!("{}   {}", r.label(), r.formula()));
    }
}

fn level(ui: &mut egui::Ui, value: &mut f64) {
    ui.horizontal(|ui| {
        ui.label("Confidence level:");
        ui.add(egui::DragValue::new(value).speed(0.005).range(0.5..=0.999));
    });
}

fn linearity_settings(ui: &mut egui::Ui, r: &mut LinearityRequest) {
    level(ui, &mut r.confidence);
    ui.horizontal(|ui| {
        let mut used = r.process_variation.is_some();
        ui.checkbox(&mut used, "Process variation (6σ of the process):");
        let mut v = r.process_variation.unwrap_or(1.0);
        ui.add_enabled(used, egui::DragValue::new(&mut v).speed(0.01).range(1e-12..=f64::MAX));
        r.process_variation = used.then_some(v);
    });
}

fn repeatability_settings(ui: &mut egui::Ui, r: &mut RepeatabilityRequest) {
    egui::Grid::new("rep_settings").num_columns(2).show(ui, |ui| repeatability_part(ui, r));
    repeatability_indices(ui, r);
    repeatability_sections(ui, r);
}

/// The drift section and the statistics of the Summary Statistics table.
fn repeatability_sections(ui: &mut egui::Ui, r: &mut RepeatabilityRequest) {
    ui.checkbox(&mut r.show_drift, "Drift over time or order");
    ui.label("Summary statistics:");
    ui.horizontal_wrapped(|ui| {
        for stat in SummaryStat::ALL {
            let mut on = r.summary.contains(stat);
            if ui.checkbox(&mut on, stat.label()).changed() {
                r.summary.set(stat, on);
            }
        }
    });
}

/// The reference part's value and the tolerance (rows of a two-column grid).
fn repeatability_part(ui: &mut egui::Ui, r: &mut RepeatabilityRequest) {
    ui.label("Reference value of the part:");
    ui.add(egui::DragValue::new(&mut r.reference).speed(0.001));
    ui.end_row();
    ui.label("Tolerance T (USL − LSL):");
    ui.add(egui::DragValue::new(&mut r.tolerance).speed(0.001).range(1e-12..=f64::MAX));
    ui.end_row();
}

/// The constants of the Cg and Cgk indices and the confidence level.
fn repeatability_indices(ui: &mut egui::Ui, r: &mut RepeatabilityRequest) {
    egui::Grid::new("rep_indices").num_columns(2).show(ui, |ui| {
        ui.label("Share of T allowed for the gauge, K (%):");
        ui.add(egui::DragValue::new(&mut r.percent_of_tolerance).speed(0.5).range(1.0..=100.0));
        ui.end_row();
        ui.label("Spread of the gauge, L (σ):");
        ui.add(egui::DragValue::new(&mut r.spread).speed(0.5).range(1.0..=12.0));
        ui.end_row();
    });
    level(ui, &mut r.confidence);
}

fn icc_settings(ui: &mut egui::Ui, r: &mut IccRequest) {
    level(ui, &mut r.confidence);
    ui.horizontal(|ui| {
        let mut given = r.increment.is_some();
        ui.checkbox(&mut given, "Measurement increment:");
        let mut v = r.increment.unwrap_or(0.01);
        ui.add_enabled(given, egui::DragValue::new(&mut v).speed(0.001).range(1e-12..=f64::MAX));
        r.increment = given.then_some(v);
        if !given {
            ui.label(RichText::new("(from the decimals of the data)").weak());
        }
    });
}

/// In-report settings of the reports this module sets up; `false` for any
/// other report.
pub fn report_settings(ui: &mut egui::Ui, saved: &mut SavedReport) -> bool {
    match saved {
        SavedReport::GaugeLinearity { request } => linearity_settings(ui, request),
        SavedReport::GaugeRepeatability { request } => repeatability_settings(ui, request),
        SavedReport::IntraclassCorrelation { request } => icc_settings(ui, request),
        SavedReport::SignalToNoise { request } => ratio_choice(ui, &mut request.ratio),
        SavedReport::DeclaredDesign { request } => declared_settings(ui, request),
        _ => {
            return crate::agreement_dialog::report_settings(ui, saved)
                || crate::tabulation_dialog::report_settings(ui, saved)
                || crate::evaluation_dialog::report_settings(ui, saved)
                || crate::factorial_dialog::report_settings(ui, saved);
        }
    }
    true
}

/// Low and high settings of each declared factor (unset: the data's range).
fn declared_settings(ui: &mut egui::Ui, r: &mut DesignDeclarationRequest) {
    ui.label("Factor settings coded −1 and +1 (unchecked: the data's smallest and largest values):");
    egui::Grid::new("declared_ranges").num_columns(3).show(ui, |ui| {
        for (i, f) in r.factors.iter_mut().enumerate() {
            let DeclaredFactor { low, high, .. } = f;
            ui.label(format!("Factor {}", i + 1));
            for (label, value) in [("Low", low), ("High", high)] {
                ui.horizontal(|ui| {
                    let mut used = value.is_some();
                    ui.checkbox(&mut used, label);
                    let mut v = value.unwrap_or(if label == "Low" { -1.0 } else { 1.0 });
                    ui.add_enabled(used, egui::DragValue::new(&mut v).speed(0.01));
                    *value = used.then_some(v);
                });
            }
            ui.end_row();
        }
    });
}

// ---------------------------------------------------------------------------
// Dialogs that create tables
// ---------------------------------------------------------------------------

/// Which table the sheet dialog creates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetKind {
    GaugeWorksheet,
    RobustDesign,
}

/// Editable factor: name and comma-separated levels.
#[derive(Clone)]
struct FactorRow {
    name: String,
    levels: String,
}

#[derive(Clone)]
pub struct SheetDialog {
    /// Opened from a menu that cannot restore remembered settings
    /// ([`SheetDialog::opening`]).
    recall: bool,
    kind: SheetKind,
    worksheet: GaugeWorksheetRequest,
    operators: String,
    randomize: bool,
    random_seed: u64,
    inner: StandardArray,
    control: Vec<FactorRow>,
    use_outer: bool,
    outer: StandardArray,
    noise: Vec<FactorRow>,
    replicates: usize,
    response: String,
}

impl SheetDialog {
    pub fn new(kind: SheetKind) -> Self {
        let rows = |names: Vec<String>, levels: &[usize]| {
            names
                .into_iter()
                .zip(levels)
                .map(|(name, &l)| FactorRow {
                    name,
                    levels: (1..=l).map(|i| i.to_string()).collect::<Vec<_>>().join(", "),
                })
                .collect()
        };
        let inner = StandardArray::L8;
        let outer = StandardArray::L4;
        Self {
            recall: false,
            kind,
            worksheet: GaugeWorksheetRequest::default(),
            operators: "A, B, C".to_owned(),
            randomize: false,
            random_seed: 1,
            inner,
            control: rows(default_factor_names(3), &inner.levels()),
            use_outer: true,
            outer,
            noise: rows(vec!["N1".into(), "N2".into()], &outer.levels()),
            replicates: 3,
            response: "Y".to_owned(),
        }
    }

    /// A new dialog from a menu that cannot reach the remembered settings:
    /// the app swaps them in before showing it ([`Self::needs_recall`]).
    pub fn opening(kind: SheetKind) -> Self {
        Self { recall: true, ..Self::new(kind) }
    }

    /// Whether the app should restore remembered settings into this dialog
    /// (once; this clears the mark).
    pub fn needs_recall(&mut self) -> bool {
        std::mem::take(&mut self.recall)
    }

    fn title(&self) -> &'static str {
        match self.kind {
            SheetKind::GaugeWorksheet => "Gauge study worksheet",
            SheetKind::RobustDesign => "Robust design (orthogonal arrays)",
        }
    }

    /// The new table and a one-line description.
    pub fn show(&mut self, ctx: &egui::Context) -> Outcome<(DataTable, String)> {
        let dialog = Dialog::new(self.title()).creates().default_width(460.0);
        let max_height = ctx.screen_rect().height() * 0.7;
        let outcome = dialog.show(ctx, |ui| {
            let built = egui::ScrollArea::vertical()
                .max_height(max_height)
                .show(ui, |ui| match self.kind {
                    SheetKind::GaugeWorksheet => self.worksheet_ui(ui),
                    SheetKind::RobustDesign => self.robust_ui(ui),
                })
                .inner;
            match built {
                Ok((table, description)) => {
                    let summary = format!("{description}, {}", kit::count(table.n_rows(), "run", "runs"));
                    kit::ready((table, description), summary)
                }
                Err(e) => Err(e.trim_end_matches('.').to_owned()),
            }
        });
        match outcome {
            Outcome::Reset => {
                *self = Self::new(self.kind);
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn worksheet_ui(&mut self, ui: &mut egui::Ui) -> Result<(DataTable, String), String> {
        hint(
            ui,
            "A run sheet listing every measurement to take, in random order, with an empty measurement column. \
             A crossed sheet can be analysed with Gauge R&R and Intraclass correlation once filled in.",
        );
        let w = &mut self.worksheet;
        ui.label("Layout:");
        ui.radio_value(&mut w.layout, GaugeLayout::Crossed, "Crossed: every operator measures every part");
        ui.radio_value(
            &mut w.layout,
            GaugeLayout::Nested,
            "Nested: each operator has their own parts (destructive tests)",
        );
        ui.add_space(4.0);
        egui::Grid::new("ws_grid").num_columns(2).show(ui, |ui| {
            ui.label(if w.layout == GaugeLayout::Crossed { "Parts:" } else { "Parts per operator:" });
            ui.add(egui::DragValue::new(&mut w.parts).range(2..=500));
            ui.end_row();
            ui.label("Operators (comma-separated):");
            ui.add(egui::TextEdit::singleline(&mut self.operators).desired_width(kit::em(ui) * 14.0));
            ui.end_row();
            ui.label("Measurements of each part per operator:");
            ui.add(egui::DragValue::new(&mut w.replicates).range(1..=20));
            ui.end_row();
            ui.label("Measurement column:");
            ui.add(egui::TextEdit::singleline(&mut w.measurement).desired_width(kit::em(ui) * 10.0));
            ui.end_row();
        });
        kit::more_options(ui, |ui| {
            ui.label("Run order:");
            ui.radio_value(
                &mut w.order,
                RunOrder::RandomWithinReplicate,
                "Random within each replicate (every part once before any repeat)",
            );
            ui.radio_value(&mut w.order, RunOrder::Random, "Completely random");
            ui.radio_value(&mut w.order, RunOrder::Standard, "Standard order");
            if w.order != RunOrder::Standard {
                ui.horizontal(|ui| {
                    ui.label("Seed:");
                    ui.add(crate::dialog_kit::seed_value(&mut w.seed));
                });
            }
        });
        w.operators = self.operators.split(',').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect();
        let layout = if w.layout == GaugeLayout::Crossed { "crossed" } else { "nested" };
        create_gauge_worksheet(w)
            .map(|t| {
                let d = format!("Gauge study worksheet ({layout}, {} operators)", w.operators.len());
                (t, d)
            })
            .map_err(|e| e.to_string())
    }

    fn robust_ui(&mut self, ui: &mut egui::Ui) -> Result<(DataTable, String), String> {
        hint(
            ui,
            "Control factors go in the columns of an inner orthogonal array; each run is repeated under the noise \
             conditions of an outer array (or replicated). Levels are comma-separated; numbers make numeric columns.",
        );
        array_section(ui, "Inner array (control factors):", "inner", &mut self.inner, &mut self.control, "Factor");
        ui.separator();
        ui.checkbox(&mut self.use_outer, "Outer array of noise factors");
        if self.use_outer {
            array_section(ui, "Outer array:", "outer", &mut self.outer, &mut self.noise, "N");
        } else {
            ui.horizontal(|ui| {
                ui.label("Response columns (replicates):");
                ui.add(egui::DragValue::new(&mut self.replicates).range(1..=50));
            });
        }
        kit::more_options(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Response name:");
                ui.add(egui::TextEdit::singleline(&mut self.response).desired_width(kit::em(ui) * 9.0));
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.randomize, "Randomize run order, seed");
                ui.add_enabled(self.randomize, crate::dialog_kit::seed_value(&mut self.random_seed));
            });
        });
        let factors = |rows: &[FactorRow]| -> Vec<ArrayFactor> {
            rows.iter()
                .map(|r| ArrayFactor {
                    name: r.name.trim().to_owned(),
                    levels: r.levels.split(',').map(|l| l.trim().to_owned()).collect(),
                })
                .collect()
        };
        let request = RobustDesignRequest {
            inner: self.inner,
            control: factors(&self.control),
            outer: self.use_outer.then_some(self.outer),
            noise: factors(&self.noise),
            replicates: self.replicates,
            response: self.response.clone(),
            randomize: self.randomize.then_some(self.random_seed),
        };
        create_robust_design(&request).map_err(|e| e.to_string())
    }
}

impl Remember for SheetDialog {
    fn memory_key(&self) -> String {
        self.title().into()
    }

    fn set_edit(&mut self, _report: Option<u64>) {}
}

/// Array choice and its factor rows (number of factors, names, levels).
fn array_section(
    ui: &mut egui::Ui,
    label: &str,
    id: &str,
    array: &mut StandardArray,
    rows: &mut Vec<FactorRow>,
    prefix: &str,
) {
    let before = *array;
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(format!("{id}_array")).selected_text(array.label()).show_list(ui, |ui| {
            for a in StandardArray::ALL {
                ui.selectable_value(array, a, format!("{} — {} runs", a.label(), a.runs()));
            }
        });
    });
    let levels = array.levels();
    let default_levels = |l: usize| (1..=l).map(|i| i.to_string()).collect::<Vec<_>>().join(", ");
    if *array != before {
        // A new array: each factor takes its new column's number of levels.
        for (r, &l) in rows.iter_mut().zip(&levels) {
            r.levels = default_levels(l);
        }
    }
    let mut k = rows.len().min(levels.len());
    ui.horizontal(|ui| {
        ui.label("Number of factors:");
        ui.add(egui::DragValue::new(&mut k).range(1..=levels.len()));
        ui.label(RichText::new(format!("(at most {})", levels.len())).weak());
    });
    rows.truncate(k);
    while rows.len() < k {
        let levels = default_levels(levels[rows.len()]);
        rows.push(FactorRow { name: format!("{prefix}{}", rows.len() + 1), levels });
    }
    egui::Grid::new(format!("{id}_factors")).num_columns(3).striped(true).show(ui, |ui| {
        ui.strong("Column");
        ui.strong("Name");
        ui.strong("Levels");
        ui.end_row();
        for (j, r) in rows.iter_mut().enumerate() {
            ui.label(format!("{} ({} levels)", j + 1, levels[j]));
            ui.add(egui::TextEdit::singleline(&mut r.name).desired_width(kit::em(ui) * 9.0));
            ui.add(egui::TextEdit::singleline(&mut r.levels).desired_width(kit::em(ui) * 11.0));
            ui.end_row();
        }
    });
}
