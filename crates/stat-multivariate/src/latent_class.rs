//! Latent class analysis of categorical items by maximum likelihood (EM),
//! with model choice across numbers of classes by BIC.
//!
//! # Model
//! Each respondent belongs to one of `K` unobserved classes with prior
//! probabilities (class sizes) `πₖ > 0`, `Σ πₖ = 1`. Within a class the `J`
//! items are independent (local independence; Lazarsfeld & Henry 1968,
//! *Latent Structure Analysis*; Goodman 1974, "Exploratory latent structure
//! analysis using both identifiable and unidentifiable models", *Biometrika*
//! 61), and item `j` takes its level `r ∈ {1, …, Rⱼ}` with probability
//! `ρⱼₖᵣ` (`Σᵣ ρⱼₖᵣ = 1`). A response pattern `y` has probability
//!
//! `P(y) = Σₖ πₖ Πⱼ Πᵣ ρⱼₖᵣ^[yⱼ = r]`.
//!
//! **Missing responses.** A missing item is left out of the product for that
//! respondent (its factor is 1), which is the likelihood of the observed
//! responses when items are missing at random (full-information maximum
//! likelihood, as poLCA and stepmix do). A respondent with every item
//! missing carries no information and must be dropped by the caller.
//!
//! The model has `m = (K − 1) + K Σⱼ (Rⱼ − 1)` free parameters.
//!
//! # Estimation
//! EM as set out by Linzer & Lewis (2011, "poLCA: An R package for
//! polytomous variable latent class analysis", *Journal of Statistical
//! Software* 42(10)). The E-step computes posterior class probabilities
//! `hᵢₖ = πₖ fₖ(yᵢ) / Σₗ πₗ fₗ(yᵢ)` on the log scale; the M-step sets
//! `πₖ = Σᵢ hᵢₖ / n` and
//!
//! `ρⱼₖᵣ = (Σᵢ∈Oⱼ hᵢₖ [yᵢⱼ = r] + α) / (Σᵢ∈Oⱼ hᵢₖ + Rⱼ α)`,
//!
//! with `Oⱼ` the respondents who answered item `j` and `α ≥ 0` an optional
//! Dirichlet smoothing (pseudo-count per level, `0` by default — plain
//! maximum likelihood). With `α > 0` the estimates are posterior modes under
//! a symmetric Dirichlet(`α + 1`) prior on each `ρⱼₖ·`, which keeps them off
//! the 0/1 boundary; the reported log-likelihood is still the plain one.
//! Iterations stop when the objective changes by less than
//! `tolerance · (1 + |ℓ|)`.
//!
//! **Starts.** EM finds local maxima, so it runs from several random starts
//! (as in poLCA): each draws every `ρⱼₖ·` uniformly at random and normalizes
//! it, with equal class sizes. The start with the largest log-likelihood is
//! kept, and the number of starts that reached it (within `10⁻⁶` relative)
//! is reported. Everything is reproducible from the seed.
//!
//! **Class order.** Classes are relabelled from largest to smallest
//! estimated size, so labels do not depend on the start (label switching).
//!
//! # Standard errors
//! From the observed information of the log-likelihood (the negative
//! Hessian), computed analytically in the multinomial-logit parameters
//! `πₖ ∝ exp γₖ`, `ρⱼₖᵣ ∝ exp βⱼₖᵣ` (one baseline per vector — its largest
//! element — fixed at 0). For one respondent with `gₖ = πₖ fₖ(yᵢ)` and
//! `aᵢₖ = ∂ log gₖ / ∂θ`,
//!
//! `∂² ℓᵢ = Σₖ hᵢₖ (∂² log gₖ + aᵢₖ aᵢₖᵀ) − sᵢ sᵢᵀ`, `sᵢ = Σₖ hᵢₖ aᵢₖ`,
//!
//! where `∂ log gₖ / ∂γ = eₖ − π`, `∂ log gₖ / ∂βⱼₖ = e_{yᵢⱼ} − ρⱼₖ` for each
//! answered item, and `∂² log gₖ` is the negative multinomial covariance
//! `−(diag p − p pᵀ)` of each block. The covariance of the logits is the
//! inverse information; the delta method (`∂p/∂β = diag p − p pᵀ`) carries
//! it to the probabilities. Linzer & Lewis (2011) use the same delta method
//! with the outer product of the scores in place of the Hessian; the two
//! agree asymptotically. A probability on the boundary (below `10⁻⁷`) has
//! no finite logit: it is held fixed and its standard error is reported as
//! not available (NaN). If the information is not positive definite (an
//! unidentified model, Goodman 1974) every standard error is NaN.
//! The standard errors use the likelihood alone, also with smoothing.
//!
//! # Goodness of fit
//! Over the `C = Πⱼ Rⱼ` response patterns, using the `N` respondents who
//! answered every item, with observed counts `oₚ` and expected counts
//! `eₚ = N P(p)`: the likelihood-ratio statistic `G² = 2 Σ oₚ ln(oₚ/eₚ)` and
//! Pearson's `X² = Σ (oₚ − eₚ)² / eₚ` (patterns never observed contribute
//! `eₚ` each), each referred to χ² on `C − 1 − m` degrees of freedom. With
//! many items most patterns are rare and these p-values are unreliable;
//! compare models by the information criteria instead.
//!
//! # Information criteria and classification
//! `AIC = −2ℓ + 2m`, `BIC = −2ℓ + m ln n` (Schwarz 1978), the
//! sample-size-adjusted BIC `−2ℓ + m ln((n + 2)/24)` (Sclove 1987), and the
//! entropy `R² = 1 − Σᵢ Σₖ (−hᵢₖ ln hᵢₖ) / (n ln K)` (Ramaswamy et al. 1993;
//! 1 for perfect separation, defined as 1 for one class).
//! [`select_latent_classes`] fits each requested number of classes and
//! chooses the smallest BIC. A number of classes with more parameters than
//! the `C − 1` the pattern table can identify (`C − 1 − m < 0`) is reported
//! as not fitted.

