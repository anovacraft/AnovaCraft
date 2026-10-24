//! Dialogs and in-report settings for the principal-components control chart
//! and the environmental monitoring limits. Both reports run through
//! [`app_model::ComparisonReport`], so their settings are edited in place from
//! the report's options menu (see [`report_settings`]).

use crate::dialog_kit::ComboExt as _;
use app_model::monitoring_limits::{ANY_GROUP, READING_KINDS, families, reading_kind_label};
use app_model::{LevelMethod, LimitsBy, MonitoringRequest, PcaChartRequest, SavedReport, SubgroupSize, TypedLevels};
use eframe::egui;
use stat_data::DataTable;
use stat_quality::{ReadingKind, SpeLimitMethod, WarningRules};

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::report_view::ReportRequest;

/// Which report the dialog sets up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitoringTool {
    PcaChart,
    Limits,
}

#[derive(Clone)]
pub struct MonitoringDialog {
    edit: Option<u64>,
    tool: MonitoringTool,
    // Principal-components chart.
    columns: Vec<bool>,
    label: Option<usize>,
    use_reference: bool,
    reference: usize,
    pca: PcaChartRequest,
    // Monitoring limits.
    value: Option<usize>,
    location: Option<usize>,
    order: Option<usize>,
    trials_from_column: bool,
    trials_column: Option<usize>,
    trials_constant: f64,
    group_column: Option<usize>,
    alert_column: Option<usize>,
    action_column: Option<usize>,
    limits: MonitoringRequest,
}

/// Whether column `c` holds only whole numbers of zero or more.
fn is_count(table: &DataTable, c: usize) -> bool {
    table
        .column(c)
        .and_then(|c| c.as_numeric())
        .is_some_and(|v| v.iter().filter(|x| x.is_finite()).all(|&x| x >= 0.0 && x.fract() == 0.0))
}

/// The level-method choices: label and whether `m` is that kind.
const METHODS: [&str; 4] =
    ["Quantiles of the fitted distribution", "Mean + k standard deviations", "Typed in", "From columns of the data"];

fn method_index(m: &LevelMethod) -> usize {
    match m {
        LevelMethod::Quantiles { .. } => 0,
        LevelMethod::Sigma { .. } => 1,
        LevelMethod::Typed(_) => 2,
        LevelMethod::Columns { .. } => 3,
    }
}

impl MonitoringDialog {
    pub fn new(tool: MonitoringTool) -> Self {
        Self {
            edit: None,
            tool,
            columns: Vec::new(),
            label: None,
            use_reference: false,
            reference: 50,
            pca: PcaChartRequest::new(Vec::new()),
            value: None,
            location: None,
            order: None,
            trials_from_column: true,
            trials_column: None,
            trials_constant: 5.0,
            group_column: None,
            alert_column: None,
            action_column: None,
            limits: MonitoringRequest::new(0, None),
        }
    }

