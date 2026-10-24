//! The control chart dialog: a chart built from column roles.
//!
//! Each role takes a column chosen from its list or dragged from the
//! variables list: the measurement, an optional subgroup ID (rows sharing a
//! value form one subgroup, so sizes may vary), an optional phase (each run
//! of equal values gets its own limits) and an optional point label, plus a
//! part number for short-run charts. The chart type can be changed here or
//! later from the report's options menu.

use app_model::{ControlChartRequest, LastSubgroups, SavedReport};
use eframe::egui;
use stat_data::DataTable;
use stat_quality::constants::MAX_RANGE_SUBGROUP;
use stat_quality::{ChartKind, StandardValues, SubgroupUse};

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};

/// The column roles of a chart.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    Measurement,
    Subgroup,
    Phase,
    Label,
    Part,
}

impl Slot {
    const ALL: [Self; 5] = [Self::Measurement, Self::Subgroup, Self::Phase, Self::Label, Self::Part];

    fn name(self) -> &'static str {
        match self {
            Self::Measurement => "Measurement",
            Self::Subgroup => "Subgroup ID",
            Self::Phase => "Phase",
            Self::Label => "Point label",
            Self::Part => "Part number",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Measurement => "The numeric column to chart (required)",
            Self::Subgroup => {
                "Rows sharing a value form one subgroup, so subgroups may differ in size \
                 (without it, consecutive rows are grouped by the subgroup size)"
            }
            Self::Phase => {
                "Each run of consecutive rows with the same value is a phase with its own center line and \
                 limits; run rules restart at each phase"
            }
            Self::Label => {
                "Names each point (from the first row of its subgroup) in tooltips and the flagged-point table"
            }
            Self::Part => {
                "Short-run charts: each part number gets its own mean and σ, and every value is standardized \
                 by its part's estimates"
            }
        }
    }
}

#[derive(Clone)]
pub struct ChartDialog {
    edit: Option<u64>,
    measurement: Option<usize>,
    subgroup: Option<usize>,
    phase: Option<usize>,
    label: Option<usize>,
    part: Option<usize>,
    kind: ChartKind,
    subgroup_size: usize,
    span: usize,
    sigma_multiple: f64,
    range_span: usize,
    /// Standard (historical) values instead of estimates.
    standard: bool,
    standard_center: f64,
    standard_sigma: f64,
    /// Show only the last subgroups.
    last: bool,
    last_count: usize,
    limits_from_last: bool,
}

impl Default for ChartDialog {
    fn default() -> Self {
        Self {
            edit: None,
            measurement: None,
            subgroup: None,
            phase: None,
            label: None,
            part: None,
            kind: ChartKind::IndividualsAndMovingRange,
            subgroup_size: 5,
            span: app_model::control_chart::DEFAULT_SPAN,
            sigma_multiple: 3.0,
            range_span: 2,
            standard: false,
            standard_center: 0.0,
            standard_sigma: 1.0,
            last: false,
            last_count: 25,
            limits_from_last: false,
        }
    }
}

