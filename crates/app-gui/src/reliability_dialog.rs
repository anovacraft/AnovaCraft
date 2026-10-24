//! Dialogs for the Reliability menu's life distribution, survival
//! regression, reliability growth, repairable systems and test plan reports.

use crate::dialog_kit::ComboExt as _;
use app_model::{
    AcceleratedRequest, BetaEstimator, DemonstrationRequest, DemonstrationSolve, EstimationRequest, GrowthRequest,
    LifeDistribution, McfLimits, McfVariance, RecurrenceRequest, SavedReport, StressRelationship, TestPlanRequest,
};
use eframe::egui;
use stat_data::DataTable;

use crate::competing_risks_dialog::CompetingRisksDialog;
use crate::degradation_dialog::DegradationDialog;
use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::life_distribution_dialog::LifeDistributionDialog;
use crate::fatigue_dialog::FatigueDialog;
use crate::life_stress_dialog::LifeStressDialog;
use crate::pcoa_dialog::PcoaDialog;
use crate::pk_dialog::PkDialog;
use crate::report_view::ReportRequest;
use crate::survival_model_dialog::{ProportionalHazardsDialog, SurvivalRegressionDialog};
use crate::warranty_dialog::WarrantyDialog;

/// Which of the four dialogs.
#[derive(Clone)]
pub enum ReliabilityKind {
    Growth(GrowthDialog),
    Recurrence(RecurrenceDialog),
    TestPlan(TestPlanDialog),
    LifeDistribution(LifeDistributionDialog),
    CompetingRisks(CompetingRisksDialog),
    Warranty(WarrantyDialog),
    LifeStress(LifeStressDialog),
    Degradation(DegradationDialog),
    ProportionalHazards(ProportionalHazardsDialog),
    SurvivalRegression(SurvivalRegressionDialog),
    Fatigue(FatigueDialog),
    // Reports from other menus that share this host (rendered as generic
    // report sections).
    Pharmacokinetics(PkDialog),
    PrincipalCoordinates(PcoaDialog),
}

/// One of the Reliability menu's dialogs (other than survival curves).
#[derive(Clone)]
pub struct ReliabilityDialog {
    edit: Option<u64>,
    kind: ReliabilityKind,
}

/// A number for a summary line.
fn num(v: f64) -> String {
    app_model::format::format_number(v, 4)
}

impl ReliabilityDialog {
    fn of(kind: ReliabilityKind) -> Self {
        Self { edit: None, kind }
    }

    /// Reliability growth (Crow–AMSAA).
    pub fn growth() -> Self {
        Self::of(ReliabilityKind::Growth(GrowthDialog::default()))
    }

    /// Repairable systems (mean cumulative function).
    pub fn recurrence() -> Self {
        Self::of(ReliabilityKind::Recurrence(RecurrenceDialog::default()))
    }

    /// Reliability test plans.
    pub fn test_plan() -> Self {
        Self::of(ReliabilityKind::TestPlan(TestPlanDialog::default()))
    }

    /// Life distributions of censored failure times.
    pub fn life_distribution() -> Self {
        Self::of(ReliabilityKind::LifeDistribution(LifeDistributionDialog::default()))
    }

    /// Competing risks (cumulative incidence, Gray's test, Fine–Gray, failure modes).
    pub fn competing_risks() -> Self {
        Self::of(ReliabilityKind::CompetingRisks(CompetingRisksDialog::default()))
    }

    /// Warranty returns (life data from counts, return forecast).
    pub fn warranty() -> Self {
        Self::of(ReliabilityKind::Warranty(WarrantyDialog::default()))
    }

    /// Life–stress (accelerated life test) models.
    pub fn life_stress() -> Self {
        Self::of(ReliabilityKind::LifeStress(LifeStressDialog::default()))
    }

    /// Degradation analysis.
    pub fn degradation() -> Self {
        Self::of(ReliabilityKind::Degradation(DegradationDialog::default()))
    }

    /// Proportional hazards (Cox) regression.
    pub fn proportional_hazards() -> Self {
        Self::of(ReliabilityKind::ProportionalHazards(ProportionalHazardsDialog::default()))
    }