    /// The dialog filled from a saved principal-components chart or
    /// monitoring-limits report.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        match saved {
            SavedReport::PcaChart { request } => {
                let d = Self::new(MonitoringTool::PcaChart);
                Some(Self {
                    columns: kit::mask(table.n_cols(), &request.columns),
                    label: request.label.and_then(|c| kit::valid(table, c)),
                    use_reference: request.reference.is_some(),
                    reference: request.reference.unwrap_or(d.reference),
                    pca: request.clone(),
                    ..d
                })
            }
            SavedReport::MonitoringLimits { request } => {
                let mut d = Self::new(MonitoringTool::Limits);
                d.value = kit::valid(table, request.value);
                d.location = request.location.and_then(|c| kit::valid(table, c));
                d.order = request.order.and_then(|c| kit::valid(table, c));
                match request.trials {
                    SubgroupSize::Column(c) => d.trials_column = kit::valid(table, c),
                    SubgroupSize::Constant(n) => (d.trials_from_column, d.trials_constant) = (false, n),
                    SubgroupSize::None => {}
                }
                if let LimitsBy::Column(c) = request.limits_by {
                    d.group_column = kit::valid(table, c);
                }
                if let LevelMethod::Columns { alert, action } = request.method {
                    d.alert_column = kit::valid(table, alert);
                    d.action_column = kit::valid(table, action);
                }
                d.limits = request.clone();
                Some(d)
            }
            _ => None,
        }
    }

    fn title(&self) -> &'static str {
        match self.tool {
            MonitoringTool::PcaChart => "Principal-components control chart",
            MonitoringTool::Limits => "Monitoring alert and action limits",
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<Box<ReportRequest>> {
        let outcome = Dialog::new(self.title()).editing(self.edit).show(ctx, |ui| {
            let saved = match self.tool {
                MonitoringTool::PcaChart => self.pca_ui(ui, table),
                MonitoringTool::Limits => self.limits_ui(ui, table),
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

    fn pca_ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "Fits principal components to a reference period and charts T² (variation along the model) and the \
             squared prediction error (variation the model does not explain) for every row.",
        );
        ui.label("Measurements (two or more):");
        kit::column_list(ui, "pca_columns", table, &mut self.columns, |i| kit::is_numeric(table, i));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Point labels:");
            kit::column_choice(ui, "pca_label", table, &mut self.label, |_| true, Some("Sequence number"));
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.use_reference, "Fit the model to the first");
            ui.add_enabled(self.use_reference, egui::DragValue::new(&mut self.reference).range(3..=1_000_000));
            ui.label("rows; monitor the rest");
        });
        kit::more_options(ui, |ui| pca_settings(ui, &mut self.pca));
        let columns: Vec<usize> = kit::chosen_columns(&self.columns)
            .into_iter()
            .filter(|&c| c < table.n_cols() && kit::is_numeric(table, c))
            .collect();
        if columns.len() < 2 {
            return Err("Choose at least two measurements".into());
        }
        let mut summary = format!("T² and SPE charts of {}", kit::count(columns.len(), "measurement", "measurements"));
        if self.use_reference {
            summary.push_str(&format!(", model from the first {} rows", self.reference));
        }
        let request = PcaChartRequest {
            columns,
            label: self.label,
            reference: self.use_reference.then_some(self.reference),
            ..self.pca.clone()
        };
        kit::ready(SavedReport::PcaChart { request }, summary)
    }

    fn limits_ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "Alert and action levels for environmental monitoring readings, set from the best-fitting \
             distribution of the data, and each location charted against them with early-warning rules.",
        );
        ui.label("Readings:");
        for kind in READING_KINDS {
            if ui.radio_value(&mut self.limits.kind, kind, reading_kind_label(kind)).changed() {
                self.limits.family = None;
            }
        }
        let kind = self.limits.kind;
        let allow_value = |i: usize| match kind {
            ReadingKind::Counts | ReadingKind::Positives => is_count(table, i),
            ReadingKind::Continuous => kit::is_numeric(table, i),
        };
        if self.value.is_some_and(|c| !allow_value(c)) {
            self.value = None;
        }
        egui::Grid::new("ml_columns").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label(if kind == ReadingKind::Positives { "Positive samples:" } else { "Reading:" });
            kit::column_choice(ui, "ml_value", table, &mut self.value, allow_value, None);
            ui.end_row();
            ui.label("Location:");
            kit::column_choice(ui, "ml_location", table, &mut self.location, |_| true, Some("One location"));
            ui.end_row();
            ui.label("Time order:");
            kit::column_choice(
                ui,
                "ml_order",
                table,
                &mut self.order,
                |i| kit::is_numeric(table, i),
                Some("Table order"),
            );
            ui.end_row();
        });
        if kind == ReadingKind::Positives {
            ui.label("Samples taken per reading:");
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.trials_from_column, true, "From a column");
                if kit::column_choice(ui, "ml_trials", table, &mut self.trials_column, |i| is_count(table, i), None) {
                    self.trials_from_column = true;
                }
            });
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.trials_from_column, false, "The same for every reading:");
                ui.add(egui::DragValue::new(&mut self.trials_constant).range(1.0..=1e6).speed(1.0));
            });
        }
        ui.add_space(4.0);
        ui.label("Set levels for:");
        let mut by = match self.limits.limits_by {
            LimitsBy::All => 0,
            LimitsBy::Location => 1,
            LimitsBy::Column(_) => 2,
        };
        ui.radio_value(&mut by, 0, "All locations together");
        ui.add_enabled_ui(self.location.is_some(), |ui| ui.radio_value(&mut by, 1, "Each location"));
        ui.horizontal(|ui| {
            ui.radio_value(&mut by, 2, "Each level of");
            if kit::column_choice(ui, "ml_group", table, &mut self.group_column, |_| true, None) {
                by = 2;
            }
        });
        if by == 1 && self.location.is_none() {
            by = 0;
        }
        self.limits.limits_by = match (by, self.group_column) {
            (1, _) => LimitsBy::Location,
            (2, Some(c)) => LimitsBy::Column(c),
            (2, None) => LimitsBy::Column(usize::MAX),
            _ => LimitsBy::All,
        };
        ui.add_space(4.0);
        levels_ui(ui, &mut self.limits, Some((table, &mut self.alert_column, &mut self.action_column)));
        kit::more_options(ui, |ui| rules_ui(ui, &mut self.limits));
        let Some(value) = self.value else {
            return Err(if kind == ReadingKind::Positives {
                "Choose the column of positive samples".into()
            } else {
                "Choose the reading column".into()
            });
        };
        if self.limits.limits_by == LimitsBy::Column(usize::MAX) {
            return Err("Choose the column whose levels share limits".into());
        }
        let trials = if kind == ReadingKind::Positives {
            if self.trials_from_column {
                match self.trials_column {
                    Some(c) => SubgroupSize::Column(c),
                    None => return Err("Choose the column of samples taken".into()),
                }
            } else {
                SubgroupSize::Constant(self.trials_constant.round().max(1.0))
            }
        } else {
            SubgroupSize::None
        };
        let method = match &self.limits.method {
            LevelMethod::Columns { .. } => match (self.alert_column, self.action_column) {
                (Some(a), Some(x)) => LevelMethod::Columns { alert: a, action: x },
                _ => return Err("Choose the alert-level and action-level columns".into()),
            },
            LevelMethod::Typed(t) if t.is_empty() => return Err("Type the levels of at least one group".into()),
            m => m.clone(),
        };
        let mut summary = format!("Monitoring limits of {}", kit::name(table, value));
        if let Some(l) = self.location {
            summary.push_str(&format!(" by {}", kit::name(table, l)));
        }
        let request = MonitoringRequest {
            value,
            trials,
            location: self.location,
            order: self.order,
            method,
            ..self.limits.clone()
        };
        kit::ready(SavedReport::MonitoringLimits { request }, summary)
    }
}

