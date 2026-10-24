//! Dialog of sequential optimization (Design ▸ This table ▸ Propose next
//! runs…): the responses with their goals, the factors with their ranges,
//! constraints, the round column, the criterion, the batch size and the
//! stop rule.
//!
//! The report runs through [`app_model::ComparisonReport`], so the batch,
//! criterion and stop rule are also edited in place from the report's
//! options menu ([`report_settings`]), which also appends the proposed runs
//! to the table.

use app_model::{ResponseGoal, SavedReport, SequentialRequest};
use eframe::egui;
use stat_data::DataTable;
use stat_doe::Goal;
use stat_predictive::{AcquisitionCriterion, CovarianceKernel};

use crate::dialog_kit::ComboExt as _;
use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::report_view::ReportRequest;

/// Dialog of sequential optimization.
#[derive(Clone)]
pub struct SequentialDialog {
    edit: Option<u64>,
    /// Opened from the Design menu, which cannot restore remembered
    /// settings; the app restores them before showing it.
    recall: bool,
    /// Response columns, indexed by column.
    responses: Vec<bool>,
    /// Each column's goal when it is a response, indexed by column.
    goals: Vec<ResponseGoal>,
    /// Factor columns, indexed by column.
    factors: Vec<bool>,
    /// Each continuous factor's search range, indexed by column (`None`:
    /// the table's smallest and largest settings).
    ranges: Vec<Option<(f64, f64)>>,
    round: Option<usize>,
    fix_nugget: bool,
    settings: SequentialRequest,
}

impl SequentialDialog {
    /// An empty dialog; a column named like a round is pre-chosen.
    pub fn new(table: &DataTable) -> Self {
        let n = table.n_cols();
        let round = (0..n).find(|&i| {
            let name = kit::name(table, i).to_lowercase();
            (name == "round" || name == "batch" || name == "iteration") && kit::is_numeric(table, i)
        });
        Self {
            edit: None,
            recall: false,
            responses: vec![false; n],
            goals: vec![ResponseGoal::new(Goal::Maximize); n],
            factors: vec![false; n],
            ranges: vec![None; n],
            round,
            fix_nugget: false,
            settings: SequentialRequest::new(Vec::new(), Vec::new()),
        }
    }

    /// A new dialog from a menu that cannot reach the remembered settings:
    /// the app swaps them in before showing it ([`Self::needs_recall`]).
    pub fn opening(table: &DataTable) -> Self {
        Self { recall: true, ..Self::new(table) }
    }

    /// Whether the app should restore remembered settings into this dialog
    /// (once; this clears the mark).
    pub fn needs_recall(&mut self) -> bool {
        std::mem::take(&mut self.recall)
    }

