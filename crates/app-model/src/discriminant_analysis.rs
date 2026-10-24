//! Discriminant analysis bound to a dataset
//! ([`stat_multivariate::discriminant`]): linear or quadratic normal-theory
//! classification of a categorical response from numeric predictors, with
//! Fisher's canonical discriminant functions. Uses the shared
//! predictive-model sections and saved columns ([`crate::prediction`]).
//!
//! **Rows.** Rows that are excluded, miss the response or miss a predictor
//! take no role. The model is fitted to the training rows; validation and
//! test rows (from a validation column or a random hold-out) are only
//! predicted, so their fit measures estimate the error on new data. With
//! k-fold cross-validation every row trains and each row is also predicted
//! by the model fitted without its fold.
//!
//! **Priors.** Proportional to the training rows of each class, or equal.
//!
//! **Saved columns.** Each class's posterior probability and the most likely
//! class (the shared prediction columns, so the model can be compared with
//! others under Compare models), the validation roles, and the scores on the
//! canonical functions.

use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_multivariate::{DiscriminantFit, DiscriminantRule, fit_discriminant};
use stat_predictive::measures::most_probable;
use stat_predictive::{Role, fold_assignment};

use crate::format::{format_number, format_p_value};
use crate::prediction::{
    PredictionFormulas, Predictions, Predictor, Response, RoleAssignment, ValidationMethod, confusion_section,
    fit_measures_section, prediction_columns,
};
use crate::report::{
    Annotations, PlotSpec, PointGroups, ReportSection, RowPoint, SectionContent, SectionKind, TextTable,
};
use crate::report_columns::ReportColumn;

const DIGITS: usize = 5;
/// Most misclassified rows listed.
const MAX_LISTED: usize = 50;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: &str, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.to_owned(), content }
}

/// Prior probabilities of the classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PriorChoice {
    /// Proportional to the training rows of each class.
    #[default]
    Proportional,
    /// Equal for every class.
    Equal,
}

impl PriorChoice {
    /// Key in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Proportional => "proportional",
            Self::Equal => "equal",
        }
    }

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Proportional => "Proportional to the training rows",
            Self::Equal => "Equal",
        }
    }
}

/// Display name of a classification rule.
#[must_use]
pub fn rule_label(rule: DiscriminantRule) -> &'static str {
    match rule {
        DiscriminantRule::Linear => "Linear (pooled covariance)",
        DiscriminantRule::Quadratic => "Quadratic (a covariance per class)",
    }
}

/// Columns and settings of a discriminant analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscriminantRequest {
    /// Categorical response (the classes).
    pub response: usize,
    /// Numeric predictors.
    pub predictors: Vec<usize>,
    /// Linear or quadratic rule.
    pub rule: DiscriminantRule,
    /// Prior probabilities.
    pub priors: PriorChoice,
    /// Training / validation / test roles.
    pub validation: ValidationMethod,
}

impl DiscriminantRequest {
    /// Linear rule, proportional priors, every row training.
    #[must_use]
    pub fn new(response: usize, predictors: Vec<usize>) -> Self {
        Self {
            response,
            predictors,
            rule: DiscriminantRule::Linear,
            priors: PriorChoice::Proportional,
            validation: ValidationMethod::None,
        }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut c = vec![self.response];
        c.extend(&self.predictors);
        c.extend(self.validation.columns());
        c
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.response = f(self.response);
        self.predictors.iter_mut().for_each(|c| *c = f(*c));
        self.validation = self.validation.clone().map_columns(f);
    }

