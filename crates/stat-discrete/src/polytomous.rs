//! Logistic regression for a response with more than two categories:
//! the proportional-odds (cumulative logit) model for ordered categories and
//! the baseline-category (multinomial) logit model for unordered ones.
//!
//! # Proportional odds (ordered response)
//! With categories `1 < 2 < … < J` and predictors `x` (no intercept column),
//!
//! `P(Y ≤ j | x) = F(αⱼ − xᵀβ)`,  `j = 1, …, J − 1`,  `F(t) = 1/(1 + e^(−t))`,
//!
//! with increasing thresholds (cut-points) `α₁ < … < α_{J−1}` and one slope
//! vector `β` shared by every cumulative split (McCullagh 1980, "Regression
//! models for ordinal data", *JRSS B* 42; Agresti, *Categorical Data
//! Analysis*, 3rd ed., 2013, §8.2). `exp(βₖ)` is the factor by which a unit
//! increase in `xₖ` multiplies the odds of a *higher* category,
//! `P(Y > j)/P(Y ≤ j)`, the same for every `j`. Category probabilities are
//! `P(Y = j) = F(αⱼ − η) − F(α_{j−1} − η)` (`α₀ = −∞`, `α_J = +∞`), computed
//! as `σ(u) σ(−l) (1 − e^(l−u))` to avoid cancellation.
//!
//! **Parallel-lines test.** The score test of the proportional-odds
//! assumption (Peterson & Harrell 1990, "Partial proportional odds models for
//! ordinal response variables", *Applied Statistics* 39) compares the model
//! with the general cumulative logit model `P(Y ≤ j) = F(αⱼ − xᵀβⱼ)`, a
//! separate slope vector per split. At the proportional-odds estimates
//! (`βⱼ = β̂` for all `j`) the general model's score `U` and expected (Fisher)
//! information `I = Σᵢ wᵢ Σⱼ (∂πᵢⱼ/∂θ)(∂πᵢⱼ/∂θ)ᵀ / πᵢⱼ` give
//! `S = Uᵀ I⁻¹ U`, referred to χ² with `p (J − 2)` degrees of freedom. Only
//! the restricted fit is needed, so the test exists even when the general
//! model would give crossing cumulative curves.
//!
//! # Baseline-category logits (unordered response)
//! With a reference category `r`,
//!
//! `ln(P(Y = j)/P(Y = r)) = β₀ⱼ + xᵀβⱼ`,  `j ≠ r`,
//!
//! so `P(Y = j) = e^(ηⱼ) / (1 + Σ_{k≠r} e^(ηₖ))` (Agresti 2013, §8.1).
//! `exp(βⱼₖ)` multiplies the odds of category `j` against the reference per
//! unit of `xₖ`.
//!
//! # Fitting
//! Newton–Raphson on the log-likelihood `ℓ = Σᵢ wᵢ ln πᵢ,yᵢ` with analytic
//! gradient and Hessian, halving a step that does not increase `ℓ` (or, for
//! the ordinal model, that would put the thresholds out of order). Both
//! log-likelihoods are concave (Pratt 1981, *JASA* 76, for the cumulative
//! logit), so the maximum is unique when it exists. For the multinomial
//! model the observed and expected information coincide (canonical link), so
//! this is also Fisher scoring. Start: thresholds at the logits of the
//! cumulative sample proportions (ordinal) or intercepts at the log ratios of
//! the category totals (nominal), slopes 0. Iteration stops when `ℓ` changes
//! by less than `10⁻¹²(|ℓ| + 1)` and every parameter by less than
//! `10⁻⁹(|θ| + 1)`; if the classes are separated by the predictors the
//! estimates drift without converging, and the fit says so.
//!
//! # Inference
//! `Cov(θ̂)` is the inverse observed information at the estimates. Wald
//! statistics `z = θ̂/se` against the standard normal, intervals
//! `θ̂ ± z_{1−α/2} se`. The null model has the thresholds (intercepts) only;
//! its log-likelihood is `Σⱼ Wⱼ ln(Wⱼ/W)` (`Wⱼ` the weight of category `j`).
//! The whole-model likelihood-ratio test is `2(ℓ − ℓ₀)` on the number of
//! slope parameters. `AIC = −2ℓ + 2k`, `BIC = −2ℓ + k ln W` (`k` all
//! parameters, thresholds or intercepts included); McFadden's pseudo-R²
//! `1 − ℓ/ℓ₀` and Nagelkerke's `(1 − e^{2(ℓ₀−ℓ)/W}) / (1 − e^{2ℓ₀/W})`.
//!
//! Weights are frequency weights: a row of weight 3 counts as three
//! identical observations (so `W`, not the number of rows, enters the BIC).

