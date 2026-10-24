//! Model terms and design matrices, including categorical predictors and
//! interaction and polynomial effects.
//!
//! A **continuous** predictor contributes one column, its values. A
//! **categorical** predictor with levels `L₁ < L₂ < … < L_k` (sorted, numerically
//! when every label is a number) contributes `k − 1` columns, one for each level
//! other than the first. In a model with main effects only, they are reference
//! indicators (1 at that level, 0 elsewhere; Draper & Smith, *Applied
//! Regression Analysis*, 3rd ed., ch. 14), so each coefficient is a difference
//! from the first level. A model mixing continuous and categorical predictors
//! is an analysis of covariance.
//!
//! # Higher-order effects
//! [`ModelDesign::with_effects`] builds any set of **effects**, each a product
//! of predictors raised to powers: `A*B` (interaction), `x^2` (polynomial),
//! `A*x` (separate slopes). An effect's columns are the products of its
//! factors' columns (every combination for categorical factors). In
//! higher-order effects, continuous predictors are **centred** at their mean
//! over the rows used, `(x − x̄)^p`, so that main effects keep their meaning
//! (the effect at the average of the other predictors) and polynomial columns
//! are far less collinear (Montgomery, Peck & Vining, *Introduction to Linear
//! Regression Analysis*, §7.1). When a categorical predictor appears in a
//! higher-order effect, all categorical predictors switch to **sum-to-zero**
//! coding (level `j > 1`: 1 at that level, −1 at the first level, 0 elsewhere),
//! the coding under which the partial F-tests below are the standard "Type III"
//! tests (Searle, *Linear Models for Unbalanced Data*, §8). Predictions and
//! fit statistics do not depend on either choice.
//!
//! [`effect_tests`] gives one partial F-test per effect — all of its columns
//! together — comparing the full model with the model that omits that effect:
//! `F = [(SSE_reduced − SSE_full)/df_effect] / MSE_full`.

use crate::ols::{LinearModelFit, OlsOptions, fit_linear_model};
use crate::spline::{SplineBasis, SplineSpec};
use stat_core::distributions::{ContinuousDistribution, FisherSnedecor};
use stat_core::summation::{dot, mean};
use stat_core::{Matrix, Result, StatError};
use std::ops::Range;

/// Highest power accepted for a continuous predictor in an effect.
pub const MAX_POWER: u8 = 5;

/// One predictor's values for every row of a dataset.
#[derive(Debug, Clone, Copy)]
pub enum PredictorValues<'a> {
    /// Numeric values.
    Continuous(&'a [f64]),
    /// Category labels.
    Categorical(&'a [String]),
}

/// A named predictor.
#[derive(Debug, Clone, Copy)]
pub struct Predictor<'a> {
    /// Name used for its term and columns.
    pub name: &'a str,
    /// Values for every row.
    pub values: PredictorValues<'a>,
}

/// How a predictor enters the design.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermKind {
    /// One column of values.
    Continuous,
    /// Columns for `levels[1..]`; `levels[0]` is the first (reference) level.
    Categorical {
        /// Sorted levels present in the rows the design was built from.
        levels: Vec<String>,
    },
}

/// One predictor (factor) of the model. For a main-effects design, `columns`
/// are its main-effect columns; otherwise the columns of its main effect if
/// the model has one (an empty range if not).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelTerm {
    /// Predictor name.
    pub name: String,
    /// Continuous or categorical.
    pub kind: TermKind,
    /// Its main-effect columns in the design matrix.
    pub columns: Range<usize>,
}

/// One effect of the model: a product of predictors raised to powers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelEffect {
    /// Name such as `"x"`, `"A*B"` or `"x^2"`.
    pub name: String,
    /// `(predictor index, power)` pairs, in predictor order.
    pub factors: Vec<(usize, u8)>,
    /// Its columns in the design matrix.
    pub columns: Range<usize>,
}

impl ModelEffect {
    /// `true` for a single predictor to the first power.
    #[must_use]
    pub fn is_main(&self) -> bool {
        matches!(self.factors.as_slice(), [(_, 1)])
    }
}