    /// The project-file form (`"analysis": "discriminant-analysis"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let name = |i: usize| crate::project::s(table.column(i).map_or("", |c| c.name.as_str()));
        crate::project::obj([
            ("analysis", crate::project::s("discriminant-analysis")),
            ("response", name(self.response)),
            ("predictors", crate::correlation_analysis::columns_json(&self.predictors, table)),
            (
                "rule",
                crate::project::s(match self.rule {
                    DiscriminantRule::Linear => "linear",
                    DiscriminantRule::Quadratic => "quadratic",
                }),
            ),
            ("priors", crate::project::s(self.priors.key())),
            ("validation", self.validation.to_json(&name)),
        ])
    }

    /// Read the project-file form; missing settings take their defaults.
    ///
    /// # Errors
    /// Missing or unknown columns, or an unknown setting.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let find = |c: &JsonValue| -> Result<usize> {
            let name = c.as_str().ok_or_else(|| invalid("discriminant analysis: expected a column name"))?;
            table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
        };
        let response = find(v.get("response").unwrap_or(&JsonValue::Null))?;
        let predictors = crate::correlation_analysis::json_columns(v, "predictors", table)?;
        let mut r = Self::new(response, predictors);
        r.rule = match v.get("rule").and_then(JsonValue::as_str) {
            None | Some("linear") => DiscriminantRule::Linear,
            Some("quadratic") => DiscriminantRule::Quadratic,
            Some(o) => return Err(invalid(format!("discriminant analysis: unknown rule {o:?}"))),
        };
        r.priors = match v.get("priors").and_then(JsonValue::as_str) {
            None | Some("proportional") => PriorChoice::Proportional,
            Some("equal") => PriorChoice::Equal,
            Some(o) => return Err(invalid(format!("discriminant analysis: unknown priors {o:?}"))),
        };
        r.validation = ValidationMethod::from_json(v.get("validation"), &find)?;
        Ok(r)
    }
}

/// A fitted discriminant analysis.
#[derive(Debug, Clone)]
pub struct DiscriminantAnalysis {
    request: DiscriminantRequest,
    response_name: String,
    predictor_names: Vec<String>,
    response: Response,
    roles: RoleAssignment,
    /// Predictor values of every table row (NaN missing).
    x: Vec<Vec<f64>>,
    fit: DiscriminantFit,
    predictions: Predictions,
    held_out: Option<Predictions>,
}

fn row_values(x: &[Vec<f64>], r: usize) -> Vec<f64> {
    x.iter().map(|c| c[r]).collect()
}

fn fit_rows(
    x: &[Vec<f64>],
    codes: &[Option<usize>],
    classes: &[String],
    rows: &[usize],
    rule: DiscriminantRule,
    priors: PriorChoice,
) -> Result<DiscriminantFit> {
    let g = classes.len();
    let data: Vec<Vec<f64>> = rows.iter().map(|&r| row_values(x, r)).collect();
    let groups: Vec<usize> = rows.iter().map(|&r| codes[r].unwrap_or(0)).collect();
    let mut counts = vec![0usize; g];
    groups.iter().for_each(|&k| counts[k] += 1);
    if let Some(k) = counts.iter().position(|&c| c == 0) {
        return Err(invalid(format!("the class {:?} has no training rows", classes[k])));
    }
    let prior: Vec<f64> = match priors {
        PriorChoice::Proportional => counts.iter().map(|&c| c as f64).collect(),
        PriorChoice::Equal => vec![1.0; g],
    };
    fit_discriminant(&data, &groups, g, rule, &prior)
}

