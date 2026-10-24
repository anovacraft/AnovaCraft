//! The shared framework for predictive models (decision trees now; forests,
//! boosting and the others of the roadmap later): which rows train, tune and
//! test a model ([`ValidationMethod`], [`RoleAssignment`]), the response as a
//! model sees it ([`Response`]), predictions for every row
//! ([`Predictions`]), and the report sections and saved columns every
//! predictive model shares ([`fit_measures_section`],
//! [`confusion_section`], [`prediction_columns`]) through the [`Predictor`]
//! interface. Fitted predictors also implement
//! [`crate::surface_profiler::SurfaceModel`], so they get the Response
//! Explorer, the 3-D profiler and several-responses desirability.
//!
//! Fit measures are those of [`stat_predictive::measures`], reported for
//! each role: training rows show how well the model fits the data it learned
//! from (optimistic), validation rows the data that chose its size, and test
//! rows (used for neither) an honest estimate for new data.

use stat_core::{Result, StatError};
use stat_data::{ColumnKind, DataTable, JsonValue, MeasurementLevel, RowStateManager, natural_order};
use stat_linear::TermValue;
use stat_predictive::measures::{ClassFit, ContinuousFit, most_probable};
use stat_predictive::{Feature, Role, Target, Value, grouped_roles, random_roles, stratified_roles};

use crate::format::format_number;
use crate::report::{ReportSection, SectionContent, SectionKind, TextTable};
use crate::report_columns::ReportColumn;
use crate::surface_profiler::{Observation, ProfileFactor, ProfileFactorKind};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Model inputs of a row (support vector machines, neural networks): each
/// continuous predictor standardized, `(x − centre) / scale` with the
/// training rows' mean and standard deviation, and each level of a
/// categorical predictor a 0/1 indicator.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InputEncoding {
    /// Per predictor: `Some((centre, scale))` for a continuous one, `None`
    /// for a categorical one (its levels become indicators).
    pub(crate) numeric: Vec<Option<(f64, f64)>>,
    /// Inputs per predictor.
    pub(crate) width: Vec<usize>,
}

impl InputEncoding {
    pub(crate) fn new(features: &[Feature], train: &[usize]) -> Self {
        let mut numeric = Vec::new();
        let mut width = Vec::new();
        for f in features {
            match f {
                Feature::Numeric(v) => {
                    let x: Vec<f64> = train.iter().map(|&r| v[r]).filter(|x| x.is_finite()).collect();
                    let n = x.len().max(1) as f64;
                    let mean = x.iter().sum::<f64>() / n;
                    let sd = (x.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0).max(1.0)).sqrt();
                    numeric.push(Some((mean, if sd > 0.0 && sd.is_finite() { sd } else { 1.0 })));
                    width.push(1);
                }
                Feature::Categorical { levels, .. } => {
                    numeric.push(None);
                    width.push(*levels);
                }
            }
        }
        Self { numeric, width }
    }

    pub(crate) fn inputs(&self) -> usize {
        self.width.iter().sum()
    }

    /// The input vector of one row's values (`None` when one is missing).
    pub(crate) fn encode(&self, value: &dyn Fn(usize) -> Value) -> Option<Vec<f64>> {
        let mut out = Vec::with_capacity(self.inputs());
        for (j, w) in self.width.iter().enumerate() {
            match (value(j), self.numeric[j]) {
                (Value::Number(x), Some((m, s))) if x.is_finite() => out.push((x - m) / s),
                (Value::Level(l), None) if l < *w => out.extend((0..*w).map(|k| f64::from(u8::from(k == l)))),
                _ => return None,
            }
        }
        Some(out)
    }
}

/// How rows are divided between training, validation and test.
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationMethod {
    /// Every row trains; tuning uses cross-validation within them.
    None,
    /// A column naming each row's role: `0`/`1`/`2` or text starting
    /// `train`, `valid` or `test` (rows with an empty cell are left out).
    Column(usize),
    /// Random shares held back for validation and test.
    Holdout {
        /// Share of rows for validation.
        validation: f64,
        /// Share of rows for test.
        test: f64,
        /// Seed of the draw.
        seed: u64,
        /// Draw within each class of the response (quartiles of a
        /// continuous one), so every role holds each in proportion.
        stratify: bool,
        /// Give whole groups of this column (e.g. a subject) the same role.
        group: Option<usize>,
    },
    /// Every row trains; tuning and the reported held-out measures use
    /// k-fold cross-validation.
    KFold {
        /// Number of folds.
        folds: usize,
        /// Seed of the fold assignment.
        seed: u64,
    },
}