    /// The dialog filled from a saved sequential optimization.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::SequentialOptimization { request } = saved else { return None };
        let mut d = Self::new(table);
        d.responses = kit::mask(table.n_cols(), &request.responses);
        d.factors = kit::mask(table.n_cols(), &request.factors);
        for (&c, g) in request.responses.iter().zip(&request.goals) {
            if let Some(slot) = d.goals.get_mut(c) {
                *slot = *g;
            }
        }
        for (&c, b) in request.factors.iter().zip(&request.bounds) {
            if let Some(slot) = d.ranges.get_mut(c) {
                *slot = *b;
            }
        }
        d.round = request.round.and_then(|c| kit::valid(table, c));
        d.fix_nugget = request.nugget.is_some();
        d.settings = request.as_ref().clone();
        Some(d)
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<Box<ReportRequest>> {
        let dialog = Dialog::new("Propose next runs").editing(self.edit);
        let outcome = dialog.show(ctx, |ui| {
            self.contents(ui, table)
                .map(|r| kit::Ready { output: Box::new(ReportRequest::Comparison(r.output)), summary: r.summary })
        });
        match outcome {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "One row per run made so far, each with its factor settings and measured responses. A Gaussian \
             process is fitted to each response and the next runs are chosen where the model expects the \
             largest gain. Rows with their factors set but a response still missing count as runs in progress: \
             the proposals keep away from them.",
        );
        let n = table.n_cols();
        for v in [&mut self.responses, &mut self.factors] {
            v.resize(n, false);
        }
        self.goals.resize(n, ResponseGoal::new(Goal::Maximize));
        self.ranges.resize(n, None);
        let width = kit::em(ui) * 17.0;
        let round = self.round;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.strong("Responses");
                let factors = self.factors.clone();
                kit::column_list(ui, "seq_responses", table, &mut self.responses, |i| {
                    !factors[i] && Some(i) != round && kit::is_numeric(table, i)
                });
            });
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.strong("Factors");
                let responses = self.responses.clone();
                kit::column_list(ui, "seq_factors", table, &mut self.factors, |i| !responses[i] && Some(i) != round);
            });
        });
        let responses: Vec<usize> = kit::chosen_columns(&self.responses).into_iter().filter(|&c| c < n).collect();
        let factors: Vec<usize> =
            kit::chosen_columns(&self.factors).into_iter().filter(|&c| c < n && !responses.contains(&c)).collect();
        if !responses.is_empty() {
            ui.add_space(4.0);
            crate::tree_dialog::heading(ui, "Goals");
            egui::Grid::new("seq_goals").num_columns(3).spacing([8.0, 4.0]).show(ui, |ui| {
                for &c in &responses {
                    ui.label(kit::name(table, c));
                    goal_ui(ui, c, &mut self.goals[c].goal);
                    ui.end_row();
                }
            });
            if responses.len() > 1 {
                kit::note(
                    ui,
                    "Several responses are combined into one overall desirability (0 to 1), each scaled between \
                     its smallest and largest observed values unless you set limits under More options.",
                );
            }
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Round column (optional):");
            let chosen = (self.responses.clone(), self.factors.clone());
            kit::column_choice(
                ui,
                "seq_round",
                table,
                &mut self.round,
                |i| !chosen.0[i] && !chosen.1[i] && kit::is_numeric(table, i),
                Some("(none: progress by run)"),
            );
        })
        .response
        .on_hover_text("Numbers the batches of runs; the progress chart shows the best value after each round");
        crate::design_dialog::constraints_box(ui, &mut self.settings.constraints);
        proposal_settings(ui, &mut self.settings, responses.len() > 1);
        kit::more_options(ui, |ui| {
            let continuous: Vec<usize> = factors.iter().copied().filter(|&c| kit::is_numeric(table, c)).collect();
            if !continuous.is_empty() {
                crate::tree_dialog::heading(ui, "Search ranges");
                kit::note(ui, "Unticked: between the table's smallest and largest settings.");
                egui::Grid::new("seq_ranges").num_columns(4).spacing([8.0, 4.0]).show(ui, |ui| {
                    for &c in &continuous {
                        range_ui(ui, table, c, &mut self.ranges[c]);
                        ui.end_row();
                    }
                });
            }
            if responses.len() > 1 {
                crate::tree_dialog::heading(ui, "Desirability limits");
                egui::Grid::new("seq_limits").num_columns(5).spacing([8.0, 4.0]).show(ui, |ui| {
                    for &c in &responses {
                        limits_ui(ui, table, c, &mut self.goals[c]);
                        ui.end_row();
                    }
                });
            }
            crate::tree_dialog::heading(ui, "Models");
            ui.horizontal(|ui| {
                ui.label("Kernel:");
                egui::ComboBox::from_id_salt("seq_kernel").selected_text(self.settings.kernel.label()).show_list(
                    ui,
                    |ui| {
                        for k in CovarianceKernel::ALL {
                            ui.selectable_value(&mut self.settings.kernel, k, k.label());
                        }
                    },
                );
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.fix_nugget, "Fix the nugget at").on_hover_text(
                    "The noise variance as a fraction of each response's variance (10⁻⁶ for a deterministic \
                     simulator); otherwise it is estimated",
                );
                let mut v = self.settings.nugget.unwrap_or(1e-6);
                ui.add_enabled(
                    self.fix_nugget,
                    egui::DragValue::new(&mut v).speed(1e-6).range(0.0..=10.0).max_decimals(8),
                );
                self.settings.nugget = self.fix_nugget.then_some(v);
            });
            ui.horizontal(|ui| {
                ui.label("Starts:");
                ui.add(egui::DragValue::new(&mut self.settings.starts).range(1..=50))
                    .on_hover_text("Each model's likelihood is maximized from this many starting points");
                ui.label("Candidates:");
                ui.add(egui::DragValue::new(&mut self.settings.candidates).range(100..=100_000).speed(50))
                    .on_hover_text("Random settings scored for each proposed run before the best few are refined");
                ui.label("Seed:");
                ui.add(kit::seed_value(&mut self.settings.seed));
            });
        });
        if responses.is_empty() {
            return Err("Choose at least one response".into());
        }
        if factors.is_empty() {
            return Err("Choose at least one factor".into());
        }
        for &c in &factors {
            if let Some(Some((lo, hi))) = kit::is_numeric(table, c).then(|| self.ranges[c])
                && lo >= hi
            {
                return Err(format!("The range of {} needs a low setting below its high one", kit::name(table, c)));
            }
        }
        let request = SequentialRequest {
            goals: responses.iter().map(|&c| self.goals[c]).collect(),
            bounds: factors.iter().map(|&c| self.ranges[c]).collect(),
            responses: responses.clone(),
            factors: factors.clone(),
            round: self.round.filter(|&c| c < n && !responses.contains(&c) && !factors.contains(&c)),
            ..self.settings.clone()
        };
        let summary = format!(
            "Propose {} for {} over {}",
            kit::count(request.batch, "run", "runs"),
            responses.iter().map(|&c| kit::name(table, c)).collect::<Vec<_>>().join(", "),
            kit::count(factors.len(), "factor", "factors"),
        );
        kit::ready(SavedReport::SequentialOptimization { request: Box::new(request) }, summary)
    }
}

