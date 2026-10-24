//! Dialogs and in-report settings for comparisons, proportions, event rates and
//! power calculations, goodness of fit for counts, crossover trials, the
//! distribution calculator and random columns, many two-group tests at once,
//! the adjustment of a column of p-values, and the area under a curve. Data in one column can be
//! split by a grouping column ("stacked") or come from several columns side
//! by side.

use crate::dialog_kit::ComboExt as _;
use app_model::compare::MedianTies;
use app_model::counts::column_levels;
use app_model::power_analysis::effect_name;
use app_model::{
    AdjustMethod, Alternative, AreaBaseline, AreaRequest, BestDirection, CalculatorRequest, ComparisonMethod,
    CrossoverRequest, FactorialRepeatedRequest, FdrMethod, GoodnessModel, GoodnessRequest, GroupComparisonRequest,
    GroupLayout, ItemOrder, ManyTestKind, ManyTestsOrder, ManyTestsRequest, MultipleResponseRequest, NestedRequest,
    OneSampleRequest, PValueRequest, PairedLayout, PairedRequest, PowerDesign, PowerRequest, PowerSolve,
    ProbabilityRegion, ProportionComparison, ProportionRequest, RateRequest, ResponseSource, SavedReport,
    ScreeningModel, ScreeningOrder, ScreeningRequest,
};
use eframe::egui::{self, RichText};
use stat_core::catalog::{Distribution, Family};
use stat_data::{Column, DataTable};
use std::sync::Arc;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::report_view::ReportRequest;

/// Which analysis the dialog sets up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareKind {
    OneSample,
    Groups,
    Paired,
    Proportions,
    Rates,
    Power,
    GoodnessOfFit,
    Crossover,
    /// Two- and three-way repeated-measures ANOVA.
    FactorialRepeated,
    /// Nested t test or nested one-way ANOVA.
    Nested,
    Calculator,
    RandomColumn,
    ManyTests,
    PValues,
    Area,
    /// One model per response column (Model menu).
    Screening,
    /// Multiple-response (check-all-that-apply) frequencies and cross-tab.
    MultipleResponseItems,
}