impl ValidationMethod {
    /// Identifier in project files and scripts.
    #[must_use]
    pub fn key(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Column(_) => "column",
            Self::Holdout { .. } => "holdout",
            Self::KFold { .. } => "k-fold",
        }
    }

    /// Columns read (a role or group column).
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        match self {
            Self::Column(c) | Self::Holdout { group: Some(c), .. } => vec![*c],
            _ => Vec::new(),
        }
    }

    /// The same method with column indices passed through `f`.
    #[must_use]
    pub fn map_columns(self, f: impl Fn(usize) -> usize) -> Self {
        match self {
            Self::Column(c) => Self::Column(f(c)),
            Self::Holdout { validation, test, seed, stratify, group } => {
                Self::Holdout { validation, test, seed, stratify, group: group.map(f) }
            }
            other => other,
        }
    }

    /// JSON object (columns by name through `col`).
    #[must_use]
    pub fn to_json(&self, col: &dyn Fn(usize) -> JsonValue) -> JsonValue {
        let mut members = vec![("method".to_owned(), JsonValue::String(self.key().to_owned()))];
        let mut put = |k: &str, v: JsonValue| members.push((k.to_owned(), v));
        match self {
            Self::None => {}
            Self::Column(c) => put("column", col(*c)),
            Self::Holdout { validation, test, seed, stratify, group } => {
                put("validation", JsonValue::number(*validation));
                put("test", JsonValue::number(*test));
                put("seed", JsonValue::Number(*seed as f64));
                put("stratify", JsonValue::Bool(*stratify));
                put("group", group.map_or(JsonValue::Null, col));
            }
            Self::KFold { folds, seed } => {
                put("folds", JsonValue::Number(*folds as f64));
                put("seed", JsonValue::Number(*seed as f64));
            }
        }
        JsonValue::Object(members)
    }

    /// Read [`Self::to_json`]'s object (`None` or null: no validation);
    /// `find` turns a column name into its index.
    ///
    /// # Errors
    /// An unknown method or a missing column.
    pub fn from_json(v: Option<&JsonValue>, find: &dyn Fn(&JsonValue) -> Result<usize>) -> Result<Self> {
        let Some(v) = v.filter(|v| !matches!(v, JsonValue::Null)) else { return Ok(Self::None) };
        let num = |k: &str| v.get(k).and_then(JsonValue::as_f64);
        let seed = v.get("seed").and_then(crate::seed::seed_from_json).unwrap_or(1);
        Ok(match v.get("method").and_then(JsonValue::as_str).unwrap_or("none") {
            "none" => Self::None,
            "column" => Self::Column(find(v.get("column").unwrap_or(&JsonValue::Null))?),
            "holdout" => Self::Holdout {
                validation: num("validation").unwrap_or(0.3),
                test: num("test").unwrap_or(0.0),
                seed,
                stratify: v.get("stratify").and_then(JsonValue::as_bool).unwrap_or(false),
                group: match v.get("group") {
                    None | Some(JsonValue::Null) => None,
                    Some(g) => Some(find(g)?),
                },
            },
            "k-fold" => Self::KFold { folds: v.get("folds").and_then(JsonValue::as_usize).unwrap_or(10), seed },
            other => return Err(invalid(format!("unknown validation method {other:?}"))),
        })
    }
}

/// The response as a predictive model sees it, for every table row.
#[derive(Debug, Clone, PartialEq)]
pub enum Response {
    /// A continuous response (NaN missing).
    Continuous(Vec<f64>),
    /// A categorical response: the classes in natural order, and each row's
    /// class (`None` missing).
    Categorical {
        /// Class labels.
        classes: Vec<String>,
        /// Class of each row.
        codes: Vec<Option<usize>>,
    },
}