impl Remember for SequentialDialog {
    fn memory_key(&self) -> String {
        "Propose next runs".into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.factors = kit::mask(table.n_cols(), columns);
    }
}

/// Maximize, minimize or a target, for response column `c`.
fn goal_ui(ui: &mut egui::Ui, c: usize, goal: &mut Goal) {
    let label = match goal {
        Goal::Maximize => "Maximize",
        Goal::Minimize => "Minimize",
        Goal::Target(_) | Goal::None => "Match a target",
    };
    let mut target = match goal {
        Goal::Target(t) => *t,
        _ => 0.0,
    };
    egui::ComboBox::from_id_salt(("seq_goal", c)).selected_text(label).show_list(ui, |ui| {
        ui.selectable_value(goal, Goal::Maximize, "Maximize");
        ui.selectable_value(goal, Goal::Minimize, "Minimize");
        if ui.selectable_label(matches!(goal, Goal::Target(_)), "Match a target").clicked() {
            *goal = Goal::Target(target);
        }
    });
    if matches!(goal, Goal::Target(_)) {
        ui.horizontal(|ui| {
            ui.label("Target:");
            if ui.add(egui::DragValue::new(&mut target).speed(0.1)).changed() {
                *goal = Goal::Target(target);
            }
        });
    } else {
        ui.label("");
    }
}

/// An optional search range for continuous factor `c`.
fn range_ui(ui: &mut egui::Ui, table: &DataTable, c: usize, range: &mut Option<(f64, f64)>) {
    let (lo, hi) = observed_range(table, c);
    let mut on = range.is_some();
    if ui.checkbox(&mut on, kit::name(table, c)).changed() {
        *range = on.then_some((lo, hi));
    }
    let (mut a, mut b) = range.unwrap_or((lo, hi));
    let speed = ((hi - lo).abs() / 200.0).max(1e-6);
    ui.label("from");
    let low = ui.add_enabled(on, egui::DragValue::new(&mut a).speed(speed));
    let high = ui.add_enabled(on, egui::DragValue::new(&mut b).speed(speed).prefix("to "));
    if on && (low.changed() || high.changed()) {
        *range = Some((a, b));
    }
}

