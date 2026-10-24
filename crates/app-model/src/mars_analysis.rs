//! Multivariate adaptive regression splines bound to a dataset
//! ([`stat_predictive::mars`]): a continuous response modelled as a sum of
//! hinge functions `max(0, x − t)`, `max(0, t − x)` and their products.
//! Uses the shared predictive-model sections ([`crate::prediction`]).
//!
//! **Rows.** Excluded rows and rows missing the response take no role.
//! The model is fitted to the training rows that have every predictor;
//! rows missing a predictor the final model uses get no prediction.
//!
//! **Size.** The forward pass builds the largest model; the backward pass
//! gives the best subset of each size. The size is chosen by the smallest
//! GCV when every row trains, by the smallest error on the validation rows
//! when there are any, or by the smallest k-fold cross-validated error
//! (each fold repeats both passes without its rows; the fit table then
//! shows each row's prediction from the fold that held it out).
//!
//! **Predictors.** Continuous predictors enter through hinges, or only
//! linearly when listed as linear; a categorical predictor enters through
//! indicators of its levels other than the first.

use stat_core::{Result, StatError};
use stat_data::{ColumnKind, DataTable, JsonValue, RowStateManager};
use stat_linear::TermValue;
use stat_predictive::mars::{Direction, Factor, MarsFit, MarsOptions, fit_mars};
use stat_predictive::{Feature, Role, fold_assignment};

use crate::format::format_number;
use crate::prediction::{
    FeatureSet, PredictionFormulas, Predictions, Predictor, PredictorInfo, Response, RoleAssignment, ValidationMethod,
    fit_measures_section, prediction_columns, setting_value,
};
use crate::report::{Annotations, PlotSpec, PointGroups, ReportSection, SectionContent, SectionKind, TextTable};
use crate::surface_profiler::{Observation, ProfileFactor, SurfaceModel};

const DIGITS: usize = 6;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Columns and settings of a MARS model.
#[derive(Debug, Clone, PartialEq)]
pub struct MarsRequest {
    /// Response column (continuous).
    pub response: usize,
    /// Predictor columns.
    pub predictors: Vec<usize>,
    /// Continuous predictors that enter only linearly (a subset of
    /// `predictors`).
    pub linear: Vec<usize>,
    /// Training / validation / test roles.
    pub validation: ValidationMethod,
    /// Greatest interaction degree.
    pub max_degree: usize,
    /// Greatest number of forward-pass terms (0: automatic).
    pub max_terms: usize,
    /// GCV cost per knot (`None`: 2 additive, 3 with interactions).
    pub penalty: Option<f64>,
    /// Forward-pass threshold on the gain in R².
    pub threshold: f64,
    /// Cases between knots (0: Friedman's rule).
    pub min_span: usize,
    /// Cases outside the outermost knots (0: Friedman's rule).
    pub end_span: usize,
}

