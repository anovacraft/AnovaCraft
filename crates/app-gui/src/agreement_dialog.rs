//! Dialogs for the attribute measurement studies: the attribute agreement
//! study of appraisers' categorical ratings and the go/no-go gauge study of
//! bias and repeatability ([`app_model::attribute_agreement`]).
//!
//! Both reports run through [`app_model::ComparisonReport`], so their
//! settings are also edited in place from the report's options menu
//! ([`report_settings`]).

use crate::dialog_kit::ComboExt as _;
use app_model::{AgreementRequest, AttributeGaugeRequest, SavedReport, agreement_categories};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::report_view::ReportRequest;

/// Which study the dialog sets up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgreementTool {
    /// Agreement of appraisers' ratings.
    Agreement,
    /// Go/no-go gauge bias and repeatability.
    Gauge,
}

/// Dialog for the attribute studies.
#[derive(Clone)]
pub struct AgreementDialog {
    edit: Option<u64>,
    tool: AgreementTool,
    part: Option<usize>,
    appraiser: Option<usize>,
    rating: Option<usize>,
    trial: Option<usize>,
    standard: Option<usize>,
    confidence: f64,
    ordinal: bool,
    /// The rating column the order and ordinal choice were set up for.
    order_for: Option<usize>,
    order: Vec<String>,
    reference: Option<usize>,
    acceptance: Option<usize>,
    checks: Option<usize>,
    per_check: bool,
    accept_label: String,
    limit: f64,
}

impl AgreementDialog {
    /// A dialog for `tool`, pre-selecting columns whose names suggest their role.
    pub fn new(tool: AgreementTool, table: &DataTable) -> Self {
        let guess = |words: &[&str], numeric: bool| {
            table.columns().iter().enumerate().position(|(i, c)| {
                let name = c.name.to_lowercase();
                words.iter().any(|w| name.contains(w)) && (!numeric || kit::is_numeric(table, i))
            })
        };
        let standard = guess(&["standard", "reference", "known"], false);
        let rating = table.columns().iter().enumerate().position(|(i, c)| {
            let name = c.name.to_lowercase();
            ["result", "rating", "grade", "decision"].iter().any(|w| name.contains(w)) && Some(i) != standard
        });
        Self {
            edit: None,
            tool,
            part: guess(&["part", "item", "sample"], false),
            appraiser: guess(&["appraiser", "inspector", "operator", "rater"], false),
            rating,
            trial: guess(&["trial", "replicate", "repeat"], false),
            standard,
            confidence: 0.95,
            ordinal: false,
            order_for: None,
            order: Vec::new(),
            reference: guess(&["reference", "standard", "size"], true),
            acceptance: guess(&["accept", "pass", "result"], false),
            checks: guess(&["check", "trial", "attempt"], true),
            per_check: false,
            accept_label: String::new(),
            limit: 0.0,
        }
    }

