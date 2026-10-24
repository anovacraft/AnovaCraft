//! Partial least squares regression bound to a dataset
//! ([`stat_predictive::pls`]), with the shared predictive-model sections
//! ([`crate::prediction`]).
//!
//! **Responses.** A continuous response gives PLS regression. A categorical
//! response gives PLS discriminant analysis (PLS-DA): one 0/1 indicator per
//! class is modelled together, and a row is assigned the class with the
//! largest predicted indicator. For the fit measures, the predicted
//! indicators are clipped to [0, 1] and rescaled to sum to 1; they rank rows
//! like probabilities but are not calibrated.
//!
//! **Rows.** Predictors must be numeric. Rows that are excluded, miss the
//! response or miss any predictor are left out (PLS has no rule for missing
//! values); their predictions are missing.
//!
//! **Number of factors.** The fewest factors whose held-out mean squared
//! error is within one standard error of the smallest
//! ([`stat_predictive::choose_factors`]): held out on the validation rows
//! when there are any, otherwise by k-fold cross-validation on the training
//! rows (the k-fold method's folds, or 10 folds). The predicted residual
//! sum of squares (PRESS) is shown factor by factor. A fixed number may be
//! set instead.

use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_linear::TermValue;
use stat_predictive::{
    Feature, PlsAlgorithm, PlsFit, Role, choose_factors, cross_validate_pls, fit_pls, fold_assignment,
};

use crate::format::format_number;
use crate::prediction::{
    FeatureSet, PredictionFormulas, Predictions, Predictor, PredictorInfo, Response, RoleAssignment, ValidationMethod,
    confusion_section, fit_measures_section, prediction_columns,
};
use crate::report::{
    Annotations, PlotSpec, PointGroups, ReportSection, RowPoint, SectionContent, SectionKind, TextTable,
};
use crate::surface_profiler::{Observation, ProfileFactor, SurfaceModel};

const DIGITS: usize = 6;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Columns and settings of a PLS model.
#[derive(Debug, Clone, PartialEq)]
pub struct PlsRequest {
    /// Response column.
    pub response: usize,
    /// Predictor columns (numeric).
    pub predictors: Vec<usize>,
    /// Training / validation / test roles.
    pub validation: ValidationMethod,
    /// Most factors considered.
    pub max_factors: usize,
    /// A fixed number of factors (`None`: chosen by validation rows or
    /// cross-validation).
    pub factors: Option<usize>,
    /// Factor extraction.
    pub algorithm: PlsAlgorithm,
    /// Standardize the predictors to unit variance.
    pub scale: bool,
}