/// How categorical predictors are coded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CategoricalCoding {
    /// 0/1 indicators of each level but the first.
    Reference,
    /// Sum-to-zero (effect) coding: the first level is −1 in every column.
    SumToZero,
}

/// A setting of one predictor, for encoding a single prediction point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TermValue {
    /// Value of a continuous predictor.
    Number(f64),
    /// Index into a categorical predictor's `levels`.
    Level(usize),
}

/// The coding of a set of predictors and effects into design-matrix columns.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelDesign {
    terms: Vec<ModelTerm>,
    effects: Vec<ModelEffect>,
    column_names: Vec<String>,
    coding: CategoricalCoding,
    /// Mean of each continuous predictor over the rows used (0 for categorical).
    centers: Vec<f64>,
    /// Spline basis of each predictor entered as a spline (`None` otherwise).
    splines: Vec<Option<SplineBasis>>,
}

/// Order labels numerically when both are numbers, otherwise as text (moved
/// to `stat-data`, which table sorting shares; re-exported here).
pub use stat_data::natural_order;

/// The main effects of `k` predictors: `[[(0, 1)], [(1, 1)], …]`.
#[must_use]
pub fn main_effects(k: usize) -> Vec<Vec<(usize, u8)>> {
    (0..k).map(|i| vec![(i, 1)]).collect()
}

/// Effect name such as `A × B` or `x²`.
#[must_use]
pub fn effect_name(names: &[&str], factors: &[(usize, u8)]) -> String {
    factors
        .iter()
        .map(|&(i, p)| {
            if p == 1 {
                names[i].to_owned()
            } else {
                format!("{}{}", names[i], stat_core::text::superscript(&p.to_string()))
            }
        })
        .collect::<Vec<_>>()
        .join(" × ")
}

/// Name of the effect of a predictor entered as a spline, e.g. `x (spline)`.
#[must_use]
pub fn spline_effect_name(name: &str) -> String {
    format!("{name} (spline)")
}

