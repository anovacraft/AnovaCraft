//! Shelf-life estimation from stability data of several batches (ICH Q1E,
//! *Evaluation for Stability Data*, 2003; Chow, *Statistical Design and
//! Analysis of Stability Studies*, 2007, ch. 3).
//!
//! # Batch poolability
//! The response (e.g. assay, % of label claim) of batch `j` at time `t` is
//! modelled by analysis of covariance, `yᵢⱼ = αⱼ + βⱼtᵢⱼ + εᵢⱼ`, fitted by
//! [`fit_linear_model`] with batch indicators and batch-by-time products. Two
//! extra-sum-of-squares F tests decide how far the batches can be pooled,
//! each at the significance level 0.25 that ICH Q1E sets (a high level, so
//! that a real batch difference is not pooled away):
//!
//! 1. equal slopes: `F = [(RSS_c − RSS_s)/(k − 1)] / [RSS_s/(n − 2k)]`, the
//!    separate-lines model `s` against the common-slope model `c`;
//! 2. if slopes are not rejected, equal intercepts:
//!    `F = [(RSS_p − RSS_c)/(k − 1)] / [RSS_c/(n − k − 1)]` against the single
//!    pooled line `p`.
//!
//! Rejecting equal slopes gives separate lines (the error variance still
//! pooled from the full model); rejecting only equal intercepts gives
//! parallel lines; otherwise one line for all batches.
//!
//! When the separate-lines model leaves no residual degrees of freedom
//! (`n = 2k`, e.g. every batch measured at only two times) the slope test is
//! impossible: the slopes are assumed common, with a note, and the intercept
//! test decides between parallel lines and one line. An F test whose extra
//! sum of squares is negligible (below 10⁻¹² of the total) gives F = 0 and
//! p = 1, even when the larger model fits exactly; a real extra sum of squares
//! over an exact fit gives F = ∞ and p = 0.
//!
//! # Shelf life
//! For an attribute with a lower acceptance limit the shelf life is the
//! earliest time at which the one-sided 95% lower confidence bound for the
//! mean, `ŷ(t) − t_{0.95, df} · se(ŷ(t))`, falls to the limit (an upper limit
//! uses the upper bound); with both limits the two-sided 95% bounds are
//! compared with both (ICH Q1E §2.4). With separate or parallel lines the
//! shortest batch shelf life applies. The search runs from time 0 to a
//! horizon (default twice the longest time observed).
//!
//! # Extrapolation limit
//! ICH Q1E (Appendix A, for long-term data analysed statistically with no
//! significant change at accelerated conditions) allows a proposed shelf life
//! of at most twice the period covered by long-term data, `X`, and no more
//! than 12 months beyond it: `min(2X, X + 12 months)`. The 12-month part needs
//! the unit of the time column, [`ShelfLifeOptions::time_unit`]; without one
//! only the `2X` part is applied. [`ShelfLifeResult::supported_shelf_life`]
//! is the estimated shelf life cut back to that limit.

use crate::ols::{LinearModelFit, OlsOptions, fit_linear_model};
use stat_core::distributions::{ContinuousDistribution, FisherSnedecor};
use stat_core::{Matrix, Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Options for [`shelf_life`].
#[derive(Debug, Clone, PartialEq)]
pub struct ShelfLifeOptions {
    /// Lower acceptance limit (for an attribute that decreases).
    pub lower_limit: Option<f64>,
    /// Upper acceptance limit (for an attribute that increases).
    pub upper_limit: Option<f64>,
    /// Significance level of the poolability tests. Default 0.25.
    pub pooling_alpha: f64,
    /// Confidence level of the bound(s). Default 0.95 (one-sided with one
    /// limit, two-sided with both).
    pub confidence_level: f64,
    /// Longest time searched; `None` for twice the longest time observed.
    pub horizon: Option<f64>,
    /// Unit of the time values, for ICH Q1E's limit of 12 months of
    /// extrapolation; `None` when unknown (only the twice-the-data limit is
    /// applied).
    pub time_unit: Option<StabilityTimeUnit>,
}

impl ShelfLifeOptions {
    /// ICH Q1E defaults with a lower acceptance limit, times in months.
    #[must_use]
    pub fn lower(limit: f64) -> Self {
        Self {
            lower_limit: Some(limit),
            upper_limit: None,
            pooling_alpha: 0.25,
            confidence_level: 0.95,
            horizon: None,
            time_unit: Some(StabilityTimeUnit::Months),
        }
    }
}

/// Unit of the time values in a stability study.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StabilityTimeUnit {
    /// Days.
    Days,
    /// Weeks.
    Weeks,
    /// Months (the usual unit of ICH stability schedules).
    Months,
    /// Years.
    Years,
}

