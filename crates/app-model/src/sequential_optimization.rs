//! Sequential optimization of an experiment in a table: Gaussian-process
//! models of the responses measured so far propose the next batch of runs
//! ([`stat_predictive::acquisition`]).
//!
//! **Data.** The factor columns are continuous (searched between bounds,
//! by default the smallest and largest settings in the table) or
//! categorical (searched over the levels in the table). Rows with every
//! factor set and every response measured are the *runs*; rows with every
//! factor set but a response still empty are *waiting for results*: the
//! proposals treat them as already in the batch (constant liar), so adding
//! the proposed runs to the table and recomputing proposes the batch after
//! them. Excluded rows are left out.
//!
//! **Goals.** One response is maximized, minimized or brought to a target
//! (closed-form expected improvement); several are combined by an overall
//! desirability (Derringer & Suich; each response's limits default to its
//! observed range), whose expected improvement is estimated by Monte
//! Carlo. An upper confidence bound can replace expected improvement.
//!
//! **Constraints** are linear inequalities and excluded combinations of
//! the continuous factors in their own units, one per line, as for optimal
//! designs ([`stat_doe::Region::parse`]). The report gives only their
//! number: it never lists or draws the excluded combinations beside the
//! proposals, and the proposals are chosen by the search, never picked by
//! the user from scored candidates (see "Patent design-arounds" in
//! CLAUDE.md before changing either).
//!
//! **Progress and stopping.** The best value (or desirability) so far is
//! charted by round, from an optional round column, or by run in table
//! order. The stop rule reports *goal met* when the best run reaches the
//! request's stop value, and (for expected improvement) *little left to
//! gain* when the largest expected improvement is below the request's
//! fraction of the response's observed range (of 1 for desirability).

use stat_core::{Result, StatError};
use stat_data::{
    CellValue, ColumnKind, DataTable, JsonValue, MeasurementLevel, RowState, RowStateManager, table_ops::natural_order,
};
use stat_doe::{Desirability, Factor, Goal, Region, overall_desirability};
use stat_predictive::acquisition::{self, AcquisitionCriterion, Candidate, Objective, ProposalOptions, SearchSpace};
use stat_predictive::{CovarianceKernel, GaussianProcess, GpOptions, Nugget, fit_gaussian_process};

use crate::editing::Edit;
use crate::format::format_number;
use crate::gaussian_process_analysis::{kernel_from_json, nugget_from_json};
use crate::multiple_responses::goal_text;
use crate::report::{PlotSpec, ReportSection, RowPoint, SectionContent, SectionKind, TextTable};

const DIGITS: usize = 6;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// What is wanted of one response, with the limits of its desirability
/// (used when several responses are combined).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResponseGoal {
    /// Maximize, minimize or a target.
    pub goal: Goal,
    /// Desirability 0 (maximize) or 1 (minimize) at or below this value
    /// (`None`: the smallest observed value).
    pub low: Option<f64>,
    /// Desirability 1 (maximize) or 0 (minimize) at or above this value
    /// (`None`: the largest observed value).
    pub high: Option<f64>,
    /// Importance in the overall desirability.
    pub importance: f64,
}

impl ResponseGoal {
    /// The goal with limits from the data and importance 1.
    #[must_use]
    pub fn new(goal: Goal) -> Self {
        Self { goal, low: None, high: None, importance: 1.0 }
    }
}

/// Columns and settings of a sequential optimization.
#[derive(Debug, Clone, PartialEq)]
pub struct SequentialRequest {
    /// Response columns.
    pub responses: Vec<usize>,
    /// Each response's goal.
    pub goals: Vec<ResponseGoal>,
    /// Factor columns.
    pub factors: Vec<usize>,
    /// Bounds of each continuous factor (`None`, or a missing entry: the
    /// table's smallest and largest settings); ignored for categorical ones.
    pub bounds: Vec<Option<(f64, f64)>>,
    /// Constraints in the factors' units, one per line (empty: none).
    pub constraints: String,
    /// Optional column numbering the rounds of runs.
    pub round: Option<usize>,
    /// Ranking criterion.
    pub criterion: AcquisitionCriterion,
    /// Runs to propose.
    pub batch: usize,
    /// Correlation function of the models.
    pub kernel: CovarianceKernel,
    /// A fixed nugget as a fraction of each response's variance (`None`:
    /// estimated).
    pub nugget: Option<f64>,
    /// Starts of each model's likelihood search.
    pub starts: usize,
    /// Stop when the largest expected improvement is below this fraction
    /// of the observed range (of 1 for desirability).
    pub stop_improvement: f64,
    /// Stop when the best run reaches this value (a response value when
    /// maximizing or minimizing, a distance from the target, or a
    /// desirability).
    pub stop_value: Option<f64>,
    /// Random candidates per proposed run.
    pub candidates: usize,
    /// Seed of the model starts, candidates and Monte Carlo draws.
    pub seed: u64,
}

impl SequentialRequest {
    /// Maximize every response (limits from the data), four runs by
    /// expected improvement, Matérn 5/2 models with the nugget estimated.
    #[must_use]
    pub fn new(responses: Vec<usize>, factors: Vec<usize>) -> Self {
        let goals = vec![ResponseGoal::new(Goal::Maximize); responses.len()];
        Self {
            responses,
            goals,
            factors,
            bounds: Vec::new(),
            constraints: String::new(),
            round: None,
            criterion: AcquisitionCriterion::ExpectedImprovement,
            batch: 4,
            kernel: CovarianceKernel::Matern52,
            nugget: None,
            starts: 5,
            stop_improvement: 0.01,
            stop_value: None,
            candidates: 2000,
            seed: 1,
        }
    }

    /// Columns read.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut c = self.responses.clone();
        c.extend(&self.factors);
        c.extend(self.round);
        c
    }

    /// The same request with column indices passed through `f`.
    #[must_use]
    pub fn map_columns(mut self, f: impl Fn(usize) -> usize) -> Self {
        self.responses.iter_mut().for_each(|c| *c = f(*c));
        self.factors.iter_mut().for_each(|c| *c = f(*c));
        self.round = self.round.map(&f);
        self
    }
}

