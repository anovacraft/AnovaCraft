//! Bayesian linear regression and analysis of variance with g-priors:
//! a Bayes factor against the intercept-only model for every model that
//! respects effect heredity, posterior model probabilities, inclusion
//! probabilities per effect and model-averaged estimates.
//!
//! **Prior** (Liang, Paulo, Molina, Clyde & Berger 2008, *JASA* 103;
//! Rouder, Morey, Speckman & Province 2012, *J. Math. Psych.* 56). The
//! intercept has a flat prior and `σ²` the Jeffreys prior `1/σ²`; with the
//! response and every design column centered, the coefficients `β` of a
//! model have `β │ σ², g ~ N(0, σ² V(g))`, block-diagonal over effects:
//!
//! * the continuous columns of a model together form one block with the
//!   Zellner–Siow prior `V = g n (XᵀX)⁻¹`, `g ~ Inv-Gamma(½, r²/2)` (a
//!   multivariate Cauchy with scale `r`; `r = √2⁄4` is the "medium" scale);
//!   or the hyper-g prior `V = g (XᵀX)⁻¹`, `π(g) = (a − 2)/2 · (1 + g)^{−a/2}`;
//! * each categorical effect (a factor, or an interaction involving one) is
//!   its own block, coded by orthonormal contrasts `Q` (`QᵀQ = I`,
//!   `Qᵀ1 = 0`, products of the factors' codes for interactions) with
//!   `V = g I`, `g ~ Inv-Gamma(½, r²/2)` (`r = ½` "medium"): the level
//!   effects `Qβ` have covariance `gσ²(I − J/a)`.
//!
//! **Bayes factor.** Integrating `β` and `σ²`,
//! `BF(g) = |I + V XᵀX|^{−1/2} (1 − yᵀX (XᵀX + V⁻¹)⁻¹ Xᵀy / yᵀy)^{−(n−1)/2}`,
//! and `BF₁₀ = ∫ BF(g) π(g) dg`. With one `g` (the usual regression case)
//! this is a one-dimensional integral in `ln g` by adaptive quadrature
//! (exact to quadrature tolerance); with several, an importance-sampling
//! estimate with a multivariate t proposal centred on the posterior mode of
//! `ln g` (Rouder et al. 2012, §6), with its Monte Carlo error.
//!
//! **Posterior.** Given `g`, `β │ g, y ~ t_{n−1}(m, S/(n − 1) M⁻¹)` with
//! `M = XᵀX + V⁻¹`, `m = M⁻¹Xᵀy` and `S = yᵀy − yᵀX m`. Estimates and
//! credible intervals are mixtures of these over `g` (quadrature nodes or
//! importance draws) and over models (with a point mass at zero for models
//! without the effect), weighted by posterior model probability.
//!
//! **Models.** Every subset of the effects in which an interaction or power
//! appears only with all of its lower-order effects (heredity), enumerated
//! up to 2¹⁴ candidate subsets; beyond that, Markov chain Monte Carlo model
//! composition (Madigan & York 1995, *Int. Stat. Rev.* 63) adding or
//! dropping one effect at a time. Model prior: uniform over models, or
//! beta-binomial(1, 1) over the number of effects (Scott & Berger 2010,
//! *Ann. Stat.* 38).

use crate::integrate::{integrate_log, peak_range};
use crate::util::{cholesky, cholesky_inverse, cholesky_log_det, cholesky_solve};
use rayon::prelude::*;
use stat_core::rng::Rng;
use stat_core::special::ln_gamma;
use stat_core::{ContinuousDistribution, Result, StatError, StudentsT};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// What kind of effect a block of columns is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectKind {
    /// Continuous columns (one shared Zellner–Siow or hyper-g block).
    Continuous,
    /// A categorical effect coded by orthonormal contrasts (its own `g`).
    Categorical,
}

/// One effect of the full model.
#[derive(Debug, Clone, PartialEq)]
pub struct Effect {
    /// Name, e.g. `"Dose"` or `"Dose × Supplement"`.
    pub name: String,
    /// Kind.
    pub kind: EffectKind,
    /// Design columns (`n` values each), uncentered.
    pub columns: Vec<Vec<f64>>,
    /// Indices of the effects this one requires (its lower-order effects).
    pub parents: Vec<usize>,
    /// Reported parameters: a label and coefficients over this effect's
    /// columns (identity rows for slopes; rows of `Q` for level effects).
    pub parameters: Vec<(String, Vec<f64>)>,
}

/// Prior on the coefficients of continuous columns.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ContinuousPrior {
    /// Zellner–Siow (multivariate Cauchy) with scale `r`.
    ZellnerSiow {
        /// Scale `r` (√2⁄4 ≈ 0.354 is "medium").
        r: f64,
    },
    /// Hyper-g with parameter `a` (2 < a ≤ 4; 3 is the usual choice).
    HyperG {
        /// `a`.
        a: f64,
    },
}

/// Prior probabilities of the models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelPrior {
    /// Every model equally likely.
    Uniform,
    /// Beta-binomial(1, 1) on the number of effects.
    BetaBinomial,
}

/// Settings of a Bayesian linear model analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct LinearSpec {
    /// Response (`n` values).
    pub y: Vec<f64>,
    /// Effects of the full model.
    pub effects: Vec<Effect>,
    /// Prior for continuous columns.
    pub continuous_prior: ContinuousPrior,
    /// Scale `r` of the categorical effects (½ is "medium").
    pub r_categorical: f64,
    /// Model prior.
    pub model_prior: ModelPrior,
    /// Only the full model (no model search).
    pub full_model_only: bool,
    /// Credible level.
    pub level: f64,
    /// Importance draws per model with several `g`s.
    pub draws: usize,
    /// Seed of the importance sampler and model search.
    pub seed: u64,
}

