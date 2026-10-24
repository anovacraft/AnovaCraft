//! Automated model search: every applicable predictive model fitted to one
//! response with its default settings and the same validation roles, ranked
//! by its error on rows it was not fitted to.
//!
//! **Models.** Decision tree, random forest, boosted trees, neural network,
//! support vector machine, multivariate adaptive regression splines
//! (continuous response only), partial least squares, and discriminant
//! analysis (categorical response and numeric predictors only). Each runs
//! exactly as its own report would with default settings, the search's
//! validation roles and the search's seed, so its row in the ranking matches
//! the report it opens. A model that cannot be fitted to the data (for
//! example discriminant analysis with a text predictor) is listed with the
//! reason and left out of the ranking.
//!
//! **Ranking.** Models are ranked by the held-out error that tunes them: on
//! the validation rows, or, with k-fold roles, from the predictions of each
//! row by the model fitted without its fold. With every row training the
//! search uses 5-fold cross-validation (seeded), since a ranking on the
//! training rows would favour the most flexible model. The measure is the
//! root mean square error for a continuous response, and the log loss
//! (mean −ln p̂ of the observed class) for a categorical one, a proper
//! scoring rule that rewards well-calibrated probabilities and separates
//! models that the coarser misclassification rate ties (ties broken by the
//! misclassification rate, then by the menu order of the models). Test rows, used by no model
//! for fitting or tuning, give an honest estimate for the winner.
//!
//! **Repeated and nested cross-validation.** With `repeats` above one and
//! k-fold roles (or every row training), every model is cross-validated again
//! with each repeat's folds (seeds `seed`, `seed + 1`, …), and the report
//! lists each model's held-out RMSE (or misclassification rate) across the
//! repeats as a mean with a t interval, drawn as a forest plot (Kim 2009).
//! A single split's ranking can turn on how the rows happened to fall; the
//! spread across repeats shows how much. With `nested`, each repeat instead
//! holds out one outer fold at a time, lets the model tune itself by k-fold
//! cross-validation on the remaining rows only, and measures it on the
//! held-out fold: since tuning never sees the rows it is scored on, the
//! error is not biased by the tuning (Varma & Simon 2006).
//!
//! **Reproducibility.** Every model is seeded from the request's seed and
//! the models are fitted in parallel with results combined in a fixed
//! order, so a search repeats exactly.

use rayon::prelude::*;
use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_predictive::Role;
use stat_predictive::measures::{ClassFit, ContinuousFit};

use crate::format::format_number;
use crate::prediction::{Predictions, Predictor, Response, ValidationMethod};
use crate::project::SavedReport;
use crate::report::{Annotations, PlotSpec, ReportSection, SectionContent, SectionKind, TextTable};
use crate::{
    BoostAnalysis, BoostRequest, DiscriminantAnalysis, DiscriminantRequest, ForestAnalysis, ForestRequest,
    MarsAnalysis, MarsRequest, NeuralAnalysis, NeuralRequest, PlsAnalysis, PlsRequest, ProfilerOptions, SvmAnalysis,
    SvmRequest, TreeAnalysis, TreeRequest,
};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Folds used when every row trains.
pub const DEFAULT_FOLDS: usize = 5;

/// A kind of predictive model the search can fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ModelKind {
    /// Classification or regression tree.
    DecisionTree,
    /// Random forest.
    RandomForest,
    /// Gradient-boosted trees.
    BoostedTrees,
    /// Neural network.
    NeuralNetwork,
    /// Support vector machine.
    SupportVectorMachine,
    /// Multivariate adaptive regression splines.
    Mars,
    /// Partial least squares.
    PartialLeastSquares,
    /// Linear or quadratic discriminant analysis.
    Discriminant,
}

impl ModelKind {
    /// Every kind, in menu order.
    pub const ALL: [Self; 8] = [
        Self::DecisionTree,
        Self::RandomForest,
        Self::BoostedTrees,
        Self::NeuralNetwork,
        Self::SupportVectorMachine,
        Self::Mars,
        Self::PartialLeastSquares,
        Self::Discriminant,
    ];

    /// Identifier in project files and scripts.
    #[must_use]
    pub fn key(&self) -> &'static str {
        match self {
            Self::DecisionTree => "decision-tree",
            Self::RandomForest => "random-forest",
            Self::BoostedTrees => "boosted-trees",
            Self::NeuralNetwork => "neural-network",
            Self::SupportVectorMachine => "support-vector-machine",
            Self::Mars => "adaptive-splines",
            Self::PartialLeastSquares => "partial-least-squares",
            Self::Discriminant => "discriminant",
        }
    }

    /// The kind named `key`.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        // Older scripts and project files call it "mars".
        let key = if key == "mars" { "adaptive-splines" } else { key };
        Self::ALL.into_iter().find(|k| k.key() == key)
    }

    /// Name for reports and menus.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::DecisionTree => "Decision tree",
            Self::RandomForest => "Random forest",
            Self::BoostedTrees => "Boosted trees",
            Self::NeuralNetwork => "Neural network",
            Self::SupportVectorMachine => "Support vector machine",
            Self::Mars => "Adaptive regression splines",
            Self::PartialLeastSquares => "Partial least squares",
            Self::Discriminant => "Discriminant analysis",
        }
    }

    /// Whether the kind models a response of this type (`categorical`).
    #[must_use]
    pub fn applies_to(&self, categorical: bool) -> bool {
        match self {
            Self::Mars => !categorical,
            Self::Discriminant => categorical,
            _ => true,
        }
    }
}

/// Columns and settings of a model search.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelSearchRequest {
    /// Response column.
    pub response: usize,
    /// Predictor columns.
    pub predictors: Vec<usize>,
    /// Training / validation / test roles (none: 5-fold cross-validation).
    pub validation: ValidationMethod,
    /// Kinds of model tried (those that do not apply to the response are
    /// skipped).
    pub models: Vec<ModelKind>,
    /// Seed of every model (and of the folds when every row trains).
    pub seed: u64,
    /// Repetitions of k-fold cross-validation with fresh folds (1: off).
    pub repeats: usize,
    /// Nested cross-validation: tune within each outer fold's training rows.
    pub nested: bool,
}

