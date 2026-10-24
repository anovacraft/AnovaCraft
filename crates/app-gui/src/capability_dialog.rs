//! Dialog for a process capability study, optionally laid out as a summary
//! page with bootstrap intervals for the non-normal indices.

use crate::dialog_kit::ComboExt as _;
use app_model::{
    BootstrapSettings, CapabilityModel, CapabilityRequest, FitFamily, GroupOrder, SavedReport, SpecLimits, WithinSigma,
};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};

/// One optional limit: whether it is used and its value.
#[derive(Clone, Copy)]
struct Limit {
    used: bool,
    value: f64,
}

#[derive(Clone)]
pub struct CapabilityDialog {
    edit: Option<u64>,
    column: Option<usize>,
    lower: Limit,
    upper: Limit,
    target: Limit,
    /// 0 moving range, 1 average range, 2 pooled SD.
    method: usize,
    subgroup_size: usize,
    /// Process distribution (normal theory, or a non-normal model).
    model: CapabilityModel,
    /// Box–Cox transform first (normal model only).
    transform: bool,
    lambda: f64,
    /// Summary page layout (charts, last subgroups, histogram, probability plot).
    summary: bool,
    /// Bootstrap intervals for the percentile indices (non-normal models).
    bootstrap: bool,
    resamples: usize,
    seed: u64,
    /// Column whose groups are also studied one by one.
    by: Option<usize>,
    group_order: GroupOrder,
    /// Probability plot with the limits in the standard layout.
    probability_plot: bool,
}

impl Default for CapabilityDialog {
    fn default() -> Self {
        Self {
            edit: None,
            column: None,
            lower: Limit { used: true, value: 0.0 },
            upper: Limit { used: true, value: 1.0 },
            target: Limit { used: false, value: 0.5 },
            method: 0,
            subgroup_size: 5,
            model: CapabilityModel::Normal,
            transform: false,
            lambda: 0.0,
            summary: false,
            bootstrap: false,
            resamples: BootstrapSettings::default().resamples,
            seed: BootstrapSettings::default().seed,
            by: None,
            group_order: GroupOrder::Labels,
            probability_plot: false,
        }
    }
}

impl CapabilityDialog {
    /// The dialog with the summary page layout already chosen.
    pub fn summary_page() -> Self {
        Self { summary: true, ..Self::default() }
    }