/// One model's Bayes factor and posterior probability.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelResult {
    /// Included effects (indices into [`LinearSpec::effects`]).
    pub effects: Vec<usize>,
    /// `ln BF` against the intercept-only model.
    pub ln_bf: f64,
    /// Relative Monte Carlo error of `BF` (0 for quadrature).
    pub error: f64,
    /// Prior probability.
    pub prior: f64,
    /// Posterior probability.
    pub posterior: f64,
    /// Ordinary least-squares R².
    pub r_squared: f64,
}

/// One effect's inclusion probabilities.
#[derive(Debug, Clone, PartialEq)]
pub struct Inclusion {
    /// Prior probability that the effect is in the model.
    pub prior: f64,
    /// Posterior probability.
    pub posterior: f64,
    /// Inclusion Bayes factor: posterior odds over prior odds, with each
    /// probability held at most `1 − ε` so that an effect in every probable
    /// model gets a large but finite factor (about 4.5·10¹⁵ times the prior
    /// odds' reciprocal) rather than ∞.
    pub bf: f64,
}

/// A model-averaged estimate.
#[derive(Debug, Clone, PartialEq)]
pub struct Estimate {
    /// Effect index.
    pub effect: usize,
    /// Parameter label.
    pub label: String,
    /// Posterior mean.
    pub mean: f64,
    /// Posterior standard deviation.
    pub sd: f64,
    /// Equal-tailed credible interval.
    pub interval: (f64, f64),
    /// Posterior probability that the parameter is zero (its effect left out).
    pub p_zero: f64,
}

/// Result of a Bayesian linear model analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct LinearResult {
    /// Models, most probable first.
    pub models: Vec<ModelResult>,
    /// Inclusion probabilities, parallel to the effects.
    pub inclusion: Vec<Inclusion>,
    /// Model-averaged estimates of every reported parameter.
    pub averaged: Vec<Estimate>,
    /// Estimates within the most probable model.
    pub best: Vec<Estimate>,
    /// Whether the models were enumerated (otherwise sampled by MC³).
    pub enumerated: bool,
    /// Number of observations.
    pub n: usize,
    /// Distinct models visited by the search when sampled.
    pub visited: usize,
}

/// Orthonormal contrasts for a factor with `levels` levels: a
/// `levels × (levels − 1)` matrix (row per level) with orthonormal columns
/// orthogonal to the constant (Helmert contrasts, normalized).
#[must_use]
pub fn orthonormal_contrasts(levels: usize) -> Vec<Vec<f64>> {
    let mut q = vec![vec![0.0; levels.saturating_sub(1)]; levels];
    for j in 1..levels {
        let k = j as f64;
        let norm = (k * (k + 1.0)).sqrt();
        for (i, row) in q.iter_mut().enumerate().take(j) {
            let _ = i;
            row[j - 1] = 1.0 / norm;
        }
        q[j][j - 1] = -k / norm;
    }
    q
}

/// Prepared data of one model: centered design and its cross-products.
struct Prepared {
    n: usize,
    p: usize,
    /// Row-major `XᵀX` of centered columns.
    xtx: Vec<f64>,
    xty: Vec<f64>,
    yty: f64,
    /// For each `g`: the columns it governs and the kind.
    blocks: Vec<(Vec<usize>, Block)>,
    /// `(effect, columns)` for each included effect.
    owners: Vec<(usize, Vec<usize>)>,
    r_squared: f64,
}

#[derive(Debug, Clone, Copy)]
enum Block {
    /// `V = g n (XᵀX)⁻¹` on these columns, `g ~ IG(½, r²/2)`.
    ZellnerSiow(f64),
    /// `V = g (XᵀX)⁻¹`, hyper-g(a).
    HyperG(f64),
    /// `V = g I`, `g ~ IG(½, r²/2)`.
    Identity(f64),
}

fn ln_inv_gamma_half(u: f64, r: f64) -> f64 {
    // g = eᵘ, IG(½, r²/2) density times the Jacobian g.
    let g = u.exp();
    0.5 * (0.5 * r * r).ln() - ln_gamma(0.5) - 0.5 * u - 0.5 * r * r / g
}

impl Prepared {
    fn new(spec: &LinearSpec, include: &[usize]) -> Result<Self> {
        let n = spec.y.len();
        let ym = spec.y.iter().sum::<f64>() / n as f64;
        let yc: Vec<f64> = spec.y.iter().map(|v| v - ym).collect();
        let mut cols: Vec<Vec<f64>> = Vec::new();
        let mut owners = Vec::new();
        let mut cont = Vec::new();
        let mut blocks = Vec::new();
        for &e in include {
            let eff = &spec.effects[e];
            let mut idx = Vec::new();
            for c in &eff.columns {
                let m = c.iter().sum::<f64>() / n as f64;
                cols.push(c.iter().map(|v| v - m).collect());
                idx.push(cols.len() - 1);
            }
            match eff.kind {
                EffectKind::Continuous => cont.extend(idx.iter().copied()),
                EffectKind::Categorical => blocks.push((idx.clone(), Block::Identity(spec.r_categorical))),
            }
            owners.push((e, idx));
        }
        if !cont.is_empty() {
            let b = match spec.continuous_prior {
                ContinuousPrior::ZellnerSiow { r } => Block::ZellnerSiow(r),
                ContinuousPrior::HyperG { a } => Block::HyperG(a),
            };
            blocks.insert(0, (cont, b));
        }
        let p = cols.len();
        let mut xtx = vec![0.0; p * p];
        for i in 0..p {
            for j in 0..=i {
                let v = stat_core::summation::dot(&cols[i], &cols[j]);
                xtx[i * p + j] = v;
                xtx[j * p + i] = v;
            }
        }
        let xty: Vec<f64> = cols.iter().map(|c| stat_core::summation::dot(c, &yc)).collect();
        let yty = stat_core::summation::dot(&yc, &yc);
        if yty.is_nan() || yty <= 0.0 {
            return Err(invalid("the response does not vary"));
        }
        let r_squared = if p == 0 {
            0.0
        } else {
            let l = cholesky(&xtx, p).ok_or_else(|| {
                invalid("the model's columns are collinear (a predictor is constant or duplicates others)")
            })?;
            let b = cholesky_solve(&l, p, &xty);
            stat_core::summation::dot(&b, &xty) / yty
        };
        Ok(Self { n, p, xtx, xty, yty, blocks, owners, r_squared })
    }