impl Response {
    /// Read column `index`: continuous when numeric with a continuous
    /// measurement level, otherwise categorical (labels as the data view
    /// shows them; empty cells missing).
    ///
    /// # Errors
    /// No such column, or a categorical response with fewer than two classes
    /// among `rows`.
    pub fn read(table: &DataTable, index: usize, rows: &[usize]) -> Result<Self> {
        let c = table.column(index).ok_or_else(|| invalid(format!("no column {index}")))?;
        if let (Some(v), MeasurementLevel::Continuous) = (c.as_numeric(), c.level) {
            return Ok(Self::Continuous(v.to_vec()));
        }
        let labels: Vec<String> = (0..table.n_rows()).map(|r| c.level_text(r)).collect();
        let mut classes: Vec<String> =
            rows.iter().map(|&r| labels[r].clone()).filter(|l| !l.trim().is_empty()).collect();
        classes.sort_by(|a, b| natural_order(a, b));
        classes.dedup();
        if classes.len() < 2 {
            return Err(invalid(format!("the response {:?} needs at least two classes", c.name)));
        }
        let codes = labels.iter().map(|l| classes.iter().position(|k| k == l)).collect();
        Ok(Self::Categorical { classes, codes })
    }

    /// `true` if row `r` has a response.
    #[must_use]
    pub fn present(&self, r: usize) -> bool {
        match self {
            Self::Continuous(y) => y[r].is_finite(),
            Self::Categorical { codes, .. } => codes[r].is_some(),
        }
    }

    /// Class labels (`None` for a continuous response).
    #[must_use]
    pub fn classes(&self) -> Option<&[String]> {
        match self {
            Self::Continuous(_) => None,
            Self::Categorical { classes, .. } => Some(classes),
        }
    }

    /// Stratum of each of `rows` for stratified sampling: the class, or the
    /// quartile of a continuous response.
    fn strata(&self, rows: &[usize]) -> Vec<usize> {
        match self {
            Self::Categorical { codes, .. } => rows.iter().map(|&r| codes[r].unwrap_or(0)).collect(),
            Self::Continuous(y) => {
                let mut order: Vec<usize> = (0..rows.len()).collect();
                order.sort_by(|&a, &b| y[rows[a]].total_cmp(&y[rows[b]]));
                let mut out = vec![0; rows.len()];
                for (rank, &k) in order.iter().enumerate() {
                    out[k] = 4 * rank / rows.len().max(1);
                }
                out
            }
        }
    }
}

impl Response {
    /// The response as the tree and forest engines take it (a missing class
    /// is coded 0; such rows are never used).
    #[must_use]
    pub fn target(&self) -> Target {
        match self {
            Self::Continuous(y) => Target::Regression(y.clone()),
            Self::Categorical { classes, codes } => Target::Classification {
                classes: codes.iter().map(|c| c.unwrap_or(0)).collect(),
                n_classes: classes.len(),
            },
        }
    }
}

/// One predictor of a tree-based model.
#[derive(Debug, Clone)]
pub struct PredictorInfo {
    /// Column name.
    pub name: String,
    /// Column type (date-time categories cannot be written in formulas).
    pub kind: ColumnKind,
    /// Levels of a categorical predictor (`None` for a continuous one).
    pub levels: Option<Vec<String>>,
    /// Range of a continuous predictor over the training rows.
    pub range: (f64, f64),
    /// Starting setting for the profilers: the training mean, or the most
    /// common level.
    pub default: TermValue,
}

/// The predictors of a tree-based model and their values for every row.
#[derive(Debug, Clone)]
pub struct FeatureSet {
    /// Description of each predictor.
    pub predictors: Vec<PredictorInfo>,
    /// Values of each predictor for every table row.
    pub features: Vec<Feature>,
}