impl ChartDialog {
    /// The dialog filled from a saved control chart.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::ControlChart { request, .. } = saved else { return None };
        let valid = |c: Option<usize>| c.and_then(|c| kit::valid(table, c));
        Some(Self {
            edit: None,
            measurement: kit::valid(table, request.column),
            subgroup: valid(request.subgroup),
            phase: valid(request.phase),
            label: valid(request.label),
            part: valid(request.part),
            kind: request.kind,
            subgroup_size: if request.kind.subgroup_use() == SubgroupUse::Individuals {
                Self::default().subgroup_size
            } else {
                request.subgroup_size
            },
            span: request.span,
            sigma_multiple: request.sigma_multiple,
            range_span: request.range_span,
            standard: request.standard.is_some(),
            standard_center: request.standard.map_or(0.0, |s| s.center),
            standard_sigma: request.standard.map_or(1.0, |s| s.sigma),
            last: request.last.is_some(),
            last_count: request.last.map_or(25, |l| l.count),
            limits_from_last: request.last.is_some_and(|l| l.limits_from_last),
        })
    }

    fn slot(&mut self, slot: Slot) -> &mut Option<usize> {
        match slot {
            Slot::Measurement => &mut self.measurement,
            Slot::Subgroup => &mut self.subgroup,
            Slot::Phase => &mut self.phase,
            Slot::Label => &mut self.label,
            Slot::Part => &mut self.part,
        }
    }

    /// After `slot` was given a column: a subgroup ID implies a subgrouped
    /// chart, a part number a short-run chart.
    fn assigned(&mut self, slot: Slot) {
        if slot == Slot::Subgroup && self.subgroup.is_some() && self.kind.subgroup_use() == SubgroupUse::Individuals {
            self.kind = ChartKind::MeanAndRange;
        }
        if slot == Slot::Part && self.part.is_some() {
            self.kind = ChartKind::ShortRun;
        }
    }

    /// Show the dialog; `others` are the other open tables, which may hold
    /// saved limits to read standard values from.
    pub fn show_with(
        &mut self,
        ctx: &egui::Context,
        table: &DataTable,
        others: &[(&str, &DataTable)],
    ) -> Outcome<ControlChartRequest> {
        let dialog = Dialog::new("Control chart").editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table, others)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::default() };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(
        &mut self,
        ui: &mut egui::Ui,
        table: &DataTable,
        others: &[(&str, &DataTable)],
    ) -> Readiness<ControlChartRequest> {
        egui::Grid::new("chart_roles").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            for slot in Slot::ALL {
                ui.label(slot.name()).on_hover_text(slot.hint());
                let allow = |i: usize| slot != Slot::Measurement || kit::is_numeric(table, i);
                let none = (slot != Slot::Measurement).then_some("None");
                let target = self.slot(slot);
                if kit::column_choice(ui, ("chart_role", slot.name()), table, target, allow, none) {
                    self.assigned(slot);
                }
                ui.end_row();
            }
        });
        ui.add_space(6.0);
        ui.label("Chart type:");
        egui::Grid::new("chart_kinds").num_columns(2).spacing([12.0, 2.0]).show(ui, |ui| {
            for (i, kind) in ChartKind::ALL.into_iter().enumerate() {
                ui.radio_value(&mut self.kind, kind, kind.label()).on_hover_text(kind_hint(kind));
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
        let usage = self.kind.subgroup_use();
        let individuals = usage == SubgroupUse::Individuals;
        if individuals && self.subgroup.is_some() {
            kit::note(ui, "This chart plots every row; the subgroup ID is not used.");
        }
        let min = if usage == SubgroupUse::Either { 1 } else { 2 };
        ui.add_enabled_ui(!individuals && self.subgroup.is_none(), |ui| {
            ui.horizontal(|ui| {
                ui.label("Subgroup size:");
                ui.add(egui::DragValue::new(&mut self.subgroup_size).range(min..=MAX_RANGE_SUBGROUP));
            });
        });
        if !individuals && self.subgroup.is_none() {
            kit::note(
                ui,
                if usage == SubgroupUse::Either {
                    "Without a subgroup ID, consecutive rows form the subgroups (1 plots individual values)."
                } else {
                    "Without a subgroup ID, consecutive rows form the subgroups."
                },
            );
        }
        let name = self.measurement.map(|c| kit::name(table, c)).unwrap_or_default();
        kit::more_options(ui, |ui| self.options_ui(ui, name, others));
        let Some(column) = self.measurement else { return Err("Choose a measurement column".into()) };
        let basic = matches!(
            self.kind,
            ChartKind::IndividualsAndMovingRange | ChartKind::MeanAndRange | ChartKind::MeanAndStdDev
        );
        if !basic && (self.sigma_multiple != 3.0 || self.standard) {
            return Err("Other σ multiples and standard values apply to individuals, X̄–R and X̄–S charts".into());
        }
        if self.standard && self.standard_sigma <= 0.0 {
            return Err("The standard σ must be positive".into());
        }
        if self.kind == ChartKind::ShortRun && self.part.is_none() {
            return Err("Choose a part number for a short-run chart".into());
        }
        // The point label may repeat another role (labelling points by phase, say).
        let part = if self.kind == ChartKind::ShortRun { self.part } else { None };
        let roles = [
            (Slot::Measurement.name(), Some(column)),
            (Slot::Subgroup.name(), if individuals { None } else { self.subgroup }),
            (Slot::Phase.name(), self.phase),
            (Slot::Part.name(), part),
        ];
        if let Some(message) = kit::duplicate_role(table, &roles) {
            return Err(message);
        }
        let subgroup_size = if individuals { 1 } else { self.subgroup_size.max(min) };
        let request = ControlChartRequest {
            column,
            kind: self.kind,
            subgroup_size,
            subgroup: self.subgroup,
            phase: self.phase,
            label: self.label,
            span: self.span,
            part,
            sigma_multiple: if basic { self.sigma_multiple } else { 3.0 },
            range_span: if self.kind == ChartKind::IndividualsAndMovingRange { self.range_span } else { 2 },
            standard: (basic && self.standard)
                .then_some(StandardValues { center: self.standard_center, sigma: self.standard_sigma }),
            last: self
                .last
                .then_some(LastSubgroups { count: self.last_count, limits_from_last: self.limits_from_last }),
        };
        let mut summary = format!("{} chart of {}", self.kind.label(), kit::name(table, column));
        if !individuals {
            match self.subgroup {
                Some(s) => summary.push_str(&format!(" in subgroups by {}", kit::name(table, s))),
                None if subgroup_size > 1 => summary.push_str(&format!(" in subgroups of {subgroup_size}")),
                None => {}
            }
        }
        if let Some(p) = self.phase {
            summary.push_str(&format!(", phases by {}", kit::name(table, p)));
        }
        if request.sigma_multiple != 3.0 {
            summary.push_str(&format!(", limits at {}σ", request.sigma_multiple));
        }
        if request.standard.is_some() {
            summary.push_str(", limits from standard values");
        }
        if let Some(l) = request.last {
            summary.push_str(&format!(", last {} subgroups", l.count));
        }
        kit::ready(request, summary)
    }

    /// The span, σ multiple, moving-range span, standard values and last
    /// subgroups.
    fn options_ui(&mut self, ui: &mut egui::Ui, measurement: &str, others: &[(&str, &DataTable)]) {
        if self.kind == ChartKind::MovingAverage {
            ui.horizontal(|ui| {
                ui.label("Moving-average span:");
                ui.add(egui::DragValue::new(&mut self.span).range(2..=50));
            });
        }
        let basic = matches!(
            self.kind,
            ChartKind::IndividualsAndMovingRange | ChartKind::MeanAndRange | ChartKind::MeanAndStdDev
        );
        ui.add_enabled_ui(basic, |ui| {
            ui.horizontal(|ui| {
                ui.label("Limits at");
                ui.add(egui::DragValue::new(&mut self.sigma_multiple).speed(0.05).range(0.5..=6.0).suffix(" σ"))
                    .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Limits at σ"));
            });
            if self.kind == ChartKind::IndividualsAndMovingRange {
                ui.horizontal(|ui| {
                    ui.label("Values per moving range:");
                    ui.add(egui::DragValue::new(&mut self.range_span).range(2..=25));
                });
            }
            ui.checkbox(&mut self.standard, "Limits from standard (historical) values");
            if self.standard {
                ui.horizontal(|ui| {
                    ui.label("Process center:");
                    ui.add(egui::DragValue::new(&mut self.standard_center).speed(0.01))
                        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Process center"));
                    ui.label("Process σ:");
                    ui.add(egui::DragValue::new(&mut self.standard_sigma).speed(0.01).range(1e-12..=f64::MAX))
                        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Process σ"));
                });
                for (title, t) in others {
                    if let Some(sv) = app_model::control_chart::standard_from_table(t, measurement) {
                        if ui
                            .button(format!("Read from {title}"))
                            .on_hover_text("Use the process center and σ saved for this measurement in that table")
                            .clicked()
                        {
                            (self.standard_center, self.standard_sigma) = (sv.center, sv.sigma);
                        }
                    }
                }
            }
        })
        .response
        .on_disabled_hover_text("These apply to individuals, X̄–R and X̄–S charts");
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.last, "Show only the last");
            ui.add_enabled(self.last, egui::DragValue::new(&mut self.last_count).range(2..=10_000))
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Last subgroups"));
            ui.label("subgroups");
        });
        if self.last {
            ui.horizontal(|ui| {
                ui.label("Limits from:");
                ui.radio_value(&mut self.limits_from_last, false, "all subgroups");
                ui.radio_value(&mut self.limits_from_last, true, "the subgroups shown");
            });
        }
    }
}

impl Remember for ChartDialog {
    fn memory_key(&self) -> String {
        "Control chart".into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.measurement = columns.iter().copied().find(|&c| kit::is_numeric(table, c));
    }
}

/// What each chart type is for, shown on hover.
fn kind_hint(kind: ChartKind) -> &'static str {
    match kind {
        ChartKind::IndividualsAndMovingRange => "One value per point, with the moving range of consecutive values",
        ChartKind::MeanAndRange => "Subgroup means and ranges",
        ChartKind::MeanAndStdDev => "Subgroup means and standard deviations",
        ChartKind::MovingAverage => {
            "Averages of the last w points: more sensitive to small sustained shifts (rule 1 only)"
        }
        ChartKind::ThreeWayRange | ChartKind::ThreeWayStdDev => {
            "When subgroup means vary more than the within-subgroup spread allows: means charted as \
             individual values, their moving ranges, and the within-subgroup spread"
        }
        ChartKind::Zone => "Each point scored 1, 2, 4 or 8 by zone; a cumulative score of 8 on one side signals",
        ChartKind::ShortRun => {
            "Several part numbers in one chart: each value standardized by its own part's mean and σ"
        }
    }
}