    /// `M = XᵀX + V(g)⁻¹` and `ln |V(g)|` for `u = ln g` per block.
    fn system(&self, u: &[f64]) -> Option<(Vec<f64>, f64)> {
        let p = self.p;
        let mut m = self.xtx.clone();
        let mut ln_det_v = 0.0;
        for ((idx, block), &ui) in self.blocks.iter().zip(u) {
            let k = idx.len();
            match *block {
                Block::Identity(_) => {
                    for &i in idx {
                        m[i * p + i] += (-ui).exp();
                    }
                    ln_det_v += k as f64 * ui;
                }
                Block::ZellnerSiow(_) | Block::HyperG(_) => {
                    // V⁻¹ = XᵀX_block / s, s = g n (ZS) or g (hyper-g).
                    let ln_s = if matches!(block, Block::ZellnerSiow(_)) { ui + (self.n as f64).ln() } else { ui };
                    let s = ln_s.exp();
                    let mut sub = vec![0.0; k * k];
                    for (a, &i) in idx.iter().enumerate() {
                        for (b, &j) in idx.iter().enumerate() {
                            sub[a * k + b] = self.xtx[i * p + j];
                            m[i * p + j] += self.xtx[i * p + j] / s;
                        }
                    }
                    let l = cholesky(&sub, k)?;
                    ln_det_v += k as f64 * ln_s - cholesky_log_det(&l, k);
                }
            }
        }
        Some((m, ln_det_v))
    }

    /// `ln BF(g)` plus the log prior density of `u = ln g` (with Jacobian).
    fn ln_integrand(&self, u: &[f64]) -> f64 {
        let Some((m, ln_det_v)) = self.system(u) else {
            return f64::NEG_INFINITY;
        };
        let Some(l) = cholesky(&m, self.p) else {
            return f64::NEG_INFINITY;
        };
        let sol = cholesky_solve(&l, self.p, &self.xty);
        let fit = stat_core::summation::dot(&sol, &self.xty);
        let ratio = (1.0 - fit / self.yty).max(1e-300);
        let ln_bf = -0.5 * (ln_det_v + cholesky_log_det(&l, self.p)) - 0.5 * (self.n as f64 - 1.0) * ratio.ln();
        let ln_prior: f64 = self
            .blocks
            .iter()
            .zip(u)
            .map(|((_, b), &ui)| match *b {
                Block::Identity(r) | Block::ZellnerSiow(r) => ln_inv_gamma_half(ui, r),
                Block::HyperG(a) => ((a - 2.0) / 2.0).ln() - 0.5 * a * ui.exp().ln_1p() + ui,
            })
            .sum();
        ln_bf + ln_prior
    }

    /// Posterior mean, `S/(n−1)` and `M⁻¹` given `u`.
    fn conditional(&self, u: &[f64]) -> Option<(Vec<f64>, f64, Vec<f64>)> {
        let (m, _) = self.system(u)?;
        let l = cholesky(&m, self.p)?;
        let mean = cholesky_solve(&l, self.p, &self.xty);
        let s = (self.yty - stat_core::summation::dot(&mean, &self.xty)).max(0.0);
        Some((mean, s / (self.n as f64 - 1.0), cholesky_inverse(&l, self.p)))
    }
}

/// Weighted nodes `(weight, u)` representing the posterior of `ln g`, and
/// the `ln` marginal likelihood with its relative error.
struct Marginal {
    ln_bf: f64,
    error: f64,
    nodes: Vec<(f64, Vec<f64>)>,
}