impl StabilityTimeUnit {
    /// Every unit, in display order.
    pub const ALL: [Self; 4] = [Self::Months, Self::Weeks, Self::Days, Self::Years];

    /// Twelve months in this unit (a year of 365.25 days).
    #[must_use]
    pub fn twelve_months(self) -> f64 {
        match self {
            Self::Days => 365.25,
            Self::Weeks => 365.25 / 7.0,
            Self::Months => 12.0,
            Self::Years => 1.0,
        }
    }

    /// Lower-case plural name ("months"), also the key in saved files.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Days => "days",
            Self::Weeks => "weeks",
            Self::Months => "months",
            Self::Years => "years",
        }
    }

    /// The unit named by [`label`](Self::label).
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|u| u.label() == label)
    }
}

/// Which rule sets [`ShelfLifeResult::extrapolation_limit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExtrapolationRule {
    /// Twice the longest time observed.
    TwiceObserved,
    /// Twelve months beyond the longest time observed.
    TwelveMonthsBeyond,
}

/// The model chosen by the poolability tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PoolingModel {
    /// A separate intercept and slope per batch.
    Separate,
    /// Separate intercepts, a common slope.
    CommonSlope,
    /// One line for all batches.
    Pooled,
}

impl PoolingModel {
    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Separate => "separate intercepts and slopes",
            Self::CommonSlope => "separate intercepts, common slope",
            Self::Pooled => "all batches pooled",
        }
    }
}

/// One extra-sum-of-squares F test.
#[derive(Debug, Clone, PartialEq)]
pub struct PoolingTest {
    /// What is tested, e.g. `"Equal slopes"`.
    pub name: String,
    /// Numerator degrees of freedom.
    pub df_numerator: usize,
    /// Denominator degrees of freedom.
    pub df_denominator: usize,
    /// F statistic.
    pub f: f64,
    /// Upper-tail p-value.
    pub p_value: f64,
}

/// A batch's fitted line and shelf life under the chosen model.
#[derive(Debug, Clone, PartialEq)]
pub struct BatchLine {
    /// Batch label (`All batches` under the pooled model).
    pub batch: String,
    /// Intercept (response at time 0).
    pub intercept: f64,
    /// Slope per unit time.
    pub slope: f64,
    /// Observations of this batch (all observations when pooled).
    pub n: usize,
    /// Earliest time the bound meets a limit; `None` beyond the horizon.
    pub shelf_life: Option<f64>,
}

/// Result of [`shelf_life`].
#[derive(Debug, Clone)]
pub struct ShelfLifeResult {
    /// Batch labels, in natural order.
    pub batches: Vec<String>,
    /// Test of equal slopes (`None` for a single batch, or when the
    /// separate-lines model has no residual degrees of freedom).
    pub slope_test: Option<PoolingTest>,
    /// Test of equal intercepts (`None` unless slopes were pooled).
    pub intercept_test: Option<PoolingTest>,
    /// Chosen model.
    pub model: PoolingModel,
    /// One line per batch, or one pooled line.
    pub lines: Vec<BatchLine>,
    /// Shelf life: the shortest over the lines (`None` when every line
    /// stays within the limits up to the horizon).
    pub shelf_life: Option<f64>,
    /// Index into `lines` of the line that sets the shelf life.
    pub limiting_line: Option<usize>,
    /// Residual degrees of freedom of the chosen model.
    pub df_error: usize,
    /// Root mean square error of the chosen model.
    pub rmse: f64,
    /// Longest time observed.
    pub max_time: f64,
    /// Horizon searched.
    pub horizon: f64,
    /// Longest shelf life ICH Q1E allows from these data:
    /// `min(2X, X + 12 months)` with `X` the longest time observed, or `2X`
    /// when the time unit is unknown.
    pub extrapolation_limit: f64,
    /// The part of the limit that binds.
    pub extrapolation_rule: ExtrapolationRule,
    /// Options used.
    pub options: ShelfLifeOptions,
    /// Notes on the analysis (e.g. a poolability test that could not be run).
    pub notes: Vec<String>,
    fit: LinearModelFit,
}