impl ModelSearchRequest {
    /// Every kind of model, 5-fold cross-validation, seed 1.
    #[must_use]
    pub fn new(response: usize, predictors: Vec<usize>) -> Self {
        Self {
            response,
            predictors,
            validation: ValidationMethod::None,
            models: ModelKind::ALL.to_vec(),
            seed: 1,
            repeats: 1,
            nested: false,
        }
    }

    /// Columns read.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut c = vec![self.response];
        c.extend(&self.predictors);
        c.extend(self.validation.columns());
        c
    }

    /// The same request with column indices passed through `f`.
    #[must_use]
    pub fn map_columns(mut self, f: impl Fn(usize) -> usize) -> Self {
        self.response = f(self.response);
        self.predictors.iter_mut().for_each(|c| *c = f(*c));
        self.validation = self.validation.map_columns(&f);
        self
    }

    /// The validation the models use: the request's, or seeded 5-fold
    /// cross-validation when every row trains.
    #[must_use]
    pub fn effective_validation(&self) -> ValidationMethod {
        match self.validation {
            ValidationMethod::None => ValidationMethod::KFold { folds: DEFAULT_FOLDS, seed: self.seed },
            ref v => v.clone(),
        }
    }

    /// The report of `kind` with the search's columns, roles and seed and
    /// otherwise default settings.
    #[must_use]
    pub fn model_report(&self, kind: ModelKind) -> SavedReport {
        let (response, predictors) = (self.response, self.predictors.clone());
        let validation = self.effective_validation();
        let seed = self.seed;
        let profilers = ProfilerOptions::default();
        match kind {
            ModelKind::DecisionTree => SavedReport::DecisionTree {
                request: TreeRequest { validation, ..TreeRequest::new(response, predictors) },
                profilers,
            },
            ModelKind::RandomForest => SavedReport::RandomForest {
                request: ForestRequest { validation, seed, ..ForestRequest::new(response, predictors) },
                profilers,
            },
            ModelKind::BoostedTrees => SavedReport::BoostedTrees {
                request: BoostRequest { validation, seed, ..BoostRequest::new(response, predictors) },
                profilers,
            },
            ModelKind::NeuralNetwork => SavedReport::NeuralNetwork {
                request: NeuralRequest { validation, seed, ..NeuralRequest::new(response, predictors) },
                profilers,
            },
            ModelKind::SupportVectorMachine => SavedReport::SupportVectorMachine {
                request: SvmRequest { validation, seed, ..SvmRequest::new(response, predictors) },
                profilers,
            },
            ModelKind::Mars => SavedReport::Mars {
                request: MarsRequest { validation, ..MarsRequest::new(response, predictors) },
                profilers,
            },
            ModelKind::PartialLeastSquares => SavedReport::PartialLeastSquares {
                request: PlsRequest { validation, ..PlsRequest::new(response, predictors) },
                profilers,
            },
            ModelKind::Discriminant => SavedReport::DiscriminantAnalysis {
                request: DiscriminantRequest { validation, ..DiscriminantRequest::new(response, predictors) },
            },
        }
    }
}

/// Held-out and test measures of one model.
#[derive(Debug, Clone, PartialEq)]
struct Scores {
    /// Rows measured and the measures (continuous: RMSE, MAE, R²;
    /// categorical: misclassification, log loss, AUC), held out.
    held_out: Option<(usize, [f64; 3])>,
    test: Option<(usize, [f64; 3])>,
    training: Option<(usize, [f64; 3])>,
}

/// One model's outcome.
#[derive(Debug, Clone, PartialEq)]
enum Outcome {
    Fitted { title: String, scores: Scores },
    Failed(String),
}

/// Each model's held-out error in every repeat of k-fold cross-validation.
pub type RepeatedErrors = Vec<(ModelKind, Vec<f64>)>;

/// A finished model search.
#[derive(Debug, Clone)]
pub struct ModelSearch {
    request: ModelSearchRequest,
    response_name: String,
    classes: Option<Vec<String>>,
    /// Rows per role, and the roles' description (from the first fitted model).
    roles: Vec<(Role, usize)>,
    roles_description: String,
    held_out_label: String,
    /// Each kind tried, in menu order, with its outcome.
    outcomes: Vec<(ModelKind, Outcome)>,
    /// Kinds that do not apply to the response.
    skipped: Vec<ModelKind>,
    /// Indices into `outcomes` of fitted models, best first.
    ranking: Vec<usize>,
    /// Repeated (or nested) cross-validation: per kind, the error of each
    /// repeat; or why it was not run.
    repeated: Option<std::result::Result<RepeatedErrors, String>>,
}

fn measures(response: &Response, predictions: &Predictions, rows: &[usize]) -> Option<(usize, [f64; 3])> {
    match (response, predictions) {
        (Response::Continuous(y), Predictions::Values(p)) => {
            let y: Vec<f64> = rows.iter().map(|&r| y[r]).collect();
            let p: Vec<f64> = rows.iter().map(|&r| p[r]).collect();
            let m = ContinuousFit::new(&y, &p)?;
            Some((m.n, [m.rmse, m.mae, m.r_squared]))
        }
        (Response::Categorical { classes, codes }, Predictions::Probabilities(p)) => {
            let (obs, probs): (Vec<usize>, Vec<Vec<f64>>) = rows
                .iter()
                .filter_map(|&r| codes[r].filter(|_| p[r].iter().all(|v| v.is_finite())).map(|c| (c, p[r].clone())))
                .unzip();
            let m = ClassFit::new(&obs, &probs, classes.len())?;
            Some((m.n, [m.misclassification, m.log_loss, m.auc]))
        }
        _ => None,
    }
}

