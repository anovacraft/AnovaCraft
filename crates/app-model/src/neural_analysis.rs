//! Neural networks bound to a dataset ([`stat_predictive::neural`]): a fully
//! connected network with one or two hidden layers for a continuous
//! response (linear output) or a categorical one (logistic or softmax
//! output). Uses the shared predictive-model sections
//! ([`crate::prediction`]).
//!
//! **Inputs.** Continuous predictors are standardized with the training
//! rows' mean and standard deviation; each level of a categorical predictor
//! becomes a 0/1 indicator. Rows missing a predictor take no role. A
//! continuous response is standardized too, and predictions are returned in
//! its units.
//!
//! **Tuning.** The weight-decay penalty α is chosen from 10⁻⁴, 10⁻³, …, 10
//! by the smallest held-out error (root mean square error in the response's
//! units, or log loss for classes): on the validation rows, or by k-fold
//! cross-validation (the request's folds, or 5 folds when every row trains).
//! Ties go to the larger penalty. α can be fixed instead. The final network
//! is fitted to the training rows; with k-fold roles the fit table shows each
//! row's prediction from the fold that held it out.

use rayon::prelude::*;
use stat_core::{Result, StatError};
use stat_data::{ColumnKind, DataTable, JsonValue, RowStateManager};
use stat_linear::TermValue;
use stat_predictive::measures::ClassFit;
use stat_predictive::{
    Activation, Feature, Network, NetworkOptions, NetworkTask, Output, Role, Solver, Value, fit_network,
    fold_assignment,
};

use crate::format::format_number;
use crate::heat_map::HeatMapSpec;
use crate::prediction::{
    FeatureSet, InputEncoding, PredictionFormulas, Predictions, Predictor, PredictorInfo, Response, RoleAssignment,
    ValidationMethod, confusion_section, fit_measures_section, prediction_columns, setting_value,
};
use crate::report::{Annotations, PlotSpec, PointGroups, ReportSection, SectionContent, SectionKind, TextTable};
use crate::surface_profiler::{Observation, ProfileFactor, SurfaceModel};

/// Training rows and held-out rows for one tuning fit.
type Split = (Vec<usize>, Vec<usize>);

const DIGITS: usize = 6;
/// Penalties tried when α is tuned.
pub const PENALTIES: [f64; 6] = [1e-4, 1e-3, 1e-2, 1e-1, 1.0, 10.0];

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Columns and settings of a neural network.
#[derive(Debug, Clone, PartialEq)]
pub struct NeuralRequest {
    /// Response column (categorical: classification; continuous:
    /// regression).
    pub response: usize,
    /// Predictor columns.
    pub predictors: Vec<usize>,
    /// Training / validation / test roles.
    pub validation: ValidationMethod,
    /// Nodes per hidden layer (one or two layers).
    pub hidden: Vec<usize>,
    /// Activation of the hidden nodes.
    pub activation: Activation,
    /// A fixed penalty α (`None`: tuned).
    pub penalty: Option<f64>,
    /// Optimizer (`None`: L-BFGS up to 2,000 training rows, Adam above).
    pub solver: Option<Solver>,
    /// Random starts.
    pub starts: usize,
    /// Most L-BFGS iterations or Adam passes.
    pub max_iter: usize,
    /// Seed of the starting weights (and of the tuning folds when every row
    /// trains).
    pub seed: u64,
}