/// What the dialog makes: a report, or (random columns) a new data column.
pub enum CompareOutput {
    Report(Box<ReportRequest>),
    /// A new data column (random draws).
    Column(Arc<Column>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProportionMode {
    OneSample,
    Groups,
    Paired,
}

#[derive(Clone)]
pub struct CompareDialog {
    /// The report being edited (`None`: a new report).
    edit: Option<u64>,
    kind: CompareKind,
    stacked: bool,
    value: Option<usize>,
    group: Option<usize>,
    chosen: Vec<bool>,
    condition: Option<usize>,
    subject: Option<usize>,
    outcome: Option<usize>,
    event: Option<String>,
    mode: ProportionMode,
    second: Option<usize>,
    exposure: Option<usize>,
    one: OneSampleRequest,
    groups: GroupComparisonRequest,
    paired: PairedRequest,
    proportion: ProportionRequest,
    rate: RateRequest,
    power: PowerRequest,
    frequency: Option<usize>,
    poisson: bool,
    equal: bool,
    weights: Vec<(String, f64)>,
    min_expected: f64,
    crossover: CrossoverRequest,
    /// Within-subject factor columns (factorial repeated measures).
    within: Vec<bool>,
    /// Between-subject factor columns (factorial repeated measures).
    between: Vec<bool>,
    /// Whether to test simple effects.
    simple: bool,
    /// Factor whose simple effects are tested.
    simple_factor: Option<usize>,
    /// Factor at whose levels they are tested.
    simple_by: Option<usize>,
    /// Confidence level (nested comparison).
    nested_level: f64,
    calculator: CalculatorRequest,
    random: CalculatorRequest,
    random_name: String,
    seed: u64,
    /// The random column for the settings it was drawn with, so the dialog
    /// does not draw it again every frame.
    random_cache: Option<(RandomKey, Result<Arc<Column>, String>)>,
    /// How many times the random column was drawn (for tests).
    random_draws: usize,
    /// Variables of many two-group tests.
    many_chosen: Vec<bool>,
    many: ManyTestsRequest,
    /// Responses, factors and settings of a screening.
    screen_responses: Vec<bool>,
    screen_factors: Vec<bool>,
    screen_subgroup: Option<usize>,
    screening: ScreeningRequest,
    /// Labels and weights of a p-value adjustment.
    label_column: Option<usize>,
    weight_column: Option<usize>,
    pvalues: PValueRequest,
    /// x column and settings of an area under a curve.
    area_x: Option<usize>,
    area: AreaRequest,
    /// Answers in one column of delimited lists (else indicator columns).
    mr_lists: bool,
    /// What separates the items of a list.
    mr_delimiter: String,
    /// Item order of a multiple-response report.
    mr_order: ItemOrder,
}

/// What a random column depends on: family, parameters (as bits), row
/// count, seed and name.
type RandomKey = (Family, Vec<u64>, usize, u64, String);

/// "Normal(0, 1)".
fn distribution_label(r: &CalculatorRequest) -> String {
    let params: Vec<String> = r.params.iter().map(|p| format!("{p}")).collect();
    format!("{}({})", r.family.label(), params.join(", "))
}

impl CompareDialog {
    pub fn new(kind: CompareKind, table: &DataTable) -> Self {
        Self {
            edit: None,
            kind,
            stacked: true,
            value: None,
            group: None,
            chosen: vec![false; table.n_cols()],
            condition: None,
            subject: None,
            outcome: None,
            event: None,
            mode: ProportionMode::Groups,
            second: None,
            exposure: None,
            one: OneSampleRequest::new(0),
            groups: GroupComparisonRequest::new(GroupLayout::Columns(Vec::new())),
            paired: PairedRequest::new(PairedLayout::Columns(Vec::new())),
            proportion: ProportionRequest::new(0, ProportionComparison::OneSample { null: 0.5 }),
            rate: RateRequest::new(0),
            power: PowerRequest::default(),
            frequency: None,
            poisson: false,
            equal: true,
            weights: Vec::new(),
            min_expected: 5.0,
            crossover: CrossoverRequest::new(0, 0, 0, 0),
            within: vec![false; table.n_cols()],
            between: vec![false; table.n_cols()],
            simple: false,
            simple_factor: None,
            simple_by: None,
            nested_level: 0.95,
            calculator: CalculatorRequest::new(Family::Normal),
            random: CalculatorRequest::new(Family::Normal),
            random_name: table.unused_column_name("Random"),
            seed: 1,
            random_cache: None,
            random_draws: 0,
            many_chosen: vec![false; table.n_cols()],
            many: ManyTestsRequest::new(Vec::new(), 0),
            screen_responses: vec![false; table.n_cols()],
            screen_factors: vec![false; table.n_cols()],
            screen_subgroup: None,
            screening: ScreeningRequest::new(Vec::new(), Vec::new()),
            label_column: None,
            weight_column: None,
            pvalues: PValueRequest::new(0),
            area_x: None,
            area: AreaRequest::new(0, 0),
            mr_lists: true,
            mr_delimiter: ";".to_owned(),
            mr_order: ItemOrder::default(),
        }
    }

    /// The dialog filled from a saved comparison, proportion, rate, power,
    /// goodness-of-fit, crossover or distribution-calculator report.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let valid = |c: usize| kit::valid(table, c);
        let n = table.n_cols();
        let d = match saved {
            SavedReport::OneSample { request } => Self {
                value: valid(request.column),
                group: request.group.and_then(valid),
                one: request.clone(),
                ..Self::new(CompareKind::OneSample, table)
            },
            SavedReport::Groups { request } => {
                let mut d = Self { groups: request.clone(), ..Self::new(CompareKind::Groups, table) };
                match &request.layout {
                    GroupLayout::Stacked { response, group } => {
                        (d.value, d.group) = (valid(*response), valid(*group));
                    }
                    GroupLayout::Columns(cols) => {
                        d.stacked = false;
                        d.chosen = kit::mask(n, cols);
                    }
                }
                d
            }
            SavedReport::Paired { request } => {
                let mut d = Self { paired: request.clone(), ..Self::new(CompareKind::Paired, table) };
                match &request.layout {
                    PairedLayout::Stacked { response, condition, subject } => {
                        (d.value, d.condition, d.subject) = (valid(*response), valid(*condition), valid(*subject));
                    }
                    PairedLayout::Columns(cols) => {
                        d.stacked = false;
                        d.chosen = kit::mask(n, cols);
                    }
                }
                d
            }
            SavedReport::Proportions { request } => {
                let mut d = Self {
                    outcome: valid(request.outcome),
                    event: request.event.clone(),
                    proportion: request.clone(),
                    ..Self::new(CompareKind::Proportions, table)
                };
                match request.comparison {
                    ProportionComparison::OneSample { .. } => d.mode = ProportionMode::OneSample,
                    ProportionComparison::Groups { group } => {
                        (d.mode, d.group) = (ProportionMode::Groups, valid(group))
                    }
                    ProportionComparison::Paired { second } => {
                        (d.mode, d.second) = (ProportionMode::Paired, valid(second));
                    }
                }
                d
            }
            SavedReport::Rates { request } => Self {
                value: valid(request.count),
                exposure: request.exposure.and_then(valid),
                group: request.group.and_then(valid),
                rate: request.clone(),
                ..Self::new(CompareKind::Rates, table)
            },
            SavedReport::Power { request } => Self { power: *request, ..Self::new(CompareKind::Power, table) },
            SavedReport::GoodnessOfFit { request } => {
                let mut d = Self {
                    value: valid(request.column),
                    frequency: request.frequency.and_then(valid),
                    ..Self::new(CompareKind::GoodnessOfFit, table)
                };
                match &request.model {
                    GoodnessModel::Poisson { min_expected } => (d.poisson, d.min_expected) = (true, *min_expected),
                    GoodnessModel::Proportions(weights) => {
                        d.equal = weights.is_empty();
                        d.weights = weights.clone();
                    }
                }
                d
            }
            SavedReport::Crossover { request } => Self {
                value: valid(request.response),
                subject: valid(request.subject),
                condition: valid(request.period),
                group: valid(request.treatment),
                crossover: request.clone(),
                ..Self::new(CompareKind::Crossover, table)
            },
            SavedReport::FactorialRepeated { request } => {
                let keep = |v: &[usize]| v.iter().copied().filter_map(valid).collect::<Vec<_>>();
                Self {
                    value: valid(request.response),
                    subject: valid(request.subject),
                    within: kit::mask(n, &keep(&request.within)),
                    between: kit::mask(n, &keep(&request.between)),
                    simple: request.simple.is_some(),
                    simple_factor: request.simple.and_then(|(a, _)| valid(a)),
                    simple_by: request.simple.and_then(|(_, b)| valid(b)),
                    ..Self::new(CompareKind::FactorialRepeated, table)
                }
            }
            SavedReport::Nested { request } => Self {
                value: valid(request.response),
                group: valid(request.group),
                subject: valid(request.subject),
                nested_level: request.level,
                ..Self::new(CompareKind::Nested, table)
            },
            SavedReport::DistributionCalculator { request } => {
                Self { calculator: request.clone(), ..Self::new(CompareKind::Calculator, table) }
            }
            SavedReport::ManyTests { request } => Self {
                many_chosen: kit::mask(n, &request.columns),
                group: valid(request.group),
                many: request.clone(),
                ..Self::new(CompareKind::ManyTests, table)
            },
            SavedReport::Screening { request } => Self {
                screen_responses: kit::mask(n, &request.responses),
                screen_factors: kit::mask(n, &request.factors),
                screen_subgroup: request.subgroup.and_then(valid),
                screening: request.clone(),
                ..Self::new(CompareKind::Screening, table)
            },
            SavedReport::AdjustPValues { request } => Self {
                value: valid(request.p_values),
                label_column: request.label.and_then(valid),
                weight_column: request.weight.and_then(valid),
                pvalues: request.clone(),
                ..Self::new(CompareKind::PValues, table)
            },
            SavedReport::AreaUnderCurve { request } => Self {
                area_x: valid(request.x),
                value: valid(request.y),
                group: request.group.and_then(valid),
                area: request.clone(),
                ..Self::new(CompareKind::Area, table)
            },
            SavedReport::MultipleResponseItems { request } => {
                let mut d = Self {
                    group: request.group.and_then(valid),
                    mr_order: request.order,
                    ..Self::new(CompareKind::MultipleResponseItems, table)
                };
                match &request.source {
                    ResponseSource::Lists { column, delimiter } => {
                        (d.mr_lists, d.value, d.mr_delimiter) = (true, valid(*column), delimiter.clone());
                    }
                    ResponseSource::Indicators { columns } => {
                        d.mr_lists = false;
                        d.chosen = kit::mask(n, columns);
                    }
                }
                d
            }
            _ => return None,
        };
        Some(d)
    }

    fn title(&self) -> &'static str {
        match self.kind {
            CompareKind::OneSample => "One-sample tests",
            CompareKind::Groups => "Compare groups",
            CompareKind::Paired => "Paired or repeated measures",
            CompareKind::Proportions => "Proportions",
            CompareKind::Rates => "Event rates",
            CompareKind::Power => "Power and sample size",
            CompareKind::GoodnessOfFit => "Goodness of fit for counts",
            CompareKind::Crossover => "Crossover trial and equivalence",
            CompareKind::FactorialRepeated => "Factorial repeated measures",
            CompareKind::Nested => "Nested comparison",
            CompareKind::Calculator => "Distribution calculator",
            CompareKind::RandomColumn => "New random column",
            CompareKind::ManyTests => "Two groups, many variables",
            CompareKind::PValues => "Adjust and combine p-values",
            CompareKind::Area => "Area under a curve",
            CompareKind::Screening => "Screen many responses",
            CompareKind::MultipleResponseItems => "Multiple response",
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<CompareOutput> {
        let mut dialog = Dialog::new(self.title()).editing(self.edit);
        dialog = match self.kind {
            CompareKind::RandomColumn => dialog.creates(),
            CompareKind::Power | CompareKind::Calculator => dialog.without_rows(),
            _ => dialog,
        };
        let outcome = dialog.show(ctx, |ui| {
            if self.kind == CompareKind::RandomColumn {
                return self
                    .random_ui(ui, table)
                    .map(|r| kit::Ready { output: CompareOutput::Column(r.output), summary: r.summary });
            }
            let readiness = match self.kind {
                CompareKind::OneSample => self.one_sample_ui(ui, table),
                CompareKind::Groups => self.groups_ui(ui, table),
                CompareKind::Paired => self.paired_ui(ui, table),
                CompareKind::Proportions => self.proportions_ui(ui, table),
                CompareKind::Rates => self.rates_ui(ui, table),
                CompareKind::Power => {
                    power_settings(ui, &mut self.power);
                    let solve = match self.power.solve {
                        PowerSolve::Power { .. } => "power".to_owned(),
                        PowerSolve::SampleSize { .. } => "sample size".to_owned(),
                        PowerSolve::Effect { .. } => effect_name(&self.power.design).to_lowercase(),
                    };
                    let summary = format!("Solve for {solve}: {}", self.power.design.label());
                    kit::ready(SavedReport::Power { request: self.power }, summary)
                }
                CompareKind::GoodnessOfFit => self.goodness_ui(ui, table),
                CompareKind::Crossover => self.crossover_ui(ui, table),
                CompareKind::ManyTests => self.many_tests_ui(ui, table),
                CompareKind::PValues => self.pvalues_ui(ui, table),
                CompareKind::Area => self.area_ui(ui, table),
                CompareKind::Screening => self.screening_ui(ui, table),
                CompareKind::MultipleResponseItems => self.multiple_response_ui(ui, table),
                CompareKind::FactorialRepeated => self.factorial_repeated_ui(ui, table),
                CompareKind::Nested => self.nested_ui(ui, table),
                CompareKind::Calculator => {
                    kit::note(
                        ui,
                        "Probabilities, quantiles, density and moments of a distribution, with the region \
                         marked on a plot. Uses no data.",
                    );
                    if let Some(why) = calculator_inputs(ui, &mut self.calculator) {
                        return Err(why);
                    }
                    let summary = format!("Probabilities and quantiles of {}", distribution_label(&self.calculator));
                    kit::ready(SavedReport::DistributionCalculator { request: self.calculator.clone() }, summary)
                }
                CompareKind::RandomColumn => unreachable!("handled above"),
            };
            readiness.map(|r| kit::Ready {
                output: CompareOutput::Report(Box::new(ReportRequest::Comparison(r.output))),
                summary: r.summary,
            })
        });
        match outcome {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(self.kind, table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn layout_choice(&mut self, ui: &mut egui::Ui, stacked_text: &str, wide_text: &str) {
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.stacked, true, stacked_text);
            ui.radio_value(&mut self.stacked, false, wide_text);
        });
    }

    /// Numeric columns ticked in the list.
    fn checklist(&mut self, ui: &mut egui::Ui, table: &DataTable, label: &str) -> Vec<usize> {
        ui.label(label);
        kit::column_list(ui, "compare_columns", table, &mut self.chosen, |i| kit::is_numeric(table, i));
        kit::chosen_columns(&self.chosen).into_iter().filter(|&i| kit::is_numeric(table, i)).collect()
    }

    fn one_sample_ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(ui, "Tests whether a column's mean, median or standard deviation equals a hypothesized value.");
        let value = self.value;
        egui::Grid::new("one_sample_cols").num_columns(2).show(ui, |ui| {
            ui.label("Data column:");
            kit::column_choice(ui, "os_value", table, &mut self.value, |i| kit::is_numeric(table, i), None);
            ui.end_row();
            ui.label("Separately by (optional):");
            kit::column_choice(ui, "os_group", table, &mut self.group, |i| Some(i) != value, Some("None"));
            ui.end_row();
        });
        ui.separator();
        let r = &mut self.one;
        number(ui, "Hypothesized mean (and median):", &mut r.null_mean, 0.01);
        alternative_ui(ui, &mut r.alternative);
        kit::more_options(ui, |ui| {
            optional_number(ui, "Known standard deviation (z test):", &mut r.known_std_dev, 1.0, true);
            optional_number(ui, "Test the standard deviation against:", &mut r.null_std_dev, 1.0, true);
            equivalence_settings(ui, &mut r.equivalence, "mean");
            level_ui(ui, &mut r.level);
        });
        let Some(column) = self.value else { return Err("Choose a data column".into()) };
        if self.group == Some(column) {
            return Err("Data and group columns must differ".into());
        }
        let mut summary = format!("One-sample tests of {} against {}", kit::name(table, column), self.one.null_mean);
        if let Some(g) = self.group {
            summary += &format!(", by {}", kit::name(table, g));
        }
        kit::ready(
            SavedReport::OneSample { request: OneSampleRequest { column, group: self.group, ..self.one.clone() } },
            summary,
        )
    }

    fn groups_ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(ui, "Compares the means, medians and distributions of two or more independent groups.");
        self.layout_choice(ui, "Values in one column, groups in another", "One column per group");
        let layout = if self.stacked {
            let value = self.value;
            egui::Grid::new("groups_cols").num_columns(2).show(ui, |ui| {
                ui.label("Data column:");
                kit::column_choice(ui, "g_value", table, &mut self.value, |i| kit::is_numeric(table, i), None);
                ui.end_row();
                ui.label("Group column:");
                kit::column_choice(ui, "g_group", table, &mut self.group, |i| Some(i) != value, None);
                ui.end_row();
            });
            match (self.value, self.group) {
                (None, _) => Err("Choose a data column"),
                (_, None) => Err("Choose a group column"),
                (Some(response), Some(group)) if response == group => Err("Data and group columns must differ"),
                (Some(response), Some(group)) => Ok(GroupLayout::Stacked { response, group }),
            }
        } else {
            let cols = self.checklist(ui, table, "Columns to compare (two or more):");
            if cols.len() >= 2 { Ok(GroupLayout::Columns(cols)) } else { Err("Choose at least two columns") }
        };
        ui.separator();
        let labels = layout
            .as_ref()
            .map(|l| GroupComparisonRequest { layout: l.clone(), ..self.groups.clone() }.group_labels(table))
            .unwrap_or_default();
        let r = &mut self.groups;
        number(ui, "Hypothesized difference (two groups):", &mut r.null_difference, 0.01);
        alternative_ui(ui, &mut r.alternative);
        kit::more_options(ui, |ui| group_options(ui, r, &labels));
        let layout = layout?;
        let summary = match &layout {
            GroupLayout::Stacked { response, group } => {
                format!("Compare {} across groups of {}", kit::name(table, *response), kit::name(table, *group))
            }
            GroupLayout::Columns(cols) => format!("Compare {}", kit::count(cols.len(), "column", "columns")),
        };
        kit::ready(SavedReport::Groups { request: GroupComparisonRequest { layout, ..self.groups.clone() } }, summary)
    }