impl DiscriminantAnalysis {
    /// Fit the model.
    ///
    /// # Errors
    /// No predictors, the response among them, a continuous response, a
    /// non-numeric predictor, an invalid validation method, a class without
    /// training rows, and errors from [`fit_discriminant`] (a singular
    /// covariance matrix, too few rows in a class for the quadratic rule).
    pub fn run(table: &DataTable, states: &RowStateManager, request: &DiscriminantRequest) -> Result<Self> {
        if request.predictors.is_empty() {
            return Err(invalid("choose at least one predictor"));
        }
        if request.predictors.contains(&request.response) {
            return Err(invalid("the response cannot also be a predictor"));
        }
        let included: Vec<usize> = (0..table.n_rows()).filter(|&r| !states.is_excluded(r)).collect();
        let response_col = table.column(request.response).ok_or_else(|| invalid("no response column"))?;
        let response_name = response_col.name.clone();
        let response = match Response::read(table, request.response, &included)? {
            Response::Continuous(_) => {
                return Err(invalid(format!(
                    "discriminant analysis classifies a categorical response; {response_name:?} is continuous (set \
                     its measurement level to nominal)"
                )));
            }
            r => r,
        };
        let mut x = Vec::with_capacity(request.predictors.len());
        let mut predictor_names = Vec::new();
        for &i in &request.predictors {
            let c = table.column(i).ok_or_else(|| invalid(format!("no column {i}")))?;
            x.push(
                c.as_numeric()
                    .ok_or_else(|| {
                        invalid(format!("discriminant analysis uses numeric predictors; {:?} is not", c.name))
                    })?
                    .to_vec(),
            );
            predictor_names.push(c.name.clone());
        }
        let mut roles = RoleAssignment::assign(table, states, &response, &request.validation)?;
        let complete = |r: usize| x.iter().all(|c| c[r].is_finite());
        for (r, role) in roles.roles.iter_mut().enumerate() {
            if role.is_some() && !complete(r) {
                *role = None;
            }
        }
        let train = roles.rows(Role::Training);
        let Response::Categorical { classes, codes } = &response else { unreachable!("checked above") };
        let fit = fit_rows(&x, codes, classes, &train, request.rule, request.priors)?;
        let g = classes.len();
        let predictions = Predictions::Probabilities(
            (0..table.n_rows())
                .map(|r| if complete(r) { fit.posterior(&row_values(&x, r)) } else { vec![f64::NAN; g] })
                .collect(),
        );
        let held_out = match roles.folds {
            Some((folds, seed)) => {
                let folds = folds.min(train.len()).max(2);
                let fold_of = fold_assignment(train.len(), folds, seed)?;
                let mut values = vec![vec![f64::NAN; g]; table.n_rows()];
                for f in 0..folds {
                    let fit_on: Vec<usize> =
                        train.iter().zip(&fold_of).filter(|(_, k)| **k != f).map(|(&r, _)| r).collect();
                    // A fold that leaves a class without rows predicts nothing.
                    let Ok(m) = fit_rows(&x, codes, classes, &fit_on, request.rule, request.priors) else { continue };
                    for (&r, _) in train.iter().zip(&fold_of).filter(|(_, k)| **k == f) {
                        values[r] = m.posterior(&row_values(&x, r));
                    }
                }
                Some(Predictions::Probabilities(values))
            }
            None => None,
        };
        Ok(Self {
            request: request.clone(),
            response_name,
            predictor_names,
            response,
            roles,
            x,
            fit,
            predictions,
            held_out,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &DiscriminantRequest {
        &self.request
    }

    /// The fitted model.
    #[must_use]
    pub fn fit(&self) -> &DiscriminantFit {
        &self.fit
    }

    fn classes(&self) -> &[String] {
        self.response.classes().unwrap_or(&[])
    }

    fn codes(&self) -> &[Option<usize>] {
        match &self.response {
            Response::Categorical { codes, .. } => codes,
            Response::Continuous(_) => &[],
        }
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let kind = match self.request.rule {
            DiscriminantRule::Linear => "Linear Discriminant Analysis",
            DiscriminantRule::Quadratic => "Quadratic Discriminant Analysis",
        };
        if self.predictor_names.len() > 6 {
            format!("{kind}: {} ~ {} predictors", self.response_name, self.predictor_names.len())
        } else {
            format!("{kind}: {} ~ {}", self.response_name, self.predictor_names.join(" + "))
        }
    }

    /// Scores on canonical function `k` for every table row (NaN where a
    /// predictor is missing).
    #[must_use]
    pub fn canonical_column(&self, k: usize) -> Vec<f64> {
        let n = self.x.first().map_or(0, Vec::len);
        (0..n)
            .map(|r| {
                let v = row_values(&self.x, r);
                if v.iter().all(|x| x.is_finite()) { self.fit.canonical_scores(&v)[k] } else { f64::NAN }
            })
            .collect()
    }

    /// Columns the report can save: each class's posterior probability, the
    /// most likely class, the validation roles, and the canonical scores
    /// (`canonical1`, `canonical2`, …).
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        let mut out = prediction_columns(self, &PredictionFormulas::default());
        for k in 0..self.fit.canonical.len() {
            out.push(ReportColumn::continuous(
                &format!("canonical{}", k + 1),
                &format!("Save canonical {} scores", k + 1),
                format!("Canonical {} {}", k + 1, self.response_name),
                self.canonical_column(k),
            ));
        }
        out
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary(), fit_measures_section(self)];
        out.extend(confusion_section(self));
        out.push(self.misclassified());
        out.push(self.means());
        if !self.fit.canonical.is_empty() {
            out.push(self.canonical_table());
            out.push(self.canonical_plot());
            out.push(self.coefficients());
            out.push(self.structure());
        }
        out
    }

    fn summary(&self) -> ReportSection {
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut push = |k: &str, v: String| t.push([k.to_owned(), v]);
        push("Response", self.response_name.clone());
        push("Rule", rule_label(self.request.rule).to_owned());
        push("Prior probabilities", self.request.priors.label().to_owned());
        push("Predictors", format!("{} ({})", self.predictor_names.len(), self.predictor_names.join(", ")));
        for role in Role::ALL {
            let n = self.roles.rows(role).len();
            if n > 0 || role == Role::Training {
                push(&format!("{} rows", role.label()), n.to_string());
            }
        }
        push("Validation", self.roles.description.clone());
        section(SectionKind::DiscriminantSummary, "Summary", SectionContent::Table(t))
    }

    fn means(&self) -> ReportSection {
        let mut heads = vec!["Class".to_owned(), "Training rows".to_owned(), "Prior".to_owned()];
        heads.extend(self.predictor_names.iter().map(|n| format!("Mean {n}")));
        let mut t = TextTable::new(heads);
        for (k, class) in self.classes().iter().enumerate() {
            let mut row = vec![class.clone(), self.fit.counts[k].to_string(), format_number(self.fit.priors[k], 4)];
            row.extend(self.fit.means[k].iter().map(|v| format_number(*v, DIGITS)));
            t.push(row);
        }
        section(SectionKind::DiscriminantMeans, "Class Means and Priors (training rows)", SectionContent::Table(t))
    }

    fn misclassified(&self) -> ReportSection {
        let title = "Misclassified Rows";
        let Predictions::Probabilities(p) = &self.predictions else { unreachable!("classification") };
        let mut t =
            TextTable::new(["Row", "Role", "Actual", "Predicted", "Posterior of predicted", "Posterior of actual"]);
        let codes = self.codes();
        let classes = self.classes();
        let mut total = 0;
        for (r, role) in self.roles.roles.iter().enumerate() {
            let (Some(role), Some(actual)) = (role, codes[r]) else { continue };
            if !p[r].iter().all(|v| v.is_finite()) {
                continue;
            }
            let predicted = most_probable(&p[r]);
            if predicted == actual {
                continue;
            }
            total += 1;
            if total <= MAX_LISTED {
                t.push([
                    (r + 1).to_string(),
                    role.label().to_owned(),
                    classes[actual].clone(),
                    classes[predicted].clone(),
                    format_number(p[r][predicted], 4),
                    format_number(p[r][actual], 4),
                ]);
            }
        }
        if total == 0 {
            return section(
                SectionKind::DiscriminantMisclassified,
                title,
                SectionContent::Note("None: every row with a role is assigned its actual class.".to_owned()),
            );
        }
        let title = if total > MAX_LISTED {
            format!("Misclassified Rows (first {MAX_LISTED} of {total})")
        } else {
            format!("Misclassified Rows ({total})")
        };
        section(SectionKind::DiscriminantMisclassified, &title, SectionContent::Table(t))
    }

    fn canonical_table(&self) -> ReportSection {
        let c = &self.fit.canonical;
        let mut t = TextTable::new([
            "Function",
            "Eigenvalue",
            "Percent",
            "Cumulative percent",
            "Canonical correlation",
            "Wilks' Λ (this and later)",
            "Chi-square",
            "DF",
            "Prob > ChiSq",
        ]);
        let mut cum = 0.0;
        for (k, prop) in c.proportions().iter().enumerate() {
            cum += prop;
            let w = c.tests[k];
            t.push([
                format!("Canonical {}", k + 1),
                format_number(c.eigenvalues[k], DIGITS),
                format!("{:.2}", 100.0 * prop),
                format!("{:.2}", 100.0 * cum),
                format_number(c.correlations[k], DIGITS),
                format_number(w.lambda, DIGITS),
                format_number(w.chi_square, DIGITS),
                format_number(w.df, 4),
                format_p_value(w.p_value),
            ]);
        }
        section(
            SectionKind::DiscriminantCanonical,
            "Canonical Discriminant Functions (eigenvalues of W⁻¹B; Bartlett's chi-square)",
            SectionContent::Table(t),
        )
    }

    fn canonical_plot(&self) -> ReportSection {
        let c = &self.fit.canonical;
        let codes = self.codes();
        let classes = self.classes();
        let two = c.len() >= 2;
        let mut points = Vec::new();
        let mut membership = Vec::new();
        for (r, role) in self.roles.roles.iter().enumerate() {
            let (Some(_), Some(k)) = (role, codes[r]) else { continue };
            let s = self.fit.canonical_scores(&row_values(&self.x, r));
            // With one function, classes are spread on the vertical axis.
            let y = if two { s[1] } else { k as f64 + 1.0 };
            points.push(RowPoint { row: r, x: s[0], y });
            membership.push(k);
        }
        let items = classes
            .iter()
            .enumerate()
            .map(|(k, name)| {
                let m = &c.centroids[k];
                (m[0], if two { m[1] } else { k as f64 + 1.0 }, format!("{name} mean"), 0)
            })
            .collect();
        let pct = c.proportions();
        let plot = PlotSpec {
            x_label: format!("Canonical 1 ({:.1}%)", 100.0 * pct[0]),
            y_label: if two { format!("Canonical 2 ({:.1}%)", 100.0 * pct[1]) } else { self.response_name.clone() },
            points,
            groups: Some(PointGroups {
                title: self.response_name.clone(),
                labels: classes.to_vec(),
                membership,
                lines: Vec::new(),
            }),
            y_ticks: (!two).then(|| classes.iter().enumerate().map(|(k, n)| (k as f64 + 1.0, n.clone())).collect()),
            annotations: Some(Annotations { groups: vec!["Class means".to_owned()], items, labels: Vec::new() }),
            ..PlotSpec::default()
        };
        let title = if two { "Canonical Plot (canonical 1 by canonical 2)" } else { "Canonical Plot (canonical 1)" };
        section(SectionKind::DiscriminantPlot, title, SectionContent::Plot(Box::new(plot)))
    }

    fn coefficients(&self) -> ReportSection {
        let c = &self.fit.canonical;
        let r = c.len();
        let mut heads = vec!["Variable".to_owned()];
        heads.extend((1..=r).map(|k| format!("Standardized {k}")));
        heads.extend((1..=r).map(|k| format!("Raw {k}")));
        let mut t = TextTable::new(heads);
        for (j, name) in self.predictor_names.iter().enumerate() {
            let mut row = vec![name.clone()];
            row.extend(c.standardized[j].iter().map(|v| format_number(*v, DIGITS)));
            row.extend(c.raw[j].iter().map(|v| format_number(*v, DIGITS)));
            t.push(row);
        }
        let mut constant = vec!["Constant".to_owned()];
        constant.extend((0..r).map(|_| String::new()));
        constant.extend((0..r).map(|k| {
            let v: f64 = -(0..self.predictor_names.len()).map(|j| self.fit.grand_mean[j] * c.raw[j][k]).sum::<f64>();
            format_number(v, DIGITS)
        }));
        t.push(constant);
        section(
            SectionKind::DiscriminantCoefficients,
            "Canonical Coefficients (raw: unit pooled within-class variance)",
            SectionContent::Table(t),
        )
    }

    fn structure(&self) -> ReportSection {
        let c = &self.fit.canonical;
        let mut heads = vec!["Variable".to_owned()];
        heads.extend((1..=c.len()).map(|k| format!("Canonical {k}")));
        let mut t = TextTable::new(heads);
        for (j, name) in self.predictor_names.iter().enumerate() {
            let mut row = vec![name.clone()];
            row.extend(c.structure[j].iter().map(|v| format_number(*v, 4)));
            t.push(row);
        }
        section(
            SectionKind::DiscriminantStructure,
            "Structure Correlations (pooled within-class)",
            SectionContent::Table(t),
        )
    }
}

impl Predictor for DiscriminantAnalysis {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::flowers;

