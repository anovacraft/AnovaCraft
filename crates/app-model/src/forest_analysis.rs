//! Random forests bound to a dataset ([`stat_predictive::forest`]): many
//! decision trees grown on bootstrap samples of the training rows, each
//! split chosen from a random subset of the predictors, their predictions
//! averaged. Uses the shared predictive-model sections ([`crate::prediction`]).
//!
//! **Rows.** As for decision trees: excluded rows and rows missing the
//! response are left out; missing predictor values are routed by surrogate
//! splits. Training rows grow the forest; validation and test rows only
//! measure it (a forest has no size to tune). Out-of-bag predictions give an
//! honest error estimate from the training rows themselves, shown as its own
//! row of the fit table; with k-fold validation the table shows the
//! cross-validated predictions instead (one forest per fold).
//!
//! **Importance.** Permutation importance (increase in out-of-bag error when
//! a predictor is shuffled) with its standard error, and the mean impurity
//! reduction per tree.

use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_linear::TermValue;
use stat_predictive::{Criterion, Feature, Forest, ForestOptions, Role, fit_forest, fold_assignment};

use crate::format::format_number;
use crate::prediction::{
    FeatureSet, PredictionFormulas, Predictions, Predictor, PredictorInfo, Response, RoleAssignment, ValidationMethod,
    confusion_section, fit_measures_section, prediction_columns, setting_value,
};
use crate::report::{PlotSpec, ReportSection, SectionContent, SectionKind, TextTable};
use crate::surface_profiler::{Observation, ProfileFactor, SurfaceModel};

const DIGITS: usize = 6;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Columns and settings of a random forest.
#[derive(Debug, Clone, PartialEq)]
pub struct ForestRequest {
    /// Response column.
    pub response: usize,
    /// Predictor columns.
    pub predictors: Vec<usize>,
    /// Training / validation / test roles.
    pub validation: ValidationMethod,
    /// Number of trees.
    pub trees: usize,
    /// Predictors tried at each split (0: √p for classification, p/3 for
    /// regression).
    pub features_per_split: usize,
    /// Bootstrap sample size as a share of the training rows.
    pub sample_fraction: f64,
    /// Smallest node that may be split.
    pub min_split: usize,
    /// Fewest rows on each side of a split.
    pub min_leaf: usize,
    /// Greatest depth.
    pub max_depth: usize,
    /// Impurity of classification trees.
    pub criterion: Criterion,
    /// Seed of the bootstrap samples and predictor draws.
    pub seed: u64,
}

