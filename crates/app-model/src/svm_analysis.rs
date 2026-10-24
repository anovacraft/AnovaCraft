//! Support vector machines bound to a dataset
//! ([`stat_predictive::svm`]): C-support vector classification (one machine
//! per pair of classes, Platt probabilities) or ε-support vector regression.
//! Uses the shared predictive-model sections ([`crate::prediction`]).
//!
//! **Inputs.** Continuous predictors are standardized with the training
//! rows' mean and standard deviation; each level of a categorical predictor
//! becomes a 0/1 indicator. Rows missing a predictor take no role. For
//! regression the response is standardized too, so ε is in response
//! standard deviations; predictions are returned in the response's units.
//!
//! **Tuning.** The cost C, and γ for the radial basis and polynomial
//! kernels, are chosen on a grid (C = 2⁻², 2⁰, …, 2⁶; γ = g, 4g, g/4, g/16
//! with g = 1 / number of inputs) by the smallest held-out error: on the
//! validation rows, or by k-fold cross-validation (the request's folds, or 5
//! folds when every row trains). Ties go to the smaller C, then the smaller
//! γ. Either value can be fixed instead. The final machine is fitted to the
//! training rows; with k-fold roles the fit table shows each row's
//! prediction from the fold that held it out.

use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_linear::TermValue;
use stat_predictive::svm::PairMachine;
use stat_predictive::{
    Feature, Kernel, Role, SvmClassifier, SvmMachine, SvmOptions, SvmTask, TuningSplit, Value, fit_classifier, fit_svr,
    fold_assignment, tune,
};

use crate::format::format_number;
use crate::prediction::{
    FeatureSet, InputEncoding, PredictionFormulas, Predictions, Predictor, PredictorInfo, Response, RoleAssignment,
    ValidationMethod, confusion_section, fit_measures_section, prediction_columns, setting_value,
};
use crate::report::{Annotations, PlotSpec, PointGroups, ReportSection, SectionContent, SectionKind, TextTable};
use crate::surface_profiler::{Observation, ProfileFactor, SurfaceModel};

const DIGITS: usize = 6;
/// Costs tried when C is tuned.
const COSTS: [f64; 5] = [0.25, 1.0, 4.0, 16.0, 64.0];
/// Multiples of 1 / (number of inputs) tried when γ is tuned.
const GAMMAS: [f64; 4] = [0.0625, 0.25, 1.0, 4.0];
/// Offset c₀ of the polynomial kernel.
const COEF0: f64 = 1.0;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Kernel family of an SVM request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvmKernel {
    /// `uᵀv`.
    Linear,
    /// `exp(−γ‖u − v‖²)`.
    Rbf,
    /// `(γuᵀv + 1)ᵈ`.
    Polynomial {
        /// Degree d (2 to 5).
        degree: u32,
    },
}

impl SvmKernel {
    /// Name for reports and project files.
    #[must_use]
    pub fn key(&self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::Rbf => "radial",
            Self::Polynomial { .. } => "polynomial",
        }
    }

    fn with_gamma(&self, gamma: f64) -> Kernel {
        match *self {
            Self::Linear => Kernel::Linear,
            Self::Rbf => Kernel::Rbf { gamma },
            Self::Polynomial { degree } => Kernel::Polynomial { degree, gamma, coef0: COEF0 },
        }
    }
}

/// Columns and settings of a support vector machine.
#[derive(Debug, Clone, PartialEq)]
pub struct SvmRequest {
    /// Response column (categorical: classification; continuous:
    /// regression).
    pub response: usize,
    /// Predictor columns.
    pub predictors: Vec<usize>,
    /// Training / validation / test roles.
    pub validation: ValidationMethod,
    /// Kernel family.
    pub kernel: SvmKernel,
    /// A fixed cost C (`None`: tuned).
    pub cost: Option<f64>,
    /// A fixed γ (`None`: tuned; unused by the linear kernel).
    pub gamma: Option<f64>,
    /// Half-width of the insensitive zone for regression, in response
    /// standard deviations.
    pub epsilon: f64,
    /// Seed of the tuning folds (when every row trains) and of the folds
    /// that fit the probability scaling.
    pub seed: u64,
}

