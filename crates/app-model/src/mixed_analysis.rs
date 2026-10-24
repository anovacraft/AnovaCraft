//! Linear and generalized linear mixed models bound to a dataset
//! ([`stat_mixed`]).
//!
//! Fixed effects are coded as in the linear model (continuous predictors as
//! they are, categorical ones as indicators, interactions and powers when
//! asked for). Each random-effects term names a grouping column, optionally
//! nested within other columns (its levels are then the combinations), with a
//! random intercept and/or random slopes on continuous columns. A binomial
//! response is either a two-valued column (the second value in sorted order is
//! the event) or a count of successes with a column of numbers of trials.
//! An ordinal response is a column of ordered categories (natural order
//! unless `order` says otherwise), fitted by the cumulative logit model.
//!
//! A frequency column repeats each row that many times (whole numbers; 0
//! drops the row). A weight column gives prior weights: the residual
//! variance of a normal response (or the dispersion of a Gamma one) is
//! divided by the weight, and the log-likelihood contributions of Poisson,
//! negative binomial and ordinal responses are multiplied by it.

use crate::format::{format_number, format_p_value};
use crate::model_data::ModelData;
use crate::report::{PlotSpec, ReportSection, RowPoint, SectionContent, SectionKind, TextTable};
use crate::report_columns::ReportColumn;
use stat_core::{Matrix, Result, StatError};
use stat_data::{DataTable, MeasurementLevel, RowStateManager};
use stat_linear::{ModelDesign, TermValue, natural_order};
use stat_mixed::{
    ContrastTest, DfMethod, MixedFamily, MixedFit, MixedModel, MixedOptions, RandomTerm, ResidualGroups, fit_mixed,
};

const DIGITS: usize = 6;

/// One random-effects term.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RandomEffectSpec {
    /// Grouping column.
    pub group: usize,
    /// Columns the grouping is nested within (its levels are then the
    /// combinations of these columns' levels with the group's).
    pub within: Vec<usize>,
    /// A random intercept for each level.
    pub intercept: bool,
    /// Continuous columns with a random slope for each level.
    pub slopes: Vec<usize>,
    /// Estimate correlations between the intercept and slopes.
    pub correlated: bool,
}

impl RandomEffectSpec {
    /// A random intercept for each level of `group`.
    #[must_use]
    pub fn intercept(group: usize) -> Self {
        Self { group, within: Vec::new(), intercept: true, slopes: Vec::new(), correlated: true }
    }

    /// Every column the term uses.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        std::iter::once(self.group).chain(self.within.iter().copied()).chain(self.slopes.iter().copied()).collect()
    }
}

/// Columns, family and options of a mixed model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MixedRequest {
    /// Response column (successes when `trials` is given).
    pub response: usize,
    /// Fixed-effect predictor columns (may be empty).
    pub predictors: Vec<usize>,
    /// Fixed effects as `(position in predictors, power)` products; empty
    /// means main effects.
    pub effects: Vec<Vec<(usize, u8)>>,
    /// Random-effects terms (at least one).
    pub random: Vec<RandomEffectSpec>,
    /// Response distribution.
    pub family: MixedFamily,
    /// Binomial: column of numbers of trials (the response is then a count
    /// of successes).
    pub trials: Option<usize>,
    /// Offset column on the linear-predictor scale (e.g. log exposure).
    pub offset: Option<usize>,
    /// Normal family: REML (otherwise maximum likelihood).
    pub reml: bool,
    /// Other families: 1 for the Laplace approximation, more for adaptive
    /// Gauss–Hermite quadrature (one random intercept only).
    pub quadrature_points: usize,
    /// Comparisons within the slices of each two-factor interaction of
    /// categorical fixed effects, pairs adjusted by this method (`None`: off).
    pub slices: Option<stat_linear::marginal::PairAdjust>,
    /// Slopes of each `factor × continuous` fixed effect by level (`None`: off).
    pub slopes: Option<stat_linear::marginal::PairAdjust>,
    /// Column of frequencies: each row counts this many times.
    pub frequency: Option<usize>,
    /// Column of prior weights (not for the binomial family, whose trials
    /// play that part).
    pub weight: Option<usize>,
    /// Normal family: cluster-robust (CR2) standard errors, clustered on the
    /// outermost grouping.
    pub robust: bool,
    /// Normal family: a residual variance per level of this column.
    pub residual_groups: Option<usize>,
    /// Normal family: denominator degrees of freedom of the tests.
    pub df_method: DfMethod,
    /// Ordinal family: order of the response categories, lowest first
    /// (categories it does not list follow in natural order).
    pub order: Vec<String>,
}

/// Contrasts of a mixed model's fixed effects: F tests with Satterthwaite
/// df for a normal response, Wald χ² otherwise ([`MixedFit::test_contrast`]).
struct MixedInference<'a>(&'a MixedFit);

impl stat_linear::marginal::ContrastInference for MixedInference<'_> {
    fn n_coefficients(&self) -> usize {
        self.0.fixed.len()
    }

    fn estimate(&self, l: &[f64]) -> Result<stat_linear::marginal::ContrastEstimate> {
        let p = self.0.fixed.len();
        if l.len() != p {
            return Err(StatError::DimensionMismatch { context: "contrast row", expected: p, found: l.len() });
        }
        let c = &self.0.fixed_covariance;
        let estimate = l.iter().zip(&self.0.fixed).map(|(a, f)| a * f.estimate).sum();
        let var: f64 = (0..p).map(|i| l[i] * (0..p).map(|j| c[(i, j)] * l[j]).sum::<f64>()).sum();
        let df = self.0.test_contrast(&[l.to_vec()]).ok().and_then(|t| t.den_df);
        Ok(stat_linear::marginal::ContrastEstimate { estimate, std_error: var.max(0.0).sqrt(), df })
    }

    fn joint(&self, l: &[Vec<f64>]) -> Result<stat_linear::marginal::JointTest> {
        let t = self.0.test_contrast(l)?;
        let f = if t.den_df.is_some() { t.statistic } else { t.statistic / t.num_df as f64 };
        Ok(stat_linear::marginal::JointTest { num_df: t.num_df, den_df: t.den_df, f, p_value: t.p_value })
    }
}

impl MixedRequest {
    /// Every column the model uses.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut out = vec![self.response];
        out.extend(&self.predictors);
        for r in &self.random {
            out.extend(r.columns());
        }
        out.extend(self.trials);
        out.extend(self.offset);
        out.extend(self.frequency);
        out.extend(self.weight);
        out.extend(self.residual_groups);
        out
    }

    /// The same request with every column index passed through `f`.
    #[must_use]
    pub fn map_columns(mut self, f: impl Fn(usize) -> usize) -> Self {
        self.response = f(self.response);
        self.predictors.iter_mut().for_each(|c| *c = f(*c));
        for r in &mut self.random {
            r.group = f(r.group);
            r.within.iter_mut().for_each(|c| *c = f(*c));
            r.slopes.iter_mut().for_each(|c| *c = f(*c));
        }
        self.trials = self.trials.map(&f);
        self.offset = self.offset.map(&f);
        self.frequency = self.frequency.map(&f);
        self.weight = self.weight.map(&f);
        self.residual_groups = self.residual_groups.map(&f);
        self
    }
}

/// A mixed model's inputs gathered from a table: the coded data, the
/// fixed-effects design and the model (random terms possibly empty).
pub(crate) struct BuiltModel {
    pub data: ModelData,
    pub design: Option<ModelDesign>,
    pub model: MixedModel,
    /// Rows used.
    pub rows: Vec<usize>,
    /// Rows left out for a missing value in any column of the model.
    pub n_missing: usize,
}