fn marginal(prep: &Prepared, draws: usize, seed: u64) -> Result<Marginal> {
    let d = prep.blocks.len();
    if d == 0 {
        return Ok(Marginal { ln_bf: 0.0, error: 0.0, nodes: vec![(1.0, Vec::new())] });
    }
    if d == 1 {
        let f = |u: f64| prep.ln_integrand(&[u]);
        let ln_bf = integrate_log(f, 40.0);
        let (a, b, peak) = peak_range(&f, 40.0).ok_or_else(|| invalid("the Bayes factor integral failed"))?;
        let k = 120;
        let nodes: Vec<(f64, Vec<f64>)> = (0..=k)
            .map(|i| {
                let u = a + (b - a) * i as f64 / k as f64;
                let w = (f(u) - peak).exp() * if i == 0 || i == k { 0.5 } else { 1.0 };
                (w, vec![u])
            })
            .collect();
        let total: f64 = nodes.iter().map(|n| n.0).sum();
        return Ok(Marginal { ln_bf, error: 0.0, nodes: nodes.into_iter().map(|(w, u)| (w / total, u)).collect() });
    }
    // Mode of the log integrand by coordinate-wise golden-section passes.
    let f = |u: &[f64]| prep.ln_integrand(u);
    let mut mode = vec![0.0; d];
    for _ in 0..30 {
        let before = mode.clone();
        for j in 0..d {
            let g = |x: f64| {
                let mut v = mode.clone();
                v[j] = x;
                f(&v)
            };
            let (lo, hi, _) = peak_range(&g, 30.0).unwrap_or((-5.0, 5.0, 0.0));
            let (mut a, mut b) = (lo, hi);
            let phi = 0.5 * (5f64.sqrt() - 1.0);
            for _ in 0..80 {
                let x1 = b - phi * (b - a);
                let x2 = a + phi * (b - a);
                if g(x1) >= g(x2) {
                    b = x2;
                } else {
                    a = x1;
                }
            }
            mode[j] = 0.5 * (a + b);
        }
        if before.iter().zip(&mode).all(|(a, b)| (a - b).abs() < 1e-6) {
            break;
        }
    }
    // Hessian by central differences; proposal covariance = its inverse.
    let h = 1e-3;
    let f0 = f(&mode);
    let mut hess = vec![0.0; d * d];
    for i in 0..d {
        for j in 0..=i {
            let at = |si: f64, sj: f64| {
                let mut v = mode.clone();
                v[i] += si * h;
                v[j] += sj * h;
                f(&v)
            };
            let v = if i == j {
                (at(1.0, 0.0) - 2.0 * f0 + at(-1.0, 0.0)) / (h * h)
            } else {
                (at(1.0, 1.0) - at(1.0, -1.0) - at(-1.0, 1.0) + at(-1.0, -1.0)) / (4.0 * h * h)
            };
            hess[i * d + j] = -v;
            hess[j * d + i] = -v;
        }
    }
    let cov = cholesky(&hess, d).map_or_else(
        || {
            let mut c = vec![0.0; d * d];
            for i in 0..d {
                c[i * d + i] = 1.0;
            }
            c
        },
        |l| cholesky_inverse(&l, d),
    );
    // Inflate the proposal a little and use t with 4 df for heavy tails.
    let mut cov = cov;
    cov.iter_mut().for_each(|v| *v *= 1.5);
    let lc = cholesky(&cov, d).ok_or_else(|| invalid("the importance-sampling proposal failed"))?;
    let nu = 4.0;
    let ln_det = cholesky_log_det(&lc, d) * 0.5;
    let ln_t_norm =
        ln_gamma(0.5 * (nu + d as f64)) - ln_gamma(0.5 * nu) - 0.5 * d as f64 * (nu * std::f64::consts::PI).ln();
    let draws = draws.max(200);
    let chunk = 256;
    let samples: Vec<(f64, Vec<f64>)> = (0..draws.div_ceil(chunk))
        .into_par_iter()
        .flat_map_iter(|c| {
            let mut rng = Rng::new(seed ^ (0x9E37_79B9_7F4A_7C15_u64.wrapping_mul(c as u64 + 1)));
            let count = chunk.min(draws - c * chunk);
            (0..count)
                .map(|_| {
                    let z: Vec<f64> = (0..d).map(|_| rng.normal()).collect();
                    let w = gamma_draw(&mut rng, 0.5 * nu) * 2.0 / nu;
                    let scale = 1.0 / w.sqrt();
                    let mut u = mode.clone();
                    let mut q = 0.0;
                    for i in 0..d {
                        let mut s = 0.0;
                        for k in 0..=i {
                            s += lc[i * d + k] * z[k];
                        }
                        u[i] += s * scale;
                    }
                    for zi in &z {
                        q += zi * zi * scale * scale;
                    }
                    let ln_q = ln_t_norm - ln_det - 0.5 * (nu + d as f64) * (q / nu).ln_1p();
                    (f(&u) - ln_q, u)
                })
                .collect::<Vec<_>>()
        })
        .collect();
    let peak = samples.iter().map(|s| s.0).filter(|v| v.is_finite()).fold(f64::NEG_INFINITY, f64::max);
    if !peak.is_finite() {
        return Err(invalid("the Bayes factor could not be estimated"));
    }
    let w: Vec<f64> = samples.iter().map(|s| if s.0.is_finite() { (s.0 - peak).exp() } else { 0.0 }).collect();
    let n = w.len() as f64;
    let mean = w.iter().sum::<f64>() / n;
    let var = w.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / (n - 1.0);
    let error = (var / n).sqrt() / mean;
    let total: f64 = w.iter().sum();
    // Keep a subsample of nodes for the posterior mixtures.
    let keep = 2000.min(samples.len());
    let step = samples.len() / keep;
    let mut nodes: Vec<(f64, Vec<f64>)> =
        samples.into_iter().zip(&w).step_by(step.max(1)).map(|((_, u), &wi)| (wi, u)).collect();
    let nt: f64 = nodes.iter().map(|x| x.0).sum();
    nodes.iter_mut().for_each(|x| x.0 /= nt);
    let _ = total;
    Ok(Marginal { ln_bf: peak + mean.ln(), error, nodes })
}