impl FeatureSet {
    /// Read `columns`: numeric columns with a continuous level split at
    /// thresholds; any other column is categorical with the levels seen in
    /// the `train` rows (labels as the data view shows them; other labels and
    /// empty cells are missing).
    ///
    /// # Errors
    /// A missing column.
    pub fn read(table: &DataTable, columns: &[usize], train: &[usize]) -> Result<Self> {
        let mut predictors = Vec::new();
        let mut features = Vec::new();
        for &p in columns {
            let c = table.column(p).ok_or_else(|| invalid(format!("no column {p}")))?;
            match (c.as_numeric(), c.level) {
                (Some(v), MeasurementLevel::Continuous) => {
                    let present: Vec<f64> = train.iter().map(|&r| v[r]).filter(|x| x.is_finite()).collect();
                    let lo = present.iter().copied().fold(f64::INFINITY, f64::min);
                    let hi = present.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                    let mean = present.iter().sum::<f64>() / present.len().max(1) as f64;
                    predictors.push(PredictorInfo {
                        name: c.name.clone(),
                        kind: c.kind(),
                        levels: None,
                        range: if lo.is_finite() { (lo, hi) } else { (0.0, 0.0) },
                        default: TermValue::Number(if mean.is_finite() { mean } else { 0.0 }),
                    });
                    features
                        .push(Feature::Numeric(v.iter().map(|x| if x.is_finite() { *x } else { f64::NAN }).collect()));
                }
                _ => {
                    let labels: Vec<String> = (0..table.n_rows()).map(|r| c.level_text(r)).collect();
                    let mut levels: Vec<String> =
                        train.iter().map(|&r| labels[r].clone()).filter(|l| !l.trim().is_empty()).collect();
                    levels.sort_by(|a, b| natural_order(a, b));
                    levels.dedup();
                    let codes: Vec<Option<usize>> = labels.iter().map(|l| levels.iter().position(|k| k == l)).collect();
                    let mut counts = vec![0usize; levels.len()];
                    for &r in train {
                        if let Some(k) = codes[r] {
                            counts[k] += 1;
                        }
                    }
                    let common = (0..levels.len()).max_by_key(|&k| (counts[k], usize::MAX - k)).unwrap_or(0);
                    predictors.push(PredictorInfo {
                        name: c.name.clone(),
                        kind: c.kind(),
                        levels: Some(levels.clone()),
                        range: (0.0, 0.0),
                        default: TermValue::Level(common),
                    });
                    features.push(Feature::Categorical { codes, levels: levels.len() });
                }
            }
        }
        Ok(Self { predictors, features })
    }

    /// The profiler factors of the predictors.
    #[must_use]
    pub fn profile_factors(&self) -> Vec<ProfileFactor> {
        self.predictors
            .iter()
            .map(|p| ProfileFactor {
                name: p.name.clone(),
                kind: match &p.levels {
                    Some(levels) => ProfileFactorKind::Categorical { levels: levels.clone() },
                    None => ProfileFactorKind::Continuous { range: p.range },
                },
            })
            .collect()
    }

    /// Observations for the 3-D profiler: `rows` with every predictor
    /// present, their response given by `response`.
    #[must_use]
    pub fn observations(&self, rows: &[usize], response: &dyn Fn(usize) -> f64) -> Vec<Observation> {
        rows.iter()
            .filter_map(|&r| {
                let settings: Option<Vec<TermValue>> = self
                    .features
                    .iter()
                    .map(|f| match f.value(r) {
                        Value::Number(x) => Some(TermValue::Number(x)),
                        Value::Level(l) => Some(TermValue::Level(l)),
                        Value::Missing => None,
                    })
                    .collect();
                settings.map(|settings| Observation { row: r, settings, response: response(r) })
            })
            .collect()
    }
}

/// A profiler setting as a predictor value (missing when it does not fit
/// the predictor).
#[must_use]
pub fn setting_value(feature: &Feature, v: &TermValue) -> Value {
    match (feature, v) {
        (Feature::Numeric(_), TermValue::Number(x)) if x.is_finite() => Value::Number(*x),
        (Feature::Categorical { levels, .. }, TermValue::Level(l)) if l < levels => Value::Level(*l),
        _ => Value::Missing,
    }
}

/// The role of every table row.
#[derive(Debug, Clone, PartialEq)]
pub struct RoleAssignment {
    /// Role of each table row; `None` for rows not used (excluded, missing
    /// response, or no role in a validation column).
    pub roles: Vec<Option<Role>>,
    /// Cross-validation folds and seed, when the method is k-fold.
    pub folds: Option<(usize, u64)>,
    /// How the roles were assigned, for reports.
    pub description: String,
}

impl RoleAssignment {
    /// Rows with role `role`, in table order.
    #[must_use]
    pub fn rows(&self, role: Role) -> Vec<usize> {
        (0..self.roles.len()).filter(|&r| self.roles[r] == Some(role)).collect()
    }