impl Remember for MonitoringDialog {
    fn memory_key(&self) -> String {
        self.title().into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        match self.tool {
            MonitoringTool::PcaChart => {
                let numeric: Vec<usize> = columns.iter().copied().filter(|&c| kit::is_numeric(table, c)).collect();
                self.columns = kit::mask(table.n_cols(), &numeric);
            }
            MonitoringTool::Limits => self.value = columns.iter().copied().find(|&c| is_count(table, c)),
        }
    }
}

/// Model settings of a principal-components chart.
pub fn pca_settings(ui: &mut egui::Ui, r: &mut PcaChartRequest) {
    ui.checkbox(&mut r.standardize, "Standardize the variables (correlation model)");
    let mut fixed = r.components.is_some();
    ui.horizontal(|ui| {
        ui.radio_value(&mut fixed, false, "Choose components by cross-validation with");
        ui.add_enabled(!fixed, egui::DragValue::new(&mut r.folds).range(2..=50));
        ui.label("folds");
    });
    ui.horizontal(|ui| {
        ui.radio_value(&mut fixed, true, "Keep");
        let mut a = r.components.unwrap_or(2);
        ui.add_enabled(fixed, egui::DragValue::new(&mut a).range(1..=200));
        ui.label("components");
        r.components = fixed.then_some(a);
    });
    ui.horizontal(|ui| {
        ui.label("False-alarm probability α per point:");
        ui.add(egui::DragValue::new(&mut r.alpha).range(1e-6..=0.2).speed(0.0005).max_decimals(5));
    });
    ui.horizontal(|ui| {
        ui.label("SPE limit:");
        for m in SpeLimitMethod::ALL {
            ui.radio_value(&mut r.spe_method, m, m.label());
        }
    });
}

