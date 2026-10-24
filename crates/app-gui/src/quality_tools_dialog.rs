//! Dialogs and in-report settings for attribute capability, between/within
//! capability and acceptance sampling. These reports run through
//! [`app_model::ComparisonReport`], so their settings are edited in place from
//! the report's options menu (see [`report_settings`]).

use crate::dialog_kit::ComboExt as _;
use app_model::{
    AcceptanceRequest, AttributeCapabilityKind, AttributeCapabilityRequest, BetweenEstimate, BetweenWithinRequest,
    OcModel, PlanDesign, SamplingPlan, SavedReport, SigmaKnowledge, SpecLimits, SubgroupBy, SubgroupSize,
    WithinEstimate,
};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::report_view::ReportRequest;

/// Which report the dialog sets up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityTool {
    AttributeCapability,
    BetweenWithin,
    Acceptance,
}

#[derive(Clone)]
pub struct QualityToolDialog {
    edit: Option<u64>,
    tool: QualityTool,
    count: Option<usize>,
    size_from_column: bool,
    size_column: Option<usize>,
    size_constant: f64,
    attribute: AttributeCapabilityRequest,
    column: Option<usize>,
    by_column: bool,
    subgroup_column: Option<usize>,
    subgroup_size: usize,
    between: BetweenWithinRequest,
    acceptance: AcceptanceRequest,
}

/// Whether column `c` holds only whole, non-negative numbers (counts of
/// nonconforming units or defects).
fn is_count(table: &DataTable, c: usize) -> bool {
    table
        .column(c)
        .and_then(|c| c.as_numeric())
        .is_some_and(|v| v.iter().filter(|x| x.is_finite()).all(|&x| x >= 0.0 && x.fract() == 0.0))
}

impl QualityToolDialog {
    pub fn new(tool: QualityTool) -> Self {
        Self {
            edit: None,
            tool,
            count: None,
            size_from_column: true,
            size_column: None,
            size_constant: 100.0,
            attribute: AttributeCapabilityRequest::new(AttributeCapabilityKind::Binomial, 0, SubgroupSize::None),
            column: None,
            by_column: true,
            subgroup_column: None,
            subgroup_size: 5,
            between: BetweenWithinRequest {
                column: 0,
                subgroup: SubgroupBy::Size(5),
                limits: SpecLimits { lower: Some(0.0), upper: Some(1.0), target: None },
                within: WithinEstimate::PooledStdDev,
                between: BetweenEstimate::MovingRange,
            },
            acceptance: AcceptanceRequest::default(),
        }
    }

