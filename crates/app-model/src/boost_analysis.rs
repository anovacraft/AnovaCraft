//! Gradient-boosted trees bound to a dataset ([`stat_predictive::boost`]):
//! a sum of small trees, each fitted to the gradient of the loss at the
//! current fit and shrunk by a learning rate. Uses the shared
//! predictive-model sections ([`crate::prediction`]).
//!
//! **Rows.** As for decision trees: excluded rows and rows missing the
//! response are left out; missing predictor values go the way each split
//! learned for them. Training rows fit the trees. Validation rows choose the
//! number of rounds (boosting stops once their loss has not improved for the
//! set number of rounds, keeping the best); with k-fold validation the
//! number of rounds minimizes the cross-validated loss instead, and the fit
//! table shows the cross-validated predictions. Test rows only measure the
//! model. Without validation, every round is kept.
//!
//! **Importance.** The total loss reduction (gain) of each predictor's
//! splits, with its share and the number of splits.

use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_linear::TermValue;
use stat_predictive::boost::{BoostLoss, loss_curve};
use stat_predictive::{
    BoostOptions, Boosted, Feature, RegressionLoss, Role, cross_validate_rounds, fit_boosted, fold_assignment,
};

use crate::format::format_number;
use crate::prediction::{
    FeatureSet, PredictionFormulas, Predictions, Predictor, PredictorInfo, Response, RoleAssignment, ValidationMethod,
    confusion_section, fit_measures_section, prediction_columns, setting_value,
};
use crate::report::{Annotations, PlotSpec, PointGroups, ReportSection, SectionContent, SectionKind, TextTable};
use crate::surface_profiler::{Observation, ProfileFactor, SurfaceModel};

const DIGITS: usize = 6;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Columns and settings of a boosted-tree model.
#[derive(Debug, Clone, PartialEq)]
pub struct BoostRequest {
    /// Response column.
    pub response: usize,
    /// Predictor columns.
    pub predictors: Vec<usize>,
    /// Training / validation / test roles.
    pub validation: ValidationMethod,
    /// Greatest number of rounds.
    pub rounds: usize,
    /// Shrinkage of each tree, in (0, 1].
    pub learning_rate: f64,
    /// Greatest depth of each tree.
    pub max_depth: usize,
    /// Fewest training rows in a leaf.
    pub min_leaf: usize,
    /// L2 penalty on leaf values.
    pub lambda: f64,
    /// Share of the training rows each round uses, in (0, 1].
    pub subsample: f64,
    /// Rounds without improvement on the validation rows before stopping
    /// (0: never stop early).
    pub early_stopping: usize,
    /// Loss for a continuous response.
    pub regression_loss: RegressionLoss,
    /// Seed of the subsamples (and of k-fold assignment).
    pub seed: u64,
}