    /// Assign roles to the rows of `table` that are not excluded and have a
    /// response.
    ///
    /// # Errors
    /// Invalid shares or folds, an unreadable role column (a value other
    /// than the three roles), or no training rows.
    pub fn assign(
        table: &DataTable,
        states: &RowStateManager,
        response: &Response,
        method: &ValidationMethod,
    ) -> Result<Self> {
        let eligible: Vec<usize> =
            (0..table.n_rows()).filter(|&r| !states.is_excluded(r) && response.present(r)).collect();
        let mut roles = vec![None; table.n_rows()];
        let mut folds = None;
        let description = match method {
            ValidationMethod::None => {
                eligible.iter().for_each(|&r| roles[r] = Some(Role::Training));
                "every row trains".to_owned()
            }
            ValidationMethod::KFold { folds: k, seed } => {
                eligible.iter().for_each(|&r| roles[r] = Some(Role::Training));
                folds = Some((*k, *seed));
                format!("every row trains; {k}-fold cross-validation (seed {seed})")
            }
            ValidationMethod::Column(c) => {
                let col = table.column(*c).ok_or_else(|| invalid(format!("no column {c}")))?;
                for &r in &eligible {
                    let text = col.level_text(r);
                    if text.trim().is_empty() {
                        continue;
                    }
                    roles[r] = Some(Role::parse(&text).ok_or_else(|| {
                        invalid(format!(
                            "the validation column {:?} has the value {text:?}; use 0/1/2 or Training/Validation/Test",
                            col.name
                        ))
                    })?);
                }
                format!("roles from the column {:?}", col.name)
            }
            ValidationMethod::Holdout { validation, test, seed, stratify, group } => {
                let drawn = if let Some(g) = group {
                    let col = table.column(*g).ok_or_else(|| invalid(format!("no column {g}")))?;
                    let mut labels: Vec<String> = eligible.iter().map(|&r| col.level_text(r)).collect();
                    let mut distinct = labels.clone();
                    distinct.sort();
                    distinct.dedup();
                    let ids: Vec<usize> = labels.drain(..).map(|l| distinct.binary_search(&l).unwrap_or(0)).collect();
                    grouped_roles(&ids, *validation, *test, *seed)?
                } else if *stratify {
                    stratified_roles(&response.strata(&eligible), *validation, *test, *seed)?
                } else {
                    random_roles(eligible.len(), *validation, *test, *seed)?
                };
                for (&r, role) in eligible.iter().zip(drawn) {
                    roles[r] = Some(role);
                }
                let how = match (group, stratify) {
                    (Some(g), _) => format!(", whole groups of {:?}", table.column(*g).map_or("", |c| c.name.as_str())),
                    (None, true) => ", stratified by the response".to_owned(),
                    (None, false) => String::new(),
                };
                format!(
                    "random: {}% validation, {}% test (seed {seed}{how})",
                    format_number(validation * 100.0, 3),
                    format_number(test * 100.0, 3)
                )
            }
        };
        let out = Self { roles, folds, description };
        if out.rows(Role::Training).is_empty() {
            return Err(invalid("no rows are left for training"));
        }
        Ok(out)
    }
}

/// Predictions for every table row (NaN where the model cannot predict).
#[derive(Debug, Clone, PartialEq)]
pub enum Predictions {
    /// Predicted values of a continuous response.
    Values(Vec<f64>),
    /// Class probabilities (one vector per row, in class order).
    Probabilities(Vec<Vec<f64>>),
}

/// A fitted predictive model, as shared report sections see it.
pub trait Predictor {
    /// Response name.
    fn response_name(&self) -> &str;
    /// The observed response.
    fn response(&self) -> &Response;
    /// The roles the model was fitted and tuned with.
    fn roles(&self) -> &RoleAssignment;
    /// Predictions for every table row.
    fn predictions(&self) -> &Predictions;
    /// Held-out predictions from cross-validation (each row predicted by the
    /// model fitted without its fold), when cross-validation was used.
    fn cross_validated(&self) -> Option<&Predictions> {
        None
    }
    /// Row label of [`Predictor::cross_validated`] in the fit table.
    fn held_out_label(&self) -> &'static str {
        "Cross-validation"
    }
}

fn measures_row(response: &Response, predictions: &Predictions, rows: &[usize]) -> Option<Vec<String>> {
    let f = |v: f64, d: usize| if v.is_nan() { "—".to_owned() } else { format_number(v, d) };
    match (response, predictions) {
        (Response::Continuous(y), Predictions::Values(p)) => {
            let y: Vec<f64> = rows.iter().map(|&r| y[r]).collect();
            let p: Vec<f64> = rows.iter().map(|&r| p[r]).collect();
            let m = ContinuousFit::new(&y, &p)?;
            Some(vec![m.n.to_string(), f(m.rmse, 6), f(m.mae, 6), f(m.r_squared, 4)])
        }
        (Response::Categorical { classes, codes }, Predictions::Probabilities(p)) => {
            let (obs, probs): (Vec<usize>, Vec<Vec<f64>>) =
                rows.iter().filter_map(|&r| codes[r].map(|c| (c, p[r].clone()))).unzip();
            let m = ClassFit::new(&obs, &probs, classes.len())?;
            Some(vec![m.n.to_string(), f(m.misclassification, 4), f(m.log_loss, 4), f(m.auc, 4)])
        }
        _ => None,
    }
}