    fn paired_ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "Compares measurements taken on the same subjects: two conditions give paired tests, three or \
             more give a repeated-measures analysis.",
        );
        self.layout_choice(ui, "Values in one column, with condition and subject columns", "One column per condition");
        let layout = if self.stacked {
            let (value, condition) = (self.value, self.condition);
            egui::Grid::new("paired_cols").num_columns(2).show(ui, |ui| {
                ui.label("Data column:");
                kit::column_choice(ui, "p_value", table, &mut self.value, |i| kit::is_numeric(table, i), None);
                ui.end_row();
                ui.label("Condition column:");
                kit::column_choice(ui, "p_cond", table, &mut self.condition, |i| Some(i) != value, None);
                ui.end_row();
                ui.label("Subject (ID) column:");
                let allow = |i| Some(i) != value && Some(i) != condition;
                kit::column_choice(ui, "p_subj", table, &mut self.subject, allow, None);
                ui.end_row();
            });
            match (self.value, self.condition, self.subject) {
                (None, _, _) => Err("Choose a data column"),
                (_, None, _) => Err("Choose a condition column"),
                (_, _, None) => Err("Choose a subject column"),
                (Some(response), Some(condition), Some(subject))
                    if response != condition && response != subject && condition != subject =>
                {
                    Ok(PairedLayout::Stacked { response, condition, subject })
                }
                _ => Err("Data, condition and subject columns must differ"),
            }
        } else {
            let cols = self.checklist(ui, table, "Columns, one per condition (values on the same row are paired):");
            if cols.len() >= 2 { Ok(PairedLayout::Columns(cols)) } else { Err("Choose at least two columns") }
        };
        ui.separator();
        let r = &mut self.paired;
        number(ui, "Hypothesized mean difference (first − second):", &mut r.null_difference, 0.01);
        alternative_ui(ui, &mut r.alternative);
        kit::more_options(ui, |ui| {
            paired_equivalence(ui, r);
            level_ui(ui, &mut r.level);
        });
        let layout = layout?;
        let summary = match &layout {
            PairedLayout::Stacked { response, condition, subject } => format!(
                "{} across {}, paired by {}",
                kit::name(table, *response),
                kit::name(table, *condition),
                kit::name(table, *subject)
            ),
            PairedLayout::Columns(cols) if cols.len() == 2 => {
                format!("Paired comparison of {} and {}", kit::name(table, cols[0]), kit::name(table, cols[1]))
            }
            PairedLayout::Columns(cols) => format!("Repeated measures of {} columns", cols.len()),
        };
        kit::ready(SavedReport::Paired { request: PairedRequest { layout, ..self.paired.clone() } }, summary)
    }

    fn proportions_ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(ui, "Counts the rows whose outcome equals the chosen event and compares the proportions.");
        let levels = self.outcome.map(|o| column_levels(table, o)).unwrap_or_default();
        egui::Grid::new("prop_cols").num_columns(2).show(ui, |ui| {
            ui.label("Outcome column:");
            if kit::column_choice(ui, "pr_outcome", table, &mut self.outcome, |_| true, None) {
                self.event = None;
            }
            ui.end_row();
            ui.label("Event (counted) value:");
            egui::ComboBox::from_id_salt("pr_event")
                .selected_text(
                    self.event
                        .clone()
                        .unwrap_or_else(|| levels.last().map_or("—".to_owned(), |l| format!("{l} (last value)"))),
                )
                .width(kit::combo_width(ui))
                .show_list(ui, |ui| {
                    for l in levels.iter().take(200) {
                        ui.selectable_value(&mut self.event, Some(l.clone()), l.as_str());
                    }
                });
            ui.end_row();
        });
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.mode, ProportionMode::OneSample, "One sample vs a value");
            ui.radio_value(&mut self.mode, ProportionMode::Groups, "By group column");
            ui.radio_value(&mut self.mode, ProportionMode::Paired, "Paired (second outcome column)");
        });
        let outcome = self.outcome;
        let comparison = match self.mode {
            ProportionMode::OneSample => {
                let mut p = match self.proportion.comparison {
                    ProportionComparison::OneSample { null } => null,
                    _ => 0.5,
                };
                ui.horizontal(|ui| {
                    ui.label("Hypothesized proportion:");
                    ui.add(egui::DragValue::new(&mut p).speed(0.005).range(0.001..=0.999));
                });
                self.proportion.comparison = ProportionComparison::OneSample { null: p };
                Ok(ProportionComparison::OneSample { null: p })
            }
            ProportionMode::Groups => {
                ui.horizontal(|ui| {
                    ui.label("Group column:");
                    kit::column_choice(ui, "pr_group", table, &mut self.group, |i| Some(i) != outcome, None);
                });
                match self.group {
                    None => Err("Choose a group column"),
                    Some(g) if Some(g) == outcome => Err("Outcome and group columns must differ"),
                    Some(group) => Ok(ProportionComparison::Groups { group }),
                }
            }
            ProportionMode::Paired => {
                ui.horizontal(|ui| {
                    ui.label("Second outcome column:");
                    kit::column_choice(ui, "pr_second", table, &mut self.second, |i| Some(i) != outcome, None);
                });
                match self.second {
                    None => Err("Choose a second outcome column"),
                    Some(s) if Some(s) == outcome => Err("The two outcome columns must differ"),
                    Some(second) => Ok(ProportionComparison::Paired { second }),
                }
            }
        };
        ui.separator();
        if self.mode != ProportionMode::Paired {
            alternative_ui(ui, &mut self.proportion.alternative);
        }
        kit::more_options(ui, |ui| level_ui(ui, &mut self.proportion.level));
        let Some(outcome) = self.outcome else { return Err("Choose an outcome column".into()) };
        let comparison = comparison?;
        let event = self.event.clone().or_else(|| levels.last().cloned()).unwrap_or_default();
        let what = format!("{} = {event}", kit::name(table, outcome));
        let summary = match comparison {
            ProportionComparison::OneSample { null } => format!("Proportion of {what} against {null}"),
            ProportionComparison::Groups { group } => format!("Proportions of {what} by {}", kit::name(table, group)),
            ProportionComparison::Paired { second } => {
                format!("Paired proportions of {} and {}", kit::name(table, outcome), kit::name(table, second))
            }
        };
        kit::ready(
            SavedReport::Proportions {
                request: ProportionRequest {
                    outcome,
                    event: self.event.clone(),
                    comparison,
                    ..self.proportion.clone()
                },
            },
            summary,
        )
    }

    fn rates_ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "Compares event counts per unit of exposure (time, area, units inspected) with exact Poisson tests.",
        );
        let (value, exposure) = (self.value, self.exposure);
        egui::Grid::new("rate_cols").num_columns(2).show(ui, |ui| {
            ui.label("Event count column:");
            kit::column_choice(ui, "r_count", table, &mut self.value, |i| kit::is_numeric(table, i), None);
            ui.end_row();
            ui.label("Exposure column (optional):");
            let allow = |i| kit::is_numeric(table, i) && Some(i) != value;
            kit::column_choice(ui, "r_exposure", table, &mut self.exposure, allow, Some("1 per row"));
            ui.end_row();
            ui.label("Group column (optional):");
            let allow = |i| Some(i) != value && Some(i) != exposure;
            kit::column_choice(ui, "r_group", table, &mut self.group, allow, Some("None"));
            ui.end_row();
        });
        ui.separator();
        let r = &mut self.rate;
        if self.group.is_none() {
            ui.horizontal(|ui| {
                ui.label("Hypothesized rate (one group):");
                ui.add(egui::DragValue::new(&mut r.null_rate).speed(0.01).range(1e-9..=f64::MAX));
            });
        }
        alternative_ui(ui, &mut r.alternative);
        kit::more_options(ui, |ui| level_ui(ui, &mut r.level));
        let Some(count) = self.value else { return Err("Choose an event count column".into()) };
        if !kit::all_different(&[Some(count), self.exposure, self.group]) {
            return Err("Count, exposure and group columns must differ".into());
        }
        let per = self.exposure.map_or("row".to_owned(), |e| kit::name(table, e).to_owned());
        let mut summary = format!("Rate of {} per {per}", kit::name(table, count));
        if let Some(g) = self.group {
            summary += &format!(" by {}", kit::name(table, g));
        }
        kit::ready(
            SavedReport::Rates {
                request: RateRequest { count, exposure: self.exposure, group: self.group, ..self.rate.clone() },
            },
            summary,
        )
    }
}