/// A proposed factor setting.
#[derive(Debug, Clone, PartialEq)]
enum CellSetting {
    /// A continuous setting.
    Number(f64),
    /// A level label.
    Level(String),
}

/// How a factor is searched.
#[derive(Debug, Clone, PartialEq)]
enum FactorSpace {
    /// Between bounds.
    Continuous { low: f64, high: f64 },
    /// Over levels (labels as the data view shows them).
    Categorical { levels: Vec<String> },
}

#[derive(Debug, Clone)]
struct FactorInfo {
    name: String,
    column: usize,
    space: FactorSpace,
}

/// A row's place on the progress chart.
#[derive(Debug, Clone, PartialEq)]
struct Progress {
    /// Horizontal positions with their labels (rounds or runs).
    steps: Vec<(f64, String)>,
    /// Best value through each step.
    best: Vec<f64>,
    /// Each run's position.
    positions: Vec<(usize, f64)>,
}

/// The proposals for an experiment's next runs.
#[derive(Debug, Clone)]
pub struct SequentialAnalysis {
    request: SequentialRequest,
    response_names: Vec<String>,
    goals: Vec<Desirability>,
    factors: Vec<FactorInfo>,
    /// Rows used as runs.
    runs: Vec<usize>,
    /// Rows waiting for results.
    pending: Vec<usize>,
    /// Each run's response values (per run, per response).
    observed: Vec<Vec<f64>>,
    /// Each run's factor settings as the data view shows them.
    labels: Vec<Vec<String>>,
    /// Each run's value: the response, or the overall desirability.
    values: Vec<f64>,
    models: Vec<GaussianProcess>,
    proposals: Vec<acquisition::Proposal>,
    progress: Progress,
    round_name: Option<String>,
    /// Round given to the proposed runs.
    next_round: Option<CellValue>,
    /// Number of constraints (their text stays in the request only).
    constraint_count: usize,
}