use crate::glm::{GlmCoefficient, LikelihoodRatioTest};
use stat_core::distributions::{ChiSquared, ContinuousDistribution, Normal};
use stat_core::{Matrix, Result, StatError};

/// Options shared by [`fit_ordinal_logistic`] and [`fit_nominal_logistic`].
#[derive(Debug, Clone, PartialEq)]
pub struct PolytomousOptions {
    /// Names of the columns of `X` (default `x1, x2, …`).
    pub names: Vec<String>,
    /// Frequency weights, one per row (empty for all 1).
    pub weights: Vec<f64>,
    /// Confidence level of the Wald intervals. Default 0.95.
    pub confidence_level: f64,
    /// Iteration limit. Default 100.
    pub max_iterations: usize,
}

impl Default for PolytomousOptions {
    fn default() -> Self {
        Self { names: Vec::new(), weights: Vec::new(), confidence_level: 0.95, max_iterations: 100 }
    }
}

/// Fit statistics common to both models.
#[derive(Debug, Clone, PartialEq)]
pub struct PolytomousStatistics {
    /// Maximized log-likelihood `ℓ`.
    pub log_likelihood: f64,
    /// Log-likelihood `ℓ₀` of the thresholds- (intercepts-) only model.
    pub null_log_likelihood: f64,
    /// Number of parameters `k`, thresholds or intercepts included.
    pub n_parameters: usize,
    /// Rows with positive weight.
    pub n_rows: usize,
    /// Total weight `W` (the number of observations).
    pub total_weight: f64,
    /// `−2ℓ + 2k`.
    pub aic: f64,
    /// `−2ℓ + k ln W`.
    pub bic: f64,
    /// `2(ℓ − ℓ₀)` against χ² on the number of slope parameters.
    pub whole_model: LikelihoodRatioTest,
    /// McFadden's `1 − ℓ/ℓ₀`.
    pub mcfadden_r2: f64,
    /// Nagelkerke's rescaled Cox–Snell R².
    pub nagelkerke_r2: f64,
    /// Newton iterations used.
    pub iterations: usize,
    /// `false` when the iteration limit was reached (e.g. separation).
    pub converged: bool,
    /// Warnings about the fit.
    pub warnings: Vec<String>,
    /// Confidence level of the Wald intervals.
    pub confidence_level: f64,
}

/// A score (Lagrange multiplier) test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScoreTest {
    /// `Uᵀ I⁻¹ U`.
    pub chi_square: f64,
    /// Degrees of freedom.
    pub df: usize,
    /// Upper-tail χ² p-value.
    pub p_value: f64,
}

/// A fitted proportional-odds (cumulative logit) model.
#[derive(Debug, Clone, PartialEq)]
pub struct OrdinalLogitFit {
    /// Response categories, lowest first.
    pub levels: Vec<String>,
    /// Thresholds `α₁ … α_{J−1}`, named `"low | high"` after the categories
    /// they separate.
    pub thresholds: Vec<GlmCoefficient>,
    /// Slopes `β`, one per column of `X`.
    pub slopes: Vec<GlmCoefficient>,
    /// Covariance of `(α, β)`, thresholds first.
    pub covariance: Matrix,
    /// Fit statistics.
    pub statistics: PolytomousStatistics,
    /// Score test of parallel lines (`None` with two categories or no
    /// predictors, or when the general model's information is singular).
    pub parallel_lines: Option<ScoreTest>,
    /// Fitted category probabilities, one row per input row.
    pub probabilities: Vec<Vec<f64>>,
}

impl OrdinalLogitFit {
    /// Category probabilities for new rows (same columns as the fit).
    ///
    /// # Errors
    /// [`StatError::DimensionMismatch`] for the wrong number of columns.
    pub fn predict(&self, x: &Matrix) -> Result<Vec<Vec<f64>>> {
        check_columns(x, self.slopes.len(), "ordinal prediction columns")?;
        let alpha: Vec<f64> = self.thresholds.iter().map(|c| c.estimate).collect();
        let beta: Vec<f64> = self.slopes.iter().map(|c| c.estimate).collect();
        Ok((0..x.nrows()).map(|i| ordinal_probabilities(&alpha, linear(x, i, &beta))).collect())
    }
}