impl ModelDesign {
    /// Main-effects design: one effect per predictor, reference coding.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] if there are no predictors, or a
    /// categorical predictor has fewer than two levels in `rows`.
    pub fn new(predictors: &[Predictor<'_>], rows: &[usize]) -> Result<Self> {
        Self::with_effects(predictors, rows, &main_effects(predictors.len()))
    }

    /// Design with the given effects, each a list of `(predictor index,
    /// power)` pairs (levels, centres and coding from the values in `rows`).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] if there are no predictors or effects, an
    /// effect is empty, repeats a predictor or another effect, refers to an
    /// unknown predictor, raises a categorical predictor to a power above 1 or
    /// a continuous one above [`MAX_POWER`], or a categorical predictor has
    /// fewer than two levels in `rows`.
    pub fn with_effects(predictors: &[Predictor<'_>], rows: &[usize], effects: &[Vec<(usize, u8)>]) -> Result<Self> {
        Self::with_splines(predictors, rows, effects, &[])
    }

    /// [`ModelDesign::with_effects`] with some continuous predictors entered
    /// as splines: `splines[i]` (missing entries mean none) expands
    /// predictor `i`'s main effect into the columns of a spline basis
    /// ([`crate::spline`]) built from its values in `rows`.
    ///
    /// # Errors
    /// As [`ModelDesign::with_effects`]; also for a spline on a categorical
    /// predictor, a spline predictor in an interaction or power, or a spline
    /// that cannot be built ([`SplineSpec::basis`]).
    pub fn with_splines(
        predictors: &[Predictor<'_>],
        rows: &[usize],
        effects: &[Vec<(usize, u8)>],
        splines: &[Option<SplineSpec>],
    ) -> Result<Self> {
        if predictors.is_empty() {
            return Err(invalid("a model needs at least one predictor"));
        }
        if effects.is_empty() {
            return Err(invalid("a model needs at least one effect"));
        }
        // Predictors: kinds, levels and centres.
        let mut kinds = Vec::with_capacity(predictors.len());
        let mut centers = Vec::with_capacity(predictors.len());
        for p in predictors {
            match p.values {
                PredictorValues::Continuous(v) => {
                    let used: Vec<f64> = rows.iter().map(|&r| v[r]).filter(|x| x.is_finite()).collect();
                    centers.push(mean(&used).unwrap_or(0.0));
                    kinds.push(TermKind::Continuous);
                }
                PredictorValues::Categorical(labels) => {
                    let mut levels: Vec<String> = rows.iter().map(|&r| labels[r].clone()).collect();
                    levels.sort_by(|a, b| natural_order(a, b));
                    levels.dedup();
                    if levels.len() < 2 {
                        return Err(invalid(format!(
                            "{:?} has only one level among the rows used, so it cannot explain any variation",
                            p.name
                        )));
                    }
                    centers.push(0.0);
                    kinds.push(TermKind::Categorical { levels });
                }
            }
        }
        // Spline bases from the values in the rows used.
        let mut bases: Vec<Option<SplineBasis>> = vec![None; predictors.len()];
        for (i, spec) in splines.iter().enumerate().take(predictors.len()) {
            let Some(spec) = spec else { continue };
            let PredictorValues::Continuous(v) = predictors[i].values else {
                return Err(invalid(format!(
                    "{:?} is categorical, so it cannot enter as a spline",
                    predictors[i].name
                )));
            };
            let used: Vec<f64> = rows.iter().map(|&r| v[r]).collect();
            bases[i] = Some(spec.basis(&used).map_err(|e| invalid(format!("{:?}: {e}", predictors[i].name)))?);
        }
        // Effects: validate and normalize.
        let names: Vec<&str> = predictors.iter().map(|p| p.name).collect();
        let mut specs: Vec<Vec<(usize, u8)>> = Vec::with_capacity(effects.len());
        for e in effects {
            let mut f = e.clone();
            f.sort_unstable();
            if f.is_empty() {
                return Err(invalid("an effect needs at least one predictor"));
            }
            if f.windows(2).any(|w| w[0].0 == w[1].0) {
                return Err(invalid("an effect may use each predictor once (use a power instead)"));
            }
            for &(i, p) in &f {
                let kind = kinds.get(i).ok_or_else(|| invalid(format!("no predictor {i}")))?;
                let max = if matches!(kind, TermKind::Continuous) { MAX_POWER } else { 1 };
                if p == 0 || p > max {
                    return Err(invalid(format!(
                        "{:?} cannot be raised to the power {p} (categorical predictors only enter to the first power)",
                        names[i]
                    )));
                }
            }
            if specs.contains(&f) {
                return Err(invalid(format!("the effect {} is listed twice", effect_name(&names, &f))));
            }
            if f.len() > 1 || f[0].1 > 1 {
                if let Some(&(i, _)) = f.iter().find(|&&(i, _)| bases[i].is_some()) {
                    return Err(invalid(format!(
                        "{:?} enters as a spline, so it cannot also be in an interaction or power",
                        names[i]
                    )));
                }
            }
            specs.push(f);
        }
        let higher_categorical = specs.iter().any(|f| {
            !matches!(f.as_slice(), [(_, 1)])
                && f.iter().any(|&(i, _)| matches!(kinds[i], TermKind::Categorical { .. }))
        });
        let coding = if higher_categorical { CategoricalCoding::SumToZero } else { CategoricalCoding::Reference };
        // Columns.
        let mut column_names = Vec::new();
        let mut model_effects = Vec::with_capacity(specs.len());
        for f in specs {
            let start = column_names.len();
            let mut labels = vec![String::new()];
            for &(i, p) in &f {
                let parts: Vec<String> = match &kinds[i] {
                    TermKind::Continuous if bases[i].is_some() => {
                        (1..=bases[i].as_ref().map_or(0, SplineBasis::n_columns))
                            .map(|k| format!("{}: spline {k}", names[i]))
                            .collect()
                    }
                    TermKind::Continuous if p == 1 => vec![names[i].to_owned()],
                    TermKind::Continuous => vec![format!("{}^{p}", names[i])],
                    TermKind::Categorical { levels } => {
                        levels[1..].iter().map(|l| format!("{}: {l}", names[i])).collect()
                    }
                };
                labels = labels
                    .iter()
                    .flat_map(|a| {
                        parts.iter().map(move |b| if a.is_empty() { b.clone() } else { format!("{a} * {b}") })
                    })
                    .collect();
            }
            column_names.extend(labels);
            let name = match f.as_slice() {
                [(i, 1)] if bases[*i].is_some() => spline_effect_name(names[*i]),
                _ => effect_name(&names, &f),
            };
            model_effects.push(ModelEffect { name, factors: f, columns: start..column_names.len() });
        }
        let terms = predictors
            .iter()
            .zip(kinds)
            .enumerate()
            .map(|(i, (p, kind))| {
                let columns =
                    model_effects.iter().find(|e| e.factors.as_slice() == [(i, 1)]).map_or(0..0, |e| e.columns.clone());
                ModelTerm { name: p.name.to_owned(), kind, columns }
            })
            .collect();
        Ok(Self { terms, effects: model_effects, column_names, coding, centers, splines: bases })
    }

    /// The spline basis of predictor `i`, when it enters as a spline.
    #[must_use]
    pub fn spline(&self, i: usize) -> Option<&SplineBasis> {
        self.splines.get(i).and_then(Option::as_ref)
    }

    /// `true` if any predictor enters as a spline.
    #[must_use]
    pub fn has_splines(&self) -> bool {
        self.splines.iter().any(Option::is_some)
    }

    /// The predictors, in order.
    #[must_use]
    pub fn terms(&self) -> &[ModelTerm] {
        &self.terms
    }

    /// The effects, in model order.
    #[must_use]
    pub fn effects(&self) -> &[ModelEffect] {
        &self.effects
    }

    /// `true` if every effect is a main effect.
    #[must_use]
    pub fn is_main_effects(&self) -> bool {
        self.effects.iter().all(ModelEffect::is_main)
    }

    /// Coding of the categorical predictors.
    #[must_use]
    pub fn coding(&self) -> CategoricalCoding {
        self.coding
    }

    /// Centre of each predictor: the mean of a continuous predictor over the
    /// rows the design was built from (0 for a categorical one). Continuous
    /// factors of interactions and powers are coded as `(x − centre)^p`.
    #[must_use]
    pub fn centers(&self) -> &[f64] {
        &self.centers
    }

    /// Names of the design columns (parameter names, without the intercept).
    #[must_use]
    pub fn column_names(&self) -> &[String] {
        &self.column_names
    }

    /// `true` if any predictor is categorical.
    #[must_use]
    pub fn has_categorical(&self) -> bool {
        self.terms.iter().any(|t| matches!(t.kind, TermKind::Categorical { .. }))
    }

    /// Encode one point (a value per predictor) as a design row.
    ///
    /// # Errors
    /// [`StatError::DimensionMismatch`] for the wrong number of values;
    /// [`StatError::InvalidArgument`] for a value of the wrong kind or an
    /// unknown level.
    pub fn encode(&self, values: &[TermValue]) -> Result<Vec<f64>> {
        if values.len() != self.terms.len() {
            return Err(StatError::DimensionMismatch {
                context: "term values",
                expected: self.terms.len(),
                found: values.len(),
            });
        }
        // Each predictor's own columns: the value, or the level coding.
        let mut basis: Vec<Vec<f64>> = Vec::with_capacity(self.terms.len());
        for (term, value) in self.terms.iter().zip(values) {
            basis.push(match (&term.kind, *value) {
                (TermKind::Continuous, TermValue::Number(x)) => vec![x],
                (TermKind::Categorical { levels }, TermValue::Level(l)) if l < levels.len() => {
                    let mut c = vec![0.0; levels.len() - 1];
                    if l > 0 {
                        c[l - 1] = 1.0;
                    } else if self.coding == CategoricalCoding::SumToZero {
                        c.iter_mut().for_each(|v| *v = -1.0);
                    }
                    c
                }
                _ => return Err(invalid(format!("invalid value for term {:?}", term.name))),
            });
        }
        let mut row = Vec::with_capacity(self.column_names.len());
        for effect in &self.effects {
            let main = effect.is_main();
            let mut cols = vec![1.0];
            for &(i, p) in &effect.factors {
                let part: Vec<f64> = match self.terms[i].kind {
                    TermKind::Continuous if main && self.spline(i).is_some() => {
                        self.spline(i).map_or_else(Vec::new, |s| s.evaluate(basis[i][0]))
                    }
                    TermKind::Continuous => {
                        let x = basis[i][0];
                        vec![if main { x } else { (x - self.centers[i]).powi(i32::from(p)) }]
                    }
                    TermKind::Categorical { .. } => basis[i].clone(),
                };
                cols = cols.iter().flat_map(|a| part.iter().map(move |b| a * b)).collect();
            }
            row.extend(cols);
        }
        Ok(row)
    }

    /// The design row for dataset row `r`, or `None` if a categorical value is
    /// not one of the design's levels (e.g. a level seen only in excluded rows).
    #[must_use]
    pub fn row(&self, predictors: &[Predictor<'_>], r: usize) -> Option<Vec<f64>> {
        let values: Option<Vec<TermValue>> = self
            .terms
            .iter()
            .zip(predictors)
            .map(|(term, p)| match (&term.kind, p.values) {
                (TermKind::Continuous, PredictorValues::Continuous(v)) => Some(TermValue::Number(v[r])),
                (TermKind::Categorical { levels }, PredictorValues::Categorical(labels)) => {
                    levels.iter().position(|l| *l == labels[r]).map(TermValue::Level)
                }
                _ => None,
            })
            .collect();
        self.encode(&values?).ok()
    }

    /// The design matrix for `rows`.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] if a row has a level the design does not know.
    pub fn matrix(&self, predictors: &[Predictor<'_>], rows: &[usize]) -> Result<Matrix> {
        let encoded: Vec<Vec<f64>> = rows
            .iter()
            .map(|&r| self.row(predictors, r).ok_or_else(|| invalid(format!("row {r} has an unknown level"))))
            .collect::<Result<_>>()?;
        if encoded.is_empty() {
            return Ok(Matrix::zeros(0, self.column_names.len()));
        }
        Matrix::from_rows(&encoded)
    }
}

/// Partial F-test of one term.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectTest {
    /// Effect name.
    pub term: String,
    /// Degrees of freedom (number of the term's columns).
    pub df: usize,
    /// Increase in the error sum of squares when the term is removed.
    pub sum_of_squares: f64,
    /// `(SS/df) / MSE`; `None` if `MSE = 0`.
    pub f_statistic: Option<f64>,
    /// Upper-tail p-value.
    pub p_value: Option<f64>,
}

/// One partial F-test per effect of `design`, given the full fit to `x`, `y`.
///
/// # Errors
/// Any error from refitting a reduced model.
pub fn effect_tests(
    design: &ModelDesign,
    x: &Matrix,
    y: &[f64],
    fit: &LinearModelFit,
    options: &OlsOptions,
) -> Result<Vec<EffectTest>> {
    let sse_full = fit.anova().error.sum_of_squares;
    let df_error = fit.anova().error.df;
    let mse = fit.anova().error.mean_square.unwrap_or(f64::NAN);
    let mut out = Vec::with_capacity(design.effects.len());
    for term in &design.effects {
        let keep: Vec<usize> = (0..x.ncols()).filter(|j| !term.columns.contains(j)).collect();
        let sse_reduced = if keep.is_empty() {
            // Intercept-only (or empty) model.
            if options.intercept {
                let m = mean(y).unwrap_or(0.0);
                let c: Vec<f64> = y.iter().map(|v| v - m).collect();
                dot(&c, &c)
            } else {
                dot(y, y)
            }
        } else {
            let cols: Vec<&[f64]> = keep.iter().map(|&j| x.col(j)).collect();
            let names = keep.iter().map(|&j| design.column_names[j].clone()).collect();
            let reduced = fit_linear_model(
                &Matrix::from_columns(&cols)?,
                y,
                &OlsOptions { predictor_names: names, ..options.clone() },
            )?;
            reduced.anova().error.sum_of_squares
        };
        let df = term.columns.len();
        let ss = (sse_reduced - sse_full).max(0.0);
        let f_statistic = (mse > 0.0).then(|| ss / df as f64 / mse);
        let p_value = match f_statistic {
            Some(f) => Some(FisherSnedecor::new(df as f64, df_error as f64)?.sf(f)?),
            None => None,
        };
        out.push(EffectTest { term: term.name.clone(), df, sum_of_squares: ss, f_statistic, p_value });
    }
    Ok(out)
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anova::one_way_anova;

    fn labels(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn indicator_coding_and_natural_level_order() {
        let g = labels(&["10", "9", "b", "a", "9", "10"]);
        let x = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let preds = [
            Predictor { name: "x", values: PredictorValues::Continuous(&x) },
            Predictor { name: "g", values: PredictorValues::Categorical(&g) },
        ];
        let d = ModelDesign::new(&preds, &[0, 1, 2, 3, 4, 5]).unwrap();
        let TermKind::Categorical { levels } = &d.terms()[1].kind else { panic!() };
        assert_eq!(levels, &["9", "10", "a", "b"], "numbers first, numerically");
        assert_eq!(d.column_names(), ["x", "g: 10", "g: a", "g: b"]);
        assert_eq!(d.terms()[1].columns, 1..4);
        assert_eq!(d.row(&preds, 0).unwrap(), [1.0, 1.0, 0.0, 0.0]);
        assert_eq!(d.row(&preds, 1).unwrap(), [2.0, 0.0, 0.0, 0.0], "reference level");
        assert_eq!(d.encode(&[TermValue::Number(7.0), TermValue::Level(3)]).unwrap(), [7.0, 0.0, 0.0, 1.0]);
        assert!(d.encode(&[TermValue::Level(0), TermValue::Level(0)]).is_err());
        // Levels come from the rows given; another level is unknown.
        let d2 = ModelDesign::new(&preds, &[0, 1]).unwrap();
        assert!(d2.row(&preds, 2).is_none());
        assert!(ModelDesign::new(&preds, &[1, 4]).is_err(), "one level only");
    }

    /// A single categorical predictor reproduces the one-way ANOVA exactly.
    #[test]
    fn single_factor_model_equals_one_way_anova() {
        let y = [4.2, 4.8, 5.1, 3.9, 5.6, 6.1, 5.9, 6.8, 4.9, 5.0, 5.3];
        let g = labels(&["A", "A", "A", "A", "B", "B", "B", "B", "C", "C", "C"]);
        let preds = [Predictor { name: "g", values: PredictorValues::Categorical(&g) }];
        let rows: Vec<usize> = (0..y.len()).collect();
        let d = ModelDesign::new(&preds, &rows).unwrap();
        let x = d.matrix(&preds, &rows).unwrap();
        let opts = OlsOptions { predictor_names: d.column_names().to_vec(), ..OlsOptions::default() };
        let fit = fit_linear_model(&x, &y, &opts).unwrap();
        let a = one_way_anova(&[&y[0..4], &y[4..8], &y[8..]]).unwrap();
        assert!((fit.anova().model.sum_of_squares - a.ss_between).abs() < 1e-12);
        assert!((fit.anova().error.sum_of_squares - a.ss_within).abs() < 1e-12);
        // Intercept = mean of the reference level; coefficients = differences.
        let c = fit.coefficients();
        assert!((c[0] - a.groups[0].mean).abs() < 1e-12);
        assert!((c[1] - (a.groups[1].mean - a.groups[0].mean)).abs() < 1e-12);
        let tests = effect_tests(&d, &x, &y, &fit, &opts).unwrap();
        assert_eq!(tests[0].df, 2);
        assert!((tests[0].f_statistic.unwrap() - a.f_statistic.unwrap()).abs() < 1e-10);
        assert!((tests[0].p_value.unwrap() - a.p_value.unwrap()).abs() < 1e-12);
    }

    /// Analysis of covariance: the effect test of the covariate equals the
    /// square of its t statistic, and the factor's test matches a refit.
    #[test]
    fn ancova_effect_tests() {
        let y = [10.1, 11.8, 13.2, 14.9, 12.0, 13.1, 15.2, 16.8, 9.4, 10.9, 12.7, 13.6];
        let x = [1.0, 2.0, 3.0, 4.0, 1.5, 2.5, 3.5, 4.5, 1.2, 2.2, 3.4, 4.1];
        let g = labels(&["p", "p", "p", "p", "q", "q", "q", "q", "r", "r", "r", "r"]);
        let preds = [
            Predictor { name: "x", values: PredictorValues::Continuous(&x) },
            Predictor { name: "g", values: PredictorValues::Categorical(&g) },
        ];
        let rows: Vec<usize> = (0..y.len()).collect();
        let d = ModelDesign::new(&preds, &rows).unwrap();
        let xm = d.matrix(&preds, &rows).unwrap();
        let opts = OlsOptions { predictor_names: d.column_names().to_vec(), ..OlsOptions::default() };
        let fit = fit_linear_model(&xm, &y, &opts).unwrap();
        let tests = effect_tests(&d, &xm, &y, &fit, &opts).unwrap();
        let t_x = fit.parameters()[1].t_statistic.unwrap();
        assert!((tests[0].f_statistic.unwrap() - t_x * t_x).abs() < 1e-9 * t_x * t_x);
        assert_eq!((tests[0].df, tests[1].df), (1, 2));
        // Factor test by hand: refit with x only.
        let only_x = fit_linear_model(&Matrix::from_columns(&[&x[..]]).unwrap(), &y, &OlsOptions::default()).unwrap();
        let ss = only_x.anova().error.sum_of_squares - fit.anova().error.sum_of_squares;
        assert!((tests[1].sum_of_squares - ss).abs() < 1e-10);
    }

    /// Balanced two-way layout with interaction: the Type III sums of squares
    /// equal the classical ones, SS_A = bn Σ(ȳᵢ.. − ȳ)², SS_B = an Σ(ȳ.ⱼ. − ȳ)²,
    /// SS_AB = n ΣΣ(ȳᵢⱼ. − ȳᵢ.. − ȳ.ⱼ. + ȳ)².
    #[test]
    fn two_way_interaction_type_iii() {
        let a = labels(&["a1", "a1", "a1", "a1", "a1", "a1", "a2", "a2", "a2", "a2", "a2", "a2"]);
        let b = labels(&["b1", "b1", "b2", "b2", "b3", "b3", "b1", "b1", "b2", "b2", "b3", "b3"]);
        let y = [12.0, 14.0, 18.0, 17.0, 21.0, 24.0, 15.0, 13.0, 25.0, 27.0, 20.0, 19.0];
        let preds = [
            Predictor { name: "A", values: PredictorValues::Categorical(&a) },
            Predictor { name: "B", values: PredictorValues::Categorical(&b) },
        ];
        let rows: Vec<usize> = (0..12).collect();
        let effects = vec![vec![(0, 1)], vec![(1, 1)], vec![(0, 1), (1, 1)]];
        let d = ModelDesign::with_effects(&preds, &rows, &effects).unwrap();
        assert_eq!(d.coding(), CategoricalCoding::SumToZero);
        assert_eq!(d.column_names(), ["A: a2", "B: b2", "B: b3", "A: a2 * B: b2", "A: a2 * B: b3"]);
        assert_eq!(d.effects()[2].name, "A × B");
        assert_eq!(d.row(&preds, 0).unwrap(), [-1.0, -1.0, -1.0, 1.0, 1.0], "first levels are −1");
        let x = d.matrix(&preds, &rows).unwrap();
        let opts = OlsOptions { predictor_names: d.column_names().to_vec(), ..OlsOptions::default() };
        let fit = fit_linear_model(&x, &y, &opts).unwrap();
        let tests = effect_tests(&d, &x, &y, &fit, &opts).unwrap();
        let grand = y.iter().sum::<f64>() / 12.0;
        let cell = |i: usize, j: usize| (y[6 * i + 2 * j] + y[6 * i + 2 * j + 1]) / 2.0;
        let a_mean = |i: usize| (0..3).map(|j| cell(i, j)).sum::<f64>() / 3.0;
        let b_mean = |j: usize| (0..2).map(|i| cell(i, j)).sum::<f64>() / 2.0;
        let ss_a = 6.0 * (0..2).map(|i| (a_mean(i) - grand).powi(2)).sum::<f64>();
        let ss_b = 4.0 * (0..3).map(|j| (b_mean(j) - grand).powi(2)).sum::<f64>();
        let ss_ab = 2.0
            * (0..2)
                .flat_map(|i| (0..3).map(move |j| (i, j)))
                .map(|(i, j)| (cell(i, j) - a_mean(i) - b_mean(j) + grand).powi(2))
                .sum::<f64>();
        for (t, (ss, df)) in tests.iter().zip([(ss_a, 1), (ss_b, 2), (ss_ab, 2)]) {
            assert!((t.sum_of_squares - ss).abs() < 1e-9 * ss.max(1.0), "{}: {} vs {ss}", t.term, t.sum_of_squares);
            assert_eq!(t.df, df);
        }
        // The full factorial model fits the cell means exactly.
        assert!(
            (fit.anova().error.sum_of_squares
                - 2.0 * 6.0 * 0.0
                - y.iter().enumerate().map(|(k, v)| (v - cell(k / 6, (k % 6) / 2)).powi(2)).sum::<f64>())
            .abs()
                < 1e-9
        );
    }

    #[test]
    fn polynomial_and_continuous_interactions_are_centred() {
        let x = [1.0, 2.0, 3.0, 4.0, 5.0];
        let z = [2.0, 0.0, 1.0, 3.0, 4.0];
        let g = labels(&["p", "q", "p", "q", "p"]);
        let preds = [
            Predictor { name: "x", values: PredictorValues::Continuous(&x) },
            Predictor { name: "z", values: PredictorValues::Continuous(&z) },
            Predictor { name: "g", values: PredictorValues::Categorical(&g) },
        ];
        let rows: Vec<usize> = (0..5).collect();
        let effects = vec![vec![(0, 1)], vec![(1, 1)], vec![(0, 2)], vec![(1, 1), (0, 1)]];
        let d = ModelDesign::with_effects(&preds, &rows, &effects).unwrap();
        assert_eq!(d.coding(), CategoricalCoding::Reference, "g only as a main effect (here not at all)");
        assert_eq!(d.column_names(), ["x", "z", "x^2", "x * z"]);
        assert_eq!(d.effects()[3].name, "x × z", "factors in predictor order");
        // x̄ = 3, z̄ = 2: row 0 → x = 1, z = 2, (1 − 3)² = 4, (1 − 3)(2 − 2) = 0.
        assert_eq!(d.row(&preds, 0).unwrap(), [1.0, 2.0, 4.0, -0.0]);
        assert_eq!(d.terms()[2].columns, 0..0, "g has no main effect here");
        assert!(!d.is_main_effects());
        // Separate slopes: g*x switches g to sum-to-zero coding.
        let slopes = vec![vec![(0, 1)], vec![(2, 1)], vec![(0, 1), (2, 1)]];
        let s = ModelDesign::with_effects(&preds, &rows, &slopes).unwrap();
        assert_eq!(s.coding(), CategoricalCoding::SumToZero);
        assert_eq!(s.row(&preds, 0).unwrap(), [1.0, -1.0, 2.0], "x, g (p is −1), (x − 3)·(−1)");
        // Invalid effects.
        for bad in [
            vec![vec![(2, 2)]],
            vec![vec![(0, 6)]],
            vec![vec![(0, 1), (0, 1)]],
            vec![vec![(0, 1)], vec![(0, 1)]],
            vec![vec![(9, 1)]],
            vec![vec![]],
            vec![],
        ] {
            assert!(ModelDesign::with_effects(&preds, &rows, &bad).is_err(), "{bad:?}");
        }
    }
}