/// A Gamma(shape, 1) draw (Marsaglia & Tsang 2000; boosted for shape < 1).
fn gamma_draw(rng: &mut Rng, shape: f64) -> f64 {
    if shape < 1.0 {
        let u = rng.next_f64().max(1e-300);
        return gamma_draw(rng, shape + 1.0) * u.powf(1.0 / shape);
    }
    let d = shape - 1.0 / 3.0;
    let c = 1.0 / (9.0 * d).sqrt();
    loop {
        let x = rng.normal();
        let v = (1.0 + c * x).powi(3);
        if v <= 0.0 {
            continue;
        }
        let u = rng.next_f64();
        if u.ln() < 0.5 * x * x + d - d * v + d * v.ln() {
            return d * v;
        }
    }
}

/// `true` when every included effect's parents are included.
fn hereditary(spec: &LinearSpec, include: &[usize]) -> bool {
    include.iter().all(|&e| spec.effects[e].parents.iter().all(|p| include.contains(p)))
}

/// Number of hereditary models of each size `0..=k`, so that the
/// beta-binomial(1, 1) prior gives each size equal mass whether the models
/// are enumerated or sampled by MC³.
///
/// Effects that are some effect's parent ("inner" effects, e.g. main
/// effects) are enumerated over their hereditary subsets `T`; every other
/// effect whose parents all lie in `T` may then be added freely, so `T`
/// contributes `z^|T| (1 + z)^a(T)` to the size-generating polynomial. With
/// more than 20 inner effects the count falls back to all subsets `C(k, s)`.
fn hereditary_size_counts(effects: &[Effect]) -> Vec<f64> {
    let k = effects.len();
    let inner: Vec<usize> = (0..k).filter(|&e| effects.iter().any(|x| x.parents.contains(&e))).collect();
    let mut counts = vec![0.0; k + 1];
    let ln_choose = |a: usize, b: usize| {
        use stat_core::special::ln_gamma;
        ln_gamma(a as f64 + 1.0) - ln_gamma(b as f64 + 1.0) - ln_gamma((a - b) as f64 + 1.0)
    };
    if inner.len() > 20 {
        for (s, c) in counts.iter_mut().enumerate() {
            *c = ln_choose(k, s).exp();
        }
        return counts;
    }
    let bit = |e: usize| inner.iter().position(|&x| x == e).map_or(0u32, |i| 1 << i);
    let parent_mask = |e: usize| effects[e].parents.iter().fold(0u32, |m, &p| m | bit(p));
    let inner_parents: Vec<u32> = inner.iter().map(|&e| parent_mask(e)).collect();
    let leaf_parents: Vec<u32> = (0..k).filter(|e| !inner.contains(e)).map(parent_mask).collect();
    // Pascal's triangle up to the number of other effects.
    let mut pascal: Vec<Vec<f64>> = vec![vec![1.0]];
    for a in 1..=leaf_parents.len() {
        let prev = &pascal[a - 1];
        pascal.push((0..=a).map(|j| if j == 0 || j == a { 1.0 } else { prev[j - 1] + prev[j] }).collect());
    }
    for t in 0u32..(1u32 << inner.len()) {
        let closed = inner_parents.iter().enumerate().all(|(i, &pm)| t & (1 << i) == 0 || pm & !t == 0);
        if !closed {
            continue;
        }
        let a = leaf_parents.iter().filter(|&&pm| pm & !t == 0).count();
        let size = t.count_ones() as usize;
        for (j, c) in pascal[a].iter().enumerate() {
            counts[size + j] += c;
        }
    }
    counts
}

/// Posterior odds over prior odds of inclusion. A probability that rounds to 1
/// is held just below it (`1 − 1e-300` would be 1 in binary64), so the
/// factor is large but finite.
fn inclusion_bf(prior: f64, posterior: f64) -> f64 {
    let odds = |p: f64| {
        let p = p.min(1.0 - f64::EPSILON);
        p / (1.0 - p)
    };
    odds(posterior) / odds(prior)
}

fn model_prior(spec: &LinearSpec, size: usize, counts: &[f64]) -> f64 {
    match spec.model_prior {
        ModelPrior::Uniform => 1.0,
        // Beta-binomial(1,1): each size equally likely, split among the
        // hereditary models of that size.
        ModelPrior::BetaBinomial => 1.0 / counts[size].max(1.0),
    }
}

/// A mixture component: weight, t location, scale and df (or a point mass
/// at zero when `scale = 0`).
type Component = (f64, f64, f64);

