//! A linear-model analysis bound to a dataset and its row state.
//!
//! [`LinearModelAnalysis::run`] fits ordinary least squares to the rows that are
//! complete (no missing value in the model's columns) and not excluded, and keeps
//! everything a report needs. It records the row state's exclusion revision, so a
//! view can call [`is_stale`](LinearModelAnalysis::is_stale) each frame and refit
//! when the user excludes or re-includes rows.
//!
//! Predictors may be continuous or categorical. Text columns, and numeric
//! columns whose measurement level is nominal or ordinal, enter as categorical
//! terms with indicator coding ([`stat_linear::design`]); a model with both kinds
//! is an analysis of covariance. Each term gets a partial F-test.
//!
//! [`LinearModelAnalysis::profile`] computes the Response Explorer: for each
//! term, the predicted response (with confidence band) as that term varies and
//! the others stay at chosen settings.

use crate::format::{format_number, format_optional, format_p_value};
use crate::report::{
    PlotSpec, Polyline, ReportOptions, ReportSection, RowPoint, SectionContent, SectionKind, TextTable,
};
use crate::report_columns::ReportColumn;
use stat_core::distributions::{ContinuousDistribution, Normal};
use stat_core::{Matrix, Result, StatError};
use stat_data::{DataTable, MeasurementLevel, RowStateManager};
use stat_inference::multiple::{ComparisonMethod, compare_all_pairs};
use stat_linear::{
    EffectTest, LinearModelFit, ModelDesign, OlsOptions, Prediction, Predictor, PredictorValues, ResidualDiagnostics,
    TermKind, TermValue, effect_tests, fit_linear_model,
};

/// Significant digits used in report tables.
const DIGITS: usize = 6;
/// Points used to draw the fitted line, confidence band and profile curves.
const CURVE_POINTS: usize = 64;
/// Most rows listed in the influence table.
const MAX_INFLUENCE_ROWS: usize = 50;
/// Studentized residuals larger than this (in absolute value) are flagged.
pub const STUDENTIZED_LIMIT: f64 = 3.0;

/// Which columns enter the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinearModelSpec {
    /// Response column index.
    pub response: usize,
    /// Predictor column indices (at least one).
    pub predictors: Vec<usize>,
    /// Model effects as `(position in predictors, power)` products, e.g.
    /// `[[(0, 1)], [(1, 1)], [(0, 1), (1, 1)], [(0, 2)]]` for `A + B + A*B + A^2`.
    /// Empty means main effects only.
    pub effects: Vec<Vec<(usize, u8)>>,
    /// Continuous predictors entered as splines, by position in
    /// `predictors` (missing entries: none); see [`stat_linear::spline`].
    pub splines: Vec<Option<stat_linear::SplineSpec>>,
}

impl LinearModelSpec {
    /// The effects to fit: the listed ones, or every main effect.
    #[must_use]
    pub fn effective_effects(&self) -> Vec<Vec<(usize, u8)>> {
        if self.effects.is_empty() { stat_linear::main_effects(self.predictors.len()) } else { self.effects.clone() }
    }
}

/// One factor's panel in the Response Explorer.
#[derive(Debug, Clone, PartialEq)]
pub enum ProfilePanel {
    /// A continuous term: the predicted response along its range.
    Continuous {
        /// Term name.
        name: String,
        /// `[x, predicted]` along the range of the rows used.
        curve: Polyline,
        /// Lower and upper confidence limits along the same grid (when the
        /// model gives limits).
        band: Option<(Polyline, Polyline)>,
        /// Range of the term in the rows used.
        range: (f64, f64),
        /// Current setting.
        value: f64,
    },
    /// A categorical term: the prediction at each level.
    Categorical {
        /// Term name.
        name: String,
        /// Levels, in model order.
        levels: Vec<String>,
        /// Prediction and confidence limits (when given) at each level.
        predictions: Vec<(f64, Option<(f64, f64)>)>,
        /// Index of the current level.
        selected: usize,
    },
}

/// The Response Explorer at one setting of every term.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    /// Response name.
    pub response: String,
    /// One panel per term, in model order.
    pub panels: Vec<ProfilePanel>,
    /// Predicted response at the current settings.
    pub predicted: f64,
    /// Confidence interval for the mean response at the current settings
    /// (when the model gives limits).
    pub confidence_interval: Option<(f64, f64)>,
    /// Confidence level of the intervals, e.g. 0.95.
    pub confidence_level: Option<f64>,
    /// A vertical range covering every panel's band, shared by all panels.
    pub y_range: (f64, f64),
    /// A note about the profile (e.g. why there are no confidence bands).
    pub note: Option<String>,
}

/// How one predictor enters, with its values for every row.
#[derive(Debug, Clone)]
pub(crate) enum PredictorData {
    Continuous(Vec<f64>),
    Categorical(Vec<String>),
}

/// A fitted linear model plus the data needed to report and plot it.
#[derive(Debug, Clone)]
pub struct LinearModelAnalysis {
    spec: LinearModelSpec,
    response_name: String,
    predictor_names: Vec<String>,
    design: ModelDesign,
    /// The predictor columns, for the prediction formula.
    factors: Vec<crate::prediction_formula::FactorColumn>,
    fit: LinearModelFit,
    /// Design matrix and response of the rows used.
    x: Matrix,
    y: Vec<f64>,
    effects: Vec<EffectTest>,
    diagnostics: ResidualDiagnostics,
    rows_used: Vec<usize>,
    complete_rows: Vec<usize>,
    n_missing: usize,
    n_excluded: usize,
    exclusion_revision: u64,
    /// Predicted value for every complete row (NaN where a level is unknown to the fit).
    predicted: Vec<f64>,
    /// Predicted value for every table row with its predictors present,
    /// whether or not it has a response (NaN otherwise).
    predicted_all: Vec<f64>,
    /// Observed response for every complete row.
    observed: Vec<f64>,
    /// Number of rows in the table the model was fitted on.
    n_table_rows: usize,
    /// Single-predictor value for every complete row (empty unless the model
    /// has exactly one, continuous, predictor).
    single_x: Vec<f64>,
    /// Range and mean of each continuous term over the rows used.
    continuous_summary: Vec<Option<(f64, f64, f64)>>,
    /// Most frequent level of each categorical term over the rows used.
    modal_level: Vec<Option<usize>>,
    /// The rows used, as profiler observations.
    observations: Vec<crate::surface_profiler::Observation>,
    /// For a one-way model (one categorical factor, main effect only): its
    /// levels and the response of the rows used at each.
    one_way: Option<(Vec<String>, Vec<Vec<f64>>)>,
}