/// The held-out, test and training measures of a fitted model.
fn score(model: &dyn Predictor) -> Scores {
    let response = model.response();
    let roles = model.roles();
    let valid = roles.rows(Role::Validation);
    let held_out = if !valid.is_empty() {
        measures(response, model.predictions(), &valid)
    } else {
        model.cross_validated().and_then(|cv| measures(response, cv, &roles.rows(Role::Training)))
    };
    Scores {
        held_out,
        test: measures(response, model.predictions(), &roles.rows(Role::Test)),
        training: measures(response, model.predictions(), &roles.rows(Role::Training)),
    }
}

/// What the search needs from each fitted model.
struct Fit {
    title: String,
    scores: Scores,
    roles: crate::prediction::RoleAssignment,
}

fn fit_one(report: &SavedReport, table: &DataTable, states: &RowStateManager) -> Result<Fit> {
    with_model(report, table, states, &|p, title| Fit { title, scores: score(p), roles: p.roles().clone() })
}

/// Run the predictive model `report` and pass it, with its title, to `f`.
fn with_model<T>(
    report: &SavedReport,
    table: &DataTable,
    states: &RowStateManager,
    f: &dyn Fn(&dyn Predictor, String) -> T,
) -> Result<T> {
    Ok(match report {
        SavedReport::DecisionTree { request, .. } => {
            let a = TreeAnalysis::run(table, states, request)?;
            f(&a, a.title())
        }
        SavedReport::RandomForest { request, .. } => {
            let a = ForestAnalysis::run(table, states, request)?;
            f(&a, a.title())
        }
        SavedReport::BoostedTrees { request, .. } => {
            let a = BoostAnalysis::run(table, states, request)?;
            f(&a, a.title())
        }
        SavedReport::NeuralNetwork { request, .. } => {
            let a = NeuralAnalysis::run(table, states, request)?;
            f(&a, a.title())
        }
        SavedReport::SupportVectorMachine { request, .. } => {
            let a = SvmAnalysis::run(table, states, request)?;
            f(&a, a.title())
        }
        SavedReport::Mars { request, .. } => {
            let a = MarsAnalysis::run(table, states, request)?;
            f(&a, a.title())
        }
        SavedReport::PartialLeastSquares { request, .. } => {
            let a = PlsAnalysis::run(table, states, request)?;
            f(&a, a.title())
        }
        SavedReport::DiscriminantAnalysis { request } => {
            let a = DiscriminantAnalysis::run(table, states, request)?;
            f(&a, a.title())
        }
        _ => return Err(invalid("not a predictive model")),
    })
}

/// Most repeats of cross-validation.
pub const MAX_REPEATS: usize = 50;

/// The error measured in repeated cross-validation: RMSE (continuous) or
/// the misclassification rate (categorical), on `rows`, of `predictions`
/// whose classes are `predicted_classes` (mapped by name onto the
/// response's classes).
fn repeat_error(
    response: &Response,
    predictions: &Predictions,
    predicted_classes: Option<&[String]>,
    rows: &[usize],
) -> Option<f64> {
    let remapped;
    let predictions = match (response.classes(), predicted_classes, predictions) {
        (Some(classes), Some(own), Predictions::Probabilities(p)) if own != classes => {
            let map: Vec<Option<usize>> = classes.iter().map(|c| own.iter().position(|o| o == c)).collect();
            remapped = Predictions::Probabilities(
                p.iter().map(|row| map.iter().map(|m| m.map_or(0.0, |k| row[k])).collect()).collect(),
            );
            &remapped
        }
        _ => predictions,
    };
    measures(response, predictions, rows).map(|(_, m)| m[0])
}

impl ModelSearch {
    /// Each kind's error in every repeat of (nested) k-fold cross-validation.
    fn repeated(
        table: &DataTable,
        states: &RowStateManager,
        request: &ModelSearchRequest,
        response: &Response,
        kinds: &[ModelKind],
    ) -> std::result::Result<RepeatedErrors, String> {
        let folds = match request.validation {
            ValidationMethod::None => DEFAULT_FOLDS,
            ValidationMethod::KFold { folds, .. } => folds,
            _ => {
                return Err("repeated cross-validation needs k-fold roles (or every row training); this search uses \
                            validation rows"
                    .to_owned());
            }
        };
        let repeats = request.repeats.clamp(1, MAX_REPEATS);
        let used: Vec<usize> = (0..table.n_rows()).filter(|&r| !states.is_excluded(r) && response.present(r)).collect();
        let jobs: Vec<(ModelKind, usize)> = kinds.iter().flat_map(|&k| (0..repeats).map(move |i| (k, i))).collect();
        let errors: Vec<Option<f64>> = jobs
            .par_iter()
            .map(|&(kind, i)| {
                let seed = request.seed.wrapping_add(i as u64);
                let inner =
                    ModelSearchRequest { validation: ValidationMethod::KFold { folds, seed }, seed, ..request.clone() };
                let report = inner.model_report(kind);
                if !request.nested {
                    return with_model(&report, table, states, &|p, _| {
                        p.cross_validated().and_then(|cv| repeat_error(response, cv, p.response().classes(), &used))
                    })
                    .ok()
                    .flatten();
                }
                // Nested: hold out each outer fold in turn.
                let outer = stat_predictive::validation::fold_assignment(used.len(), folds, seed ^ 0x9e37_79b9).ok()?;
                let mut pooled: Predictions = match response.classes() {
                    Some(c) => Predictions::Probabilities(vec![vec![f64::NAN; c.len()]; table.n_rows()]),
                    None => Predictions::Values(vec![f64::NAN; table.n_rows()]),
                };
                for f in 0..folds {
                    let held: Vec<usize> = used.iter().zip(&outer).filter(|(_, k)| **k == f).map(|(r, _)| *r).collect();
                    let mut inner_states = states.clone();
                    inner_states.set_excluded(held.iter().copied(), true);
                    let (pred, own) = with_model(&report, table, &inner_states, &|p, _| {
                        (p.predictions().clone(), p.response().classes().map(<[String]>::to_vec))
                    })
                    .ok()?;
                    match (&mut pooled, pred) {
                        (Predictions::Values(all), Predictions::Values(v)) => {
                            for &r in &held {
                                all[r] = v[r];
                            }
                        }
                        (Predictions::Probabilities(all), Predictions::Probabilities(v)) => {
                            let (classes, own) = (response.classes()?, own?);
                            for &r in &held {
                                all[r] = classes
                                    .iter()
                                    .map(|c| own.iter().position(|o| o == c).map_or(0.0, |k| v[r][k]))
                                    .collect();
                            }
                        }
                        _ => return None,
                    }
                }
                repeat_error(response, &pooled, None, &used)
            })
            .collect();
        Ok(kinds
            .iter()
            .enumerate()
            .map(|(j, &k)| (k, errors[j * repeats..(j + 1) * repeats].iter().filter_map(|e| *e).collect()))
            .collect())
    }
}