/// The equation of one non-reference category of a baseline-category model.
#[derive(Debug, Clone, PartialEq)]
pub struct NominalEquation {
    /// Index of the category in [`NominalLogitFit::levels`].
    pub level: usize,
    /// `Intercept` then one slope per column of `X`.
    pub coefficients: Vec<GlmCoefficient>,
}

/// A fitted baseline-category (multinomial) logit model.
#[derive(Debug, Clone, PartialEq)]
pub struct NominalLogitFit {
    /// Response categories.
    pub levels: Vec<String>,
    /// Index of the reference category.
    pub reference: usize,
    /// One equation per non-reference category, in category order.
    pub equations: Vec<NominalEquation>,
    /// Covariance of the coefficients, equation by equation.
    pub covariance: Matrix,
    /// Fit statistics.
    pub statistics: PolytomousStatistics,
    /// Fitted category probabilities, one row per input row.
    pub probabilities: Vec<Vec<f64>>,
}

impl NominalLogitFit {
    /// Category probabilities for new rows (same columns as the fit).
    ///
    /// # Errors
    /// [`StatError::DimensionMismatch`] for the wrong number of columns.
    pub fn predict(&self, x: &Matrix) -> Result<Vec<Vec<f64>>> {
        let p = self.equations.first().map_or(0, |e| e.coefficients.len() - 1);
        check_columns(x, p, "nominal prediction columns")?;
        let theta: Vec<f64> = self.equations.iter().flat_map(|e| e.coefficients.iter().map(|c| c.estimate)).collect();
        Ok((0..x.nrows()).map(|i| nominal_probabilities(&theta, x, i, self.levels.len(), self.reference)).collect())
    }
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn check_columns(x: &Matrix, expected: usize, context: &'static str) -> Result<()> {
    if x.ncols() == expected {
        Ok(())
    } else {
        Err(StatError::DimensionMismatch { context, expected, found: x.ncols() })
    }
}

fn linear(x: &Matrix, i: usize, beta: &[f64]) -> f64 {
    beta.iter().enumerate().map(|(j, b)| x[(i, j)] * b).sum()
}

/// Logistic function `1/(1 + e^(−t))`, evaluated without overflow.
fn sigmoid(t: f64) -> f64 {
    if t >= 0.0 {
        1.0 / (1.0 + (-t).exp())
    } else {
        let e = t.exp();
        e / (1.0 + e)
    }
}

/// `F(u) − F(l)` for `l < u` (either may be infinite).
fn interval_probability(l: f64, u: f64) -> f64 {
    if l == f64::NEG_INFINITY {
        sigmoid(u)
    } else if u == f64::INFINITY {
        sigmoid(-l)
    } else {
        sigmoid(u) * sigmoid(-l) * -(l - u).exp_m1()
    }
}

fn ordinal_probabilities(alpha: &[f64], eta: f64) -> Vec<f64> {
    let cut = |j: usize| if j == 0 { f64::NEG_INFINITY } else { alpha.get(j - 1).map_or(f64::INFINITY, |a| a - eta) };
    (0..=alpha.len()).map(|k| interval_probability(cut(k), cut(k + 1))).collect()
}

fn nominal_probabilities(theta: &[f64], x: &Matrix, i: usize, levels: usize, reference: usize) -> Vec<f64> {
    let q = x.ncols() + 1;
    let mut eta = vec![0.0; levels];
    let mut e = 0;
    for (k, slot) in eta.iter_mut().enumerate() {
        if k == reference {
            continue;
        }
        let b = &theta[e * q..(e + 1) * q];
        *slot = b[0] + linear(x, i, &b[1..]);
        e += 1;
    }
    let m = eta.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let total: f64 = eta.iter().map(|v| (v - m).exp()).sum();
    eta.iter().map(|v| (v - m).exp() / total).collect()
}

/// Dense symmetric matrix (row-major, `n × n`).
#[derive(Debug, Clone)]
struct Sym {
    n: usize,
    a: Vec<f64>,
}

impl Sym {
    fn zeros(n: usize) -> Self {
        Self { n, a: vec![0.0; n * n] }
    }