impl LinearModelAnalysis {
    /// Fit the model described by `spec` to `table`, honouring row exclusion.
    ///
    /// # Errors
    /// * [`StatError::InvalidArgument`] if a column index is invalid, the response
    ///   is not numeric or is also a predictor, there are no predictors, or a
    ///   categorical predictor has a single level in the rows used.
    /// * Any error from [`fit_linear_model`] (too few rows, collinear predictors…).
    pub fn run(table: &DataTable, states: &RowStateManager, spec: &LinearModelSpec) -> Result<Self> {
        let coded = Coded::new(table, states, spec)?;
        let observations = coded.observations();
        let Coded {
            response_name,
            predictor_names,
            predictor_data,
            complete_rows,
            rows_used,
            n_missing,
            n_excluded,
            design,
            x,
            y,
            observed,
        } = coded;
        let predictors = borrow_predictors(&predictor_names, &predictor_data);
        let options = OlsOptions { predictor_names: design.column_names().to_vec(), ..OlsOptions::default() };
        let fit = fit_linear_model(&x, &y, &options)?;
        let effects = effect_tests(&design, &x, &y, &fit, &options)?;
        let diagnostics = fit.residual_diagnostics();

        // Predictions for every row with its predictors present, including
        // excluded rows and rows without a response (new rows to score),
        // except rows at a level the fit has not seen.
        let encoded: Vec<Option<Vec<f64>>> = (0..table.n_rows())
            .map(|r| design.row(&predictors, r).filter(|row| row.iter().all(|v| v.is_finite())))
            .collect();
        let known: Vec<Vec<f64>> = encoded.iter().flatten().cloned().collect();
        let mut known_predictions = if known.is_empty() {
            Vec::new()
        } else {
            fit.predict(&Matrix::from_rows(&known)?)?.into_iter().map(|p| p.fitted).collect()
        }
        .into_iter();
        let predicted_all: Vec<f64> = encoded
            .iter()
            .map(|e| if e.is_some() { known_predictions.next().unwrap_or(f64::NAN) } else { f64::NAN })
            .collect();
        let predicted: Vec<f64> = complete_rows.iter().map(|&r| predicted_all[r]).collect();
        let single_x = match predictor_data.as_slice() {
            [PredictorData::Continuous(v)] => complete_rows.iter().map(|&r| v[r]).collect(),
            _ => Vec::new(),
        };

        let continuous_summary = predictor_data
            .iter()
            .map(|d| match d {
                PredictorData::Continuous(v) => {
                    let used: Vec<f64> = rows_used.iter().map(|&r| v[r]).collect();
                    let lo = used.iter().copied().fold(f64::INFINITY, f64::min);
                    let hi = used.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                    let mean = stat_core::summation::mean(&used).unwrap_or(f64::NAN);
                    Some((lo, hi, mean))
                }
                PredictorData::Categorical(_) => None,
            })
            .collect();
        let modal_level = design
            .terms()
            .iter()
            .zip(&predictor_data)
            .map(|(term, d)| match (&term.kind, d) {
                (TermKind::Categorical { levels }, PredictorData::Categorical(labels)) => {
                    let counts: Vec<usize> =
                        levels.iter().map(|l| rows_used.iter().filter(|&&r| labels[r] == *l).count()).collect();
                    // First level with the largest count.
                    let best = counts.iter().copied().max().unwrap_or(0);
                    counts.iter().position(|&c| c == best)
                }
                _ => None,
            })
            .collect();

        let one_way = match (predictor_data.as_slice(), design.terms(), design.effects().len()) {
            ([PredictorData::Categorical(labels)], [term], 1) => match &term.kind {
                TermKind::Categorical { levels } => {
                    let groups = levels
                        .iter()
                        .map(|l| rows_used.iter().zip(&y).filter(|(r, _)| labels[**r] == *l).map(|(_, v)| *v).collect())
                        .collect();
                    Some((levels.clone(), groups))
                }
                _ => None,
            },
            _ => None,
        };
        Ok(Self {
            n_table_rows: table.n_rows(),
            spec: spec.clone(),
            response_name,
            predictor_names,
            design,
            factors: crate::prediction_formula::factor_columns(table, &spec.predictors),
            fit,
            x,
            y,
            effects,
            diagnostics,
            rows_used,
            complete_rows,
            n_missing,
            n_excluded,
            exclusion_revision: states.exclusion_revision(),
            predicted,
            predicted_all,
            observed,
            single_x,
            continuous_summary,
            modal_level,
            observations,
            one_way,
        })
    }
}

/// A model coded on the rows a fit would use, before fitting.
pub(crate) struct Coded {
    pub(crate) response_name: String,
    pub(crate) predictor_names: Vec<String>,
    pub(crate) predictor_data: Vec<PredictorData>,
    complete_rows: Vec<usize>,
    pub(crate) rows_used: Vec<usize>,
    pub(crate) n_missing: usize,
    pub(crate) n_excluded: usize,
    /// Coding of the predictors and effects.
    pub(crate) design: ModelDesign,
    /// Design matrix without the intercept.
    pub(crate) x: Matrix,
    /// Response of the rows used.
    pub(crate) y: Vec<f64>,
    observed: Vec<f64>,
}

impl Coded {
    /// Validate `spec` and code it on the complete, included rows.
    pub(crate) fn new(table: &DataTable, states: &RowStateManager, spec: &LinearModelSpec) -> Result<Self> {
        Self::with_rows(table, states, spec, &|_| true)
    }

    /// As [`Coded::new`], keeping only rows for which `also` holds too (for
    /// several responses: every response present).
    pub(crate) fn with_rows(
        table: &DataTable,
        states: &RowStateManager,
        spec: &LinearModelSpec,
        also: &dyn Fn(usize) -> bool,
    ) -> Result<Self> {
        if spec.predictors.is_empty() {
            return Err(invalid("choose at least one predictor"));
        }
        if spec.predictors.contains(&spec.response) {
            return Err(invalid("the response cannot also be a predictor"));
        }
        let column = |i: usize| table.column(i).ok_or_else(|| invalid(format!("no column {i}")));
        let response_col = column(spec.response)?;
        let y_all = response_col
            .as_numeric()
            .ok_or_else(|| invalid(format!("the response {:?} must be numeric", response_col.name)))?;
        let mut predictor_names = Vec::with_capacity(spec.predictors.len());
        let mut predictor_data = Vec::with_capacity(spec.predictors.len());
        for &i in &spec.predictors {
            let col = column(i)?;
            predictor_names.push(col.name.clone());
            predictor_data.push(match (col.as_numeric(), col.level) {
                (Some(v), MeasurementLevel::Continuous) => PredictorData::Continuous(v.to_vec()),
                // Nominal / ordinal numbers and text are categories.
                _ => PredictorData::Categorical((0..table.n_rows()).map(|r| col.level_text(r)).collect()),
            });
        }
        let present = |d: &PredictorData, r: usize| match d {
            PredictorData::Continuous(v) => v[r].is_finite(),
            PredictorData::Categorical(v) => !v[r].trim().is_empty(),
        };
        let complete_rows: Vec<usize> = (0..table.n_rows())
            .filter(|&r| y_all[r].is_finite() && also(r) && predictor_data.iter().all(|d| present(d, r)))
            .collect();
        let rows_used: Vec<usize> = complete_rows.iter().copied().filter(|&r| !states.is_excluded(r)).collect();
        let n_missing = table.n_rows() - complete_rows.len();
        let n_excluded = complete_rows.len() - rows_used.len();

        let predictors = borrow_predictors(&predictor_names, &predictor_data);
        let design = ModelDesign::with_splines(&predictors, &rows_used, &spec.effective_effects(), &spec.splines)?;
        let x = design.matrix(&predictors, &rows_used)?;
        let y: Vec<f64> = rows_used.iter().map(|&r| y_all[r]).collect();
        let observed: Vec<f64> = complete_rows.iter().map(|&r| y_all[r]).collect();
        Ok(Self {
            response_name: response_col.name.clone(),
            predictor_names,
            predictor_data,
            complete_rows,
            rows_used,
            n_missing,
            n_excluded,
            design,
            x,
            y,
            observed,
        })
    }

    /// The rows used, as profiler observations.
    pub(crate) fn observations(&self) -> Vec<crate::surface_profiler::Observation> {
        self.rows_used
            .iter()
            .zip(&self.y)
            .map(|(&r, &response)| crate::surface_profiler::Observation {
                row: r,
                settings: self
                    .design
                    .terms()
                    .iter()
                    .zip(&self.predictor_data)
                    .map(|(term, d)| match (&term.kind, d) {
                        (TermKind::Categorical { levels }, PredictorData::Categorical(labels)) => {
                            TermValue::Level(levels.iter().position(|l| *l == labels[r]).unwrap_or(0))
                        }
                        (_, PredictorData::Continuous(v)) => TermValue::Number(v[r]),
                        _ => TermValue::Number(f64::NAN),
                    })
                    .collect(),
                response,
            })
            .collect()
    }
}

impl LinearModelAnalysis {
    /// The model specification.
    #[must_use]
    pub fn spec(&self) -> &LinearModelSpec {
        &self.spec
    }

    /// The underlying fit.
    #[must_use]
    pub fn fit(&self) -> &LinearModelFit {
        &self.fit
    }

    /// Design matrix (without the intercept) and response of the rows used.
    #[must_use]
    pub fn model_data(&self) -> (&Matrix, &[f64]) {
        (&self.x, &self.y)
    }

    /// The coding of the predictors.
    #[must_use]
    pub fn design(&self) -> &ModelDesign {
        &self.design
    }

    /// Partial F-test of each term.
    #[must_use]
    pub fn effect_tests(&self) -> &[EffectTest] {
        &self.effects
    }