impl PlsRequest {
    /// Up to 10 factors chosen automatically, SIMPLS, standardized
    /// predictors, every row training.
    #[must_use]
    pub fn new(response: usize, predictors: Vec<usize>) -> Self {
        Self {
            response,
            predictors,
            validation: ValidationMethod::None,
            max_factors: 10,
            factors: None,
            algorithm: PlsAlgorithm::Simpls,
            scale: true,
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

/// How the number of factors was chosen.
#[derive(Debug, Clone, PartialEq)]
enum FactorChoice {
    Fixed,
    Validation,
    CrossValidation { folds: usize },
}

/// A fitted PLS model.
#[derive(Debug, Clone)]
pub struct PlsAnalysis {
    request: PlsRequest,
    response_name: String,
    profile_name: String,
    response: Response,
    roles: RoleAssignment,
    predictors: Vec<PredictorInfo>,
    /// Predictor values of every table row (NaN missing).
    x: Vec<Vec<f64>>,
    /// Rows the model was fitted to.
    fitted_rows: Vec<usize>,
    fit: PlsFit,
    factors: usize,
    choice: FactorChoice,
    /// Held-out PRESS for 0..=max factors (validation rows or
    /// cross-validation), and the number of rows it sums over.
    press: Option<(Vec<f64>, usize)>,
    predictions: Predictions,
    held_out: Option<Predictions>,
    profile_class: usize,
}

/// Response columns: the response itself, or one indicator per class.
fn response_columns(response: &Response, rows: &[usize]) -> Vec<Vec<f64>> {
    match response {
        Response::Continuous(y) => vec![rows.iter().map(|&r| y[r]).collect()],
        Response::Categorical { classes, codes } => (0..classes.len())
            .map(|k| rows.iter().map(|&r| f64::from(u8::from(codes[r] == Some(k)))).collect())
            .collect(),
    }
}

/// Class scores as pseudo-probabilities: clipped to [0, 1] and rescaled to
/// sum to 1 (uniform when all are 0).
fn pseudo_probabilities(scores: &[f64]) -> Vec<f64> {
    let clipped: Vec<f64> = scores.iter().map(|s| s.clamp(0.0, 1.0)).collect();
    let total: f64 = clipped.iter().sum();
    if total > 0.0 {
        clipped.iter().map(|c| c / total).collect()
    } else {
        vec![1.0 / scores.len().max(1) as f64; scores.len()]
    }
}

impl PlsAnalysis {
    /// Fit the model.
    ///
    /// # Errors
    /// No predictors, the response among them, a non-numeric predictor, too
    /// few classes, an invalid validation method, too few complete training
    /// rows, or predictors without variation.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &PlsRequest) -> Result<Self> {
        if request.predictors.is_empty() {
            return Err(invalid("choose at least one predictor"));
        }
        if request.predictors.contains(&request.response) {
            return Err(invalid("the response cannot also be a predictor"));
        }
        if request.max_factors == 0 || request.factors == Some(0) {
            return Err(invalid("PLS needs at least one factor"));
        }
        let included: Vec<usize> = (0..table.n_rows()).filter(|&r| !states.is_excluded(r)).collect();
        let response = Response::read(table, request.response, &included)?;
        let response_name = table.column(request.response).map_or(String::new(), |c| c.name.clone());
        let mut roles = RoleAssignment::assign(table, states, &response, &request.validation)?;
        let all_train = roles.rows(Role::Training);
        let FeatureSet { predictors, features } = FeatureSet::read(table, &request.predictors, &all_train)?;
        let mut x = Vec::with_capacity(features.len());
        for (info, f) in predictors.iter().zip(features) {
            match f {
                Feature::Numeric(v) => x.push(v),
                Feature::Categorical { .. } => {
                    return Err(invalid(format!(
                        "PLS uses numeric predictors; {:?} is categorical (use indicator columns, or set its \
                         measurement level to continuous)",
                        info.name
                    )));
                }
            }
        }
        // Rows missing a predictor take no role.
        let complete = |r: usize| x.iter().all(|c| c[r].is_finite());
        for (r, role) in roles.roles.iter_mut().enumerate() {
            if role.is_some() && !complete(r) {
                *role = None;
            }
        }
        let train = roles.rows(Role::Training);
        let valid = roles.rows(Role::Validation);
        if train.len() < 3 {
            return Err(invalid("PLS needs at least three training rows with every predictor present"));
        }
        let pick =
            |rows: &[usize]| -> Vec<Vec<f64>> { x.iter().map(|c| rows.iter().map(|&r| c[r]).collect()).collect() };
        let max = request.max_factors.min(x.len()).min(train.len() - 1);
        let scale_y = false;
        let y_train = response_columns(&response, &train);
        let fit = fit_pls(&pick(&train), &y_train, max, request.scale, scale_y, request.algorithm)?;
        let max = fit.factors();
        let width = response.classes().map_or(1, <[String]>::len);
        let row_x = |r: usize| -> Vec<f64> { x.iter().map(|c| c[r]).collect() };

        let (press, choice, fold_info) = if let Some(f) = request.factors {
            // With k-fold roles the folds still give held-out predictions.
            let fold_of = match roles.folds {
                Some((folds, seed)) => Some(fold_assignment(train.len(), folds.min(train.len()).max(2), seed)?),
                None => None,
            };
            (None, FactorChoice::Fixed, (f.min(max), fold_of))
        } else if !valid.is_empty() {
            let y_valid = response_columns(&response, &valid);
            let errors: Vec<Vec<f64>> = (0..=max)
                .map(|a| {
                    valid
                        .iter()
                        .enumerate()
                        .map(|(i, &r)| {
                            let p = fit.predict(a, &row_x(r));
                            y_valid.iter().zip(&p).map(|(c, pk)| (c[i] - pk).powi(2)).sum::<f64>()
                        })
                        .collect()
                })
                .collect();
            let best = choose_factors(&errors);
            let press = errors.iter().map(|e| e.iter().sum()).collect();
            (Some((press, valid.len())), FactorChoice::Validation, (best, None))
        } else {
            let (folds, seed) = roles.folds.unwrap_or((10, 1));
            let folds = folds.min(train.len()).max(2);
            let fold_of = fold_assignment(train.len(), folds, seed)?;
            let errors =
                cross_validate_pls(&pick(&train), &y_train, &fold_of, max, request.scale, scale_y, request.algorithm)?;
            let best = choose_factors(&errors);
            let press = errors.iter().map(|e| e.iter().sum()).collect();
            (Some((press, train.len())), FactorChoice::CrossValidation { folds }, (best, Some(fold_of)))
        };
        let (factors, fold_of) = fold_info;
        let factors = factors.max(1);

        let to_prediction = |p: Vec<f64>| if response.classes().is_some() { pseudo_probabilities(&p) } else { p };
        let wrap = |values: Vec<Vec<f64>>| match &response {
            Response::Continuous(_) => Predictions::Values(values.into_iter().map(|v| v[0]).collect()),
            Response::Categorical { .. } => Predictions::Probabilities(values),
        };
        let predictions =
            wrap(
                (0..table.n_rows())
                    .map(|r| {
                        if complete(r) { to_prediction(fit.predict(factors, &row_x(r))) } else { vec![f64::NAN; width] }
                    })
                    .collect(),
            );
        // Cross-validated predictions with the chosen number of factors (k-fold only).
        let held_out = match (&fold_of, roles.folds) {
            (Some(fold_of), Some(_)) => {
                let mut values = vec![vec![f64::NAN; width]; table.n_rows()];
                let folds = fold_of.iter().copied().max().map_or(0, |m| m + 1);
                for f in 0..folds {
                    let fit_rows: Vec<usize> =
                        train.iter().zip(fold_of).filter(|(_, k)| **k != f).map(|(&r, _)| r).collect();
                    if fit_rows.len() < 2 {
                        continue;
                    }
                    let m = fit_pls(
                        &pick(&fit_rows),
                        &response_columns(&response, &fit_rows),
                        factors,
                        request.scale,
                        scale_y,
                        request.algorithm,
                    )?;
                    for (&r, _) in train.iter().zip(fold_of).filter(|(_, k)| **k == f) {
                        values[r] = to_prediction(m.predict(factors, &row_x(r)));
                    }
                }
                Some(wrap(values))
            }
            _ => None,
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
            Some(c) => format!("Score {response_name} = {}", c[profile_class]),
        };
        Ok(Self {
            request: request.clone(),
            response_name,
            profile_name,
            response,
            roles,
            predictors,
            x,
            fitted_rows: train,
            fit,
            factors,
            choice,
            press,
            predictions,
            held_out,
            profile_class,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &PlsRequest {
        &self.request
    }

    /// The fitted model (every factor extracted).
    #[must_use]
    pub fn fit(&self) -> &PlsFit {
        &self.fit
    }

    /// Number of factors used for predictions.
    #[must_use]
    pub fn factors(&self) -> usize {
        self.factors
    }

    fn is_classification(&self) -> bool {
        self.response.classes().is_some()
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let names: Vec<&str> = self.predictors.iter().map(|p| p.name.as_str()).collect();
        let kind = if self.is_classification() { "PLS Discriminant Analysis" } else { "Partial Least Squares" };
        if names.len() > 6 {
            format!("{kind}: {} ~ {} predictors", self.response_name, names.len())
        } else {
            format!("{kind}: {} ~ {}", self.response_name, names.join(" + "))
        }
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary_section(), self.factor_table()];
        out.extend(self.press_plot());
        out.push(fit_measures_section(self));
        out.extend(confusion_section(self));
        out.push(self.vip_section());
        out.push(self.coefficient_section());
        out.push(self.loading_section());
        out.extend(self.score_plot());
        out
    }

    fn summary_section(&self) -> ReportSection {
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut row = |a: &str, b: String| t.push([a.to_owned(), b]);
        row("Response", self.response_name.clone());
        row(
            "Model",
            match self.response.classes() {
                Some(c) => format!(
                    "PLS discriminant analysis: one indicator per class ({} classes), largest predicted indicator wins",
                    c.len()
                ),
                None => "PLS regression".to_owned(),
            },
        );
        row(
            "Algorithm",
            match self.request.algorithm {
                PlsAlgorithm::Nipals => "NIPALS".to_owned(),
                PlsAlgorithm::Simpls => "SIMPLS (de Jong 1993)".to_owned(),
            },
        );
        row(
            "Predictors",
            format!(
                "{} ({})",
                self.predictors.len(),
                if self.request.scale { "centered and scaled to unit variance" } else { "centered" }
            ),
        );
        for role in Role::ALL {
            let n = self.roles.rows(role).len();
            if n > 0 || role == Role::Training {
                row(&format!("{} rows", role.label()), n.to_string());
            }
        }
        row(
            "Factors",
            format!(
                "{} of {} ({})",
                self.factors,
                self.fit.factors(),
                match &self.choice {
                    FactorChoice::Fixed => "set in the request".to_owned(),
                    FactorChoice::Validation => {
                        "fewest within one standard error of the smallest PRESS on the validation rows".to_owned()
                    }
                    FactorChoice::CrossValidation { folds } => {
                        format!("fewest within one standard error of the smallest {folds}-fold cross-validated PRESS")
                    }
                }
            ),
        );
        let cum = |v: &[f64]| v.iter().take(self.factors).sum::<f64>();
        row("X variance explained", format!("{}%", format_number(100.0 * cum(&self.fit.x_explained), 4)));
        row(
            if self.is_classification() { "Class indicator variance explained" } else { "Y variance explained" },
            format!("{}%", format_number(100.0 * cum(&self.fit.y_explained), 4)),
        );
        ReportSection {
            kind: SectionKind::PlsSummary,
            title: "PLS Summary".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    /// Held-out root mean squared error per factor count.
    fn held_out_rmse(&self) -> Option<Vec<f64>> {
        let (press, n) = self.press.as_ref()?;
        let width = self.response.classes().map_or(1, <[String]>::len);
        Some(press.iter().map(|p| (p / (*n * width).max(1) as f64).sqrt()).collect())
    }

    fn held_out_name(&self) -> &'static str {
        match self.choice {
            FactorChoice::Validation => "Validation",
            _ => "Cross-validated",
        }
    }

    fn factor_table(&self) -> ReportSection {
        let rmse = self.held_out_rmse();
        let mut cols = vec![
            "Factors".to_owned(),
            "X explained".to_owned(),
            "X cumulative".to_owned(),
            "Y explained".to_owned(),
            "Y cumulative".to_owned(),
        ];
        if rmse.is_some() {
            cols.push("PRESS".to_owned());
            cols.push(format!("{} RMSE", self.held_out_name()));
        }
        cols.push(String::new());
        let mut t = TextTable::new(cols);
        let pct = |v: f64| format!("{}%", format_number(100.0 * v, 4));
        let (mut cx, mut cy) = (0.0, 0.0);
        for a in 0..=self.fit.factors() {
            let mut row = vec![a.to_string()];
            if a == 0 {
                row.extend(["", "", "", ""].map(str::to_owned));
            } else {
                cx += self.fit.x_explained[a - 1];
                cy += self.fit.y_explained[a - 1];
                row.extend([pct(self.fit.x_explained[a - 1]), pct(cx), pct(self.fit.y_explained[a - 1]), pct(cy)]);
            }
            if let (Some((press, _)), Some(r)) = (&self.press, &rmse) {
                row.push(format_number(press[a], DIGITS));
                row.push(format_number(r[a], DIGITS));
            }
            row.push(if a == self.factors { "◀ used".to_owned() } else { String::new() });
            t.push(row);
        }
        ReportSection {
            kind: SectionKind::PlsFactors,
            title: "Variance Explained by Factor".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    fn press_plot(&self) -> Option<ReportSection> {
        let rmse = self.held_out_rmse()?;
        let line: Vec<[f64; 2]> = rmse.iter().enumerate().map(|(a, &v)| [a as f64, v]).collect();
        let chosen = line[self.factors];
        let name = self.held_out_name();
        let spec = PlotSpec {
            x_label: "Factors".to_owned(),
            y_label: format!("{name} RMSE"),
            groups: Some(PointGroups {
                title: "Error".to_owned(),
                labels: vec![format!("{name} RMSE")],
                membership: Vec::new(),
                lines: vec![line],
            }),
            annotations: Some(Annotations {
                groups: vec!["Factors used".to_owned()],
                items: vec![(chosen[0], chosen[1], format!("used: {} factors", self.factors), 0)],
                labels: Vec::new(),
            }),
            ..PlotSpec::default()
        };
        Some(ReportSection {
            kind: SectionKind::PlsPress,
            title: format!("{name} Error by Number of Factors"),
            content: SectionContent::Plot(Box::new(spec)),
        })
    }

    fn vip_section(&self) -> ReportSection {
        let vip = self.fit.vip(self.factors);
        let mut order: Vec<usize> = (0..vip.len()).collect();
        order.sort_by(|&a, &b| vip[b].total_cmp(&vip[a]));
        let mut t = TextTable::new(["Predictor", "VIP", "Reading"]);
        for j in order {
            let reading = if vip[j] >= 1.0 {
                "important"
            } else if vip[j] >= 0.8 {
                "moderate"
            } else {
                "minor"
            };
            t.push([self.predictors[j].name.clone(), format_number(vip[j], 4), reading.to_owned()]);
        }
        ReportSection {
            kind: SectionKind::VariableImportance,
            title: format!(
                "Variable Importance in Projection ({} factors; above 1 important, below 0.8 minor)",
                self.factors
            ),
            content: SectionContent::Table(t),
        }
    }

    fn coefficient_section(&self) -> ReportSection {
        let (b0, b) = self.fit.coefficients(self.factors);
        let heads: Vec<String> = match self.response.classes() {
            Some(c) => c.iter().map(|k| format!("Indicator {k}")).collect(),
            None => vec![self.response_name.clone()],
        };
        let mut cols = vec!["Term".to_owned()];
        cols.extend(heads.iter().cloned());
        if !self.is_classification() {
            cols.push("Standardized".to_owned());
        }
        let mut t = TextTable::new(cols);
        let mut first = vec!["Intercept".to_owned()];
        first.extend(b0.iter().map(|v| format_number(*v, DIGITS)));
        if !self.is_classification() {
            first.push(String::new());
        }
        t.push(first);
        let y_sd = {
            let y = response_columns(&self.response, &self.fitted_rows);
            let n = y[0].len() as f64;
            let m = y[0].iter().sum::<f64>() / n;
            (y[0].iter().map(|v| (v - m).powi(2)).sum::<f64>() / (n - 1.0).max(1.0)).sqrt()
        };
        for (j, p) in self.predictors.iter().enumerate() {
            let mut row = vec![p.name.clone()];
            row.extend(b[j].iter().map(|v| format_number(*v, DIGITS)));
            if !self.is_classification() {
                // Slope per standard deviation of the predictor, in response SDs.
                let x_sd = {
                    let c: Vec<f64> = self.fitted_rows.iter().map(|&r| self.x[j][r]).collect();
                    let n = c.len() as f64;
                    let m = c.iter().sum::<f64>() / n;
                    (c.iter().map(|v| (v - m).powi(2)).sum::<f64>() / (n - 1.0).max(1.0)).sqrt()
                };
                row.push(if y_sd > 0.0 { format_number(b[j][0] * x_sd / y_sd, 4) } else { String::new() });
            }
            t.push(row);
        }
        ReportSection {
            kind: SectionKind::PlsCoefficients,
            title: format!("Coefficients ({} factors, original units)", self.factors),
            content: SectionContent::Table(t),
        }
    }

    fn loading_section(&self) -> ReportSection {
        let shown = self.factors.max(2).min(self.fit.factors());
        let mut cols = vec!["Predictor".to_owned()];
        for a in 1..=shown {
            cols.push(format!("Weight {a}"));
        }
        for a in 1..=shown {
            cols.push(format!("Loading {a}"));
        }
        let mut t = TextTable::new(cols);
        for (j, p) in self.predictors.iter().enumerate() {
            let mut row = vec![p.name.clone()];
            for a in 0..shown {
                let w = &self.fit.weights[a];
                let nw = w.iter().map(|v| v * v).sum::<f64>().sqrt();
                row.push(format_number(if nw > 0.0 { w[j] / nw } else { 0.0 }, 4));
            }
            for a in 0..shown {
                row.push(format_number(self.fit.x_loadings[a][j], 4));
            }
            t.push(row);
        }
        ReportSection {
            kind: SectionKind::PlsLoadings,
            title: "X Weights and Loadings (scaled predictors)".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    fn score_plot(&self) -> Option<ReportSection> {
        if self.fit.factors() < 2 {
            return None;
        }
        let rows: Vec<usize> = (0..self.roles.roles.len()).filter(|&r| self.roles.roles[r].is_some()).collect();
        let points: Vec<RowPoint> = rows
            .iter()
            .map(|&r| {
                let s = self.fit.transform(2, &self.x.iter().map(|c| c[r]).collect::<Vec<_>>());
                RowPoint { row: r, x: s[0], y: s[1] }
            })
            .collect();
        let groups = match &self.response {
            Response::Categorical { classes, codes } => Some(PointGroups {
                title: self.response_name.clone(),
                labels: classes.clone(),
                membership: rows.iter().map(|&r| codes[r].unwrap_or(0)).collect(),
                lines: Vec::new(),
            }),
            Response::Continuous(_) => None,
        };
        // Weight arrows, scaled to the score range, from the origin.
        let reach = points.iter().map(|p| p.x.abs().max(p.y.abs())).fold(0.0, f64::max);
        let (w1, w2) = (&self.fit.weights[0], &self.fit.weights[1]);
        let longest = (0..w1.len()).map(|j| w1[j].hypot(w2[j])).fold(0.0, f64::max);
        let segments = if longest > 0.0 {
            let s = 0.8 * reach / longest;
            (0..w1.len()).map(|j| vec![[0.0, 0.0], [s * w1[j], s * w2[j]]]).collect()
        } else {
            Vec::new()
        };
        let pct = |v: f64| format_number(100.0 * v, 3);
        let spec = PlotSpec {
            x_label: format!(
                "Factor 1 score (X {}%, Y {}%)",
                pct(self.fit.x_explained[0]),
                pct(self.fit.y_explained[0])
            ),
            y_label: format!(
                "Factor 2 score (X {}%, Y {}%)",
                pct(self.fit.x_explained[1]),
                pct(self.fit.y_explained[1])
            ),
            points,
            groups,
            segments,
            ..PlotSpec::default()
        };
        Some(ReportSection {
            kind: SectionKind::ScorePlot,
            title: "Score Plot (lines show the predictors' weights)".to_owned(),
            content: SectionContent::Plot(Box::new(spec)),
        })
    }

    /// The prediction equation of a continuous response as a formula.
    fn prediction_formula(&self) -> Option<String> {
        if self.is_classification() {
            return None;
        }
        let (b0, b) = self.fit.coefficients(self.factors);
        let mut f = crate::script_text::number(b0[0]);
        for (p, row) in self.predictors.iter().zip(&b) {
            f.push_str(&format!(
                " + {} * {}",
                crate::script_text::number(row[0]),
                crate::formula::column_reference(&p.name)
            ));
        }
        Some(f)
    }

    /// Columns the report can save: predicted values (also as a formula
    /// column) or class scores as pseudo-probabilities and the most likely
    /// class, and the validation roles.
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<crate::ReportColumn> {
        let formulas = PredictionFormulas { prediction: self.prediction_formula(), probabilities: Vec::new() };
        prediction_columns(self, &formulas)
    }
}

impl Predictor for PlsAnalysis {
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

impl SurfaceModel for PlsAnalysis {
    // For PLS-DA the profilers show one class's predicted indicator.
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
                let x: Vec<f64> = s
                    .iter()
                    .map(|v| match v {
                        TermValue::Number(x) => *x,
                        TermValue::Level(_) => f64::NAN,
                    })
                    .collect();
                Ok(self.fit.predict(self.factors, &x)[self.profile_class])
            })
            .collect()
    }

    fn observations(&self) -> Vec<Observation> {
        let set = FeatureSet {
            predictors: Vec::new(),
            features: self.x.iter().map(|c| Feature::Numeric(c.clone())).collect(),
        };
        set.observations(&self.fitted_rows, &|r| match &self.response {
            Response::Continuous(y) => y[r],
            Response::Categorical { codes, .. } => f64::from(u8::from(codes[r] == Some(self.profile_class))),
        })
    }

    fn profile_note(&self) -> Option<String> {
        Some(format!(
            "Linear in the predictors ({} factors); no confidence bands.{}",
            self.factors,
            if self.is_classification() {
                format!(" Shown: the predicted indicator {} (not a calibrated probability).", self.profile_name)
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
    request: &PlsRequest,
    col: &dyn Fn(usize) -> JsonValue,
) -> Vec<(&'static str, JsonValue)> {
    vec![
        ("validation", request.validation.to_json(col)),
        ("max_factors", JsonValue::Number(request.max_factors as f64)),
        ("factors", request.factors.map_or(JsonValue::Null, |f| JsonValue::Number(f as f64))),
        (
            "algorithm",
            JsonValue::String(if request.algorithm == PlsAlgorithm::Nipals { "nipals" } else { "simpls" }.to_owned()),
        ),
        ("scale", JsonValue::Bool(request.scale)),
    ]
}

/// Read a request from a project file's report entry (missing members take
/// their defaults).
///
/// # Errors
/// An unknown algorithm or validation method, or a missing column.
pub fn request_from_json(
    v: &JsonValue,
    response: usize,
    predictors: Vec<usize>,
    find: &dyn Fn(&JsonValue) -> Result<usize>,
) -> Result<PlsRequest> {
    let mut r = PlsRequest::new(response, predictors);
    r.validation = ValidationMethod::from_json(v.get("validation"), find)?;
    r.max_factors = v.get("max_factors").and_then(JsonValue::as_usize).unwrap_or(r.max_factors);
    r.factors = v.get("factors").and_then(JsonValue::as_usize);
    r.scale = v.get("scale").and_then(JsonValue::as_bool).unwrap_or(r.scale);
    if let Some(a) = v.get("algorithm").and_then(JsonValue::as_str) {
        r.algorithm = match a {
            "nipals" => PlsAlgorithm::Nipals,
            "simpls" => PlsAlgorithm::Simpls,
            other => return Err(invalid(format!("partial least squares: unknown algorithm {other:?}"))),
        };
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    /// 80 rows: 12 correlated "spectral" predictors from two latent
    /// constituents, a continuous response and a class.
    fn table() -> DataTable {
        let mut rng = stat_core::rng::Rng::new(8);
        let n = 80;
        let z: Vec<[f64; 2]> = (0..n).map(|_| [rng.normal(), rng.normal()]).collect();
        let mut cols = Vec::new();
        for j in 0..12 {
            let a = (j as f64 / 3.0).sin();
            let b = (j as f64 / 5.0).cos();
            cols.push(Column::numeric(
                format!("w{j}"),
                z.iter().map(|zi| a * zi[0] + b * zi[1] + 0.05 * rng.normal()).collect(),
            ));
        }
        cols.push(Column::numeric("y", z.iter().map(|zi| 5.0 + 2.0 * zi[0] - zi[1] + 0.1 * rng.normal()).collect()));
        // Two classes split by a line in the latent space.
        cols.push(Column::text(
            "class",
            z.iter().map(|zi| if zi[0] + 0.5 * zi[1] > 0.0 { "Pass" } else { "Fail" }.to_owned()).collect(),
        ));
        cols.push(Column::text(
            "role",
            (0..n).map(|i| ["Training", "Training", "Validation", "Test"][i % 4].to_owned()).collect(),
        ));
        DataTable::new("spectra", cols).unwrap()
    }

    #[test]
    fn regression_chooses_few_factors_and_saves_a_formula() {
        let t = table();
        let rows = RowStateManager::new(t.n_rows());
        let req = PlsRequest::new(12, (0..12).collect());
        let a = PlsAnalysis::run(&t, &rows, &req).unwrap();
        // Two latent constituents, plus small factors for the noise in the
        // predictors themselves.
        assert!((2..=4).contains(&a.factors()), "{} factors", a.factors());
        assert!(a.title().starts_with("Partial Least Squares: y ~ 12 predictors"));
        let kinds: Vec<SectionKind> = a.sections().iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::PlsSummary,
                SectionKind::PlsFactors,
                SectionKind::PlsPress,
                SectionKind::FitByRole,
                SectionKind::VariableImportance,
                SectionKind::PlsCoefficients,
                SectionKind::PlsLoadings,
                SectionKind::ScorePlot,
            ]
        );
        // The saved formula reproduces the predictions.
        let cols = a.saveable_columns();
        let predicted = &cols[0];
        assert_eq!(predicted.key, "predicted");
        let formula = predicted.formula.as_ref().unwrap();
        let computed = crate::formula::evaluate(&t, None, formula, 0..t.n_rows()).unwrap().values;
        for (c, v) in computed.iter().zip(&predicted.values) {
            let crate::formula::FormulaValue::Number(c) = c else { panic!("{c:?}") };
            assert!((c - v).abs() < 1e-9 * v.abs().max(1.0), "{c} {v}");
        }
        // A fixed number of factors with k-fold roles still gives held-out
        // predictions.
        let fixed = PlsRequest {
            factors: Some(2),
            validation: ValidationMethod::KFold { folds: 5, seed: 3 },
            ..PlsRequest::new(12, (0..12).collect())
        };
        let b = PlsAnalysis::run(&t, &rows, &fixed).unwrap();
        assert_eq!(b.factors(), 2);
        let Some(Predictions::Values(cv)) = b.cross_validated() else { panic!("no held-out predictions") };
        assert!(cv.iter().all(|v| v.is_finite()));
        // Profiler moves with the dominant predictor.
        let s = a.default_profile_settings();
        let p = a.predict_settings(std::slice::from_ref(&s)).unwrap();
        assert!(p[0].is_finite());
    }

    #[test]
    fn discriminant_analysis_with_validation_rows_and_json() {
        let t = table();
        let rows = RowStateManager::new(t.n_rows());
        let mut req = PlsRequest::new(13, (0..12).collect());
        req.validation = ValidationMethod::Column(14);
        req.algorithm = PlsAlgorithm::Nipals;
        let a = PlsAnalysis::run(&t, &rows, &req).unwrap();
        assert!(a.title().starts_with("PLS Discriminant Analysis: class"));
        let s = a.sections();
        assert!(s.iter().any(|x| x.kind == SectionKind::ConfusionMatrix));
        let SectionContent::Table(fit) = &s.iter().find(|x| x.kind == SectionKind::FitByRole).unwrap().content else {
            panic!()
        };
        assert_eq!(fit.rows.iter().map(|r| r[0].as_str()).collect::<Vec<_>>(), ["Training", "Validation", "Test"]);
        let err = fit.rows[2][2].parse::<f64>().unwrap();
        assert!(err < 0.15, "test misclassification {err}");
        let Predictions::Probabilities(p) = a.predictions() else { panic!() };
        assert!(p.iter().all(|row| (row.iter().sum::<f64>() - 1.0).abs() < 1e-12));
        assert!(a.saveable_columns().iter().all(|c| c.formula.is_none()));
        // JSON round trip.
        req.factors = Some(3);
        req.scale = false;
        let col = |i: usize| JsonValue::String(t.column(i).unwrap().name.clone());
        let v = JsonValue::Object(
            request_members_to_json(&req, &col).into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
        );
        let find = |c: &JsonValue| Ok(t.find(c.as_str().unwrap()).unwrap());
        assert_eq!(request_from_json(&v, req.response, req.predictors.clone(), &find).unwrap(), req);
        // A text predictor is refused.
        assert!(PlsAnalysis::run(&t, &rows, &PlsRequest::new(12, vec![0, 13])).is_err());
    }
}