    fn at(&self, i: usize, j: usize) -> f64 {
        self.a[i * self.n + j]
    }

    fn add(&mut self, i: usize, j: usize, v: f64) {
        self.a[i * self.n + j] += v;
    }

    /// Lower Cholesky factor, `None` unless positive definite.
    fn cholesky(&self) -> Option<Vec<f64>> {
        let n = self.n;
        let mut l = vec![0.0; n * n];
        let scale = (0..n).map(|i| self.at(i, i).abs()).fold(0.0, f64::max).max(f64::MIN_POSITIVE);
        for j in 0..n {
            let mut d = self.at(j, j);
            for k in 0..j {
                d -= l[j * n + k] * l[j * n + k];
            }
            if d.is_nan() || d <= 1e-13 * scale || d.is_infinite() {
                return None;
            }
            let d = d.sqrt();
            l[j * n + j] = d;
            for i in j + 1..n {
                let mut s = self.at(i, j);
                for k in 0..j {
                    s -= l[i * n + k] * l[j * n + k];
                }
                l[i * n + j] = s / d;
            }
        }
        Some(l)
    }

    fn solve(&self, l: &[f64], b: &[f64]) -> Vec<f64> {
        let n = self.n;
        let mut y = b.to_vec();
        for i in 0..n {
            for k in 0..i {
                y[i] -= l[i * n + k] * y[k];
            }
            y[i] /= l[i * n + i];
        }
        for i in (0..n).rev() {
            for k in i + 1..n {
                y[i] -= l[k * n + i] * y[k];
            }
            y[i] /= l[i * n + i];
        }
        y
    }