    /// The dialog filled from a saved attribute capability, between/within
    /// capability or acceptance sampling report.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let d = match saved {
            SavedReport::AttributeCapability { request } => {
                let mut d = Self::new(QualityTool::AttributeCapability);
                d.count = kit::valid(table, request.count);
                match request.size {
                    SubgroupSize::Column(c) => d.size_column = kit::valid(table, c),
                    SubgroupSize::Constant(n) => (d.size_from_column, d.size_constant) = (false, n),
                    SubgroupSize::None => {}
                }
                d.attribute = *request;
                d
            }
            SavedReport::BetweenWithinCapability { request } => {
                let mut d = Self::new(QualityTool::BetweenWithin);
                d.column = kit::valid(table, request.column);
                match request.subgroup {
                    SubgroupBy::Column(c) => d.subgroup_column = kit::valid(table, c),
                    SubgroupBy::Size(n) => (d.by_column, d.subgroup_size) = (false, n),
                }
                d.between = *request;
                d
            }
            SavedReport::AcceptanceSampling { request } => {
                Self { acceptance: request.clone(), ..Self::new(QualityTool::Acceptance) }
            }
            _ => return None,
        };
        Some(d)
    }

    fn title(&self) -> &'static str {
        match self.tool {
            QualityTool::AttributeCapability => "Attribute capability",
            QualityTool::BetweenWithin => "Between/within capability",
            QualityTool::Acceptance => "Acceptance sampling plan",
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<Box<ReportRequest>> {
        let mut dialog = Dialog::new(self.title()).editing(self.edit);
        if self.tool == QualityTool::Acceptance {
            dialog = dialog.without_rows();
        }
        let outcome = dialog.show(ctx, |ui| {
            let saved = match self.tool {
                QualityTool::AttributeCapability => self.attribute_ui(ui, table),
                QualityTool::BetweenWithin => self.between_ui(ui, table),
                QualityTool::Acceptance => self.acceptance_ui(ui),
            };
            saved.map(|r| kit::Ready { output: Box::new(ReportRequest::Comparison(r.output)), summary: r.summary })
        });
        match outcome {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(self.tool) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn attribute_ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(ui, "Percent defective or defects per unit with exact intervals, and a chart of stability.");
        if self.count.is_some_and(|c| !is_count(table, c)) {
            self.count = None;
        }
        for kind in AttributeCapabilityKind::ALL {
            ui.radio_value(&mut self.attribute.kind, kind, kind.label());
        }
        let binomial = self.attribute.kind == AttributeCapabilityKind::Binomial;
        ui.add_space(4.0);
        ui.label(if binomial {
            "Defective units per sample (one row each):"
        } else {
            "Defects per sample (one row each):"
        });
        kit::column_choice(ui, "aq_count", table, &mut self.count, |i| is_count(table, i), None);
        ui.add_space(4.0);
        ui.label("Units inspected per sample:");
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.size_from_column, true, "From a column");
            let allow = |i: usize| if binomial { is_count(table, i) } else { kit::is_numeric(table, i) };
            if kit::column_choice(ui, "aq_size", table, &mut self.size_column, allow, None) {
                self.size_from_column = true;
            }
        });
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.size_from_column, false, "The same for every sample:");
            ui.add(egui::DragValue::new(&mut self.size_constant).range(1.0..=1e9).speed(1.0));
        });
        kit::more_options(ui, |ui| attribute_settings(ui, &mut self.attribute, false));
        let Some(count) = self.count else { return Err("Choose a count column".into()) };
        let size = if self.size_from_column {
            match self.size_column {
                Some(c) if c == count => {
                    return Err(kit::duplicate_role(
                        table,
                        &[("the count column", Some(count)), ("the units-inspected column", Some(c))],
                    )
                    .unwrap_or_default());
                }
                Some(c) => SubgroupSize::Column(c),
                None => return Err("Choose the column of units inspected".into()),
            }
        } else {
            SubgroupSize::Constant(if binomial { self.size_constant.round().max(1.0) } else { self.size_constant })
        };
        let model = if binomial { "Binomial" } else { "Poisson" };
        let mut summary = format!("{model} capability of {}", kit::name(table, count));
        match size {
            SubgroupSize::Column(c) => summary.push_str(&format!(", units inspected from {}", kit::name(table, c))),
            SubgroupSize::Constant(n) => summary.push_str(&format!(", {n} units per sample")),
            SubgroupSize::None => {}
        }
        let request = AttributeCapabilityRequest { count, size, ..self.attribute };
        kit::ready(SavedReport::AttributeCapability { request }, summary)
    }

    fn between_ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "Capability of a process run in batches or parallel streams, with the batch-to-batch variation \
             separated from the variation within a batch.",
        );
        egui::Grid::new("bw_columns").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Measurement:");
            kit::column_choice(ui, "bw_column", table, &mut self.column, |i| kit::is_numeric(table, i), None);
            ui.end_row();
        });
        ui.label("Subgroups (batches or streams):");
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.by_column, true, "Rows with the same value of");
            let measurement = self.column;
            let allow = |i: usize| Some(i) != measurement;
            if kit::column_choice(ui, "bw_group", table, &mut self.subgroup_column, allow, None) {
                self.by_column = true;
            }
        });
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.by_column, false, "Consecutive rows, this many at a time:");
            ui.add(egui::DragValue::new(&mut self.subgroup_size).range(2..=1000));
        });
        ui.add_space(4.0);
        between_limits(ui, &mut self.between);
        kit::more_options(ui, |ui| between_estimators(ui, &mut self.between));
        let Some(column) = self.column else { return Err("Choose a measurement column".into()) };
        let subgroup = if self.by_column {
            match self.subgroup_column {
                Some(c) if c == column => return Err("Measurement and subgroup must be different columns".into()),
                Some(c) => SubgroupBy::Column(c),
                None => return Err("Choose the subgroup column".into()),
            }
        } else {
            SubgroupBy::Size(self.subgroup_size)
        };
        let limits = self.between.limits;
        if limits.lower.is_none() && limits.upper.is_none() {
            return Err("Set a lower or an upper specification limit".into());
        }
        let groups = match subgroup {
            SubgroupBy::Column(c) => format!("by {}", kit::name(table, c)),
            SubgroupBy::Size(n) => format!("of {n}"),
        };
        let summary = format!("Between/within capability of {} in subgroups {groups}", kit::name(table, column));
        kit::ready(
            SavedReport::BetweenWithinCapability { request: BetweenWithinRequest { column, subgroup, ..self.between } },
            summary,
        )
    }

    fn acceptance_ui(&mut self, ui: &mut egui::Ui) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "Designs a lot-by-lot inspection plan from the producer's and consumer's risk points \
             and draws its OC, AOQ and ATI curves. Uses no data columns.",
        );
        let r = &mut self.acceptance;
        acceptance_design(ui, r);
        ui.separator();
        acceptance_plans(ui, r);
        kit::more_options(ui, |ui| acceptance_model(ui, r));
        if r.design == PlanDesign::None && r.plans.is_empty() {
            return Err("Add a plan to compare".into());
        }
        let design = DESIGNS.iter().find(|d| same_design(d.1, r.design)).map_or("", |d| d.0);
        let mut summary = if r.design == PlanDesign::None {
            String::from("OC curves")
        } else {
            format!("{design} for AQL {} and LTPD {}", r.risks.aql, r.risks.ltpd)
        };
        if !r.plans.is_empty() {
            let plans = kit::count(r.plans.len(), "plan", "plans");
            if r.design == PlanDesign::None {
                summary.push_str(&format!(" of {plans}"));
            } else {
                summary.push_str(&format!(", compared with {plans}"));
            }
        }
        kit::ready(SavedReport::AcceptanceSampling { request: r.clone() }, summary)
    }
}