    /// Parametric (accelerated failure time) survival regression.
    pub fn survival_regression() -> Self {
        Self::of(ReliabilityKind::SurvivalRegression(SurvivalRegressionDialog::default()))
    }

    /// Fatigue (stress–life) curves.
    pub fn fatigue() -> Self {
        Self::of(ReliabilityKind::Fatigue(FatigueDialog::default()))
    }

    /// Describe ▸ Pharmacokinetic parameters (non-compartmental).
    pub fn pharmacokinetics() -> Self {
        Self::of(ReliabilityKind::Pharmacokinetics(PkDialog::default()))
    }

    /// Multivariate ▸ Principal coordinates.
    pub fn principal_coordinates() -> Self {
        Self::of(ReliabilityKind::PrincipalCoordinates(PcoaDialog::default()))
    }

    /// The dialog filled from a saved growth, repairable-systems, test-plan
    /// or life-distribution report.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let kind = match saved {
            SavedReport::Growth { request } => ReliabilityKind::Growth(GrowthDialog::from_request(request, table)),
            SavedReport::Recurrence { request } => {
                ReliabilityKind::Recurrence(RecurrenceDialog::from_request(request, table))
            }
            SavedReport::TestPlan { request } => ReliabilityKind::TestPlan(TestPlanDialog::from_request(request)),
            SavedReport::LifeDistribution { request } => {
                ReliabilityKind::LifeDistribution(LifeDistributionDialog::from_saved(request, table))
            }
            SavedReport::CompetingRisks { request } => {
                ReliabilityKind::CompetingRisks(CompetingRisksDialog::from_request(request, table))
            }
            SavedReport::Warranty { request } => {
                ReliabilityKind::Warranty(WarrantyDialog::from_request(request, table))
            }
            SavedReport::LifeStress { request } => {
                ReliabilityKind::LifeStress(LifeStressDialog::from_saved(request, table))
            }
            SavedReport::Degradation { request } => {
                ReliabilityKind::Degradation(DegradationDialog::from_saved(request, table))
            }
            SavedReport::ProportionalHazards { request, .. } => {
                ReliabilityKind::ProportionalHazards(ProportionalHazardsDialog::from_saved(request, table))
            }
            SavedReport::SurvivalRegression { request, .. } => {
                ReliabilityKind::SurvivalRegression(SurvivalRegressionDialog::from_saved(request, table))
            }
            SavedReport::Fatigue { request } => ReliabilityKind::Fatigue(FatigueDialog::from_saved(request, table)),
            SavedReport::Pharmacokinetics { request } => {
                ReliabilityKind::Pharmacokinetics(PkDialog::from_saved(request, table))
            }
            SavedReport::PrincipalCoordinates { request } => {
                ReliabilityKind::PrincipalCoordinates(PcoaDialog::from_saved(request, table))
            }
            _ => return None,
        };
        Some(Self::of(kind))
    }

    fn title(&self) -> &'static str {
        match self.kind {
            ReliabilityKind::Growth(_) => "Reliability growth",
            ReliabilityKind::Recurrence(_) => "Repairable systems",
            ReliabilityKind::TestPlan(_) => "Reliability test plan",
            ReliabilityKind::LifeDistribution(_) => "Life distributions",
            ReliabilityKind::CompetingRisks(_) => "Competing risks",
            ReliabilityKind::Warranty(_) => "Warranty returns",
            ReliabilityKind::LifeStress(_) => "Life–stress models",
            ReliabilityKind::Degradation(_) => "Degradation",
            ReliabilityKind::ProportionalHazards(_) => "Proportional hazards",
            ReliabilityKind::SurvivalRegression(_) => "Parametric survival regression",
            ReliabilityKind::Fatigue(_) => "Fatigue curves",
            ReliabilityKind::Pharmacokinetics(_) => "Pharmacokinetic parameters",
            ReliabilityKind::PrincipalCoordinates(_) => "Principal coordinates",
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<Box<ReportRequest>> {
        let mut dialog = Dialog::new(self.title()).editing(self.edit);
        if matches!(self.kind, ReliabilityKind::TestPlan(_)) {
            dialog = dialog.without_rows();
        }
        let outcome = dialog.show(ctx, |ui| match &mut self.kind {
            ReliabilityKind::Growth(d) => d.ui(ui, table),
            ReliabilityKind::Recurrence(d) => d.ui(ui, table),
            ReliabilityKind::TestPlan(d) => d.ui(ui),
            ReliabilityKind::LifeDistribution(d) => d.ui(ui, table),
            ReliabilityKind::CompetingRisks(d) => d.ui(ui, table),
            ReliabilityKind::Warranty(d) => d.ui(ui, table),
            ReliabilityKind::LifeStress(d) => d.ui(ui, table),
            ReliabilityKind::Degradation(d) => d.ui(ui, table),
            ReliabilityKind::ProportionalHazards(d) => d.ui(ui, table),
            ReliabilityKind::SurvivalRegression(d) => d.ui(ui, table),
            ReliabilityKind::Fatigue(d) => d.ui(ui, table),
            ReliabilityKind::Pharmacokinetics(d) => d.ui(ui, table),
            ReliabilityKind::PrincipalCoordinates(d) => d.ui(ui, table),
        });
        match outcome {
            Outcome::Reset => {
                self.kind = match self.kind {
                    ReliabilityKind::Growth(_) => ReliabilityKind::Growth(GrowthDialog::default()),
                    ReliabilityKind::Recurrence(_) => ReliabilityKind::Recurrence(RecurrenceDialog::default()),
                    ReliabilityKind::TestPlan(_) => ReliabilityKind::TestPlan(TestPlanDialog::default()),
                    ReliabilityKind::LifeDistribution(_) => {
                        ReliabilityKind::LifeDistribution(LifeDistributionDialog::default())
                    }
                    ReliabilityKind::CompetingRisks(_) => {
                        ReliabilityKind::CompetingRisks(CompetingRisksDialog::default())
                    }
                    ReliabilityKind::Warranty(_) => ReliabilityKind::Warranty(WarrantyDialog::default()),
                    ReliabilityKind::LifeStress(_) => ReliabilityKind::LifeStress(LifeStressDialog::default()),
                    ReliabilityKind::Degradation(_) => ReliabilityKind::Degradation(DegradationDialog::default()),
                    ReliabilityKind::ProportionalHazards(_) => {
                        ReliabilityKind::ProportionalHazards(ProportionalHazardsDialog::default())
                    }
                    ReliabilityKind::SurvivalRegression(_) => {
                        ReliabilityKind::SurvivalRegression(SurvivalRegressionDialog::default())
                    }
                    ReliabilityKind::Fatigue(_) => ReliabilityKind::Fatigue(FatigueDialog::default()),
                    ReliabilityKind::Pharmacokinetics(_) => ReliabilityKind::Pharmacokinetics(PkDialog::default()),
                    ReliabilityKind::PrincipalCoordinates(_) => {
                        ReliabilityKind::PrincipalCoordinates(PcoaDialog::default())
                    }
                };
                Outcome::Open
            }
            outcome => outcome.map(|saved| Box::new(ReportRequest::Reliability(Box::new(saved)))),
        }
    }
}