impl NeuralRequest {
    /// Three tanh nodes in one hidden layer, α tuned, three starts, every
    /// row training.
    #[must_use]
    pub fn new(response: usize, predictors: Vec<usize>) -> Self {
        Self {
            response,
            predictors,
            validation: ValidationMethod::None,
            hidden: vec![3],
            activation: Activation::Tanh,
            penalty: None,
            solver: None,
            starts: 3,
            max_iter: 2000,
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

    fn options(&self, penalty: f64) -> NetworkOptions {
        NetworkOptions {
            hidden: self.hidden.clone(),
            activation: self.activation,
            penalty,
            solver: self.solver,
            starts: self.starts,
            max_iter: self.max_iter,
            seed: self.seed,
            ..NetworkOptions::default()
        }
    }
}

/// The fitted network, with the response's centre and scale (0 and 1 for
/// classes).
#[derive(Debug, Clone, PartialEq)]
struct Fitted {
    network: Network,
    centre: f64,
    scale: f64,
}

impl Fitted {
    fn predict(&self, x: &[f64]) -> Vec<f64> {
        let p = self.network.predict(x);
        if self.network.output() == Output::Linear { vec![self.centre + self.scale * p[0]] } else { p }
    }
}

/// How the held-out errors of the penalties were measured.
#[derive(Debug, Clone, PartialEq)]
enum Tuning {
    /// α fixed.
    Fixed,
    /// Validation rows.
    Validation,
    /// k-fold cross-validation of the training rows.
    CrossValidation { folds: usize },
}

/// A fitted neural network.
#[derive(Debug, Clone)]
pub struct NeuralAnalysis {
    request: NeuralRequest,
    response_name: String,
    profile_name: String,
    response: Response,
    roles: RoleAssignment,
    predictors: Vec<PredictorInfo>,
    features: Vec<Feature>,
    encoding: InputEncoding,
    fitted: Fitted,
    tuning: Tuning,
    /// Each penalty with its held-out error.
    grid: Vec<(f64, f64)>,
    predictions: Predictions,
    held_out: Option<Predictions>,
    profile_class: usize,
}

impl NeuralAnalysis {
    /// Tune, fit and evaluate the network.
    ///
    /// # Errors
    /// No predictors, the response among them, too few classes, invalid
    /// settings, an invalid validation method, too few complete training
    /// rows, or a fit that diverges.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &NeuralRequest) -> Result<Self> {
        if request.predictors.is_empty() {
            return Err(invalid("choose at least one predictor"));
        }
        if request.predictors.contains(&request.response) {
            return Err(invalid("the response cannot also be a predictor"));
        }
        if request.hidden.is_empty() || request.hidden.len() > 2 || request.hidden.iter().any(|&h| h == 0 || h > 1000) {
            return Err(invalid("use one or two hidden layers of 1 to 1,000 nodes"));
        }
        if request.penalty.is_some_and(|a| !(a >= 0.0 && a.is_finite())) {
            return Err(invalid("the penalty must not be negative"));
        }
        if request.starts == 0 || request.starts > 100 {
            return Err(invalid("use 1 to 100 random starts"));
        }
        if request.max_iter == 0 {
            return Err(invalid("allow at least one iteration"));
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
            return Err(invalid("a neural network needs at least three training rows with every predictor present"));
        }
        let encoding = InputEncoding::new(&features, &train);
        let inputs: Vec<Option<Vec<f64>>> =
            (0..table.n_rows()).map(|r| encoding.encode(&|j| features[j].value(r))).collect();
        let x = |r: usize| inputs[r].as_deref().unwrap_or(&[]);
        let width = response.classes().map_or(1, <[String]>::len);
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
        let task_for = |rows: &[usize]| -> NetworkTask {
            match &response {
                Response::Continuous(y) => {
                    NetworkTask::Regression { target: rows.iter().map(|&r| (y[r] - centre) / scale).collect() }
                }
                Response::Categorical { codes, classes } => NetworkTask::Classification {
                    class: rows.iter().map(|&r| codes[r].unwrap_or(0)).collect(),
                    classes: classes.len(),
                },
            }
        };
        let fit = |rows: &[usize], penalty: f64| -> Result<Fitted> {
            let px: Vec<&[f64]> = rows.iter().map(|&r| x(r)).collect();
            let network = fit_network(&px, &task_for(rows), &request.options(penalty))?;
            Ok(Fitted { network, centre, scale })
        };
        // Held-out error of `model` on `rows`: RMSE in the response's units, or log loss.
        let error = |model: &Fitted, rows: &[usize]| -> f64 {
            match &response {
                Response::Continuous(y) => {
                    let s: f64 = rows.iter().map(|&r| (model.predict(x(r))[0] - y[r]).powi(2)).sum();
                    (s / rows.len().max(1) as f64).sqrt()
                }
                Response::Categorical { codes, classes } => {
                    let obs: Vec<usize> = rows.iter().map(|&r| codes[r].unwrap_or(0)).collect();
                    let probs: Vec<Vec<f64>> = rows.iter().map(|&r| model.predict(x(r))).collect();
                    ClassFit::new(&obs, &probs, classes.len()).map_or(f64::INFINITY, |m| m.log_loss)
                }
            }
        };

        let (tuning, grid, penalty) = match request.penalty {
            Some(a) => (Tuning::Fixed, Vec::new(), a),
            None => {
                let (tuning, splits): (Tuning, Vec<Split>) = if !valid.is_empty() {
                    (Tuning::Validation, vec![(train.clone(), valid.clone())])
                } else {
                    let (folds, seed) = roles.folds.unwrap_or((5, request.seed));
                    let folds = folds.min(train.len()).max(2);
                    let fold_of = fold_assignment(train.len(), folds, seed)?;
                    let splits = (0..folds)
                        .map(|f| {
                            let (out, fit_rows): (Vec<_>, Vec<_>) =
                                train.iter().copied().zip(fold_of.iter().copied()).partition(|&(_, k)| k == f);
                            (fit_rows.into_iter().map(|p| p.0).collect(), out.into_iter().map(|p| p.0).collect())
                        })
                        .collect();
                    (Tuning::CrossValidation { folds }, splits)
                };
                let errors: Vec<f64> = PENALTIES
                    .par_iter()
                    .map(|&a| {
                        // Pooled over folds: RMSE from the summed squares, log loss from the summed losses.
                        let mut total = 0.0;
                        let mut count = 0usize;
                        for (fit_rows, held) in &splits {
                            let Ok(m) = fit(fit_rows, a) else { return f64::INFINITY };
                            let e = error(&m, held);
                            let n = held.len();
                            total += if response.classes().is_some() { e * n as f64 } else { e * e * n as f64 };
                            count += n;
                        }
                        let mean = total / count.max(1) as f64;
                        if response.classes().is_some() { mean } else { mean.sqrt() }
                    })
                    .collect();
                // Smallest error; ties to the larger penalty.
                let best = (0..PENALTIES.len()).rev().min_by(|&a, &b| errors[a].total_cmp(&errors[b])).unwrap_or(0);
                (tuning, PENALTIES.iter().copied().zip(errors).collect(), PENALTIES[best])
            }
        };
        let fitted = fit(&train, penalty)?;

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
                    let Ok(m) = fit(&fit_rows, penalty) else { continue };
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
            tuning,
            grid,
            predictions,
            held_out,
            profile_class,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &NeuralRequest {
        &self.request
    }

    /// The penalty α used.
    #[must_use]
    pub fn penalty(&self) -> f64 {
        self.fitted.network.penalty
    }

    /// The fitted network (on standardized inputs, and a standardized
    /// continuous response).
    #[must_use]
    pub fn network(&self) -> &Network {
        &self.fitted.network
    }

    fn is_classification(&self) -> bool {
        self.response.classes().is_some()
    }

    fn error_name(&self) -> &'static str {
        if self.is_classification() { "Log loss" } else { "RMSE" }
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let names: Vec<&str> = self.predictors.iter().map(|p| p.name.as_str()).collect();
        format!("Neural Network ({}): {} ~ {}", self.structure(), self.response_name, names.join(" + "))
    }

    /// Hidden layer sizes and activation, e.g. "3 tanh" or "5–3 logistic".
    fn structure(&self) -> String {
        let sizes: Vec<String> = self.request.hidden.iter().map(ToString::to_string).collect();
        format!("{} {}", sizes.join("–"), self.request.activation.key())
    }

    /// Names of the network's inputs (a categorical predictor gives one per
    /// level).
    fn input_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for info in &self.predictors {
            match &info.levels {
                Some(levels) => names.extend(levels.iter().map(|l| format!("{} = {l}", info.name))),
                None => names.push(info.name.clone()),
            }
        }
        names
    }