impl Remember for QualityToolDialog {
    fn memory_key(&self) -> String {
        self.title().into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        match self.tool {
            QualityTool::AttributeCapability => self.count = columns.iter().copied().find(|&c| is_count(table, c)),
            QualityTool::BetweenWithin => {
                self.column = columns.iter().copied().find(|&c| kit::is_numeric(table, c));
            }
            QualityTool::Acceptance => {}
        }
    }
}

/// An optional limit: a checkbox and its value.
fn optional(ui: &mut egui::Ui, label: &str, value: &mut Option<f64>, fallback: f64) {
    let mut used = value.is_some();
    ui.checkbox(&mut used, label);
    match (used, value.as_mut()) {
        (true, Some(v)) => {
            ui.add(egui::DragValue::new(v).speed(0.01));
        }
        (true, None) => *value = Some(fallback),
        (false, _) => {
            *value = None;
            ui.label("—");
        }
    }
    ui.end_row();
}

/// Settings of an attribute capability study (`in_report`: also the model).
pub fn attribute_settings(ui: &mut egui::Ui, r: &mut AttributeCapabilityRequest, in_report: bool) {
    if in_report {
        for kind in AttributeCapabilityKind::ALL {
            ui.radio_value(&mut r.kind, kind, kind.label());
        }
    }
    ui.horizontal(|ui| {
        ui.label("Confidence level:");
        ui.add(egui::DragValue::new(&mut r.level).speed(0.005).range(0.5..=0.999).fixed_decimals(3));
    });
}

/// Specification limits and σ estimators of a between/within study.
pub fn between_settings(ui: &mut egui::Ui, r: &mut BetweenWithinRequest) {
    between_limits(ui, r);
    between_estimators(ui, r);
}