impl MarsRequest {
    /// Default settings ([`MarsOptions::default`]) with every row training.
    #[must_use]
    pub fn new(response: usize, predictors: Vec<usize>) -> Self {
        let d = MarsOptions::default();
        Self {
            response,
            predictors,
            linear: Vec::new(),
            validation: ValidationMethod::None,
            max_degree: d.max_degree,
            max_terms: d.max_terms,
            penalty: d.penalty,
            threshold: d.threshold,
            min_span: d.min_span,
            end_span: d.end_span,
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
        self.linear.iter_mut().for_each(|c| *c = f(*c));
        self.validation = self.validation.map_columns(&f);
        self
    }

    fn options(&self) -> MarsOptions {
        MarsOptions {
            max_degree: self.max_degree,
            max_terms: self.max_terms,
            penalty: self.penalty,
            threshold: self.threshold,
            min_span: self.min_span,
            end_span: self.end_span,
        }
    }
}

/// How the number of terms was chosen.
#[derive(Debug, Clone, PartialEq)]
enum SizeChoice {
    /// Smallest GCV.
    Gcv,
    /// Smallest mean squared error on the validation rows (`mse[k − 1]`).
    Validation { mse: Vec<f64> },
    /// Smallest cross-validated mean squared error.
    CrossValidation { mse: Vec<f64>, folds: usize },
}

/// A fitted MARS model.
#[derive(Debug, Clone)]
pub struct MarsAnalysis {
    request: MarsRequest,
    response_name: String,
    response: Response,
    roles: RoleAssignment,
    predictors: Vec<PredictorInfo>,
    features: Vec<Feature>,
    model: MarsFit,
    choice: SizeChoice,
    predictions: Predictions,
    held_out: Option<Predictions>,
}

/// Mean squared error of each subset size over `rows` (sizes beyond a
/// fold's sequence are missing).
fn subset_mse(fit: &MarsFit, features: &[Feature], y: &[f64], rows: &[usize]) -> Result<Vec<f64>> {
    Ok(fit.validation_errors(features, y, rows)?.into_iter().map(|(e, n)| e / n.max(1) as f64).collect())
}

fn argmin(v: &[f64]) -> usize {
    (0..v.len()).min_by(|&a, &b| v[a].total_cmp(&v[b]).then(a.cmp(&b))).unwrap_or(0)
}

impl MarsAnalysis {
    /// Fit and evaluate the model.
    ///
    /// # Errors
    /// No predictors, the response among them or not continuous, invalid
    /// validation or MARS settings, or too few complete training rows.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &MarsRequest) -> Result<Self> {
        if request.predictors.is_empty() {
            return Err(invalid("choose at least one predictor"));
        }
        if request.predictors.contains(&request.response) {
            return Err(invalid("the response cannot also be a predictor"));
        }
        let included: Vec<usize> = (0..table.n_rows()).filter(|&r| !states.is_excluded(r)).collect();
        let response = Response::read(table, request.response, &included)?;
        let response_name = table.column(request.response).map_or(String::new(), |c| c.name.clone());
        let Response::Continuous(y) = &response else {
            return Err(invalid(format!("Adaptive regression splines model a continuous response; {response_name:?} is categorical")));
        };
        let y = y.clone();
        let roles = RoleAssignment::assign(table, states, &response, &request.validation)?;
        let train = roles.rows(Role::Training);
        let valid = roles.rows(Role::Validation);
        let FeatureSet { predictors, features } = FeatureSet::read(table, &request.predictors, &train)?;
        let linear: Vec<bool> = request.predictors.iter().map(|c| request.linear.contains(c)).collect();
        let options = request.options();
        let mut model = fit_mars(&features, &linear, &y, &train, &options)?;
        let mut held_out = None;
        let choice = if let Some((folds, seed)) = roles.folds {
            let folds = folds.min(train.len()).max(2);
            let fold_of = fold_assignment(train.len(), folds, seed)?;
            let mut sse = vec![0.0; model.subsets.len()];
            let mut counted = vec![0usize; model.subsets.len()];
            let mut fold_fits = Vec::with_capacity(folds);
            for f in 0..folds {
                let fit_rows: Vec<usize> =
                    train.iter().zip(&fold_of).filter(|(_, k)| **k != f).map(|(&r, _)| r).collect();
                let out_rows: Vec<usize> =
                    train.iter().zip(&fold_of).filter(|(_, k)| **k == f).map(|(&r, _)| r).collect();
                // A fold whose fitting rows cannot be fitted (a constant
                // response, too few rows) leaves its rows unpredicted.
                let Ok(m) = fit_mars(&features, &linear, &y, &fit_rows, &options) else { continue };
                let errors = m.validation_errors(&features, &y, &out_rows)?;
                let Some(&last) = errors.last() else { continue };
                for k in 0..sse.len() {
                    // A fold whose sequence is shorter keeps its largest model.
                    let (e, n) = errors.get(k).copied().unwrap_or(last);
                    sse[k] += e;
                    counted[k] += n;
                }
                fold_fits.push((m, out_rows));
            }
            let mse: Vec<f64> =
                sse.iter().zip(&counted).map(|(s, &c)| if c == 0 { f64::NAN } else { s / c as f64 }).collect();
            let size = argmin(&mse) + 1;
            model.select(size)?;
            let mut values = vec![f64::NAN; table.n_rows()];
            for (mut m, out_rows) in fold_fits {
                m.select(size.min(m.subsets.len()))?;
                for r in out_rows {
                    values[r] = m.predict(&|j| features[j].value(r));
                }
            }
            held_out = Some(Predictions::Values(values));
            SizeChoice::CrossValidation { mse, folds }
        } else if valid.is_empty() {
            SizeChoice::Gcv
        } else {
            let mse = subset_mse(&model, &features, &y, &valid)?;
            model.select(argmin(&mse) + 1)?;
            SizeChoice::Validation { mse }
        };
        let predictions =
            Predictions::Values((0..table.n_rows()).map(|r| model.predict(&|j| features[j].value(r))).collect());
        Ok(Self {
            request: request.clone(),
            response_name,
            response,
            roles,
            predictors,
            features,
            model,
            choice,
            predictions,
            held_out,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &MarsRequest {
        &self.request
    }

    /// The fitted model.
    #[must_use]
    pub fn model(&self) -> &MarsFit {
        &self.model
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let names: Vec<&str> = self.predictors.iter().map(|p| p.name.as_str()).collect();
        let degree = match self.request.max_degree {
            1 => "additive".to_owned(),
            d => format!("degree {d}"),
        };
        format!("Adaptive splines ({degree}): {} ~ {}", self.response_name, names.join(" + "))
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        vec![
            self.summary_section(),
            fit_measures_section(self),
            self.basis_section(),
            self.importance_section(),
            self.selection_plot(),
            self.forward_section(),
        ]
    }

    fn level_label(&self, feature: usize, level: usize) -> String {
        self.predictors[feature].levels.as_ref().and_then(|l| l.get(level)).cloned().unwrap_or_default()
    }

    /// A basis function as text, e.g. `max(0, Load − 41.2) × max(0, 3 − Speed)`.
    fn term_label(&self, t: usize) -> String {
        let factors = &self.model.basis[t].factors;
        if factors.is_empty() {
            return "Intercept".to_owned();
        }
        let parts: Vec<String> = factors
            .iter()
            .map(|f| {
                let name = &self.predictors[f.feature()].name;
                match *f {
                    Factor::Hinge { knot, direction: Direction::Right, .. } => {
                        format!("max(0, {name} − {})", format_number(knot, DIGITS))
                    }
                    Factor::Hinge { knot, direction: Direction::Left, .. } => {
                        format!("max(0, {} − {name})", format_number(knot, DIGITS))
                    }
                    Factor::Linear { .. } => name.clone(),
                    Factor::Level { feature, level } => format!("[{name} = {}]", self.level_label(feature, level)),
                }
            })
            .collect();
        parts.join(" × ")
    }

    fn size_description(&self) -> String {
        let k = self.model.selected.len();
        let m = self.model.subsets.len();
        match &self.choice {
            SizeChoice::Gcv => format!("{k} of {m} (smallest GCV)"),
            SizeChoice::Validation { .. } => format!("{k} of {m} (smallest validation error)"),
            SizeChoice::CrossValidation { folds, .. } => {
                format!("{k} of {m} (smallest {folds}-fold cross-validated error)")
            }
        }
    }

    fn summary_section(&self) -> ReportSection {
        let m = &self.model;
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut row = |a: &str, b: String| t.push([a.to_owned(), b]);
        row("Response", self.response_name.clone());
        for role in Role::ALL {
            let n = self.roles.rows(role).len();
            if n > 0 || role == Role::Training {
                row(&format!("{} rows", role.label()), n.to_string());
            }
        }
        row("Rows fitted (training rows with every predictor)", m.n.to_string());
        row(
            "Interaction degree",
            if self.request.max_degree == 1 { "1 (additive)".to_owned() } else { self.request.max_degree.to_string() },
        );
        row("Forward-pass terms", format!("{} ({})", m.basis.len(), m.stop.describe()));
        row("Terms kept", self.size_description());
        row("RSS", format_number(m.rss(), DIGITS));
        row("R²", format_number(m.r_squared(), 4));
        row("GCV", format_number(m.gcv(), DIGITS));
        row("Generalized R² (1 − GCV / GCV of the mean)", format_number(m.generalized_r_squared(), 4));
        row(
            "Settings",
            format!(
                "GCV penalty {} per knot; threshold {}; minimum span {}{} cases; end span {} cases",
                format_number(m.penalty, 4),
                format_number(self.request.threshold, 4),
                m.min_span,
                if self.request.min_span == 0 { " (for the intercept parent)" } else { "" },
                m.end_span
            ),
        );
        if !self.request.linear.is_empty() {
            let names: Vec<&str> = self
                .request
                .predictors
                .iter()
                .zip(&self.predictors)
                .filter(|(c, _)| self.request.linear.contains(c))
                .map(|(_, p)| p.name.as_str())
                .collect();
            row("Linear predictors", names.join(", "));
        }
        ReportSection {
            kind: SectionKind::MarsSummary,
            title: "Adaptive Splines Summary".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    fn basis_section(&self) -> ReportSection {
        let mut t = TextTable::new(["Basis function", "Coefficient", "Degree"]);
        for (&term, b) in self.model.selected.iter().zip(&self.model.coefficients) {
            t.push([self.term_label(term), format_number(*b, DIGITS), self.model.basis[term].degree().to_string()]);
        }
        ReportSection {
            kind: SectionKind::MarsBasis,
            title: format!("Basis Functions ({} terms)", self.model.selected.len()),
            content: SectionContent::Table(t),
        }
    }

    fn importance_section(&self) -> ReportSection {
        let imp = self.model.importance(self.predictors.len());
        let mut order: Vec<usize> = (0..self.predictors.len()).collect();
        order.sort_by(|&a, &b| imp[b].subsets.cmp(&imp[a].subsets).then(imp[b].gcv.total_cmp(&imp[a].gcv)));
        let top = imp.iter().map(|i| i.gcv.max(0.0).sqrt()).fold(0.0, f64::max);
        let mut t = TextTable::new(["Predictor", "Subsets", "GCV reduction", "Relative", "RSS reduction"]);
        for j in order {
            let rel = if top > 0.0 { 100.0 * imp[j].gcv.max(0.0).sqrt() / top } else { 0.0 };
            t.push([
                self.predictors[j].name.clone(),
                imp[j].subsets.to_string(),
                format_number(imp[j].gcv, DIGITS),
                format_number(rel, 3),
                format_number(imp[j].rss, DIGITS),
            ]);
        }
        ReportSection {
            kind: SectionKind::VariableImportance,
            title: "Variable Importance (over the backward-pass subsets up to the size kept; relative: √ of the GCV \
                    reduction, largest = 100)"
                .to_owned(),
            content: SectionContent::Table(t),
        }
    }

    /// GCV (and the held-out mean squared error, when used) against the
    /// number of terms, with the size kept marked.
    fn selection_plot(&self) -> ReportSection {
        let gcv: Vec<[f64; 2]> = self
            .model
            .subsets
            .iter()
            .enumerate()
            .filter(|(_, s)| s.gcv.is_finite())
            .map(|(k, s)| [(k + 1) as f64, s.gcv])
            .collect();
        let mut labels = vec!["GCV".to_owned()];
        let mut lines = vec![gcv];
        let held = match &self.choice {
            SizeChoice::Gcv => None,
            SizeChoice::Validation { mse } => Some(("Validation MSE", mse)),
            SizeChoice::CrossValidation { mse, .. } => Some(("Cross-validated MSE", mse)),
        };
        if let Some((name, mse)) = held {
            labels.push(name.to_owned());
            lines.push(
                mse.iter().enumerate().filter(|(_, v)| v.is_finite()).map(|(k, &v)| [(k + 1) as f64, v]).collect(),
            );
        }
        let kept = self.model.selected.len() as f64;
        let g = lines.len() - 1;
        let mut items = Vec::new();
        if let Some(&[x, y]) = lines[g].iter().find(|p| p[0] == kept) {
            items.push((x, y, format!("kept: {} terms", self.model.selected.len()), g));
        }
        let spec = PlotSpec {
            x_label: "Number of terms".to_owned(),
            y_label: "Mean squared error".to_owned(),
            groups: Some(PointGroups {
                title: "Criterion".to_owned(),
                labels: labels.clone(),
                membership: Vec::new(),
                lines,
            }),
            annotations: Some(Annotations { groups: labels, items, labels: Vec::new() }),
            ..PlotSpec::default()
        };
        ReportSection {
            kind: SectionKind::MarsSelection,
            title: "Model Size (GCV by number of terms)".to_owned(),
            content: SectionContent::Plot(Box::new(spec)),
        }
    }

    fn forward_section(&self) -> ReportSection {
        let mut t = TextTable::new(["Step", "Parent", "Terms added", "R²"]);
        for (k, s) in self.model.forward.iter().enumerate() {
            let added: Vec<String> = s.added.iter().map(|&a| self.term_label(a)).collect();
            t.push([(k + 1).to_string(), self.term_label(s.parent), added.join("; "), format_number(s.r_squared, 4)]);
        }
        ReportSection {
            kind: SectionKind::MarsForward,
            title: format!("Forward Pass (stopped: {})", self.model.stop.describe()),
            content: SectionContent::Table(t),
        }
    }

    /// The selected model as a formula (see [`crate::formula`]): a sum of
    /// coefficients times products of `max(…, 0)` hinges, predictor values
    /// and level indicators (`switch` on the label; an unknown level gives a
    /// missing value).
    ///
    /// # Errors
    /// A date-time column used as a category (its labels have no formula
    /// equivalent).
    pub fn prediction_formula(&self) -> Result<String> {
        use crate::formula::column_reference;
        use crate::script_text::{number, string};
        let mut out = String::new();
        for (k, (&term, &b)) in self.model.selected.iter().zip(&self.model.coefficients).enumerate() {
            let mut parts = vec![number(b)];
            for f in &self.model.basis[term].factors {
                let p = &self.predictors[f.feature()];
                let x = column_reference(&p.name);
                parts.push(match *f {
                    Factor::Hinge { knot, direction: Direction::Right, .. } => {
                        format!("max({x} - {}, 0.0)", number(knot))
                    }
                    Factor::Hinge { knot, direction: Direction::Left, .. } => {
                        format!("max({} - {x}, 0.0)", number(knot))
                    }
                    Factor::Linear { .. } => x,
                    Factor::Level { level, .. } => {
                        if matches!(p.kind, ColumnKind::DateTime(_)) {
                            return Err(invalid(format!(
                                "the date-time column {:?} is used as a category; a formula cannot match its labels",
                                p.name
                            )));
                        }
                        let arms: Vec<String> = p
                            .levels
                            .as_deref()
                            .unwrap_or_default()
                            .iter()
                            .enumerate()
                            .map(|(i, l)| format!("{} => {}", string(l), if i == level { 1 } else { 0 }))
                            .collect();
                        format!("(switch text({x}) {{ {}, _ => missing() }})", arms.join(", "))
                    }
                });
            }
            if k > 0 {
                out.push_str("\n+ ");
            }
            out.push_str(&parts.join(" * "));
        }
        Ok(out)
    }

    /// Columns the report can save: predicted values, the prediction
    /// formula as a formula column, and the validation roles.
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

impl Predictor for MarsAnalysis {
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

impl SurfaceModel for MarsAnalysis {
    fn response_name(&self) -> &str {
        &self.response_name
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
                Ok(self.model.predict(&|j| setting_value(&self.features[j], &s[j])))
            })
            .collect()
    }

    fn observations(&self) -> Vec<Observation> {
        let set = FeatureSet { predictors: Vec::new(), features: self.features.clone() };
        set.observations(&self.roles.rows(Role::Training), &|r| match &self.response {
            Response::Continuous(y) => y[r],
            Response::Categorical { .. } => f64::NAN,
        })
    }

    fn profile_note(&self) -> Option<String> {
        Some(format!(
            "Sum of {} basis functions, piecewise linear in each continuous predictor; no confidence bands.",
            self.model.selected.len()
        ))
    }
}

// ---- Project files and scripts ------------------------------------------------

/// The request's members after `response` and `predictors`, for project files.
#[must_use]
pub fn request_members_to_json(
    request: &MarsRequest,
    col: &dyn Fn(usize) -> JsonValue,
) -> Vec<(&'static str, JsonValue)> {
    let n = |x: usize| JsonValue::Number(x as f64);
    let mut out = vec![
        ("validation", request.validation.to_json(col)),
        ("linear", JsonValue::Array(request.linear.iter().map(|&c| col(c)).collect())),
        ("degree", n(request.max_degree)),
        ("max_terms", n(request.max_terms)),
        ("threshold", JsonValue::number(request.threshold)),
        ("min_span", n(request.min_span)),
        ("end_span", n(request.end_span)),
    ];
    if let Some(p) = request.penalty {
        out.push(("penalty", JsonValue::number(p)));
    }
    out
}

/// Read a request from a project file's report entry (missing members take
/// their defaults).
///
/// # Errors
/// A missing column, an unknown validation method, a degree of 0, a
/// negative penalty or a threshold outside [0, 1).
pub fn request_from_json(
    v: &JsonValue,
    response: usize,
    predictors: Vec<usize>,
    find: &dyn Fn(&JsonValue) -> Result<usize>,
) -> Result<MarsRequest> {
    let mut r = MarsRequest::new(response, predictors);
    r.validation = ValidationMethod::from_json(v.get("validation"), find)?;
    if let Some(JsonValue::Array(cols)) = v.get("linear") {
        r.linear = cols.iter().map(find).collect::<Result<_>>()?;
    }
    let size = |k: &str| v.get(k).and_then(JsonValue::as_usize);
    r.max_degree = size("degree").unwrap_or(r.max_degree);
    if r.max_degree == 0 {
        return Err(invalid("adaptive splines: the degree must be at least 1"));
    }
    r.max_terms = size("max_terms").unwrap_or(r.max_terms);
    r.min_span = size("min_span").unwrap_or(r.min_span);
    r.end_span = size("end_span").unwrap_or(r.end_span);
    if let Some(t) = v.get("threshold").and_then(JsonValue::as_f64) {
        if !(0.0..1.0).contains(&t) {
            return Err(invalid("adaptive splines: the threshold must be in [0, 1)"));
        }
        r.threshold = t;
    }
    if let Some(p) = v.get("penalty").and_then(JsonValue::as_f64) {
        if p.is_nan() || p < 0.0 {
            return Err(invalid("adaptive splines: the penalty must not be negative"));
        }
        r.penalty = Some(p);
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    /// y = 4·max(0, x − 0.5) + small noise; one row is missing x, so its
    /// held-out prediction is missing and must not count in the MSE.
    fn hinge() -> DataTable {
        let n = 30;
        let mut x: Vec<f64> = (0..n).map(|i| i as f64 / (n - 1) as f64).collect();
        let y: Vec<f64> =
            x.iter().enumerate().map(|(i, v)| 4.0 * (v - 0.5).max(0.0) + 0.01 * ((i * 7 % 5) as f64 - 2.0)).collect();
        x[7] = f64::NAN;
        DataTable::new("h", vec![Column::numeric("x", x), Column::numeric("y", y)]).unwrap()
    }

    #[test]
    fn cross_validated_mse_divides_by_the_rows_predicted() {
        let t = hinge();
        let states = RowStateManager::new(t.n_rows());
        let mut req = MarsRequest::new(1, vec![0]);
        req.validation = ValidationMethod::KFold { folds: 3, seed: 5 };
        let a = MarsAnalysis::run(&t, &states, &req).unwrap();
        let SizeChoice::CrossValidation { mse, folds } = &a.choice else { panic!("cross-validation") };
        // Hand sum: refit each fold and average the finite squared errors.
        let train = a.roles.rows(Role::Training);
        let y = t.column(1).unwrap().as_numeric().unwrap().to_vec();
        let fold_of = fold_assignment(train.len(), *folds, 5).unwrap();
        let mut sse = vec![0.0; mse.len()];
        let mut count = vec![0usize; mse.len()];
        for f in 0..*folds {
            let fit_rows: Vec<usize> = train.iter().zip(&fold_of).filter(|(_, k)| **k != f).map(|(&r, _)| r).collect();
            let m = fit_mars(&a.features, &[false], &y, &fit_rows, &req.options()).unwrap();
            for (&r, _) in train.iter().zip(&fold_of).filter(|(_, k)| **k == f) {
                for (k, (s, c)) in sse.iter_mut().zip(count.iter_mut()).enumerate() {
                    let size = (k + 1).min(m.subsets.len());
                    let b = m.subset_coefficients(size).unwrap();
                    let e = m.predict_subset(size, &b, &|j| a.features[j].value(r)) - y[r];
                    if e.is_finite() {
                        *s += e * e;
                        *c += 1;
                    }
                }
            }
        }
        // The intercept-only model predicts the row missing x; larger ones do not.
        assert_eq!(count[0], train.len());
        if train.contains(&7) {
            assert!(count.last().is_some_and(|&c| c < train.len()), "{count:?}");
        }
        for (k, m) in mse.iter().enumerate() {
            let want = sse[k] / count[k] as f64;
            assert!((m - want).abs() <= 1e-9 * want.max(1.0), "size {}: {m} vs {want}", k + 1);
        }
    }

    #[test]
    fn a_fold_that_cannot_be_fitted_is_skipped() {
        // Constant but for one row: the fold without that row has a constant
        // response, which MARS cannot fit; that fold is skipped instead of
        // failing the report.
        let mut y = vec![2.0; 12];
        y[5] = 3.0;
        let t =
            DataTable::new("c", vec![Column::numeric("x", (0..12).map(f64::from).collect()), Column::numeric("y", y)])
                .unwrap();
        let mut req = MarsRequest::new(1, vec![0]);
        req.validation = ValidationMethod::KFold { folds: 4, seed: 1 };
        let a = MarsAnalysis::run(&t, &RowStateManager::new(12), &req).unwrap();
        let SizeChoice::CrossValidation { mse, .. } = &a.choice else { panic!() };
        assert!(mse.iter().all(|m| m.is_finite()), "{mse:?}");
        let Some(Predictions::Values(held)) = &a.held_out else { panic!() };
        assert!(held.iter().any(|v| v.is_nan()), "the skipped fold's rows have no prediction");
    }
}