/// "Fit by Role": the fit measures of `model` on its training, validation
/// and test rows, and from cross-validation when used.
#[must_use]
pub fn fit_measures_section(model: &dyn Predictor) -> ReportSection {
    let response = model.response();
    let headers: Vec<&str> = match response {
        Response::Continuous(_) => vec!["Rows", "Observations", "RMSE", "Mean abs. error", "R²"],
        Response::Categorical { .. } => vec!["Rows", "Observations", "Misclassification", "Log loss", "AUC"],
    };
    let mut t = TextTable::new(headers);
    for role in Role::ALL {
        let rows = model.roles().rows(role);
        if let Some(cells) = measures_row(response, model.predictions(), &rows) {
            t.push(std::iter::once(role.label().to_owned()).chain(cells).collect::<Vec<_>>());
        }
    }
    if let Some(cv) = model.cross_validated() {
        let rows = model.roles().rows(Role::Training);
        if let Some(cells) = measures_row(response, cv, &rows) {
            t.push(std::iter::once(model.held_out_label().to_owned()).chain(cells).collect::<Vec<_>>());
        }
    }
    let auc_note = match response.classes() {
        Some(c) if c.len() == 2 => format!("; AUC of P({})", c[1]),
        Some(_) => "; AUC averaged over each class against the rest".to_owned(),
        None => String::new(),
    };
    ReportSection {
        kind: SectionKind::FitByRole,
        title: format!("Fit by Role ({}{auc_note})", model.roles().description),
        content: SectionContent::Table(t),
    }
}

/// Confusion matrices (actual by predicted counts) of a categorical
/// response, one block per role; `None` for a continuous response.
#[must_use]
pub fn confusion_section(model: &dyn Predictor) -> Option<ReportSection> {
    let (Response::Categorical { classes, codes }, Predictions::Probabilities(p)) =
        (model.response(), model.predictions())
    else {
        return None;
    };
    let headers: Vec<String> = ["Rows".to_owned(), "Actual".to_owned()]
        .into_iter()
        .chain(classes.iter().map(|c| format!("Predicted {c}")))
        .collect();
    let mut t = TextTable::new(headers);
    for role in Role::ALL {
        let rows = model.roles().rows(role);
        if rows.is_empty() {
            continue;
        }
        let mut counts = vec![vec![0usize; classes.len()]; classes.len()];
        for &r in &rows {
            if let (Some(c), true) = (codes[r], p[r].iter().all(|v| v.is_finite())) {
                counts[c][most_probable(&p[r])] += 1;
            }
        }
        for (k, class) in classes.iter().enumerate() {
            let label = if k == 0 { role.label().to_owned() } else { String::new() };
            t.push(
                [label, class.clone()].into_iter().chain(counts[k].iter().map(ToString::to_string)).collect::<Vec<_>>(),
            );
        }
    }
    Some(ReportSection {
        kind: SectionKind::ConfusionMatrix,
        title: "Confusion Matrix (counts of actual by most probable class)".to_owned(),
        content: SectionContent::Table(t),
    })
}

/// Formulas computing a model's predictions, for formula columns.
#[derive(Debug, Clone, Default)]
pub struct PredictionFormulas {
    /// The predicted value (continuous) or most probable class.
    pub prediction: Option<String>,
    /// Each class's probability (categorical).
    pub probabilities: Vec<String>,
}