impl ModelSearch {
    /// Fit and rank every applicable model.
    ///
    /// # Errors
    /// No predictors, the response among them, no model kinds, an unreadable
    /// response or validation method, no held-out rows to rank by (roles
    /// with test rows but no validation rows), or no model that could be
    /// fitted.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &ModelSearchRequest) -> Result<Self> {
        if request.predictors.is_empty() {
            return Err(invalid("choose at least one predictor"));
        }
        if request.predictors.contains(&request.response) {
            return Err(invalid("the response cannot also be a predictor"));
        }
        let included: Vec<usize> = (0..table.n_rows()).filter(|&r| !states.is_excluded(r)).collect();
        let response = Response::read(table, request.response, &included)?;
        let response_name = table.column(request.response).map_or(String::new(), |c| c.name.clone());
        let categorical = response.classes().is_some();
        let validation = request.effective_validation();
        let roles = crate::prediction::RoleAssignment::assign(table, states, &response, &validation)?;
        if roles.rows(Role::Validation).is_empty() && roles.folds.is_none() {
            return Err(invalid(
                "the search ranks models on held-out rows: give some rows the validation role, or use k-fold \
                 cross-validation",
            ));
        }
        let mut kinds: Vec<ModelKind> = request.models.clone();
        kinds.sort();
        kinds.dedup();
        if kinds.is_empty() {
            return Err(invalid("choose at least one kind of model"));
        }
        let (tried, skipped): (Vec<ModelKind>, Vec<ModelKind>) =
            kinds.into_iter().partition(|k| k.applies_to(categorical));
        // Models fit on rayon's threads, each with the caller's interrupt
        // (see `stat_core::interrupt`): Cancel stops the search between and
        // inside models.
        let interrupt = stat_core::interrupt::current();
        if let Some(i) = &interrupt {
            i.begin(tried.len());
        }
        let fits: Vec<Result<Fit>> = tried
            .par_iter()
            .map(|&k| match &interrupt {
                None => fit_one(&request.model_report(k), table, states),
                Some(i) => {
                    i.check()?;
                    let fit = stat_core::interrupt::with(std::sync::Arc::clone(i), || {
                        fit_one(&request.model_report(k), table, states)
                    });
                    i.step();
                    fit
                }
            })
            .collect();
        if let Some(i) = &interrupt {
            i.check()?;
        }
        let mut outcomes = Vec::new();
        let mut first: Option<(Vec<(Role, usize)>, String)> = None;
        for (&kind, fit) in tried.iter().zip(fits) {
            match fit {
                Ok(f) => {
                    if first.is_none() {
                        let counts = Role::ALL.iter().map(|&r| (r, f.roles.rows(r).len())).collect();
                        first = Some((counts, f.roles.description.clone()));
                    }
                    outcomes.push((kind, Outcome::Fitted { title: f.title, scores: f.scores }));
                }
                Err(e) => outcomes.push((kind, Outcome::Failed(e.to_string()))),
            }
        }
        let Some((role_counts, roles_description)) = first else {
            let reasons: Vec<String> = outcomes
                .iter()
                .map(|(k, o)| match o {
                    Outcome::Failed(e) => format!("{}: {e}", k.label()),
                    Outcome::Fitted { .. } => String::new(),
                })
                .collect();
            return Err(invalid(format!("no model could be fitted ({})", reasons.join("; "))));
        };
        // Rank: smallest held-out RMSE, or log loss with ties by misclassification; then menu order.
        let key = |o: &Outcome| -> Option<(f64, f64)> {
            match o {
                Outcome::Fitted { scores: Scores { held_out: Some((_, m)), .. }, .. }
                    if m[0].is_finite() && m[1].is_finite() =>
                {
                    Some(if categorical { (m[1], m[0]) } else { (m[0], 0.0) })
                }
                _ => None,
            }
        };
        let mut ranking: Vec<usize> = (0..outcomes.len()).filter(|&i| key(&outcomes[i].1).is_some()).collect();
        ranking.sort_by(|&a, &b| {
            let (ka, kb) = (key(&outcomes[a].1).unwrap_or_default(), key(&outcomes[b].1).unwrap_or_default());
            ka.0.total_cmp(&kb.0).then(ka.1.total_cmp(&kb.1)).then(outcomes[a].0.cmp(&outcomes[b].0))
        });
        let held_out_label = if roles.rows(Role::Validation).is_empty() {
            format!("{}-fold cross-validation", roles.folds.map_or(DEFAULT_FOLDS, |f| f.0))
        } else {
            "validation rows".to_owned()
        };
        let repeated = (request.repeats > 1 || request.nested).then(|| {
            let ranked: Vec<ModelKind> = ranking.iter().map(|&i| outcomes[i].0).collect();
            Self::repeated(table, states, request, &response, &ranked)
        });
        Ok(Self {
            request: request.clone(),
            response_name,
            classes: response.classes().map(<[String]>::to_vec),
            roles: role_counts,
            roles_description,
            held_out_label,
            outcomes,
            skipped,
            ranking,
            repeated,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &ModelSearchRequest {
        &self.request
    }

    /// The best model's kind (`None` when no model could be ranked).
    #[must_use]
    pub fn best(&self) -> Option<ModelKind> {
        self.ranking.first().map(|&i| self.outcomes[i].0)
    }

    /// Kinds fitted, best first.
    #[must_use]
    pub fn ranked(&self) -> Vec<ModelKind> {
        self.ranking.iter().map(|&i| self.outcomes[i].0).collect()
    }

    /// Kinds that were fitted (in menu order).
    #[must_use]
    pub fn fitted(&self) -> Vec<ModelKind> {
        self.outcomes.iter().filter(|(_, o)| matches!(o, Outcome::Fitted { .. })).map(|(k, _)| *k).collect()
    }

    fn categorical(&self) -> bool {
        self.classes.is_some()
    }

    /// Which of the three measures ranks the models: RMSE, or the log loss.
    fn rank_index(&self) -> usize {
        usize::from(self.categorical())
    }

    fn measure_names(&self) -> [&'static str; 3] {
        if self.categorical() { ["Misclassification", "Log loss", "AUC"] } else { ["RMSE", "Mean abs. error", "R²"] }
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Model Search: {} ({} predictors)", self.response_name, self.request.predictors.len())
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary_section(), self.ranking_section()];
        out.extend(self.plot_section());
        out.extend(self.repeated_sections());
        out
    }

    fn summary_section(&self) -> ReportSection {
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut row = |a: &str, b: String| t.push([a.to_owned(), b]);
        row("Response", self.response_name.clone());
        row(
            "Task",
            match &self.classes {
                Some(c) => format!("classification ({} classes)", c.len()),
                None => "regression (continuous response)".to_owned(),
            },
        );
        row("Roles", self.roles_description.clone());
        for &(role, n) in &self.roles {
            if n > 0 || role == Role::Training {
                row(&format!("{} rows", role.label()), n.to_string());
            }
        }
        let k = self.rank_index();
        let names = self.measure_names();
        let main = names[k];
        row(
            "Ranked by",
            if self.categorical() {
                format!("{} on the {} (ties by {})", main.to_lowercase(), self.held_out_label, names[0].to_lowercase())
            } else {
                format!("{main} on the {}", self.held_out_label)
            },
        );
        if let Some(&i) = self.ranking.first() {
            if let (kind, Outcome::Fitted { scores, .. }) = &self.outcomes[i] {
                let held = scores.held_out.map_or(String::new(), |(_, m)| format_number(m[k], 4));
                let test = scores
                    .test
                    .map_or(String::new(), |(_, m)| format!("; on the test rows {}", format_number(m[k], 4)));
                let measure = if self.categorical() { main.to_lowercase() } else { main.to_owned() };
                row("Best model", format!("{} ({measure} {held}{test})", kind.label()));
            }
        }
        row("Models fitted", format!("{} of {}", self.fitted().len(), self.outcomes.len()));
        for (kind, o) in &self.outcomes {
            if let Outcome::Failed(e) = o {
                row(&format!("Not fitted: {}", kind.label()), e.clone());
            }
        }
        if !self.skipped.is_empty() {
            let names: Vec<&str> = self.skipped.iter().map(ModelKind::label).collect();
            row(
                "Not applicable",
                format!(
                    "{} ({} response)",
                    names.join(", "),
                    if self.categorical() { "a categorical" } else { "a continuous" }
                ),
            );
        }
        row("Settings", "each model's defaults, tuned on the same roles".to_owned());
        row("Seed", self.request.seed.to_string());
        ReportSection {
            kind: SectionKind::ModelSearchSummary,
            title: "Model Search Summary".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    fn ranking_section(&self) -> ReportSection {
        let names = self.measure_names();
        let has_test = self.roles.iter().any(|&(r, n)| r == Role::Test && n > 0);
        let mut head = vec!["Rank".to_owned(), "Model".to_owned()];
        head.extend(names.iter().map(|m| format!("{m} (held out)")));
        if has_test {
            head.extend(names.iter().map(|m| format!("{m} (test)")));
        }
        let k = self.rank_index();
        head.push(format!("{} (training)", names[k]));
        head.push(String::new());
        let mut t = TextTable::new(head);
        let f = |v: f64| if v.is_finite() { format_number(v, 4) } else { "—".to_owned() };
        let cells = |m: Option<(usize, [f64; 3])>| -> Vec<String> {
            m.map_or_else(|| vec!["—".to_owned(); 3], |(_, v)| v.iter().map(|&x| f(x)).collect())
        };
        for (rank, &i) in self.ranking.iter().enumerate() {
            let (kind, Outcome::Fitted { scores, .. }) = &self.outcomes[i] else { continue };
            let mut r = vec![(rank + 1).to_string(), kind.label().to_owned()];
            r.extend(cells(scores.held_out));
            if has_test {
                r.extend(cells(scores.test));
            }
            r.push(scores.training.map_or("—".to_owned(), |(_, v)| f(v[k])));
            r.push(if rank == 0 { "◀ best".to_owned() } else { String::new() });
            t.push(r);
        }
        ReportSection {
            kind: SectionKind::ModelSearchRanking,
            title: format!("Models Ranked by Held-out {} ({})", names[k], self.held_out_label),
            content: SectionContent::Table(t),
        }
    }

    /// Held-out error of each model, best at the left, labelled with its
    /// name.
    fn plot_section(&self) -> Option<ReportSection> {
        if self.ranking.is_empty() {
            return None;
        }
        let k = self.rank_index();
        let name = self.measure_names()[k];
        let mut items = Vec::new();
        let mut ticks = Vec::new();
        for (rank, &i) in self.ranking.iter().enumerate() {
            let (kind, Outcome::Fitted { scores: Scores { held_out: Some((_, m)), .. }, .. }) = &self.outcomes[i]
            else {
                continue;
            };
            let x = (rank + 1) as f64;
            items.push((x, m[k], format_number(m[k], 4), 0));
            ticks.push((x, kind.label().to_owned()));
        }
        let spec = PlotSpec {
            x_label: "Model (best first)".to_owned(),
            y_label: format!("Held-out {name}"),
            x_ticks: Some(ticks),
            annotations: Some(Annotations { groups: vec![format!("Held-out {name}")], items, labels: Vec::new() }),
            ..PlotSpec::default()
        };
        Some(ReportSection {
            kind: SectionKind::ModelSearchPlot,
            title: format!("Held-out {name} by Model"),
            content: SectionContent::Plot(Box::new(spec)),
        })
    }

    /// Each model's error across the repeats (mean, SD and a t interval for
    /// the mean), as a table and a forest plot.
    fn repeated_sections(&self) -> Vec<ReportSection> {
        let Some(repeated) = &self.repeated else { return Vec::new() };
        let r = &self.request;
        let how = if r.nested {
            format!("nested {}-fold cross-validation", self.folds())
        } else {
            format!("{}-fold cross-validation", self.folds())
        };
        let what = if self.categorical() { "Misclassification" } else { "RMSE" };
        let title = format!("{what} across {} repeats of {how}", r.repeats.clamp(1, MAX_REPEATS));
        let rows = match repeated {
            Err(e) => {
                return vec![ReportSection {
                    kind: SectionKind::RepeatedCrossValidation,
                    title,
                    content: SectionContent::Note(format!("Not available: {e}")),
                }];
            }
            Ok(rows) => rows,
        };
        let mut t = TextTable::new([
            "Model".to_owned(),
            "Repeats".to_owned(),
            format!("Mean {what}"),
            "Std Dev".to_owned(),
            "Lower 95%".to_owned(),
            "Upper 95%".to_owned(),
            "Best".to_owned(),
            "Worst".to_owned(),
        ]);
        let mut plot = Vec::new();
        for (kind, errors) in rows {
            let s = repeat_summary(errors);
            let f = |v: f64| if v.is_finite() { format_number(v, 4) } else { "—".to_owned() };
            t.push([
                kind.label().to_owned(),
                errors.len().to_string(),
                f(s.mean),
                f(s.sd),
                f(s.interval.0),
                f(s.interval.1),
                f(s.min),
                f(s.max),
            ]);
            if s.mean.is_finite() {
                plot.push(crate::fdr::IntervalRow {
                    label: kind.label().to_owned(),
                    estimate: s.mean,
                    interval: if s.interval.0.is_finite() { s.interval } else { (s.mean, s.mean) },
                    group: 0,
                });
            }
        }
        let mut out = vec![ReportSection {
            kind: SectionKind::RepeatedCrossValidation,
            title: format!(
                "{title} (mean with a 95% t interval across repeats; models in ranking order; seeds {} to {})",
                r.seed,
                r.seed.wrapping_add(r.repeats.clamp(1, MAX_REPEATS) as u64 - 1)
            ),
            content: SectionContent::Table(t),
        }];
        if !plot.is_empty() {
            out.push(crate::fdr::interval_plot(
                SectionKind::RepeatedCrossValidationPlot,
                format!("Forest Plot of {what} by Model ({how})"),
                format!("Held-out {what}"),
                &plot,
                vec![format!("Mean {what}")],
                None,
            ));
        }
        out
    }

    fn folds(&self) -> usize {
        match self.request.validation {
            ValidationMethod::KFold { folds, .. } => folds,
            _ => DEFAULT_FOLDS,
        }
    }

    /// Each ranked model's errors across the repeats, when repeated (or
    /// nested) cross-validation ran.
    #[must_use]
    pub fn repeated_errors(&self) -> Option<&[(ModelKind, Vec<f64>)]> {
        self.repeated.as_ref().and_then(|r| r.as_ref().ok()).map(Vec::as_slice)
    }

    /// The report each fitted model's row stands for, by kind.
    #[must_use]
    pub fn model_report(&self, kind: ModelKind) -> SavedReport {
        self.request.model_report(kind)
    }

    /// The report title of a fitted model.
    #[must_use]
    pub fn model_title(&self, kind: ModelKind) -> Option<&str> {
        self.outcomes.iter().find_map(|(k, o)| match o {
            Outcome::Fitted { title, .. } if *k == kind => Some(title.as_str()),
            _ => None,
        })
    }
}

/// Mean, standard deviation, 95% t interval of the mean, and range of the
/// errors of the repeats.
#[derive(Debug, Clone, Copy, PartialEq)]
struct RepeatSummary {
    mean: f64,
    sd: f64,
    interval: (f64, f64),
    min: f64,
    max: f64,
}

fn repeat_summary(errors: &[f64]) -> RepeatSummary {
    let n = errors.len();
    let mean = if n == 0 { f64::NAN } else { errors.iter().sum::<f64>() / n as f64 };
    let sd =
        if n < 2 { f64::NAN } else { (errors.iter().map(|e| (e - mean).powi(2)).sum::<f64>() / (n - 1) as f64).sqrt() };
    let interval = if n < 2 {
        (f64::NAN, f64::NAN)
    } else {
        use stat_core::distributions::{ContinuousDistribution, StudentsT};
        let t = StudentsT::new((n - 1) as f64).and_then(|d| d.quantile(0.975)).unwrap_or(f64::NAN);
        let h = t * sd / (n as f64).sqrt();
        (mean - h, mean + h)
    };
    RepeatSummary {
        mean,
        sd,
        interval,
        min: errors.iter().copied().fold(f64::INFINITY, f64::min),
        max: errors.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    }
}

// ---- Project files and scripts ------------------------------------------------

/// The request's members after `response` and `predictors`, for project files.
#[must_use]
pub fn request_members_to_json(
    request: &ModelSearchRequest,
    col: &dyn Fn(usize) -> JsonValue,
) -> Vec<(&'static str, JsonValue)> {
    vec![
        ("validation", request.validation.to_json(col)),
        ("models", JsonValue::Array(request.models.iter().map(|k| JsonValue::String(k.key().to_owned())).collect())),
        ("seed", JsonValue::Number(request.seed as f64)),
        ("repeats", JsonValue::Number(request.repeats as f64)),
        ("nested", JsonValue::Bool(request.nested)),
    ]
}

/// Read a request from a project file's report entry (missing members take
/// their defaults).
///
/// # Errors
/// An unknown model kind or validation method, or a missing column.
pub fn request_from_json(
    v: &JsonValue,
    response: usize,
    predictors: Vec<usize>,
    find: &dyn Fn(&JsonValue) -> Result<usize>,
) -> Result<ModelSearchRequest> {
    let mut r = ModelSearchRequest::new(response, predictors);
    r.validation = ValidationMethod::from_json(v.get("validation"), find)?;
    if let Some(JsonValue::Array(items)) = v.get("models") {
        r.models = items
            .iter()
            .map(|m| {
                let key = m.as_str().unwrap_or_default();
                ModelKind::from_key(key).ok_or_else(|| invalid(format!("model search: unknown model {key:?}")))
            })
            .collect::<Result<_>>()?;
    }
    r.seed = v.get("seed").and_then(crate::seed::seed_from_json).unwrap_or(r.seed);
    r.repeats = v.get("repeats").and_then(JsonValue::as_usize).unwrap_or(1).clamp(1, MAX_REPEATS);
    r.nested = v.get("nested").and_then(JsonValue::as_bool).unwrap_or(false);
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn roles(n: usize, rng: &mut stat_core::rng::Rng) -> Vec<String> {
        (0..n)
            .map(|_| {
                let u = rng.next_f64();
                if u < 0.6 {
                    "Training"
                } else if u < 0.8 {
                    "Validation"
                } else {
                    "Test"
                }
                .to_owned()
            })
            .collect()
    }

    /// A response that is a sum of hinge functions with knots (an additive
    /// MARS model, the default degree), plus noise: MARS wins.
    #[test]
    fn hinge_truth_is_won_by_mars() {
        let mut rng = stat_core::rng::Rng::new(41);
        let n = 500;
        let (mut a, mut b, mut c, mut y) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for _ in 0..n {
            let (u, v, w) = (rng.next_f64() * 10.0, rng.next_f64() * 10.0, rng.next_f64() * 10.0);
            a.push(u);
            b.push(v);
            c.push(w);
            y.push(3.0 * (u - 4.0).max(0.0) - 2.0 * (6.0 - v).max(0.0) + 1.5 * (w - 7.0).max(0.0) + 0.5 * rng.normal());
        }
        let role = roles(n, &mut rng);
        let t = DataTable::new(
            "hinges",
            vec![
                Column::numeric("a", a),
                Column::numeric("b", b),
                Column::numeric("c", c),
                Column::numeric("y", y),
                Column::text("role", role),
            ],
        )
        .unwrap();
        let states = RowStateManager::new(n);
        let request =
            ModelSearchRequest { validation: ValidationMethod::Column(4), ..ModelSearchRequest::new(3, vec![0, 1, 2]) };
        let s = ModelSearch::run(&t, &states, &request).unwrap();
        assert_eq!(s.best(), Some(ModelKind::Mars), "{:?}", s.sections()[1]);
        // Discriminant analysis does not apply to a continuous response.
        assert_eq!(s.skipped, [ModelKind::Discriminant]);
        assert_eq!(s.fitted().len(), 7);
        let sections = s.sections();
        let kinds: Vec<SectionKind> = sections.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [SectionKind::ModelSearchSummary, SectionKind::ModelSearchRanking, SectionKind::ModelSearchPlot]
        );
        let SectionContent::Table(rank) = &sections[1].content else { panic!() };
        assert_eq!(rank.rows[0][1], "Adaptive regression splines");
        assert_eq!(rank.rows[0].last().unwrap(), "◀ best");
        // The ranking matches each model's own report.
        let mars = s.model_report(ModelKind::Mars);
        let own = crate::run_report(&mars, &t, &states).unwrap();
        assert_eq!(Some(own.title.as_str()), s.model_title(ModelKind::Mars));
        // Same seed, same search.
        let again = ModelSearch::run(&t, &states, &request).unwrap();
        assert_eq!(again.sections(), sections);
    }

    /// Two classes separated by a smooth curved boundary built from tanh
    /// nodes (a neural network's own model class): the neural network
    /// wins; with every row training the search uses 5-fold
    /// cross-validation.
    #[test]
    fn network_truth_is_won_by_the_network() {
        let mut rng = stat_core::rng::Rng::new(7);
        let n = 600;
        let (mut a, mut b, mut class) = (Vec::new(), Vec::new(), Vec::new());
        for _ in 0..n {
            let (u, v) = (rng.next_f64() * 4.0 - 2.0, rng.next_f64() * 4.0 - 2.0);
            let f = 2.0 * (1.5 * u + v).tanh() - 1.5 * (u - 1.2 * v + 0.5).tanh() + (2.0 * v - 1.0).tanh();
            let p = 1.0 / (1.0 + (-4.0 * f).exp());
            a.push(u);
            b.push(v);
            class.push(if rng.next_f64() < p { "Yes" } else { "No" }.to_owned());
        }
        let t = DataTable::new(
            "curve",
            vec![Column::numeric("a", a), Column::numeric("b", b), Column::text("class", class)],
        )
        .unwrap();
        let states = RowStateManager::new(n);
        let request = ModelSearchRequest::new(2, vec![0, 1]);
        let s = ModelSearch::run(&t, &states, &request).unwrap();
        assert_eq!(s.held_out_label, "5-fold cross-validation");
        assert_eq!(s.skipped, [ModelKind::Mars]);
        assert_eq!(s.best(), Some(ModelKind::NeuralNetwork), "{:?}", s.sections()[1]);
    }

    /// Repeated k-fold: the first repeat uses the search's own folds, so its
    /// error is each model's held-out RMSE in the ranking; the summary is the
    /// mean, sample SD and t interval of the repeats. Nested k-fold scores
    /// every row once per repeat on a model tuned without it.
    #[test]
    fn repeated_and_nested_cross_validation() {
        let mut rng = stat_core::rng::Rng::new(5);
        let n = 120;
        let x1: Vec<f64> = (0..n).map(|_| rng.next_f64() * 4.0).collect();
        let x2: Vec<f64> = (0..n).map(|_| rng.next_f64()).collect();
        let y: Vec<f64> =
            x1.iter().zip(&x2).map(|(a, b)| (a - 2.0).max(0.0) * 3.0 + b + 0.3 * rng.next_f64()).collect();
        let t =
            DataTable::new("t", vec![Column::numeric("x1", x1), Column::numeric("x2", x2), Column::numeric("y", y)])
                .unwrap();
        let states = RowStateManager::new(n);
        let request = ModelSearchRequest {
            models: vec![ModelKind::DecisionTree, ModelKind::Mars],
            repeats: 4,
            ..ModelSearchRequest::new(2, vec![0, 1])
        };
        let s = ModelSearch::run(&t, &states, &request).unwrap();
        let errors = s.repeated_errors().unwrap();
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[0].0, s.best().unwrap(), "ranking order");
        for (kind, e) in errors {
            assert_eq!(e.len(), 4);
            let (_, Outcome::Fitted { scores, .. }) = s.outcomes.iter().find(|(k, _)| k == kind).unwrap() else {
                panic!()
            };
            assert!((e[0] - scores.held_out.unwrap().1[0]).abs() < 1e-12, "first repeat = the search's folds");
            assert!(e.iter().any(|v| (v - e[0]).abs() > 1e-9), "other repeats use other folds");
        }
        let sections = s.sections();
        let table = sections.iter().find(|x| x.kind == SectionKind::RepeatedCrossValidation).unwrap();
        let SectionContent::Table(tab) = &table.content else { panic!() };
        let e = &errors[0].1;
        let mean = e.iter().sum::<f64>() / 4.0;
        let sd = (e.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / 3.0).sqrt();
        // t(0.975, 3) = 3.182446305284263.
        let lower = mean - 3.182_446_305_284_263 * sd / 2.0;
        assert_eq!(tab.rows[0][2], format_number(mean, 4));
        assert_eq!(tab.rows[0][3], format_number(sd, 4));
        assert_eq!(tab.rows[0][4], format_number(lower, 4));
        assert!(sections.iter().any(|x| x.kind == SectionKind::RepeatedCrossValidationPlot));

        let nested = ModelSearchRequest { repeats: 2, nested: true, ..request.clone() };
        let a = ModelSearch::run(&t, &states, &nested).unwrap();
        let b = ModelSearch::run(&t, &states, &nested).unwrap();
        assert_eq!(a.repeated_errors(), b.repeated_errors(), "reproducible");
        for (_, e) in a.repeated_errors().unwrap() {
            assert_eq!(e.len(), 2);
            assert!(e.iter().all(|v| v.is_finite() && *v > 0.0));
        }
        // Validation rows: no k-fold to repeat.
        let col: Vec<String> = (0..n).map(|i| if i % 4 == 0 { "Validation" } else { "Training" }.to_owned()).collect();
        let mut t2 = t.clone();
        t2.insert_column(3, Column::text("role", col)).unwrap();
        let held = ModelSearchRequest { validation: ValidationMethod::Column(3), ..request };
        let c = ModelSearch::run(&t2, &states, &held).unwrap();
        assert!(c.repeated_errors().is_none());
        let note = c.sections().into_iter().find(|x| x.kind == SectionKind::RepeatedCrossValidation).unwrap();
        assert!(matches!(note.content, SectionContent::Note(_)));
    }

    #[test]
    fn json_and_errors() {
        let t = DataTable::new(
            "t",
            vec![
                Column::numeric("x", vec![1.0, 2.0, 3.0, 4.0]),
                Column::numeric("y", vec![1.0, 2.0, 3.0, 5.0]),
                Column::text("role", vec!["Training".into(), "Training".into(), "Test".into(), "Test".into()]),
            ],
        )
        .unwrap();
        let request = ModelSearchRequest {
            validation: ValidationMethod::Holdout { validation: 0.3, test: 0.1, seed: 4, stratify: false, group: None },
            models: vec![ModelKind::NeuralNetwork, ModelKind::Mars],
            seed: 12,
            ..ModelSearchRequest::new(1, vec![0])
        };
        let col = |i: usize| JsonValue::String(t.column(i).unwrap().name.clone());
        let v = JsonValue::Object(
            request_members_to_json(&request, &col).into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
        );
        let find = |c: &JsonValue| Ok(t.find(c.as_str().unwrap()).unwrap());
        assert_eq!(request_from_json(&v, 1, vec![0], &find).unwrap(), request);
        let bad = JsonValue::Object(vec![("models".to_owned(), JsonValue::Array(vec![JsonValue::String("x".into())]))]);
        assert!(request_from_json(&bad, 1, vec![0], &find).is_err());
        // Test rows but no validation rows: nothing to rank by.
        let states = RowStateManager::new(4);
        let only_test =
            ModelSearchRequest { validation: ValidationMethod::Column(2), ..ModelSearchRequest::new(1, vec![0]) };
        assert!(ModelSearch::run(&t, &states, &only_test).is_err());
        assert!(ModelSearch::run(&t, &states, &ModelSearchRequest::new(1, vec![1])).is_err());
    }
}