impl ForestRequest {
    /// Default settings ([`ForestOptions::default`]) with every row training.
    #[must_use]
    pub fn new(response: usize, predictors: Vec<usize>) -> Self {
        let d = ForestOptions::default();
        Self {
            response,
            predictors,
            validation: ValidationMethod::None,
            trees: d.trees,
            features_per_split: d.features_per_split,
            sample_fraction: d.sample_fraction,
            min_split: d.min_split,
            min_leaf: d.min_leaf,
            max_depth: d.max_depth,
            criterion: d.criterion,
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

    fn options(&self) -> ForestOptions {
        ForestOptions {
            trees: self.trees,
            features_per_split: self.features_per_split,
            sample_fraction: self.sample_fraction,
            min_split: self.min_split,
            min_leaf: self.min_leaf,
            max_depth: self.max_depth,
            criterion: self.criterion,
            seed: self.seed,
            ..ForestOptions::default()
        }
    }
}

/// A fitted random forest.
#[derive(Debug, Clone)]
pub struct ForestAnalysis {
    request: ForestRequest,
    response_name: String,
    profile_name: String,
    response: Response,
    roles: RoleAssignment,
    predictors: Vec<PredictorInfo>,
    features: Vec<Feature>,
    forest: Forest,
    predictions: Predictions,
    held_out: Option<Predictions>,
    held_out_label: &'static str,
    permutation: Vec<(f64, f64)>,
    impurity: Vec<f64>,
    profile_class: usize,
}

impl ForestAnalysis {
    /// Grow and evaluate the forest.
    ///
    /// # Errors
    /// No predictors, the response among them, too few classes, invalid
    /// validation or forest settings, or no training rows.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &ForestRequest) -> Result<Self> {
        if request.predictors.is_empty() {
            return Err(invalid("choose at least one predictor"));
        }
        if request.predictors.contains(&request.response) {
            return Err(invalid("the response cannot also be a predictor"));
        }
        if request.min_leaf == 0 || request.max_depth == 0 {
            return Err(invalid("leaves need at least one row and trees at least one level"));
        }
        let included: Vec<usize> = (0..table.n_rows()).filter(|&r| !states.is_excluded(r)).collect();
        let response = Response::read(table, request.response, &included)?;
        let response_name = table.column(request.response).map_or(String::new(), |c| c.name.clone());
        let roles = RoleAssignment::assign(table, states, &response, &request.validation)?;
        let train = roles.rows(Role::Training);
        let FeatureSet { predictors, features } = FeatureSet::read(table, &request.predictors, &train)?;
        let target = response.target();
        let options = request.options();
        let forest = fit_forest(&features, &target, &train, &options)?;

        let wrap = |values: Vec<Vec<f64>>| match &response {
            Response::Continuous(_) => Predictions::Values(values.into_iter().map(|v| v[0]).collect()),
            Response::Categorical { .. } => Predictions::Probabilities(values),
        };
        let width = response.classes().map_or(1, <[String]>::len);
        let predictions =
            wrap((0..table.n_rows()).map(|r| forest.predict(&|j| features[j].value(r))).collect::<Vec<_>>());
        let (held_out, held_out_label) = if let Some((folds, seed)) = roles.folds {
            let folds = folds.min(train.len()).max(2);
            let fold_of = fold_assignment(train.len(), folds, seed)?;
            let mut values = vec![vec![f64::NAN; width]; table.n_rows()];
            for f in 0..folds {
                let fit_rows: Vec<usize> =
                    train.iter().zip(&fold_of).filter(|(_, k)| **k != f).map(|(&r, _)| r).collect();
                // A fold whose remaining rows cannot grow a forest (too few
                // rows, or a single class) leaves its held-out rows missing.
                let Ok(model) = fit_forest(&features, &target, &fit_rows, &options) else {
                    continue;
                };
                for (&r, _) in train.iter().zip(&fold_of).filter(|(_, k)| **k == f) {
                    values[r] = model.predict(&|j| features[j].value(r));
                }
            }
            (Some(wrap(values)), "Cross-validation")
        } else {
            let values = (0..table.n_rows())
                .map(|r| forest.oob_predictions[r].clone().unwrap_or_else(|| vec![f64::NAN; width]))
                .collect();
            (Some(wrap(values)), "Out-of-bag")
        };
        let permutation = forest.permutation_importance(&features, &target, request.seed);
        let impurity = forest.impurity_importance(features.len());
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
            forest,
            predictions,
            held_out,
            held_out_label,
            permutation,
            impurity,
            profile_class,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &ForestRequest {
        &self.request
    }

    /// The fitted forest.
    #[must_use]
    pub fn forest(&self) -> &Forest {
        &self.forest
    }

    /// Permutation importance (mean increase in out-of-bag error, standard
    /// error) of each predictor, in request order.
    #[must_use]
    pub fn permutation_importance(&self) -> &[(f64, f64)] {
        &self.permutation
    }

    fn is_classification(&self) -> bool {
        self.response.classes().is_some()
    }

    fn error_name(&self) -> &'static str {
        if self.is_classification() { "Misclassification rate" } else { "Mean squared error" }
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let names: Vec<&str> = self.predictors.iter().map(|p| p.name.as_str()).collect();
        format!("Random forest: {} ~ {}", self.response_name, names.join(" + "))
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary_section(), fit_measures_section(self)];
        out.extend(confusion_section(self));
        out.push(self.importance_section());
        out.push(self.oob_plot());
        out
    }