    /// Columns a report can add to the table: predicted values for every row
    /// with its predictors present (rows without a response included, so new
    /// rows can be scored), and residuals (observed − predicted) for every row
    /// with complete data; excluded rows included (NaN where a categorical
    /// level is unknown to the fit, and for rows with missing values).
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        let predicted = self.predicted_all.clone();
        let mut residual = vec![f64::NAN; self.n_table_rows];
        for ((&r, &p), &o) in self.complete_rows.iter().zip(&self.predicted).zip(&self.observed) {
            residual[r] = o - p;
        }
        let y = &self.response_name;
        let name = format!("Predicted {y}");
        let mut out =
            vec![ReportColumn::continuous("predicted", "Save predicted values", name.clone(), predicted.clone())];
        out.extend(crate::report_columns::prediction_formula_column(self.prediction_formula(), &name, predicted));
        out.push(ReportColumn::continuous("residual", "Save residuals", format!("Residual {y}"), residual));
        out
    }

    /// The prediction equation as a formula column (see
    /// [`crate::prediction_formula`]).
    ///
    /// # Errors
    /// If the formula cannot be written (e.g. a date-time column used as a category).
    pub fn prediction_formula(&self) -> Result<String> {
        let b = self.fit.coefficients();
        match b.split_first() {
            Some((&b0, slopes)) if slopes.len() == self.design.column_names().len() => {
                crate::prediction_formula::prediction_formula(
                    &self.design,
                    &self.factors,
                    b0,
                    slopes,
                    crate::prediction_formula::Link::Identity,
                )
            }
            _ => Err(StatError::InvalidArgument { message: "the model has no intercept".into() }),
        }
    }

    /// Dataset rows the model was fitted to.
    #[must_use]
    pub fn rows_used(&self) -> &[usize] {
        &self.rows_used
    }

    /// `true` if row exclusion has changed since the fit, so it should be rerun.
    #[must_use]
    pub fn is_stale(&self, states: &RowStateManager) -> bool {
        states.exclusion_revision() != self.exclusion_revision
    }

    /// Report title, e.g. `"Linear Model: y ~ x1 + x2"`.
    #[must_use]
    pub fn title(&self) -> String {
        let effects: Vec<&str> = self.design.effects().iter().map(|e| e.name.as_str()).collect();
        format!("Linear Model: {} ~ {}", self.response_name, effects.join(" + "))
    }

    /// Report sections, honouring `options`. (The Response Explorer is
    /// interactive and comes from [`profile`](Self::profile) instead.)
    #[must_use]
    pub fn sections(&self, options: &ReportOptions) -> Vec<ReportSection> {
        let mut out = vec![self.fit_statistics()];
        let names: Vec<String> = self.design.terms().iter().map(|t| t.name.clone()).collect();
        out.extend(crate::report::hierarchy_note(&names, &self.spec.effective_effects()));
        out.extend(
            crate::report::EffectPareto::new(
                self.effects.iter().map(|e| (e.term.clone(), e.p_value.unwrap_or(f64::NAN))),
                "partial F tests",
            )
            .section(),
        );
        if options.show_anova {
            out.push(self.anova_table());
        }
        out.push(self.parameter_table(options.show_confidence_limits));
        out.extend(crate::marginal_sections::spline_knots_section(&self.design));
        if self.design.effects().len() > 1 || self.design.has_categorical() || self.design.has_splines() {
            out.push(self.effect_table());
        }
        if options.show_effect_sizes {
            out.push(self.effect_size_section());
        }
        if options.show_equal_variances {
            out.extend(self.equal_variances_section());
        }
        if let Some(method) = options.pairwise.filter(|&m| m != ComparisonMethod::Dunn) {
            out.extend(self.pairwise_section(method));
        }
        if let (true, Some((levels, groups))) = (options.linear_trend, &self.one_way) {
            if levels.len() >= 3 {
                out.push(crate::effect_size_sections::linear_trend_section(levels, groups, PAIRWISE_LEVEL));
            }
        }
        if let (Some(c), Some((levels, groups))) = (options.control_level, &self.one_way) {
            if c < levels.len() {
                let heading = self.predictor_names.first().map_or("", String::as_str);
                out.extend(crate::compare::control_sections(
                    heading,
                    &self.response_name,
                    levels,
                    groups,
                    c,
                    PAIRWISE_LEVEL,
                    stat_inference::Alternative::TwoSided,
                ));
            }
        }
        if let (Some(direction), Some((levels, groups))) = (options.best, &self.one_way) {
            let heading = self.predictor_names.first().map_or("", String::as_str);
            out.extend(crate::compare::best_sections(
                heading,
                &self.response_name,
                levels,
                groups,
                PAIRWISE_LEVEL,
                direction,
            ));
        }
        if options.slices.is_some() || options.slopes.is_some() {
            let beta = self.fit.coefficients();
            let df = self.fit.anova().error.df as f64;
            let inference =
                stat_linear::marginal::WaldInference { beta: &beta, covariance: self.fit.covariance(), df: Some(df) };
            out.extend(crate::marginal_sections::sections(
                &self.design,
                &inference,
                options.slices,
                options.slopes,
                PAIRWISE_LEVEL,
                None,
            ));
        }
        if options.show_fit_plot {
            out.push(self.fit_plot(options.show_confidence_band));
        }
        if options.show_residual_plot {
            out.push(self.residual_plot());
        }
        if options.show_diagnostics {
            out.push(self.influence_table());
            out.push(self.cooks_distance_plot());
            out.push(self.residual_quantile_plot());
        }
        out.push(crate::report::prediction_formula_section(self.prediction_formula()));
        out
    }

    fn fit_statistics(&self) -> ReportSection {
        let s = self.fit.summary();
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut row = |name: &str, value: String| t.push([name.to_owned(), value]);
        row("R²", format_number(s.r_squared, DIGITS));
        row("Adjusted R²", format_number(s.adj_r_squared, DIGITS));
        row("Root mean square error", format_number(s.root_mean_square_error, DIGITS));
        row("Mean of response", format_number(s.mean_response, DIGITS));
        row("Observations used", s.n_observations.to_string());
        if self.n_excluded > 0 {
            row("Rows excluded", self.n_excluded.to_string());
        }
        if self.n_missing > 0 {
            row("Rows with missing values", self.n_missing.to_string());
        }
        row("AICc", format_optional(s.aicc, DIGITS));
        row("BIC", format_optional(s.bic, DIGITS));
        section(SectionKind::FitStatistics, "Fit Statistics", SectionContent::Table(t))
    }

    fn anova_table(&self) -> ReportSection {
        let a = self.fit.anova();
        let mut t = TextTable::new(["Source", "DF", "Sum of Squares", "Mean Square", "F", "p-value"]);
        t.push([
            "Model".to_owned(),
            a.model.df.to_string(),
            format_number(a.model.sum_of_squares, DIGITS),
            format_optional(a.model.mean_square, DIGITS),
            format_optional(a.f_statistic, DIGITS),
            a.p_value.map_or_else(|| "—".to_owned(), format_p_value),
        ]);
        t.push([
            "Error".to_owned(),
            a.error.df.to_string(),
            format_number(a.error.sum_of_squares, DIGITS),
            format_optional(a.error.mean_square, DIGITS),
            String::new(),
            String::new(),
        ]);
        let total = if a.centered { "Total (corrected)" } else { "Total (uncorrected)" };
        t.push([
            total.to_owned(),
            a.total.df.to_string(),
            format_number(a.total.sum_of_squares, DIGITS),
            String::new(),
            String::new(),
            String::new(),
        ]);
        section(SectionKind::AnalysisOfVariance, "Analysis of Variance", SectionContent::Table(t))
    }

    fn parameter_table(&self, show_limits: bool) -> ReportSection {
        let level = (self.fit.confidence_level() * 100.0).round();
        let mut cols = vec!["Term".to_owned(), "Estimate".into(), "Std Error".into(), "t".into(), "p-value".into()];
        if show_limits {
            cols.push(format!("Lower {level}%"));
            cols.push(format!("Upper {level}%"));
        }
        let mut t = TextTable::new(cols);
        for p in self.fit.parameters() {
            let mut row = vec![
                p.name.clone(),
                format_number(p.estimate, DIGITS),
                format_number(p.std_error, DIGITS),
                format_optional(p.t_statistic, 4),
                p.p_value.map_or_else(|| "—".to_owned(), format_p_value),
            ];
            if show_limits {
                row.push(format_number(p.ci_lower, DIGITS));
                row.push(format_number(p.ci_upper, DIGITS));
            }
            t.push(row);
        }
        let mut s = section(SectionKind::ParameterEstimates, "Parameter Estimates", SectionContent::Table(t));
        if self.design.has_categorical() {
            let refs: Vec<String> = self
                .design
                .terms()
                .iter()
                .filter_map(|term| match &term.kind {
                    TermKind::Categorical { levels } => Some(format!("{} = {}", term.name, levels[0])),
                    TermKind::Continuous => None,
                })
                .collect();
            s.title = match self.design.coding() {
                stat_linear::CategoricalCoding::Reference => {
                    format!("Parameter Estimates (reference levels: {})", refs.join(", "))
                }
                stat_linear::CategoricalCoding::SumToZero => format!(
                    "Parameter Estimates (sum-to-zero coding; first levels {}; continuous predictors centered in \
                     higher-order terms)",
                    refs.join(", ")
                ),
            };
        } else if !self.design.is_main_effects() {
            s.title = "Parameter Estimates (predictors centered at their means in higher-order terms)".to_owned();
        }
        s
    }

    /// Whether the model has a single categorical factor and nothing else
    /// (one-way analysis of variance), so pairwise comparisons and tests of
    /// equal variances apply.
    #[must_use]
    pub fn is_one_way(&self) -> bool {
        self.one_way.is_some()
    }

    /// Levels of the factor of a one-way model, in report order (`None` for
    /// other models).
    #[must_use]
    pub fn one_way_levels(&self) -> Option<&[String]> {
        self.one_way.as_ref().map(|(levels, _)| levels.as_slice())
    }

    /// Tests of equal variances across the levels of a one-way model.
    fn equal_variances_section(&self) -> Option<ReportSection> {
        let (_, groups) = self.one_way.as_ref()?;
        Some(crate::compare::equal_variances_section(
            groups,
            "Tests of Equal Variances (small p-values favour Games–Howell comparisons)",
        ))
    }

    /// Comparisons of every pair of levels of a one-way model by `method`,
    /// with 95% intervals. The pooled-variance methods use the pooled error
    /// mean square, which in a one-way model is the model's error mean
    /// square on its `N − k` df; Games–Howell, Tamhane T2 and Dunnett T3 use
    /// Welch standard errors and df for each pair.
    fn pairwise_section(&self, method: ComparisonMethod) -> Option<ReportSection> {
        let (levels, groups) = self.one_way.as_ref()?;
        let content = match compare_all_pairs(groups, method, PAIRWISE_LEVEL) {
            Ok(list) => SectionContent::Table(crate::compare::pairwise_table(levels, &list, method, PAIRWISE_LEVEL)),
            Err(e) => SectionContent::Note(format!("Not available: {e}")),
        };
        Some(section(
            SectionKind::PairwiseComparisons,
            &crate::compare::pairwise_title(method, PAIRWISE_LEVEL),
            content,
        ))
    }

    /// η², partial η², partial ω² and Cohen's f of each effect, from its
    /// partial (drop-one) sum of squares, with noncentral-F intervals at the
    /// model's confidence level.
    fn effect_size_section(&self) -> ReportSection {
        let a = self.fit.anova();
        let n = self.fit.summary().n_observations as f64;
        let rows = self
            .effects
            .iter()
            .map(|e| {
                let r = if a.centered {
                    stat_inference::effect_sizes::anova_effect_size(
                        e.sum_of_squares,
                        e.df as f64,
                        a.error.sum_of_squares,
                        a.error.df as f64,
                        a.total.sum_of_squares,
                        n,
                        self.fit.confidence_level(),
                    )
                } else {
                    Err(invalid("the model has no intercept"))
                };
                (e.term.clone(), r)
            })
            .collect();
        crate::effect_size_sections::anova_effect_size_section(rows, self.fit.confidence_level())
    }

    fn effect_table(&self) -> ReportSection {
        let mut t = TextTable::new(["Term", "DF", "Sum of Squares", "F", "p-value"]);
        for e in &self.effects {
            t.push([
                e.term.clone(),
                e.df.to_string(),
                format_number(e.sum_of_squares, DIGITS),
                format_optional(e.f_statistic, DIGITS),
                e.p_value.map_or_else(|| "—".to_owned(), format_p_value),
            ]);
        }
        section(SectionKind::EffectTests, "Tests by Effect", SectionContent::Table(t))
    }

    fn fit_plot(&self, show_band: bool) -> ReportSection {
        if self.single_x.is_empty() {
            let points = self.points(&self.predicted, &self.observed);
            let line = diagonal(&points);
            let spec = PlotSpec {
                x_time: None,
                y_time: None,
                x_label: format!("Predicted {}", self.response_name),
                y_label: self.response_name.clone(),
                points,
                line,
                line_name: Some("Reference line y = x".to_owned()),
                band: None,
                groups: None,
                x_ticks: None,
                panes: Vec::new(),
                annotations: None,
                segments: Vec::new(),
                ..PlotSpec::default()
            };
            return section(
                SectionKind::ObservedVsPredicted,
                "Observed vs Predicted",
                SectionContent::Plot(Box::new(spec)),
            );
        }
        let points = self.points(&self.single_x, &self.observed);
        let (line, band) = self.fitted_curve(show_band);
        let spec = PlotSpec {
            x_time: None,
            y_time: None,
            x_label: self.predictor_names[0].clone(),
            y_label: self.response_name.clone(),
            points,
            line,
            band,
            groups: None,
            x_ticks: None,
            panes: Vec::new(),
            annotations: None,
            segments: Vec::new(),
            ..PlotSpec::default()
        };
        section(SectionKind::FittedLine, "Fitted Line", SectionContent::Plot(Box::new(spec)))
    }

    fn residual_plot(&self) -> ReportSection {
        let residuals: Vec<f64> = self.observed.iter().zip(&self.predicted).map(|(o, p)| o - p).collect();
        let points = self.points(&self.predicted, &residuals);
        let line = x_range(&points).map(|(lo, hi)| vec![[lo, 0.0], [hi, 0.0]]);
        let spec = PlotSpec {
            x_time: None,
            y_time: None,
            x_label: format!("Predicted {}", self.response_name),
            y_label: "Residual".to_owned(),
            points,
            line,
            line_name: Some("Zero".to_owned()),
            band: None,
            groups: None,
            x_ticks: None,
            panes: Vec::new(),
            annotations: None,
            segments: Vec::new(),
            ..PlotSpec::default()
        };
        section(SectionKind::ResidualsVsPredicted, "Residuals vs Predicted", SectionContent::Plot(Box::new(spec)))
    }

    /// Thresholds used to flag rows: leverage `2k/n` and Cook's distance `4/n`.
    #[must_use]
    pub fn influence_thresholds(&self) -> (f64, f64) {
        let s = self.fit.summary();
        let n = s.n_observations as f64;
        (2.0 * s.n_parameters as f64 / n, 4.0 / n)
    }

    fn influence_table(&self) -> ReportSection {
        let (h_limit, d_limit) = self.influence_thresholds();
        let d = &self.diagnostics;
        let mut flagged: Vec<usize> = (0..self.rows_used.len())
            .filter(|&i| {
                d.leverage[i] > h_limit
                    || d.studentized_residuals[i].abs() > STUDENTIZED_LIMIT
                    || d.cooks_distance[i] > d_limit
            })
            .collect();
        let title = "Influence Diagnostics";
        if flagged.is_empty() {
            return section(
                SectionKind::InfluenceDiagnostics,
                title,
                SectionContent::Note(format!(
                    "No row exceeds the thresholds: leverage > 2k/n = {}, |studentized residual| > {STUDENTIZED_LIMIT}, \
                     Cook's distance > 4/n = {}.",
                    format_number(h_limit, 3),
                    format_number(d_limit, 3)
                )),
            );
        }
        flagged.sort_by(|&a, &b| d.cooks_distance[b].total_cmp(&d.cooks_distance[a]));
        let shown = flagged.len().min(MAX_INFLUENCE_ROWS);
        let mut t = TextTable::new(["Row", "Leverage", "Studentized residual", "Cook's distance", "Flags"]);
        for &i in &flagged[..shown] {
            let mut flags = Vec::new();
            if d.leverage[i] > h_limit {
                flags.push("high leverage");
            }
            if d.studentized_residuals[i].abs() > STUDENTIZED_LIMIT {
                flags.push("large residual");
            }
            if d.cooks_distance[i] > d_limit {
                flags.push("influential");
            }
            t.push([
                (self.rows_used[i] + 1).to_string(),
                format_number(d.leverage[i], 4),
                format_number(d.studentized_residuals[i], 4),
                format_number(d.cooks_distance[i], 4),
                flags.join(", "),
            ]);
        }
        let title = format!(
            "{title} ({} of {} rows flagged{}; leverage > {}, |t| > {STUDENTIZED_LIMIT}, D > {})",
            flagged.len(),
            self.rows_used.len(),
            if shown < flagged.len() { format!(", largest {shown} shown") } else { String::new() },
            format_number(h_limit, 3),
            format_number(d_limit, 3)
        );
        section(SectionKind::InfluenceDiagnostics, &title, SectionContent::Table(t))
    }

    fn cooks_distance_plot(&self) -> ReportSection {
        let (_, d_limit) = self.influence_thresholds();
        let points: Vec<RowPoint> = self
            .rows_used
            .iter()
            .zip(&self.diagnostics.cooks_distance)
            .filter(|(_, d)| d.is_finite())
            .map(|(&row, &d)| RowPoint { row, x: (row + 1) as f64, y: d })
            .collect();
        let line = x_range(&points).map(|(lo, hi)| vec![[lo, d_limit], [hi, d_limit]]);
        let spec = PlotSpec {
            x_time: None,
            y_time: None,
            x_label: "Row".to_owned(),
            y_label: "Cook's distance".to_owned(),
            points,
            line,
            line_name: Some("4/n threshold".to_owned()),
            band: None,
            groups: None,
            x_ticks: None,
            panes: Vec::new(),
            annotations: None,
            segments: Vec::new(),
            ..PlotSpec::default()
        };
        section(SectionKind::CooksDistance, "Cook's Distance (line at 4/n)", SectionContent::Plot(Box::new(spec)))
    }

    fn residual_quantile_plot(&self) -> ReportSection {
        let mut pairs: Vec<(usize, f64)> = self
            .rows_used
            .iter()
            .zip(&self.diagnostics.studentized_residuals)
            .filter(|(_, t)| t.is_finite())
            .map(|(&r, &t)| (r, t))
            .collect();
        pairs.sort_by(|a, b| a.1.total_cmp(&b.1));
        let points = normal_quantile_points(&pairs);
        let line = diagonal(&points);
        let spec = PlotSpec {
            x_time: None,
            y_time: None,
            x_label: "Normal quantile".to_owned(),
            y_label: "Studentized residual".to_owned(),
            points,
            line,
            line_name: Some("Normal reference line".to_owned()),
            band: None,
            groups: None,
            x_ticks: None,
            panes: Vec::new(),
            annotations: None,
            segments: Vec::new(),
            ..PlotSpec::default()
        };
        section(SectionKind::ResidualQuantiles, "Residual Normal Quantile Plot", SectionContent::Plot(Box::new(spec)))
    }

    fn points(&self, x: &[f64], y: &[f64]) -> Vec<RowPoint> {
        self.complete_rows
            .iter()
            .zip(x.iter().zip(y))
            .filter(|(_, (x, y))| x.is_finite() && y.is_finite())
            .map(|(&row, (&x, &y))| RowPoint { row, x, y })
            .collect()
    }

    /// Fitted line and confidence band over the range of the rows used.
    fn fitted_curve(&self, show_band: bool) -> (Option<Polyline>, Option<(Polyline, Polyline)>) {
        let Some(Some((lo, hi, _))) = self.continuous_summary.first() else { return (None, None) };
        let grid = grid(*lo, *hi);
        // Through the design, so polynomial terms of the one predictor are included.
        let points: Vec<Vec<TermValue>> = grid.iter().map(|&g| vec![TermValue::Number(g)]).collect();
        let Ok(pred) = self.predict_points(&points) else { return (None, None) };
        let line = grid.iter().zip(&pred).map(|(&g, p)| [g, p.fitted]).collect();
        let band = show_band.then(|| {
            let lower = grid.iter().zip(&pred).map(|(&g, p)| [g, p.confidence_interval.0]).collect();
            let upper = grid.iter().zip(&pred).map(|(&g, p)| [g, p.confidence_interval.1]).collect();
            (lower, upper)
        });
        (Some(line), band)
    }

    /// Starting settings for the Response Explorer: the mean of each
    /// continuous term and the most frequent level of each categorical term,
    /// over the rows used.
    #[must_use]
    pub fn default_settings(&self) -> Vec<TermValue> {
        self.design
            .terms()
            .iter()
            .enumerate()
            .map(|(j, term)| match term.kind {
                TermKind::Continuous => TermValue::Number(self.continuous_summary[j].map_or(0.0, |s| s.2)),
                TermKind::Categorical { .. } => TermValue::Level(self.modal_level[j].unwrap_or(0)),
            })
            .collect()
    }

    /// `true` if `settings` has one value of the right kind per term.
    #[must_use]
    pub fn settings_fit(&self, settings: &[TermValue]) -> bool {
        settings.len() == self.design.terms().len()
            && self.design.terms().iter().zip(settings).all(|(term, v)| match (&term.kind, v) {
                (TermKind::Continuous, TermValue::Number(x)) => x.is_finite(),
                (TermKind::Categorical { levels }, TermValue::Level(l)) => *l < levels.len(),
                _ => false,
            })
    }

    fn predict_points(&self, settings: &[Vec<TermValue>]) -> Result<Vec<Prediction>> {
        let rows: Vec<Vec<f64>> = settings.iter().map(|s| self.design.encode(s)).collect::<Result<_>>()?;
        self.fit.predict(&Matrix::from_rows(&rows)?)
    }

    /// The Response Explorer at `settings` (one [`TermValue`] per term).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] if `settings` does not match the terms.
    pub fn profile(&self, settings: &[TermValue]) -> Result<Profile> {
        crate::surface_profiler::profile(self, settings)
    }
}