impl ShelfLifeResult {
    /// The shelf life cut back to [`extrapolation_limit`](Self::extrapolation_limit):
    /// `min(shelf life, limit)`, or the limit when the bound stays within the
    /// acceptance limits up to a horizon at or beyond it. `None` when the
    /// bound stays within them up to a horizon shorter than the limit.
    #[must_use]
    pub fn supported_shelf_life(&self) -> Option<f64> {
        match self.shelf_life {
            Some(s) => Some(s.min(self.extrapolation_limit)),
            None => (self.horizon >= self.extrapolation_limit).then_some(self.extrapolation_limit),
        }
    }

    /// Mean response and its confidence bounds `(mean, lower, upper)` for
    /// line `line` at time `t`, at the bound's level (the lower bound is the
    /// one-sided bound when only a lower limit is set, and similarly upper).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for an unknown line.
    pub fn bounds(&self, line: usize, t: f64) -> Result<(f64, f64, f64)> {
        let row = self.design_row(line, t)?;
        let p = self.fit.predict(&Matrix::from_rows(&[row])?)?;
        let pr = p[0];
        Ok((pr.fitted, pr.confidence_interval.0, pr.confidence_interval.1))
    }

    fn design_row(&self, line: usize, t: f64) -> Result<Vec<f64>> {
        if line >= self.lines.len() {
            return Err(invalid(format!("no line {line}")));
        }
        let k = self.batches.len();
        Ok(match self.model {
            PoolingModel::Pooled => vec![t],
            PoolingModel::CommonSlope => {
                let mut r = vec![t];
                r.extend((1..k).map(|j| if j == line { 1.0 } else { 0.0 }));
                r
            }
            PoolingModel::Separate => {
                let mut r = vec![t];
                r.extend((1..k).map(|j| if j == line { 1.0 } else { 0.0 }));
                r.extend((1..k).map(|j| if j == line { t } else { 0.0 }));
                r
            }
        })
    }

    /// `true` when the bound at `t` lies outside an acceptance limit.
    fn fails(&self, line: usize, t: f64) -> Result<bool> {
        let (_, lo, hi) = self.bounds(line, t)?;
        Ok(self.options.lower_limit.is_some_and(|l| lo < l) || self.options.upper_limit.is_some_and(|u| hi > u))
    }

    fn crossing(&self, line: usize) -> Result<Option<f64>> {
        const STEPS: usize = 2000;
        if self.fails(line, 0.0)? {
            return Ok(Some(0.0));
        }
        let mut prev = 0.0;
        for i in 1..=STEPS {
            let t = self.horizon * i as f64 / STEPS as f64;
            if self.fails(line, t)? {
                let (mut a, mut b) = (prev, t);
                for _ in 0..60 {
                    let m = 0.5 * (a + b);
                    if self.fails(line, m)? {
                        b = m;
                    } else {
                        a = m;
                    }
                }
                return Ok(Some(0.5 * (a + b)));
            }
            prev = t;
        }
        Ok(None)
    }
}

/// Extra-sum-of-squares F test; `total_ss` (the response's corrected total)
/// sets the scale below which the extra sum of squares is rounding noise.
fn f_test(
    name: &str,
    rss_reduced: f64,
    rss_full: f64,
    df_num: usize,
    df_den: usize,
    total_ss: f64,
) -> Result<PoolingTest> {
    let extra = rss_reduced - rss_full;
    let ms_err = rss_full / df_den as f64;
    let (f, p_value) = if extra <= 1e-12 * total_ss {
        (0.0, 1.0)
    } else if ms_err > 0.0 {
        let f = extra / df_num as f64 / ms_err;
        (f, FisherSnedecor::new(df_num as f64, df_den as f64)?.sf(f)?)
    } else {
        (f64::INFINITY, 0.0)
    };
    Ok(PoolingTest { name: name.to_owned(), df_numerator: df_num, df_denominator: df_den, f, p_value })
}