/// Specification limits of a between/within study.
fn between_limits(ui: &mut egui::Ui, r: &mut BetweenWithinRequest) {
    ui.label("Specification limits (at least one):");
    let l = &mut r.limits;
    let (lo, hi) = (l.lower, l.upper);
    egui::Grid::new("bw_limits").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
        optional(ui, "Lower limit (LSL)", &mut l.lower, hi.map_or(0.0, |u| u - 1.0));
        optional(
            ui,
            "Target",
            &mut l.target,
            match (lo, hi) {
                (Some(a), Some(b)) => 0.5 * (a + b),
                (Some(a), None) | (None, Some(a)) => a,
                (None, None) => 0.0,
            },
        );
        optional(ui, "Upper limit (USL)", &mut l.upper, lo.map_or(1.0, |v| v + 1.0));
    });
}

/// σ estimators of a between/within study.
fn between_estimators(ui: &mut egui::Ui, r: &mut BetweenWithinRequest) {
    ui.label("Between-subgroup σ from:");
    ui.radio_value(
        &mut r.between,
        BetweenEstimate::MovingRange,
        "Moving range of subgroup means (batches in time order)",
    );
    ui.radio_value(
        &mut r.between,
        BetweenEstimate::VarianceComponents,
        "Variance components (parallel streams; order does not matter)",
    );
    ui.add_enabled_ui(r.between == BetweenEstimate::MovingRange, |ui| {
        ui.label("Within-subgroup σ from:");
        ui.radio_value(&mut r.within, WithinEstimate::PooledStdDev, "Pooled standard deviation");
        ui.radio_value(&mut r.within, WithinEstimate::AverageRange, "Average range (subgroups of 2 to 25)");
    });
}

fn whole(ui: &mut egui::Ui, label: &str, v: &mut u64, min: u64) {
    ui.label(label);
    ui.add(egui::DragValue::new(v).range(min..=10_000_000));
}

/// The plans an acceptance sampling report can design.
const DESIGNS: [(&str, PlanDesign); 5] = [
    ("Single attribute plan (n, c)", PlanDesign::Single),
    ("Double attribute plan (n₁, c₁, n₂, c₂)", PlanDesign::Double { second_ratio: 2 }),
    ("Variables plan, σ known (k-method)", PlanDesign::Variables { sigma: SigmaKnowledge::Known }),
    ("Variables plan, σ unknown (k-method)", PlanDesign::Variables { sigma: SigmaKnowledge::Unknown }),
    ("None: compare the plans below", PlanDesign::None),
];

/// The same kind of design (double plans of any ratio are one kind).
fn same_design(a: PlanDesign, b: PlanDesign) -> bool {
    match (a, b) {
        (PlanDesign::Double { .. }, PlanDesign::Double { .. }) => true,
        _ => a == b,
    }
}

/// Settings of an acceptance sampling report (the options menu).
pub fn acceptance_settings(ui: &mut egui::Ui, r: &mut AcceptanceRequest) {
    acceptance_design(ui, r);
    acceptance_model(ui, r);
    ui.separator();
    acceptance_plans(ui, r);
}

/// The plan to design, its risk points and lot size.
fn acceptance_design(ui: &mut egui::Ui, r: &mut AcceptanceRequest) {
    let current = DESIGNS.iter().find(|d| same_design(d.1, r.design)).map_or("", |d| d.0);
    ui.horizontal(|ui| {
        ui.label("Design:");
        let width = kit::em(ui) * 17.0;
        egui::ComboBox::from_id_salt("acceptance_design").selected_text(current).width(width).show_list(ui, |ui| {
            for (name, d) in DESIGNS {
                if ui.selectable_label(same_design(d, r.design), name).clicked() && !same_design(d, r.design) {
                    r.design = d;
                }
            }
        });
    });
    egui::Grid::new("acceptance_risks").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
        let drag = |ui: &mut egui::Ui, label: &str, v: &mut f64, range: std::ops::RangeInclusive<f64>| {
            ui.label(label);
            ui.add(egui::DragValue::new(v).speed(0.001).range(range).max_decimals(5));
            ui.end_row();
        };
        drag(ui, "Producer's quality level p₁ (AQL):", &mut r.risks.aql, 0.0001..=0.9);
        drag(ui, "Producer's risk α:", &mut r.risks.alpha, 0.001..=0.49);
        drag(ui, "Consumer's quality level p₂ (LTPD):", &mut r.risks.ltpd, 0.0002..=0.95);
        drag(ui, "Consumer's risk β:", &mut r.risks.beta, 0.001..=0.49);
        whole(ui, "Lot size N:", &mut r.lot_size, 2);
        ui.end_row();
        if let PlanDesign::Double { second_ratio } = &mut r.design {
            whole(ui, "Second sample ÷ first sample:", second_ratio, 1);
            *second_ratio = (*second_ratio).clamp(1, 5);
            ui.end_row();
        }
    });
}