impl CompareDialog {
    fn goodness_ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "Tests whether the categories of a column occur in given proportions, or whether a column of counts \
             follows a Poisson distribution (chi-square and likelihood-ratio tests).",
        );
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.poisson, false, "Categories in given proportions");
            ui.radio_value(&mut self.poisson, true, "Counts from a Poisson distribution");
        });
        let (poisson, value) = (self.poisson, self.value);
        if poisson && value.is_some_and(|v| !kit::is_numeric(table, v)) {
            self.value = None;
        }
        egui::Grid::new("gof_cols").num_columns(2).show(ui, |ui| {
            ui.label(if poisson { "Count column:" } else { "Category column:" });
            let allow = |i| !poisson || kit::is_numeric(table, i);
            kit::column_choice(ui, "gof_value", table, &mut self.value, allow, None);
            ui.end_row();
            ui.label("Frequency column (optional):");
            let allow = |i| kit::is_numeric(table, i) && Some(i) != value;
            kit::column_choice(ui, "gof_freq", table, &mut self.frequency, allow, Some("1 per row"));
            ui.end_row();
        });
        ui.separator();
        let model = if self.poisson {
            kit::more_options(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Pool neighbouring counts until each cell expects at least:")
                        .on_hover_text("Cochran's rule uses 5; 0 pools only the open upper tail");
                    ui.add(egui::DragValue::new(&mut self.min_expected).speed(0.1).range(0.0..=50.0));
                });
            });
            GoodnessModel::Poisson { min_expected: self.min_expected }
        } else {
            let levels = self.value.map(|c| column_levels(table, c)).unwrap_or_default();
            if self.value.is_some()
                && (self.weights.len() != levels.len() || self.weights.iter().zip(&levels).any(|((a, _), b)| a != b))
            {
                self.weights = levels.iter().map(|l| (l.clone(), 1.0)).collect();
            }
            ui.checkbox(&mut self.equal, "Equal proportions");
            if !self.equal {
                kit::note(
                    ui,
                    "Hypothesized proportions or ratios (e.g. 9 : 3 : 3 : 1); they are rescaled to sum to 1.",
                );
                egui::ScrollArea::vertical().id_salt("gof_weights").max_height(kit::em(ui) * 12.0).show(ui, |ui| {
                    egui::Grid::new("gof_weight_grid").num_columns(2).show(ui, |ui| {
                        for (label, w) in &mut self.weights {
                            ui.label(label.as_str());
                            ui.add(egui::DragValue::new(w).speed(0.01).range(0.0..=1e9));
                            ui.end_row();
                        }
                    });
                });
            }
            GoodnessModel::Proportions(if self.equal { Vec::new() } else { self.weights.clone() })
        };
        let Some(column) = self.value else {
            return Err(if self.poisson { "Choose a count column" } else { "Choose a category column" }.into());
        };
        if self.frequency == Some(column) {
            return Err("Data and frequency columns must differ".into());
        }
        let against = match &model {
            GoodnessModel::Poisson { .. } => "a Poisson distribution",
            GoodnessModel::Proportions(w) if w.is_empty() => "equal proportions",
            GoodnessModel::Proportions(_) => "the given proportions",
        };
        let summary = format!("Goodness of fit of {} to {against}", kit::name(table, column));
        kit::ready(
            SavedReport::GoodnessOfFit { request: GoodnessRequest { column, frequency: self.frequency, model } },
            summary,
        )
    }

    fn crossover_ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "Two-period, two-treatment crossover: one row per subject and period. Estimates treatment, period and \
             carryover effects and tests average equivalence (two one-sided tests).",
        );
        egui::Grid::new("xo_cols").num_columns(2).show(ui, |ui| {
            ui.label("Response column:");
            kit::column_choice(ui, "xo_resp", table, &mut self.value, |i| kit::is_numeric(table, i), None);
            ui.end_row();
            ui.label("Subject (ID) column:");
            kit::column_choice(ui, "xo_subj", table, &mut self.subject, |_| true, None);
            ui.end_row();
            ui.label("Period column (two values):");
            kit::column_choice(ui, "xo_period", table, &mut self.condition, |_| true, None);
            ui.end_row();
            ui.label("Treatment column (two values):");
            kit::column_choice(ui, "xo_trt", table, &mut self.group, |_| true, None);
            ui.end_row();
        });
        ui.separator();
        let treatments = self.group.map(|g| column_levels(table, g)).unwrap_or_default();
        crossover_comparison(ui, &mut self.crossover);
        kit::more_options(ui, |ui| {
            crossover_reference(ui, &mut self.crossover, &treatments);
            crossover_alpha(ui, &mut self.crossover);
        });
        let Some(response) = self.value else { return Err("Choose a response column".into()) };
        let Some(subject) = self.subject else { return Err("Choose a subject column".into()) };
        let Some(period) = self.condition else { return Err("Choose a period column".into()) };
        let Some(treatment) = self.group else { return Err("Choose a treatment column".into()) };
        if !kit::all_different(&[Some(response), Some(subject), Some(period), Some(treatment)]) {
            return Err("Choose four different columns".into());
        }
        let summary = format!(
            "Crossover trial of {} by {}, {} periods",
            kit::name(table, response),
            kit::name(table, treatment),
            kit::name(table, period)
        );
        kit::ready(
            SavedReport::Crossover {
                request: CrossoverRequest { response, subject, period, treatment, ..self.crossover.clone() },
            },
            summary,
        )
    }

    fn many_tests_ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "Runs the same test of two groups on every chosen column, adjusts the p-values for the number of \
             tests, and plots each column's difference against its p-value (a volcano plot).",
        );
        let group = self.group;
        let width = kit::em(ui) * 17.0;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.label("Variables (numeric columns):");
                kit::column_list(ui, "many_columns", table, &mut self.many_chosen, |i| {
                    kit::is_numeric(table, i) && Some(i) != group
                });
            });
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.label("Group column (two values):");
                kit::column_choice(ui, "many_group", table, &mut self.group, |_| true, None);
            });
        });
        ui.separator();
        many_tests_settings(ui, &mut self.many);
        let columns: Vec<usize> = kit::chosen_columns(&self.many_chosen)
            .into_iter()
            .filter(|&i| kit::is_numeric(table, i) && Some(i) != self.group)
            .collect();
        let Some(group) = self.group else { return Err("Choose a group column".into()) };
        if columns.is_empty() {
            return Err("Choose at least one variable".into());
        }
        let levels = column_levels(table, group);
        if levels.len() != 2 {
            return Err(format!(
                "The group column must have two values; {} has {}",
                kit::name(table, group),
                levels.len()
            ));
        }
        let summary = format!(
            "{} of {} by {}",
            self.many.test.label().split(" (").next().unwrap_or_default(),
            kit::count(columns.len(), "column", "columns"),
            kit::name(table, group)
        );
        kit::ready(
            SavedReport::ManyTests { request: ManyTestsRequest { columns, group, ..self.many.clone() } },
            summary,
        )
    }

    fn screening_ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "Fits the same model to every chosen response column, adjusts each effect's p-values across the \
             responses for the false discovery rate, and plots each response's effect against its p-value.",
        );
        let width = kit::em(ui) * 15.0;
        let (factors_mask, subgroup) = (self.screen_factors.clone(), self.screen_subgroup);
        let responses_mask = self.screen_responses.clone();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.label("Responses (numeric columns):");
                kit::column_list(ui, "screen_responses", table, &mut self.screen_responses, |i| {
                    kit::is_numeric(table, i) && !factors_mask[i] && Some(i) != subgroup
                });
            });
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.label("Factors:");
                kit::column_list(ui, "screen_factors", table, &mut self.screen_factors, |i| {
                    !responses_mask[i] && Some(i) != subgroup
                });
            });
        });
        ui.horizontal(|ui| {
            ui.label("Screen each level of (optional):");
            let (r, f) = (self.screen_responses.clone(), self.screen_factors.clone());
            kit::column_choice(
                ui,
                "screen_subgroup",
                table,
                &mut self.screen_subgroup,
                |i| !r[i] && !f[i],
                Some("None"),
            );
        });
        ui.separator();
        screening_settings(ui, &mut self.screening);
        let factors: Vec<usize> = kit::chosen_columns(&self.screen_factors)
            .into_iter()
            .filter(|&i| i < table.n_cols() && Some(i) != self.screen_subgroup)
            .collect();
        let responses: Vec<usize> = kit::chosen_columns(&self.screen_responses)
            .into_iter()
            .filter(|&i| kit::is_numeric(table, i) && !factors.contains(&i) && Some(i) != self.screen_subgroup)
            .collect();
        if responses.is_empty() {
            return Err("Choose at least one response".into());
        }
        if factors.is_empty() {
            return Err("Choose at least one factor".into());
        }
        if self.screening.model == ScreeningModel::KruskalWallis
            && (factors.len() != 1 || kit::is_continuous(table, factors[0]))
        {
            return Err("Kruskal–Wallis takes exactly one categorical factor".into());
        }
        let by = self.screen_subgroup.map_or_else(String::new, |g| format!(", each level of {}", kit::name(table, g)));
        let summary = format!(
            "{} of {} on {}{by}",
            self.screening.model.label().split(" (").next().unwrap_or_default(),
            kit::count(responses.len(), "response", "responses"),
            factors.iter().map(|&f| kit::name(table, f)).collect::<Vec<_>>().join(", ")
        );
        kit::ready(
            SavedReport::Screening {
                request: ScreeningRequest {
                    responses,
                    factors,
                    subgroup: self.screen_subgroup,
                    ..self.screening.clone()
                },
            },
            summary,
        )
    }

    fn pvalues_ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "Adjusts a column of p-values (one test per row) for multiple testing by six methods and Storey's \
             q-values, and combines them into one overall test (Fisher, Stouffer).",
        );
        let value = self.value;
        egui::Grid::new("pvalue_cols").num_columns(2).show(ui, |ui| {
            ui.label("p-value column:");
            kit::column_choice(ui, "pv_column", table, &mut self.value, |i| kit::is_numeric(table, i), None);
            ui.end_row();
            ui.label("Test names (optional):");
            kit::column_choice(
                ui,
                "pv_label",
                table,
                &mut self.label_column,
                |i| Some(i) != value,
                Some("Row numbers"),
            );
            ui.end_row();
        });
        ui.separator();
        pvalue_settings(ui, &mut self.pvalues);
        kit::more_options(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Weights for Stouffer's test (optional):");
                kit::column_choice(
                    ui,
                    "pv_weight",
                    table,
                    &mut self.weight_column,
                    |i| Some(i) != value && kit::is_numeric(table, i),
                    Some("Equal"),
                );
            });
        });
        let Some(p_values) = self.value else { return Err("Choose the column of p-values".into()) };
        let summary = format!("Adjust and combine the p-values in {}", kit::name(table, p_values));
        kit::ready(
            SavedReport::AdjustPValues {
                request: PValueRequest {
                    p_values,
                    label: self.label_column.filter(|&l| l != p_values),
                    weight: self.weight_column.filter(|&w| w != p_values),
                    ..self.pvalues.clone()
                },
            },
            summary,
        )
    }

    fn factorial_repeated_ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "Repeated-measures ANOVA with one to three within-subject factors and any between-subject factors, \
             from long data: one row per subject and within-subject cell. Each within-subject effect is tested \
             with Mauchly's test of sphericity and Greenhouse–Geisser and Huynh–Feldt corrections.",
        );
        egui::Grid::new("frm_cols").num_columns(2).show(ui, |ui| {
            ui.label("Response column:");
            kit::column_choice(ui, "frm_resp", table, &mut self.value, |i| kit::is_numeric(table, i), None);
            ui.end_row();
            ui.label("Subject (ID) column:");
            kit::column_choice(ui, "frm_subj", table, &mut self.subject, |_| true, None);
            ui.end_row();
        });
        let (value, subject) = (self.value, self.subject);
        let free = |i: usize| Some(i) != value && Some(i) != subject;
        ui.columns(2, |cols| {
            cols[0].label("Within-subject factors:");
            let between = self.between.clone();
            kit::column_list(&mut cols[0], "frm_within", table, &mut self.within, |i| {
                free(i) && !between.get(i).copied().unwrap_or(false)
            });
            cols[1].label("Between-subject factors:");
            let within = self.within.clone();
            kit::column_list(&mut cols[1], "frm_between", table, &mut self.between, |i| {
                free(i) && !within.get(i).copied().unwrap_or(false)
            });
        });
        let within: Vec<usize> = kit::chosen_columns(&self.within).into_iter().filter(|&i| free(i)).collect();
        let between: Vec<usize> =
            kit::chosen_columns(&self.between).into_iter().filter(|&i| free(i) && !within.contains(&i)).collect();
        let factors: Vec<usize> = within.iter().chain(&between).copied().collect();
        ui.separator();
        ui.checkbox(&mut self.simple, "Simple effects of one factor at each level of another");
        if self.simple {
            let simple_factor = self.simple_factor;
            egui::Grid::new("frm_simple").num_columns(2).show(ui, |ui| {
                ui.label("Test the factor:");
                kit::column_choice(ui, "frm_sf", table, &mut self.simple_factor, |i| factors.contains(&i), None);
                ui.end_row();
                ui.label("At each level of:");
                kit::column_choice(
                    ui,
                    "frm_sb",
                    table,
                    &mut self.simple_by,
                    |i| factors.contains(&i) && Some(i) != simple_factor,
                    None,
                );
                ui.end_row();
            });
        }
        let Some(response) = value else { return Err("Choose a response column".into()) };
        let Some(subject) = subject else { return Err("Choose a subject column".into()) };
        if response == subject {
            return Err("Choose different response and subject columns".into());
        }
        if within.is_empty() {
            return Err("Choose at least one within-subject factor".into());
        }
        if within.len() > 3 {
            return Err("Choose at most three within-subject factors".into());
        }
        if factors.len() > 4 {
            return Err("Choose at most four factors in all".into());
        }
        let simple = if self.simple {
            match (self.simple_factor, self.simple_by) {
                (Some(a), Some(b)) if a != b && factors.contains(&a) && factors.contains(&b) => Some((a, b)),
                _ => return Err("Choose the two factors for simple effects".into()),
            }
        } else {
            None
        };
        let names: Vec<&str> = factors.iter().map(|&c| kit::name(table, c)).collect();
        let summary = format!(
            "{} by {} (subjects: {})",
            kit::name(table, response),
            names.join(" × "),
            kit::name(table, subject)
        );
        kit::ready(
            SavedReport::FactorialRepeated {
                request: FactorialRepeatedRequest { response, subject, within, between, simple },
            },
            summary,
        )
    }

    fn multiple_response_ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "Counts check-all-that-apply answers: how many cases picked each item, as a percent of cases \
             and of all picks. With a group column, a cross-tab and a chi-square test per item with a \
             Bonferroni-adjusted overall test.",
        );
        ui.horizontal(|ui| {
            ui.label("Answers are in:");
            ui.radio_value(&mut self.mr_lists, true, "One column of lists");
            ui.radio_value(&mut self.mr_lists, false, "One column per item");
        });
        let group = self.group;
        let source = if self.mr_lists {
            let value = self.value;
            egui::Grid::new("mr_cols").num_columns(2).show(ui, |ui| {
                ui.label("Answer column:");
                kit::column_choice(ui, "mr_column", table, &mut self.value, |i| Some(i) != group, None);
                ui.end_row();
                ui.label("Items separated by:");
                ui.add(egui::TextEdit::singleline(&mut self.mr_delimiter).desired_width(kit::em(ui) * 4.0))
                    .on_hover_text("Leave empty to treat each cell as a single item");
                ui.end_row();
                ui.label("Cross-tab by (optional):");
                kit::column_choice(ui, "mr_group", table, &mut self.group, |i| Some(i) != value, Some("No groups"));
                ui.end_row();
            });
            let Some(column) = self.value else { return Err("Choose the answer column".into()) };
            ResponseSource::Lists { column, delimiter: self.mr_delimiter.clone() }
        } else {
            ui.label("Item columns (a non-zero number, yes, true or x means picked):");
            kit::column_list(ui, "mr_indicators", table, &mut self.chosen, |i| Some(i) != group);
            let chosen = kit::chosen_columns(&self.chosen);
            ui.horizontal(|ui| {
                ui.label("Cross-tab by (optional):");
                kit::column_choice(ui, "mr_group", table, &mut self.group, |i| !chosen.contains(&i), Some("No groups"));
            });
            let columns: Vec<usize> = chosen.into_iter().filter(|&c| Some(c) != self.group).collect();
            if columns.len() < 2 {
                return Err("Tick at least two item columns".into());
            }
            ResponseSource::Indicators { columns }
        };
        ui.horizontal(|ui| {
            ui.label("Order items:");
            for o in ItemOrder::ALL {
                ui.radio_value(&mut self.mr_order, o, o.label());
            }
        });
        let group = self.group.filter(|g| match &source {
            ResponseSource::Lists { column, .. } => g != column,
            ResponseSource::Indicators { columns } => !columns.contains(g),
        });
        let mut summary = match &source {
            ResponseSource::Lists { column, .. } => format!("Items picked in {}", kit::name(table, *column)),
            ResponseSource::Indicators { columns } => format!("Items picked across {} columns", columns.len()),
        };
        if let Some(g) = group {
            summary.push_str(&format!(", by {}", kit::name(table, g)));
        }
        kit::ready(
            SavedReport::MultipleResponseItems {
                request: MultipleResponseRequest { source, group, order: self.mr_order },
            },
            summary,
        )
    }

    fn area_ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "Joins the mean response at each x with straight lines and measures the area between that curve \
             and a baseline (trapezoid rule), with a standard error when every x has replicates, the area above \
             and below the baseline, and each peak above it.",
        );
        let (x, y) = (self.area_x, self.value);
        egui::Grid::new("area_cols").num_columns(2).show(ui, |ui| {
            ui.label("x (time, dose…):");
            kit::column_choice(
                ui,
                "area_x",
                table,
                &mut self.area_x,
                |i| kit::is_numeric(table, i) && Some(i) != y,
                None,
            );
            ui.end_row();
            ui.label("Response:");
            kit::column_choice(
                ui,
                "area_y",
                table,
                &mut self.value,
                |i| kit::is_numeric(table, i) && Some(i) != x,
                None,
            );
            ui.end_row();
            ui.label("One curve per (optional):");
            kit::column_choice(
                ui,
                "area_group",
                table,
                &mut self.group,
                |i| Some(i) != x && Some(i) != y,
                Some("One curve"),
            );
            ui.end_row();
        });
        ui.separator();
        area_settings(ui, &mut self.area);
        let Some(x) = self.area_x else { return Err("Choose the x column".into()) };
        let Some(y) = self.value else { return Err("Choose the response column".into()) };
        if x == y {
            return Err("Choose different columns for x and the response".into());
        }
        let group = self.group.filter(|&g| g != x && g != y);
        let mut summary = format!("Area under {} against {}", kit::name(table, y), kit::name(table, x));
        if let Some(g) = group {
            summary.push_str(&format!(", one curve per {}", kit::name(table, g)));
        }
        kit::ready(SavedReport::AreaUnderCurve { request: AreaRequest { x, y, group, ..self.area.clone() } }, summary)
    }

    fn nested_ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "Compares groups when each subject (animal, plate, batch) contributes several measurements: a mixed \
             model with subjects as a random effect, tested with Satterthwaite degrees of freedom, and the \
             classical nested ANOVA. With two groups this is a nested t test.",
        );
        egui::Grid::new("nest_cols").num_columns(2).show(ui, |ui| {
            ui.label("Response column:");
            kit::column_choice(ui, "nest_resp", table, &mut self.value, |i| kit::is_numeric(table, i), None);
            ui.end_row();
            ui.label("Group column:");
            kit::column_choice(ui, "nest_group", table, &mut self.group, |_| true, None);
            ui.end_row();
            ui.label("Subject column (nested in groups):");
            kit::column_choice(ui, "nest_subj", table, &mut self.subject, |_| true, None);
            ui.end_row();
        });
        kit::more_options(ui, |ui| level_ui(ui, &mut self.nested_level));
        let Some(response) = self.value else { return Err("Choose a response column".into()) };
        let Some(group) = self.group else { return Err("Choose a group column".into()) };
        let Some(subject) = self.subject else { return Err("Choose a subject column".into()) };
        if !kit::all_different(&[Some(response), Some(group), Some(subject)]) {
            return Err("Choose three different columns".into());
        }
        let summary = format!(
            "{} by {}, subjects {} nested in groups",
            kit::name(table, response),
            kit::name(table, group),
            kit::name(table, subject)
        );
        kit::ready(
            SavedReport::Nested {
                request: NestedRequest { level: self.nested_level, ..NestedRequest::new(response, group, subject) },
            },
            summary,
        )
    }

    /// How many times the random column has been drawn.
    #[cfg(test)]
    pub(crate) fn random_draws(&self) -> usize {
        self.random_draws
    }

    fn random_ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<Arc<Column>> {
        kit::note(
            ui,
            "Adds a column of random draws, one per row, from any distribution. The same seed always gives the \
             same values; undo removes the column.",
        );
        let invalid = distribution_settings(ui, "random", &mut self.random);
        ui.horizontal(|ui| {
            ui.label("Column name:");
            ui.add(egui::TextEdit::singleline(&mut self.random_name).desired_width(kit::em(ui) * 12.0));
        });
        kit::more_options(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Seed:");
                ui.add(crate::dialog_kit::seed_value(&mut self.seed));
            });
        });
        if let Some(why) = invalid {
            return Err(why);
        }
        let name = self.random_name.trim();
        if name.is_empty() {
            return Err("Name the new column".into());
        }
        let n = table.n_rows();
        let key: RandomKey = (
            self.random.family,
            self.random.params.iter().map(|p| p.to_bits()).collect(),
            n,
            self.seed,
            name.to_owned(),
        );
        if self.random_cache.as_ref().is_none_or(|(k, _)| *k != key) {
            let column = app_model::random_column(name, self.random.family, &self.random.params, n, self.seed)
                .map(Arc::new)
                .map_err(|e| e.to_string());
            self.random_draws += 1;
            self.random_cache = Some((key, column));
        }
        let column = self.random_cache.as_ref().map(|(_, c)| c.clone()).expect("just filled")?;
        let summary =
            format!("Column “{name}” of {} from {}", kit::count(n, "draw", "draws"), distribution_label(&self.random));
        kit::ready(column, summary)
    }
}