impl BoostRequest {
    /// Default settings ([`BoostOptions::default`]) with every row training.
    #[must_use]
    pub fn new(response: usize, predictors: Vec<usize>) -> Self {
        let d = BoostOptions::default();
        Self {
            response,
            predictors,
            validation: ValidationMethod::None,
            rounds: d.rounds,
            learning_rate: d.learning_rate,
            max_depth: d.max_depth,
            min_leaf: d.min_leaf,
            lambda: d.lambda,
            subsample: d.subsample,
            early_stopping: d.early_stopping,
            regression_loss: d.regression_loss,
            seed: d.seed,
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

    fn options(&self) -> BoostOptions {
        BoostOptions {
            rounds: self.rounds,
            learning_rate: self.learning_rate,
            max_depth: self.max_depth,
            min_leaf: self.min_leaf,
            lambda: self.lambda,
            subsample: self.subsample,
            early_stopping: self.early_stopping,
            regression_loss: self.regression_loss,
            seed: self.seed,
            ..BoostOptions::default()
        }
    }
}

/// How the number of rounds was chosen.
#[derive(Debug, Clone, PartialEq)]
enum RoundChoice {
    /// Every round kept (no validation rows).
    All,
    /// Validation rows: the round with the smallest validation loss.
    Validation { stopped_early: bool },
    /// The round with the smallest cross-validated loss (`curve`).
    CrossValidation { curve: Vec<f64>, folds: usize },
}

/// A fitted boosted-tree model.
#[derive(Debug, Clone)]
pub struct BoostAnalysis {
    request: BoostRequest,
    response_name: String,
    profile_name: String,
    response: Response,
    roles: RoleAssignment,
    predictors: Vec<PredictorInfo>,
    features: Vec<Feature>,
    model: Boosted,
    choice: RoundChoice,
    predictions: Predictions,
    held_out: Option<Predictions>,
    gain: Vec<f64>,
    splits: Vec<usize>,
    profile_class: usize,
}

impl BoostAnalysis {
    /// Fit and evaluate the model.
    ///
    /// # Errors
    /// No predictors, the response among them, too few classes, invalid
    /// validation or boosting settings, or no training rows.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &BoostRequest) -> Result<Self> {
        if request.predictors.is_empty() {
            return Err(invalid("choose at least one predictor"));
        }
        if request.predictors.contains(&request.response) {
            return Err(invalid("the response cannot also be a predictor"));
        }
        let included: Vec<usize> = (0..table.n_rows()).filter(|&r| !states.is_excluded(r)).collect();
        let response = Response::read(table, request.response, &included)?;
        let response_name = table.column(request.response).map_or(String::new(), |c| c.name.clone());
        let roles = RoleAssignment::assign(table, states, &response, &request.validation)?;
        let train = roles.rows(Role::Training);
        let valid = roles.rows(Role::Validation);
        let FeatureSet { predictors, features } = FeatureSet::read(table, &request.predictors, &train)?;
        let target = response.target();
        let options = request.options();
        let width = response.classes().map_or(1, <[String]>::len);
        let wrap = |values: Vec<Vec<f64>>| match &response {
            Response::Continuous(_) => Predictions::Values(values.into_iter().map(|v| v[0]).collect()),
            Response::Categorical { .. } => Predictions::Probabilities(values),
        };

        let (model, choice, held_out) = if let Some((folds, seed)) = roles.folds {
            let folds = folds.min(train.len()).max(2);
            let fold_of = fold_assignment(train.len(), folds, seed)?;
            let (curve, best) = cross_validate_rounds(&features, &target, &train, &fold_of, &options)?;
            let fixed = BoostOptions { rounds: best, early_stopping: 0, ..options };
            let model = fit_boosted(&features, &target, &train, &[], &fixed)?;
            let mut values = vec![vec![f64::NAN; width]; table.n_rows()];
            for f in 0..folds {
                let fit_rows: Vec<usize> =
                    train.iter().zip(&fold_of).filter(|(_, k)| **k != f).map(|(&r, _)| r).collect();
                if fit_rows.is_empty() {
                    continue;
                }
                let m = fit_boosted(&features, &target, &fit_rows, &[], &fixed)?;
                for (&r, _) in train.iter().zip(&fold_of).filter(|(_, k)| **k == f) {
                    values[r] = m.predict(&|j| features[j].value(r));
                }
            }
            (model, RoundChoice::CrossValidation { curve, folds }, Some(wrap(values)))
        } else if valid.is_empty() {
            let all = BoostOptions { early_stopping: 0, ..options };
            (fit_boosted(&features, &target, &train, &[], &all)?, RoundChoice::All, None)
        } else {
            let model = fit_boosted(&features, &target, &train, &valid, &options)?;
            let stopped_early = model.stopped_early;
            (model, RoundChoice::Validation { stopped_early }, None)
        };
        let predictions =
            wrap((0..table.n_rows()).map(|r| model.predict(&|j| features[j].value(r))).collect::<Vec<_>>());
        let gain = model.gain_importance(features.len());
        let splits = model.split_counts(features.len());
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
            model,
            choice,
            predictions,
            held_out,
            gain,
            splits,
            profile_class,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &BoostRequest {
        &self.request
    }

    /// The fitted model.
    #[must_use]
    pub fn model(&self) -> &Boosted {
        &self.model
    }

    /// Total split gain of each predictor, in request order.
    #[must_use]
    pub fn gain_importance(&self) -> &[f64] {
        &self.gain
    }

    fn is_classification(&self) -> bool {
        self.response.classes().is_some()
    }

    fn loss_name(&self) -> &'static str {
        match self.model.loss {
            BoostLoss::Squared => "squared error",
            BoostLoss::Absolute => "absolute error",
            BoostLoss::Logistic => "logistic",
            BoostLoss::Multinomial(_) => "multinomial",
        }
    }

    fn loss_label(&self) -> &'static str {
        match self.model.loss {
            BoostLoss::Squared => "Half squared error",
            BoostLoss::Absolute => "Absolute error",
            BoostLoss::Logistic | BoostLoss::Multinomial(_) => "Log loss",
        }
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let names: Vec<&str> = self.predictors.iter().map(|p| p.name.as_str()).collect();
        format!("Boosted Trees: {} ~ {}", self.response_name, names.join(" + "))
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary_section(), fit_measures_section(self)];
        out.extend(confusion_section(self));
        out.push(self.importance_section());
        out.push(self.loss_plot());
        out
    }

    fn summary_section(&self) -> ReportSection {
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut row = |a: &str, b: String| t.push([a.to_owned(), b]);
        row("Response", self.response_name.clone());
        row(
            "Model",
            match self.response.classes() {
                Some(c) if c.len() == 2 => {
                    format!("{} loss; one tree per round for the log-odds of {}", self.loss_name(), c[1])
                }
                Some(c) => format!("{} loss; one tree per class per round ({} classes)", self.loss_name(), c.len()),
                None => format!("regression, {} loss", self.loss_name()),
            },
        );
        for role in Role::ALL {
            let n = self.roles.rows(role).len();
            if n > 0 || role == Role::Training {
                row(&format!("{} rows", role.label()), n.to_string());
            }
        }
        row(
            "Rounds kept",
            match &self.choice {
                RoundChoice::All => format!("{} (every round; no validation rows)", self.model.best_rounds),
                RoundChoice::Validation { stopped_early } => format!(
                    "{} of {} (smallest validation loss{})",
                    self.model.best_rounds,
                    self.model.training_curve.len(),
                    if *stopped_early {
                        format!("; stopped after {} rounds without improvement", self.request.early_stopping)
                    } else {
                        String::new()
                    }
                ),
                RoundChoice::CrossValidation { folds, .. } => format!(
                    "{} of {} (smallest {folds}-fold cross-validated loss)",
                    self.model.best_rounds, self.request.rounds
                ),
            },
        );
        let trees: usize = self.model.trees.iter().map(Vec::len).sum();
        let leaves: usize = self.model.trees.iter().flatten().map(|t| t.leaves()).sum();
        row("Trees", trees.to_string());
        row("Mean leaves per tree", format_number(leaves as f64 / trees.max(1) as f64, 4));
        row(
            "Settings",
            format!(
                "learning rate {}; depth up to {}; at least {} rows per leaf; L2 penalty {}; {}% of the \
                 training rows per round; seed {}",
                format_number(self.request.learning_rate, 4),
                self.request.max_depth,
                self.request.min_leaf,
                format_number(self.request.lambda, 4),
                format_number(100.0 * self.request.subsample, 3),
                self.request.seed
            ),
        );
        ReportSection {
            kind: SectionKind::BoostSummary,
            title: "Boosting Summary".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    fn importance_section(&self) -> ReportSection {
        let mut order: Vec<usize> = (0..self.predictors.len()).collect();
        order.sort_by(|&a, &b| self.gain[b].total_cmp(&self.gain[a]));
        let total: f64 = self.gain.iter().sum();
        let mut t = TextTable::new(["Predictor", "Gain", "Share", "Splits"]);
        for j in order {
            let share = if total > 0.0 { 100.0 * self.gain[j] / total } else { 0.0 };
            t.push([
                self.predictors[j].name.clone(),
                format_number(self.gain[j], DIGITS),
                format!("{}%", format_number(share, 3)),
                self.splits[j].to_string(),
            ]);
        }
        ReportSection {
            kind: SectionKind::VariableImportance,
            title: "Variable Importance (total loss reduction of each predictor's splits)".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    /// Training and validation (or cross-validated) loss by round, with a
    /// marker at the end of each curve and at the round kept.
    fn loss_plot(&self) -> ReportSection {
        let as_line = |v: &[f64]| -> Vec<[f64; 2]> {
            v.iter().enumerate().filter(|(_, e)| e.is_finite()).map(|(k, &e)| [(k + 1) as f64, e]).collect()
        };
        let target = self.response.target();
        let (training, held, held_name) = match &self.choice {
            RoundChoice::All => (as_line(&self.model.training_curve), Vec::new(), ""),
            RoundChoice::Validation { .. } => {
                (as_line(&self.model.training_curve), as_line(&self.model.validation_curve), "Validation")
            }
            RoundChoice::CrossValidation { curve, .. } => {
                let train = self.roles.rows(Role::Training);
                (as_line(&loss_curve(&self.model, &self.features, &target, &train)), as_line(curve), "Cross-validation")
            }
        };
        let mut labels = vec!["Training".to_owned()];
        let mut lines = vec![training];
        if !held.is_empty() {
            labels.push(held_name.to_owned());
            lines.push(held);
        }
        let mut items = Vec::new();
        for (g, line) in lines.iter().enumerate() {
            if let Some(&[x, y]) = line.last() {
                items.push((x, y, labels[g].clone(), g));
            }
        }
        let best = self.model.best_rounds as f64;
        if lines.len() > 1 {
            if let Some(&[x, y]) = lines[1].iter().find(|p| p[0] == best) {
                items.push((x, y, format!("kept: {} rounds", self.model.best_rounds), 1));
            }
        }
        let spec = PlotSpec {
            x_label: "Round".to_owned(),
            y_label: self.loss_label().to_owned(),
            groups: Some(PointGroups {
                title: "Loss".to_owned(),
                labels: labels.clone(),
                membership: Vec::new(),
                lines,
            }),
            annotations: Some(Annotations { groups: labels, items, labels: Vec::new() }),
            ..PlotSpec::default()
        };
        ReportSection {
            kind: SectionKind::BoostLoss,
            title: format!("{} by Round", self.loss_label()),
            content: SectionContent::Plot(Box::new(spec)),
        }
    }

    /// Columns the report can save: predicted values, or class probabilities
    /// and the most likely class (as values), and the validation roles.
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<crate::ReportColumn> {
        prediction_columns(self, &PredictionFormulas::default())
    }
}

impl Predictor for BoostAnalysis {
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

impl SurfaceModel for BoostAnalysis {
    // The profilers show the class probability, e.g. "P(Churn = Yes)".
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
                let p = self.model.predict(&|j| setting_value(&self.features[j], &s[j]));
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
            "Sum of {} trees, so predictions change in steps; no confidence bands.{}",
            self.model.trees.iter().map(Vec::len).sum::<usize>(),
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
    request: &BoostRequest,
    col: &dyn Fn(usize) -> JsonValue,
) -> Vec<(&'static str, JsonValue)> {
    let n = |x: usize| JsonValue::Number(x as f64);
    vec![
        ("validation", request.validation.to_json(col)),
        ("rounds", n(request.rounds)),
        ("learning_rate", JsonValue::number(request.learning_rate)),
        ("max_depth", n(request.max_depth)),
        ("min_leaf", n(request.min_leaf)),
        ("lambda", JsonValue::number(request.lambda)),
        ("subsample", JsonValue::number(request.subsample)),
        ("early_stopping", n(request.early_stopping)),
        (
            "loss",
            JsonValue::String(
                if request.regression_loss == RegressionLoss::Absolute { "absolute" } else { "squared" }.to_owned(),
            ),
        ),
        ("seed", JsonValue::Number(request.seed as f64)),
    ]
}

/// Read a request from a project file's report entry (missing members take
/// their defaults).
///
/// # Errors
/// An unknown loss or validation method, a missing column, or a learning
/// rate or subsample share outside (0, 1].
pub fn request_from_json(
    v: &JsonValue,
    response: usize,
    predictors: Vec<usize>,
    find: &dyn Fn(&JsonValue) -> Result<usize>,
) -> Result<BoostRequest> {
    let mut r = BoostRequest::new(response, predictors);
    r.validation = ValidationMethod::from_json(v.get("validation"), find)?;
    let size = |k: &str| v.get(k).and_then(JsonValue::as_usize);
    r.rounds = size("rounds").unwrap_or(r.rounds);
    r.max_depth = size("max_depth").unwrap_or(r.max_depth);
    r.min_leaf = size("min_leaf").unwrap_or(r.min_leaf);
    r.early_stopping = size("early_stopping").unwrap_or(r.early_stopping);
    r.seed = v.get("seed").and_then(crate::seed::seed_from_json).unwrap_or(r.seed);
    for (key, slot) in [("learning_rate", &mut r.learning_rate), ("subsample", &mut r.subsample)] {
        if let Some(x) = v.get(key).and_then(JsonValue::as_f64) {
            if !(x > 0.0 && x <= 1.0) {
                return Err(invalid(format!("boosted trees: {key} must be in (0, 1]")));
            }
            *slot = x;
        }
    }
    if let Some(l) = v.get("lambda").and_then(JsonValue::as_f64) {
        if l.is_nan() || l < 0.0 {
            return Err(invalid("boosted trees: lambda must not be negative"));
        }
        r.lambda = l;
    }
    if let Some(c) = v.get("loss").and_then(JsonValue::as_str) {
        r.regression_loss = match c {
            "squared" => RegressionLoss::Squared,
            "absolute" => RegressionLoss::Absolute,
            other => return Err(invalid(format!("boosted trees: unknown loss {other:?}"))),
        };
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loans() -> (DataTable, Vec<usize>, usize, usize) {
        let t = crate::sample::loans();
        let c = |n: &str| t.find(n).unwrap();
        let predictors =
            vec![c("Credit history"), c("Employment"), c("Age"), c("Income (k)"), c("Debt ratio"), c("Loan amount")];
        let (outcome, rate) = (c("Outcome"), c("Interest rate (%)"));
        (t, predictors, outcome, rate)
    }

    #[test]
    fn classification_with_early_stopping() {
        let (t, predictors, outcome, _) = loans();
        let mut req = BoostRequest::new(outcome, predictors);
        req.validation = ValidationMethod::Column(t.find("Validation").unwrap());
        let rows = RowStateManager::new(t.n_rows());
        let a = BoostAnalysis::run(&t, &rows, &req).unwrap();
        assert!(a.title().starts_with("Boosted Trees: Outcome ~ Credit history"));
        let s = a.sections();
        let kinds: Vec<SectionKind> = s.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::BoostSummary,
                SectionKind::FitByRole,
                SectionKind::ConfusionMatrix,
                SectionKind::VariableImportance,
                SectionKind::BoostLoss,
            ]
        );
        assert_eq!(a.model().loss, BoostLoss::Multinomial(3));
        assert!(a.model().best_rounds < 300, "{}", a.model().best_rounds);
        let SectionContent::Table(fit) = &s[1].content else { panic!() };
        let labels: Vec<&str> = fit.rows.iter().map(|r| r[0].as_str()).collect();
        assert_eq!(labels, ["Training", "Validation", "Test"]);
        let err = |row: &Vec<String>| row[2].parse::<f64>().unwrap();
        assert!(err(&fit.rows[2]) < 0.35, "test misclassification {}", err(&fit.rows[2]));
        // Credit history and debt ratio carry the most gain.
        let g = a.gain_importance();
        let mut order: Vec<usize> = (0..g.len()).collect();
        order.sort_by(|&x, &y| g[y].total_cmp(&g[x]));
        assert!(order[..2].contains(&0) && order[..2].contains(&4), "{g:?}");
        // The loss plot: training and validation curves, the kept round marked.
        let SectionContent::Plot(p) = &s[4].content else { panic!() };
        let groups = p.groups.as_ref().unwrap();
        assert_eq!(groups.labels, ["Training", "Validation"]);
        assert_eq!(groups.lines[1].len(), a.model().validation_curve.len());
        assert!(p.annotations.as_ref().unwrap().items.iter().any(|i| i.2.starts_with("kept")));
        let keys: Vec<String> = a.saveable_columns().into_iter().map(|c| c.key).collect();
        assert_eq!(keys, ["probability_1", "probability_2", "probability_3", "most_likely", "validation"]);
        // Profiler: a poor history lowers the chance of repaying.
        let mut good = a.default_profile_settings();
        good[0] = TermValue::Level(1);
        let mut poor = good.clone();
        poor[0] = TermValue::Level(2);
        let p = a.predict_settings(&[good, poor]).unwrap();
        assert!(p[0] > p[1] + 0.2, "{p:?}");
    }

    #[test]
    fn regression_with_cross_validation_and_json() {
        let (t, predictors, _, rate) = loans();
        let mut req = BoostRequest::new(rate, predictors);
        req.rounds = 120;
        req.validation = ValidationMethod::KFold { folds: 3, seed: 2 };
        let rows = RowStateManager::new(t.n_rows());
        let a = BoostAnalysis::run(&t, &rows, &req).unwrap();
        let s = a.sections();
        let SectionContent::Table(fit) = &s[1].content else { panic!() };
        assert_eq!(fit.rows.iter().map(|r| r[0].as_str()).collect::<Vec<_>>(), ["Training", "Cross-validation"]);
        let r2 = fit.rows[1][4].parse::<f64>().unwrap();
        assert!(r2 > 0.7, "cross-validated R² {r2}");
        assert_eq!(s.len(), 4, "no confusion matrix for regression");
        let SectionContent::Plot(p) = &s[3].content else { panic!() };
        let groups = p.groups.as_ref().unwrap();
        assert_eq!(groups.lines[1].len(), 120);
        assert_eq!(groups.lines[0].len(), a.model().best_rounds);
        // JSON round trip.
        req.regression_loss = RegressionLoss::Absolute;
        req.learning_rate = 0.05;
        let col = |i: usize| JsonValue::String(t.column(i).unwrap().name.clone());
        let members = request_members_to_json(&req, &col);
        let v = JsonValue::Object(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect());
        let find = |c: &JsonValue| Ok(t.find(c.as_str().unwrap()).unwrap());
        let back = request_from_json(&v, req.response, req.predictors.clone(), &find).unwrap();
        assert_eq!(back, req);
        let bad = JsonValue::Object(vec![("learning_rate".to_owned(), JsonValue::number(1.5))]);
        assert!(request_from_json(&bad, 0, vec![1], &find).is_err());
    }
}