fn rss(fit: &LinearModelFit) -> f64 {
    fit.anova().error.sum_of_squares
}

/// Estimate the shelf life from stability data.
///
/// `time`, `response` and `batch` are parallel, one row per measurement.
///
/// # Errors
/// * [`StatError::DimensionMismatch`] for slices of different lengths.
/// * [`StatError::NonFiniteInput`] for NaN or infinite values.
/// * [`StatError::InvalidArgument`] without an acceptance limit, with a lower
///   limit above the upper, a bad level, a time below zero, a non-positive
///   horizon, or a batch with fewer than two distinct times.
/// * Errors from [`fit_linear_model`] (e.g. a rank-deficient design). With
///   only two times per batch the separate-lines model is skipped (see the
///   module notes) rather than failing.
pub fn shelf_life(
    time: &[f64],
    response: &[f64],
    batch: &[String],
    options: &ShelfLifeOptions,
) -> Result<ShelfLifeResult> {
    let n = time.len();
    if response.len() != n || batch.len() != n {
        return Err(StatError::DimensionMismatch {
            context: "time, response and batch",
            expected: n,
            found: if response.len() == n { batch.len() } else { response.len() },
        });
    }
    if time.iter().chain(response).any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "stability data" });
    }
    if options.lower_limit.is_none() && options.upper_limit.is_none() {
        return Err(invalid("give a lower or an upper acceptance limit (or both)"));
    }
    if let (Some(l), Some(u)) = (options.lower_limit, options.upper_limit) {
        if l >= u {
            return Err(invalid("the lower acceptance limit must be below the upper"));
        }
    }
    for level in [options.pooling_alpha, options.confidence_level] {
        if !(level > 0.0 && level < 1.0) {
            return Err(invalid("significance and confidence levels must lie in (0, 1)"));
        }
    }
    if options.confidence_level <= 0.5 {
        return Err(invalid("the confidence level must be above 50% for a one-sided bound"));
    }
    if time.iter().any(|&t| t < 0.0) {
        return Err(invalid("times must not be negative"));
    }
    let mut batches: Vec<String> = Vec::new();
    for b in batch {
        if !batches.contains(b) {
            batches.push(b.clone());
        }
    }
    batches.sort_by(|a, b| stat_data::natural_order(a, b));
    let k = batches.len();
    let index: Vec<usize> = batch.iter().map(|b| batches.iter().position(|x| x == b).unwrap_or(0)).collect();
    for (j, name) in batches.iter().enumerate() {
        let mut ts: Vec<f64> = (0..n).filter(|&i| index[i] == j).map(|i| time[i]).collect();
        ts.sort_by(f64::total_cmp);
        ts.dedup();
        if ts.len() < 2 {
            return Err(invalid(format!("batch {name} needs measurements at two or more times")));
        }
    }
    let max_time = time.iter().copied().fold(0.0, f64::max);
    let horizon = options.horizon.unwrap_or(2.0 * max_time);
    if !(horizon > 0.0 && horizon.is_finite()) {
        return Err(invalid("the horizon must be positive"));
    }
    let (extrapolation_limit, extrapolation_rule) = match options.time_unit {
        Some(unit) if max_time + unit.twelve_months() < 2.0 * max_time => {
            (max_time + unit.twelve_months(), ExtrapolationRule::TwelveMonthsBeyond)
        }
        _ => (2.0 * max_time, ExtrapolationRule::TwiceObserved),
    };
    let both = options.lower_limit.is_some() && options.upper_limit.is_some();
    // A one-sided 95% bound is the end of a two-sided 90% interval.
    let fit_level = if both { options.confidence_level } else { 1.0 - 2.0 * (1.0 - options.confidence_level) };
    let ols = OlsOptions { confidence_level: fit_level, ..OlsOptions::default() };
    let indicator = |j: usize| -> Vec<f64> { index.iter().map(|&b| if b == j { 1.0 } else { 0.0 }).collect() };
    let dummies: Vec<Vec<f64>> = (1..k).map(indicator).collect();
    let products: Vec<Vec<f64>> = dummies.iter().map(|d| d.iter().zip(time).map(|(a, t)| a * t).collect()).collect();
    let columns = |with_dummies: bool, with_products: bool| -> Result<Matrix> {
        let mut cols: Vec<&[f64]> = vec![time];
        if with_dummies {
            cols.extend(dummies.iter().map(Vec::as_slice));
        }
        if with_products {
            cols.extend(products.iter().map(Vec::as_slice));
        }
        Matrix::from_columns(&cols)
    };
    let pooled = fit_linear_model(&columns(false, false)?, response, &ols)?;
    let total_ss = pooled.anova().total.sum_of_squares;
    let mut notes = Vec::new();
    let (model, fit, slope_test, intercept_test) = if k == 1 {
        (PoolingModel::Pooled, pooled, None, None)
    } else {
        let common = fit_linear_model(&columns(true, false)?, response, &ols)?;
        // The separate-lines model has 2k parameters; with n = 2k it fits
        // exactly and leaves nothing to test the slopes against.
        let slopes = if n > 2 * k {
            let separate = fit_linear_model(&columns(true, true)?, response, &ols)?;
            let test =
                f_test("Equal slopes", rss(&common), rss(&separate), k - 1, separate.anova().error.df, total_ss)?;
            Some((test, separate))
        } else {
            notes.push(
                "The separate-lines model has no residual degrees of freedom (each batch has only two \
                 measurements), so equal slopes could not be tested: the slopes are assumed common."
                    .to_owned(),
            );
            None
        };
        match slopes {
            Some((test, separate)) if test.p_value < options.pooling_alpha => {
                (PoolingModel::Separate, separate, Some(test), None)
            }
            slopes => {
                let slopes = slopes.map(|(t, _)| t);
                let intercepts =
                    f_test("Equal intercepts", rss(&pooled), rss(&common), k - 1, common.anova().error.df, total_ss)?;
                if intercepts.p_value < options.pooling_alpha {
                    (PoolingModel::CommonSlope, common, slopes, Some(intercepts))
                } else {
                    (PoolingModel::Pooled, pooled, slopes, Some(intercepts))
                }
            }
        }
    };
    let b = fit.coefficients();
    let lines: Vec<BatchLine> = match model {
        PoolingModel::Pooled => {
            vec![BatchLine { batch: "All batches".to_owned(), intercept: b[0], slope: b[1], n, shelf_life: None }]
        }
        _ => (0..k)
            .map(|j| {
                let shift = if j == 0 { 0.0 } else { b[1 + j] };
                let slope_shift = if model == PoolingModel::Separate && j > 0 { b[k + j] } else { 0.0 };
                BatchLine {
                    batch: batches[j].clone(),
                    intercept: b[0] + shift,
                    slope: b[1] + slope_shift,
                    n: index.iter().filter(|&&i| i == j).count(),
                    shelf_life: None,
                }
            })
            .collect(),
    };
    let summary = *fit.summary();
    let mut result = ShelfLifeResult {
        batches,
        slope_test,
        intercept_test,
        model,
        lines,
        shelf_life: None,
        limiting_line: None,
        df_error: fit.anova().error.df,
        rmse: summary.root_mean_square_error,
        max_time,
        horizon,
        extrapolation_limit,
        extrapolation_rule,
        options: options.clone(),
        notes,
        fit,
    };
    for j in 0..result.lines.len() {
        result.lines[j].shelf_life = result.crossing(j)?;
    }
    let limiting = result
        .lines
        .iter()
        .enumerate()
        .filter_map(|(j, l)| l.shelf_life.map(|s| (j, s)))
        .min_by(|a, b| a.1.total_cmp(&b.1));
    result.shelf_life = limiting.map(|(_, s)| s);
    result.limiting_line = limiting.map(|(j, _)| j);
    Ok(result)
}

#[cfg(test)]
mod tests;