    fn summary_section(&self) -> ReportSection {
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut row = |a: &str, b: String| t.push([a.to_owned(), b]);
        row("Response", self.response_name.clone());
        row(
            "Forest",
            match self.response.classes() {
                Some(c) => format!(
                    "classification, {} classes, {} impurity; class probabilities averaged over trees",
                    c.len(),
                    if self.request.criterion == Criterion::Gini { "Gini" } else { "entropy" }
                ),
                None => "regression (squared error); tree means averaged".to_owned(),
            },
        );
        for role in Role::ALL {
            let n = self.roles.rows(role).len();
            if n > 0 || role == Role::Training {
                row(&format!("{} rows", role.label()), n.to_string());
            }
        }
        row("Trees", self.forest.trees.len().to_string());
        row("Predictors tried per split", format!("{} of {}", self.forest.features_per_split, self.predictors.len()));
        row(
            "Bootstrap sample",
            format!(
                "{}% of the training rows, drawn with replacement",
                format_number(100.0 * self.request.sample_fraction, 3)
            ),
        );
        let leaves: usize = self.forest.trees.iter().map(|t| t.leaves(-1.0).len()).sum();
        row("Mean leaves per tree", format_number(leaves as f64 / self.forest.trees.len().max(1) as f64, 4));
        if let Some(&e) = self.forest.oob_error_curve.last() {
            row(&format!("Out-of-bag {}", self.error_name().to_lowercase()), format_number(e, DIGITS));
        }
        row(
            "Growing limits",
            format!(
                "split nodes of {} or more rows; at least {} rows per side; depth up to {}; seed {}",
                self.request.min_split, self.request.min_leaf, self.request.max_depth, self.request.seed
            ),
        );
        ReportSection {
            kind: SectionKind::ForestSummary,
            title: "Forest Summary".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    fn importance_section(&self) -> ReportSection {
        let mut order: Vec<usize> = (0..self.predictors.len()).collect();
        order.sort_by(|&a, &b| self.permutation[b].0.total_cmp(&self.permutation[a].0));
        let total: f64 = self.impurity.iter().sum();
        let unit = if self.is_classification() { "misclassification" } else { "MSE" };
        let mut t = TextTable::new(vec![
            "Predictor".to_owned(),
            format!("Permutation (increase in OOB {unit})"),
            "Standard error".to_owned(),
            "Impurity reduction per tree".to_owned(),
            "Impurity share".to_owned(),
        ]);
        for j in order {
            let (m, se) = self.permutation[j];
            let share = if total > 0.0 { 100.0 * self.impurity[j] / total } else { 0.0 };
            t.push([
                self.predictors[j].name.clone(),
                format_number(m, DIGITS),
                format_number(se, 4),
                format_number(self.impurity[j], DIGITS),
                format!("{}%", format_number(share, 3)),
            ]);
        }
        ReportSection {
            kind: SectionKind::VariableImportance,
            title: "Variable Importance (sorted by permutation importance)".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    fn oob_plot(&self) -> ReportSection {
        let curve: Vec<[f64; 2]> = self
            .forest
            .oob_error_curve
            .iter()
            .enumerate()
            .filter(|(_, e)| e.is_finite())
            .map(|(k, &e)| [(k + 1) as f64, e])
            .collect();
        let spec = PlotSpec {
            x_label: "Trees".to_owned(),
            y_label: format!("Out-of-bag {}", self.error_name().to_lowercase()),
            line: Some(curve),
            line_name: Some("Out-of-bag error".to_owned()),
            ..PlotSpec::default()
        };
        ReportSection {
            kind: SectionKind::OobError,
            title: "Out-of-Bag Error by Number of Trees".to_owned(),
            content: SectionContent::Plot(Box::new(spec)),
        }
    }

    /// Columns the report can save: predicted values, or class probabilities
    /// and the most likely class (as values: a forest is too large to write
    /// as a formula), and the validation roles.
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<crate::ReportColumn> {
        prediction_columns(self, &PredictionFormulas::default())
    }
}

impl Predictor for ForestAnalysis {
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

    fn held_out_label(&self) -> &'static str {
        self.held_out_label
    }
}

impl SurfaceModel for ForestAnalysis {
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
                let p = self.forest.predict(&|j| setting_value(&self.features[j], &s[j]));
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
            "Average of {} trees, so predictions change in many small steps; no confidence bands.{}",
            self.forest.trees.len(),
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
    request: &ForestRequest,
    col: &dyn Fn(usize) -> JsonValue,
) -> Vec<(&'static str, JsonValue)> {
    let n = |x: usize| JsonValue::Number(x as f64);
    vec![
        ("validation", request.validation.to_json(col)),
        ("trees", n(request.trees)),
        ("features_per_split", n(request.features_per_split)),
        ("sample_fraction", JsonValue::number(request.sample_fraction)),
        ("min_split", n(request.min_split)),
        ("min_leaf", n(request.min_leaf)),
        ("max_depth", n(request.max_depth)),
        (
            "criterion",
            JsonValue::String(if request.criterion == Criterion::Gini { "gini" } else { "entropy" }.to_owned()),
        ),
        ("seed", JsonValue::Number(request.seed as f64)),
    ]
}

/// Read a request from a project file's report entry (missing members take
/// their defaults).
///
/// # Errors
/// An unknown criterion or validation method, a missing column, or a sample
/// fraction outside (0, 1].
pub fn request_from_json(
    v: &JsonValue,
    response: usize,
    predictors: Vec<usize>,
    find: &dyn Fn(&JsonValue) -> Result<usize>,
) -> Result<ForestRequest> {
    let mut r = ForestRequest::new(response, predictors);
    r.validation = ValidationMethod::from_json(v.get("validation"), find)?;
    let size = |k: &str| v.get(k).and_then(JsonValue::as_usize);
    r.trees = size("trees").unwrap_or(r.trees);
    r.features_per_split = size("features_per_split").unwrap_or(r.features_per_split);
    r.min_split = size("min_split").unwrap_or(r.min_split);
    r.min_leaf = size("min_leaf").unwrap_or(r.min_leaf);
    r.max_depth = size("max_depth").unwrap_or(r.max_depth);
    r.seed = v.get("seed").and_then(crate::seed::seed_from_json).unwrap_or(r.seed);
    if let Some(f) = v.get("sample_fraction").and_then(JsonValue::as_f64) {
        if !(f > 0.0 && f <= 1.0) {
            return Err(invalid("random forest: the sample fraction must be in (0, 1]"));
        }
        r.sample_fraction = f;
    }
    if let Some(c) = v.get("criterion").and_then(JsonValue::as_str) {
        r.criterion = match c {
            "gini" => Criterion::Gini,
            "entropy" => Criterion::Entropy,
            other => return Err(invalid(format!("random forest: unknown criterion {other:?}"))),
        };
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::catalog;

    fn loans() -> (DataTable, Vec<usize>, usize, usize) {
        let t = crate::sample::loans();
        let c = |n: &str| t.find(n).unwrap();
        let predictors =
            vec![c("Credit history"), c("Employment"), c("Age"), c("Income (k)"), c("Debt ratio"), c("Loan amount")];
        let (outcome, rate) = (c("Outcome"), c("Interest rate (%)"));
        (t, predictors, outcome, rate)
    }

    #[test]
    fn classification_forest_report() {
        let (t, predictors, outcome, _) = loans();
        let mut req = ForestRequest::new(outcome, predictors);
        req.trees = 60;
        req.validation =
            ValidationMethod::Holdout { validation: 0.0, test: 0.25, seed: 3, stratify: true, group: None };
        let rows = RowStateManager::new(t.n_rows());
        let a = ForestAnalysis::run(&t, &rows, &req).unwrap();
        assert!(a.title().starts_with("Random forest: Outcome ~ Credit history"));
        let kinds: Vec<SectionKind> = a.sections().iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::ForestSummary,
                SectionKind::FitByRole,
                SectionKind::ConfusionMatrix,
                SectionKind::VariableImportance,
                SectionKind::OobError,
            ]
        );
        let s = a.sections();
        let SectionContent::Table(fit) = &s[1].content else { panic!() };
        let labels: Vec<&str> = fit.rows.iter().map(|r| r[0].as_str()).collect();
        assert_eq!(labels, ["Training", "Test", "Out-of-bag"]);
        // Credit history and debt ratio drive the simulated outcome.
        let perm = a.permutation_importance();
        let top = (0..perm.len()).max_by(|&x, &y| perm[x].0.total_cmp(&perm[y].0)).unwrap();
        assert_eq!(top, 0, "{perm:?}");
        assert!(perm[4].0 > perm[2].0 && perm[4].0 > perm[5].0, "{perm:?}");
        // Test error is close to the out-of-bag error, and both beat chance.
        let err = |row: &Vec<String>| row[2].parse::<f64>().unwrap();
        let (test, oob) = (err(&fit.rows[1]), err(&fit.rows[2]));
        assert!(test < 0.35 && oob < 0.35, "{test} {oob}");
        // Saved columns are values (no formulas).
        let cols = a.saveable_columns();
        let keys: Vec<&str> = cols.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys, ["probability_1", "probability_2", "probability_3", "most_likely", "validation"]);
        assert!(cols.iter().all(|c| c.formula.is_none()));
        // Profiler: a poor history raises the default probability.
        assert_eq!(SurfaceModel::response_name(&a), "P(Outcome = Repaid)");
        let mut good = a.default_profile_settings();
        good[0] = TermValue::Level(1);
        let mut poor = good.clone();
        poor[0] = TermValue::Level(2);
        let p = a.predict_settings(&[good, poor]).unwrap();
        assert!(p[0] > p[1] + 0.2, "{p:?}");
    }

    #[test]
    fn regression_forest_with_cross_validation_and_json() {
        let (t, predictors, _, rate) = loans();
        let mut req = ForestRequest::new(rate, predictors);
        req.trees = 30;
        req.validation = ValidationMethod::KFold { folds: 3, seed: 1 };
        let rows = RowStateManager::new(t.n_rows());
        let a = ForestAnalysis::run(&t, &rows, &req).unwrap();
        let s = a.sections();
        let SectionContent::Table(fit) = &s[1].content else { panic!() };
        assert_eq!(fit.rows.iter().map(|r| r[0].as_str()).collect::<Vec<_>>(), ["Training", "Cross-validation"]);
        let r2 = fit.rows[1][4].parse::<f64>().unwrap();
        assert!(r2 > 0.6, "cross-validated R² {r2}");
        assert_eq!(s.len(), 4, "no confusion matrix for regression");
        let SectionContent::Plot(p) = &s[3].content else { panic!() };
        assert_eq!(p.line.as_ref().unwrap().len(), 30);
        // JSON round trip.
        let col = |i: usize| JsonValue::String(t.column(i).unwrap().name.clone());
        let mut members = vec![("response".to_owned(), col(req.response))];
        members.extend(request_members_to_json(&req, &col).into_iter().map(|(k, v)| (k.to_owned(), v)));
        let v = JsonValue::Object(members);
        let find = |v: &JsonValue| t.find(v.as_str().unwrap_or_default()).ok_or_else(|| invalid("no column"));
        assert_eq!(request_from_json(&v, req.response, req.predictors.clone(), &find).unwrap(), req);
        assert!(catalog().iter().any(|i| i.name.starts_with("Loan applications")));
    }

    #[test]
    fn small_training_sets_still_cross_validate() {
        // Three training rows, ten folds asked for: the folds drop to three
        // (one row each) and every row gets a held-out prediction, even from
        // a fold whose two fitting rows share one class.
        let (t, predictors, outcome, rate) = loans();
        let codes: Vec<String> = (0..t.n_rows()).map(|r| t.column(outcome).unwrap().level_text(r)).collect();
        let other = codes.iter().position(|c| c != &codes[0]).unwrap();
        let keep = [0, other, (1..t.n_rows()).find(|&r| r != other && codes[r] == codes[0]).unwrap()];
        let mut rows = RowStateManager::new(t.n_rows());
        rows.set_excluded((0..t.n_rows()).filter(|r| !keep.contains(r)), true);
        for response in [outcome, rate] {
            let mut req = ForestRequest::new(response, predictors.clone());
            req.trees = 10;
            req.validation = ValidationMethod::KFold { folds: 10, seed: 4 };
            let a = ForestAnalysis::run(&t, &rows, &req).unwrap();
            assert_eq!(a.held_out_label, "Cross-validation");
            let finite = |r: usize| match &a.held_out {
                Some(Predictions::Values(v)) => v[r].is_finite(),
                Some(Predictions::Probabilities(p)) => p[r].iter().all(|x| x.is_finite()),
                None => false,
            };
            assert!(keep.iter().all(|&r| finite(r)), "every training row is predicted");
        }
        // One fold asked for is raised to two instead of failing.
        let mut req = ForestRequest::new(rate, predictors);
        req.trees = 10;
        req.validation = ValidationMethod::KFold { folds: 1, seed: 4 };
        assert!(ForestAnalysis::run(&t, &RowStateManager::new(t.n_rows()), &req).is_ok());
    }
}