impl Remember for CompareDialog {
    fn memory_key(&self) -> String {
        self.title().to_owned()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
        // Remembered settings need not hold a drawn column.
        self.random_cache = None;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        let numeric: Vec<usize> = columns.iter().copied().filter(|&c| kit::is_numeric(table, c)).collect();
        match self.kind {
            CompareKind::OneSample
            | CompareKind::Rates
            | CompareKind::Crossover
            | CompareKind::FactorialRepeated
            | CompareKind::Nested => {
                self.value = numeric.first().copied();
            }
            CompareKind::Groups | CompareKind::Paired if numeric.len() >= 2 => {
                self.stacked = false;
                self.chosen = kit::mask(table.n_cols(), &numeric);
            }
            CompareKind::Groups | CompareKind::Paired => self.value = numeric.first().copied(),
            CompareKind::Proportions => self.outcome = columns.first().copied(),
            CompareKind::GoodnessOfFit => self.value = columns.first().copied(),
            CompareKind::ManyTests => self.many_chosen = kit::mask(table.n_cols(), &numeric),
            CompareKind::Screening => self.screen_responses = kit::mask(table.n_cols(), &numeric),
            CompareKind::PValues => self.value = numeric.first().copied(),
            CompareKind::Area => {
                self.area_x = numeric.first().copied();
                self.value = numeric.get(1).copied();
            }
            CompareKind::MultipleResponseItems => {
                if columns.len() >= 2 {
                    self.mr_lists = false;
                    self.chosen = kit::mask(table.n_cols(), columns);
                } else {
                    self.value = columns.first().copied();
                }
            }
            CompareKind::Power | CompareKind::Calculator | CompareKind::RandomColumn => {}
        }
    }
}

/// Test, adjustment, α and table order of many two-group tests (dialog and
/// report options).
pub fn many_tests_settings(ui: &mut egui::Ui, r: &mut ManyTestsRequest) {
    ui.horizontal(|ui| {
        ui.label("Test:");
        for t in ManyTestKind::ALL {
            ui.radio_value(&mut r.test, t, t.label());
        }
    });
    adjust_choice(ui, "many_adjust", &mut r.adjust);
    alpha_ui(ui, &mut r.alpha);
    ui.horizontal(|ui| {
        ui.label("Order the table by:");
        for o in ManyTestsOrder::ALL {
            ui.radio_value(&mut r.order, o, o.label());
        }
    });
}