pub(crate) fn borrow_predictors<'a>(names: &'a [String], data: &'a [PredictorData]) -> Vec<Predictor<'a>> {
    names
        .iter()
        .zip(data)
        .map(|(name, d)| Predictor {
            name,
            values: match d {
                PredictorData::Continuous(v) => PredictorValues::Continuous(v),
                PredictorData::Categorical(v) => PredictorValues::Categorical(v),
            },
        })
        .collect()
}

/// `CURVE_POINTS` evenly spaced values from `lo` to `hi` (one value if equal).
fn grid(lo: f64, hi: f64) -> Vec<f64> {
    if !(lo.is_finite() && hi.is_finite()) {
        return Vec::new();
    }
    let n = if hi > lo { CURVE_POINTS } else { 1 };
    (0..n).map(|i| lo + (hi - lo) * i as f64 / (n.max(2) - 1) as f64).collect()
}

/// Points of a normal quantile plot for `(row, value)` pairs sorted by value:
/// x is the standard normal quantile of the plotting position
/// `(i − 3/8)/(n + 1/4)` (Blom 1958), y the value.
#[must_use]
pub fn normal_quantile_points(sorted: &[(usize, f64)]) -> Vec<RowPoint> {
    let n = sorted.len() as f64;
    let z = Normal::standard();
    sorted
        .iter()
        .enumerate()
        .filter_map(|(i, &(row, y))| {
            let p = (i as f64 + 1.0 - 0.375) / (n + 0.25);
            z.quantile(p).ok().map(|x| RowPoint { row, x, y })
        })
        .collect()
}