impl Remember for ReliabilityDialog {
    fn memory_key(&self) -> String {
        self.title().into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        let first_numeric = columns.iter().copied().find(|&c| kit::is_numeric(table, c));
        match &mut self.kind {
            ReliabilityKind::Growth(d) => d.time = first_numeric,
            ReliabilityKind::Recurrence(d) => {
                d.system = columns.first().copied();
                d.time = columns.iter().skip(1).copied().find(|&c| kit::is_numeric(table, c));
            }
            ReliabilityKind::TestPlan(_) => {}
            ReliabilityKind::LifeDistribution(d) => d.prefill(columns, table),
            ReliabilityKind::CompetingRisks(d) => d.prefill(columns, table),
            ReliabilityKind::Warranty(d) => d.prefill(columns, table),
            ReliabilityKind::LifeStress(d) => d.prefill(columns, table),
            ReliabilityKind::Degradation(d) => d.prefill(columns, table),
            ReliabilityKind::ProportionalHazards(d) => d.prefill(columns, table),
            ReliabilityKind::SurvivalRegression(d) => d.prefill(columns, table),
            ReliabilityKind::Fatigue(d) => d.prefill(columns, table),
            ReliabilityKind::Pharmacokinetics(d) => d.prefill(columns, table),
            ReliabilityKind::PrincipalCoordinates(d) => d.prefill(columns, table),
        }
    }
}