    fn request(t: &DataTable) -> DiscriminantRequest {
        DiscriminantRequest::new(
            t.find("Variety").unwrap(),
            ["Sepal length", "Sepal width", "Petal length", "Petal width"].iter().map(|c| t.find(c).unwrap()).collect(),
        )
    }

    #[test]
    fn linear_rule_with_sections_and_saved_columns() {
        let t = flowers();
        let rows = RowStateManager::new(t.n_rows());
        let a = DiscriminantAnalysis::run(&t, &rows, &request(&t)).unwrap();
        assert!(a.title().starts_with("Linear Discriminant Analysis: Variety ~ Sepal length"));
        let kinds: Vec<SectionKind> = a.sections().iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::DiscriminantSummary,
                SectionKind::FitByRole,
                SectionKind::ConfusionMatrix,
                SectionKind::DiscriminantMisclassified,
                SectionKind::DiscriminantMeans,
                SectionKind::DiscriminantCanonical,
                SectionKind::DiscriminantPlot,
                SectionKind::DiscriminantCoefficients,
                SectionKind::DiscriminantStructure,
            ]
        );
        let Predictions::Probabilities(p) = a.predictions() else { panic!() };
        assert!(p.iter().all(|r| (r.iter().sum::<f64>() - 1.0).abs() < 1e-12));
        let keys: Vec<String> = a.saveable_columns().iter().map(|c| c.key.clone()).collect();
        assert_eq!(
            keys,
            [
                "probability_1",
                "probability_2",
                "probability_3",
                "most_likely",
                "validation",
                "canonical1",
                "canonical2"
            ]
        );
        // The canonical plot: one point per row, centroids as annotations.
        let s = a.sections();
        let SectionContent::Plot(plot) = &s[6].content else { panic!() };
        assert_eq!(plot.points.len(), t.n_rows());
        assert_eq!(plot.annotations.as_ref().unwrap().items.len(), 3);
        // Canonical scores of the rows average to each class's centroid.
        let can1 = a.canonical_column(0);
        let codes = a.codes().to_vec();
        for k in 0..3 {
            let v: Vec<f64> = (0..t.n_rows()).filter(|&r| codes[r] == Some(k)).map(|r| can1[r]).collect();
            let mean = v.iter().sum::<f64>() / v.len() as f64;
            assert!((mean - a.fit().canonical.centroids[k][0]).abs() < 1e-9);
        }
    }

    #[test]
    fn validation_roles_k_fold_quadratic_and_json() {
        let t = flowers();
        let rows = RowStateManager::new(t.n_rows());
        let req = DiscriminantRequest {
            rule: DiscriminantRule::Quadratic,
            priors: PriorChoice::Equal,
            validation: ValidationMethod::Holdout { validation: 0.3, test: 0.0, seed: 4, stratify: true, group: None },
            ..request(&t)
        };
        let a = DiscriminantAnalysis::run(&t, &rows, &req).unwrap();
        assert!(a.title().starts_with("Quadratic"));
        let s = a.sections();
        let SectionContent::Table(fit) = &s[1].content else { panic!() };
        assert_eq!(fit.rows.iter().map(|r| r[0].as_str()).collect::<Vec<_>>(), ["Training", "Validation"]);
        assert!(a.fit().priors.iter().all(|p| (p - 1.0 / 3.0).abs() < 1e-12));
        let back = DiscriminantRequest::from_json(&req.to_json(&t), &t).unwrap();
        assert_eq!(back, req);
        let kf = DiscriminantRequest { validation: ValidationMethod::KFold { folds: 5, seed: 2 }, ..request(&t) };
        let b = DiscriminantAnalysis::run(&t, &rows, &kf).unwrap();
        let Some(Predictions::Probabilities(cv)) = b.cross_validated() else { panic!("held out") };
        assert!(cv.iter().all(|r| r.iter().all(|v| v.is_finite())));
        assert_eq!(DiscriminantRequest::from_json(&kf.to_json(&t), &t).unwrap(), kf);
        // A continuous response or a text predictor is refused.
        let cont = DiscriminantRequest::new(t.find("Sepal length").unwrap(), vec![t.find("Petal length").unwrap()]);
        assert!(DiscriminantAnalysis::run(&t, &rows, &cont).is_err());
        let text = DiscriminantRequest::new(t.find("Variety").unwrap(), vec![t.find("Variety").unwrap()]);
        assert!(DiscriminantAnalysis::run(&t, &rows, &text).is_err());
    }
}