/// Model, false-discovery-rate method, α and table order of a screening
/// (dialog and report options).
pub fn screening_settings(ui: &mut egui::Ui, r: &mut ScreeningRequest) {
    ui.horizontal(|ui| {
        ui.label("Model:");
        egui::ComboBox::from_id_salt("screen_model").selected_text(r.model.label()).show_list(ui, |ui| {
            for m in ScreeningModel::ALL {
                ui.selectable_value(&mut r.model, m, m.label());
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label("False discovery rate by:");
        for m in FdrMethod::ALL {
            ui.radio_value(&mut r.fdr, m, m.label());
        }
    });
    alpha_ui(ui, &mut r.alpha);
    ui.horizontal(|ui| {
        ui.label("Order the tables by:");
        for o in ScreeningOrder::ALL {
            ui.radio_value(&mut r.order, o, o.label());
        }
    });
}

/// Baseline, confidence level and smallest peak listed of an area under a
/// curve (dialog and report options).
pub fn area_settings(ui: &mut egui::Ui, r: &mut AreaRequest) {
    ui.horizontal(|ui| {
        ui.label("Baseline:");
        let first = matches!(r.baseline, AreaBaseline::FirstPoints(_));
        if ui.radio(!first, "A value").clicked() && first {
            r.baseline = AreaBaseline::Value(0.0);
        }
        if ui.radio(first, "Mean of the first points").clicked() && !first {
            r.baseline = AreaBaseline::FirstPoints(1);
        }
        match &mut r.baseline {
            AreaBaseline::Value(v) => {
                ui.add(egui::DragValue::new(v).speed(0.1)).on_hover_text("The area is measured above this value");
            }
            AreaBaseline::FirstPoints(k) => {
                ui.add(egui::DragValue::new(k).range(1..=1000).suffix(" points"))
                    .on_hover_text("The baseline is the mean response of the curve's first points (lowest x)");
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label("Confidence level:");
        ui.add(egui::DragValue::new(&mut r.level).speed(0.005).range(0.5..=0.999).fixed_decimals(3));
    });
    ui.horizontal(|ui| {
        ui.label("List peaks at least this share of the tallest:");
        let mut pct = 100.0 * r.min_peak_share;
        if ui.add(egui::DragValue::new(&mut pct).speed(1.0).range(0.0..=100.0).suffix(" %")).changed() {
            r.min_peak_share = pct / 100.0;
        }
    });
}

/// α and Storey's λ of a p-value adjustment (dialog and report options).
pub fn pvalue_settings(ui: &mut egui::Ui, r: &mut PValueRequest) {
    alpha_ui(ui, &mut r.alpha);
    ui.horizontal(|ui| {
        ui.label("Storey's λ (share of null p-values above it):");
        ui.add(egui::DragValue::new(&mut r.lambda).speed(0.01).range(0.0..=0.95).fixed_decimals(2))
            .on_hover_text("π̂₀ = #{p > λ} / (m(1 − λ)); 0.5 is Storey's (2002) default, 0 gives Benjamini–Hochberg");
    });
}

fn alpha_ui(ui: &mut egui::Ui, alpha: &mut f64) {
    ui.horizontal(|ui| {
        ui.label("Significance level α:");
        ui.add(egui::DragValue::new(alpha).speed(0.001).range(0.0001..=0.5).fixed_decimals(3));
    });
}

fn adjust_choice(ui: &mut egui::Ui, id: &str, method: &mut AdjustMethod) {
    ui.horizontal(|ui| {
        ui.label("Adjust p-values by:");
        egui::ComboBox::from_id_salt(id).selected_text(method.label()).show_list(ui, |ui| {
            for m in AdjustMethod::ALL {
                ui.selectable_value(method, m, m.label()).on_hover_text(format!("Controls the {}", m.controls()));
            }
        });
    });
}

/// Family and parameter editors shared by the calculator and random
/// columns. Returns why the parameters are not valid, if they are not.
fn distribution_settings(ui: &mut egui::Ui, id: &str, r: &mut CalculatorRequest) -> Option<String> {
    egui::Grid::new(format!("{id}_dist")).num_columns(2).show(ui, |ui| {
        ui.label("Distribution:");
        let mut family = r.family;
        egui::ComboBox::from_id_salt(format!("{id}_family"))
            .selected_text(family.label())
            .width(kit::combo_width(ui))
            .show_list(ui, |ui| {
                for f in Family::ALL {
                    ui.selectable_value(&mut family, f, f.label());
                }
            });
        ui.end_row();
        if family != r.family {
            *r = CalculatorRequest { region: r.region, probability: r.probability, ..CalculatorRequest::new(family) };
        }
        for (spec, v) in r.family.parameters().iter().zip(r.params.iter_mut()) {
            ui.label(format!("{}:", spec.name));
            if spec.integer {
                ui.add(egui::DragValue::new(v).speed(1.0).fixed_decimals(0).range(0.0..=1e9));
                *v = v.round();
            } else {
                ui.add(egui::DragValue::new(v).speed(0.01));
            }
            ui.end_row();
        }
    });
    Distribution::new(r.family, &r.params).err().map(|e| e.to_string())
}

/// Editor for the distribution calculator (dialog and report options).
pub fn calculator_settings(ui: &mut egui::Ui, r: &mut CalculatorRequest) {
    if let Some(why) = calculator_inputs(ui, r) {
        ui.colored_label(ui.visuals().warn_fg_color, why);
    }
}

/// The calculator's editors; returns why the parameters are not valid.
fn calculator_inputs(ui: &mut egui::Ui, r: &mut CalculatorRequest) -> Option<String> {
    let invalid = distribution_settings(ui, "calc", r);
    ui.horizontal(|ui| {
        ui.label("Probability of:");
        for region in ProbabilityRegion::ALL {
            ui.radio_value(&mut r.region, region, region.label());
        }
    });
    egui::Grid::new("calc_values").num_columns(2).show(ui, |ui| {
        ui.label("a:");
        ui.add(egui::DragValue::new(&mut r.values.0).speed(0.01));
        ui.end_row();
        if r.region.two_sided() {
            ui.label("b:");
            ui.add(egui::DragValue::new(&mut r.values.1).speed(0.01));
            ui.end_row();
            if r.values.1 < r.values.0 {
                r.values.1 = r.values.0;
            }
        }
        ui.label("Quantile at probability p:");
        ui.add(egui::DragValue::new(&mut r.probability).speed(0.001).range(0.0001..=0.9999).fixed_decimals(4));
        ui.end_row();
    });
    invalid
}

/// Editor for a crossover's comparison settings.
pub fn crossover_settings(ui: &mut egui::Ui, r: &mut CrossoverRequest, treatments: &[String]) {
    crossover_reference(ui, r, treatments);
    crossover_comparison(ui, r);
    crossover_alpha(ui, r);
}

fn crossover_reference(ui: &mut egui::Ui, r: &mut CrossoverRequest, treatments: &[String]) {
    if !treatments.is_empty() {
        ui.horizontal(|ui| {
            ui.label("Reference treatment:");
            let shown = r.reference.clone().unwrap_or_else(|| "first in order".to_owned());
            egui::ComboBox::from_id_salt("xo_reference").selected_text(shown).show_list(ui, |ui| {
                ui.selectable_value(&mut r.reference, None, "first in order");
                for t in treatments {
                    ui.selectable_value(&mut r.reference, Some(t.clone()), t);
                }
            });
        });
    }
}

/// Ratio or difference, and the equivalence bounds.
fn crossover_comparison(ui: &mut egui::Ui, r: &mut CrossoverRequest) {
    ui.horizontal(|ui| {
        ui.label("Compare:");
        let before = r.ratio;
        ui.radio_value(&mut r.ratio, true, "ratio of geometric means (log scale)");
        ui.radio_value(&mut r.ratio, false, "difference of means");
        if r.ratio != before {
            r.bounds = if r.ratio { (0.8, 1.25) } else { (-1.0, 1.0) };
        }
    });
    ui.horizontal(|ui| {
        ui.label("Equivalent when the interval lies between");
        let low = if r.ratio { 1e-6 } else { f64::MIN };
        ui.add(egui::DragValue::new(&mut r.bounds.0).speed(0.01).range(low..=f64::MAX));
        ui.label("and");
        ui.add(egui::DragValue::new(&mut r.bounds.1).speed(0.01));
        if r.bounds.1 <= r.bounds.0 {
            r.bounds.1 = r.bounds.0 + 0.01;
        }
    });
}

fn crossover_alpha(ui: &mut egui::Ui, r: &mut CrossoverRequest) {
    ui.horizontal(|ui| {
        ui.label("Level α of each one-sided test:").on_hover_text("α = 0.05 gives a 90% interval");
        ui.add(egui::DragValue::new(&mut r.alpha).speed(0.001).range(0.001..=0.25).fixed_decimals(3));
    });
}

fn common_settings(ui: &mut egui::Ui, alternative: &mut Alternative, level: &mut f64, one_sided_allowed: bool) {
    if one_sided_allowed {
        alternative_ui(ui, alternative);
    }
    level_ui(ui, level);
}

fn alternative_ui(ui: &mut egui::Ui, alternative: &mut Alternative) {
    ui.horizontal(|ui| {
        ui.label("Alternative:");
        ui.radio_value(alternative, Alternative::TwoSided, "≠ two-sided");
        ui.radio_value(alternative, Alternative::Less, "< less");
        ui.radio_value(alternative, Alternative::Greater, "> greater");
    });
}

fn level_ui(ui: &mut egui::Ui, level: &mut f64) {
    ui.horizontal(|ui| {
        ui.label("Confidence level:");
        ui.add(egui::DragValue::new(level).speed(0.001).range(0.5..=0.999).fixed_decimals(3));
    });
}

fn number(ui: &mut egui::Ui, label: &str, value: &mut f64, speed: f64) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(value).speed(speed));
    });
}

fn optional_number(ui: &mut egui::Ui, label: &str, value: &mut Option<f64>, default: f64, positive: bool) {
    ui.horizontal(|ui| {
        let mut on = value.is_some();
        if ui.checkbox(&mut on, label).changed() {
            *value = on.then_some(value.unwrap_or(default));
        }
        if let Some(v) = value {
            let drag = egui::DragValue::new(v).speed(0.01);
            ui.add(if positive { drag.range(1e-9..=f64::MAX) } else { drag });
        }
    });
}

fn equivalence_settings(ui: &mut egui::Ui, bounds: &mut Option<(f64, f64)>, what: &str) {
    ui.horizontal(|ui| {
        let mut on = bounds.is_some();
        if ui.checkbox(&mut on, format!("Equivalence test: {what} between")).changed() {
            *bounds = on.then_some(bounds.unwrap_or((-1.0, 1.0)));
        }
        if let Some((lo, hi)) = bounds {
            ui.add(egui::DragValue::new(lo).speed(0.01));
            ui.label("and");
            ui.add(egui::DragValue::new(hi).speed(0.01));
            if *hi <= *lo {
                *hi = *lo + 0.01;
            }
        }
    });
}

pub fn one_sample_settings(ui: &mut egui::Ui, r: &mut OneSampleRequest) {
    number(ui, "Hypothesized mean (and median):", &mut r.null_mean, 0.01);
    optional_number(ui, "Known standard deviation (z test):", &mut r.known_std_dev, 1.0, true);
    optional_number(ui, "Test the standard deviation against:", &mut r.null_std_dev, 1.0, true);
    equivalence_settings(ui, &mut r.equivalence, "mean");
    common_settings(ui, &mut r.alternative, &mut r.level, true);
}

pub fn group_settings(ui: &mut egui::Ui, r: &mut GroupComparisonRequest, labels: &[String]) {
    number(ui, "Hypothesized difference (two groups):", &mut r.null_difference, 0.01);
    group_options(ui, r, labels);
    common_settings(ui, &mut r.alternative, &mut r.level, true);
}

/// Equivalence, multiple comparisons, control, best and median-test settings.
fn group_options(ui: &mut egui::Ui, r: &mut GroupComparisonRequest, labels: &[String]) {
    equivalence_settings(ui, &mut r.equivalence, "difference (two groups)");
    ui.horizontal(|ui| {
        ui.label("All-pairs method (three or more groups):");
        egui::ComboBox::from_id_salt("group_pairwise").selected_text(r.method.label()).show_list(ui, |ui| {
            for m in ComparisonMethod::MEANS.into_iter().chain([ComparisonMethod::Dunn]) {
                ui.selectable_value(&mut r.method, m, m.label()).on_hover_text(crate::report_view::pairwise_hint(m));
            }
        });
    });
    ui.checkbox(&mut r.linear_trend, "Test for a linear trend across the groups in their order (three or more)")
        .on_hover_text("A contrast with equally spaced coefficients, and a test of departure from a straight line");
    ui.checkbox(&mut r.ratio_of_means, "Ratio of the two means with Fieller's interval (two groups)").on_hover_text(
        "The first group's mean divided by the second's, tested against 1; useful when effects are relative, such \
         as relative potency or percent change",
    );
    ui.horizontal(|ui| {
        ui.label("Compare with a control (Dunnett):").on_hover_text(
            "Each other group against the control, with simultaneous intervals; the alternative and confidence \
             level below apply",
        );
        egui::ComboBox::from_id_salt("group_control").selected_text(r.control.as_deref().unwrap_or("Off")).show_list(
            ui,
            |ui| {
                ui.selectable_value(&mut r.control, None, "Off");
                for l in labels {
                    ui.selectable_value(&mut r.control, Some(l.clone()), l);
                }
            },
        );
    });
    ui.horizontal(|ui| {
        ui.label("Comparisons with the best (Hsu):").on_hover_text(
            "Each group against the best of the others, with constrained simultaneous intervals that show which \
             groups can be ruled out as the best; uses the confidence level below",
        );
        ui.radio_value(&mut r.best, None, "off");
        ui.radio_value(&mut r.best, Some(BestDirection::Largest), "larger is better");
        ui.radio_value(&mut r.best, Some(BestDirection::Smallest), "smaller is better");
    });
    ui.horizontal(|ui| {
        ui.label("Median test: values equal to the median count as").on_hover_text("Mood's median test");
        ui.radio_value(&mut r.median_ties, MedianTies::Below, "not above");
        ui.radio_value(&mut r.median_ties, MedianTies::Above, "above");
        ui.radio_value(&mut r.median_ties, MedianTies::Ignore, "left out");
    });
}

pub fn paired_settings(ui: &mut egui::Ui, r: &mut PairedRequest) {
    number(ui, "Hypothesized mean difference (first − second):", &mut r.null_difference, 0.01);
    paired_equivalence(ui, r);
    common_settings(ui, &mut r.alternative, &mut r.level, true);
}

/// The equivalence test of a paired comparison (difference or ratio).
fn paired_equivalence(ui: &mut egui::Ui, r: &mut PairedRequest) {
    let before = r.ratio_equivalence;
    if r.ratio_equivalence {
        equivalence_settings(ui, &mut r.equivalence, "ratio first / second");
    } else {
        equivalence_settings(ui, &mut r.equivalence, "mean difference");
    }
    if r.equivalence.is_some() {
        ui.checkbox(&mut r.ratio_equivalence, "Test the ratio of geometric means (log scale; bounds are ratios)")
            .on_hover_text(
                "For positive data such as concentrations: equivalence when the ratio lies within, e.g., 0.8 to 1.25",
            );
        if r.ratio_equivalence != before {
            r.equivalence = Some(if r.ratio_equivalence { (0.8, 1.25) } else { (-1.0, 1.0) });
        }
    }
}

pub fn rate_settings(ui: &mut egui::Ui, r: &mut RateRequest, one_group: bool) {
    if one_group {
        ui.horizontal(|ui| {
            ui.label("Hypothesized rate (one group):");
            ui.add(egui::DragValue::new(&mut r.null_rate).speed(0.01).range(1e-9..=f64::MAX));
        });
    }
    common_settings(ui, &mut r.alternative, &mut r.level, true);
}

const DESIGNS: [&str; 22] = [
    "One-sample or paired t",
    "Two-sample t",
    "One-way ANOVA",
    "One proportion",
    "Two proportions",
    "One standard deviation",
    "Two variances",
    "Equivalence, one sample or paired",
    "Equivalence, two samples",
    "One-sample z",
    "Paired t",
    "One Poisson rate",
    "Two Poisson rates",
    "Margin of error: mean",
    "Margin of error: proportion",
    "Margin of error: standard deviation",
    "Margin of error: rate",
    "Tolerance interval",
    "Tolerance interval, distribution-free",
    "Two-level factorial",
    "Screening design",
    "General full factorial",
];

fn default_design(index: usize) -> PowerDesign {
    match index {
        0 => PowerDesign::OneSampleT { difference: 0.5, std_dev: 1.0 },
        1 => PowerDesign::TwoSampleT { difference: 0.5, std_dev: 1.0, ratio: 1.0 },
        2 => PowerDesign::OneWayAnova { groups: 3, max_difference: 1.0, std_dev: 1.0 },
        3 => PowerDesign::OneProportion { null: 0.5, actual: 0.6 },
        4 => PowerDesign::TwoProportions { first: 0.6, second: 0.5, ratio: 1.0 },
        5 => PowerDesign::OneStdDev { null: 1.0, actual: 1.5 },
        6 => PowerDesign::TwoVariances { variance_ratio: 2.0, ratio: 1.0 },
        7 => PowerDesign::EquivalenceOneSample { lower: -0.5, upper: 0.5, difference: 0.0, std_dev: 1.0 },
        8 => PowerDesign::EquivalenceTwoSample { lower: -0.5, upper: 0.5, difference: 0.0, std_dev: 1.0, ratio: 1.0 },
        9 => PowerDesign::OneSampleZ { difference: 0.5, std_dev: 1.0 },
        10 => PowerDesign::PairedT { difference: 0.5, std_dev: 1.0, correlation: 0.5 },
        11 => PowerDesign::OneRate { null: 2.0, actual: 2.5, exposure: 1.0 },
        12 => PowerDesign::TwoRates { first: 3.0, second: 2.0, exposure: 1.0, ratio: 1.0 },
        13 => PowerDesign::MeanMargin { margin: 0.5, std_dev: 2.0 },
        14 => PowerDesign::ProportionMargin { margin: 0.05, proportion: 0.5 },
        15 => PowerDesign::StdDevMargin { margin: 0.2, std_dev: 1.0 },
        16 => PowerDesign::RateMargin { margin: 0.5, rate: 4.0, exposure: 1.0 },
        17 => {
            PowerDesign::ToleranceInterval { proportion: 0.9, confidence: 0.95, max_proportion: 0.97, two_sided: true }
        }
        18 => PowerDesign::DistributionFreeTolerance { proportion: 0.95, removed: 2, two_sided: true },
        19 => PowerDesign::TwoLevelFactorial {
            factors: 4,
            fraction: 0,
            center_points: 0,
            interactions: false,
            effect: 1.0,
            std_dev: 1.0,
        },
        20 => PowerDesign::Screening { runs: 12, factors: 7, center_points: 0, effect: 1.0, std_dev: 1.0 },
        _ => PowerDesign::GeneralFactorial { levels: 3, other_cells: 2, max_difference: 1.0, std_dev: 1.0 },
    }
}

fn design_index(d: &PowerDesign) -> usize {
    DESIGNS.iter().position(|n| *n == d.label()).unwrap_or(0)
}

fn drag(ui: &mut egui::Ui, label: &str, value: &mut f64, range: std::ops::RangeInclusive<f64>, speed: f64) {
    ui.label(label);
    ui.add(egui::DragValue::new(value).speed(speed).range(range));
    ui.end_row();
}

/// Editor for a power question (used by the dialog and the report options).
pub fn power_settings(ui: &mut egui::Ui, r: &mut PowerRequest) {
    let mut index = design_index(&r.design);
    egui::ComboBox::from_id_salt("power_design").selected_text(DESIGNS[index]).width(kit::em(ui) * 18.0).show_list(
        ui,
        |ui| {
            for (i, name) in DESIGNS.iter().enumerate() {
                ui.selectable_value(&mut index, i, *name);
            }
        },
    );
    if index != design_index(&r.design) {
        r.design = default_design(index);
    }
    let big = f64::MAX;
    egui::Grid::new("power_params").num_columns(2).show(ui, |ui| {
        match &mut r.design {
            PowerDesign::OneSampleT { difference, std_dev } => {
                drag(ui, "Difference to detect:", difference, -big..=big, 0.01);
                drag(ui, "Standard deviation (of differences if paired):", std_dev, 1e-9..=big, 0.01);
            }
            PowerDesign::TwoSampleT { difference, std_dev, ratio } => {
                drag(ui, "Difference between means:", difference, -big..=big, 0.01);
                drag(ui, "Standard deviation:", std_dev, 1e-9..=big, 0.01);
                drag(ui, "Allocation n₂/n₁:", ratio, 0.05..=20.0, 0.01);
            }
            PowerDesign::OneWayAnova { groups, max_difference, std_dev } => {
                ui.label("Number of groups:");
                ui.add(egui::DragValue::new(groups).range(2..=50));
                ui.end_row();
                drag(ui, "Largest difference between means:", max_difference, 0.0..=big, 0.01);
                drag(ui, "Standard deviation:", std_dev, 1e-9..=big, 0.01);
            }
            PowerDesign::OneProportion { null, actual } => {
                drag(ui, "Hypothesized proportion p₀:", null, 0.001..=0.999, 0.005);
                drag(ui, "True proportion p₁:", actual, 0.001..=0.999, 0.005);
            }
            PowerDesign::TwoProportions { first, second, ratio } => {
                drag(ui, "First proportion p₁:", first, 0.001..=0.999, 0.005);
                drag(ui, "Second proportion p₂:", second, 0.001..=0.999, 0.005);
                drag(ui, "Allocation n₂/n₁:", ratio, 0.05..=20.0, 0.01);
            }
            PowerDesign::OneStdDev { null, actual } => {
                drag(ui, "Hypothesized σ₀:", null, 1e-9..=big, 0.01);
                drag(ui, "True σ₁:", actual, 1e-9..=big, 0.01);
            }
            PowerDesign::TwoVariances { variance_ratio, ratio } => {
                drag(ui, "Variance ratio σ₁²/σ₂²:", variance_ratio, 1e-6..=1e6, 0.01);
                drag(ui, "Allocation n₂/n₁:", ratio, 0.05..=20.0, 0.01);
            }
            PowerDesign::EquivalenceOneSample { lower, upper, difference, std_dev } => {
                drag(ui, "Lower bound:", lower, -big..=big, 0.01);
                drag(ui, "Upper bound:", upper, -big..=big, 0.01);
                drag(ui, "True difference:", difference, -big..=big, 0.01);
                drag(ui, "Standard deviation:", std_dev, 1e-9..=big, 0.01);
            }
            PowerDesign::EquivalenceTwoSample { lower, upper, difference, std_dev, ratio } => {
                drag(ui, "Lower bound:", lower, -big..=big, 0.01);
                drag(ui, "Upper bound:", upper, -big..=big, 0.01);
                drag(ui, "True difference:", difference, -big..=big, 0.01);
                drag(ui, "Standard deviation:", std_dev, 1e-9..=big, 0.01);
                drag(ui, "Allocation n₂/n₁:", ratio, 0.05..=20.0, 0.01);
            }
            PowerDesign::OneSampleZ { difference, std_dev } => {
                drag(ui, "Difference to detect:", difference, -big..=big, 0.01);
                drag(ui, "Known standard deviation:", std_dev, 1e-9..=big, 0.01);
            }
            PowerDesign::PairedT { difference, std_dev, correlation } => {
                drag(ui, "Mean difference to detect:", difference, -big..=big, 0.01);
                drag(ui, "Standard deviation (each condition):", std_dev, 1e-9..=big, 0.01);
                drag(ui, "Correlation between the pairs:", correlation, -0.99..=0.99, 0.01);
            }
            PowerDesign::OneRate { null, actual, exposure } => {
                drag(ui, "Hypothesized rate λ₀:", null, 1e-9..=big, 0.01);
                drag(ui, "True rate λ₁:", actual, 1e-9..=big, 0.01);
                drag(ui, "Exposure per unit:", exposure, 1e-9..=big, 0.01);
            }
            PowerDesign::TwoRates { first, second, exposure, ratio } => {
                drag(ui, "First rate λ₁:", first, 1e-9..=big, 0.01);
                drag(ui, "Second rate λ₂:", second, 1e-9..=big, 0.01);
                drag(ui, "Exposure per unit:", exposure, 1e-9..=big, 0.01);
                drag(ui, "Allocation n₂/n₁:", ratio, 0.05..=20.0, 0.01);
            }
            PowerDesign::MeanMargin { margin, std_dev } => {
                drag(ui, "Margin of error E:", margin, 1e-9..=big, 0.01);
                drag(ui, "Standard deviation:", std_dev, 1e-9..=big, 0.01);
            }
            PowerDesign::ProportionMargin { margin, proportion } => {
                drag(ui, "Margin of error E:", margin, 1e-4..=0.5, 0.001);
                drag(ui, "Expected proportion:", proportion, 0.001..=0.999, 0.005);
            }
            PowerDesign::StdDevMargin { margin, std_dev } => {
                drag(ui, "Margin of error E (upper limit − s):", margin, 1e-9..=big, 0.01);
                drag(ui, "Standard deviation:", std_dev, 1e-9..=big, 0.01);
            }
            PowerDesign::RateMargin { margin, rate, exposure } => {
                drag(ui, "Margin of error E:", margin, 1e-9..=big, 0.01);
                drag(ui, "Expected rate:", rate, 1e-9..=big, 0.01);
                drag(ui, "Exposure per unit:", exposure, 1e-9..=big, 0.01);
            }
            PowerDesign::ToleranceInterval { proportion, confidence, max_proportion, two_sided } => {
                drag(ui, "Proportion covered P:", proportion, 0.5..=0.9999, 0.001);
                drag(ui, "Confidence:", confidence, 0.5..=0.9999, 0.001);
                drag(ui, "Content limit P′ (> P):", max_proportion, 0.5..=0.99999, 0.001);
                ui.label("Interval:");
                ui.checkbox(two_sided, "two-sided");
                ui.end_row();
            }
            PowerDesign::DistributionFreeTolerance { proportion, removed, two_sided } => {
                drag(ui, "Proportion covered P:", proportion, 0.5..=0.9999, 0.001);
                ui.label("Interval:");
                if ui.checkbox(two_sided, "two-sided").changed() && *two_sided {
                    *removed = (*removed).max(2);
                }
                ui.end_row();
                ui.label("Order statistics outside the interval:").on_hover_text(
                    "m = 1 uses the smallest (or largest) value as the limit; m = 2 with two sides uses both \
                     extremes; larger m gives a narrower interval from more values",
                );
                ui.add(egui::DragValue::new(removed).range(if *two_sided { 2 } else { 1 }..=50));
                ui.end_row();
            }
            PowerDesign::TwoLevelFactorial { factors, fraction, center_points, interactions, effect, std_dev } => {
                ui.label("Factors:");
                ui.add(egui::DragValue::new(factors).range(2..=15));
                ui.end_row();
                ui.label("Fraction 1/2^q, q:");
                ui.add(egui::DragValue::new(fraction).range(0..=(*factors).saturating_sub(1)));
                ui.end_row();
                ui.label("Center points:");
                ui.add(egui::DragValue::new(center_points).range(0..=100));
                ui.end_row();
                ui.label("Model:");
                ui.checkbox(interactions, "two-factor interactions");
                ui.end_row();
                drag(ui, "Effect to detect (high − low):", effect, 0.0..=big, 0.01);
                drag(ui, "Standard deviation:", std_dev, 1e-9..=big, 0.01);
            }
            PowerDesign::Screening { runs, factors, center_points, effect, std_dev } => {
                ui.label("Runs per replicate (multiple of 4):");
                ui.add(egui::DragValue::new(runs).range(4..=200));
                ui.end_row();
                ui.label("Factors:");
                ui.add(egui::DragValue::new(factors).range(1..=199));
                ui.end_row();
                ui.label("Center points:");
                ui.add(egui::DragValue::new(center_points).range(0..=100));
                ui.end_row();
                drag(ui, "Effect to detect (high − low):", effect, 0.0..=big, 0.01);
                drag(ui, "Standard deviation:", std_dev, 1e-9..=big, 0.01);
            }
            PowerDesign::GeneralFactorial { levels, other_cells, max_difference, std_dev } => {
                ui.label("Levels of the factor tested:");
                ui.add(egui::DragValue::new(levels).range(2..=50));
                ui.end_row();
                ui.label("Cells of the other factors:");
                ui.add(egui::DragValue::new(other_cells).range(1..=1000));
                ui.end_row();
                drag(ui, "Largest difference between level means:", max_difference, 0.0..=big, 0.01);
                drag(ui, "Standard deviation:", std_dev, 1e-9..=big, 0.01);
            }
        }
        if !r.design.is_estimation() {
            drag(ui, "Significance level α:", &mut r.alpha, 0.001..=0.2, 0.001);
        } else if !matches!(
            r.design,
            PowerDesign::ToleranceInterval { .. } | PowerDesign::DistributionFreeTolerance { .. }
        ) {
            let mut confidence = 1.0 - r.alpha;
            drag(ui, "Confidence level:", &mut confidence, 0.5..=0.999, 0.001);
            r.alpha = 1.0 - confidence;
        }
    });
    let equivalence =
        matches!(r.design, PowerDesign::EquivalenceOneSample { .. } | PowerDesign::EquivalenceTwoSample { .. });
    if r.design.uses_alternative() {
        ui.horizontal(|ui| {
            ui.label("Alternative:");
            ui.radio_value(&mut r.alternative, Alternative::TwoSided, "≠ two-sided");
            ui.radio_value(&mut r.alternative, Alternative::Less, "< less");
            ui.radio_value(&mut r.alternative, Alternative::Greater, "> greater");
        });
    }
    ui.separator();
    ui.label("Solve for:");
    let (mut n, mut target) = match r.solve {
        PowerSolve::Power { n } => (n, 0.8),
        PowerSolve::SampleSize { target } => (30, target),
        PowerSolve::Effect { n, target } => (n, target),
    };
    let mut which = match r.solve {
        PowerSolve::Power { .. } => 0,
        PowerSolve::SampleSize { .. } => 1,
        PowerSolve::Effect { .. } => 2,
    };
    ui.horizontal(|ui| {
        ui.radio_value(&mut which, 1, "Sample size");
        ui.radio_value(&mut which, 0, "Power");
        if !equivalence {
            let word = match r.design {
                PowerDesign::DistributionFreeTolerance { .. } => "Largest",
                _ if r.design.uses_alternative() || r.design.is_factorial() => "Detectable",
                _ => "Smallest",
            };
            ui.radio_value(&mut which, 2, format!("{word} {}", effect_name(&r.design).to_lowercase()));
        }
    });
    if equivalence && which == 2 {
        which = 1;
    }
    let target_name = match r.design {
        PowerDesign::DistributionFreeTolerance { .. } => "Confidence:",
        _ if r.design.is_estimation() => "Target probability:",
        _ => "Target power:",
    };
    let n_name = if r.design.is_factorial() { "Replicates:" } else { "Sample size per group:" };
    let min_n = r.design.min_n().max(1);
    ui.horizontal(|ui| {
        if which != 1 {
            ui.label(n_name);
            ui.add(egui::DragValue::new(&mut n).range(min_n..=10_000_000));
        }
        if which != 0 {
            ui.label(target_name);
            ui.add(egui::DragValue::new(&mut target).speed(0.005).range(0.06..=0.999).fixed_decimals(3));
        }
    });
    r.solve = match which {
        0 => PowerSolve::Power { n },
        1 => PowerSolve::SampleSize { target },
        _ => PowerSolve::Effect { n, target },
    };
}

/// Settings of a comparison-type report, edited from its options menu.
pub fn report_settings(ui: &mut egui::Ui, saved: &mut SavedReport, labels: &[String]) {
    match saved {
        SavedReport::OneSample { request } => one_sample_settings(ui, request),
        SavedReport::Groups { request } => group_settings(ui, request, labels),
        SavedReport::Paired { request } => paired_settings(ui, request),
        SavedReport::Proportions { request } => {
            if let ProportionComparison::OneSample { null } = &mut request.comparison {
                ui.horizontal(|ui| {
                    ui.label("Hypothesized proportion:");
                    ui.add(egui::DragValue::new(null).speed(0.005).range(0.001..=0.999));
                });
            }
            let paired = matches!(request.comparison, ProportionComparison::Paired { .. });
            common_settings(ui, &mut request.alternative, &mut request.level, !paired);
        }
        SavedReport::Rates { request } => {
            let one = request.group.is_none();
            rate_settings(ui, request, one);
        }
        SavedReport::Power { request } => power_settings(ui, request),
        SavedReport::Crossover { request } => crossover_settings(ui, request, labels),
        SavedReport::Nested { request } => level_ui(ui, &mut request.level),
        SavedReport::Bayesian { request } => crate::bayes_dialog::report_settings(ui, request, labels),
        SavedReport::Signal { request } => crate::signal_dialog::report_settings(ui, request),
        SavedReport::ManyTests { request } => many_tests_settings(ui, request),
        SavedReport::TreatmentEffects { request } => crate::treatment_dialog::report_settings(ui, request),
        SavedReport::Screening { request } => screening_settings(ui, request),
        SavedReport::AdjustPValues { request } => pvalue_settings(ui, request),
        SavedReport::AreaUnderCurve { request } => area_settings(ui, request),
        SavedReport::MultipleResponseItems { request } => {
            ui.horizontal(|ui| {
                ui.label("Order items:");
                for o in ItemOrder::ALL {
                    ui.radio_value(&mut request.order, o, o.label());
                }
            });
            if let ResponseSource::Lists { delimiter, .. } = &mut request.source {
                ui.horizontal(|ui| {
                    ui.label("Items separated by:");
                    ui.add(egui::TextEdit::singleline(delimiter).desired_width(kit::em(ui) * 4.0));
                });
            }
        }
        SavedReport::DistributionCalculator { request } => calculator_settings(ui, request),
        SavedReport::ImagePlot { request } => crate::plot3d_dialog::image_settings(ui, request),
        SavedReport::GoodnessOfFit { request } => {
            if let GoodnessModel::Poisson { min_expected } = &mut request.model {
                ui.horizontal(|ui| {
                    ui.label("Pool neighbouring counts until each cell expects at least:");
                    ui.add(egui::DragValue::new(min_expected).speed(0.1).range(0.0..=50.0));
                });
            } else {
                ui.label(RichText::new("Change the proportions from the Compare menu's dialog.").weak());
            }
        }
        other => {
            if !crate::sequential_dialog::report_settings(ui, other) {
                crate::quality_tools_dialog::report_settings(ui, other);
            }
        }
    }
}