fn confidence(ui: &mut egui::Ui, value: &mut f64) {
    ui.label("Confidence level:");
    ui.add(egui::DragValue::new(value).range(0.5..=0.999).speed(0.005).max_decimals(3));
    ui.end_row();
}

/// Reliability growth: failure times, optional event and phase columns.
#[derive(Clone)]
pub struct GrowthDialog {
    time: Option<usize>,
    event: Option<usize>,
    phase: Option<usize>,
    time_terminated: bool,
    end_time: f64,
    estimator: BetaEstimator,
    confidence: f64,
}

impl Default for GrowthDialog {
    fn default() -> Self {
        Self {
            time: None,
            event: None,
            phase: None,
            time_terminated: false,
            end_time: 1000.0,
            estimator: BetaEstimator::Unbiased,
            confidence: GrowthRequest::DEFAULT_CONFIDENCE,
        }
    }
}

impl GrowthDialog {
    fn from_request(r: &GrowthRequest, table: &DataTable) -> Self {
        let valid = |c: usize| kit::valid(table, c);
        Self {
            time: valid(r.time),
            event: r.event.and_then(valid),
            phase: r.phase.and_then(valid),
            time_terminated: r.end_time.is_some(),
            end_time: r.end_time.unwrap_or(1000.0),
            estimator: r.estimator,
            confidence: r.confidence,
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "Power-law (Crow–AMSAA) model of the failures of one system under development: one row per failure, \
             at its cumulative test time.",
        );
        let numeric = |i: usize| kit::is_numeric(table, i);
        egui::Grid::new("growth_columns").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Cumulative time:");
            kit::column_choice(ui, "growth_time", table, &mut self.time, numeric, None);
            ui.end_row();
            ui.label("Event (optional):");
            kit::column_choice(
                ui,
                "growth_event",
                table,
                &mut self.event,
                numeric,
                Some("None: every row is a failure"),
            );
            ui.end_row();
            ui.label("");
            kit::note(ui, "0 = end of the test (or phase) without a failure; other values = failure.");
            ui.end_row();
            ui.label("Phase (optional):");
            kit::column_choice(ui, "growth_phase", table, &mut self.phase, |_| true, Some("None"));
            ui.end_row();
            ui.label("Test ended:");
            ui.vertical(|ui| {
                ui.radio_value(&mut self.time_terminated, false, "At the last failure (or an end-of-test row)");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.time_terminated, true, "At time");
                    ui.add_enabled(
                        self.time_terminated,
                        egui::DragValue::new(&mut self.end_time).range(1e-9..=1e12).speed(1.0),
                    );
                });
            });
            ui.end_row();
        });
        kit::more_options(ui, |ui| {
            egui::Grid::new("growth_settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("Shape estimate:");
                ui.vertical(|ui| {
                    ui.radio_value(&mut self.estimator, BetaEstimator::Unbiased, "Unbiased");
                    ui.radio_value(&mut self.estimator, BetaEstimator::MaximumLikelihood, "Maximum likelihood");
                });
                ui.end_row();
                confidence(ui, &mut self.confidence);
            });
        });
        let Some(time) = self.time else { return Err("Choose a cumulative time column".into()) };
        if self.event == Some(time) {
            return Err("Event and time must be different columns".into());
        }
        if self.phase == Some(time) {
            return Err("Phase and time must be different columns".into());
        }
        let mut summary = format!("Reliability growth of {}", kit::name(table, time));
        if let Some(p) = self.phase {
            summary.push_str(&format!(" by {}", kit::name(table, p)));
        }
        if self.time_terminated {
            summary.push_str(&format!(", test ended at {}", num(self.end_time)));
        }
        let request = GrowthRequest {
            time,
            event: self.event,
            phase: self.phase,
            end_time: self.time_terminated.then_some(self.end_time),
            estimator: self.estimator,
            confidence: self.confidence,
        };
        kit::ready(SavedReport::Growth { request }, summary)
    }
}