    fn output_names(&self) -> Vec<String> {
        match (self.response.classes(), self.fitted.network.output()) {
            (Some(c), Output::Logistic) => vec![format!("P({} = {})", self.response_name, c[1])],
            (Some(c), _) => c.iter().map(|k| format!("P({} = {k})", self.response_name)).collect(),
            (None, _) => vec![self.response_name.clone()],
        }
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary_section(), self.diagram_section()];
        if !self.grid.is_empty() {
            out.push(self.tuning_table());
            out.push(self.tuning_plot());
        }
        out.push(fit_measures_section(self));
        out.extend(confusion_section(self));
        out.extend(self.weight_sections());
        out
    }

    fn summary_section(&self) -> ReportSection {
        let net = &self.fitted.network;
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut row = |a: &str, b: String| t.push([a.to_owned(), b]);
        row("Response", self.response_name.clone());
        let output = match (self.response.classes(), net.output()) {
            (Some(c), Output::Logistic) => {
                format!("one logistic node, the probability of {} (against {}); cross-entropy loss", c[1], c[0])
            }
            (Some(c), _) => format!("softmax over {} classes; cross-entropy loss", c.len()),
            (None, _) => "one linear node (standardized response); squared-error loss".to_owned(),
        };
        let hidden: Vec<String> = self
            .request
            .hidden
            .iter()
            .enumerate()
            .map(|(k, h)| format!("layer {}: {h} node{}", k + 1, if *h == 1 { "" } else { "s" }))
            .collect();
        row("Hidden layers", format!("{}; activation {}", hidden.join(", "), self.request.activation.formula()));
        row("Output", output);
        for role in Role::ALL {
            let n = self.roles.rows(role).len();
            if n > 0 || role == Role::Training {
                row(&format!("{} rows", role.label()), n.to_string());
            }
        }
        let how = match &self.tuning {
            Tuning::Fixed => "set in the request".to_owned(),
            Tuning::Validation => "smallest validation error".to_owned(),
            Tuning::CrossValidation { folds } => format!("smallest {folds}-fold cross-validated error"),
        };
        row("Penalty α", format!("{} ({how}); objective = mean loss + (α / 2n) Σ w²", format_number(net.penalty, 4)));
        row("Weights and biases", net.parameter_count().to_string());
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
        row(
            "Solver",
            format!(
                "{}, {} {}{}",
                net.solver.label(),
                net.iterations,
                if net.solver == Solver::Lbfgs { "iterations" } else { "passes" },
                if net.converged {
                    ""
                } else {
                    " (stopped at the iteration limit; more iterations may lower the objective a little)"
                }
            ),
        );
        let starts: Vec<String> = net.start_objectives.iter().map(|f| format_number(*f, 5)).collect();
        row(
            "Random starts",
            format!(
                "{}: final objectives {}; kept start {}",
                net.start_objectives.len(),
                starts.join(", "),
                net.best_start + 1
            ),
        );
        row("Training objective", format_number(net.objective, DIGITS));
        row("Seed", self.request.seed.to_string());
        ReportSection {
            kind: SectionKind::NeuralSummary,
            title: "Neural Network Summary".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    /// The network as layers of labelled nodes joined by their connections.
    fn diagram_section(&self) -> ReportSection {
        let sizes = self.fitted.network.sizes().to_vec();
        let layers = sizes.len();
        let y_of = |j: usize, m: usize| (m as f64 - 1.0) / 2.0 - j as f64;
        let inputs = self.input_names();
        let outputs = self.output_names();
        let mut group_names = vec!["Inputs".to_owned()];
        for k in 1..layers - 1 {
            group_names.push(format!("Hidden layer {k}"));
        }
        group_names.push("Output".to_owned());
        let mut items = Vec::new();
        for (l, &m) in sizes.iter().enumerate() {
            for j in 0..m {
                // Inputs are named on the vertical axis, clear of the connections.
                let label = if l == 0 {
                    String::new()
                } else if l == layers - 1 {
                    outputs.get(j).cloned().unwrap_or_default()
                } else {
                    format!("H{l}.{}", j + 1)
                };
                items.push((l as f64, y_of(j, m), label, l));
            }
        }
        let mut segments = Vec::new();
        for (l, layer) in self.fitted.network.layers.iter().enumerate() {
            for o in 0..layer.outputs {
                for i in 0..layer.inputs {
                    segments.push(vec![[l as f64, y_of(i, layer.inputs)], [(l + 1) as f64, y_of(o, layer.outputs)]]);
                }
            }
        }
        let activation = self.request.activation.key();
        let ticks = (0..layers)
            .map(|l| {
                let label = if l == 0 {
                    "Inputs".to_owned()
                } else if l == layers - 1 {
                    "Output".to_owned()
                } else {
                    format!("Hidden {l} ({activation})")
                };
                (l as f64, label)
            })
            .collect();
        let spec = PlotSpec {
            x_label: String::new(),
            y_label: String::new(),
            x_ticks: Some(ticks),
            y_ticks: Some(inputs.iter().enumerate().map(|(j, name)| (y_of(j, sizes[0]), name.clone())).collect()),
            annotations: Some(Annotations { groups: group_names, items, labels: Vec::new() }),
            segments,
            ..PlotSpec::default()
        };
        ReportSection {
            kind: SectionKind::NeuralDiagram,
            title: "Network Diagram".to_owned(),
            content: SectionContent::Plot(Box::new(spec)),
        }
    }

    fn tuning_table(&self) -> ReportSection {
        let mut t = TextTable::new(["Penalty α", self.error_name(), ""]);
        let used = self.fitted.network.penalty;
        for &(a, e) in &self.grid {
            t.push([
                format_number(a, 4),
                if e.is_finite() { format_number(e, DIGITS) } else { "—".to_owned() },
                if a == used { "◀ used".to_owned() } else { String::new() },
            ]);
        }
        ReportSection {
            kind: SectionKind::NeuralTuning,
            title: format!("Penalty Tuning ({})", self.tuning_label()),
            content: SectionContent::Table(t),
        }
    }

    fn tuning_label(&self) -> String {
        match &self.tuning {
            Tuning::Validation => "validation rows".to_owned(),
            Tuning::CrossValidation { folds } => format!("{folds}-fold cross-validation"),
            Tuning::Fixed => String::new(),
        }
    }

    /// Held-out error against log₁₀ α, with the chosen penalty marked.
    fn tuning_plot(&self) -> ReportSection {
        let line: Vec<[f64; 2]> = self.grid.iter().filter(|g| g.1.is_finite()).map(|&(a, e)| [a.log10(), e]).collect();
        let used = self.fitted.network.penalty;
        let items: Vec<(f64, f64, String, usize)> = self
            .grid
            .iter()
            .filter(|g| g.0 == used && g.1.is_finite())
            .map(|&(a, e)| (a.log10(), e, "used".to_owned(), 0))
            .collect();
        let label = format!("Held-out {}", self.error_name().to_lowercase().replace("rmse", "RMSE"));
        let spec = PlotSpec {
            x_label: "log₁₀ α".to_owned(),
            y_label: label.clone(),
            groups: Some(PointGroups {
                title: String::new(),
                labels: vec![label.clone()],
                membership: Vec::new(),
                lines: vec![line],
            }),
            annotations: Some(Annotations { groups: vec![label.clone()], items, labels: Vec::new() }),
            ..PlotSpec::default()
        };
        ReportSection {
            kind: SectionKind::NeuralTuningPlot,
            title: format!("{label} by Penalty"),
            content: SectionContent::Plot(Box::new(spec)),
        }
    }

    /// One heat map per layer of weights: rows are the layer's nodes,
    /// columns the nodes feeding it and the bias.
    fn weight_sections(&self) -> Vec<ReportSection> {
        let net = &self.fitted.network;
        let layers = net.layers.len();
        let hidden_names = |l: usize, m: usize| (0..m).map(|j| format!("H{l}.{}", j + 1)).collect::<Vec<_>>();
        let outputs = self.output_names();
        net.layers
            .iter()
            .enumerate()
            .map(|(l, layer)| {
                let from = if l == 0 { self.input_names() } else { hidden_names(l, layer.inputs) };
                let to = if l == layers - 1 { outputs.clone() } else { hidden_names(l + 1, layer.outputs) };
                let mut columns = from.clone();
                columns.push("Bias".to_owned());
                let values: Vec<Vec<f64>> = (0..layer.outputs)
                    .map(|o| (0..layer.inputs).map(|i| layer.weight(o, i)).chain([layer.bias[o]]).collect())
                    .collect();
                let text = values.iter().map(|r| r.iter().map(|v| format_number(*v, 3)).collect()).collect();
                let limit = values.iter().flatten().fold(0.0f64, |m, v| m.max(v.abs()));
                let from_name = if l == 0 { "Inputs".to_owned() } else { format!("Hidden Layer {l}") };
                let to_name = if l == layers - 1 { "Output".to_owned() } else { format!("Hidden Layer {}", l + 1) };
                ReportSection {
                    kind: SectionKind::NeuralWeights,
                    title: format!("Weights: {from_name} to {to_name}"),
                    content: SectionContent::HeatMap(HeatMapSpec {
                        rows: to,
                        columns,
                        values,
                        text,
                        measure: "Weight".to_owned(),
                        limit: if limit > 0.0 { limit } else { 1.0 },
                    }),
                }
            })
            .collect()
    }

    /// The fitted network as a formula (see [`crate::formula`]) for a
    /// continuous response: standardized inputs, then each hidden node, then
    /// the output in the response's units.
    ///
    /// # Errors
    /// A categorical response, or a date-time column used as a category.
    pub fn prediction_formula(&self) -> Result<String> {
        use crate::formula::column_reference;
        use crate::script_text::{number, string};
        if self.is_classification() {
            return Err(invalid("prediction formulas are written for a continuous response"));
        }
        let mut out = String::new();
        let mut names = Vec::new();
        for (j, p) in self.predictors.iter().enumerate() {
            let x = column_reference(&p.name);
            match (self.encoding.numeric[j], &p.levels) {
                (Some((centre, scale)), _) => {
                    let v = format!("x{}", names.len() + 1);
                    out.push_str(&format!("let {v} = ({x} - {}) / {};\n", number(centre), number(scale)));
                    names.push(v);
                }
                (None, levels) => {
                    if matches!(p.kind, ColumnKind::DateTime(_)) {
                        return Err(invalid(format!(
                            "the date-time column {:?} is used as a category; a formula cannot match its labels",
                            p.name
                        )));
                    }
                    for (level, _) in levels.as_deref().unwrap_or_default().iter().enumerate() {
                        let arms: Vec<String> = levels
                            .as_deref()
                            .unwrap_or_default()
                            .iter()
                            .enumerate()
                            .map(|(i, l)| format!("{} => {}", string(l), if i == level { "1.0" } else { "0.0" }))
                            .collect();
                        let v = format!("x{}", names.len() + 1);
                        out.push_str(&format!(
                            "let {v} = switch text({x}) {{ {}, _ => missing() }};\n",
                            arms.join(", ")
                        ));
                        names.push(v);
                    }
                }
            }
        }
        let activation = match self.request.activation {
            Activation::Tanh => |z: String| format!("tanh({z})"),
            Activation::Logistic => |z: String| format!("logistic({z})"),
            Activation::Relu => |z: String| format!("max({z}, 0.0)"),
            Activation::Identity => |z: String| z,
        };
        let net = &self.fitted.network;
        let last = net.layers.len() - 1;
        let mut previous = names;
        for (l, layer) in net.layers.iter().enumerate() {
            let mut current = Vec::new();
            for o in 0..layer.outputs {
                let mut z = number(layer.bias[o]);
                for (i, name) in previous.iter().enumerate() {
                    z.push_str(&format!(" + {} * {name}", number(layer.weight(o, i))));
                }
                if l == last {
                    out.push_str(&format!("{} + {} * ({z})", number(self.fitted.centre), number(self.fitted.scale)));
                } else {
                    let v = format!("h{}_{}", l + 1, o + 1);
                    out.push_str(&format!("let {v} = {};\n", activation(z)));
                    current.push(v);
                }
            }
            previous = current;
        }
        Ok(out)
    }

    /// Columns the report can save: predicted values (and, for a continuous
    /// response, the prediction formula as a formula column), or class
    /// probabilities and the most likely class, and the validation roles.
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<crate::ReportColumn> {
        let mut out = prediction_columns(self, &PredictionFormulas::default());
        if let (Ok(f), Predictions::Values(p)) = (self.prediction_formula(), &self.predictions) {
            let mut c = crate::ReportColumn::continuous(
                "prediction_formula",
                "Save prediction formula",
                format!("Predicted {} (formula)", self.response_name),
                p.clone(),
            );
            c.formula = Some(f);
            out.insert(1, c);
        }
        out
    }
}

impl Predictor for NeuralAnalysis {
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

impl SurfaceModel for NeuralAnalysis {
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
    request: &NeuralRequest,
    col: &dyn Fn(usize) -> JsonValue,
) -> Vec<(&'static str, JsonValue)> {
    vec![
        ("validation", request.validation.to_json(col)),
        ("hidden", JsonValue::Array(request.hidden.iter().map(|&h| JsonValue::Number(h as f64)).collect())),
        ("activation", JsonValue::String(request.activation.key().to_owned())),
        ("penalty", request.penalty.map_or(JsonValue::Null, JsonValue::number)),
        ("solver", request.solver.map_or(JsonValue::Null, |s| JsonValue::String(s.key().to_owned()))),
        ("starts", JsonValue::Number(request.starts as f64)),
        ("max_iter", JsonValue::Number(request.max_iter as f64)),
        ("seed", JsonValue::Number(request.seed as f64)),
    ]
}

/// Read a request from a project file's report entry (missing members take
/// their defaults).
///
/// # Errors
/// An unknown activation, solver or validation method, a missing column,
/// hidden layers that are not one or two positive counts, or a negative
/// penalty.
pub fn request_from_json(
    v: &JsonValue,
    response: usize,
    predictors: Vec<usize>,
    find: &dyn Fn(&JsonValue) -> Result<usize>,
) -> Result<NeuralRequest> {
    let mut r = NeuralRequest::new(response, predictors);
    r.validation = ValidationMethod::from_json(v.get("validation"), find)?;
    match v.get("hidden") {
        None | Some(JsonValue::Null) => {}
        Some(JsonValue::Array(items)) => {
            let sizes: Option<Vec<usize>> = items.iter().map(JsonValue::as_usize).collect();
            match sizes {
                Some(s) if (1..=2).contains(&s.len()) && s.iter().all(|&h| (1..=1000).contains(&h)) => r.hidden = s,
                _ => return Err(invalid("neural network: hidden must list one or two node counts")),
            }
        }
        Some(other) => match other.as_usize() {
            Some(h) if (1..=1000).contains(&h) => r.hidden = vec![h],
            _ => return Err(invalid("neural network: hidden must list one or two node counts")),
        },
    }
    if let Some(a) = v.get("activation").and_then(JsonValue::as_str) {
        r.activation =
            Activation::from_key(a).ok_or_else(|| invalid(format!("neural network: unknown activation {a:?}")))?;
    }
    if let Some(a) = v.get("penalty").and_then(JsonValue::as_f64) {
        if !(a >= 0.0 && a.is_finite()) {
            return Err(invalid("neural network: penalty must not be negative"));
        }
        r.penalty = Some(a);
    }
    if let Some(s) = v.get("solver").and_then(JsonValue::as_str) {
        r.solver = match s {
            "auto" => None,
            _ => Some(Solver::from_key(s).ok_or_else(|| invalid(format!("neural network: unknown solver {s:?}")))?),
        };
    }
    if let Some(s) = v.get("starts").and_then(JsonValue::as_usize) {
        r.starts = s.clamp(1, 100);
    }
    if let Some(m) = v.get("max_iter").and_then(JsonValue::as_usize) {
        r.max_iter = m.max(1);
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
        let req = NeuralRequest {
            validation: ValidationMethod::Column(5),
            hidden: vec![4],
            ..NeuralRequest::new(4, vec![0, 1])
        };
        let a = NeuralAnalysis::run(&t, &rows, &req).unwrap();
        assert_eq!(a.title(), "Neural Network (4 tanh): class ~ a + b");
        let s = a.sections();
        let kinds: Vec<SectionKind> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::NeuralSummary,
                SectionKind::NeuralDiagram,
                SectionKind::NeuralTuning,
                SectionKind::NeuralTuningPlot,
                SectionKind::FitByRole,
                SectionKind::ConfusionMatrix,
                SectionKind::NeuralWeights,
                SectionKind::NeuralWeights,
            ]
        );
        let SectionContent::Table(grid) = &s[2].content else { panic!() };
        assert_eq!(grid.rows.len(), PENALTIES.len());
        assert_eq!(grid.rows.iter().filter(|r| r[2] == "◀ used").count(), 1);
        let SectionContent::Table(fit) = &s[4].content else { panic!() };
        let err = fit.rows[2][2].parse::<f64>().unwrap();
        assert!(err < 0.1, "test misclassification {err}");
        let Predictions::Probabilities(p) = a.predictions() else { panic!() };
        assert!(p.iter().all(|row| (row.iter().sum::<f64>() - 1.0).abs() < 1e-9));
        // Diagram: 2 inputs, 4 hidden, 1 output node; 2·4 + 4·1 connections.
        let SectionContent::Plot(d) = &s[1].content else { panic!() };
        assert_eq!(d.annotations.as_ref().unwrap().items.len(), 7);
        assert_eq!(d.segments.len(), 12);
        // Weight maps: hidden ← inputs (4 × (2 + bias)), output ← hidden (1 × (4 + bias)).
        let SectionContent::HeatMap(w) = &s[6].content else { panic!() };
        assert_eq!((w.rows.len(), w.columns.len()), (4, 3));
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
        // Same seed, same report.
        let b = NeuralAnalysis::run(&t, &rows, &req).unwrap();
        assert_eq!(a.sections(), b.sections());
    }

    #[test]
    fn regression_with_folds_formula_and_json() {
        let t = table();
        let rows = RowStateManager::new(t.n_rows());
        let req = NeuralRequest {
            validation: ValidationMethod::KFold { folds: 4, seed: 2 },
            hidden: vec![5, 3],
            ..NeuralRequest::new(3, vec![0, 1, 2])
        };
        let a = NeuralAnalysis::run(&t, &rows, &req).unwrap();
        let s = a.sections();
        let SectionContent::Table(fit) = &s[4].content else { panic!() };
        assert_eq!(fit.rows.iter().map(|r| r[0].as_str()).collect::<Vec<_>>(), ["Training", "Cross-validation"]);
        let r2 = fit.rows[1][4].parse::<f64>().unwrap();
        assert!(r2 > 0.9, "cross-validated R² {r2}");
        assert_eq!(s.iter().filter(|x| x.kind == SectionKind::NeuralWeights).count(), 3);
        // The formula reproduces the predictions.
        let formula = a.prediction_formula().unwrap();
        assert!(formula.contains("tanh("), "{formula}");
        let cols = a.saveable_columns();
        assert_eq!(cols[1].key, "prediction_formula");
        let eval = crate::formula::evaluate(&t, None, &formula, 0..t.n_rows()).unwrap();
        let Predictions::Values(p) = a.predictions() else { panic!() };
        for (v, q) in eval.values.iter().zip(p) {
            let crate::formula::FormulaValue::Number(v) = v else { panic!("{v:?}") };
            assert!((v - q).abs() < 1e-9 * (1.0 + q.abs()), "{v} vs {q}");
        }
        // A fixed penalty: no tuning sections.
        let fixed = NeuralRequest { penalty: Some(0.01), ..NeuralRequest::new(3, vec![0, 1, 2]) };
        let b = NeuralAnalysis::run(&t, &rows, &fixed).unwrap();
        assert_eq!(b.penalty(), 0.01);
        assert!(!b.sections().iter().any(|x| x.kind == SectionKind::NeuralTuning));
        // JSON round trip.
        let mut req = req;
        req.activation = Activation::Relu;
        req.penalty = Some(0.5);
        req.solver = Some(Solver::Adam);
        req.starts = 4;
        req.max_iter = 300;
        let col = |i: usize| JsonValue::String(t.column(i).unwrap().name.clone());
        let v = JsonValue::Object(
            request_members_to_json(&req, &col).into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
        );
        let find = |c: &JsonValue| Ok(t.find(c.as_str().unwrap()).unwrap());
        assert_eq!(request_from_json(&v, req.response, req.predictors.clone(), &find).unwrap(), req);
        let bad = JsonValue::Object(vec![("activation".to_owned(), JsonValue::String("sine".to_owned()))]);
        assert!(request_from_json(&bad, 0, vec![1], &find).is_err());
        let bad = JsonValue::Object(vec![("hidden".to_owned(), JsonValue::Array(Vec::new()))]);
        assert!(request_from_json(&bad, 0, vec![1], &find).is_err());
        assert!(
            NeuralAnalysis::run(&t, &rows, &NeuralRequest { hidden: vec![], ..NeuralRequest::new(3, vec![0]) })
                .is_err()
        );
    }
}