use stat_core::distributions::{ChiSquared, ContinuousDistribution};
use stat_core::rng::Rng;
use stat_core::{Result, StatError};

use crate::spd::{cholesky, inverse};

/// Probabilities below this are treated as on the boundary of the
/// parameter space when computing standard errors.
const BOUNDARY: f64 = 1e-7;

/// Settings of one latent class fit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LatentClassOptions {
    /// Number of classes `K` (at least 1).
    pub k: usize,
    /// Number of random EM starts (at least 1).
    pub starts: usize,
    /// Most EM iterations per start.
    pub max_iterations: usize,
    /// Relative change in the objective that ends the iterations.
    pub tolerance: f64,
    /// Dirichlet smoothing `α ≥ 0` added to every level count in the M-step
    /// (0: maximum likelihood).
    pub smoothing: f64,
    /// Seed of the random starts.
    pub seed: u64,
}

impl LatentClassOptions {
    /// `k` classes; 10 starts; up to 5000 iterations with tolerance 1e-10;
    /// no smoothing; seed 1.
    #[must_use]
    pub fn new(k: usize) -> Self {
        Self { k, starts: 10, max_iterations: 5000, tolerance: 1e-10, smoothing: 0.0, seed: 1 }
    }
}

/// Goodness of fit of a latent class model over the response patterns.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternFit {
    /// Respondents who answered every item (`N`).
    pub complete_rows: usize,
    /// Number of possible response patterns `C = Πⱼ Rⱼ`.
    pub cells: f64,
    /// Distinct patterns observed among the complete respondents.
    pub observed_patterns: usize,
    /// Likelihood-ratio statistic `G²`.
    pub g2: f64,
    /// Pearson's `X²`.
    pub chi2: f64,
    /// Residual degrees of freedom `C − 1 − m` (may be negative).
    pub df: f64,
    /// p-value of `G²` (NaN when `df ≤ 0`).
    pub g2_p: f64,
    /// p-value of `X²` (NaN when `df ≤ 0`).
    pub chi2_p: f64,
}