/// Repairable systems: system, age, optional event and population columns.
#[derive(Clone)]
pub struct RecurrenceDialog {
    system: Option<usize>,
    time: Option<usize>,
    event: Option<usize>,
    group: Option<usize>,
    variance: McfVariance,
    limits: McfLimits,
    confidence: f64,
}

impl Default for RecurrenceDialog {
    fn default() -> Self {
        Self {
            system: None,
            time: None,
            event: None,
            group: None,
            variance: McfVariance::Robust,
            limits: McfLimits::Log,
            confidence: RecurrenceRequest::DEFAULT_CONFIDENCE,
        }
    }
}

impl RecurrenceDialog {
    fn from_request(r: &RecurrenceRequest, table: &DataTable) -> Self {
        let valid = |c: usize| kit::valid(table, c);
        Self {
            system: valid(r.system),
            time: valid(r.time),
            event: r.event.and_then(valid),
            group: r.group.and_then(valid),
            variance: r.variance,
            limits: r.limits,
            confidence: r.confidence,
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "Mean cumulative number of repairs per system (Nelson), process fits and trend tests: one row per \
             repair, with the system and its age.",
        );
        let numeric = |i: usize| kit::is_numeric(table, i);
        egui::Grid::new("recurrence_columns").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("System:");
            kit::column_choice(ui, "recurrence_system", table, &mut self.system, |_| true, None);
            ui.end_row();
            ui.label("Age:");
            kit::column_choice(ui, "recurrence_time", table, &mut self.time, numeric, None);
            ui.end_row();
            ui.label("Event (optional):");
            kit::column_choice(
                ui,
                "recurrence_event",
                table,
                &mut self.event,
                numeric,
                Some("None: every row is a repair"),
            );
            ui.end_row();
            ui.label("");
            kit::note(ui, "0 = end of observation of that system; other values = repair.");
            ui.end_row();
            ui.label("Populations (optional):");
            kit::column_choice(ui, "recurrence_group", table, &mut self.group, |_| true, Some("None"));
            ui.end_row();
        });
        kit::more_options(ui, |ui| {
            egui::Grid::new("recurrence_settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("Variance:");
                ui.vertical(|ui| {
                    ui.radio_value(&mut self.variance, McfVariance::Robust, "Robust (no Poisson assumption)");
                    ui.radio_value(&mut self.variance, McfVariance::Poisson, "Poisson process");
                });
                ui.end_row();
                ui.label("Limits:");
                ui.vertical(|ui| {
                    ui.radio_value(&mut self.limits, McfLimits::Log, "Log (never negative)");
                    ui.radio_value(&mut self.limits, McfLimits::Normal, "Normal (can fall below zero)");
                });
                ui.end_row();
                confidence(ui, &mut self.confidence);
            });
        });
        let Some(system) = self.system else { return Err("Choose the system column".into()) };
        let Some(time) = self.time else { return Err("Choose the age column".into()) };
        let request = RecurrenceRequest {
            system,
            time,
            event: self.event,
            group: self.group,
            variance: self.variance,
            limits: self.limits,
            confidence: self.confidence,
        };
        let cols = request.columns();
        if (1..cols.len()).any(|i| cols[..i].contains(&cols[i])) {
            return Err("Choose a different column for each role".into());
        }
        let mut summary =
            format!("Mean cumulative repairs per {} by {}", kit::name(table, system), kit::name(table, time));
        if let Some(g) = self.group {
            summary.push_str(&format!(", populations of {}", kit::name(table, g)));
        }
        kit::ready(SavedReport::Recurrence { request }, summary)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PlanKind {
    Demonstration,
    Estimation,
    Accelerated,
}

/// Test plans: no columns.
#[derive(Clone)]
pub struct TestPlanDialog {
    kind: PlanKind,
    demonstration: DemonstrationRequest,
    by_units: bool,
    test_time: f64,
    units: usize,
    estimation: EstimationRequest,
    accelerated: AcceleratedRequest,
    own_plan: bool,
    levels: Vec<(f64, f64)>,
}