    /// The dialog filled from a saved attribute agreement or gauge report.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let valid = |c: usize| kit::valid(table, c);
        let d = match saved {
            SavedReport::AttributeAgreement { request } => Self {
                part: valid(request.part),
                appraiser: valid(request.appraiser),
                rating: valid(request.rating),
                trial: request.trial.and_then(valid),
                standard: request.standard.and_then(valid),
                confidence: request.confidence,
                ordinal: request.ordinal,
                order_for: valid(request.rating),
                order: request.order.clone(),
                ..Self::new(AgreementTool::Agreement, table)
            },
            SavedReport::AttributeGauge { request } => Self {
                reference: valid(request.reference),
                acceptance: valid(request.acceptance),
                checks: request.checks.and_then(valid),
                per_check: request.checks.is_none(),
                accept_label: request.accept_label.clone(),
                limit: request.limit,
                ..Self::new(AgreementTool::Gauge, table)
            },
            _ => return None,
        };
        Some(d)
    }

    fn title(&self) -> &'static str {
        match self.tool {
            AgreementTool::Agreement => "Attribute agreement",
            AgreementTool::Gauge => "Attribute gauge bias and repeatability",
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<Box<ReportRequest>> {
        let dialog = Dialog::new(self.title()).editing(self.edit);
        let outcome = dialog.show(ctx, |ui| {
            let ready = match self.tool {
                AgreementTool::Agreement => self.agreement(ui, table),
                AgreementTool::Gauge => self.gauge(ui, table),
            };
            ready.map(|r| kit::Ready { output: Box::new(ReportRequest::Comparison(r.output)), summary: r.summary })
        });
        match outcome {
            Outcome::Reset => {
                let fresh = Self::new(self.tool, table);
                *self = Self {
                    edit: self.edit,
                    part: None,
                    appraiser: None,
                    rating: None,
                    trial: None,
                    standard: None,
                    reference: None,
                    acceptance: None,
                    checks: None,
                    ..fresh
                };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn agreement(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "One row per rating: the part, the appraiser and the category given, with the trial and the part's \
             known standard category in optional columns. Every appraiser rates every part equally often.",
        );
        let any = |_: usize| true;
        egui::Grid::new("agreement_cols").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Part:");
            kit::column_choice(ui, "agr_part", table, &mut self.part, any, None);
            ui.end_row();
            ui.label("Appraiser:");
            kit::column_choice(ui, "agr_appraiser", table, &mut self.appraiser, any, None);
            ui.end_row();
            ui.label("Rating:");
            kit::column_choice(ui, "agr_rating", table, &mut self.rating, any, None);
            ui.end_row();
            ui.label("Trial:");
            kit::column_choice(ui, "agr_trial", table, &mut self.trial, any, Some("None: trials in row order"));
            ui.end_row();
            ui.label("Standard:");
            kit::column_choice(ui, "agr_standard", table, &mut self.standard, any, Some("None: no known standard"));
            ui.end_row();
        });
        kit::more_options(ui, |ui| level(ui, &mut self.confidence));
        let Some(part) = self.part else { return Err("Choose a part column".into()) };
        let Some(appraiser) = self.appraiser else { return Err("Choose an appraiser column".into()) };
        let Some(rating) = self.rating else { return Err("Choose a rating column".into()) };
        let mut request = AgreementRequest::new(table, part, appraiser, rating);
        if self.order_for != Some(rating) {
            // A new rating column: ordered when its level says so, natural order.
            self.order_for = Some(rating);
            self.ordinal = request.ordinal;
            self.order.clear();
        }
        request.trial = self.trial;
        request.standard = self.standard;
        if !kit::all_different(&request.columns().into_iter().map(Some).collect::<Vec<_>>()) {
            return Err("A column is chosen twice".into());
        }
        // Keep the order chosen so far; add categories of a newly chosen standard.
        request.order = std::mem::take(&mut self.order);
        self.order = agreement_categories(table, &request).map_err(|e| e.to_string())?;
        ui.checkbox(&mut self.ordinal, "Ordered categories (weighted kappa, Kendall's W and τ_b)");
        if self.ordinal {
            order_list(ui, &mut self.order);
        }
        request.confidence = self.confidence;
        request.ordinal = self.ordinal;
        request.order = if self.ordinal { self.order.clone() } else { Vec::new() };
        let summary = format!(
            "Agreement of {} ratings of {} by {}",
            kit::name(table, rating),
            kit::name(table, part),
            kit::name(table, appraiser)
        );
        kit::ready(SavedReport::AttributeAgreement { request }, summary)
    }

    fn gauge(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "Parts of known reference value near one specification limit, each checked repeatedly with the \
             gauge (20 times in the AIAG design).",
        );
        let numeric = |i: usize| kit::is_numeric(table, i);
        let any = |_: usize| true;
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.per_check, false, "Counts: accepted and checks per row");
            ui.radio_value(&mut self.per_check, true, "One row per check");
        });
        egui::Grid::new("gauge_cols").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Reference value:");
            kit::column_choice(ui, "ag_ref", table, &mut self.reference, numeric, None);
            ui.end_row();
            if self.per_check {
                ui.label("Result of each check:");
                kit::column_choice(ui, "ag_result", table, &mut self.acceptance, any, None);
                ui.end_row();
                ui.label("Result meaning accepted:");
                let labels = self.acceptance.map(|c| distinct(table, c)).unwrap_or_default();
                if !labels.contains(&self.accept_label) {
                    self.accept_label = labels.first().cloned().unwrap_or_default();
                }
                egui::ComboBox::from_id_salt("ag_label").selected_text(self.accept_label.clone()).show_list(ui, |ui| {
                    for l in labels {
                        ui.selectable_value(&mut self.accept_label, l.clone(), l);
                    }
                });
                ui.end_row();
            } else {
                ui.label("Times accepted:");
                kit::column_choice(ui, "ag_acc", table, &mut self.acceptance, numeric, None);
                ui.end_row();
                ui.label("Times checked:");
                kit::column_choice(ui, "ag_checks", table, &mut self.checks, numeric, None);
                ui.end_row();
            }
        });
        ui.horizontal(|ui| {
            ui.label("Specification limit the gauge checks:");
            ui.add(egui::DragValue::new(&mut self.limit).speed(0.001).max_decimals(6));
        });
        let Some(reference) = self.reference else { return Err("Choose the reference-value column".into()) };
        let Some(acceptance) = self.acceptance else {
            return Err(if self.per_check {
                "Choose the column with each check's result".into()
            } else {
                "Choose the times-accepted column".into()
            });
        };
        let mut request = AttributeGaugeRequest::counts(reference, acceptance, self.checks.unwrap_or(0), self.limit);
        if self.per_check {
            if self.accept_label.is_empty() {
                return Err("The result column has no values".into());
            }
            request.checks = None;
            request.accept_label = self.accept_label.clone();
        } else {
            let Some(checks) = self.checks else { return Err("Choose the times-checked column".into()) };
            request.checks = Some(checks);
        }
        if !kit::all_different(&request.columns().into_iter().map(Some).collect::<Vec<_>>()) {
            return Err("A column is chosen twice".into());
        }
        let summary = format!("Go/no-go gauge study of {} at limit {}", kit::name(table, reference), self.limit);
        kit::ready(SavedReport::AttributeGauge { request }, summary)
    }
}