impl SvmRequest {
    /// Radial basis kernel, C and γ tuned, ε = 0.1, every row training.
    #[must_use]
    pub fn new(response: usize, predictors: Vec<usize>) -> Self {
        Self {
            response,
            predictors,
            validation: ValidationMethod::None,
            kernel: SvmKernel::Rbf,
            cost: None,
            gamma: None,
            epsilon: 0.1,
            seed: 1,
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
}

/// The fitted machine.
#[derive(Debug, Clone, PartialEq)]
enum Fitted {
    Classifier(SvmClassifier),
    /// The machine on the standardized response, with its centre and scale.
    Regressor(SvmMachine, f64, f64),
}

impl Fitted {
    fn predict(&self, x: &[f64]) -> Vec<f64> {
        match self {
            Self::Classifier(c) => c.probabilities(x),
            Self::Regressor(m, centre, scale) => vec![centre + scale * m.decision(x)],
        }
    }

    fn support_vectors(&self) -> (usize, usize) {
        match self {
            Self::Classifier(c) => (c.support_vectors, c.pairs.iter().map(|m| m.machine.bounded).sum()),
            Self::Regressor(m, ..) => (m.support.len(), m.bounded),
        }
    }

    /// Class pairs whose machine predicts one class everywhere.
    fn constant_pairs(&self) -> Vec<(usize, usize)> {
        match self {
            Self::Classifier(c) => c.pairs.iter().filter(|p| p.constant).map(|p| (p.first, p.second)).collect(),
            Self::Regressor(..) => Vec::new(),
        }
    }

    fn machines(&self) -> Vec<&SvmMachine> {
        match self {
            Self::Classifier(c) => c.pairs.iter().map(|p: &PairMachine| &p.machine).collect(),
            Self::Regressor(m, ..) => vec![m],
        }
    }
}

/// How the held-out errors of the tuning grid were measured.
#[derive(Debug, Clone, PartialEq)]
enum Tuning {
    /// C (and γ) fixed.
    Fixed,
    /// Validation rows.
    Validation,
    /// k-fold cross-validation of the training rows.
    CrossValidation { folds: usize },
}

/// A fitted support vector machine.
#[derive(Debug, Clone)]
pub struct SvmAnalysis {
    request: SvmRequest,
    response_name: String,
    profile_name: String,
    response: Response,
    roles: RoleAssignment,
    predictors: Vec<PredictorInfo>,
    features: Vec<Feature>,
    encoding: InputEncoding,
    fitted: Fitted,
    cost: f64,
    gamma: Option<f64>,
    tuning: Tuning,
    /// Each grid point (C, γ) with its held-out error.
    grid: Vec<(f64, Option<f64>, f64)>,
    predictions: Predictions,
    held_out: Option<Predictions>,
    profile_class: usize,
}

impl SvmAnalysis {
    /// Tune, fit and evaluate the model.
    ///
    /// # Errors
    /// No predictors, the response among them, too few classes, invalid
    /// settings, an invalid validation method, or too few complete training
    /// rows.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &SvmRequest) -> Result<Self> {
        if request.predictors.is_empty() {
            return Err(invalid("choose at least one predictor"));
        }
        if request.predictors.contains(&request.response) {
            return Err(invalid("the response cannot also be a predictor"));
        }
        if request.cost.is_some_and(|c| !(c > 0.0 && c.is_finite())) {
            return Err(invalid("the cost C must be positive"));
        }
        if request.gamma.is_some_and(|g| !(g > 0.0 && g.is_finite())) {
            return Err(invalid("γ must be positive"));
        }
        if !(request.epsilon >= 0.0 && request.epsilon.is_finite()) {
            return Err(invalid("ε must not be negative"));
        }
        if let SvmKernel::Polynomial { degree } = request.kernel {
            if !(1..=10).contains(&degree) {
                return Err(invalid("the polynomial degree must be between 1 and 10"));
            }
        }
        let included: Vec<usize> = (0..table.n_rows()).filter(|&r| !states.is_excluded(r)).collect();
        let response = Response::read(table, request.response, &included)?;
        let response_name = table.column(request.response).map_or(String::new(), |c| c.name.clone());
        let mut roles = RoleAssignment::assign(table, states, &response, &request.validation)?;
        let all_train = roles.rows(Role::Training);
        let FeatureSet { predictors, features } = FeatureSet::read(table, &request.predictors, &all_train)?;
        let complete = |r: usize| features.iter().all(|f| f.value(r) != Value::Missing);
        for (r, role) in roles.roles.iter_mut().enumerate() {
            if role.is_some() && !complete(r) {
                *role = None;
            }
        }
        let train = roles.rows(Role::Training);
        let valid = roles.rows(Role::Validation);
        if train.len() < 3 {
            return Err(invalid("an SVM needs at least three training rows with every predictor present"));
        }
        let encoding = InputEncoding::new(&features, &train);
        let inputs: Vec<Option<Vec<f64>>> =
            (0..table.n_rows()).map(|r| encoding.encode(&|j| features[j].value(r))).collect();
        let x = |r: usize| inputs[r].as_deref().unwrap_or(&[]);
        let width = response.classes().map_or(1, <[String]>::len);
        // Task over table rows (targets for every row; only role rows are used).
        let (centre, scale) = match &response {
            Response::Continuous(y) => {
                let v: Vec<f64> = train.iter().map(|&r| y[r]).collect();
                let n = v.len() as f64;
                let mean = v.iter().sum::<f64>() / n;
                let sd = (v.iter().map(|a| (a - mean).powi(2)).sum::<f64>() / (n - 1.0)).sqrt();
                (mean, if sd > 0.0 { sd } else { 1.0 })
            }
            Response::Categorical { .. } => (0.0, 1.0),
        };
        let task_for = |rows: &[usize]| -> SvmTask {
            match &response {
                Response::Continuous(y) => SvmTask::Regression {
                    target: rows.iter().map(|&r| (y[r] - centre) / scale).collect(),
                    epsilon: request.epsilon,
                },
                Response::Categorical { codes, classes } => SvmTask::Classification {
                    class: rows.iter().map(|&r| codes[r].unwrap_or(0)).collect(),
                    classes: classes.len(),
                },
            }
        };
        let options = SvmOptions::default();
        let fit = |rows: &[usize], kernel: Kernel, cost: f64| -> Result<Fitted> {
            let px: Vec<&[f64]> = rows.iter().map(|&r| x(r)).collect();
            Ok(match task_for(rows) {
                SvmTask::Classification { class, classes } => Fitted::Classifier(fit_classifier(
                    &px,
                    &class,
                    classes,
                    cost,
                    kernel,
                    Some(request.seed),
                    &options,
                )?),
                SvmTask::Regression { target, epsilon } => {
                    Fitted::Regressor(fit_svr(&px, &target, cost, epsilon, kernel, &options)?, centre, scale)
                }
            })
        };

        // Tuning grid.
        let base = 1.0 / encoding.inputs().max(1) as f64;
        let costs: Vec<f64> = request.cost.map_or(COSTS.to_vec(), |c| vec![c]);
        let gammas: Vec<Option<f64>> = match request.kernel {
            SvmKernel::Linear => vec![None],
            _ => request.gamma.map_or(GAMMAS.iter().map(|g| Some(g * base)).collect(), |g| vec![Some(g)]),
        };
        let mut candidates = Vec::new();
        for &g in &gammas {
            for &c in &costs {
                candidates.push((g, c));
            }
        }
        let kernel_of = |g: Option<f64>| request.kernel.with_gamma(g.unwrap_or(base));
        let (tuning, grid, best) = if candidates.len() == 1 {
            (Tuning::Fixed, Vec::new(), candidates[0])
        } else {
            let (points, split, tuning) = if !valid.is_empty() {
                let points: Vec<usize> = train.iter().chain(&valid).copied().collect();
                let split = TuningSplit::Holdout {
                    train: (0..train.len()).collect(),
                    valid: (train.len()..points.len()).collect(),
                };
                (points, split, Tuning::Validation)
            } else {
                let (folds, seed) = roles.folds.unwrap_or((5, request.seed));
                let folds = folds.min(train.len()).max(2);
                (
                    train.clone(),
                    TuningSplit::Folds(fold_assignment(train.len(), folds, seed)?),
                    Tuning::CrossValidation { folds },
                )
            };
            let px: Vec<&[f64]> = points.iter().map(|&r| x(r)).collect();
            let task = task_for(&points);
            let kernels: Vec<(Kernel, f64)> = candidates.iter().map(|&(g, c)| (kernel_of(g), c)).collect();
            let errors = tune(&px, &task, &kernels, &split, &options)?;
            // Smallest error; ties to the smaller C, then the smaller γ.
            let mut order: Vec<usize> = (0..candidates.len()).collect();
            order.sort_by(|&a, &b| {
                let (ga, ca) = candidates[a];
                let (gb, cb) = candidates[b];
                errors[a]
                    .total_cmp(&errors[b])
                    .then(ca.total_cmp(&cb))
                    .then(ga.unwrap_or(0.0).total_cmp(&gb.unwrap_or(0.0)))
            });
            let grid = candidates.iter().zip(&errors).map(|(&(g, c), &e)| (c, g, e)).collect();
            (tuning, grid, candidates[order[0]])
        };
        let (gamma, cost) = best;
        let kernel = kernel_of(gamma);
        let fitted = fit(&train, kernel, cost)?;
        let gamma = kernel.gamma();

        let wrap = |values: Vec<Vec<f64>>| match &response {
            Response::Continuous(_) => Predictions::Values(values.into_iter().map(|v| v[0]).collect()),
            Response::Categorical { .. } => Predictions::Probabilities(values),
        };
        let predictions = wrap(
            (0..table.n_rows())
                .map(|r| if inputs[r].is_some() { fitted.predict(x(r)) } else { vec![f64::NAN; width] })
                .collect(),
        );
        let held_out = match roles.folds {
            Some((folds, seed)) => {
                let folds = folds.min(train.len()).max(2);
                let fold_of = fold_assignment(train.len(), folds, seed)?;
                let mut values = vec![vec![f64::NAN; width]; table.n_rows()];
                for f in 0..folds {
                    let fit_rows: Vec<usize> =
                        train.iter().zip(&fold_of).filter(|(_, k)| **k != f).map(|(&r, _)| r).collect();
                    let Ok(m) = fit(&fit_rows, kernel, cost) else { continue };
                    for (&r, _) in train.iter().zip(&fold_of).filter(|(_, k)| **k == f) {
                        values[r] = m.predict(x(r));
                    }
                }
                Some(wrap(values))
            }
            None => None,
        };
        let profile_class = match &response {
            Response::Continuous(_) => 0,
            Response::Categorical { classes, .. } if classes.len() == 2 => 1,
            Response::Categorical { codes, classes } => {
                let mut counts = vec![0usize; classes.len()];
                for &r in &train {
                    if let Some(c) = codes[r] {
                        counts[c] += 1;
                    }
                }
                (0..classes.len()).max_by_key(|&k| (counts[k], usize::MAX - k)).unwrap_or(0)
            }
        };
        let profile_name = match response.classes() {
            None => response_name.clone(),
            Some(c) => format!("P({response_name} = {})", c[profile_class]),
        };
        Ok(Self {
            request: request.clone(),
            response_name,
            profile_name,
            response,
            roles,
            predictors,
            features,
            encoding,
            fitted,
            cost,
            gamma,
            tuning,
            grid,
            predictions,
            held_out,
            profile_class,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &SvmRequest {
        &self.request
    }

    /// The cost C used.
    #[must_use]
    pub fn cost(&self) -> f64 {
        self.cost
    }

    /// The kernel's γ used (`None` for the linear kernel).
    #[must_use]
    pub fn gamma(&self) -> Option<f64> {
        self.gamma
    }

    /// Distinct support vectors, and those at the bound C (summed over the
    /// pairwise machines).
    #[must_use]
    pub fn support_vectors(&self) -> (usize, usize) {
        self.fitted.support_vectors()
    }

    fn is_classification(&self) -> bool {
        self.response.classes().is_some()
    }

    fn error_name(&self) -> &'static str {
        if self.is_classification() { "Misclassification" } else { "Mean squared error (standardized)" }
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let names: Vec<&str> = self.predictors.iter().map(|p| p.name.as_str()).collect();
        format!(
            "Support Vector Machine ({} kernel): {} ~ {}",
            self.request.kernel.key(),
            self.response_name,
            names.join(" + ")
        )
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary_section()];
        if !self.grid.is_empty() {
            out.push(self.tuning_table());
            out.push(self.tuning_plot());
        }
        out.push(fit_measures_section(self));
        out.extend(confusion_section(self));
        out.extend(self.weights_section());
        out
    }

    fn kernel_text(&self) -> String {
        let g = self.gamma.map_or(String::new(), |g| format_number(g, 4));
        match self.request.kernel {
            SvmKernel::Linear => "linear, uᵀv".to_owned(),
            SvmKernel::Rbf => format!("radial basis, exp(−γ‖u − v‖²) with γ = {g}"),
            SvmKernel::Polynomial { degree } => format!("polynomial, (γuᵀv + 1)^{degree} with γ = {g}"),
        }
    }

    fn summary_section(&self) -> ReportSection {
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut row = |a: &str, b: String| t.push([a.to_owned(), b]);
        row("Response", self.response_name.clone());
        row(
            "Model",
            match self.response.classes() {
                Some(c) if c.len() == 2 => format!("C-support vector classification of {} against {}", c[1], c[0]),
                Some(c) => format!(
                    "C-support vector classification, one machine per pair of classes ({} classes, {} machines)",
                    c.len(),
                    self.fitted.machines().len()
                ),
                None => format!(
                    "ε-support vector regression, ε = {} response standard deviations",
                    format_number(self.request.epsilon, 4)
                ),
            },
        );
        for role in Role::ALL {
            let n = self.roles.rows(role).len();
            if n > 0 || role == Role::Training {
                row(&format!("{} rows", role.label()), n.to_string());
            }
        }
        row("Kernel", self.kernel_text());
        let how = match &self.tuning {
            Tuning::Fixed => "set in the request".to_owned(),
            Tuning::Validation => "smallest validation error on the grid".to_owned(),
            Tuning::CrossValidation { folds } => format!("smallest {folds}-fold cross-validated error on the grid"),
        };
        row("Cost C", format!("{} ({how})", format_number(self.cost, 4)));
        let (sv, bounded) = self.fitted.support_vectors();
        let n_train = self.roles.rows(Role::Training).len();
        row(
            "Support vectors",
            format!(
                "{sv} of {n_train} training rows ({}%); {bounded} at the bound C",
                format_number(100.0 * sv as f64 / n_train.max(1) as f64, 3)
            ),
        );
        let categorical = self.encoding.numeric.iter().filter(|n| n.is_none()).count();
        row(
            "Inputs",
            format!(
                "{} ({} continuous predictors standardized{})",
                self.encoding.inputs(),
                self.encoding.numeric.len() - categorical,
                if categorical > 0 {
                    format!("; {categorical} categorical as one indicator per level")
                } else {
                    String::new()
                }
            ),
        );
        if self.is_classification() {
            row("Probabilities", "Platt scaling of each machine (5-fold), coupled across pairs".to_owned());
        }
        let constant = self.fitted.constant_pairs();
        if !constant.is_empty() {
            let classes = self.response.classes().unwrap_or_default();
            let name = |k: usize| classes.get(k).map_or("?", String::as_str);
            let pairs: Vec<String> =
                constant.iter().map(|&(a, b)| format!("{} against {}", name(b), name(a))).collect();
            row(
                "Warning",
                format!(
                    "no separating direction found for {}: the best machine has zero weights and puts every row in \
                     one class, so its probabilities are flat (the AUC is 0.5). A curved kernel (radial or polynomial) may find one.",
                    pairs.join(", ")
                ),
            );
        }
        if self.fitted.machines().iter().any(|m| !m.converged) {
            row("Note", "the solver stopped at its iteration limit before meeting its tolerance".to_owned());
        }
        ReportSection {
            kind: SectionKind::SvmSummary,
            title: "Support Vector Machine Summary".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    fn tuning_table(&self) -> ReportSection {
        let has_gamma = self.grid.iter().any(|g| g.1.is_some());
        let mut head = vec!["C".to_owned()];
        if has_gamma {
            head.push("γ".to_owned());
        }
        head.push(self.error_name().to_owned());
        head.push(String::new());
        let mut t = TextTable::new(head);
        for &(c, g, e) in &self.grid {
            let mut r = vec![format_number(c, 4)];
            if has_gamma {
                r.push(g.map_or(String::new(), |g| format_number(g, 4)));
            }
            r.push(format_number(e, DIGITS));
            r.push(if c == self.cost && g == self.gamma { "◀ used".to_owned() } else { String::new() });
            t.push(r);
        }
        ReportSection {
            kind: SectionKind::SvmTuning,
            title: format!(
                "Tuning ({})",
                match &self.tuning {
                    Tuning::Validation => "validation rows".to_owned(),
                    Tuning::CrossValidation { folds } => format!("{folds}-fold cross-validation"),
                    Tuning::Fixed => String::new(),
                }
            ),
            content: SectionContent::Table(t),
        }
    }

    /// Held-out error against log₂ C, one line per γ, each line labelled at
    /// its end and the chosen point marked.
    fn tuning_plot(&self) -> ReportSection {
        let mut gammas: Vec<Option<f64>> = self.grid.iter().map(|g| g.1).collect();
        gammas.sort_by(|a, b| a.unwrap_or(0.0).total_cmp(&b.unwrap_or(0.0)));
        gammas.dedup();
        let labels: Vec<String> = gammas
            .iter()
            .map(|g| g.map_or("Linear kernel".to_owned(), |g| format!("γ = {}", format_number(g, 3))))
            .collect();
        let lines: Vec<Vec<[f64; 2]>> = gammas
            .iter()
            .map(|g| self.grid.iter().filter(|p| p.1 == *g).map(|&(c, _, e)| [c.log2(), e]).collect())
            .collect();
        let mut items: Vec<(f64, f64, String, usize)> = lines
            .iter()
            .enumerate()
            .filter_map(|(k, l)| l.last().map(|&[x, y]| (x, y, labels[k].clone(), k)))
            .collect();
        if let Some(k) = gammas.iter().position(|g| *g == self.gamma) {
            if let Some(&(c, _, e)) = self.grid.iter().find(|p| p.0 == self.cost && p.1 == self.gamma) {
                items.push((c.log2(), e, "used".to_owned(), k));
            }
        }
        let spec = PlotSpec {
            x_label: "log₂ C".to_owned(),
            y_label: self.error_name().to_owned(),
            groups: Some(PointGroups { title: "γ".to_owned(), labels: labels.clone(), membership: Vec::new(), lines }),
            annotations: Some(Annotations { groups: labels, items, labels: Vec::new() }),
            ..PlotSpec::default()
        };
        ReportSection {
            kind: SectionKind::SvmTuningPlot,
            title: format!("Held-out {} by Cost", self.error_name().to_lowercase()),
            content: SectionContent::Plot(Box::new(spec)),
        }
    }

    /// Weights of a linear machine with one decision function, in
    /// standardized input units.
    fn weights_section(&self) -> Option<ReportSection> {
        let machines = self.fitted.machines();
        let [m] = machines.as_slice() else { return None };
        let w = m.linear_weights()?;
        let mut names = Vec::new();
        for info in &self.predictors {
            match &info.levels {
                Some(levels) => names.extend(levels.iter().map(|l| format!("{} = {l}", info.name))),
                None => names.push(info.name.clone()),
            }
        }
        let mut order: Vec<usize> = (0..w.len()).collect();
        order.sort_by(|&a, &b| w[b].abs().total_cmp(&w[a].abs()));
        let mut t = TextTable::new(["Input", "Weight"]);
        for k in order {
            t.push([names.get(k).cloned().unwrap_or_default(), format_number(w[k], DIGITS)]);
        }
        t.push(["Offset (−ρ)".to_owned(), format_number(-m.rho, DIGITS)]);
        Some(ReportSection {
            kind: SectionKind::SvmWeights,
            title: "Linear Weights (standardized inputs; larger magnitude, more influence)".to_owned(),
            content: SectionContent::Table(t),
        })
    }

    /// Columns the report can save: predicted values, or class probabilities
    /// and the most likely class (as values), and the validation roles.
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<crate::ReportColumn> {
        prediction_columns(self, &PredictionFormulas::default())
    }
}

impl Predictor for SvmAnalysis {
    fn response_name(&self) -> &str {
        &self.response_name
    }

    fn response(&self) -> &Response {
        &self.response
    }

    fn roles(&self) -> &RoleAssignment {
        &self.roles
    }

    fn predictions(&self) -> &Predictions {
        &self.predictions
    }

    fn cross_validated(&self) -> Option<&Predictions> {
        self.held_out.as_ref()
    }
}

impl SurfaceModel for SvmAnalysis {
    // The profilers show the class probability, e.g. "P(Fault = Yes)".
    #[allow(clippy::misnamed_getters)]
    fn response_name(&self) -> &str {
        &self.profile_name
    }

    fn profile_factors(&self) -> Vec<ProfileFactor> {
        FeatureSet { predictors: self.predictors.clone(), features: Vec::new() }.profile_factors()
    }

    fn default_profile_settings(&self) -> Vec<TermValue> {
        self.predictors.iter().map(|p| p.default).collect()
    }

    fn predict_settings(&self, points: &[Vec<TermValue>]) -> Result<Vec<f64>> {
        points
            .iter()
            .map(|s| {
                if s.len() != self.predictors.len() {
                    return Err(invalid("the settings do not match the predictors"));
                }
                let x = self
                    .encoding
                    .encode(&|j| setting_value(&self.features[j], &s[j]))
                    .ok_or_else(|| invalid("every predictor needs a value"))?;
                let p = self.fitted.predict(&x);
                Ok(p[if self.is_classification() { self.profile_class } else { 0 }])
            })
            .collect()
    }

    fn observations(&self) -> Vec<Observation> {
        let set = FeatureSet { predictors: Vec::new(), features: self.features.clone() };
        set.observations(&self.roles.rows(Role::Training), &|r| match &self.response {
            Response::Continuous(y) => y[r],
            Response::Categorical { codes, .. } => f64::from(u8::from(codes[r] == Some(self.profile_class))),
        })
    }

    fn profile_note(&self) -> Option<String> {
        Some(format!(
            "No confidence bands.{}",
            if self.is_classification() {
                format!(" Shown: the probability {}.", self.profile_name)
            } else {
                String::new()
            }
        ))
    }
}

// ---- Project files and scripts ------------------------------------------------

/// The request's members after `response` and `predictors`, for project files.
#[must_use]
pub fn request_members_to_json(
    request: &SvmRequest,
    col: &dyn Fn(usize) -> JsonValue,
) -> Vec<(&'static str, JsonValue)> {
    let opt = |x: Option<f64>| x.map_or(JsonValue::Null, JsonValue::number);
    let mut out = vec![
        ("validation", request.validation.to_json(col)),
        ("kernel", JsonValue::String(request.kernel.key().to_owned())),
    ];
    if let SvmKernel::Polynomial { degree } = request.kernel {
        out.push(("degree", JsonValue::Number(f64::from(degree))));
    }
    out.extend([
        ("cost", opt(request.cost)),
        ("gamma", opt(request.gamma)),
        ("epsilon", JsonValue::number(request.epsilon)),
        ("seed", JsonValue::Number(request.seed as f64)),
    ]);
    out
}

/// Read a request from a project file's report entry (missing members take
/// their defaults).
///
/// # Errors
/// An unknown kernel or validation method, a missing column, or a
/// non-positive cost or γ, or a negative ε.
pub fn request_from_json(
    v: &JsonValue,
    response: usize,
    predictors: Vec<usize>,
    find: &dyn Fn(&JsonValue) -> Result<usize>,
) -> Result<SvmRequest> {
    let mut r = SvmRequest::new(response, predictors);
    r.validation = ValidationMethod::from_json(v.get("validation"), find)?;
    if let Some(k) = v.get("kernel").and_then(JsonValue::as_str) {
        r.kernel = match k {
            "linear" => SvmKernel::Linear,
            "radial" => SvmKernel::Rbf,
            "polynomial" => SvmKernel::Polynomial {
                degree: v.get("degree").and_then(JsonValue::as_usize).map_or(3, |d| d.min(10) as u32),
            },
            other => return Err(invalid(format!("support vector machine: unknown kernel {other:?}"))),
        };
    }
    for (key, slot) in [("cost", &mut r.cost), ("gamma", &mut r.gamma)] {
        if let Some(x) = v.get(key).and_then(JsonValue::as_f64) {
            if !(x > 0.0 && x.is_finite()) {
                return Err(invalid(format!("support vector machine: {key} must be positive")));
            }
            *slot = Some(x);
        }
    }
    if let Some(e) = v.get("epsilon").and_then(JsonValue::as_f64) {
        if !(e >= 0.0 && e.is_finite()) {
            return Err(invalid("support vector machine: epsilon must not be negative"));
        }
        r.epsilon = e;
    }
    r.seed = v.get("seed").and_then(crate::seed::seed_from_json).unwrap_or(r.seed);
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    /// 240 rows: a ring-shaped class in (a, b), a text group predictor, a
    /// continuous response curved in a, and a role column.
    fn table() -> DataTable {
        let mut rng = stat_core::rng::Rng::new(11);
        let n = 240;
        let (mut a, mut b, mut y, mut class, mut group, mut role) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for i in 0..n {
            let (u, v) = (rng.next_f64() * 4.0 - 2.0, rng.next_f64() * 4.0 - 2.0);
            let g = if i % 2 == 0 { "P" } else { "Q" };
            a.push(u);
            b.push(v);
            y.push((2.0 * u).sin() + 0.5 * v + if g == "Q" { 1.0 } else { 0.0 } + 0.1 * rng.normal());
            class.push(if u * u + v * v < 1.3 { "In" } else { "Out" }.to_owned());
            group.push(g.to_owned());
            role.push(["Training", "Training", "Validation", "Test"][i % 4].to_owned());
        }
        DataTable::new(
            "ring",
            vec![
                Column::numeric("a", a),
                Column::numeric("b", b),
                Column::text("group", group),
                Column::numeric("y", y),
                Column::text("class", class),
                Column::text("role", role),
            ],
        )
        .unwrap()
    }

    #[test]
    fn classification_tuned_on_validation_rows() {
        let t = table();
        let rows = RowStateManager::new(t.n_rows());
        let req = SvmRequest { validation: ValidationMethod::Column(5), ..SvmRequest::new(4, vec![0, 1]) };
        let a = SvmAnalysis::run(&t, &rows, &req).unwrap();
        assert!(a.title().starts_with("Support Vector Machine (radial kernel): class ~ a + b"));
        let s = a.sections();
        let kinds: Vec<SectionKind> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::SvmSummary,
                SectionKind::SvmTuning,
                SectionKind::SvmTuningPlot,
                SectionKind::FitByRole,
                SectionKind::ConfusionMatrix,
            ]
        );
        let SectionContent::Table(grid) = &s[1].content else { panic!() };
        assert_eq!(grid.rows.len(), 20);
        assert_eq!(grid.rows.iter().filter(|r| r[3] == "◀ used").count(), 1);
        let SectionContent::Table(fit) = &s[3].content else { panic!() };
        let err = fit.rows[2][2].parse::<f64>().unwrap();
        assert!(err < 0.12, "test misclassification {err}");
        let Predictions::Probabilities(p) = a.predictions() else { panic!() };
        assert!(p.iter().all(|row| (row.iter().sum::<f64>() - 1.0).abs() < 1e-6));
        // Profiler: the centre is "In" (class 0), so P(class = Out) is low there.
        let mut centre = a.default_profile_settings();
        centre[0] = TermValue::Number(0.0);
        centre[1] = TermValue::Number(0.0);
        let mut corner = centre.clone();
        corner[0] = TermValue::Number(1.9);
        corner[1] = TermValue::Number(1.9);
        let q = a.predict_settings(&[centre, corner]).unwrap();
        assert!(q[0] < 0.2 && q[1] > 0.8, "{q:?}");
        let keys: Vec<String> = a.saveable_columns().into_iter().map(|c| c.key).collect();
        assert_eq!(keys, ["probability_1", "probability_2", "most_likely", "validation"]);
    }

    #[test]
    fn regression_with_folds_categorical_input_and_json() {
        let t = table();
        let rows = RowStateManager::new(t.n_rows());
        let req = SvmRequest {
            validation: ValidationMethod::KFold { folds: 4, seed: 2 },
            cost: Some(16.0),
            ..SvmRequest::new(3, vec![0, 1, 2])
        };
        let a = SvmAnalysis::run(&t, &rows, &req).unwrap();
        assert_eq!(a.cost(), 16.0);
        let s = a.sections();
        let SectionContent::Table(grid) = &s[1].content else { panic!() };
        assert_eq!(grid.rows.len(), 4, "γ tuned alone");
        let SectionContent::Table(fit) = &s[3].content else { panic!() };
        assert_eq!(fit.rows.iter().map(|r| r[0].as_str()).collect::<Vec<_>>(), ["Training", "Cross-validation"]);
        let r2 = fit.rows[1][4].parse::<f64>().unwrap();
        assert!(r2 > 0.85, "cross-validated R² {r2}");
        // A linear kernel with C fixed: no tuning, weights shown, and the group
        // indicator carries the 1.0 shift.
        let lin = SvmRequest { kernel: SvmKernel::Linear, cost: Some(1.0), ..SvmRequest::new(3, vec![0, 1, 2]) };
        let b = SvmAnalysis::run(&t, &rows, &lin).unwrap();
        let kinds: Vec<SectionKind> = b.sections().iter().map(|x| x.kind).collect();
        assert_eq!(kinds, [SectionKind::SvmSummary, SectionKind::FitByRole, SectionKind::SvmWeights]);
        // JSON round trip.
        let mut req = req;
        req.kernel = SvmKernel::Polynomial { degree: 2 };
        req.gamma = Some(0.5);
        req.epsilon = 0.2;
        let col = |i: usize| JsonValue::String(t.column(i).unwrap().name.clone());
        let v = JsonValue::Object(
            request_members_to_json(&req, &col).into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
        );
        let find = |c: &JsonValue| Ok(t.find(c.as_str().unwrap()).unwrap());
        assert_eq!(request_from_json(&v, req.response, req.predictors.clone(), &find).unwrap(), req);
        let bad = JsonValue::Object(vec![("kernel".to_owned(), JsonValue::String("sigmoid".to_owned()))]);
        assert!(request_from_json(&bad, 0, vec![1], &find).is_err());
        assert!(SvmAnalysis::run(&t, &rows, &SvmRequest { cost: Some(0.0), ..SvmRequest::new(3, vec![0]) }).is_err());
    }
}