/// How the levels are set (`columns`: the dialog's level-column choices,
/// or `None` in a report, where the columns stay as they are).
fn levels_ui(
    ui: &mut egui::Ui,
    r: &mut MonitoringRequest,
    columns: Option<(&DataTable, &mut Option<usize>, &mut Option<usize>)>,
) {
    let current = method_index(&r.method);
    ui.horizontal(|ui| {
        ui.label("Levels from:");
        egui::ComboBox::from_id_salt("ml_method").selected_text(METHODS[current]).width(kit::em(ui) * 16.0).show_list(
            ui,
            |ui| {
                for (k, label) in METHODS.iter().enumerate() {
                    let allowed = k != 3 || columns.is_some() || current == 3;
                    if ui.add_enabled(allowed, egui::SelectableLabel::new(k == current, *label)).clicked()
                        && k != current
                    {
                        r.method = match k {
                            0 => LevelMethod::DEFAULT,
                            1 => LevelMethod::Sigma { alert: 2.0, action: 3.0 },
                            2 => LevelMethod::Typed(vec![TypedLevels {
                                group: ANY_GROUP.into(),
                                alert: 1.0,
                                action: 5.0,
                            }]),
                            _ => LevelMethod::Columns { alert: 0, action: 0 },
                        };
                    }
                }
            },
        );
    });
    let kind = r.kind;
    match &mut r.method {
        LevelMethod::Quantiles { alert, action } => {
            egui::Grid::new("ml_quantiles").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
                ui.label("Alert level probability:");
                ui.add(egui::DragValue::new(alert).range(0.5..=0.9999).speed(0.001).max_decimals(4));
                ui.end_row();
                ui.label("Action level probability:");
                ui.add(egui::DragValue::new(action).range(0.5..=0.99999).speed(0.0005).max_decimals(5));
                ui.end_row();
            });
            let list = families(kind);
            let shown = r.family.as_deref().and_then(|k| list.iter().find(|f| f.0 == k)).map_or("Best by AIC", |f| f.1);
            ui.horizontal(|ui| {
                ui.label("Distribution:");
                egui::ComboBox::from_id_salt("ml_family").selected_text(shown).width(kit::em(ui) * 14.0).show_list(
                    ui,
                    |ui| {
                        ui.selectable_value(&mut r.family, None, "Best by AIC");
                        for (key, label) in &list {
                            ui.selectable_value(&mut r.family, Some((*key).to_owned()), *label);
                        }
                    },
                );
            });
        }
        LevelMethod::Sigma { alert, action } => {
            ui.horizontal(|ui| {
                ui.label("Alert: mean +");
                ui.add(egui::DragValue::new(alert).range(0.5..=10.0).speed(0.1));
                ui.label("SD;  action: mean +");
                ui.add(egui::DragValue::new(action).range(0.5..=10.0).speed(0.1));
                ui.label("SD");
            });
        }
        LevelMethod::Typed(levels) => {
            kit::note(ui, "Leave the group empty for levels that apply to every group not listed.");
            let mut remove = None;
            egui::Grid::new("ml_typed").num_columns(4).spacing([8.0, 4.0]).show(ui, |ui| {
                ui.label("Group");
                ui.label("Alert");
                ui.label("Action");
                ui.end_row();
                for (i, l) in levels.iter_mut().enumerate() {
                    ui.add(egui::TextEdit::singleline(&mut l.group).desired_width(kit::em(ui) * 8.0).hint_text("any"));
                    ui.add(egui::DragValue::new(&mut l.alert).speed(0.1));
                    ui.add(egui::DragValue::new(&mut l.action).speed(0.1));
                    if ui.small_button("Remove").clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
            if let Some(i) = remove {
                levels.remove(i);
            }
            if ui.button("Add a group").clicked() {
                levels.push(TypedLevels { group: String::new(), alert: 1.0, action: 5.0 });
            }
        }
        LevelMethod::Columns { .. } => match columns {
            Some((table, alert, action)) => {
                egui::Grid::new("ml_level_columns").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
                    ui.label("Alert-level column:");
                    kit::column_choice(ui, "ml_alert_col", table, alert, |i| kit::is_numeric(table, i), None);
                    ui.end_row();
                    ui.label("Action-level column:");
                    kit::column_choice(ui, "ml_action_col", table, action, |i| kit::is_numeric(table, i), None);
                    ui.end_row();
                });
            }
            None => kit::note(ui, "Levels come from the columns chosen in the dialog."),
        },
    }
}

/// Early-warning rules and which charts to show.
fn rules_ui(ui: &mut egui::Ui, r: &mut MonitoringRequest) {
    let d = WarningRules::default();
    let mut window = r.rules.alerts_in_window.is_some();
    let (mut n, mut m) = r.rules.alerts_in_window.or(d.alerts_in_window).unwrap_or((2, 5));
    ui.horizontal(|ui| {
        ui.checkbox(&mut window, "Warn at");
        ui.add_enabled(window, egui::DragValue::new(&mut n).range(1..=20));
        ui.label("alert excursions within");
        ui.add_enabled(window, egui::DragValue::new(&mut m).range(1..=50));
        ui.label("readings");
    });
    r.rules.alerts_in_window = window.then_some((n, m.max(n)));
    let mut rising = r.rules.rising_run.is_some();
    let mut k = r.rules.rising_run.or(d.rising_run).unwrap_or(3);
    ui.horizontal(|ui| {
        ui.checkbox(&mut rising, "Warn at");
        ui.add_enabled(rising, egui::DragValue::new(&mut k).range(2..=20));
        ui.label("increases in a row");
    });
    r.rules.rising_run = rising.then_some(k);
    ui.checkbox(&mut r.signals_only, "Chart only the locations with a signal");
}

/// In-report settings of the reports this module sets up; `false` for any
/// other report.
pub fn report_settings(ui: &mut egui::Ui, saved: &mut SavedReport) -> bool {
    match saved {
        SavedReport::PcaChart { request } => {
            ui.horizontal(|ui| {
                let mut use_reference = request.reference.is_some();
                ui.checkbox(&mut use_reference, "Fit the model to the first");
                let mut n = request.reference.unwrap_or(50);
                ui.add_enabled(use_reference, egui::DragValue::new(&mut n).range(3..=1_000_000));
                ui.label("rows");
                request.reference = use_reference.then_some(n);
            });
            pca_settings(ui, request);
        }
        SavedReport::MonitoringLimits { request } => {
            levels_ui(ui, request, None);
            ui.separator();
            rules_ui(ui, request);
        }
        _ => return false,
    }
    true
}