/// Confidence level of the pairwise-comparison intervals.
const PAIRWISE_LEVEL: f64 = 0.95;

fn section(kind: SectionKind, title: &str, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.to_owned(), content }
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn x_range(points: &[RowPoint]) -> Option<(f64, f64)> {
    let (lo, hi) = points.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.x), b.max(p.x)));
    (lo <= hi).then_some((lo, hi))
}

/// The line y = x across the joint range of the points.
fn diagonal(points: &[RowPoint]) -> Option<Polyline> {
    let (lo, hi) =
        points.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.x).min(p.y), b.max(p.x).max(p.y)));
    (lo <= hi).then(|| vec![[lo, lo], [hi, hi]])
}

impl crate::surface_profiler::SurfaceModel for LinearModelAnalysis {
    fn response_name(&self) -> &str {
        &self.response_name
    }

    fn profile_factors(&self) -> Vec<crate::surface_profiler::ProfileFactor> {
        use crate::surface_profiler::{ProfileFactor, ProfileFactorKind};
        self.design
            .terms()
            .iter()
            .enumerate()
            .map(|(j, term)| ProfileFactor {
                name: term.name.clone(),
                kind: match &term.kind {
                    TermKind::Continuous => {
                        let (lo, hi, _) = self.continuous_summary[j].unwrap_or((0.0, 1.0, 0.5));
                        ProfileFactorKind::Continuous { range: (lo, hi) }
                    }
                    TermKind::Categorical { levels } => ProfileFactorKind::Categorical { levels: levels.clone() },
                },
            })
            .collect()
    }

    fn default_profile_settings(&self) -> Vec<TermValue> {
        self.default_settings()
    }