impl Default for TestPlanDialog {
    fn default() -> Self {
        Self {
            kind: PlanKind::Demonstration,
            demonstration: DemonstrationRequest::default(),
            by_units: true,
            test_time: 1000.0,
            units: 20,
            estimation: EstimationRequest::default(),
            accelerated: AcceleratedRequest::default(),
            own_plan: false,
            levels: vec![(80.0, 0.5), (100.0, 0.3), (120.0, 0.2)],
        }
    }
}

fn probability(ui: &mut egui::Ui, label: &str, value: &mut f64) {
    ui.label(label);
    ui.add(egui::DragValue::new(value).range(1e-6..=0.999_999).speed(0.001).max_decimals(6));
    ui.end_row();
}

fn positive(ui: &mut egui::Ui, label: &str, value: &mut f64) {
    ui.label(label);
    ui.add(egui::DragValue::new(value).range(1e-9..=1e12).speed(0.1));
    ui.end_row();
}

fn distribution(ui: &mut egui::Ui, value: &mut LifeDistribution, spread: &mut f64) {
    ui.label("Life distribution:");
    ui.horizontal(|ui| {
        ui.radio_value(value, LifeDistribution::Weibull, "Weibull");
        ui.radio_value(value, LifeDistribution::Lognormal, "Lognormal");
    });
    ui.end_row();
    let label = match value {
        LifeDistribution::Weibull => "Weibull shape β (planning value):",
        LifeDistribution::Lognormal => "Lognormal σ (planning value):",
    };
    positive(ui, label, spread);
}

/// "B10" for the 10th percentile.
fn percentile_name(p: f64) -> String {
    format!("B{}", num(p * 100.0))
}

impl TestPlanDialog {
    fn from_request(request: &TestPlanRequest) -> Self {
        let mut d = Self::default();
        match request {
            TestPlanRequest::Demonstration(r) => {
                d.kind = PlanKind::Demonstration;
                d.demonstration = *r;
                match r.solve {
                    DemonstrationSolve::Units { test_time } => {
                        d.by_units = true;
                        d.test_time = test_time;
                    }
                    DemonstrationSolve::TestTime { units } => {
                        d.by_units = false;
                        d.units = units;
                    }
                }
            }
            TestPlanRequest::Estimation(r) => {
                d.kind = PlanKind::Estimation;
                d.estimation = *r;
            }
            TestPlanRequest::Accelerated(r) => {
                d.kind = PlanKind::Accelerated;
                d.accelerated = r.clone();
                if !r.levels.is_empty() {
                    d.own_plan = true;
                    d.levels.clone_from(&r.levels);
                }
            }
        }
        d
    }

