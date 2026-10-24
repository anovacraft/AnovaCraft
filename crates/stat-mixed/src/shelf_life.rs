//! Shelf life with batch as a random effect (Chow & Shao, "Estimating drug
//! shelf-life with random batches", *Biometrics* 47, 1991; ICH Q1E, 2003).
//!
//! Batches in a stability study are a sample of the batches that will be
//! made. Treating them as random, the response of batch `j` at time `t` is
//!
//! ```text
//! yᵢⱼ = (β₀ + b₀ⱼ) + (β₁ + b₁ⱼ) tᵢⱼ + εᵢⱼ,   (b₀ⱼ, b₁ⱼ) ~ N(0, G),   ε ~ N(0, σ²),
//! ```
//!
//! fitted by REML with a correlated random intercept and slope. Two bounds
//! can set the shelf life, each the earliest time at which it meets an
//! acceptance limit (one-sided at the confidence level with one limit,
//! two-sided with both):
//!
//! * the **population mean** `β₀ + β₁t`: `x̂ ∓ t_ν · √(xᵀ Cov(β̂) x)`;
//! * a **future batch**'s mean `β₀ + b₀ + (β₁ + b₁)t`, whose prediction
//!   variance adds the batch-to-batch variation:
//!   `x̂ ∓ t_ν · √(xᵀ Cov(β̂) x + xᵀ Ĝ x)`, with `x = (1, t)`.
//!
//! `ν` is Satterthwaite's degrees of freedom of the variance in the bound
//! (see [`MixedFit::prediction_variance`]). The future-batch bound is the
//! conservative one ICH Q1E's batch-to-batch concern points to; the
//! population-mean bound describes the average batch.

use stat_core::distributions::{ContinuousDistribution, Normal, StudentsT};
use stat_core::{Matrix, Result, StatError};

use crate::family::MixedFamily;
use crate::fit::{MixedFit, MixedModel, MixedOptions, RandomTerm, fit_mixed};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Options for [`random_batch_shelf_life`].
#[derive(Debug, Clone, PartialEq)]
pub struct RandomBatchOptions {
    /// Lower acceptance limit.
    pub lower_limit: Option<f64>,
    /// Upper acceptance limit.
    pub upper_limit: Option<f64>,
    /// Confidence level of the bounds (one-sided with one limit).
    pub confidence_level: f64,
    /// Longest time searched; `None` for twice the longest time observed.
    pub horizon: Option<f64>,
}

/// Result of [`random_batch_shelf_life`].
#[derive(Debug, Clone)]
pub struct RandomBatchShelfLife {
    /// The fitted mixed model (fixed effects `Intercept`, time; one random
    /// term by batch with an intercept and a slope).
    pub fit: MixedFit,
    /// Batch labels, in order of first appearance.
    pub batches: Vec<String>,
    /// Each batch's predicted line (BLUP intercept, BLUP slope).
    pub batch_lines: Vec<(f64, f64)>,
    /// Earliest time the bound for the population mean meets a limit.
    pub mean_shelf_life: Option<f64>,
    /// Earliest time the prediction bound for a future batch meets a limit.
    pub batch_shelf_life: Option<f64>,
    /// Longest time observed.
    pub max_time: f64,
    /// Horizon searched.
    pub horizon: f64,
    /// Options used.
    pub options: RandomBatchOptions,
}

impl RandomBatchShelfLife {
    /// `(estimate, lower, upper)` at time `t` for the population mean, or
    /// (with `future_batch`) for the mean of a future batch.
    ///
    /// # Errors
    /// Errors from the t quantile.
    pub fn bounds(&self, t: f64, future_batch: bool) -> Result<(f64, f64, f64)> {
        let x = [1.0, t];
        let est = self.fit.fixed[0].estimate + self.fit.fixed[1].estimate * t;
        let random = future_batch.then_some((0, &x[..]));
        let (var, df) = self.fit.prediction_variance(&x, random)?;
        let both = self.options.lower_limit.is_some() && self.options.upper_limit.is_some();
        let tail = 1.0 - self.options.confidence_level;
        let prob = if both { 1.0 - tail / 2.0 } else { 1.0 - tail };
        let q = match df {
            Some(d) => StudentsT::new(d)?.quantile(prob)?,
            None => Normal::new(0.0, 1.0)?.quantile(prob)?,
        };
        let half = q * var.max(0.0).sqrt();
        Ok((est, est - half, est + half))
    }