    fn inverse(&self) -> Option<Matrix> {
        let l = self.cholesky()?;
        let n = self.n;
        let mut out = Matrix::zeros(n, n);
        for j in 0..n {
            let mut e = vec![0.0; n];
            e[j] = 1.0;
            for (i, v) in self.solve(&l, &e).into_iter().enumerate() {
                out[(i, j)] = v;
            }
        }
        Some(out)
    }
}

/// Log-likelihood, gradient and negative Hessian at a parameter vector, or
/// `None` where the model is undefined (thresholds out of order).
type Evaluation = Option<(f64, Vec<f64>, Sym)>;

struct Maximum {
    theta: Vec<f64>,
    log_likelihood: f64,
    information: Sym,
    iterations: usize,
    converged: bool,
}

/// Newton–Raphson with step halving.
fn maximize(mut theta: Vec<f64>, max_iterations: usize, eval: impl Fn(&[f64]) -> Evaluation) -> Result<Maximum> {
    let (mut ll, mut grad, mut info) = eval(&theta).ok_or_else(|| invalid("the starting values are invalid"))?;
    let mut converged = false;
    let mut iterations = 0;
    while iterations < max_iterations {
        iterations += 1;
        let Some(l) = info.cholesky() else {
            if iterations == 1 {
                return Err(invalid(
                    "the model's predictor columns are linearly dependent (or a column is constant), so their \
                     effects cannot be separated",
                ));
            }
            break;
        };
        let step = info.solve(&l, &grad);
        let mut t = 1.0;
        let mut accepted = None;
        for _ in 0..40 {
            let trial: Vec<f64> = theta.iter().zip(&step).map(|(a, s)| a + t * s).collect();
            if let Some(e) = eval(&trial) {
                if e.0.is_finite() && e.0 >= ll - 1e-12 * (ll.abs() + 1.0) {
                    accepted = Some((trial, e));
                    break;
                }
            }
            t *= 0.5;
        }
        let Some((trial, (new_ll, new_grad, new_info))) = accepted else {
            // No step improves ℓ: at the maximum to rounding.
            converged = true;
            break;
        };
        let small_step = trial.iter().zip(&theta).all(|(a, b)| (a - b).abs() <= 1e-9 * (b.abs() + 1.0));
        let flat = (new_ll - ll).abs() <= 1e-12 * (ll.abs() + 1.0);
        theta = trial;
        ll = new_ll;
        grad = new_grad;
        info = new_info;
        if small_step && flat {
            converged = true;
            break;
        }
    }
    Ok(Maximum { theta, log_likelihood: ll, information: info, iterations, converged })
}

/// Validated inputs.
struct Prepared {
    weights: Vec<f64>,
    names: Vec<String>,
    level_weight: Vec<f64>,
    total: f64,
    n_rows: usize,
}

fn prepare(x: &Matrix, y: &[usize], levels: &[String], options: &PolytomousOptions) -> Result<Prepared> {
    let n = y.len();
    if x.nrows() != n {
        return Err(StatError::DimensionMismatch { context: "rows of X vs. responses", expected: n, found: x.nrows() });
    }
    let weights = if options.weights.is_empty() { vec![1.0; n] } else { options.weights.clone() };
    if weights.len() != n {
        return Err(StatError::DimensionMismatch { context: "weights", expected: n, found: weights.len() });
    }
    if weights.iter().any(|w| !(w.is_finite() && *w >= 0.0)) {
        return Err(invalid("weights must be finite and non-negative"));
    }
    if !options.names.is_empty() && options.names.len() != x.ncols() {
        return Err(StatError::DimensionMismatch {
            context: "column names",
            expected: x.ncols(),
            found: options.names.len(),
        });
    }
    if options.confidence_level.is_nan() || options.confidence_level <= 0.0 || options.confidence_level >= 1.0 {
        return Err(invalid("the confidence level must be between 0 and 1"));
    }
    if levels.len() < 2 {
        return Err(invalid("the response needs at least two categories"));
    }
    if !x.is_finite() {
        return Err(StatError::NonFiniteInput { context: "predictor values" });
    }
    let mut level_weight = vec![0.0; levels.len()];
    for (&k, &w) in y.iter().zip(&weights) {
        *level_weight
            .get_mut(k)
            .ok_or_else(|| invalid(format!("response code {k} is not one of the {} categories", levels.len())))? += w;
    }
    if let Some(k) = level_weight.iter().position(|&w| w <= 0.0) {
        return Err(invalid(format!("the response category {:?} has no observations", levels[k])));
    }
    let total: f64 = level_weight.iter().sum();
    let names = if options.names.is_empty() {
        (1..=x.ncols()).map(|j| format!("x{j}")).collect()
    } else {
        options.names.clone()
    };
    Ok(Prepared { n_rows: weights.iter().filter(|&&w| w > 0.0).count(), weights, names, level_weight, total })
}

fn coefficients(
    names: impl IntoIterator<Item = String>,
    theta: &[f64],
    covariance: &Matrix,
    offset: usize,
    z: f64,
) -> Result<Vec<GlmCoefficient>> {
    let normal = Normal::standard();
    names
        .into_iter()
        .enumerate()
        .map(|(j, name)| {
            let estimate = theta[offset + j];
            let std_error = covariance[(offset + j, offset + j)].max(0.0).sqrt();
            let statistic = estimate / std_error;
            Ok(GlmCoefficient {
                name,
                estimate,
                std_error,
                statistic,
                p_value: if statistic.is_nan() { f64::NAN } else { 2.0 * normal.sf(statistic.abs())? },
                confidence_interval: (estimate - z * std_error, estimate + z * std_error),
            })
        })
        .collect()
}

fn statistics(
    prep: &Prepared,
    max: &Maximum,
    n_slopes: usize,
    options: &PolytomousOptions,
) -> Result<PolytomousStatistics> {
    let ll = max.log_likelihood;
    let w = prep.total;
    let ll0: f64 = prep.level_weight.iter().map(|&wk| wk * (wk / w).ln()).sum();
    let k = max.theta.len();
    let chi = (2.0 * (ll - ll0)).max(0.0);
    let p_value = if n_slopes == 0 { 1.0 } else { ChiSquared::new(n_slopes as f64)?.sf(chi)? };
    let cox_snell = -(2.0 * (ll0 - ll) / w).exp_m1();
    let max_cs = -(2.0 * ll0 / w).exp_m1();
    let mut warnings = Vec::new();
    if !max.converged {
        warnings.push(format!(
            "The fit did not converge in {} iterations: the predictors may separate the categories, and the estimates \
             and standard errors are then unreliable.",
            max.iterations
        ));
    }
    Ok(PolytomousStatistics {
        log_likelihood: ll,
        null_log_likelihood: ll0,
        n_parameters: k,
        n_rows: prep.n_rows,
        total_weight: w,
        aic: -2.0 * ll + 2.0 * k as f64,
        bic: -2.0 * ll + k as f64 * w.ln(),
        whole_model: LikelihoodRatioTest { chi_square: chi, df: n_slopes, p_value },
        mcfadden_r2: if ll0 < 0.0 { 1.0 - ll / ll0 } else { 0.0 },
        nagelkerke_r2: if max_cs > 0.0 { cox_snell / max_cs } else { 0.0 },
        iterations: max.iterations,
        converged: max.converged,
        warnings,
        confidence_level: options.confidence_level,
    })
}

fn covariance_of(max: &Maximum) -> Result<Matrix> {
    max.information.inverse().ok_or_else(|| {
        invalid(
            "the information matrix is singular at the estimates: the predictors may separate the categories, or \
             columns are linearly dependent",
        )
    })
}

/// Fit the proportional-odds (cumulative logit) model of the ordered
/// response `y` (category codes `0 … J−1`, lowest first, named by `levels`)
/// on the columns of `x` (no intercept column; the thresholds play its
/// part), with the score test of parallel lines.
///
/// # Errors
/// * [`StatError::DimensionMismatch`] if `x`, `y`, the weights or the names
///   disagree in size.
/// * [`StatError::InvalidArgument`] for fewer than two categories, a code
///   outside `0 … J−1`, a category without observations, negative weights or
///   a bad confidence level.
/// * [`StatError::InvalidArgument`] for linearly dependent or constant
///   columns, or a singular information matrix at the estimates.
pub fn fit_ordinal_logistic(
    x: &Matrix,
    y: &[usize],
    levels: &[String],
    options: &PolytomousOptions,
) -> Result<OrdinalLogitFit> {
    let prep = prepare(x, y, levels, options)?;
    let cuts = levels.len() - 1;
    let p = x.ncols();
    // Start: thresholds at the logits of the cumulative proportions.
    let mut theta = Vec::with_capacity(cuts + p);
    let mut cumulative = 0.0;
    for wk in &prep.level_weight[..cuts] {
        cumulative += wk;
        let f = cumulative / prep.total;
        theta.push((f / (1.0 - f)).ln());
    }
    theta.extend(std::iter::repeat_n(0.0, p));
    let eval = |t: &[f64]| ordinal_evaluation(x, y, &prep.weights, cuts, t);
    let max = maximize(theta, options.max_iterations, eval)?;
    let covariance = covariance_of(&max)?;
    let z = Normal::standard().quantile(0.5 + options.confidence_level / 2.0)?;
    let threshold_names = (0..cuts).map(|j| format!("{} | {}", levels[j], levels[j + 1]));
    let thresholds = coefficients(threshold_names, &max.theta, &covariance, 0, z)?;
    let slopes = coefficients(prep.names.clone(), &max.theta, &covariance, cuts, z)?;
    let mut statistics = statistics(&prep, &max, p, options)?;
    let parallel_lines = if cuts >= 2 && p > 0 {
        let test = parallel_lines_test(x, y, &prep.weights, cuts, &max.theta)?;
        if test.is_none() {
            statistics.warnings.push(
                "The score test of parallel lines could not be computed: the general model's information is \
                 singular at these estimates."
                    .to_owned(),
            );
        }
        test
    } else {
        None
    };
    let alpha = &max.theta[..cuts];
    let beta = &max.theta[cuts..];
    let probabilities = (0..x.nrows()).map(|i| ordinal_probabilities(alpha, linear(x, i, beta))).collect();
    Ok(OrdinalLogitFit {
        levels: levels.to_vec(),
        thresholds,
        slopes,
        covariance,
        statistics,
        parallel_lines,
        probabilities,
    })
}

/// Logistic density `F(1 − F)` and its derivative `f(1 − 2F)` at `t`
/// (zero at ±∞).
fn density(t: f64) -> (f64, f64) {
    if t.is_infinite() {
        return (0.0, 0.0);
    }
    let f = sigmoid(t);
    let d = f * (1.0 - f);
    (d, d * (1.0 - 2.0 * f))
}

fn ordinal_evaluation(x: &Matrix, y: &[usize], w: &[f64], cuts: usize, theta: &[f64]) -> Evaluation {
    let (alpha, beta) = theta.split_at(cuts);
    if alpha.windows(2).any(|a| a[1] <= a[0]) || theta.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let q = theta.len();
    let p = beta.len();
    let mut ll = 0.0;
    let mut grad = vec![0.0; q];
    let mut info = Sym::zeros(q);
    let mut du = vec![0.0; q];
    let mut dl = vec![0.0; q];
    for (i, (&k, &wi)) in y.iter().zip(w).enumerate() {
        if wi == 0.0 {
            continue;
        }
        let eta = linear(x, i, beta);
        let upper = if k < cuts { alpha[k] - eta } else { f64::INFINITY };
        let lower = if k > 0 { alpha[k - 1] - eta } else { f64::NEG_INFINITY };
        let prob = interval_probability(lower, upper);
        if prob.is_nan() || prob <= 0.0 {
            return Some((f64::NEG_INFINITY, grad, info));
        }
        ll += wi * prob.ln();
        let (a, a1) = density(upper);
        let (b, b1) = density(lower);
        // Second derivatives of ln(F(u) − F(l)) in (u, l).
        let huu = a1 / prob - (a / prob).powi(2);
        let hll = -b1 / prob - (b / prob).powi(2);
        let hul = a * b / (prob * prob);
        // du/dθ and dl/dθ: the threshold indicator and −x.
        du.iter_mut().for_each(|v| *v = 0.0);
        dl.iter_mut().for_each(|v| *v = 0.0);
        if k < cuts {
            du[k] = 1.0;
        }
        if k > 0 {
            dl[k - 1] = 1.0;
        }
        for j in 0..p {
            du[cuts + j] = -x[(i, j)];
            dl[cuts + j] = -x[(i, j)];
        }
        for r in 0..q {
            grad[r] += wi * (a * du[r] - b * dl[r]) / prob;
            for c in 0..=r {
                let h = huu * du[r] * du[c] + hll * dl[r] * dl[c] + hul * (du[r] * dl[c] + dl[r] * du[c]);
                info.add(r, c, -wi * h);
            }
        }
    }
    for r in 0..q {
        for c in 0..r {
            let v = info.at(r, c);
            info.a[c * q + r] = v;
        }
    }
    Some((ll, grad, info))
}

/// Score test of `β₁ = … = β_{J−1}` in the general cumulative logit model,
/// at the proportional-odds estimates `theta = (α, β)`.
fn parallel_lines_test(x: &Matrix, y: &[usize], w: &[f64], cuts: usize, theta: &[f64]) -> Result<Option<ScoreTest>> {
    let (alpha, beta) = theta.split_at(cuts);
    let p = beta.len();
    // General-model parameters: α₁…α_{J−1}, then β₁ (p values), …, β_{J−1}.
    let q = cuts + cuts * p;
    let mut score = vec![0.0; q];
    let mut info = Sym::zeros(q);
    // Gradient of γⱼ = F(αⱼ − xᵀβⱼ): fⱼ at αⱼ, −fⱼ x in the βⱼ block.
    let gamma_grad = |i: usize, j: usize, f: f64, g: &mut [f64]| {
        g[j] += f;
        for m in 0..p {
            g[cuts + j * p + m] -= f * x[(i, m)];
        }
    };
    for (i, (&k, &wi)) in y.iter().zip(w).enumerate() {
        if wi == 0.0 {
            continue;
        }
        let eta = linear(x, i, beta);
        let probs = ordinal_probabilities(alpha, eta);
        let dens: Vec<f64> = alpha.iter().map(|a| density(a - eta).0).collect();
        for (c, &prob) in probs.iter().enumerate() {
            if prob.is_nan() || prob <= 0.0 {
                continue;
            }
            // ∂π_c/∂θ = ∂γ_c/∂θ − ∂γ_{c−1}/∂θ.
            let mut g = vec![0.0; q];
            if c < cuts {
                gamma_grad(i, c, dens[c], &mut g);
            }
            if c > 0 {
                gamma_grad(i, c - 1, -dens[c - 1], &mut g);
            }
            if c == k {
                for (s, v) in score.iter_mut().zip(&g) {
                    *s += wi * v / prob;
                }
            }
            for r in 0..q {
                if g[r] == 0.0 {
                    continue;
                }
                for s in 0..q {
                    info.add(r, s, wi * g[r] * g[s] / prob);
                }
            }
        }
    }
    let Some(l) = info.cholesky() else { return Ok(None) };
    let solved = info.solve(&l, &score);
    let chi: f64 = score.iter().zip(&solved).map(|(a, b)| a * b).sum::<f64>().max(0.0);
    let df = p * (cuts - 1);
    Ok(Some(ScoreTest { chi_square: chi, df, p_value: ChiSquared::new(df as f64)?.sf(chi)? }))
}

/// Fit the baseline-category (multinomial) logit model of the response `y`
/// (category codes `0 … J−1`, named by `levels`) on the columns of `x` (an
/// intercept is added to every equation), against the category `reference`.
///
/// # Errors
/// As [`fit_ordinal_logistic`], and [`StatError::InvalidArgument`] if
/// `reference` is not a category.
pub fn fit_nominal_logistic(
    x: &Matrix,
    y: &[usize],
    levels: &[String],
    reference: usize,
    options: &PolytomousOptions,
) -> Result<NominalLogitFit> {
    let prep = prepare(x, y, levels, options)?;
    if reference >= levels.len() {
        return Err(invalid(format!("the reference category {reference} is not one of the {}", levels.len())));
    }
    let n_levels = levels.len();
    let q = x.ncols() + 1;
    let mut theta = Vec::with_capacity((n_levels - 1) * q);
    for k in (0..n_levels).filter(|&k| k != reference) {
        theta.push((prep.level_weight[k] / prep.level_weight[reference]).ln());
        theta.extend(std::iter::repeat_n(0.0, q - 1));
    }
    let eval = |t: &[f64]| nominal_evaluation(x, y, &prep.weights, n_levels, reference, t);
    let max = maximize(theta, options.max_iterations, eval)?;
    let covariance = covariance_of(&max)?;
    let z = Normal::standard().quantile(0.5 + options.confidence_level / 2.0)?;
    let mut equations = Vec::with_capacity(n_levels - 1);
    for (e, level) in (0..n_levels).filter(|&k| k != reference).enumerate() {
        let names = std::iter::once("Intercept".to_owned()).chain(prep.names.iter().cloned());
        equations
            .push(NominalEquation { level, coefficients: coefficients(names, &max.theta, &covariance, e * q, z)? });
    }
    let statistics = statistics(&prep, &max, (n_levels - 1) * (q - 1), options)?;
    let probabilities = (0..x.nrows()).map(|i| nominal_probabilities(&max.theta, x, i, n_levels, reference)).collect();
    Ok(NominalLogitFit { levels: levels.to_vec(), reference, equations, covariance, statistics, probabilities })
}

fn nominal_evaluation(
    x: &Matrix,
    y: &[usize],
    w: &[f64],
    levels: usize,
    reference: usize,
    theta: &[f64],
) -> Evaluation {
    if theta.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let q = x.ncols() + 1;
    let eqs = levels - 1;
    let dim = theta.len();
    let mut ll = 0.0;
    let mut grad = vec![0.0; dim];
    let mut info = Sym::zeros(dim);
    let mut xt = vec![0.0; q];
    for (i, (&k, &wi)) in y.iter().zip(w).enumerate() {
        if wi == 0.0 {
            continue;
        }
        let probs = nominal_probabilities(theta, x, i, levels, reference);
        ll += wi * probs[k].ln();
        xt[0] = 1.0;
        for j in 1..q {
            xt[j] = x[(i, j - 1)];
        }
        // Probabilities of the non-reference categories, by equation.
        let pe: Vec<f64> = (0..levels).filter(|&c| c != reference).map(|c| probs[c]).collect();
        let ye: Vec<f64> = (0..levels).filter(|&c| c != reference).map(|c| f64::from(u8::from(c == k))).collect();
        for a in 0..eqs {
            for r in 0..q {
                grad[a * q + r] += wi * (ye[a] - pe[a]) * xt[r];
            }
            for b in 0..=a {
                let v = wi * pe[a] * (f64::from(u8::from(a == b)) - pe[b]);
                for r in 0..q {
                    for s in 0..q {
                        info.add(a * q + r, b * q + s, v * xt[r] * xt[s]);
                    }
                }
            }
        }
    }
    for a in 0..eqs {
        for b in 0..a {
            for r in 0..q {
                for s in 0..q {
                    let v = info.at(a * q + r, b * q + s);
                    info.a[(b * q + s) * dim + a * q + r] = v;
                }
            }
        }
    }
    if !ll.is_finite() {
        return Some((f64::NEG_INFINITY, grad, info));
    }
    Some((ll, grad, info))
}

#[cfg(test)]
mod tests;