/// The distribution of the number nonconforming for attribute plans.
fn acceptance_model(ui: &mut egui::Ui, r: &mut AcceptanceRequest) {
    ui.horizontal(|ui| {
        ui.label("Model for attribute plans:");
        let width = kit::em(ui) * 14.0;
        egui::ComboBox::from_id_salt("acceptance_model").selected_text(r.model.label()).width(width).show_list(
            ui,
            |ui| {
                for m in OcModel::ALL {
                    ui.selectable_value(&mut r.model, m, m.label());
                }
            },
        );
    });
}

/// Plans entered for comparison, with buttons to add more.
fn acceptance_plans(ui: &mut egui::Ui, r: &mut AcceptanceRequest) {
    ui.label("Plans to compare (optional):");
    let mut remove = None;
    for (i, plan) in r.plans.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label(format!("Plan {}:", i + 1));
            match plan {
                SamplingPlan::Single { n, c } => {
                    ui.label("n");
                    ui.add(egui::DragValue::new(n).range(1..=10_000_000));
                    ui.label("c");
                    ui.add(egui::DragValue::new(c).range(0..=100_000));
                }
                SamplingPlan::Double { n1, c1, n2, c2 } => {
                    for (name, v) in [("n₁", n1), ("c₁", c1), ("n₂", n2), ("c₂", c2)] {
                        ui.label(name);
                        ui.add(egui::DragValue::new(v).range(0..=10_000_000));
                    }
                }
                SamplingPlan::Variables { n, k, sigma } => {
                    ui.label("n");
                    ui.add(egui::DragValue::new(n).range(1..=10_000_000));
                    ui.label("k");
                    ui.add(egui::DragValue::new(k).speed(0.01).range(-10.0..=10.0));
                    ui.radio_value(sigma, SigmaKnowledge::Known, "σ known");
                    ui.radio_value(sigma, SigmaKnowledge::Unknown, "σ unknown");
                }
            }
            if ui.small_button("Remove").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        r.plans.remove(i);
    }
    ui.horizontal(|ui| {
        if ui.button("Add single plan").clicked() {
            r.plans.push(SamplingPlan::Single { n: 50, c: 1 });
        }
        if ui.button("Add double plan").clicked() {
            r.plans.push(SamplingPlan::Double { n1: 50, c1: 1, n2: 100, c2: 3 });
        }
        if ui.button("Add variables plan").clicked() {
            r.plans.push(SamplingPlan::Variables { n: 15, k: 1.9, sigma: SigmaKnowledge::Known });
        }
    });
}

/// In-report settings of the reports this module sets up; `false` for any
/// other report.
pub fn report_settings(ui: &mut egui::Ui, saved: &mut SavedReport) -> bool {
    match saved {
        SavedReport::AttributeCapability { request } => attribute_settings(ui, request, true),
        SavedReport::BetweenWithinCapability { request } => between_settings(ui, request),
        SavedReport::AcceptanceSampling { request } => acceptance_settings(ui, request),
        _ => {
            return crate::monitoring_dialog::report_settings(ui, saved)
                || crate::study_dialogs::report_settings(ui, saved);
        }
    }
    true
}