/// A fitted latent class model. Classes are ordered from largest to
/// smallest.
#[derive(Debug, Clone, PartialEq)]
pub struct LatentClassFit {
    /// Number of levels `Rⱼ` of each item.
    pub levels: Vec<usize>,
    /// Class sizes (prior probabilities) `πₖ`.
    pub class_sizes: Vec<f64>,
    /// Standard errors of the class sizes (NaN when not available).
    pub class_size_se: Vec<f64>,
    /// Item-response probabilities `ρⱼₖᵣ`, indexed `[item][class][level]`.
    pub probabilities: Vec<Vec<Vec<f64>>>,
    /// Standard errors of `ρⱼₖᵣ`, same layout (NaN when not available).
    pub probability_se: Vec<Vec<Vec<f64>>>,
    /// Posterior class probabilities `hᵢₖ` (`n` rows of `K`).
    pub posterior: Vec<Vec<f64>>,
    /// Maximized log-likelihood `ℓ`.
    pub log_likelihood: f64,
    /// EM iterations of the kept start.
    pub iterations: usize,
    /// Whether the kept start met the tolerance.
    pub converged: bool,
    /// Starts whose log-likelihood came within `10⁻⁶` (relative) of the best.
    pub starts_at_best: usize,
    /// Starts run.
    pub starts: usize,
    /// Dirichlet smoothing used.
    pub smoothing: f64,
    /// Pattern goodness of fit.
    pub pattern_fit: PatternFit,
}

impl LatentClassFit {
    /// Number of classes `K`.
    #[must_use]
    pub fn k(&self) -> usize {
        self.class_sizes.len()
    }

    /// Number of respondents `n`.
    #[must_use]
    pub fn n(&self) -> usize {
        self.posterior.len()
    }

    /// Free parameters `m = (K − 1) + K Σ (Rⱼ − 1)`.
    #[must_use]
    pub fn n_parameters(&self) -> usize {
        parameters(self.k(), &self.levels)
    }

    /// `AIC = −2ℓ + 2m`.
    #[must_use]
    pub fn aic(&self) -> f64 {
        -2.0 * self.log_likelihood + 2.0 * self.n_parameters() as f64
    }

    /// `BIC = −2ℓ + m ln n` (smaller is better).
    #[must_use]
    pub fn bic(&self) -> f64 {
        -2.0 * self.log_likelihood + self.n_parameters() as f64 * (self.n() as f64).ln()
    }

    /// Sample-size-adjusted BIC, `−2ℓ + m ln((n + 2)/24)` (Sclove 1987).
    #[must_use]
    pub fn adjusted_bic(&self) -> f64 {
        -2.0 * self.log_likelihood + self.n_parameters() as f64 * ((self.n() as f64 + 2.0) / 24.0).ln()
    }

    /// Entropy `R² = 1 − Σ −h ln h / (n ln K)`; 1 for one class.
    #[must_use]
    pub fn entropy_r2(&self) -> f64 {
        let k = self.k();
        if k < 2 {
            return 1.0;
        }
        let e: f64 = self.posterior.iter().flatten().filter(|&&h| h > 0.0).map(|&h| -h * h.ln()).sum();
        1.0 - e / (self.n() as f64 * (k as f64).ln())
    }

    /// Most likely (modal) class of each respondent (`0..K`).
    #[must_use]
    pub fn classification(&self) -> Vec<usize> {
        self.posterior.iter().map(|h| argmax(h)).collect()
    }

    /// Average posterior probabilities by modal class: row `a`, column `b`
    /// is the mean of `hᵢ_b` over the respondents assigned to class `a`
    /// (NaN for an empty class). A diagonal near 1 means clear assignment.
    #[must_use]
    pub fn average_posterior(&self) -> Vec<Vec<f64>> {
        let k = self.k();
        let mut sum = vec![vec![0.0; k]; k];
        let mut count = vec![0usize; k];
        for (h, c) in self.posterior.iter().zip(self.classification()) {
            count[c] += 1;
            for (s, v) in sum[c].iter_mut().zip(h) {
                *s += v;
            }
        }
        sum.into_iter()
            .zip(count)
            .map(|(row, n)| row.into_iter().map(|s| if n == 0 { f64::NAN } else { s / n as f64 }).collect())
            .collect()
    }