/// Columns a predictive model can save: predicted values (continuous), or
/// each class's probability and the most probable class (categorical), as
/// formula columns when `formulas` gives them; and the validation roles.
#[must_use]
pub fn prediction_columns(model: &dyn Predictor, formulas: &PredictionFormulas) -> Vec<ReportColumn> {
    let name = model.response_name();
    let mut out = Vec::new();
    let with_formula = |mut c: ReportColumn, f: Option<&String>| {
        c.formula = f.cloned();
        c
    };
    match (model.response(), model.predictions()) {
        (Response::Continuous(_), Predictions::Values(p)) => {
            out.push(with_formula(
                ReportColumn::continuous("predicted", "Save predicted values", format!("Predicted {name}"), p.clone()),
                formulas.prediction.as_ref(),
            ));
        }
        (Response::Categorical { classes, .. }, Predictions::Probabilities(p)) => {
            for (k, class) in classes.iter().enumerate() {
                let key = format!("probability_{}", k + 1);
                let c = ReportColumn::continuous(
                    &key,
                    &format!("Save probability of {class}"),
                    format!("Prob {name} = {class}"),
                    p.iter().map(|row| row.get(k).copied().unwrap_or(f64::NAN)).collect(),
                );
                out.push(with_formula(c, formulas.probabilities.get(k)));
            }
            let most: Vec<String> = p
                .iter()
                .map(|row| {
                    if row.iter().all(|v| v.is_finite()) { classes[most_probable(row)].clone() } else { String::new() }
                })
                .collect();
            out.push(with_formula(
                ReportColumn::text("most_likely", "Save most likely class", format!("Most likely {name}"), most),
                formulas.prediction.as_ref(),
            ));
        }
        _ => {}
    }
    let roles: Vec<String> =
        model.roles().roles.iter().map(|r| r.map_or(String::new(), |r| r.label().to_owned())).collect();
    out.push(ReportColumn::text("validation", "Save validation roles", "Validation".to_owned(), roles));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn table() -> DataTable {
        let n = 40;
        DataTable::new(
            "t",
            vec![
                Column::numeric("y", (0..n).map(|i| f64::from(i as u32)).collect()),
                Column::text("g", (0..n).map(|i| format!("s{}", i / 4)).collect()),
                Column::text(
                    "role",
                    (0..n).map(|i| ["Training", "validation", "TEST", ""][i % 4].to_owned()).collect(),
                ),
                Column::text("bad", (0..n).map(|_| "hold".to_owned()).collect()),
            ],
        )
        .unwrap()
    }

    #[test]
    fn roles_from_every_method() {
        let t = table();
        let mut states = RowStateManager::new(40);
        states.set_excluded([0], true);
        let y = Response::read(&t, 0, &(0..40).collect::<Vec<_>>()).unwrap();
        let none = RoleAssignment::assign(&t, &states, &y, &ValidationMethod::None).unwrap();
        assert_eq!(none.rows(Role::Training).len(), 39);
        assert_eq!(none.roles[0], None, "excluded rows have no role");

        let col = RoleAssignment::assign(&t, &states, &y, &ValidationMethod::Column(2)).unwrap();
        assert_eq!(col.rows(Role::Validation).len(), 10);
        assert_eq!(col.rows(Role::Test).len(), 10);
        assert_eq!(col.rows(Role::Training).len(), 9);
        assert_eq!(col.roles[3], None, "an empty cell leaves the row out");
        assert!(RoleAssignment::assign(&t, &states, &y, &ValidationMethod::Column(3)).is_err());

        let grouped =
            ValidationMethod::Holdout { validation: 0.3, test: 0.0, seed: 2, stratify: false, group: Some(1) };
        let g = RoleAssignment::assign(&t, &RowStateManager::new(40), &y, &grouped).unwrap();
        for s in 0..10 {
            assert!((0..4).all(|k| g.roles[s * 4 + k] == g.roles[s * 4]), "groups stay together");
        }
        assert_eq!(g.rows(Role::Validation).len(), 12);

        let strat = ValidationMethod::Holdout { validation: 0.25, test: 0.25, seed: 2, stratify: true, group: None };
        let s = RoleAssignment::assign(&t, &RowStateManager::new(40), &y, &strat).unwrap();
        // Each quartile of y (ten rows) gives about a quarter to validation.
        for q in 0..4 {
            let v = (q * 10..q * 10 + 10).filter(|&r| s.roles[r] == Some(Role::Validation)).count();
            assert!((2..=3).contains(&v), "quartile {q}: {v}");
        }

        // JSON round trip, columns by name.
        let col_json = |i: usize| JsonValue::String(t.column(i).unwrap().name.clone());
        let find = |v: &JsonValue| t.find(v.as_str().unwrap_or_default()).ok_or_else(|| invalid("no column"));
        for m in [
            grouped,
            strat,
            ValidationMethod::Column(2),
            ValidationMethod::KFold { folds: 5, seed: 3 },
            ValidationMethod::None,
        ] {
            assert_eq!(ValidationMethod::from_json(Some(&m.to_json(&col_json)), &find).unwrap(), m);
        }
    }
}