    fn predict_settings(&self, points: &[Vec<TermValue>]) -> Result<Vec<f64>> {
        if points.iter().any(|s| !self.settings_fit(s)) {
            return Err(invalid("the settings do not match the model's terms"));
        }
        Ok(self.predict_points(points)?.into_iter().map(|p| p.fitted).collect())
    }

    fn predict_with_limits(&self, points: &[Vec<TermValue>]) -> Result<Vec<crate::surface_profiler::Limited>> {
        if points.iter().any(|s| !self.settings_fit(s)) {
            return Err(invalid("the settings do not match the model's terms"));
        }
        Ok(self.predict_points(points)?.into_iter().map(|p| (p.fitted, Some(p.confidence_interval))).collect())
    }

    fn confidence_level(&self) -> Option<f64> {
        Some(self.fit.confidence_level())
    }

    fn observations(&self) -> Vec<crate::surface_profiler::Observation> {
        self.observations.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::{Column, SelectionMode};

    fn table() -> DataTable {
        DataTable::new(
            "t",
            vec![
                Column::numeric("x", vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]),
                Column::numeric("y", vec![2.0, 4.0, 5.0, 4.0, 5.0, f64::NAN]),
                Column::numeric("z", vec![0.5, 0.1, 0.9, 0.3, 0.7, 0.2]),
                Column::text("g", vec!["a".into(); 6]),
            ],
        )
        .unwrap()
    }

    fn simple_spec() -> LinearModelSpec {
        LinearModelSpec { response: 1, predictors: vec![0], effects: Vec::new(), splines: Vec::new() }
    }

    /// Response with a group shift and a covariate, for ANCOVA tests.
    fn ancova_table() -> DataTable {
        let x = vec![1.0, 2.0, 3.0, 4.0, 1.5, 2.5, 3.5, 4.5, 1.2, 2.2, 3.4, 4.1, 2.0];
        let y = vec![10.1, 11.8, 13.2, 14.9, 12.0, 13.1, 15.2, 16.8, 9.4, 10.9, 12.7, 13.6, 30.0];
        let g: Vec<String> =
            ["p", "p", "p", "p", "q", "q", "q", "q", "r", "r", "r", "r", "s"].iter().map(|s| (*s).to_owned()).collect();
        DataTable::new("a", vec![Column::numeric("x", x), Column::numeric("y", y), Column::text("g", g)]).unwrap()
    }