    /// The dialog filled from a saved capability study.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::Capability { request } = saved else { return None };
        let d = Self::default();
        let limit = |v: Option<f64>, fallback: Limit| {
            v.map_or(Limit { used: false, ..fallback }, |value| Limit { used: true, value })
        };
        let (method, subgroup_size) = match request.within {
            WithinSigma::MovingRange => (0, d.subgroup_size),
            WithinSigma::AverageRange { subgroup_size } => (1, subgroup_size),
            WithinSigma::PooledStdDev { subgroup_size } => (2, subgroup_size),
        };
        Some(Self {
            column: kit::valid(table, request.column),
            lower: limit(request.limits.lower, d.lower),
            upper: limit(request.limits.upper, d.upper),
            target: limit(request.limits.target, d.target),
            method,
            subgroup_size,
            model: request.model,
            transform: request.box_cox.is_some(),
            lambda: request.box_cox.unwrap_or(d.lambda),
            summary: request.summary,
            bootstrap: request.bootstrap.is_some(),
            resamples: request.bootstrap.map_or(d.resamples, |b| b.resamples),
            seed: request.bootstrap.map_or(d.seed, |b| b.seed),
            by: request.by.and_then(|c| kit::valid(table, c)),
            group_order: request.group_order,
            probability_plot: request.probability_plot,
            ..d
        })
    }

    fn title(&self) -> &'static str {
        if self.summary { "Capability summary page" } else { "Process capability" }
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<CapabilityRequest> {
        // The title follows the layout check box, so the window is kept under one id.
        let dialog = Dialog::new(self.title()).id_salt("capability_dialog").editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, summary: self.summary, ..Self::default() };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<CapabilityRequest> {
        ui.label("Measurement (numeric, in production order):");
        kit::column_choice(ui, "capability_column", table, &mut self.column, |i| kit::is_numeric(table, i), None);
        ui.horizontal(|ui| {
            ui.label("Also by group of:");
            kit::column_choice(ui, "capability_by", table, &mut self.by, |_| true, Some("(no groups)"));
        });
        ui.add_space(6.0);
        ui.label("Specification limits (at least one):");
        egui::Grid::new("capability_limits").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
            for (label, limit) in [
                ("Lower limit (LSL)", &mut self.lower),
                ("Target", &mut self.target),
                ("Upper limit (USL)", &mut self.upper),
            ] {
                ui.checkbox(&mut limit.used, label);
                ui.add_enabled(limit.used, egui::DragValue::new(&mut limit.value).speed(0.01));
                ui.end_row();
            }
        });
        ui.add_space(6.0);
        ui.label("Short-term (within) σ from:");
        ui.radio_value(&mut self.method, 0, "Moving ranges of consecutive values (individuals)");
        ui.radio_value(&mut self.method, 1, "Average range of consecutive subgroups");
        ui.radio_value(&mut self.method, 2, "Pooled standard deviation of consecutive subgroups");
        ui.add_enabled_ui(self.method != 0, |ui| {
            ui.horizontal(|ui| {
                ui.label("Subgroup size:");
                ui.add(egui::DragValue::new(&mut self.subgroup_size).range(2..=25));
            });
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Process distribution:");
            egui::ComboBox::from_id_salt("capability_model")
                .selected_text(self.model_label())
                .width(kit::em(ui) * 16.0)
                .show_list(ui, |ui| {
                    if ui.selectable_label(self.model == CapabilityModel::Normal && !self.transform, "Normal").clicked()
                    {
                        (self.model, self.transform) = (CapabilityModel::Normal, false);
                    }
                    if ui
                        .selectable_label(
                            self.model == CapabilityModel::Normal && self.transform,
                            "Normal after Box–Cox",
                        )
                        .clicked()
                    {
                        (self.model, self.transform) = (CapabilityModel::Normal, true);
                    }
                    ui.separator();
                    for m in [CapabilityModel::BestFit, CapabilityModel::Johnson, CapabilityModel::Nonparametric]
                        .into_iter()
                        .chain(
                            FitFamily::ALL
                                .into_iter()
                                .filter(|f| *f != FitFamily::Normal)
                                .map(CapabilityModel::Distribution),
                        )
                    {
                        if ui.selectable_label(self.model == m, m.label()).clicked() {
                            (self.model, self.transform) = (m, false);
                        }
                    }
                });
        });
        if self.model == CapabilityModel::Normal && self.transform {
            ui.horizontal(|ui| {
                ui.label("Box–Cox power λ =");
                ui.add(egui::DragValue::new(&mut self.lambda).range(-5.0..=5.0).speed(0.05));
            });
        }
        let hint = match self.model {
            CapabilityModel::Normal if self.transform => {
                "Indices on the transformed scale; Describe ▸ Distribution suggests a λ."
            }
            CapabilityModel::Normal => "Normal-theory indices with confidence intervals.",
            CapabilityModel::Nonparametric => {
                "Indices from the sample's own percentiles; rough below about 740 values."
            }
            _ => {
                "Percentile-based indices and expected rates from the fitted distribution; \
                 Describe ▸ Identify distribution compares fits."
            }
        };
        kit::note(ui, hint);
        ui.add_space(6.0);
        ui.checkbox(&mut self.summary, "Summary page layout").on_hover_text(
            "Adds control charts, the last subgroups against the limits, a histogram and a probability plot",
        );
        let percentile = self.model != CapabilityModel::Normal;
        kit::more_options(ui, |ui| {
            ui.add_enabled(
                !self.summary,
                egui::Checkbox::new(&mut self.probability_plot, "Probability plot with the limits"),
            )
            .on_disabled_hover_text("The summary page always has one");
            ui.add_enabled_ui(self.by.is_some(), |ui| group_order_choice(ui, &mut self.group_order));
            ui.add_enabled_ui(percentile, |ui| {
                ui.checkbox(&mut self.bootstrap, "Bootstrap intervals for the percentile indices");
                ui.add_enabled_ui(self.bootstrap, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Resamples:");
                        ui.add(egui::DragValue::new(&mut self.resamples).range(20..=100_000));
                        ui.label("Seed:");
                        ui.add(crate::dialog_kit::seed_value(&mut self.seed));
                    });
                });
            })
            .response
            .on_disabled_hover_text("Normal-theory indices already have exact confidence intervals");
        });
        let Some(column) = self.column else { return Err("Choose a measurement column".into()) };
        let limits = SpecLimits {
            lower: self.lower.used.then_some(self.lower.value),
            upper: self.upper.used.then_some(self.upper.value),
            target: self.target.used.then_some(self.target.value),
        };
        if limits.lower.is_none() && limits.upper.is_none() {
            return Err("Set a lower or an upper specification limit".into());
        }
        if matches!((limits.lower, limits.upper), (Some(l), Some(u)) if l >= u) {
            return Err("The lower limit must be below the upper limit".into());
        }
        if self.by == Some(column) {
            return Err("Choose a group column other than the measurement".into());
        }
        let within = match self.method {
            0 => WithinSigma::MovingRange,
            1 => WithinSigma::AverageRange { subgroup_size: self.subgroup_size },
            _ => WithinSigma::PooledStdDev { subgroup_size: self.subgroup_size },
        };
        let request = CapabilityRequest {
            column,
            limits,
            within,
            box_cox: (self.transform && self.model == CapabilityModel::Normal).then_some(self.lambda),
            model: self.model,
            summary: self.summary,
            bootstrap: (self.bootstrap && percentile)
                .then_some(BootstrapSettings { resamples: self.resamples, seed: self.seed }),
            by: self.by,
            group_order: self.group_order,
            probability_plot: self.probability_plot && !self.summary,
        };
        let bounds: Vec<String> = [("LSL", limits.lower), ("USL", limits.upper)]
            .into_iter()
            .filter_map(|(name, v)| v.map(|v| format!("{name} {v}")))
            .collect();
        let what = if self.summary { "Capability summary page" } else { "Capability" };
        let by = self.by.map(|b| format!(", by {}", kit::name(table, b))).unwrap_or_default();
        let summary = format!(
            "{what} of {} against {} ({}){by}",
            kit::name(table, column),
            bounds.join(" and "),
            self.model_label()
        );
        kit::ready(request, summary)
    }

    fn model_label(&self) -> String {
        if self.model == CapabilityModel::Normal && self.transform {
            "Normal after Box–Cox".to_owned()
        } else {
            self.model.label()
        }
    }
}

/// Order of the groups in the Ppk by group chart.
fn group_order_choice(ui: &mut egui::Ui, order: &mut GroupOrder) {
    ui.horizontal(|ui| {
        ui.label("Order the Ppk chart:");
        egui::ComboBox::from_id_salt("capability_group_order").selected_text(order.label()).show_list(ui, |ui| {
            for o in GroupOrder::ALL {
                ui.selectable_value(order, o, o.label());
            }
        });
    });
}

/// In-report options of a capability study added by the by column and the
/// probability plot.
pub fn report_options(ui: &mut egui::Ui, request: &mut CapabilityRequest) {
    if !request.summary {
        ui.checkbox(&mut request.probability_plot, "Probability plot with the limits");
    }
    if request.by.is_some() {
        group_order_choice(ui, &mut request.group_order);
    }
}

impl Remember for CapabilityDialog {
    fn memory_key(&self) -> String {
        self.title().into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.column = columns.iter().copied().find(|&c| kit::is_numeric(table, c));
    }
}