/// Optional desirability limits of response column `c`.
fn limits_ui(ui: &mut egui::Ui, table: &DataTable, c: usize, g: &mut ResponseGoal) {
    let (lo, hi) = observed_range(table, c);
    ui.label(kit::name(table, c));
    for (slot, default, text) in [(&mut g.low, lo, "Low"), (&mut g.high, hi, "High")] {
        let mut on = slot.is_some();
        if ui.checkbox(&mut on, text).changed() {
            *slot = on.then_some(default);
        }
        let mut v = slot.unwrap_or(default);
        if ui.add_enabled(on, egui::DragValue::new(&mut v).speed(((hi - lo).abs() / 200.0).max(1e-6))).changed() && on {
            *slot = Some(v);
        }
    }
    ui.add(egui::DragValue::new(&mut g.importance).speed(0.05).range(0.01..=10.0).prefix("importance "));
}

/// The smallest and largest finite values of numeric column `c`.
fn observed_range(table: &DataTable, c: usize) -> (f64, f64) {
    let values = table.column(c).and_then(|col| col.as_numeric()).unwrap_or(&[]);
    let lo = values.iter().copied().filter(|v| v.is_finite()).fold(f64::INFINITY, f64::min);
    let hi = values.iter().copied().filter(|v| v.is_finite()).fold(f64::NEG_INFINITY, f64::max);
    if lo.is_finite() { (lo, hi) } else { (0.0, 1.0) }
}

/// Criterion, batch size and stop rule, shared by the dialog and the
/// report's options menu.
fn proposal_settings(ui: &mut egui::Ui, r: &mut SequentialRequest, several: bool) {
    crate::tree_dialog::heading(ui, "Next runs");
    ui.horizontal(|ui| {
        ui.label("Runs to propose:");
        ui.add(egui::DragValue::new(&mut r.batch).range(1..=24)).on_hover_text(
            "Runs after the first are chosen as if the earlier proposals had come out at the best value so far, \
             so a batch spreads out",
        );
    });
    let ei = matches!(r.criterion, AcquisitionCriterion::ExpectedImprovement);
    if ui
        .radio(ei, "Expected improvement")
        .on_hover_text("The gain over the best run the model expects, counting both the prediction and its uncertainty")
        .clicked()
    {
        r.criterion = AcquisitionCriterion::ExpectedImprovement;
    }
    ui.horizontal(|ui| {
        let mut kappa = match r.criterion {
            AcquisitionCriterion::UpperConfidenceBound { kappa } => kappa,
            AcquisitionCriterion::ExpectedImprovement => 2.0,
        };
        if ui
            .radio(!ei, "Upper confidence bound, with")
            .on_hover_text("The prediction plus this many standard errors: larger values explore more")
            .clicked()
        {
            r.criterion = AcquisitionCriterion::UpperConfidenceBound { kappa };
        }
        if ui.add_enabled(!ei, egui::DragValue::new(&mut kappa).speed(0.05).range(0.0..=10.0)).changed() {
            r.criterion = AcquisitionCriterion::UpperConfidenceBound { kappa };
        }
        ui.label("standard errors");
    });
    crate::tree_dialog::heading(ui, "Stop rule");
    ui.horizontal(|ui| {
        let mut on = r.stop_value.is_some();
        let mut v = r.stop_value.unwrap_or(if several { 0.9 } else { 0.0 });
        let text =
            if several { "Stop when the best overall desirability reaches" } else { "Stop when the best run reaches" };
        if ui.checkbox(&mut on, text).on_hover_text("For a target, the distance from it").changed() {
            r.stop_value = on.then_some(v);
        }
        if ui.add_enabled(on, egui::DragValue::new(&mut v).speed(0.1)).changed() && on {
            r.stop_value = Some(v);
        }
    });
    ui.horizontal(|ui| {
        ui.label("or when the largest expected improvement is below");
        let mut pct = r.stop_improvement * 100.0;
        if ui.add(egui::DragValue::new(&mut pct).speed(0.1).range(0.0..=50.0).suffix(" %")).changed() {
            r.stop_improvement = pct / 100.0;
        }
        ui.label(if several { "(of desirability 1)" } else { "of the observed range" });
    });
}

/// In-report settings of a sequential optimization; `false` for any other
/// report.
pub fn report_settings(ui: &mut egui::Ui, saved: &mut SavedReport) -> bool {
    let SavedReport::SequentialOptimization { request } = saved else { return false };
    let several = request.responses.len() > 1;
    proposal_settings(ui, request, several);
    true
}