    fn fails(&self, t: f64, future: bool) -> Result<bool> {
        let (_, lo, hi) = self.bounds(t, future)?;
        Ok(self.options.lower_limit.is_some_and(|l| lo < l) || self.options.upper_limit.is_some_and(|u| hi > u))
    }

    fn crossing(&self, future: bool) -> Result<Option<f64>> {
        const STEPS: usize = 2000;
        if self.fails(0.0, future)? {
            return Ok(Some(0.0));
        }
        let mut prev = 0.0;
        for i in 1..=STEPS {
            let t = self.horizon * i as f64 / STEPS as f64;
            if self.fails(t, future)? {
                let (mut a, mut b) = (prev, t);
                for _ in 0..60 {
                    let m = 0.5 * (a + b);
                    if self.fails(m, future)? {
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

/// Estimate the shelf life with batches as random (see the module notes).
///
/// `time`, `response` and `batch` are parallel, one row per measurement.
///
/// # Errors
/// * [`StatError::DimensionMismatch`] for slices of different lengths.
/// * [`StatError::NonFiniteInput`] for NaN or infinite values.
/// * [`StatError::InvalidArgument`] without an acceptance limit, with a lower
///   limit above the upper, a bad level, negative times, a non-positive
///   horizon, fewer than three batches, or a batch with fewer than two
///   distinct times.
/// * Errors from [`fit_mixed`].
pub fn random_batch_shelf_life(
    time: &[f64],
    response: &[f64],
    batch: &[String],
    options: &RandomBatchOptions,
) -> Result<RandomBatchShelfLife> {
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
    if !(options.confidence_level > 0.5 && options.confidence_level < 1.0) {
        return Err(invalid("the confidence level must lie between 50% and 100%"));
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
    if batches.len() < 3 {
        return Err(invalid("batches as a random effect need three or more batches"));
    }
    let codes: Vec<usize> = batch.iter().map(|b| batches.iter().position(|x| x == b).unwrap_or(0)).collect();
    for (j, name) in batches.iter().enumerate() {
        let mut ts: Vec<f64> = (0..n).filter(|&i| codes[i] == j).map(|i| time[i]).collect();
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
    let ones = vec![1.0; n];
    let model = MixedModel {
        y: response.to_vec(),
        x: Matrix::from_columns(&[ones.clone(), time.to_vec()])?,
        x_names: vec!["Intercept".to_owned(), "Time".to_owned()],
        weights: Vec::new(),
        offset: Vec::new(),
        random: vec![RandomTerm {
            group: "Batch".to_owned(),
            levels: batches.clone(),
            codes,
            effects: vec!["Intercept".to_owned(), "Time".to_owned()],
            z: Matrix::from_columns(&[ones, time.to_vec()])?,
            correlated: true,
        }],
        family: MixedFamily::Normal,
        residual_groups: None,
        categories: Vec::new(),
    };
    let fit =
        fit_mixed(&model, &MixedOptions { confidence_level: options.confidence_level, ..MixedOptions::default() })?;
    let (b0, b1) = (fit.fixed[0].estimate, fit.fixed[1].estimate);
    let batch_lines = fit.modes[0].values.iter().map(|v| (b0 + v[0], b1 + v[1])).collect();
    let mut out = RandomBatchShelfLife {
        fit,
        batches,
        batch_lines,
        mean_shelf_life: None,
        batch_shelf_life: None,
        max_time,
        horizon,
        options: options.clone(),
    };
    out.mean_shelf_life = out.crossing(false)?;
    out.batch_shelf_life = out.crossing(true)?;
    Ok(out)
}