impl Remember for AgreementDialog {
    fn memory_key(&self) -> String {
        self.title().into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }
}

fn distinct(table: &DataTable, c: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(col) = table.column(c) {
        for r in 0..table.n_rows() {
            let t = col.level_text(r);
            if !t.is_empty() && !out.contains(&t) {
                out.push(t);
            }
        }
    }
    out
}

fn level(ui: &mut egui::Ui, value: &mut f64) {
    ui.horizontal(|ui| {
        ui.label("Confidence level:");
        ui.add(egui::DragValue::new(value).speed(0.005).range(0.5..=0.999));
    });
}

/// Categories lowest first, each with buttons to move it.
fn order_list(ui: &mut egui::Ui, order: &mut [String]) {
    ui.label("Category order (lowest first):");
    let mut swap = None;
    egui::ScrollArea::vertical().id_salt("agreement_order").max_height(140.0).show(ui, |ui| {
        let n = order.len();
        for (k, level) in order.iter().enumerate() {
            ui.horizontal(|ui| {
                // Named for screen readers (and tests): the arrows alone say nothing.
                let named = |response: egui::Response, name: String| {
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name.clone()));
                    response.on_hover_text(name)
                };
                let up = ui.add_enabled(k > 0, egui::Button::new("▲").small());
                if named(up, format!("Move {level} lower")).clicked() {
                    swap = Some(k - 1);
                }
                let down = ui.add_enabled(k + 1 < n, egui::Button::new("▼").small());
                if named(down, format!("Move {level} higher")).clicked() {
                    swap = Some(k);
                }
                ui.label(format!("{}. {level}", k + 1));
            });
        }
    });
    if let Some(k) = swap {
        order.swap(k, k + 1);
    }
}

fn gauge_settings(ui: &mut egui::Ui, r: &mut AttributeGaugeRequest) {
    ui.horizontal(|ui| {
        ui.label("Specification limit the gauge checks:");
        ui.add(egui::DragValue::new(&mut r.limit).speed(0.001).max_decimals(6));
    });
}

fn agreement_settings(ui: &mut egui::Ui, r: &mut AgreementRequest) {
    ui.checkbox(&mut r.ordinal, "Ordered categories (weighted kappa, Kendall's W and τ_b)");
    level(ui, &mut r.confidence);
}

/// In-report settings of the reports this module sets up; `false` for any
/// other report.
pub fn report_settings(ui: &mut egui::Ui, saved: &mut SavedReport) -> bool {
    match saved {
        SavedReport::AttributeAgreement { request } => agreement_settings(ui, request),
        SavedReport::AttributeGauge { request } => gauge_settings(ui, request),
        _ => return false,
    }
    true
}