fn mixture_summary(components: &[Component], df: f64, level: f64) -> (f64, f64, (f64, f64)) {
    let t = StudentsT::new(df.max(1.0)).ok();
    let var_t = if df > 2.0 { df / (df - 2.0) } else { f64::NAN };
    let mean: f64 = components.iter().map(|c| c.0 * c.1).sum();
    let m2: f64 = components.iter().map(|c| c.0 * (c.1 * c.1 + c.2 * c.2 * var_t)).sum();
    let sd = (m2 - mean * mean).max(0.0).sqrt();
    // Quantiles from at most 500 components: systematic resampling of a
    // large importance-sample mixture (the moments above use every one).
    let thinned: Vec<Component>;
    let quantile_components = if components.len() > 500 {
        let m = 500;
        let total: f64 = components.iter().map(|c| c.0).sum();
        let mut out = Vec::with_capacity(m);
        let (mut acc, mut k) = (0.0, 0);
        for j in 0..m {
            let target = (j as f64 + 0.5) / m as f64 * total;
            while k + 1 < components.len() && acc + components[k].0 < target {
                acc += components[k].0;
                k += 1;
            }
            out.push((1.0 / m as f64, components[k].1, components[k].2));
        }
        thinned = out;
        &thinned[..]
    } else {
        components
    };
    let cdf = |x: f64| -> f64 {
        quantile_components
            .iter()
            .map(|&(w, m, s)| {
                if s == 0.0 {
                    if x >= m { w } else { 0.0 }
                } else {
                    w * t.as_ref().map_or(f64::NAN, |t| t.cdf((x - m) / s).unwrap_or(f64::NAN))
                }
            })
            .sum()
    };
    let span = components.iter().map(|c| c.1.abs() + 40.0 * c.2).fold(1e-12, f64::max);
    let quant = |p: f64| {
        let (mut lo, mut hi) = (-span, span);
        for _ in 0..80 {
            let mid = 0.5 * (lo + hi);
            if cdf(mid) < p {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    };
    let tail = 0.5 * (1.0 - level);
    (mean, sd, (quant(tail), quant(1.0 - tail)))
}

/// A model's conditional posterior of its coefficients at one quadrature
/// node of g: node weight, mean, σ² scale and the inverse of the precision.
type NodeConditional = (f64, Vec<f64>, f64, Vec<f64>);

/// Fit every model (or search the model space) and summarize.
///
/// # Errors
/// No effects, effects with the wrong number of rows, a constant response,
/// collinear columns, too few observations, or invalid prior scales.
pub fn bayesian_linear_model(spec: &LinearSpec) -> Result<LinearResult> {
    fit_models(spec, 14)
}

/// [`bayesian_linear_model`], enumerating every model when there are at most
/// `enumerate_up_to` effects and sampling them by MC³ otherwise.
fn fit_models(spec: &LinearSpec, enumerate_up_to: usize) -> Result<LinearResult> {
    let n = spec.y.len();
    let k = spec.effects.len();
    if k == 0 {
        return Err(invalid("add at least one effect"));
    }
    if spec.y.iter().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "response" });
    }
    if spec.effects.iter().any(|e| e.columns.iter().any(|c| c.len() != n) || e.columns.is_empty()) {
        return Err(invalid("every effect needs one or more columns with a value for every row"));
    }
    let p_full: usize = spec.effects.iter().map(|e| e.columns.len()).sum();
    if n < p_full + 3 {
        return Err(StatError::InsufficientData { required: p_full + 3, found: n });
    }
    if spec.r_categorical.is_nan() || spec.r_categorical <= 0.0 {
        return Err(invalid("the prior scale of categorical effects must be positive"));
    }
    match spec.continuous_prior {
        ContinuousPrior::ZellnerSiow { r } if r.is_nan() || r <= 0.0 => {
            return Err(invalid("the prior scale of continuous effects must be positive"));
        }
        ContinuousPrior::HyperG { a } if !(a > 2.0 && a <= 4.0) => {
            return Err(invalid("the hyper-g parameter a must lie in (2, 4]"));
        }
        _ => {}
    }
    if !(spec.level > 0.0 && spec.level < 1.0) {
        return Err(invalid("the credible level must lie between 0 and 1"));
    }
    // Candidate models.
    let all: Vec<usize> = (0..k).collect();
    let enumerated = spec.full_model_only || k <= enumerate_up_to;
    // One definition of the model prior for both branches.
    let size_counts = hereditary_size_counts(&spec.effects);
    let mut candidates: Vec<Vec<usize>> = if spec.full_model_only {
        vec![Vec::new(), all.clone()]
    } else if enumerated {
        (0u32..(1u32 << k))
            .map(|mask| (0..k).filter(|&e| mask & (1 << e) != 0).collect::<Vec<_>>())
            .filter(|m| hereditary(spec, m))
            .collect()
    } else {
        Vec::new()
    };
    let fit = |m: &Vec<usize>, i: usize| -> Result<(ModelResult, Marginal, Prepared)> {
        let prep = Prepared::new(spec, m)?;
        let mg = marginal(&prep, spec.draws, spec.seed.wrapping_add(i as u64 * 7919))?;
        Ok((
            ModelResult {
                effects: m.clone(),
                ln_bf: mg.ln_bf,
                error: mg.error,
                prior: 0.0,
                posterior: 0.0,
                r_squared: prep.r_squared,
            },
            mg,
            prep,
        ))
    };
    let mut visited = 0;
    let mut fitted: Vec<(ModelResult, Marginal, Prepared)>;
    if enumerated {
        fitted = candidates.par_iter().enumerate().map(|(i, m)| fit(m, i)).collect::<Result<Vec<_>>>()?;
    } else {
        // MC³ over hereditary models.
        let mut rng = Rng::new(spec.seed);
        let mut cache: std::collections::HashMap<Vec<usize>, f64> = std::collections::HashMap::new();
        let mut current: Vec<usize> = Vec::new();
        let ln_score = |m: &Vec<usize>, cache: &mut std::collections::HashMap<Vec<usize>, f64>| -> Result<f64> {
            if let Some(&v) = cache.get(m) {
                return Ok(v);
            }
            let prep = Prepared::new(spec, m)?;
            let v = marginal(&prep, spec.draws.min(2000), spec.seed)?.ln_bf;
            cache.insert(m.clone(), v);
            Ok(v)
        };
        let size_prior = |m: &[usize]| model_prior(spec, m.len(), &size_counts).ln();
        let mut cur = ln_score(&current, &mut cache)? + size_prior(&current);
        for _ in 0..20_000 {
            let e = rng.below(k);
            let mut prop = current.clone();
            if let Some(pos) = prop.iter().position(|&x| x == e) {
                prop.remove(pos);
            } else {
                prop.push(e);
                prop.sort_unstable();
            }
            if !hereditary(spec, &prop) {
                continue;
            }
            let s = ln_score(&prop, &mut cache)? + size_prior(&prop);
            if (rng.next_f64().max(1e-300)).ln() < s - cur {
                current = prop;
                cur = s;
            }
        }
        visited = cache.len();
        candidates = cache.into_keys().collect();
        candidates.sort();
        fitted = candidates.par_iter().enumerate().map(|(i, m)| fit(m, i)).collect::<Result<Vec<_>>>()?;
    }
    // Model priors and posteriors.
    for (m, _, _) in &mut fitted {
        m.prior = model_prior(spec, m.effects.len(), &size_counts);
    }
    let prior_total: f64 = fitted.iter().map(|f| f.0.prior).sum();
    let peak = fitted.iter().map(|f| f.0.ln_bf + f.0.prior.ln()).fold(f64::NEG_INFINITY, f64::max);
    let mut post: Vec<f64> = fitted.iter().map(|f| (f.0.ln_bf + f.0.prior.ln() - peak).exp()).collect();
    let pt: f64 = post.iter().sum();
    post.iter_mut().for_each(|p| *p /= pt);
    for ((m, _, _), &p) in fitted.iter_mut().zip(&post) {
        m.prior /= prior_total;
        m.posterior = p;
    }
    let inclusion: Vec<Inclusion> = (0..k)
        .map(|e| {
            let prior: f64 = fitted.iter().filter(|f| f.0.effects.contains(&e)).map(|f| f.0.prior).sum();
            let posterior: f64 = fitted.iter().filter(|f| f.0.effects.contains(&e)).map(|f| f.0.posterior).sum();
            Inclusion { prior, posterior, bf: inclusion_bf(prior, posterior) }
        })
        .collect();
    // Estimates.
    let df = n as f64 - 1.0;
    let mut order: Vec<usize> = (0..fitted.len()).collect();
    order.sort_by(|&a, &b| fitted[b].0.posterior.total_cmp(&fitted[a].0.posterior));
    let best_index = order[0];
    // Each model's conditional posteriors at its nodes, computed once.
    let wanted = |i: usize| fitted[i].0.posterior > 1e-6 || i == best_index;
    let conditionals: Vec<Vec<NodeConditional>> = (0..fitted.len())
        .into_par_iter()
        .map(|i| {
            if !wanted(i) {
                return Vec::new();
            }
            let (_, mg, prep) = &fitted[i];
            mg.nodes.iter().filter_map(|(w, u)| prep.conditional(u).map(|(m, s2, minv)| (*w, m, s2, minv))).collect()
        })
        .collect();
    let components = |i: usize, e: usize, coef: &[f64]| -> Vec<Component> {
        let prep = &fitted[i].2;
        let Some((_, cols)) = prep.owners.iter().find(|o| o.0 == e) else {
            return vec![(1.0, 0.0, 0.0)];
        };
        conditionals[i]
            .iter()
            .map(|(w, mean, s2, minv)| {
                let m: f64 = cols.iter().zip(coef).map(|(&c, &a)| a * mean[c]).sum();
                let mut v = 0.0;
                for (&ci, &ai) in cols.iter().zip(coef) {
                    for (&cj, &aj) in cols.iter().zip(coef) {
                        v += ai * aj * minv[ci * prep.p + cj];
                    }
                }
                (*w, m, (s2 * v).max(0.0).sqrt())
            })
            .collect()
    };
    let mut averaged = Vec::new();
    let mut best = Vec::new();
    for (e, eff) in spec.effects.iter().enumerate() {
        for (label, coef) in &eff.parameters {
            let mut comps = Vec::new();
            for &i in order.iter().take_while(|&&i| fitted[i].0.posterior > 1e-6) {
                let w = fitted[i].0.posterior;
                comps.extend(components(i, e, coef).into_iter().map(|c| (c.0 * w, c.1, c.2)));
            }
            let total: f64 = comps.iter().map(|c| c.0).sum();
            comps.iter_mut().for_each(|c| c.0 /= total);
            let (mean, sd, interval) = mixture_summary(&comps, df, spec.level);
            averaged.push(Estimate {
                effect: e,
                label: label.clone(),
                mean,
                sd,
                interval,
                p_zero: 1.0 - inclusion[e].posterior,
            });
            if fitted[best_index].0.effects.contains(&e) {
                let c = components(best_index, e, coef);
                let (mean, sd, interval) = mixture_summary(&c, df, spec.level);
                best.push(Estimate { effect: e, label: label.clone(), mean, sd, interval, p_zero: 0.0 });
            }
        }
    }
    let models = order.iter().map(|&i| fitted[i].0.clone()).collect();
    Ok(LinearResult { models, inclusion, averaged, best, enumerated, n, visited })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrasts_are_orthonormal() {
        for a in 2..6 {
            let q = orthonormal_contrasts(a);
            for j in 0..a - 1 {
                let s: f64 = (0..a).map(|i| q[i][j]).sum();
                assert!(s.abs() < 1e-14);
                for k in 0..a - 1 {
                    let d: f64 = (0..a).map(|i| q[i][j] * q[i][k]).sum();
                    assert!((d - if j == k { 1.0 } else { 0.0 }).abs() < 1e-14);
                }
            }
        }
    }

    #[test]
    fn single_g_matches_zellner_siow_formula() {
        // One continuous predictor: BF = ∫ (1+gn)^{(n−2)/2} (1+gn(1−R²))^{−(n−1)/2} π(g) dg.
        let x: Vec<f64> = (0..15).map(|i| f64::from(i) * 0.7).collect();
        let y: Vec<f64> = x.iter().enumerate().map(|(i, v)| 0.3 * v + ((i * 7 % 5) as f64 - 2.0) * 0.4).collect();
        let spec = LinearSpec {
            y: y.clone(),
            effects: vec![Effect {
                name: "x".into(),
                kind: EffectKind::Continuous,
                columns: vec![x.clone()],
                parents: vec![],
                parameters: vec![("x".into(), vec![1.0])],
            }],
            continuous_prior: ContinuousPrior::ZellnerSiow { r: 0.354 },
            r_categorical: 0.5,
            model_prior: ModelPrior::Uniform,
            full_model_only: false,
            level: 0.95,
            draws: 1000,
            seed: 1,
        };
        let r = bayesian_linear_model(&spec).unwrap();
        let m = r.models.iter().find(|m| m.effects == [0]).unwrap();
        let n = 15.0;
        let r2 = m.r_squared;
        let direct = integrate_log(
            |u| {
                let g = u.exp();
                0.5 * (n - 2.0) * (g * n).ln_1p() - 0.5 * (n - 1.0) * (g * n * (1.0 - r2)).ln_1p()
                    + ln_inv_gamma_half(u, 0.354)
            },
            40.0,
        );
        assert!((m.ln_bf - direct).abs() < 1e-8, "{} vs {direct}", m.ln_bf);
        assert!((r.inclusion[0].posterior - m.posterior).abs() < 1e-12);
    }

    fn effect(name: &str, column: Vec<f64>, parents: Vec<usize>) -> Effect {
        Effect {
            name: name.into(),
            kind: EffectKind::Continuous,
            columns: vec![column],
            parents,
            parameters: vec![(name.into(), vec![1.0])],
        }
    }

    /// Hereditary models of A, B, A×B: {∅}, {A}, {B}, {A, B}, {A, B, A×B} — one
    /// of size 0, two of size 1, one of size 2 and one of size 3 (the
    /// all-subsets count C(3, 2) = 3 was wrong). Five effects with two
    /// interactions over three main effects are checked against brute force.
    #[test]
    fn hereditary_counts_match_enumeration() {
        let e = |p: Vec<usize>| effect("e", vec![0.0], p);
        assert_eq!(hereditary_size_counts(&[e(vec![]), e(vec![]), e(vec![0, 1])]), [1.0, 2.0, 1.0, 1.0]);
        let effects = [e(vec![]), e(vec![]), e(vec![]), e(vec![0, 1]), e(vec![1, 2]), e(vec![0, 1, 2, 3, 4])];
        let spec_effects = effects.to_vec();
        let mut brute = vec![0.0; 7];
        for mask in 0u32..64 {
            let m: Vec<usize> = (0..6).filter(|&j| mask & (1 << j) != 0).collect();
            if m.iter().all(|&j| spec_effects[j].parents.iter().all(|p| m.contains(p))) {
                brute[m.len()] += 1.0;
            }
        }
        assert_eq!(hereditary_size_counts(&effects), brute);
    }

    /// The beta-binomial prior and the inclusion probabilities agree whether
    /// the models are enumerated or sampled by MC³ (forced here with three
    /// effects), and a posterior of 1 gives a finite inclusion Bayes factor.
    #[test]
    fn mc3_uses_the_enumerated_model_prior() {
        let n = 40;
        let a: Vec<f64> = (0..n).map(|i| f64::from(i % 5) - 2.0).collect();
        let b: Vec<f64> = (0..n).map(|i| f64::from((i * 7) % 6) - 2.5).collect();
        let ab: Vec<f64> = a.iter().zip(&b).map(|(x, y)| x * y).collect();
        let y: Vec<f64> =
            (0..n).map(|i| 3.0 * a[i as usize] + 0.15 * b[i as usize] + 0.3 * f64::from((i * 13) % 7 - 3)).collect();
        let spec = LinearSpec {
            y,
            effects: vec![effect("A", a, vec![]), effect("B", b, vec![]), effect("A×B", ab, vec![0, 1])],
            continuous_prior: ContinuousPrior::ZellnerSiow { r: 0.354 },
            r_categorical: 0.5,
            model_prior: ModelPrior::BetaBinomial,
            full_model_only: false,
            level: 0.95,
            draws: 1000,
            seed: 3,
        };
        let full = fit_models(&spec, 14).unwrap();
        let sampled = fit_models(&spec, 2).unwrap();
        assert!(full.enumerated && !sampled.enumerated);
        assert_eq!(sampled.models.len(), 5, "MC³ visits every hereditary model");
        for m in &full.models {
            let s = sampled.models.iter().find(|x| x.effects == m.effects).unwrap();
            assert!((s.prior - m.prior).abs() < 1e-12, "{:?}: {} vs {}", m.effects, s.prior, m.prior);
        }
        for (f, s) in full.inclusion.iter().zip(&sampled.inclusion) {
            assert!((f.posterior - s.posterior).abs() < 0.02, "{} vs {}", f.posterior, s.posterior);
        }
        // A dominates: its posterior inclusion is 1 to rounding.
        assert!(full.inclusion[0].posterior > 1.0 - 1e-12);
        assert!(full.inclusion[0].bf.is_finite() && full.inclusion[0].bf > 1e12, "{}", full.inclusion[0].bf);
        // A posterior of exactly 1 (or a prior of 1) stays finite.
        let bf = inclusion_bf(0.6, 1.0);
        assert!(bf.is_finite() && (bf / ((1.0 / f64::EPSILON - 1.0) / 1.5) - 1.0).abs() < 1e-12, "{bf}");
        assert_eq!(inclusion_bf(1.0, 1.0), 1.0);
    }
}