    #[test]
    fn fits_complete_rows_and_reports() {
        let t = table();
        let states = RowStateManager::new(t.n_rows());
        let a = LinearModelAnalysis::run(&t, &states, &simple_spec()).unwrap();
        assert_eq!(a.rows_used(), &[0, 1, 2, 3, 4], "row 5 has a missing response");
        assert_eq!(a.title(), "Linear Model: y ~ x");
        // Same data as stat-linear's hand-checked example: slope 0.6, intercept 2.2.
        let sections = a.sections(&ReportOptions::default());
        let kinds: Vec<_> = sections.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::FitStatistics,
                SectionKind::EffectPareto,
                SectionKind::AnalysisOfVariance,
                SectionKind::ParameterEstimates,
                SectionKind::FittedLine,
                SectionKind::PredictionFormula
            ]
        );
        let SectionContent::Table(params) = &sections[3].content else { panic!("expected a table") };
        assert_eq!(params.cell(1, "Term"), Some("x"));
        assert_eq!(params.cell(1, "Estimate"), Some("0.6"));
        assert_eq!(params.cell(0, "Estimate"), Some("2.2"));
        assert!(params.columns.contains(&"Lower 95%".to_owned()));
        let SectionContent::Table(stats) = &sections[0].content else { panic!() };
        assert!(stats.rows.iter().any(|r| r[0] == "Rows with missing values" && r[1] == "1"));
    }

    /// Saved predictions cover rows without a response (new rows to score),
    /// as the prediction formula does; residuals need a response.
    #[test]
    fn saved_predictions_include_rows_without_a_response() {
        let t = table();
        let a = LinearModelAnalysis::run(&t, &RowStateManager::new(6), &simple_spec()).unwrap();
        let columns = a.saveable_columns();
        let column = |key: &str| &columns.iter().find(|c| c.key == key).unwrap().values;
        let predicted = column("predicted");
        // y = 2.2 + 0.6 x, so row 5 (x = 6, y missing) predicts 5.8.
        assert!((predicted[5] - 5.8).abs() < 1e-10, "{predicted:?}");
        assert!((predicted[0] - 2.8).abs() < 1e-10, "{predicted:?}");
        assert!((column("prediction_formula")[5] - 5.8).abs() < 1e-10);
        assert!(column("residual")[5].is_nan());
        assert!((column("residual")[0] - (2.0 - 2.8)).abs() < 1e-10);
        let formula = crate::formula::evaluate(&t, None, &a.prediction_formula().unwrap(), 5..6).unwrap();
        assert_eq!(formula.values.len(), 1);
        let crate::formula::FormulaValue::Number(v) = formula.values[0] else { panic!("{formula:?}") };
        assert!((v - predicted[5]).abs() < 1e-10);
    }

    #[test]
    fn fitted_line_and_band_bracket_the_line() {
        let t = table();
        let a = LinearModelAnalysis::run(&t, &RowStateManager::new(6), &simple_spec()).unwrap();
        let s = a.sections(&ReportOptions::default());
        let SectionContent::Plot(plot) = &s[4].content else { panic!("expected a plot") };
        assert_eq!(plot.points.len(), 5);
        let line = plot.line.as_ref().unwrap();
        assert_eq!(line.first().unwrap()[0], 1.0);
        assert_eq!(line.last().unwrap()[0], 5.0);
        assert!((line[0][1] - (2.2 + 0.6)).abs() < 1e-12);
        let (lo, hi) = plot.band.as_ref().unwrap();
        for ((l, m), u) in lo.iter().zip(line).zip(hi) {
            assert!(l[1] < m[1] && m[1] < u[1]);
        }
    }

    #[test]
    fn options_toggle_sections() {
        let t = table();
        let a = LinearModelAnalysis::run(&t, &RowStateManager::new(6), &simple_spec()).unwrap();
        let opts = ReportOptions {
            show_anova: false,
            show_confidence_limits: false,
            show_fit_plot: false,
            show_confidence_band: false,
            show_residual_plot: true,
            show_diagnostics: false,
            show_profiler: false,
            show_surface_profiler: false,
            show_surface_observed: false,
            pairwise: None,
            show_equal_variances: false,
            control_level: None,
            best: None,
            show_effect_sizes: false,
            linear_trend: false,
            slices: None,
            slopes: None,
        };
        let s = a.sections(&opts);
        let kinds: Vec<_> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::FitStatistics,
                SectionKind::EffectPareto,
                SectionKind::ParameterEstimates,
                SectionKind::ResidualsVsPredicted,
                SectionKind::PredictionFormula
            ]
        );
        let SectionContent::Table(params) = &s[2].content else { panic!() };
        assert_eq!(params.columns.len(), 5, "no confidence-limit columns");
        let SectionContent::Plot(res) = &s[3].content else { panic!() };
        let sum: f64 = res.points.iter().map(|p| p.y).sum();
        assert!(sum.abs() < 1e-12, "residuals of an intercept model sum to zero");
    }

    #[test]
    fn exclusion_makes_analysis_stale_and_refit_drops_rows() {
        let t = table();
        let mut states = RowStateManager::new(6);
        let a = LinearModelAnalysis::run(&t, &states, &simple_spec()).unwrap();
        states.select([1], SelectionMode::Replace);
        assert!(!a.is_stale(&states), "selection alone does not invalidate the fit");
        states.exclude_selected();
        assert!(a.is_stale(&states));
        let b = LinearModelAnalysis::run(&t, &states, &simple_spec()).unwrap();
        assert_eq!(b.rows_used(), &[0, 2, 3, 4]);
        assert!(!b.is_stale(&states));
        // The excluded row is still plotted, so it can be seen and re-included.
        let s = b.sections(&ReportOptions::default());
        let SectionContent::Plot(plot) = &s[4].content else { panic!() };
        assert!(plot.points.iter().any(|p| p.row == 1));
    }

    #[test]
    fn several_predictors_use_observed_vs_predicted_and_effect_tests() {
        let t = table();
        let spec = LinearModelSpec { response: 1, predictors: vec![0, 2], effects: Vec::new(), splines: Vec::new() };
        let a = LinearModelAnalysis::run(&t, &RowStateManager::new(6), &spec).unwrap();
        assert_eq!(a.title(), "Linear Model: y ~ x + z");
        let s = a.sections(&ReportOptions::default());
        assert_eq!(s[4].kind, SectionKind::EffectTests);
        assert_eq!(s[5].kind, SectionKind::ObservedVsPredicted);
        let SectionContent::Plot(plot) = &s[5].content else { panic!() };
        assert_eq!(plot.line_label(), "Reference line y = x");
        let with_residuals = a.sections(&ReportOptions { show_residual_plot: true, ..ReportOptions::default() });
        let res = with_residuals.iter().find(|x| x.kind == SectionKind::ResidualsVsPredicted).unwrap();
        let SectionContent::Plot(r) = &res.content else { panic!() };
        assert_eq!(r.line_label(), "Zero");
        for (p, &row) in plot.points.iter().zip(a.rows_used()) {
            assert!(
                (p.x - a.fit().fitted_values()[a.rows_used().iter().position(|&r| r == row).unwrap()]).abs() < 1e-12
            );
        }
        // With continuous terms, each effect F equals the square of its t.
        for (e, p) in a.effect_tests().iter().zip(&a.fit().parameters()[1..]) {
            let t = p.t_statistic.unwrap();
            assert!((e.f_statistic.unwrap() - t * t).abs() < 1e-8 * t * t);
        }
    }

    #[test]
    fn rejects_bad_specs() {
        let t = table();
        let s = RowStateManager::new(6);
        let run = |response, predictors: Vec<usize>| {
            LinearModelAnalysis::run(
                &t,
                &s,
                &LinearModelSpec { response, predictors, effects: Vec::new(), splines: Vec::new() },
            )
        };
        assert!(run(1, vec![]).is_err());
        assert!(run(1, vec![1]).is_err());
        assert!(run(1, vec![3]).is_err(), "a categorical predictor with one level");
        assert!(run(3, vec![0]).is_err(), "text response");
        assert!(run(1, vec![9]).is_err());
    }

    #[test]
    fn non_hierarchical_models_get_a_note() {
        let t = ancova_table();
        let mut states = RowStateManager::new(t.n_rows());
        // Row 12 is the only row at level s.
        states.set_excluded([12], true);
        let note = |effects: Vec<Vec<(usize, u8)>>| {
            let spec = LinearModelSpec { response: 1, predictors: vec![0, 2], effects, splines: Vec::new() };
            let a = LinearModelAnalysis::run(&t, &states, &spec).unwrap();
            a.sections(&ReportOptions::default()).into_iter().find(|s| s.kind == SectionKind::HierarchyNote)
        };
        assert!(note(vec![vec![(0, 1)], vec![(1, 1)], vec![(0, 1), (1, 1)]]).is_none());
        let s = note(vec![vec![(1, 1)], vec![(0, 1), (1, 1)]]).expect("x*g without x");
        let SectionContent::Note(text) = s.content else { panic!() };
        assert!(text.contains("without x"), "{text}");
    }

    #[test]
    fn categorical_predictors_and_unknown_levels() {
        let t = ancova_table();
        let mut states = RowStateManager::new(t.n_rows());
        // Row 12 is the only "s" row; exclude it, so level s is unknown to the fit.
        states.set_excluded([12], true);
        let spec = LinearModelSpec { response: 1, predictors: vec![0, 2], effects: Vec::new(), splines: Vec::new() };
        let a = LinearModelAnalysis::run(&t, &states, &spec).unwrap();
        assert!(a.design().has_categorical());
        let names: Vec<&str> = a.fit().parameters().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Intercept", "x", "g: q", "g: r"]);
        let s = a.sections(&ReportOptions::default());
        assert!(s[3].title.contains("reference levels: g = p"));
        let SectionContent::Table(effects) = &s[4].content else { panic!() };
        assert_eq!(effects.cell(1, "Term"), Some("g"));
        assert_eq!(effects.cell(1, "DF"), Some("2"));
        // The excluded row at the unknown level is left out of the plot.
        let SectionContent::Plot(plot) = &s[5].content else { panic!() };
        assert_eq!(plot.points.len(), 12);
        assert!(plot.points.iter().all(|p| p.row != 12));
    }

    #[test]
    fn one_way_models_get_pairwise_comparisons_and_variance_tests() {
        let a = [24.5, 23.5, 26.4, 27.1, 29.9];
        let b = [28.4, 34.2, 29.5, 32.2, 30.1];
        let c = [26.1, 28.3, 24.3, 26.2, 27.8];
        let y: Vec<f64> = a.iter().chain(&b).chain(&c).copied().collect();
        let g: Vec<String> = (0..15).map(|i| ["A", "B", "C"][i / 5].to_owned()).collect();
        let x: Vec<f64> = (0..15).map(f64::from).collect();
        let t =
            DataTable::new("t", vec![Column::numeric("y", y), Column::text("g", g), Column::numeric("x", x)]).unwrap();
        let rows = RowStateManager::new(15);
        let one_way = LinearModelSpec { response: 0, predictors: vec![1], effects: Vec::new(), splines: Vec::new() };
        let a = LinearModelAnalysis::run(&t, &rows, &one_way).unwrap();
        assert!(a.is_one_way());
        let s = a.sections(&ReportOptions::default());
        // Default: Tukey–Kramer on the model's error mean square, matching the
        // Compare report on the same data (SciPy tukey_hsd: A−B p = 0.01445).
        let tukey = s.iter().find(|x| x.kind == SectionKind::PairwiseComparisons).expect("pairwise section");
        assert!(tukey.title.starts_with("All Pairs: Tukey–Kramer"));
        let SectionContent::Table(tab) = &tukey.content else { panic!() };
        assert_eq!(tab.rows[0][5], "0.0144");
        assert_eq!(tab.rows[0][4], "12", "error df N − k");
        // Equal-variance tests come first, so the choice of method can follow them.
        let pos = |k: SectionKind| s.iter().position(|x| x.kind == k).unwrap();
        assert!(pos(SectionKind::EqualVariances) < pos(SectionKind::PairwiseComparisons));
        let SectionContent::Table(var) = &s[pos(SectionKind::EqualVariances)].content else { panic!() };
        assert_eq!(var.rows.len(), 3);
        assert!(var.rows[1][0].starts_with("Brown–Forsythe"));
        let opts = |m| ReportOptions { pairwise: Some(m), ..ReportOptions::default() };
        let s = a.sections(&opts(ComparisonMethod::HolmT));
        let holm = s.iter().find(|x| x.kind == SectionKind::PairwiseComparisons).unwrap();
        assert!(holm.title.contains("Holm"));
        let s = a.sections(&opts(ComparisonMethod::GamesHowell));
        let gh = s.iter().find(|x| x.kind == SectionKind::PairwiseComparisons).expect("Games–Howell section");
        assert!(gh.title.contains("Games–Howell"));
        let SectionContent::Table(tab) = &gh.content else { panic!() };
        // SciPy (studentized_range) on the Welch df of each pair:
        // A−B: q 4.2841, df 7.9602, p 0.03906; A−C: q 0.27915, p 0.97882; B−C: q 4.8953, p 0.02470.
        assert_eq!(tab.rows.len(), 3);
        assert_eq!(tab.rows[0][0], "A − B");
        assert_eq!(tab.rows[0][3], "4.2841");
        assert_eq!(tab.rows[0][4], "7.96");
        assert_eq!(tab.rows[0][5], "0.0391");
        assert_eq!(tab.rows[1][5], "0.9788");
        assert_eq!(tab.rows[2][5], "0.0247");
        let off =
            a.sections(&ReportOptions { pairwise: None, show_equal_variances: false, ..ReportOptions::default() });
        assert!(!off.iter().any(|x| matches!(x.kind, SectionKind::PairwiseComparisons | SectionKind::EqualVariances)));
        // A covariate or a second effect makes it not one-way.
        let ancova = LinearModelSpec { response: 0, predictors: vec![1, 2], effects: Vec::new(), splines: Vec::new() };
        let a = LinearModelAnalysis::run(&t, &rows, &ancova).unwrap();
        assert!(!a.is_one_way());
        assert!(
            !a.sections(&ReportOptions::default())
                .iter()
                .any(|x| matches!(x.kind, SectionKind::PairwiseComparisons | SectionKind::EqualVariances))
        );
        // A single continuous predictor is not one-way either.
        let slr = LinearModelSpec { response: 0, predictors: vec![2], effects: Vec::new(), splines: Vec::new() };
        assert!(!LinearModelAnalysis::run(&t, &rows, &slr).unwrap().is_one_way());
    }

    #[test]
    fn nominal_numeric_columns_are_categorical() {
        let mut t = ancova_table();
        let codes: Vec<f64> = (0..t.n_rows()).map(|r| (r % 3) as f64).collect();
        t.insert_column(3, Column::numeric("batch", codes)).unwrap();
        t.set_level(3, MeasurementLevel::Nominal).unwrap();
        let spec = LinearModelSpec { response: 1, predictors: vec![0, 3], effects: Vec::new(), splines: Vec::new() };
        let a = LinearModelAnalysis::run(&t, &RowStateManager::new(t.n_rows()), &spec).unwrap();
        assert_eq!(a.fit().parameters().len(), 4, "intercept, x and two indicators");
    }

    #[test]
    fn diagnostics_sections() {
        let t = ancova_table();
        let spec = LinearModelSpec { response: 1, predictors: vec![0], effects: Vec::new(), splines: Vec::new() };
        let a = LinearModelAnalysis::run(&t, &RowStateManager::new(t.n_rows()), &spec).unwrap();
        let opts = ReportOptions { show_diagnostics: true, ..ReportOptions::default() };
        let s = a.sections(&opts);
        let kinds: Vec<_> = s.iter().map(|x| x.kind).collect();
        assert!(kinds.ends_with(&[
            SectionKind::InfluenceDiagnostics,
            SectionKind::CooksDistance,
            SectionKind::ResidualQuantiles,
            SectionKind::PredictionFormula
        ]));
        // Row 13 (index 12, y = 30) is an outlier: flagged first.
        let SectionContent::Table(infl) = &s[s.len() - 4].content else { panic!("expected flagged rows") };
        assert_eq!(infl.cell(0, "Row"), Some("13"));
        assert!(infl.cell(0, "Flags").unwrap().contains("large residual"));
        let SectionContent::Plot(cook) = &s[s.len() - 3].content else { panic!() };
        assert_eq!(cook.points.len(), 13);
        // The reference line is the 4/n threshold, not a fit, and says so in the key.
        assert_eq!(cook.line_label(), "4/n threshold");
        assert!(cook.line.as_ref().unwrap().iter().all(|p| (p[1] - 4.0 / 13.0).abs() < 1e-12));
        assert_eq!(cook.point_label(), "Rows");
        let SectionContent::Plot(qq) = &s[s.len() - 2].content else { panic!() };
        assert_eq!(qq.line_label(), "Normal reference line");
        assert!(qq.points.windows(2).all(|w| w[0].x < w[1].x && w[0].y <= w[1].y));
        assert_eq!(qq.points.last().unwrap().row, 12);
    }

    #[test]
    fn response_explorer() {
        let t = ancova_table();
        let mut states = RowStateManager::new(t.n_rows());
        states.set_excluded([12], true);
        let spec = LinearModelSpec { response: 1, predictors: vec![0, 2], effects: Vec::new(), splines: Vec::new() };
        let a = LinearModelAnalysis::run(&t, &states, &spec).unwrap();
        let settings = a.default_settings();
        assert!(a.settings_fit(&settings));
        let TermValue::Number(x_mean) = settings[0] else { panic!() };
        assert!((x_mean - 32.9 / 12.0).abs() < 1e-12);
        assert_eq!(settings[1], TermValue::Level(0), "all levels tie at 4 rows: first");
        let p = a.profile(&settings).unwrap();
        // The prediction equals the fitted equation at the settings.
        let c = a.fit().coefficients();
        assert!((p.predicted - (c[0] + c[1] * x_mean)).abs() < 1e-10);
        let ProfilePanel::Continuous { curve, band, range, .. } = &p.panels[0] else { panic!() };
        assert_eq!(*range, (1.0, 4.5));
        assert_eq!(curve.len(), crate::surface_profiler::CURVE_POINTS);
        assert!(band.as_ref().unwrap().0.iter().zip(curve).all(|(l, m)| l[1] < m[1]));
        let ProfilePanel::Categorical { levels, predictions, selected, .. } = &p.panels[1] else { panic!() };
        assert_eq!(levels, &["p", "q", "r"]);
        assert_eq!(*selected, 0);
        assert!((predictions[1].0 - predictions[0].0 - c[2]).abs() < 1e-10, "level shift = coefficient");
        let ci = p.confidence_interval.unwrap();
        assert!(p.y_range.0 < ci.0 && p.y_range.1 > ci.1);
        // Changing a setting moves the prediction along the panel's curve.
        let moved = a.profile(&[TermValue::Number(4.0), TermValue::Level(2)]).unwrap();
        assert!((moved.predicted - (c[0] + 4.0 * c[1] + c[3])).abs() < 1e-10);
        assert!(a.profile(&[TermValue::Level(0), TermValue::Level(0)]).is_err());
        assert!(!a.settings_fit(&[TermValue::Number(1.0), TermValue::Level(3)]));
    }

    /// Polynomial terms through the effects list reproduce NIST StRD certified
    /// fit statistics (invariant to the centring used): Pontius (quadratic) and
    /// Wampler 3–5 (quintic).
    #[test]
    fn polynomial_effects_match_strd() {
        for d in stat_strd::linear_regression_datasets() {
            let degree: u8 = match d.name {
                "Pontius" => 2,
                "Wampler3" | "Wampler4" | "Wampler5" => 5,
                _ => continue,
            };
            let x = d.predictors.col(0).to_vec();
            let table = DataTable::new(
                d.name,
                vec![stat_data::Column::numeric("x", x), stat_data::Column::numeric("y", d.response.clone())],
            )
            .unwrap();
            let rows = RowStateManager::new(table.n_rows());
            let spec = LinearModelSpec {
                response: 1,
                predictors: vec![0],
                effects: (1..=degree).map(|p| vec![(0, p)]).collect(),
                splines: Vec::new(),
            };
            let a = LinearModelAnalysis::run(&table, &rows, &spec).unwrap();
            let s = a.fit().summary();
            let c = &d.certified;
            let rel = |x: f64, y: f64| (x - y).abs() / y.abs();
            assert!(rel(s.root_mean_square_error, c.residual_sd) < 1e-8, "{}: {}", d.name, s.root_mean_square_error);
            assert!((s.r_squared - c.r_squared).abs() < 1e-10, "{}", d.name);
            assert_eq!(a.effect_tests().len(), usize::from(degree));
            assert!(a.title().ends_with("x + x²") || a.title().contains("x⁵"), "{}", a.title());
            // The fitted curve follows the polynomial through the design.
            let sections = a.sections(&ReportOptions::default());
            let plot = sections.iter().find(|s| s.kind == SectionKind::FittedLine).unwrap();
            let SectionContent::Plot(p) = &plot.content else { panic!() };
            assert!(p.line.as_ref().is_some_and(|l| l.len() > 10));
        }
    }

    /// Interaction effects feed the Response Explorer: with y = 1 + 2x + 3z + 4xz
    /// exactly, the predicted slope in x depends on z.
    #[test]
    fn interaction_effects_in_the_profiler() {
        let x: Vec<f64> = (0..12).map(|i| f64::from(i % 4)).collect();
        let z: Vec<f64> = (0..12).map(|i| f64::from(i / 4)).collect();
        let y: Vec<f64> = x.iter().zip(&z).map(|(a, b)| 1.0 + 2.0 * a + 3.0 * b + 4.0 * a * b).collect();
        let table = DataTable::new(
            "t",
            vec![
                stat_data::Column::numeric("x", x),
                stat_data::Column::numeric("z", z),
                stat_data::Column::numeric("y", y),
            ],
        )
        .unwrap();
        let rows = RowStateManager::new(12);
        let spec = LinearModelSpec {
            response: 2,
            predictors: vec![0, 1],
            effects: vec![vec![(0, 1)], vec![(1, 1)], vec![(0, 1), (1, 1)]],
            splines: Vec::new(),
        };
        let a = LinearModelAnalysis::run(&table, &rows, &spec).unwrap();
        assert!(a.fit().summary().r_squared > 1.0 - 1e-12);
        assert_eq!(a.title(), "Linear Model: y ~ x + z + x × z");
        let at = |z: f64| {
            let p = a.profile(&[TermValue::Number(1.0), TermValue::Number(z)]).unwrap();
            let p2 = a.profile(&[TermValue::Number(2.0), TermValue::Number(z)]).unwrap();
            p2.predicted - p.predicted
        };
        assert!((at(0.0) - 2.0).abs() < 1e-9 && (at(2.0) - 10.0).abs() < 1e-9, "slope 2 + 4z");
    }
}