    fn ui(&mut self, ui: &mut egui::Ui) -> Readiness<SavedReport> {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.kind, PlanKind::Demonstration, "Demonstration");
            ui.selectable_value(&mut self.kind, PlanKind::Estimation, "Estimation");
            ui.selectable_value(&mut self.kind, PlanKind::Accelerated, "Accelerated");
        });
        ui.separator();
        let (request, summary) = match self.kind {
            PlanKind::Demonstration => {
                kit::note(ui, "Units and test time to show a reliability with a confidence, allowing a few failures.");
                let d = &mut self.demonstration;
                egui::Grid::new("plan_demo").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    probability(ui, "Reliability to demonstrate:", &mut d.reliability);
                    positive(ui, "At time:", &mut d.time);
                    probability(ui, "Confidence:", &mut d.confidence);
                    positive(ui, "Assumed Weibull shape β:", &mut d.shape);
                    ui.label("Failures allowed:");
                    ui.add(egui::DragValue::new(&mut d.failures).range(0..=50));
                    ui.end_row();
                    ui.label("Find:");
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut self.by_units, true, "Units, testing each for");
                            ui.add_enabled(
                                self.by_units,
                                egui::DragValue::new(&mut self.test_time).range(1e-9..=1e12).speed(1.0),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut self.by_units, false, "Test time, with units");
                            ui.add_enabled(!self.by_units, egui::DragValue::new(&mut self.units).range(1..=1_000_000));
                        });
                    });
                    ui.end_row();
                });
                d.solve = if self.by_units {
                    DemonstrationSolve::Units { test_time: self.test_time }
                } else {
                    DemonstrationSolve::TestTime { units: self.units }
                };
                let find = if self.by_units { "units needed" } else { "test time needed" };
                let summary = format!(
                    "Demonstration plan: {find} to show reliability {} at {} with {} confidence",
                    num(d.reliability),
                    num(d.time),
                    num(d.confidence)
                );
                (TestPlanRequest::Demonstration(*d), summary)
            }
            PlanKind::Estimation => {
                kit::note(ui, "Units needed to estimate a percentile of life within a precision factor.");
                let e = &mut self.estimation;
                egui::Grid::new("plan_estimation").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    distribution(ui, &mut e.distribution, &mut e.spread);
                    ui.label("Fraction failing by the end of the test:");
                    ui.add(
                        egui::DragValue::new(&mut e.fraction_failing).range(0.001..=1.0).speed(0.005).max_decimals(4),
                    );
                    ui.end_row();
                    probability(ui, "Percentile to estimate (0.1 = B10):", &mut e.percentile);
                    probability(ui, "Confidence:", &mut e.confidence);
                    ui.label("Precision factor R (interval t̂/R to t̂·R):");
                    ui.add(egui::DragValue::new(&mut e.precision).range(1.01..=100.0).speed(0.01).max_decimals(3));
                    ui.end_row();
                });
                let summary = format!(
                    "Estimation plan: units to estimate {} life within a factor of {}",
                    percentile_name(e.percentile),
                    num(e.precision)
                );
                (TestPlanRequest::Estimation(*e), summary)
            }
            PlanKind::Accelerated => {
                kit::note(
                    ui,
                    "Optimum and compromise plans for a test at raised stress, to estimate a percentile of life at \
                     use conditions.",
                );
                let a = &mut self.accelerated;
                egui::Grid::new("plan_accelerated").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    distribution(ui, &mut a.distribution, &mut a.spread);
                    ui.label("Stress relationship:");
                    egui::ComboBox::from_id_salt("plan_relationship").selected_text(a.relationship.label()).show_list(
                        ui,
                        |ui| {
                            for r in StressRelationship::ALL {
                                ui.selectable_value(&mut a.relationship, r, r.label());
                            }
                        },
                    );
                    ui.end_row();
                    ui.label("Use stress:");
                    ui.add(egui::DragValue::new(&mut a.use_stress).speed(1.0));
                    ui.end_row();
                    ui.label("Highest test stress:");
                    ui.add(egui::DragValue::new(&mut a.high_stress).speed(1.0));
                    ui.end_row();
                    probability(ui, "P(fail by test end) at use:", &mut a.p_use);
                    probability(ui, "P(fail by test end) at highest stress:", &mut a.p_high);
                    probability(ui, "Percentile at use to estimate:", &mut a.percentile);
                    probability(ui, "Confidence:", &mut a.confidence);
                    ui.label("Precision factor R:");
                    ui.add(egui::DragValue::new(&mut a.precision).range(1.01..=100.0).speed(0.01).max_decimals(3));
                    ui.end_row();
                });
                kit::more_options(ui, |ui| {
                    ui.checkbox(&mut self.own_plan, "Also evaluate my own plan (stress, share of units):");
                    if self.own_plan {
                        let mut remove = None;
                        for (i, (x, f)) in self.levels.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                ui.add(egui::DragValue::new(x).speed(1.0).prefix("stress "));
                                ui.add(egui::DragValue::new(f).range(0.001..=1.0).speed(0.01).prefix("share "));
                                if ui.small_button("Remove").clicked() {
                                    remove = Some(i);
                                }
                            });
                        }
                        if let Some(i) = remove {
                            self.levels.remove(i);
                        }
                        if ui.small_button("Add level").clicked() {
                            self.levels.push((a.high_stress, 0.2));
                        }
                    }
                });
                a.levels = if self.own_plan { self.levels.clone() } else { Vec::new() };
                let mut summary = format!(
                    "Accelerated test plan for {} life at stress {}",
                    percentile_name(a.percentile),
                    num(a.use_stress)
                );
                if !a.levels.is_empty() {
                    summary.push_str(&format!(", and your plan of {}", kit::count(a.levels.len(), "level", "levels")));
                }
                (TestPlanRequest::Accelerated(a.clone()), summary)
            }
        };
        kit::ready(SavedReport::TestPlan { request }, summary)
    }
}