impl SequentialAnalysis {
    /// Fit the response models and propose the next runs.
    ///
    /// # Errors
    /// No responses or factors, a column used twice, a goal missing or
    /// invalid, a date-time or unusable factor, bounds that do not span a
    /// range, invalid constraints, fewer than three complete runs, a model
    /// that cannot be fitted, or constraints that no setting within the ranges satisfies.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &SequentialRequest) -> Result<Self> {
        if request.responses.is_empty() || request.factors.is_empty() {
            return Err(invalid("choose at least one response and one factor"));
        }
        let mut used = request.columns();
        used.sort_unstable();
        if used.windows(2).any(|w| w[0] == w[1]) {
            return Err(invalid("a column can be only one of response, factor or round"));
        }
        if request.goals.len() != request.responses.len() {
            return Err(invalid("give every response a goal"));
        }
        if request.goals.iter().any(|g| g.goal == Goal::None) {
            return Err(invalid("give every response a goal (maximize, minimize or a target)"));
        }
        if request.batch == 0 || request.batch > 50 {
            return Err(invalid("propose 1 to 50 runs"));
        }
        if request.starts == 0 || request.starts > 100 {
            return Err(invalid("use 1 to 100 model starts"));
        }
        if request.nugget.is_some_and(|v| !(v >= 0.0 && v.is_finite())) {
            return Err(invalid("the nugget must not be negative"));
        }
        if !(request.stop_improvement >= 0.0 && request.stop_improvement.is_finite()) {
            return Err(invalid("the expected-improvement stop fraction must not be negative"));
        }
        if let AcquisitionCriterion::UpperConfidenceBound { kappa } = request.criterion
            && !(kappa >= 0.0 && kappa.is_finite())
        {
            return Err(invalid("the confidence-bound width must not be negative"));
        }
        let column = |c: usize| table.column(c).ok_or_else(|| invalid(format!("no column {c}")));
        let included: Vec<usize> = (0..table.n_rows()).filter(|&r| !states.is_excluded(r)).collect();
        let mut response_values = Vec::new();
        let mut response_names = Vec::new();
        for &c in &request.responses {
            let col = column(c)?;
            let v = col
                .as_numeric()
                .filter(|_| !matches!(col.kind(), ColumnKind::DateTime(_)))
                .ok_or_else(|| invalid(format!("the response {:?} must be numeric", col.name)))?;
            response_values.push(v.to_vec());
            response_names.push(col.name.clone());
        }
        // Factors and each row's setting of them.
        let mut factors = Vec::new();
        let mut settings: Vec<Vec<Option<f64>>> = Vec::new(); // per factor: value or level index, per row
        for (j, &c) in request.factors.iter().enumerate() {
            let col = column(c)?;
            if matches!(col.kind(), ColumnKind::DateTime(_)) {
                return Err(invalid(format!(
                    "the factor {:?} is a date or time; use a numeric or categorical factor",
                    col.name
                )));
            }
            match (col.as_numeric(), col.level) {
                (Some(v), MeasurementLevel::Continuous) => {
                    let present: Vec<f64> = included.iter().map(|&r| v[r]).filter(|x| x.is_finite()).collect();
                    let (lo, hi) = match request.bounds.get(j).copied().flatten() {
                        Some(b) => b,
                        None => (
                            present.iter().copied().fold(f64::INFINITY, f64::min),
                            present.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                        ),
                    };
                    if !(lo.is_finite() && hi.is_finite() && lo < hi) {
                        return Err(invalid(format!(
                            "the factor {:?} needs a lower bound below its upper bound",
                            col.name
                        )));
                    }
                    settings.push(v.iter().map(|x| x.is_finite().then_some(*x)).collect());
                    factors.push(FactorInfo {
                        name: col.name.clone(),
                        column: c,
                        space: FactorSpace::Continuous { low: lo, high: hi },
                    });
                }
                _ => {
                    let labels: Vec<String> = (0..table.n_rows()).map(|r| col.cell_text(r)).collect();
                    let mut levels: Vec<String> =
                        included.iter().map(|&r| labels[r].clone()).filter(|l| !l.trim().is_empty()).collect();
                    levels.sort_by(|a, b| natural_order(a, b));
                    levels.dedup();
                    if levels.len() < 2 {
                        return Err(invalid(format!(
                            "the categorical factor {:?} needs at least two levels",
                            col.name
                        )));
                    }
                    settings
                        .push(labels.iter().map(|l| levels.iter().position(|k| k == l).map(|k| k as f64)).collect());
                    factors.push(FactorInfo {
                        name: col.name.clone(),
                        column: c,
                        space: FactorSpace::Categorical { levels },
                    });
                }
            }
        }
        // Constraints over the continuous factors.
        let continuous: Vec<usize> =
            (0..factors.len()).filter(|&j| matches!(factors[j].space, FactorSpace::Continuous { .. })).collect();
        let doe_factors: Vec<Factor> = continuous
            .iter()
            .map(|&j| match factors[j].space {
                FactorSpace::Continuous { low, high } => Factor::new(factors[j].name.clone(), low, high),
                FactorSpace::Categorical { .. } => unreachable!("continuous"),
            })
            .collect();
        let region = if request.constraints.trim().is_empty() {
            Region::cube(continuous.len())
        } else {
            if continuous.is_empty() {
                return Err(invalid("constraints apply to continuous factors, and there are none"));
            }
            Region::parse(&request.constraints, &doe_factors)?
        };
        let constraint_count = region.describe().len();
        let level_counts: Vec<usize> = factors
            .iter()
            .filter_map(|f| match &f.space {
                FactorSpace::Categorical { levels } => Some(levels.len()),
                FactorSpace::Continuous { .. } => None,
            })
            .collect();
        let candidate = |r: usize| -> Option<Candidate> {
            let mut c = Candidate { continuous: Vec::new(), levels: Vec::new() };
            for (j, f) in factors.iter().enumerate() {
                let v = settings[j][r]?;
                match f.space {
                    FactorSpace::Continuous { low, high } => c.continuous.push((v - low) / (high - low)),
                    FactorSpace::Categorical { .. } => c.levels.push(v as usize),
                }
            }
            Some(c)
        };
        let mut runs = Vec::new();
        let mut pending = Vec::new();
        let mut run_points = Vec::new();
        let mut pending_points = Vec::new();
        for &r in &included {
            let Some(c) = candidate(r) else { continue };
            if response_values.iter().all(|y| y[r].is_finite()) {
                runs.push(r);
                run_points.push(c);
            } else {
                pending.push(r);
                pending_points.push(c);
            }
        }
        if runs.len() < 3 {
            return Err(invalid(
                "sequential optimization needs at least three runs with every factor set and every response measured",
            ));
        }
        let observed: Vec<Vec<f64>> = runs.iter().map(|&r| response_values.iter().map(|y| y[r]).collect()).collect();
        let labels: Vec<Vec<String>> = runs
            .iter()
            .map(|&r| {
                factors.iter().map(|f| table.column(f.column).map_or(String::new(), |c| c.cell_text(r))).collect()
            })
            .collect();
        // Goals with their limits filled in from the data.
        let goals: Vec<Desirability> = request
            .goals
            .iter()
            .enumerate()
            .map(|(k, g)| {
                let ys: Vec<f64> = observed.iter().map(|o| o[k]).collect();
                let lo = ys.iter().copied().fold(f64::INFINITY, f64::min);
                let hi = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                let span = (hi - lo).max(1e-9 * (1.0 + lo.abs()));
                let (dlo, dhi) = match g.goal {
                    Goal::Target(t) => (if lo < t { lo } else { t - span }, if hi > t { hi } else { t + span }),
                    _ => (lo, if hi > lo { hi } else { lo + span }),
                };
                let mut d = Desirability::new(g.goal, g.low.unwrap_or(dlo), g.high.unwrap_or(dhi));
                d.importance = g.importance;
                d.validate().map_err(|e| invalid(format!("{}: {e}", response_names[k])))?;
                Ok(d)
            })
            .collect::<Result<_>>()?;
        let feasible = |u: &[f64]| {
            let coded: Vec<f64> = u.iter().map(|v| 2.0 * v - 1.0).collect();
            region.contains(&coded)
        };
        let space = SearchSpace { continuous: continuous.len(), levels: level_counts, feasible: &feasible };
        let x: Vec<Vec<f64>> = run_points.iter().map(|c| space.encode(c)).collect();
        let options = GpOptions {
            kernel: request.kernel,
            nugget: request.nugget.map_or(Nugget::Estimated, Nugget::Fixed),
            starts: request.starts,
            seed: request.seed,
            ..GpOptions::default()
        };
        let models: Vec<GaussianProcess> = (0..request.responses.len())
            .map(|k| {
                let y: Vec<f64> = observed.iter().map(|o| o[k]).collect();
                fit_gaussian_process(&x, &y, &options)
                    .map_err(|e| invalid(format!("the model of {} could not be fitted: {e}", response_names[k])))
            })
            .collect::<Result<_>>()?;
        let weights: Vec<f64> = goals.iter().map(|g| g.importance).collect();
        let overall = |ys: &[f64]| {
            let d: Vec<f64> = goals.iter().zip(ys).map(|(g, &y)| g.of(y)).collect();
            overall_desirability(&d, &weights)
        };
        let engine_goal = |g: &Desirability| match g.goal {
            Goal::Maximize | Goal::None => acquisition::Goal::Maximize,
            Goal::Minimize => acquisition::Goal::Minimize,
            Goal::Target(t) => acquisition::Goal::Target(t),
        };
        let single = goals.len() == 1;
        let objective = if single {
            Objective::Single(engine_goal(&goals[0]))
        } else {
            Objective::Desirability {
                desirability: &overall,
                goals: goals.iter().map(engine_goal).collect(),
                samples: 256,
            }
        };
        let values: Vec<f64> = observed.iter().map(|o| if single { o[0] } else { overall(o) }).collect();
        let proposal_options = ProposalOptions {
            criterion: request.criterion,
            batch: request.batch,
            candidates: request.candidates.clamp(100, 100_000),
            seed: request.seed,
        };
        let proposals = acquisition::propose_batch(&models, &objective, &space, &pending_points, &proposal_options)?;
        // Progress by round or by run.
        let score = |v: f64| if single { engine_goal(&goals[0]).score(v) } else { v };
        let (progress, round_name, next_round) = progress(table, request.round, &runs, &values, &score)?;
        Ok(Self {
            request: request.clone(),
            response_names,
            goals,
            factors,
            runs,
            pending,
            observed,
            labels,
            values,
            models,
            proposals,
            progress,
            round_name,
            next_round,
            constraint_count,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &SequentialRequest {
        &self.request
    }

    /// The proposed runs.
    #[must_use]
    pub fn proposals(&self) -> &[acquisition::Proposal] {
        &self.proposals
    }

    /// The response models (inputs: continuous factors scaled to 0–1, then
    /// an indicator per level).
    #[must_use]
    pub fn models(&self) -> &[GaussianProcess] {
        &self.models
    }

    fn single(&self) -> bool {
        self.goals.len() == 1
    }

    fn score(&self, v: f64) -> f64 {
        if self.single() {
            match self.goals[0].goal {
                Goal::Minimize => -v,
                Goal::Target(t) => -(v - t).abs(),
                _ => v,
            }
        } else {
            v
        }
    }

    /// Index (into the runs) of the best run.
    fn best(&self) -> usize {
        (0..self.values.len())
            .max_by(|&a, &b| self.score(self.values[a]).total_cmp(&self.score(self.values[b])))
            .unwrap_or(0)
    }

    /// What the runs' values are called.
    fn value_name(&self) -> String {
        if self.single() { self.response_names[0].clone() } else { "Desirability".to_owned() }
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Sequential Optimization: {}", self.response_names.join(", "))
    }

    /// Factor settings of a proposal in the factors' units (continuous
    /// settings rounded to four significant digits of the range).
    fn decoded(&self, p: &Candidate) -> Vec<CellSetting> {
        let (mut ci, mut li) = (0, 0);
        self.factors
            .iter()
            .map(|f| match &f.space {
                FactorSpace::Continuous { low, high } => {
                    let v = low + p.continuous[ci] * (high - low);
                    ci += 1;
                    // Four significant digits of the range; dividing by a power
                    // of ten (rather than multiplying by its inverse) keeps
                    // the rounded value the nearest double to its decimal.
                    let e = ((high - low).log10()).floor() as i32 - 3;
                    let rounded = if e < 0 {
                        let f = 10f64.powi(-e);
                        (v * f).round() / f
                    } else {
                        let f = 10f64.powi(e);
                        (v / f).round() * f
                    };
                    CellSetting::Number(rounded.clamp(*low, *high))
                }
                FactorSpace::Categorical { levels } => {
                    let l = levels[p.levels[li]].clone();
                    li += 1;
                    CellSetting::Level(l)
                }
            })
            .collect()
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        vec![
            self.summary_section(),
            self.models_section(),
            self.best_section(),
            self.proposal_section(),
            self.progress_section(),
            self.stop_section(),
        ]
    }

    fn criterion_label(&self) -> String {
        match self.request.criterion {
            AcquisitionCriterion::ExpectedImprovement => "Expected improvement".to_owned(),
            AcquisitionCriterion::UpperConfidenceBound { kappa } => {
                format!("Upper confidence bound (κ = {})", format_number(kappa, 3))
            }
        }
    }

    fn summary_section(&self) -> ReportSection {
        let mut t = TextTable::new(["Setting", "Value"]);
        for (name, g) in self.response_names.iter().zip(&self.goals) {
            let (goal, target) = goal_text(g.goal);
            let target = target.map_or(String::new(), |v| format!(" {}", format_number(v, DIGITS)));
            let limits = if self.single() {
                String::new()
            } else {
                format!(
                    "; desirability 0–1 between {} and {}, importance {}",
                    format_number(g.low, DIGITS),
                    format_number(g.high, DIGITS),
                    format_number(g.importance, 3)
                )
            };
            t.push([format!("Response {name}"), format!("{goal}{target}{limits}")]);
        }
        for f in &self.factors {
            let range = match &f.space {
                FactorSpace::Continuous { low, high } => {
                    format!("continuous, {} to {}", format_number(*low, DIGITS), format_number(*high, DIGITS))
                }
                FactorSpace::Categorical { levels } => format!("categorical: {}", levels.join(", ")),
            };
            t.push([format!("Factor {}", f.name), range]);
        }
        // A count only: listing the excluded combinations beside the
        // proposals is avoided on purpose (CLAUDE.md, "Patent design-arounds").
        match self.constraint_count {
            0 => {}
            1 => t.push(["Constraints".to_owned(), "1 (as entered; every proposed run satisfies it)".to_owned()]),
            n => t.push(["Constraints".to_owned(), format!("{n} (as entered; every proposed run satisfies them)")]),
        }
        t.push(["Runs with results".to_owned(), self.runs.len().to_string()]);
        t.push([
            "Runs waiting for results".to_owned(),
            if self.pending.is_empty() {
                "none".to_owned()
            } else {
                format!("{} (treated as already in the batch)", self.pending.len())
            },
        ]);
        let combine = if self.single() {
            String::new()
        } else {
            "; several responses combine by overall desirability (Monte Carlo, 256 draws)".to_owned()
        };
        t.push(["Criterion".to_owned(), format!("{}{combine}", self.criterion_label())]);
        t.push([
            "Batch".to_owned(),
            format!(
                "{} run{}{}",
                self.request.batch,
                if self.request.batch == 1 { "" } else { "s" },
                if self.request.batch > 1 {
                    " (constant liar: each later run assumes the earlier ones match the best run)"
                } else {
                    ""
                }
            ),
        ]);
        t.push(["Seed".to_owned(), self.request.seed.to_string()]);
        ReportSection {
            kind: SectionKind::SequentialSummary,
            title: "Sequential Optimization Summary".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    /// Names of the model inputs.
    fn input_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .factors
            .iter()
            .filter(|f| matches!(f.space, FactorSpace::Continuous { .. }))
            .map(|f| f.name.clone())
            .collect();
        for f in &self.factors {
            if let FactorSpace::Categorical { levels } = &f.space {
                names.extend(levels.iter().map(|l| format!("{} = {l}", f.name)));
            }
        }
        names
    }

    fn models_section(&self) -> ReportSection {
        let names = self.input_names();
        let mut t = TextTable::new([
            "Response",
            "Covariance",
            "Length scales (fraction of range)",
            "Noise SD",
            "Leave-one-out RMSE",
            "Log likelihood",
        ]);
        for (k, m) in self.models.iter().enumerate() {
            let ls: Vec<String> = names
                .iter()
                .zip(&m.hyper.length_scales)
                .map(|(n, l)| format!("{n} {}", format_number(*l, 3)))
                .collect();
            let loo = m.leave_one_out();
            let rmse = (loo.iter().zip(&m.y).map(|((p, _), y)| (p - y).powi(2)).sum::<f64>() / loo.len() as f64).sqrt();
            t.push([
                self.response_names[k].clone(),
                m.kernel.label().to_owned(),
                ls.join(", "),
                format_number(m.noise_sd(), 4),
                format_number(rmse, 4),
                format_number(m.log_likelihood_original(), DIGITS),
            ]);
        }
        ReportSection {
            kind: SectionKind::SequentialModels,
            title: "Gaussian-Process Models of the Responses".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    fn best_section(&self) -> ReportSection {
        let mut headers = vec!["Row".to_owned()];
        headers.extend(self.factors.iter().map(|f| f.name.clone()));
        headers.extend(self.response_names.iter().cloned());
        if !self.single() {
            headers.push("Desirability".to_owned());
        }
        let mut t = TextTable::new(headers);
        let b = self.best();
        let r = self.runs[b];
        let mut row = vec![(r + 1).to_string()];
        row.extend(self.labels[b].iter().cloned());
        row.extend(self.observed[b].iter().map(|v| format_number(*v, DIGITS)));
        if !self.single() {
            row.push(format_number(self.values[b], 4));
        }
        t.push(row);
        ReportSection {
            kind: SectionKind::SequentialBestRun,
            title: "Best Run So Far".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    fn proposal_section(&self) -> ReportSection {
        let mut headers = vec!["Run".to_owned()];
        if let Some(name) = &self.round_name {
            headers.push(name.clone());
        }
        headers.extend(self.factors.iter().map(|f| f.name.clone()));
        for name in &self.response_names {
            headers.push(format!("Predicted {name}"));
            headers.push("Std error".to_owned());
        }
        headers.push(self.criterion_label());
        let mut t = TextTable::new(headers);
        for (i, p) in self.proposals.iter().enumerate() {
            let mut row = vec![format!("New {}", i + 1)];
            if self.round_name.is_some() {
                row.push(self.next_round.as_ref().map_or(String::new(), |c| match c {
                    CellValue::Number(x) => format_number(*x, 6),
                    CellValue::Text(s) => s.clone(),
                }));
            }
            for s in self.decoded(&p.point) {
                row.push(match s {
                    CellSetting::Number(x) => format_number(x, DIGITS),
                    CellSetting::Level(l) => l,
                });
            }
            for (m, s) in p.means.iter().zip(&p.sds) {
                row.push(format_number(*m, DIGITS));
                row.push(format_number(*s, 4));
            }
            row.push(format_number(p.acquisition, 4));
            t.push(row);
        }
        ReportSection {
            kind: SectionKind::SequentialProposals,
            title: format!(
                "Proposed Runs ({}; add them to the table from the report's menu)",
                if self.proposals.len() == 1 { "1 run".to_owned() } else { format!("{} runs", self.proposals.len()) }
            ),
            content: SectionContent::Table(t),
        }
    }

    fn progress_section(&self) -> ReportSection {
        let p = &self.progress;
        let points: Vec<RowPoint> =
            p.positions.iter().zip(&self.values).map(|(&(row, x), &y)| RowPoint { row, x, y }).collect();
        let mut line = Vec::new();
        for (i, ((x, _), b)) in p.steps.iter().zip(&p.best).enumerate() {
            if i > 0 {
                line.push([*x, p.best[i - 1]]);
            }
            line.push([*x, *b]);
        }
        let by = self.round_name.clone().unwrap_or_else(|| "Run".to_owned());
        let spec = PlotSpec {
            x_label: by.clone(),
            y_label: self.value_name(),
            points,
            point_name: Some("Runs".to_owned()),
            line: Some(line),
            line_name: Some("Best so far".to_owned()),
            x_ticks: self.round_name.as_ref().map(|_| p.steps.clone()),
            ..PlotSpec::default()
        };
        ReportSection {
            kind: SectionKind::SequentialProgress,
            title: format!("Best {} by {}", self.value_name(), by),
            content: SectionContent::Plot(Box::new(spec)),
        }
    }

    /// Whether the stop rule is met, and why.
    #[must_use]
    pub fn stop_rule(&self) -> (bool, String) {
        let b = self.best();
        let best = self.values[b];
        let name = self.value_name();
        if let Some(v) = self.request.stop_value {
            let met = match (self.single(), self.goals[0].goal) {
                (true, Goal::Minimize) => best <= v,
                (true, Goal::Target(t)) => (best - t).abs() <= v,
                _ => best >= v,
            };
            if met {
                return (
                    true,
                    format!(
                        "Stop: the goal is met. The best run (row {}) has {name} {}, reaching the stop value {}.",
                        self.runs[b] + 1,
                        format_number(best, DIGITS),
                        format_number(v, DIGITS)
                    ),
                );
            }
        }
        let first = self.proposals.first().map_or(0.0, |p| p.acquisition);
        match self.request.criterion {
            AcquisitionCriterion::ExpectedImprovement => {
                let scale = if self.single() {
                    let lo = self.values.iter().copied().fold(f64::INFINITY, f64::min);
                    let hi = self.values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                    (hi - lo).max(f64::MIN_POSITIVE)
                } else {
                    1.0
                };
                let threshold = self.request.stop_improvement * scale;
                let what =
                    if self.single() { "of the observed range of the response" } else { "of the desirability scale" };
                if first < threshold {
                    (
                        true,
                        format!(
                            "Stop: little is left to gain. The largest expected improvement, {}, is below {} ({} {what}).",
                            format_number(first, 4),
                            format_number(threshold, 4),
                            format_number(self.request.stop_improvement, 3)
                        ),
                    )
                } else {
                    (
                        false,
                        format!(
                            "Continue: the largest expected improvement, {}, is above the stop threshold {} ({} {what}).{}",
                            format_number(first, 4),
                            format_number(threshold, 4),
                            format_number(self.request.stop_improvement, 3),
                            self.stop_value_note()
                        ),
                    )
                }
            }
            AcquisitionCriterion::UpperConfidenceBound { .. } => (
                false,
                format!(
                    "Continue: with an upper confidence bound only a stop value ends the search.{}",
                    self.stop_value_note()
                ),
            ),
        }
    }

    fn stop_value_note(&self) -> String {
        self.request.stop_value.map_or(String::new(), |v| {
            format!(
                " The best {} so far, {}, has not reached the stop value {}.",
                self.value_name(),
                format_number(self.values[self.best()], DIGITS),
                format_number(v, DIGITS)
            )
        })
    }

    fn stop_section(&self) -> ReportSection {
        ReportSection {
            kind: SectionKind::SequentialStopRule,
            title: "Stop Rule".to_owned(),
            content: SectionContent::Note(self.stop_rule().1),
        }
    }

    /// The edit appending the proposed runs to `table`: factor settings,
    /// the next round number when a round column is used, everything else
    /// empty (to be filled in with the results).
    ///
    /// # Errors
    /// A factor column that no longer holds the analysed values.
    pub fn new_runs_edit(&self, table: &DataTable) -> Result<Edit> {
        let n = table.n_rows();
        let mut rows = Vec::new();
        for (i, p) in self.proposals.iter().enumerate() {
            let mut values = table.empty_row();
            for (f, s) in self.factors.iter().zip(self.decoded(&p.point)) {
                let col = table.column(f.column).ok_or_else(|| invalid("a factor column was removed"))?;
                let numeric = col.as_numeric().is_some();
                values[f.column] = match s {
                    CellSetting::Number(x) => CellValue::Number(x),
                    CellSetting::Level(l) if numeric => CellValue::Number(
                        l.trim().parse().map_err(|_| invalid(format!("level {l:?} of {} is not a number", f.name)))?,
                    ),
                    CellSetting::Level(l) => CellValue::Text(l),
                };
            }
            if let (Some(c), Some(v)) = (self.request.round, &self.next_round) {
                let numeric = table.column(c).is_some_and(|col| col.as_numeric().is_some());
                values[c] = match (v, numeric) {
                    (CellValue::Text(s), true) => CellValue::Number(s.parse().unwrap_or(f64::NAN)),
                    (CellValue::Number(x), false) => CellValue::Text(format_number(*x, 6)),
                    (v, _) => v.clone(),
                };
            }
            rows.push((n + i, values, RowState::default()));
        }
        Ok(Edit::Batch {
            label: format!("add {} proposed run{}", rows.len(), if rows.len() == 1 { "" } else { "s" }),
            edits: vec![Edit::InsertRows { rows }],
        })
    }
}

/// The progress chart's steps: by the round column's values (numbers in
/// order, or labels in natural order) or by run in table order; and the
/// round the next runs get.
#[allow(clippy::type_complexity)]
fn progress(
    table: &DataTable,
    round: Option<usize>,
    runs: &[usize],
    values: &[f64],
    score: &dyn Fn(f64) -> f64,
) -> Result<(Progress, Option<String>, Option<CellValue>)> {
    let better = |a: f64, b: f64| if score(a) >= score(b) { a } else { b };
    let Some(c) = round else {
        let mut best = Vec::new();
        let mut current = f64::NAN;
        for &v in values {
            current = if current.is_nan() { v } else { better(v, current) };
            best.push(current);
        }
        let steps = (1..=runs.len()).map(|k| (k as f64, k.to_string())).collect();
        let positions = runs.iter().enumerate().map(|(k, &r)| (r, (k + 1) as f64)).collect();
        return Ok((Progress { steps, best, positions }, None, None));
    };
    let col = table.column(c).ok_or_else(|| invalid(format!("no column {c}")))?;
    let labels: Vec<String> = runs.iter().map(|&r| col.cell_text(r)).collect();
    let numbers: Option<Vec<f64>> = runs
        .iter()
        .map(|&r| {
            col.as_numeric()
                .map(|v| v[r])
                .or_else(|| col.cell_text(r).trim().parse().ok())
                .filter(|x: &f64| x.is_finite())
        })
        .collect();
    let mut keys: Vec<String> = labels.clone();
    match &numbers {
        Some(n) => {
            let mut order: Vec<usize> = (0..runs.len()).collect();
            order.sort_by(|&a, &b| n[a].total_cmp(&n[b]));
            keys = order.iter().map(|&i| labels[i].clone()).collect();
        }
        None => keys.sort_by(|a, b| natural_order(a, b)),
    }
    keys.dedup();
    let mut best = Vec::new();
    let mut current = f64::NAN;
    for k in &keys {
        for (v, l) in values.iter().zip(&labels) {
            if l == k {
                current = if current.is_nan() { *v } else { better(*v, current) };
            }
        }
        best.push(current);
    }
    let steps: Vec<(f64, String)> = keys.iter().enumerate().map(|(i, k)| ((i + 1) as f64, k.clone())).collect();
    let positions = runs
        .iter()
        .zip(&labels)
        .map(|(&r, l)| (r, keys.iter().position(|k| k == l).map_or(f64::NAN, |i| (i + 1) as f64)))
        .collect();
    let next = match &numbers {
        Some(n) => CellValue::Number(n.iter().copied().fold(f64::NEG_INFINITY, f64::max).floor() + 1.0),
        None => CellValue::Text(format!("{}", keys.len() + 1)),
    };
    Ok((Progress { steps, best, positions }, Some(col.name.clone()), Some(next)))
}

// ---- Project files and scripts ------------------------------------------------

fn goal_to_json(d: &ResponseGoal) -> JsonValue {
    let (goal, target) = goal_text(d.goal);
    let num = |x: Option<f64>| x.map_or(JsonValue::Null, JsonValue::number);
    JsonValue::Object(vec![
        ("goal".to_owned(), JsonValue::String(goal.to_lowercase())),
        ("low".to_owned(), num(d.low)),
        ("target".to_owned(), target.map_or(JsonValue::Null, JsonValue::number)),
        ("high".to_owned(), num(d.high)),
        ("importance".to_owned(), JsonValue::number(d.importance)),
    ])
}

/// The request's members after `responses` and `factors`, for project
/// files.
#[must_use]
pub fn request_members_to_json(
    request: &SequentialRequest,
    col: &dyn Fn(usize) -> JsonValue,
) -> Vec<(&'static str, JsonValue)> {
    let bounds = request
        .bounds
        .iter()
        .map(|b| match b {
            None => JsonValue::Null,
            Some((lo, hi)) => JsonValue::Array(vec![JsonValue::number(*lo), JsonValue::number(*hi)]),
        })
        .collect();
    let (criterion, kappa) = match request.criterion {
        AcquisitionCriterion::ExpectedImprovement => ("expected-improvement", JsonValue::Null),
        AcquisitionCriterion::UpperConfidenceBound { kappa } => ("upper-confidence-bound", JsonValue::number(kappa)),
    };
    vec![
        ("goals", JsonValue::Array(request.goals.iter().map(goal_to_json).collect())),
        ("bounds", JsonValue::Array(bounds)),
        ("constraints", JsonValue::String(request.constraints.clone())),
        ("round", request.round.map_or(JsonValue::Null, col)),
        ("criterion", JsonValue::String(criterion.to_owned())),
        ("kappa", kappa),
        ("batch", JsonValue::Number(request.batch as f64)),
        ("kernel", JsonValue::String(request.kernel.key().to_owned())),
        ("nugget", request.nugget.map_or(JsonValue::Null, JsonValue::number)),
        ("starts", JsonValue::Number(request.starts as f64)),
        ("stop_improvement", JsonValue::number(request.stop_improvement)),
        ("stop_value", request.stop_value.map_or(JsonValue::Null, JsonValue::number)),
        ("candidates", JsonValue::Number(request.candidates as f64)),
        ("seed", JsonValue::Number(request.seed as f64)),
    ]
}

/// Read a request from a project file's report entry (missing members take
/// their defaults).
///
/// # Errors
/// An unknown goal, criterion or kernel, a missing column, bounds that are
/// not pairs of numbers, or a negative nugget.
pub fn request_from_json(
    v: &JsonValue,
    responses: Vec<usize>,
    factors: Vec<usize>,
    find: &dyn Fn(&JsonValue) -> Result<usize>,
) -> Result<SequentialRequest> {
    let bad = |what: &str| invalid(format!("sequential optimization: invalid {what}"));
    let mut r = SequentialRequest::new(responses, factors);
    let num = |g: &JsonValue, key: &str| g.get(key).and_then(JsonValue::as_f64);
    if let Some(list) = v.get("goals").and_then(JsonValue::as_array) {
        r.goals = list
            .iter()
            .map(|g| {
                let goal = match g.get("goal").and_then(JsonValue::as_str) {
                    Some("maximize") | None => Goal::Maximize,
                    Some("minimize") => Goal::Minimize,
                    Some("target") => Goal::Target(num(g, "target").ok_or_else(|| bad("target"))?),
                    _ => return Err(bad("goal (use maximize, minimize or target)")),
                };
                Ok(ResponseGoal {
                    goal,
                    low: num(g, "low"),
                    high: num(g, "high"),
                    importance: num(g, "importance").unwrap_or(1.0),
                })
            })
            .collect::<Result<_>>()?;
    }
    if let Some(list) = v.get("bounds").and_then(JsonValue::as_array) {
        r.bounds = list
            .iter()
            .map(|b| match b {
                JsonValue::Null => Ok(None),
                JsonValue::Array(pair) => match pair[..] {
                    [ref lo, ref hi] => {
                        Ok(Some((lo.as_f64().ok_or_else(|| bad("bounds"))?, hi.as_f64().ok_or_else(|| bad("bounds"))?)))
                    }
                    _ => Err(bad("bounds")),
                },
                _ => Err(bad("bounds")),
            })
            .collect::<Result<_>>()?;
    }
    if let Some(c) = v.get("constraints").and_then(JsonValue::as_str) {
        r.constraints = c.to_owned();
    }
    r.round = match v.get("round") {
        None | Some(JsonValue::Null) => None,
        Some(c) => Some(find(c)?),
    };
    r.criterion = match v.get("criterion").and_then(JsonValue::as_str) {
        None | Some("expected-improvement") => AcquisitionCriterion::ExpectedImprovement,
        Some("upper-confidence-bound") => {
            AcquisitionCriterion::UpperConfidenceBound { kappa: num(v, "kappa").unwrap_or(2.0) }
        }
        Some(_) => return Err(bad("criterion (use expected-improvement or upper-confidence-bound)")),
    };
    if let Some(b) = v.get("batch").and_then(JsonValue::as_usize) {
        r.batch = b.clamp(1, 50);
    }
    r.kernel = kernel_from_json(v)?;
    r.nugget = nugget_from_json(v)?;
    if let Some(s) = v.get("starts").and_then(JsonValue::as_usize) {
        r.starts = s.clamp(1, 100);
    }
    if let Some(s) = num(v, "stop_improvement") {
        r.stop_improvement = s.max(0.0);
    }
    r.stop_value = num(v, "stop_value");
    if let Some(c) = v.get("candidates").and_then(JsonValue::as_usize) {
        r.candidates = c.clamp(100, 100_000);
    }
    r.seed = v.get("seed").and_then(crate::seed::seed_from_json).unwrap_or(r.seed);
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    /// Twelve runs of y = 80 − (t − 60)²/40 − (c − 2)² + 3·[s = B], with an
    /// impurity rising with t, a round column, and one run waiting.
    fn table() -> DataTable {
        let mut rng = stat_core::rng::Rng::new(8);
        let (mut t, mut c, mut s, mut y, mut imp, mut round) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for i in 0..13 {
            let (tt, cc) = (30.0 + 60.0 * rng.next_f64(), 4.0 * rng.next_f64());
            let b = i % 2 == 1;
            t.push(tt);
            c.push(cc);
            s.push(if b { "B" } else { "A" }.to_owned());
            if i == 12 {
                y.push(f64::NAN);
                imp.push(f64::NAN);
            } else {
                y.push(80.0 - (tt - 60.0).powi(2) / 40.0 - (cc - 2.0).powi(2) + if b { 3.0 } else { 0.0 });
                imp.push(0.05 * tt + 0.1 * rng.normal());
            }
            round.push(if i < 8 { 1.0 } else { 2.0 });
        }
        DataTable::new(
            "reaction",
            vec![
                Column::numeric("Temp", t),
                Column::numeric("Cat", c),
                Column::text("Solvent", s),
                Column::numeric("Yield", y),
                Column::numeric("Impurity", imp),
                Column::numeric("Round", round),
            ],
        )
        .unwrap()
    }

    #[test]
    fn proposes_runs_toward_the_optimum() {
        let t = table();
        let rows = RowStateManager::new(t.n_rows());
        let req = SequentialRequest { round: Some(5), ..SequentialRequest::new(vec![3], vec![0, 1, 2]) };
        let a = SequentialAnalysis::run(&t, &rows, &req).unwrap();
        assert_eq!(a.title(), "Sequential Optimization: Yield");
        let kinds: Vec<SectionKind> = a.sections().iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::SequentialSummary,
                SectionKind::SequentialModels,
                SectionKind::SequentialBestRun,
                SectionKind::SequentialProposals,
                SectionKind::SequentialProgress,
                SectionKind::SequentialStopRule,
            ]
        );
        assert_eq!(a.runs.len(), 12);
        assert_eq!(a.pending, vec![12]);
        assert_eq!(a.proposals().len(), 4);
        // Solvent B is better everywhere; the first proposal uses it.
        let first = a.decoded(&a.proposals()[0].point);
        assert_eq!(first[2], CellSetting::Level("B".to_owned()));
        // Progress: rounds 1 and 2, best never decreasing.
        assert_eq!(a.progress.steps.iter().map(|s| s.1.as_str()).collect::<Vec<_>>(), ["1", "2"]);
        assert!(a.progress.best[1] >= a.progress.best[0]);
        assert_eq!(a.next_round, Some(CellValue::Number(3.0)));
        // Appending the runs: factor settings and round 3, responses empty.
        let edit = a.new_runs_edit(&t).unwrap();
        let mut t2 = t.clone();
        let mut rows2 = rows.clone();
        edit.apply(&mut t2, &mut rows2).unwrap();
        assert_eq!(t2.n_rows(), 17);
        assert_eq!(t2.column(5).unwrap().as_numeric().unwrap()[16], 3.0);
        assert!(t2.column(3).unwrap().as_numeric().unwrap()[16].is_nan());
        // Re-running treats the new rows as waiting and proposes elsewhere.
        let b = SequentialAnalysis::run(&t2, &rows2, &req).unwrap();
        assert_eq!(b.pending.len(), 5);
        let far = |p: &Candidate, q: &Candidate| {
            p.levels != q.levels
                || p.continuous.iter().zip(&q.continuous).map(|(u, v)| (u - v).powi(2)).sum::<f64>().sqrt() > 0.01
        };
        for p in b.proposals() {
            assert!(a.proposals().iter().all(|q| far(&p.point, &q.point)), "{p:?} repeats a waiting run");
        }
        // Same seed, same report.
        let c = SequentialAnalysis::run(&t, &rows, &req).unwrap();
        assert_eq!(a.sections(), c.sections());
    }

    #[test]
    fn desirability_constraints_stop_rule_and_json() {
        let t = table();
        let rows = RowStateManager::new(t.n_rows());
        let mut req = SequentialRequest::new(vec![3, 4], vec![0, 1]);
        req.goals[1] = ResponseGoal::new(Goal::Minimize);
        req.constraints = "Temp + 10*Cat <= 90".to_owned();
        req.batch = 2;
        let a = SequentialAnalysis::run(&t, &rows, &req).unwrap();
        for p in a.proposals() {
            let s = a.decoded(&p.point);
            let (CellSetting::Number(tt), CellSetting::Number(cc)) = (&s[0], &s[1]) else { panic!() };
            assert!(tt + 10.0 * cc <= 90.0 + 0.1, "{tt} {cc}");
            assert_eq!(p.means.len(), 2);
        }
        assert!(a.values.iter().all(|d| (0.0..=1.0).contains(d)));
        // The summary counts the constraints but does not repeat them.
        let SectionContent::Table(summary) = &a.sections()[0].content else { panic!("summary is a table") };
        let cells: Vec<&str> = summary.rows.iter().flatten().map(String::as_str).collect();
        assert!(cells.contains(&"1 (as entered; every proposed run satisfies it)"), "{cells:?}");
        assert!(!cells.iter().any(|c| c.contains("Temp + 10*Cat")), "{cells:?}");
        // A stop value already reached.
        let met = SequentialRequest { stop_value: Some(0.0), ..req.clone() };
        assert!(SequentialAnalysis::run(&t, &rows, &met).unwrap().stop_rule().0);
        // Upper confidence bound, target goal, JSON round trip.
        let mut ucb = SequentialRequest::new(vec![3], vec![0, 1, 2]);
        ucb.goals[0] = ResponseGoal::new(Goal::Target(75.0));
        ucb.criterion = AcquisitionCriterion::UpperConfidenceBound { kappa: 1.5 };
        ucb.bounds = vec![Some((30.0, 90.0)), None, None];
        ucb.round = Some(5);
        ucb.stop_value = Some(0.5);
        ucb.nugget = Some(1e-6);
        let b = SequentialAnalysis::run(&t, &rows, &ucb).unwrap();
        assert!(!b.stop_rule().1.is_empty());
        let col = |i: usize| JsonValue::String(t.column(i).unwrap().name.clone());
        let find = |c: &JsonValue| Ok(t.find(c.as_str().unwrap()).unwrap());
        for r in [&req, &ucb] {
            let v = JsonValue::Object(
                request_members_to_json(r, &col).into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
            );
            let back = request_from_json(&v, r.responses.clone(), r.factors.clone(), &find).unwrap();
            assert_eq!(&back, r);
        }
        // Errors: too few runs, bad constraint, a column used twice.
        let mut few = RowStateManager::new(t.n_rows());
        few.set_excluded(2..13, true);
        assert!(SequentialAnalysis::run(&t, &few, &SequentialRequest::new(vec![3], vec![0])).is_err());
        let bad =
            SequentialRequest { constraints: "Pressure < 3".to_owned(), ..SequentialRequest::new(vec![3], vec![0]) };
        assert!(SequentialAnalysis::run(&t, &rows, &bad).is_err());
        assert!(SequentialAnalysis::run(&t, &rows, &SequentialRequest::new(vec![3], vec![3])).is_err());
    }
}