    /// Posterior class probabilities of new response patterns (missing
    /// items left out).
    ///
    /// # Errors
    /// A pattern of the wrong length or with a level out of range.
    pub fn predict(&self, responses: &[Vec<Option<usize>>]) -> Result<Vec<Vec<f64>>> {
        check_codes(responses, &self.levels)?;
        let model = Model { pi: self.class_sizes.clone(), rho: self.probabilities.clone() };
        Ok(e_step(responses, &model).0)
    }
}

/// Free parameters of `k` classes over items with `levels`.
#[must_use]
pub fn parameters(k: usize, levels: &[usize]) -> usize {
    k.saturating_sub(1) + k * levels.iter().map(|r| r.saturating_sub(1)).sum::<usize>()
}

/// Number of response patterns `Πⱼ Rⱼ` (as a float: it can be huge).
fn cells(levels: &[usize]) -> f64 {
    levels.iter().map(|&r| r as f64).product()
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn argmax(z: &[f64]) -> usize {
    let mut best = 0;
    for (k, &v) in z.iter().enumerate() {
        if v > z[best] {
            best = k;
        }
    }
    best
}

fn check_codes(responses: &[Vec<Option<usize>>], levels: &[usize]) -> Result<()> {
    let j = levels.len();
    for row in responses {
        if row.len() != j {
            return Err(invalid("every respondent needs one entry per item"));
        }
        if let Some((item, code)) =
            row.iter().enumerate().find_map(|(i, y)| y.filter(|&c| c >= levels[i]).map(|c| (i, c)))
        {
            return Err(invalid(format!(
                "item {} has level code {code}, beyond its {} levels",
                item + 1,
                levels[item]
            )));
        }
    }
    Ok(())
}

fn check_data(responses: &[Vec<Option<usize>>], levels: &[usize]) -> Result<()> {
    if levels.is_empty() {
        return Err(invalid("choose at least one item"));
    }
    if let Some(j) = levels.iter().position(|&r| r < 2) {
        return Err(invalid(format!("item {} needs at least two levels", j + 1)));
    }
    if responses.len() < 2 {
        return Err(StatError::InsufficientData { required: 2, found: responses.len() });
    }
    check_codes(responses, levels)?;
    if responses.iter().any(|row| row.iter().all(Option::is_none)) {
        return Err(invalid("a respondent has every item missing; drop such rows first"));
    }
    Ok(())
}

/// Current parameters: class sizes and `[item][class][level]` probabilities.
struct Model {
    pi: Vec<f64>,
    rho: Vec<Vec<Vec<f64>>>,
}

/// Posterior probabilities and the log-likelihood.
fn e_step(responses: &[Vec<Option<usize>>], model: &Model) -> (Vec<Vec<f64>>, f64) {
    let k = model.pi.len();
    let log_pi: Vec<f64> = model.pi.iter().map(|p| p.ln()).collect();
    let log_rho: Vec<Vec<Vec<f64>>> =
        model.rho.iter().map(|item| item.iter().map(|c| c.iter().map(|p| p.ln()).collect()).collect()).collect();
    let mut ll = 0.0;
    let mut post = Vec::with_capacity(responses.len());
    let mut logs = vec![0.0; k];
    for row in responses {
        for (c, l) in logs.iter_mut().enumerate() {
            *l = log_pi[c] + row.iter().enumerate().filter_map(|(j, y)| y.map(|r| log_rho[j][c][r])).sum::<f64>();
        }
        let m = logs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let lse = m + logs.iter().map(|v| (v - m).exp()).sum::<f64>().ln();
        ll += lse;
        post.push(logs.iter().map(|v| (v - lse).exp()).collect());
    }
    (post, ll)
}

/// M-step from posterior probabilities.
fn m_step(responses: &[Vec<Option<usize>>], levels: &[usize], h: &[Vec<f64>], alpha: f64) -> Model {
    let k = h[0].len();
    let n = responses.len() as f64;
    let pi: Vec<f64> = (0..k).map(|c| h.iter().map(|hi| hi[c]).sum::<f64>() / n).collect();
    let mut counts: Vec<Vec<Vec<f64>>> = levels.iter().map(|&r| vec![vec![alpha; r]; k]).collect();
    for (row, hi) in responses.iter().zip(h) {
        for (j, y) in row.iter().enumerate() {
            if let Some(r) = *y {
                for c in 0..k {
                    counts[j][c][r] += hi[c];
                }
            }
        }
    }
    let rho = counts
        .into_iter()
        .map(|item| {
            item.into_iter()
                .map(|cls| {
                    let total: f64 = cls.iter().sum();
                    if total > 0.0 {
                        cls.iter().map(|v| v / total).collect()
                    } else {
                        vec![1.0 / cls.len() as f64; cls.len()]
                    }
                })
                .collect()
        })
        .collect();
    Model { pi, rho }
}

/// Log of the Dirichlet smoothing term `α Σ ln ρ` (0 without smoothing).
fn penalty(model: &Model, alpha: f64) -> f64 {
    if alpha == 0.0 {
        return 0.0;
    }
    alpha * model.rho.iter().flatten().flatten().map(|p| p.ln()).sum::<f64>()
}

/// One EM run from `model`.
fn em(
    responses: &[Vec<Option<usize>>],
    levels: &[usize],
    mut model: Model,
    options: &LatentClassOptions,
) -> (Model, Vec<Vec<f64>>, f64, usize, bool) {
    let (mut h, mut ll) = e_step(responses, &model);
    let mut objective = ll + penalty(&model, options.smoothing);
    let mut iterations = 0;
    let mut converged = false;
    while iterations < options.max_iterations {
        iterations += 1;
        model = m_step(responses, levels, &h, options.smoothing);
        let (hn, lln) = e_step(responses, &model);
        h = hn;
        ll = lln;
        let next = ll + penalty(&model, options.smoothing);
        let change = next - objective;
        objective = next;
        if change.abs() <= options.tolerance * (1.0 + objective.abs()) {
            converged = true;
            break;
        }
    }
    (model, h, ll, iterations, converged)
}

/// A random start: every `ρⱼₖ·` drawn uniformly and normalized, equal sizes.
fn random_start(levels: &[usize], k: usize, rng: &mut Rng) -> Model {
    let rho = levels
        .iter()
        .map(|&r| {
            (0..k)
                .map(|_| {
                    let v: Vec<f64> = (0..r).map(|_| 0.05 + rng.next_f64()).collect();
                    let s: f64 = v.iter().sum();
                    v.into_iter().map(|x| x / s).collect()
                })
                .collect()
        })
        .collect();
    Model { pi: vec![1.0 / k as f64; k], rho }
}

/// Fit a `K`-class model by EM from `options.starts` random starts, keeping
/// the largest log-likelihood; classes are ordered by size.
///
/// `responses[i][j]` is respondent `i`'s level code (`0..levels[j]`) on item
/// `j`, or `None` when missing.
///
/// # Errors
/// [`StatError::InvalidArgument`] for no items, an item with fewer than two
/// levels, a code out of range, a respondent with every item missing,
/// `K = 0`, `K` above the number of respondents, more parameters than the
/// pattern table identifies (`Πⱼ Rⱼ − 1 < m`), no starts or a negative
/// smoothing; [`StatError::InsufficientData`] for fewer than two
/// respondents.
pub fn fit_latent_classes(
    responses: &[Vec<Option<usize>>],
    levels: &[usize],
    options: &LatentClassOptions,
) -> Result<LatentClassFit> {
    check_data(responses, levels)?;
    let (n, k) = (responses.len(), options.k);
    if k == 0 || k > n {
        return Err(invalid(format!("the number of classes must be between 1 and the number of rows ({n})")));
    }
    if options.starts == 0 {
        return Err(invalid("use at least one start"));
    }
    if !(options.smoothing >= 0.0 && options.smoothing.is_finite()) {
        return Err(invalid("the smoothing must be zero or positive"));
    }
    let m = parameters(k, levels);
    if (m as f64) > cells(levels) - 1.0 {
        return Err(invalid(format!(
            "{k} classes need {m} parameters, more than the {} response patterns can identify",
            cells(levels) - 1.0
        )));
    }
    let starts = if k == 1 { 1 } else { options.starts };
    let mut results = Vec::with_capacity(starts);
    for s in 0..starts {
        let seed = options.seed.wrapping_add((s as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let start = random_start(levels, k, &mut Rng::new(seed));
        results.push(em(responses, levels, start, options));
    }
    let best = (0..results.len()).max_by(|&a, &b| results[a].2.total_cmp(&results[b].2)).unwrap_or(0);
    let best_ll = results[best].2;
    let starts_at_best = results.iter().filter(|r| (r.2 - best_ll).abs() <= 1e-6 * (1.0 + best_ll.abs())).count();
    let (model, h, ll, iterations, converged) = results.swap_remove(best);
    // Largest class first.
    let mut order: Vec<usize> = (0..k).collect();
    order.sort_by(|&a, &b| model.pi[b].total_cmp(&model.pi[a]).then(a.cmp(&b)));
    let model = Model {
        pi: order.iter().map(|&c| model.pi[c]).collect(),
        rho: model.rho.iter().map(|item| order.iter().map(|&c| item[c].clone()).collect()).collect(),
    };
    let posterior: Vec<Vec<f64>> = h.iter().map(|hi| order.iter().map(|&c| hi[c]).collect()).collect();
    let (class_size_se, probability_se) = standard_errors(responses, levels, &model, &posterior);
    let pattern_fit = pattern_fit(responses, levels, &model, m);
    Ok(LatentClassFit {
        levels: levels.to_vec(),
        class_sizes: model.pi,
        class_size_se,
        probabilities: model.rho,
        probability_se,
        posterior,
        log_likelihood: ll,
        iterations,
        converged,
        starts_at_best,
        starts,
        smoothing: options.smoothing,
        pattern_fit,
    })
}

/// `G²` and `X²` over the response patterns of the complete respondents.
fn pattern_fit(responses: &[Vec<Option<usize>>], levels: &[usize], model: &Model, m: usize) -> PatternFit {
    let mut counts: std::collections::BTreeMap<Vec<usize>, usize> = std::collections::BTreeMap::new();
    for row in responses {
        if let Some(p) = row.iter().copied().collect::<Option<Vec<usize>>>() {
            *counts.entry(p).or_default() += 1;
        }
    }
    let total: usize = counts.values().sum();
    let nf = total as f64;
    let (mut g2, mut chi2, mut expected_seen) = (0.0, 0.0, 0.0);
    for (p, &o) in &counts {
        let prob: f64 = (0..model.pi.len())
            .map(|c| model.pi[c] * p.iter().enumerate().map(|(j, &r)| model.rho[j][c][r]).product::<f64>())
            .sum();
        let e = nf * prob;
        let o = o as f64;
        expected_seen += e;
        g2 += 2.0 * o * (o / e).ln();
        chi2 += (o - e).powi(2) / e;
    }
    // Every unobserved pattern adds its expected count.
    chi2 += (nf - expected_seen).max(0.0);
    let c = cells(levels);
    let df = c - 1.0 - m as f64;
    let p_value = |x: f64| {
        if df > 0.0 && total > 0 { ChiSquared::new(df).and_then(|d| d.sf(x)).unwrap_or(f64::NAN) } else { f64::NAN }
    };
    PatternFit {
        complete_rows: total,
        cells: c,
        observed_patterns: counts.len(),
        g2,
        chi2,
        df,
        g2_p: p_value(g2),
        chi2_p: p_value(chi2),
    }
}

/// One free logit parameter: class size (`item = None`) or `ρⱼₖᵣ`.
#[derive(Clone, Copy)]
struct Free {
    item: Option<usize>,
    class: usize,
    level: usize,
}

/// Standard errors of `π` and `ρ` from the analytic observed information
/// (module docs).
#[allow(clippy::type_complexity)]
fn standard_errors(
    responses: &[Vec<Option<usize>>],
    levels: &[usize],
    model: &Model,
    h: &[Vec<f64>],
) -> (Vec<f64>, Vec<Vec<Vec<f64>>>) {
    let k = model.pi.len();
    let nan_rho = || levels.iter().map(|&r| vec![vec![f64::NAN; r]; k]).collect::<Vec<_>>();
    // Baselines (largest element) and free parameters.
    let base_pi = argmax(&model.pi);
    let base_rho: Vec<Vec<usize>> = model.rho.iter().map(|item| item.iter().map(|p| argmax(p)).collect()).collect();
    let mut free: Vec<Free> = Vec::new();
    let mut pi_index = vec![None; k];
    for c in 0..k {
        if c != base_pi && model.pi[c] > BOUNDARY {
            pi_index[c] = Some(free.len());
            free.push(Free { item: None, class: c, level: 0 });
        }
    }
    let mut rho_index: Vec<Vec<Vec<Option<usize>>>> = levels.iter().map(|&r| vec![vec![None; r]; k]).collect();
    for (j, &rj) in levels.iter().enumerate() {
        for c in 0..k {
            for r in 0..rj {
                if r != base_rho[j][c] && model.rho[j][c][r] > BOUNDARY {
                    rho_index[j][c][r] = Some(free.len());
                    free.push(Free { item: Some(j), class: c, level: r });
                }
            }
        }
    }
    let m = free.len();
    // Observed information I = −H.
    let mut info = vec![vec![0.0; m]; m];
    let n = responses.len() as f64;
    // Curvature of log g: π block n (diag π − π πᵀ); item blocks N_jk (diag ρ − ρ ρᵀ).
    let mut answered = vec![vec![0.0; k]; levels.len()];
    for (row, hi) in responses.iter().zip(h) {
        for (j, y) in row.iter().enumerate() {
            if y.is_some() {
                for c in 0..k {
                    answered[j][c] += hi[c];
                }
            }
        }
    }
    for (a, fa) in free.iter().enumerate() {
        for (b, fb) in free.iter().enumerate() {
            let v = match (fa.item, fb.item) {
                (None, None) => {
                    let (pa, pb) = (model.pi[fa.class], model.pi[fb.class]);
                    n * (if a == b { pa } else { 0.0 } - pa * pb)
                }
                (Some(ja), Some(jb)) if ja == jb && fa.class == fb.class => {
                    let p = &model.rho[ja][fa.class];
                    answered[ja][fa.class] * (if a == b { p[fa.level] } else { 0.0 } - p[fa.level] * p[fb.level])
                }
                _ => 0.0,
            };
            info[a][b] += v;
        }
    }
    // − Σᵢ [Σₖ hᵢₖ aᵢₖ aᵢₖᵀ − sᵢ sᵢᵀ].
    let mut s = vec![0.0; m];
    let mut support: Vec<(usize, f64)> = Vec::new();
    for (row, hi) in responses.iter().zip(h) {
        s.iter_mut().for_each(|v| *v = 0.0);
        for c in 0..k {
            support.clear();
            for (c2, idx) in pi_index.iter().enumerate() {
                if let Some(i) = *idx {
                    support.push((i, f64::from(u8::from(c2 == c)) - model.pi[c2]));
                }
            }
            for (j, y) in row.iter().enumerate() {
                if let Some(obs) = *y {
                    for (r, idx) in rho_index[j][c].iter().enumerate() {
                        if let Some(i) = *idx {
                            support.push((i, f64::from(u8::from(r == obs)) - model.rho[j][c][r]));
                        }
                    }
                }
            }
            let w = hi[c];
            for &(i, ai) in &support {
                s[i] += w * ai;
                for &(l, al) in &support {
                    info[i][l] -= w * ai * al;
                }
            }
        }
        for i in 0..m {
            if s[i] != 0.0 {
                for l in 0..m {
                    info[i][l] += s[i] * s[l];
                }
            }
        }
    }
    if m == 0 {
        return (vec![0.0; k], levels.iter().map(|&r| vec![vec![0.0; r]; k]).collect());
    }
    let Some(l) = cholesky(&info) else {
        return (vec![f64::NAN; k], nan_rho());
    };
    let cov = inverse(&l);
    // Delta method for one probability vector p with free logits at `idx`:
    // ∂p_r/∂β_s = p_r (δ_rs − p_s).
    let delta = |p: &[f64], idx: &[Option<usize>]| -> Vec<f64> {
        let frees: Vec<(usize, usize)> = idx.iter().enumerate().filter_map(|(s, i)| i.map(|i| (s, i))).collect();
        (0..p.len())
            .map(|r| {
                if p[r] <= BOUNDARY {
                    return f64::NAN;
                }
                let grad: Vec<f64> = frees.iter().map(|&(s, _)| p[r] * (f64::from(u8::from(r == s)) - p[s])).collect();
                let mut v = 0.0;
                for (a, &(_, ia)) in frees.iter().enumerate() {
                    for (b, &(_, ib)) in frees.iter().enumerate() {
                        v += grad[a] * cov[ia][ib] * grad[b];
                    }
                }
                v.max(0.0).sqrt()
            })
            .collect()
    };
    let pi_se = delta(&model.pi, &pi_index);
    let rho_se =
        (0..levels.len()).map(|j| (0..k).map(|c| delta(&model.rho[j][c], &rho_index[j][c])).collect()).collect();
    (pi_se, rho_se)
}

/// One candidate of a [`LatentClassSelection`].
#[derive(Debug, Clone, PartialEq)]
pub struct LatentClassCandidate {
    /// Number of classes.
    pub k: usize,
    /// The fit, or why it failed.
    pub fit: std::result::Result<LatentClassFit, String>,
}

impl LatentClassCandidate {
    /// BIC of the fit (`None` if it failed).
    #[must_use]
    pub fn bic(&self) -> Option<f64> {
        self.fit.as_ref().ok().map(LatentClassFit::bic)
    }
}

/// Every number of classes tried, and the one with the smallest BIC.
#[derive(Debug, Clone, PartialEq)]
pub struct LatentClassSelection {
    /// Candidates in increasing number of classes.
    pub candidates: Vec<LatentClassCandidate>,
    /// Index of the chosen candidate.
    pub best: usize,
}

impl LatentClassSelection {
    /// The chosen model.
    #[must_use]
    pub fn chosen(&self) -> &LatentClassFit {
        match &self.candidates[self.best].fit {
            Ok(fit) => fit,
            Err(_) => unreachable!("the chosen candidate was fitted"),
        }
    }
}

/// Fit every number of classes in `ks` with the settings of `base` (in
/// parallel, deterministically) and choose the smallest BIC.
///
/// # Errors
/// No numbers of classes, invalid data (as in [`fit_latent_classes`]), or
/// every fit failing.
pub fn select_latent_classes(
    responses: &[Vec<Option<usize>>],
    levels: &[usize],
    ks: &[usize],
    base: &LatentClassOptions,
) -> Result<LatentClassSelection> {
    use rayon::prelude::*;
    check_data(responses, levels)?;
    if ks.is_empty() {
        return Err(invalid("choose at least one number of classes"));
    }
    let candidates: Vec<LatentClassCandidate> = ks
        .par_iter()
        .map(|&k| LatentClassCandidate {
            k,
            fit: fit_latent_classes(responses, levels, &LatentClassOptions { k, ..*base }).map_err(|e| e.to_string()),
        })
        .collect();
    let best = candidates
        .iter()
        .enumerate()
        .filter_map(|(i, c)| c.bic().map(|b| (i, b)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
        .ok_or_else(|| {
            let why = candidates.iter().find_map(|c| c.fit.as_ref().err().cloned()).unwrap_or_default();
            invalid(format!("no latent class model could be fitted: {why}"))
        })?;
    Ok(LatentClassSelection { candidates, best })
}

#[cfg(test)]
mod tests;