/// Gather the complete, included rows of `request` and code its fixed and
/// random effects (shared with the Bayesian models, which allow no random
/// effects).
///
/// # Errors
/// Invalid columns, a response unsuited to the family, random terms with
/// neither intercept nor slopes, or no complete rows.
pub(crate) fn build_model(table: &DataTable, states: &RowStateManager, request: &MixedRequest) -> Result<BuiltModel> {
    if request.trials.is_some() && request.family != MixedFamily::Binomial {
        return Err(invalid("a trials column applies to the binomial family only"));
    }
    let normal = request.family == MixedFamily::Normal;
    if request.weight.is_some() && request.family == MixedFamily::Binomial {
        return Err(invalid(
            "a binomial response takes no weight column: use a trials column, or a frequency column for repeated rows",
        ));
    }
    if !normal && (request.residual_groups.is_some() || request.robust || request.df_method != DfMethod::Satterthwaite)
    {
        return Err(invalid(
            "separate residual variances, robust standard errors and between–within degrees of freedom apply to \
                 normal responses only",
        ));
    }
    let ordinal = request.family == MixedFamily::Ordinal;
    let binary = request.family == MixedFamily::Binomial && request.trials.is_none();
    let data = if ordinal {
        ModelData::gather_categorical(table, states, request.response, &request.predictors, &request.order)?
    } else {
        ModelData::gather_any(table, states, request.response, &request.predictors, binary)?
    };
    let column = |i: usize| table.column(i).ok_or_else(|| invalid(format!("no column {i}")));
    let numeric = |i: usize, what: &str| -> Result<Vec<f64>> {
        let c = column(i)?;
        c.as_numeric()
            .map(<[f64]>::to_vec)
            .ok_or_else(|| invalid(format!("the {what} column {:?} must be numeric", c.name)))
    };
    let labels = |i: usize| -> Result<Vec<String>> {
        let c = column(i)?;
        Ok((0..table.n_rows()).map(|r| c.level_text(r)).collect())
    };
    // Extra columns: groupings (as labels), slopes, trials, offset.
    let mut group_labels = Vec::new();
    let mut slope_values = Vec::new();
    for (k, spec) in request.random.iter().enumerate() {
        if !spec.intercept && spec.slopes.is_empty() {
            return Err(invalid(format!("random effect {} has neither an intercept nor slopes", k + 1)));
        }
        let parts: Vec<Vec<String>> =
            std::iter::once(spec.group).chain(spec.within.iter().copied()).map(&labels).collect::<Result<_>>()?;
        group_labels.push(parts);
        let mut slopes = Vec::new();
        for &s in &spec.slopes {
            let c = column(s)?;
            if c.level != MeasurementLevel::Continuous {
                return Err(invalid(format!("random slopes need a continuous column; {:?} is not", c.name)));
            }
            slopes.push(numeric(s, "slope")?);
        }
        slope_values.push(slopes);
    }
    let trials = request.trials.map(|t| numeric(t, "trials")).transpose()?;
    let offset_col = request.offset.map(|o| numeric(o, "offset")).transpose()?;
    let freq_col = request.frequency.map(|o| numeric(o, "frequency")).transpose()?;
    let weight_col = request.weight.map(|o| numeric(o, "weight")).transpose()?;
    let res_labels = request.residual_groups.map(labels).transpose()?;
    if let Some(f) = &freq_col {
        if data.rows_used.iter().any(|&r| f[r].is_finite() && (f[r] < 0.0 || f[r].fract() != 0.0)) {
            return Err(invalid("frequencies must be whole numbers, 0 or more"));
        }
    }
    if let Some(w) = &weight_col {
        if data.rows_used.iter().any(|&r| w[r].is_finite() && w[r] < 0.0) {
            return Err(invalid("weights must not be negative"));
        }
    }
    let ok = |r: usize| {
        group_labels.iter().flatten().all(|l| !l[r].trim().is_empty())
            && slope_values.iter().flatten().all(|v| v[r].is_finite())
            && trials.as_ref().is_none_or(|t| t[r].is_finite() && t[r] > 0.0)
            && offset_col.as_ref().is_none_or(|o| o[r].is_finite())
            && freq_col.as_ref().is_none_or(|f| f[r].is_finite() && f[r] > 0.0)
            && weight_col.as_ref().is_none_or(|w| w[r].is_finite() && w[r] > 0.0)
            && res_labels.as_ref().is_none_or(|l| !l[r].trim().is_empty())
    };
    let mut data = data;
    let before = data.rows_used.len();
    data.rows_used.retain(|&r| ok(r));
    let n_missing = data.n_missing + (before - data.rows_used.len());
    // Each row repeated by its frequency.
    let rows: Vec<usize> = match &freq_col {
        None => data.rows_used.clone(),
        Some(f) => data.rows_used.iter().flat_map(|&r| std::iter::repeat_n(r, f[r] as usize)).collect(),
    };
    let n = rows.len();
    if n == 0 {
        return Err(invalid("no rows have values for every column of the model"));
    }
    if n > 5_000_000 {
        return Err(invalid("the frequencies add up to more than 5 million observations"));
    }
    // An ordinal response's categories: those of the rows used, codes 0, 1, ….
    if ordinal {
        let present: Vec<bool> =
            (0..data.categories.len()).map(|k| rows.iter().any(|&r| data.y_all[r] == k as f64)).collect();
        let remap: Vec<f64> = present
            .iter()
            .scan(0.0, |next, &p| {
                let code = if p { *next } else { f64::NAN };
                if p {
                    *next += 1.0;
                }
                Some(code)
            })
            .collect();
        for v in &mut data.y_all {
            if v.is_finite() {
                *v = remap[*v as usize];
            }
        }
        data.categories = data.categories.iter().zip(&present).filter(|(_, p)| **p).map(|(c, _)| c.clone()).collect();
        if data.categories.len() < 2 {
            return Err(invalid(format!(
                "the ordinal response {:?} needs two or more categories among the rows used",
                data.response_name
            )));
        }
    }

    // Response and prior weights.
    let (y, weights) = match &trials {
        Some(t) => {
            let mut y = Vec::with_capacity(n);
            for &r in &rows {
                let (s, m) = (data.y_all[r], t[r]);
                if s < 0.0 || s > m || (s - s.round()).abs() > 1e-9 || (m - m.round()).abs() > 1e-9 {
                    return Err(invalid("successes must be whole numbers from 0 to the number of trials"));
                }
                y.push(s / m);
            }
            (y, rows.iter().map(|&r| t[r]).collect())
        }
        None => {
            let w = weight_col.as_ref().map_or_else(|| vec![1.0; n], |w| rows.iter().map(|&r| w[r]).collect());
            (rows.iter().map(|&r| data.y_all[r]).collect::<Vec<f64>>(), w)
        }
    };
    let offset: Vec<f64> = offset_col.as_ref().map_or_else(|| vec![0.0; n], |o| rows.iter().map(|&r| o[r]).collect());

    // Fixed effects: intercept plus the coded predictors.
    let (design, x, x_names) = if request.predictors.is_empty() {
        if ordinal {
            (None, Matrix::zeros(n, 0), Vec::new())
        } else {
            (None, Matrix::from_columns(&[vec![1.0; n]])?, vec!["Intercept".to_owned()])
        }
    } else {
        let preds = data.borrowed();
        let effects = if request.effects.is_empty() {
            stat_linear::main_effects(request.predictors.len())
        } else {
            request.effects.clone()
        };
        let design = ModelDesign::with_effects(&preds, &rows, &effects)?;
        let m = design.matrix(&preds, &rows)?;
        // The thresholds of an ordinal model take the intercept's place.
        let mut cols: Vec<Vec<f64>> = if ordinal { Vec::new() } else { vec![vec![1.0; n]] };
        cols.extend((0..m.ncols()).map(|j| m.col(j).to_vec()));
        let intercept = (!ordinal).then(|| "Intercept".to_owned());
        let names = intercept.into_iter().chain(design.column_names().iter().cloned()).collect();
        (Some(design), Matrix::from_columns(&cols)?, names)
    };

    // Random-effects terms.
    let mut random = Vec::with_capacity(request.random.len());
    for (k, spec) in request.random.iter().enumerate() {
        let parts = &group_labels[k];
        // Levels: combinations, outer columns first, in natural order.
        let key = |r: usize| -> Vec<String> { parts.iter().rev().map(|p| p[r].clone()).collect() };
        let mut levels: Vec<Vec<String>> = rows.iter().map(|&r| key(r)).collect();
        levels.sort_by(|a, b| {
            a.iter().zip(b).map(|(x, y)| natural_order(x, y)).find(|o| o.is_ne()).unwrap_or(std::cmp::Ordering::Equal)
        });
        levels.dedup();
        let codes: Vec<usize> = rows
            .iter()
            .map(|&r| {
                levels
                    .binary_search_by(|l| {
                        l.iter()
                            .zip(&key(r))
                            .map(|(x, y)| natural_order(x, y))
                            .find(|o| o.is_ne())
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .unwrap_or(0)
            })
            .collect();
        let name_of = |c: usize| table.column(c).map_or(String::new(), |c| c.name.clone());
        let group = if spec.within.is_empty() {
            name_of(spec.group)
        } else {
            format!(
                "{}[{}]",
                name_of(spec.group),
                spec.within.iter().map(|&c| name_of(c)).collect::<Vec<_>>().join(", ")
            )
        };
        let mut zcols = Vec::new();
        let mut effects = Vec::new();
        if spec.intercept {
            zcols.push(vec![1.0; n]);
            effects.push("Intercept".to_owned());
        }
        for (j, &s) in spec.slopes.iter().enumerate() {
            zcols.push(rows.iter().map(|&r| slope_values[k][j][r]).collect());
            effects.push(name_of(s));
        }
        random.push(RandomTerm {
            group,
            levels: levels.iter().map(|l| l.join(" / ")).collect(),
            codes,
            effects,
            z: Matrix::from_columns(&zcols)?,
            correlated: spec.correlated,
        });
    }
    let residual_groups = match &res_labels {
        None => None,
        Some(l) => {
            let mut levels: Vec<String> = rows.iter().map(|&r| l[r].clone()).collect();
            levels.sort_by(|a, b| natural_order(a, b));
            levels.dedup();
            let codes = rows.iter().map(|&r| levels.iter().position(|v| *v == l[r]).unwrap_or(0)).collect();
            let name = request.residual_groups.and_then(|c| table.column(c)).map_or(String::new(), |c| c.name.clone());
            Some(ResidualGroups { name, levels, codes })
        }
    };
    let categories = if ordinal { data.categories.clone() } else { Vec::new() };
    let model =
        MixedModel { y, x, x_names, weights, offset, random, family: request.family, residual_groups, categories };
    Ok(BuiltModel { data, design, model, rows, n_missing })
}

/// A fitted mixed model and what its report needs.
#[derive(Debug, Clone)]
pub struct MixedAnalysis {
    request: MixedRequest,
    data: ModelData,
    design: Option<ModelDesign>,
    fit: MixedFit,
    /// Name and test of each fixed effect (term).
    effect_tests: Vec<(String, ContrastTest)>,
    y: Vec<f64>,
    weights: Vec<f64>,
    rows_used: Vec<usize>,
    /// Rows in the dataset (length of saved columns).
    n_table_rows: usize,
    n_missing: usize,
    profile_name: String,
    /// Names of the table's columns.
    column_names: Vec<String>,
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

impl MixedAnalysis {
    /// Fit the model to the complete, included rows.
    ///
    /// # Errors
    /// Invalid columns, no random effects, a response unsuited to the
    /// family, or errors from [`fit_mixed`].
    pub fn run(table: &DataTable, states: &RowStateManager, request: &MixedRequest) -> Result<Self> {
        if request.random.is_empty() {
            return Err(invalid("add at least one random effect"));
        }
        let BuiltModel { data, design, model, rows, n_missing } = build_model(table, states, request)?;
        let (y, weights) = (model.y.clone(), model.weights.clone());
        let options = MixedOptions {
            reml: request.reml,
            quadrature_points: if request.family == MixedFamily::Normal { 1 } else { request.quadrature_points.max(1) },
            robust: request.robust,
            df_method: request.df_method,
            ..MixedOptions::default()
        };
        let fit = fit_mixed(&model, &options)?;
        let mut effect_tests = Vec::new();
        let shift = usize::from(request.family != MixedFamily::Ordinal);
        if let Some(d) = &design {
            let p = fit.fixed.len();
            for e in d.effects() {
                let l: Vec<Vec<f64>> = e
                    .columns
                    .clone()
                    .map(|j| {
                        let mut row = vec![0.0; p];
                        row[j + shift] = 1.0;
                        row
                    })
                    .collect();
                if let Ok(t) = fit.test_contrast(&l) {
                    effect_tests.push((e.name.clone(), t));
                }
            }
        }
        let mut out = Self {
            request: request.clone(),
            data,
            design,
            fit,
            effect_tests,
            y,
            weights,
            rows_used: rows,
            n_table_rows: table.n_rows(),
            n_missing,
            profile_name: String::new(),
            column_names: (0..table.n_cols()).filter_map(|c| table.column(c).map(|c| c.name.clone())).collect(),
        };
        out.profile_name = out.mean_name();
        Ok(out)
    }

    /// Name of the modelled mean.
    #[must_use]
    pub fn mean_name(&self) -> String {
        if self.is_ordinal() {
            return self.data.response_name.clone();
        }
        match (&self.data.binary, self.request.trials) {
            (Some(b), _) => format!("P({} = {})", self.data.response_name, b.event),
            (None, Some(_)) => format!("Proportion of {}", self.data.response_name),
            (None, None) => self.data.response_name.clone(),
        }
    }

    /// The fit.
    #[must_use]
    pub fn fit(&self) -> &MixedFit {
        &self.fit
    }

    /// Each fixed effect's name and its coefficients' positions (the
    /// intercept is 0); empty for an intercept-only model.
    pub(crate) fn effect_columns(&self) -> Vec<(String, Vec<usize>)> {
        let shift = usize::from(!self.is_ordinal());
        self.design.as_ref().map_or_else(Vec::new, |d| {
            d.effects().iter().map(|e| (e.name.clone(), e.columns.clone().map(|j| j + shift).collect())).collect()
        })
    }

    fn is_ordinal(&self) -> bool {
        self.request.family == MixedFamily::Ordinal
    }

    /// Ordinal response: its categories, lowest first.
    #[must_use]
    pub fn categories(&self) -> &[String] {
        &self.data.categories
    }

    /// Name and F (or Wald) test of each fixed effect.
    #[must_use]
    pub fn effect_tests(&self) -> &[(String, ContrastTest)] {
        &self.effect_tests
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &MixedRequest {
        &self.request
    }

    /// Dataset rows used, parallel to the fit's fitted values.
    #[must_use]
    pub fn rows_used(&self) -> &[usize] {
        &self.rows_used
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let kind = match self.request.family {
            MixedFamily::Normal => "Linear Mixed Model",
            MixedFamily::Ordinal => "Ordinal Mixed Model",
            _ => "Generalized Linear Mixed Model",
        };
        let fixed = if self.data.predictor_names.is_empty() {
            if self.is_ordinal() { "thresholds".to_owned() } else { "1".to_owned() }
        } else {
            match &self.design {
                Some(d) => d.effects().iter().map(|e| e.name.clone()).collect::<Vec<_>>().join(" + "),
                None => self.data.predictor_names.join(" + "),
            }
        };
        let random: Vec<String> = self
            .fit
            .modes
            .iter()
            .map(|m| {
                let effects = m
                    .effects
                    .iter()
                    .map(|e| if e == "Intercept" { "1".to_owned() } else { e.clone() })
                    .collect::<Vec<_>>();
                format!("({} | {})", effects.join(" + "), m.group)
            })
            .collect();
        format!("{kind}: {} ~ {fixed} + {}", self.mean_name(), random.join(" + "))
    }

    /// One value per dataset row (NaN for rows not used).
    fn per_row(&self, values: &[f64]) -> Vec<f64> {
        let mut out = vec![f64::NAN; self.n_table_rows];
        for (&r, &v) in self.rows_used.iter().zip(values) {
            out[r] = v;
        }
        out
    }

    /// Columns a report can add to the table: predictions with and without
    /// the random effects, and residuals (each NaN for rows not used).
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        let response = self.mean_name();
        if self.is_ordinal() {
            let cats = &self.data.categories;
            let mut out: Vec<ReportColumn> = cats
                .iter()
                .enumerate()
                .map(|(k, c)| {
                    let values: Vec<f64> = self.fit.category_probabilities.iter().map(|p| p[k]).collect();
                    ReportColumn::continuous(
                        &format!("prob{}", k + 1),
                        &format!("Save probability of {c} (with random effects)"),
                        format!("P({response} = {c})"),
                        self.per_row(&values),
                    )
                })
                .collect();
            let mut most = vec![String::new(); self.n_table_rows];
            for (&r, p) in self.rows_used.iter().zip(&self.fit.category_probabilities) {
                let k = (0..p.len()).max_by(|&a, &b| p[a].total_cmp(&p[b])).unwrap_or(0);
                most[r] = cats[k].clone();
            }
            out.push(ReportColumn::text(
                "predicted",
                "Save most likely category",
                format!("Most likely {response} (with random effects)"),
                most,
            ));
            return out;
        }
        vec![
            ReportColumn::continuous(
                "conditional",
                "Save predictions with random effects",
                format!("Predicted {response} (with random effects)"),
                self.conditional_column(),
            ),
            ReportColumn::continuous(
                "marginal",
                "Save population-level predictions",
                format!("Predicted {response} (fixed effects)"),
                self.marginal_column(),
            ),
            ReportColumn::continuous(
                "residual",
                "Save residuals",
                format!("Residual {response}"),
                self.residual_column(),
            ),
        ]
    }

    /// Predicted means including the random effects, per dataset row.
    #[must_use]
    pub fn conditional_column(&self) -> Vec<f64> {
        self.per_row(&self.fit.fitted)
    }

    /// Predicted means from the fixed effects only, per dataset row.
    #[must_use]
    pub fn marginal_column(&self) -> Vec<f64> {
        self.per_row(&self.fit.fitted_marginal)
    }

    /// Residuals from the conditional means (Pearson residuals for families
    /// other than the normal), per dataset row.
    #[must_use]
    pub fn residual_column(&self) -> Vec<f64> {
        self.per_row(&self.residuals())
    }

    /// Expected category number `Σ k·pₖ` (categories numbered from 1) of
    /// each row's probabilities.
    fn expected_scores(probabilities: &[Vec<f64>]) -> Vec<f64> {
        probabilities.iter().map(|p| p.iter().enumerate().map(|(k, v)| (k + 1) as f64 * v).sum()).collect()
    }

    fn residuals(&self) -> Vec<f64> {
        let fam = self.request.family;
        if self.is_ordinal() {
            let scores = Self::expected_scores(&self.fit.category_probabilities);
            return self.y.iter().zip(&scores).map(|(y, s)| y + 1.0 - s).collect();
        }
        let k = self.fit.scale.as_ref().map_or(1.0, |s| s.value);
        self.y
            .iter()
            .zip(&self.fit.fitted)
            .zip(&self.weights)
            .map(|((&y, &mu), &w)| match fam {
                MixedFamily::Normal => y - mu,
                MixedFamily::Gamma => (y - mu) / (mu * mu * k / w).sqrt(),
                _ => (y - mu) / (fam.variance(mu, k) / w).sqrt(),
            })
            .collect()
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary()];
        if !self.request.predictors.is_empty() && !self.request.effects.is_empty() {
            out.extend(crate::report::hierarchy_note(&self.data.predictor_names, &self.request.effects));
        }
        let test = if self.fit.family == MixedFamily::Normal { "F tests, Satterthwaite df" } else { "Wald χ² tests" };
        out.extend(
            crate::report::EffectPareto::new(self.effect_tests.iter().map(|(n, t)| (n.clone(), t.p_value)), test)
                .section(),
        );
        if self.is_ordinal() {
            out.push(self.threshold_table());
        }
        if !self.fit.fixed.is_empty() {
            out.push(self.fixed_table());
        }
        if !self.effect_tests.is_empty() {
            out.push(self.effect_table());
        }
        if let (Some(design), true) = (&self.design, self.request.slices.is_some() || self.request.slopes.is_some()) {
            let scale = (self.fit.family != MixedFamily::Normal).then_some("link scale");
            out.extend(crate::marginal_sections::sections(
                design,
                &MixedInference(&self.fit),
                self.request.slices,
                self.request.slopes,
                self.fit.confidence_level(),
                scale,
            ));
        }
        out.push(self.variance_table());
        if let Some(t) = &self.fit.equal_variance_test {
            out.push(self.variance_test_table(t));
        }
        if !self.fit.correlations.is_empty() {
            out.push(self.correlation_table());
        }
        out.push(self.modes_table());
        if self.is_ordinal() {
            out.push(self.category_table());
        }
        out.push(self.fitted_plot());
        out.push(self.residual_plot());
        out
    }

    /// The tests' degrees of freedom, as a phrase for titles.
    fn df_phrase(&self) -> String {
        match self.fit.df_label.as_str() {
            "Satterthwaite" => "Satterthwaite degrees of freedom".to_owned(),
            "between–within" => "between–within degrees of freedom".to_owned(),
            "residual" => "residual degrees of freedom".to_owned(),
            other => format!("{other} degrees of freedom"),
        }
    }

    fn threshold_table(&self) -> ReportSection {
        let f = &self.fit;
        let level = format_number(f.confidence_level() * 100.0, 3);
        let mut t = TextTable::new(vec![
            "Threshold".to_owned(),
            "Estimate".to_owned(),
            "Std Error".to_owned(),
            "z".to_owned(),
            format!("Lower {level}%"),
            format!("Upper {level}%"),
            "P(at or below) at zero".to_owned(),
        ]);
        for c in &f.thresholds {
            t.push([
                c.name.clone(),
                format_number(c.estimate, DIGITS),
                format_number(c.std_error, DIGITS),
                format_number(c.statistic, 4),
                format_number(c.lower, DIGITS),
                format_number(c.upper, DIGITS),
                format_number(1.0 / (1.0 + (-c.estimate).exp()), 4),
            ]);
        }
        ReportSection {
            kind: SectionKind::Thresholds,
            title: format!(
                "Thresholds (logit P({} ≤ category) = threshold − linear predictor)",
                self.data.response_name
            ),
            content: SectionContent::Table(t),
        }
    }

    fn category_table(&self) -> ReportSection {
        let f = &self.fit;
        let mut t = TextTable::new([
            "Category",
            "Count",
            "Observed proportion",
            "Mean predicted (with random effects)",
            "Mean predicted (random effects at zero)",
        ]);
        let wsum: f64 = self.weights.iter().sum();
        for (k, c) in self.data.categories.iter().enumerate() {
            let count = self.y.iter().filter(|&&y| y as usize == k).count();
            let observed: f64 =
                self.y.iter().zip(&self.weights).filter(|&(&y, _)| y as usize == k).map(|(_, w)| w).sum::<f64>() / wsum;
            let mean = |p: &[Vec<f64>]| p.iter().zip(&self.weights).map(|(r, w)| r[k] * w).sum::<f64>() / wsum;
            t.push([
                c.clone(),
                count.to_string(),
                format_number(observed, 4),
                format_number(mean(&f.category_probabilities), 4),
                format_number(mean(&f.category_probabilities_marginal), 4),
            ]);
        }
        ReportSection {
            kind: SectionKind::CategoryProbabilities,
            title: "Category Probabilities (observed and predicted proportions)".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    fn variance_test_table(&self, test: &stat_mixed::VarianceTest) -> ReportSection {
        let mut t = TextTable::new(["Test", "χ²", "DF", "p-value"]);
        t.push([
            "Likelihood ratio, one residual variance".to_owned(),
            format_number(test.statistic, DIGITS),
            test.df.to_string(),
            format_p_value(test.p_value),
        ]);
        let by = self.request.residual_groups.map_or_else(String::new, |_| {
            self.fit.residual_by_group.first().map_or(String::new(), |_| " across groups".to_owned())
        });
        ReportSection {
            kind: SectionKind::ResidualVarianceTest,
            title: format!("Test of Equal Residual Variances{by}"),
            content: SectionContent::Table(t),
        }
    }

    fn summary(&self) -> ReportSection {
        let f = &self.fit;
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut row = |a: &str, b: String| t.push([a.to_owned(), b]);
        row("Response", self.mean_name());
        row("Distribution", f.family.label().to_owned());
        if let Some(b) = &self.data.binary {
            row("Event modelled", format!("{} = {} (versus {})", self.data.response_name, b.event, b.other));
        }
        row("Estimation", f.method.clone());
        row("Observations used", f.n_obs.to_string());
        for (group, levels) in &f.n_levels {
            row(&format!("Levels of {group}"), levels.to_string());
        }
        if self.data.n_excluded > 0 {
            row("Rows excluded", self.data.n_excluded.to_string());
        }
        if self.n_missing > 0 {
            row("Rows with missing values", self.n_missing.to_string());
        }
        let crit = if f.method == "REML" { "−2 log likelihood (REML)" } else { "−2 log likelihood" };
        row(crit, format_number(f.minus_two_log_likelihood, DIGITS));
        row("AIC", format_number(f.aic, DIGITS));
        row("BIC", format_number(f.bic, DIGITS));
        if let Some(s) = &f.scale {
            let se = s.std_error.map_or(String::new(), |e| format!(" (std error {})", format_number(e, 4)));
            row(&s.name, format!("{}{se}", format_number(s.value, DIGITS)));
        }
        if self.request.offset.is_some() {
            row("Offset", "included in the linear predictor".to_owned());
        }
        if let Some(c) = self.request.frequency {
            row("Frequencies", format!("rows repeated by {}", self.column_name(c)));
        }
        if let Some(c) = self.request.weight {
            row("Prior weights", self.column_name(c));
        }
        if let Some(c) = self.request.residual_groups {
            row("Residual variance", format!("separate for each level of {}", self.column_name(c)));
        }
        if self.is_ordinal() {
            row("Categories (lowest first)", self.data.categories.join(", "));
            row("Coefficients", "positive values favour higher categories".to_owned());
        }
        if f.family == MixedFamily::Normal {
            match &f.robust {
                Some(r) => row(
                    "Standard errors",
                    format!("robust (CR2), clustered by {} ({} clusters)", r.cluster, r.n_clusters),
                ),
                None => row("Degrees of freedom", f.df_label.clone()),
            }
        }
        for w in &f.warnings {
            row("Warning", w.clone());
        }
        ReportSection {
            kind: SectionKind::ModelSummary,
            title: "Model Summary".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    fn column_name(&self, c: usize) -> String {
        self.column_names.get(c).cloned().unwrap_or_default()
    }

    fn fixed_table(&self) -> ReportSection {
        let f = &self.fit;
        let level = format_number(f.confidence_level() * 100.0, 3);
        let normal = f.family == MixedFamily::Normal;
        let ratio = match f.family {
            MixedFamily::Normal => None,
            MixedFamily::Binomial | MixedFamily::Ordinal => Some("Odds ratio"),
            MixedFamily::Poisson | MixedFamily::NegativeBinomial => Some("Rate ratio"),
            MixedFamily::Gamma => Some("Mean ratio"),
        };
        let mut heads: Vec<String> = vec!["Term".into(), "Estimate".into(), "Std Error".into()];
        if f.robust.is_some() {
            heads.push("Model-based Std Error".into());
        }
        if normal {
            heads.push("DF".into());
        }
        heads.extend([if normal { "t" } else { "z" }.to_owned(), "p-value".into()]);
        heads.push(format!("Lower {level}%"));
        heads.push(format!("Upper {level}%"));
        if let Some(r) = ratio {
            heads.push(r.to_owned());
            heads.push(format!("{r} lower {level}%"));
            heads.push(format!("{r} upper {level}%"));
        }
        let mut t = TextTable::new(heads);
        for (j, c) in f.fixed.iter().enumerate() {
            let mut row = vec![c.name.clone(), format_number(c.estimate, DIGITS), format_number(c.std_error, DIGITS)];
            if let Some(r) = &f.robust {
                row.push(format_number(r.model_std_errors[j], DIGITS));
            }
            if normal {
                row.push(c.df.map_or("—".to_owned(), |d| format_number(d, 4)));
            }
            row.extend([format_number(c.statistic, 4), format_p_value(c.p_value)]);
            row.push(format_number(c.lower, DIGITS));
            row.push(format_number(c.upper, DIGITS));
            if ratio.is_some() {
                if c.name == "Intercept" {
                    row.extend([String::new(), String::new(), String::new()]);
                } else {
                    row.extend([c.estimate.exp(), c.lower.exp(), c.upper.exp()].map(|v| format_number(v, DIGITS)));
                }
            }
            t.push(row);
        }
        let mut title = match (&f.robust, normal) {
            (Some(r), _) => format!(
                "Fixed Effects (robust CR2 standard errors clustered by {}, t tests with Bell–McCaffrey degrees of freedom)",
                r.cluster
            ),
            (None, true) => format!("Fixed Effects (t tests with {})", self.df_phrase()),
            (None, false) if self.is_ordinal() => {
                "Fixed Effects (Wald z tests; odds ratios of a higher category)".to_owned()
            }
            (None, false) => "Fixed Effects (Wald z tests)".to_owned(),
        };
        if let Some(d) = &self.design {
            let refs: Vec<String> = d
                .terms()
                .iter()
                .filter_map(|term| match &term.kind {
                    stat_linear::TermKind::Categorical { levels } if d.is_main_effects() => {
                        Some(format!("{} = {}", term.name, levels[0]))
                    }
                    _ => None,
                })
                .collect();
            if !refs.is_empty() {
                title = format!("{title}; reference levels: {}", refs.join(", "));
            }
        }
        ReportSection { kind: SectionKind::ParameterEstimates, title, content: SectionContent::Table(t) }
    }

    fn effect_table(&self) -> ReportSection {
        let normal = self.fit.family == MixedFamily::Normal;
        let heads: Vec<&str> = if normal {
            vec!["Effect", "Num DF", "Den DF", "F", "p-value"]
        } else {
            vec!["Effect", "DF", "χ²", "p-value"]
        };
        let mut t = TextTable::new(heads);
        for (name, test) in &self.effect_tests {
            let mut row = vec![name.clone(), test.num_df.to_string()];
            if normal {
                row.push(test.den_df.map_or("—".to_owned(), |d| format_number(d, 4)));
            }
            row.push(format_number(test.statistic, DIGITS));
            row.push(format_p_value(test.p_value));
            t.push(row);
        }
        let title = match (&self.fit.robust, normal) {
            (Some(_), _) => "Tests by Fixed Effect (robust Hotelling T² approximation, HTZ)".to_owned(),
            (None, true) => {
                format!("Tests by Fixed Effect (F tests, {})", self.df_phrase().replace("degrees", "denominator degrees"))
            }
            (None, false) => "Tests by Fixed Effect (Wald χ²)".to_owned(),
        };
        ReportSection { kind: SectionKind::EffectTests, title, content: SectionContent::Table(t) }
    }

    fn variance_table(&self) -> ReportSection {
        let f = &self.fit;
        let level = format_number(f.confidence_level() * 100.0, 3);
        // Share of the total variance: normal models with intercepts only.
        let simple = f.family == MixedFamily::Normal
            && f.residual_by_group.is_empty()
            && f.modes.iter().all(|m| m.effects == ["Intercept"]);
        let total: f64 = f.variance_components.iter().map(|c| c.variance).sum::<f64>()
            + f.residual.as_ref().map_or(0.0, |r| r.variance);
        let residuals: Vec<&stat_mixed::VarianceComponent> = if f.residual_by_group.is_empty() {
            f.residual.iter().collect()
        } else {
            f.residual_by_group.iter().collect()
        };
        let mut heads: Vec<String> =
            vec!["Group".into(), "Effect".into(), "Variance".into(), "Std Dev".into(), "Std Error".into()];
        heads.push(format!("Lower {level}%"));
        heads.push(format!("Upper {level}%"));
        if simple {
            heads.push("% of total".into());
        }
        let mut t = TextTable::new(heads);
        for c in f.variance_components.iter().chain(residuals) {
            let mut row = vec![
                c.group.clone(),
                c.effect.clone(),
                format_number(c.variance, DIGITS),
                format_number(c.std_dev, DIGITS),
                c.std_error.map_or("—".to_owned(), |v| format_number(v, 4)),
                c.interval.map_or("—".to_owned(), |v| format_number(v.0, DIGITS)),
                c.interval.map_or("—".to_owned(), |v| format_number(v.1, DIGITS)),
            ];
            if simple {
                row.push(format_number(100.0 * c.variance / total, 4));
            }
            t.push(row);
        }
        ReportSection {
            kind: SectionKind::VarianceComponents,
            title: "Random Effects (variance components, Satterthwaite limits)".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    fn correlation_table(&self) -> ReportSection {
        let mut t = TextTable::new(["Group", "Effects", "Correlation"]);
        for c in &self.fit.correlations {
            t.push([c.group.clone(), format!("{} × {}", c.first, c.second), format_number(c.value, 4)]);
        }
        ReportSection {
            kind: SectionKind::RandomCorrelations,
            title: "Random-Effect Correlations".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    fn modes_table(&self) -> ReportSection {
        let mut t = TextTable::new(["Group", "Level", "Effect", "Value", "Conditional SD"]);
        for m in &self.fit.modes {
            for (j, level) in m.levels.iter().enumerate() {
                for (e, effect) in m.effects.iter().enumerate() {
                    t.push([
                        m.group.clone(),
                        level.clone(),
                        effect.clone(),
                        format_number(m.values[j][e], DIGITS),
                        format_number(m.std_errors[j][e], 4),
                    ]);
                }
            }
        }
        let title = if self.fit.family == MixedFamily::Normal {
            "Random Effect Predictions (BLUPs)"
        } else {
            "Random Effect Predictions (conditional modes)"
        };
        ReportSection {
            kind: SectionKind::ConditionalModes,
            title: title.to_owned(),
            content: SectionContent::Table(t),
        }
    }

    fn plot(
        &self,
        x: &[f64],
        y: &[f64],
        (x_label, y_label): (&str, &str),
        line: Option<Vec<[f64; 2]>>,
        line_name: &str,
    ) -> PlotSpec {
        PlotSpec {
            x_time: None,
            y_time: None,
            x_label: x_label.to_owned(),
            y_label: y_label.to_owned(),
            points: self.rows_used.iter().zip(x.iter().zip(y)).map(|(&row, (&x, &y))| RowPoint { row, x, y }).collect(),
            line,
            line_name: Some(line_name.to_owned()),
            band: None,
            groups: None,
            x_ticks: None,
            panes: Vec::new(),
            annotations: None,
            segments: Vec::new(),
            ..PlotSpec::default()
        }
    }

    /// Predicted values on the response's scale and the observed values
    /// (category numbers from 1 for an ordinal response, against expected
    /// category numbers).
    fn fitted_and_observed(&self) -> (Vec<f64>, Vec<f64>, &'static str) {
        if self.is_ordinal() {
            let scores = Self::expected_scores(&self.fit.category_probabilities);
            (scores, self.y.iter().map(|y| y + 1.0).collect(), "Expected category number (with random effects)")
        } else {
            (self.fit.fitted.clone(), self.y.clone(), "Predicted mean (with random effects)")
        }
    }

    fn fitted_plot(&self) -> ReportSection {
        let (fitted, observed, x_label) = self.fitted_and_observed();
        let fitted = &fitted;
        let (lo, hi) =
            fitted.iter().chain(&observed).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
        let y_label =
            if self.is_ordinal() { format!("{} (category number)", self.mean_name()) } else { self.mean_name() };
        let spec = self.plot(
            fitted,
            &observed,
            (x_label, &y_label),
            (lo < hi).then(|| vec![[lo, lo], [hi, hi]]),
            "Reference line y = x",
        );
        ReportSection {
            kind: SectionKind::ObservedVsPredicted,
            title: "Observed vs Predicted".to_owned(),
            content: SectionContent::Plot(Box::new(spec)),
        }
    }

    fn residual_plot(&self) -> ReportSection {
        let res = self.residuals();
        let (fitted, _, x_label) = self.fitted_and_observed();
        let fitted = &fitted;
        let (lo, hi) = fitted.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
        let label = match self.fit.family {
            MixedFamily::Normal => "Residual",
            MixedFamily::Ordinal => "Observed − expected category number",
            _ => "Pearson residual",
        };
        let spec = self.plot(fitted, &res, (x_label, label), (lo < hi).then(|| vec![[lo, 0.0], [hi, 0.0]]), "Zero");
        ReportSection {
            kind: SectionKind::ResidualsVsPredicted,
            title: "Residuals vs Predicted".to_owned(),
            content: SectionContent::Plot(Box::new(spec)),
        }
    }

    /// Fixed-effects design row (with intercept) at profiler settings.
    fn encode(&self, settings: &[TermValue]) -> Result<Vec<f64>> {
        let d = self.design.as_ref().ok_or_else(|| invalid("the model has no fixed-effect predictors"))?;
        let mut row = vec![1.0];
        row.extend(d.encode(settings)?);
        Ok(row)
    }
}

impl crate::surface_profiler::SurfaceModel for MixedAnalysis {
    fn response_name(&self) -> &str {
        &self.profile_name
    }

    fn profile_factors(&self) -> Vec<crate::surface_profiler::ProfileFactor> {
        if self.is_ordinal() {
            return Vec::new();
        }
        self.design.as_ref().map(|d| self.data.profile_factors(d)).unwrap_or_default()
    }

    fn observations(&self) -> Vec<crate::surface_profiler::Observation> {
        let Some(d) = &self.design else { return Vec::new() };
        // Observations on the modelled scale (proportions for a binomial
        // response with trials).
        let mut obs = self.data.observations(d);
        for o in &mut obs {
            if let Some(i) = self.rows_used.iter().position(|&r| r == o.row) {
                o.response = self.y[i];
            }
        }
        obs
    }

    fn default_profile_settings(&self) -> Vec<TermValue> {
        self.design.as_ref().map(|d| self.data.default_settings(d)).unwrap_or_default()
    }

    fn predict_settings(&self, points: &[Vec<TermValue>]) -> Result<Vec<f64>> {
        Ok(self.predict_with_limits(points)?.into_iter().map(|(m, _)| m).collect())
    }

    fn predict_with_limits(&self, points: &[Vec<TermValue>]) -> Result<Vec<crate::surface_profiler::Limited>> {
        let f = &self.fit;
        let beta: Vec<f64> = f.fixed.iter().map(|c| c.estimate).collect();
        let p = beta.len();
        let z = stat_core::Normal::new(0.0, 1.0)?;
        let crit = stat_core::ContinuousDistribution::quantile(&z, 0.5 + f.confidence_level() / 2.0)?;
        points
            .iter()
            .map(|s| {
                let x = self.encode(s)?;
                let eta: f64 = x.iter().zip(&beta).map(|(a, b)| a * b).sum();
                let mut var = 0.0;
                for i in 0..p {
                    for j in 0..p {
                        var += x[i] * f.fixed_covariance[(i, j)] * x[j];
                    }
                }
                let se = var.max(0.0).sqrt();
                let fam = f.family;
                Ok((
                    fam.inverse_link(eta),
                    Some((fam.inverse_link(eta - crit * se), fam.inverse_link(eta + crit * se))),
                ))
            })
            .collect()
    }

    fn confidence_level(&self) -> Option<f64> {
        Some(self.fit.confidence_level())
    }

    fn profile_note(&self) -> Option<String> {
        let offset = if self.request.offset.is_some() { " with the offset at 0" } else { "" };
        Some(format!(
            "Population-level prediction (random effects at zero{offset}), with Wald limits{}.",
            if self.fit.family == MixedFamily::Normal { "" } else { " computed on the link scale and transformed" }
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    /// Dyestuff with a text batch column and an unused covariate.
    fn dyestuff() -> DataTable {
        let y = vec![
            1545., 1440., 1440., 1520., 1580., 1540., 1555., 1490., 1560., 1495., 1595., 1550., 1605., 1510., 1560.,
            1445., 1440., 1595., 1465., 1545., 1595., 1630., 1515., 1635., 1625., 1520., 1455., 1450., 1480., 1445.,
        ];
        let batch: Vec<String> = (0..30).map(|i| ["A", "B", "C", "D", "E", "F"][i / 5].to_owned()).collect();
        let prep: Vec<f64> = (0..30).map(|i| (i % 5) as f64).collect();
        DataTable::new(
            "dye",
            vec![Column::numeric("Yield", y), Column::text("Batch", batch), Column::numeric("Prep", prep)],
        )
        .unwrap()
    }

    fn request(random: Vec<RandomEffectSpec>) -> MixedRequest {
        MixedRequest {
            response: 0,
            predictors: Vec::new(),
            effects: Vec::new(),
            random,
            family: MixedFamily::Normal,
            trials: None,
            offset: None,
            reml: true,
            quadrature_points: 1,
            slices: None,
            slopes: None,
            frequency: None,
            weight: None,
            robust: false,
            residual_groups: None,
            df_method: stat_mixed::DfMethod::Satterthwaite,
            order: Vec::new(),
        }
    }

    #[test]
    fn intercept_only_normal_model_reports_variance_components() {
        let t = dyestuff();
        let a =
            MixedAnalysis::run(&t, &RowStateManager::new(30), &request(vec![RandomEffectSpec::intercept(1)])).unwrap();
        assert_eq!(a.title(), "Linear Mixed Model: Yield ~ 1 + (1 | Batch)");
        let s = a.sections();
        let kinds: Vec<SectionKind> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::ModelSummary,
                SectionKind::ParameterEstimates,
                SectionKind::VarianceComponents,
                SectionKind::ConditionalModes,
                SectionKind::ObservedVsPredicted,
                SectionKind::ResidualsVsPredicted
            ]
        );
        let SectionContent::Table(v) = &s[2].content else { panic!() };
        assert_eq!(v.cell(0, "Group"), Some("Batch"));
        assert_eq!(v.cell(1, "Group"), Some("Residual"));
        // 1764.05 / (1764.05 + 2451.25) = 41.85%.
        assert_eq!(v.cell(0, "% of total"), Some("41.85"));
        let SectionContent::Table(m) = &s[3].content else { panic!() };
        assert_eq!((m.rows.len(), m.cell(4, "Level")), (6, Some("E")));
        let SectionContent::Table(est) = &s[1].content else { panic!() };
        assert_eq!(est.cell(0, "DF"), Some("5"));
        // Excluding a batch's rows refits without them.
        let mut rows = RowStateManager::new(30);
        rows.set_excluded(0..5, true);
        let b = MixedAnalysis::run(&t, &rows, &request(vec![RandomEffectSpec::intercept(1)])).unwrap();
        assert_eq!(b.fit().n_obs, 25);
        assert_eq!(b.fit().n_levels[0].1, 5);
        // Columns per dataset row.
        let col = b.conditional_column();
        assert_eq!(col.len(), 30);
        assert!(col[..5].iter().all(|v| v.is_nan()) && col[5..].iter().all(|v| v.is_finite()));
    }

    #[test]
    fn binomial_with_trials_slopes_nesting_and_profiler() {
        use crate::surface_profiler::SurfaceModel;
        // Herds within regions, a period factor, a dose with a random slope by region.
        let n = 48;
        let region: Vec<String> = (0..n).map(|i| format!("R{}", i % 3)).collect();
        let herd: Vec<String> = (0..n).map(|i| format!("H{}", (i / 3) % 4)).collect();
        let period: Vec<String> = (0..n).map(|i| format!("P{}", i / 12)).collect();
        let dose: Vec<f64> = (0..n).map(|i| (i % 5) as f64).collect();
        let size: Vec<f64> = (0..n).map(|i| 5.0 + (i % 7) as f64).collect();
        let successes: Vec<f64> = (0..n)
            .map(|i| ((size[i] * (0.2 + 0.1 * dose[i] / 4.0 + 0.05 * (i % 3) as f64)).round()).min(size[i]))
            .collect();
        let t = DataTable::new(
            "h",
            vec![
                Column::numeric("Cases", successes),
                Column::numeric("Size", size),
                Column::text("Period", period),
                Column::numeric("Dose", dose),
                Column::text("Region", region),
                Column::text("Herd", herd),
            ],
        )
        .unwrap();
        let req = MixedRequest {
            response: 0,
            predictors: vec![2, 3],
            effects: Vec::new(),
            random: vec![
                RandomEffectSpec { group: 4, within: Vec::new(), intercept: true, slopes: vec![3], correlated: false },
                RandomEffectSpec { group: 5, within: vec![4], intercept: true, slopes: Vec::new(), correlated: true },
            ],
            family: MixedFamily::Binomial,
            trials: Some(1),
            offset: None,
            reml: true,
            quadrature_points: 1,
            slices: None,
            slopes: None,
            frequency: None,
            weight: None,
            robust: false,
            residual_groups: None,
            df_method: stat_mixed::DfMethod::Satterthwaite,
            order: Vec::new(),
        };
        let a = MixedAnalysis::run(&t, &RowStateManager::new(n), &req).unwrap();
        assert!(a.title().starts_with("Generalized Linear Mixed Model: Proportion of Cases ~ Period + Dose"));
        assert!(a.title().contains("(1 + Dose | Region)") && a.title().contains("(1 | Herd[Region])"));
        // Herd within region: 12 combinations.
        assert_eq!(a.fit().n_levels, [("Region".to_owned(), 3), ("Herd[Region]".to_owned(), 12)]);
        assert_eq!(a.fit().modes[1].levels[0], "R0 / H0");
        let s = a.sections();
        let tests = s.iter().find(|x| x.kind == SectionKind::EffectTests).unwrap();
        let SectionContent::Table(tt) = &tests.content else { panic!() };
        assert_eq!((tt.cell(0, "Effect"), tt.cell(0, "DF")), (Some("Period"), Some("3")));
        let SectionContent::Table(est) = &s[2].content else { panic!() };
        assert!(est.columns.contains(&"Odds ratio".to_owned()));
        let SectionContent::EffectPareto(pareto) = &s[1].content else { panic!() };
        assert_eq!(pareto.effects.len(), 2);
        // Profiler: population-level probabilities inside their limits.
        let settings = a.default_profile_settings();
        let p = a.predict_with_limits(&[settings]).unwrap();
        let (m, Some((lo, hi))) = p[0] else { panic!() };
        assert!(lo < m && m < hi && (0.0..1.0).contains(&m));
        assert_eq!(a.observations().len(), n);
        assert!(a.observations().iter().all(|o| (0.0..=1.0).contains(&o.response)));
        // Errors: trials with another family; a categorical slope; no random effects.
        let bad = MixedRequest { family: MixedFamily::Poisson, ..req.clone() };
        assert!(MixedAnalysis::run(&t, &RowStateManager::new(n), &bad).is_err());
        let mut bad = req.clone();
        bad.random[0].slopes = vec![2];
        assert!(MixedAnalysis::run(&t, &RowStateManager::new(n), &bad).is_err());
        let mut bad = req.clone();
        bad.random.clear();
        assert!(MixedAnalysis::run(&t, &RowStateManager::new(n), &bad).is_err());
        let mut bad = req;
        bad.random[0].intercept = false;
        bad.random[0].slopes.clear();
        assert!(MixedAnalysis::run(&t, &RowStateManager::new(n), &bad).is_err());
    }

    #[test]
    fn requests_list_and_map_their_columns() {
        let r = MixedRequest {
            response: 0,
            predictors: vec![1],
            effects: Vec::new(),
            random: vec![RandomEffectSpec {
                group: 2,
                within: vec![3],
                intercept: true,
                slopes: vec![4],
                correlated: true,
            }],
            family: MixedFamily::Poisson,
            trials: None,
            offset: Some(5),
            reml: false,
            quadrature_points: 1,
            slices: None,
            slopes: None,
            frequency: None,
            weight: None,
            robust: false,
            residual_groups: None,
            df_method: stat_mixed::DfMethod::Satterthwaite,
            order: Vec::new(),
        };
        assert_eq!(r.columns(), [0, 1, 2, 3, 4, 5]);
        let m = r.map_columns(|c| c + 10);
        assert_eq!(m.columns(), [10, 11, 12, 13, 14, 15]);
    }

    /// A frequency column gives the same fit as repeating the rows.
    #[test]
    fn frequencies_repeat_rows_and_weights_reach_the_fit() {
        let t = dyestuff();
        let freq: Vec<f64> = (0..30).map(|i| (i % 3) as f64).collect();
        let mut cols: Vec<Column> = (0..3).map(|c| t.column(c).unwrap().clone()).collect();
        cols.push(Column::numeric("Count", freq.clone()));
        let with_freq = DataTable::new("f", cols).unwrap();
        let req = MixedRequest { frequency: Some(3), ..request(vec![RandomEffectSpec::intercept(1)]) };
        let a = MixedAnalysis::run(&with_freq, &RowStateManager::new(30), &req).unwrap();
        // The expanded table.
        let keep: Vec<usize> = (0..30).flat_map(|i| std::iter::repeat_n(i, freq[i] as usize)).collect();
        let expanded = DataTable::new(
            "e",
            (0..3)
                .map(|c| {
                    let col = t.column(c).unwrap();
                    match col.as_numeric() {
                        Some(v) => Column::numeric(&col.name, keep.iter().map(|&i| v[i]).collect()),
                        None => Column::text(&col.name, keep.iter().map(|&i| col.level_text(i)).collect()),
                    }
                })
                .collect(),
        )
        .unwrap();
        let b = MixedAnalysis::run(
            &expanded,
            &RowStateManager::new(keep.len()),
            &request(vec![RandomEffectSpec::intercept(1)]),
        )
        .unwrap();
        assert_eq!(a.fit().n_obs, keep.len());
        assert!((a.fit().minus_two_log_likelihood - b.fit().minus_two_log_likelihood).abs() < 1e-8);
        assert!((a.fit().fixed[0].std_error - b.fit().fixed[0].std_error).abs() < 1e-8);
        let SectionContent::Table(sum) = &a.sections()[0].content else { panic!() };
        assert!(sum.rows.iter().any(|r| r[0] == "Frequencies" && r[1] == "rows repeated by Count"));
        // Weights: with every weight 2, Var(e) = σ²/2 is unchanged, so σ² doubles.
        let mut cols: Vec<Column> = (0..3).map(|c| t.column(c).unwrap().clone()).collect();
        cols.push(Column::numeric("W", vec![2.0; 30]));
        let weighted = DataTable::new("w", cols).unwrap();
        let req = MixedRequest { weight: Some(3), ..request(vec![RandomEffectSpec::intercept(1)]) };
        let w = MixedAnalysis::run(&weighted, &RowStateManager::new(30), &req).unwrap();
        let plain =
            MixedAnalysis::run(&t, &RowStateManager::new(30), &request(vec![RandomEffectSpec::intercept(1)])).unwrap();
        let r = |a: &MixedAnalysis| a.fit().residual.as_ref().unwrap().variance;
        assert!((r(&w) - 2.0 * r(&plain)).abs() < 1e-6 * r(&plain));
        // Bad frequencies and binomial weights are refused.
        let mut cols: Vec<Column> = (0..3).map(|c| t.column(c).unwrap().clone()).collect();
        cols.push(Column::numeric("Count", vec![1.5; 30]));
        let bad = DataTable::new("b", cols).unwrap();
        let req = MixedRequest { frequency: Some(3), ..request(vec![RandomEffectSpec::intercept(1)]) };
        assert!(MixedAnalysis::run(&bad, &RowStateManager::new(30), &req).is_err());
        let req = MixedRequest {
            weight: Some(2),
            family: MixedFamily::Binomial,
            ..request(vec![RandomEffectSpec::intercept(1)])
        };
        assert!(MixedAnalysis::run(&t, &RowStateManager::new(30), &req).is_err());
    }

    /// The ordinal sample: thresholds, odds ratios, category table and
    /// probability columns (values checked against clmm in stat-mixed).
    #[test]
    fn ordinal_report_sections_and_columns() {
        let sample = crate::sample::wine_bitterness();
        let SavedReportCase { request } = wine_request(&sample);
        let a = MixedAnalysis::run(&sample, &RowStateManager::new(72), &request).unwrap();
        assert_eq!(a.title(), "Ordinal Mixed Model: Bitterness ~ Temperature + Skin contact + (1 | Judge)");
        let s = a.sections();
        let kinds: Vec<SectionKind> = s.iter().map(|x| x.kind).collect();
        assert!(kinds.contains(&SectionKind::Thresholds) && kinds.contains(&SectionKind::CategoryProbabilities));
        let th = s.iter().find(|x| x.kind == SectionKind::Thresholds).unwrap();
        let SectionContent::Table(th) = &th.content else { panic!() };
        assert_eq!((th.rows.len(), th.cell(0, "Threshold")), (4, Some("1|2")));
        // clmm: 1|2 = −1.6237.
        assert_eq!(th.cell(0, "Estimate").map(|v| &v[..6]), Some("-1.623"));
        let est = s.iter().find(|x| x.kind == SectionKind::ParameterEstimates).unwrap();
        let SectionContent::Table(est) = &est.content else { panic!() };
        assert_eq!(est.cell(0, "Term"), Some("Temperature: warm"));
        // exp(3.0630) = 21.39.
        assert!(est.cell(0, "Odds ratio").unwrap().starts_with("21.3"));
        let cats = s.iter().find(|x| x.kind == SectionKind::CategoryProbabilities).unwrap();
        let SectionContent::Table(cats) = &cats.content else { panic!() };
        assert_eq!((cats.rows.len(), cats.cell(0, "Count")), (5, Some("5")));
        let cols = a.saveable_columns();
        assert_eq!(cols.len(), 6);
        assert_eq!(cols[0].name, "P(Bitterness = 1)");
        let total: f64 = cols[..5].iter().map(|c| c.values[0]).sum();
        assert!((total - 1.0).abs() < 1e-12);
        assert!(cols[5].text.as_ref().unwrap().iter().all(|c| !c.is_empty()));
        assert!(crate::surface_profiler::SurfaceModel::profile_factors(&a).is_empty());
        // Category order from the request: reversed, the thresholds negate.
        let reversed = MixedRequest { order: ["5", "4", "3", "2", "1"].map(str::to_owned).to_vec(), ..request };
        let b = MixedAnalysis::run(&sample, &RowStateManager::new(72), &reversed).unwrap();
        assert!((b.fit().fixed[0].estimate + a.fit().fixed[0].estimate).abs() < 1e-3);
        assert_eq!(b.categories()[0], "5");
    }

    struct SavedReportCase {
        request: MixedRequest,
    }

    fn wine_request(t: &DataTable) -> SavedReportCase {
        let col = |name: &str| t.find(name).unwrap();
        SavedReportCase {
            request: MixedRequest {
                response: col("Bitterness"),
                predictors: vec![col("Temperature"), col("Skin contact")],
                random: vec![RandomEffectSpec::intercept(col("Judge"))],
                family: MixedFamily::Ordinal,
                ..request(Vec::new())
            },
        }
    }

    /// Residual variances by group, robust errors and between–within df in
    /// the report.
    #[test]
    fn residual_groups_robust_and_between_within_sections() {
        let t = crate::sample::plant_growth();
        let col = |name: &str| t.find(name).unwrap();
        let base = MixedRequest {
            response: col("Height"),
            predictors: vec![col("Fertilizer"), col("Light hours")],
            random: vec![RandomEffectSpec::intercept(col("Greenhouse"))],
            ..request(Vec::new())
        };
        let rows = RowStateManager::new(t.n_rows());
        let req = MixedRequest { residual_groups: Some(col("Fertilizer")), robust: true, ..base.clone() };
        let a = MixedAnalysis::run(&t, &rows, &req).unwrap();
        let s = a.sections();
        let v = s.iter().find(|x| x.kind == SectionKind::VarianceComponents).unwrap();
        let SectionContent::Table(v) = &v.content else { panic!() };
        let residual_rows = v.rows.iter().filter(|r| r[0] == "Residual").count();
        assert_eq!(residual_rows, a.fit().residual_by_group.len());
        assert!(residual_rows >= 2 && !v.columns.contains(&"% of total".to_owned()));
        assert!(s.iter().any(|x| x.kind == SectionKind::ResidualVarianceTest));
        let est = s.iter().find(|x| x.kind == SectionKind::ParameterEstimates).unwrap();
        assert!(est.title.contains("robust CR2") && est.title.contains("Greenhouse"));
        let SectionContent::Table(est) = &est.content else { panic!() };
        assert!(est.columns.contains(&"Model-based Std Error".to_owned()));
        let tests = s.iter().find(|x| x.kind == SectionKind::EffectTests).unwrap();
        assert!(tests.title.contains("HTZ"));
        // Between–within: light hours vary within greenhouses.
        let bw = MixedRequest { df_method: DfMethod::BetweenWithin, ..base.clone() };
        let b = MixedAnalysis::run(&t, &rows, &bw).unwrap();
        assert_eq!(b.fit().df_label, "between–within");
        assert!(b.sections().iter().any(|x| x.title.contains("between–within")));
        // Normal-only options are refused for other families.
        let bad = MixedRequest { family: MixedFamily::Poisson, robust: true, ..base };
        assert!(MixedAnalysis::run(&t, &rows, &bad).is_err());
    }
}
