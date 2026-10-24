//! Gaussian-process regression (Rasmussen & Williams, *Gaussian Processes
//! for Machine Learning*, 2006, ch. 2 and 5; Santner, Williams & Notz, *The
//! Design and Analysis of Computer Experiments*, 2nd ed., 2018, ch. 3).
//!
//! **Model.** The response, standardized to mean 0 and (population)
//! standard deviation 1, is a zero-mean Gaussian process plus independent
//! noise:
//!
//! ```text
//! y = f(x) + ε,   f ~ GP(0, σ_f² r(x, x′)),   ε ~ N(0, σ_n²)
//! ```
//!
//! with an anisotropic ("automatic relevance") correlation over the scaled
//! distances `d_j = (x_j − x′_j)² / ℓ_j²`, one length scale `ℓ_j` per input:
//!
//! * squared exponential: `r = exp(−½ Σ d_j)`;
//! * Matérn 5/2: with `ρ = √(Σ d_j)`, `r = (1 + √5 ρ + 5ρ²/3) exp(−√5 ρ)`.
//!
//! The noise variance `σ_n²` (the *nugget*) is estimated, or fixed (a small
//! fixed nugget makes the fit interpolate a deterministic simulator). A
//! jitter of 10⁻¹⁰ is always added to the diagonal.
//!
//! **Fitting.** The hyperparameters `θ = (ln σ_f², ln ℓ_1, …, ln ℓ_d,
//! ln σ_n²)` maximize the log marginal likelihood (R&W eq. 2.30)
//!
//! ```text
//! ln p(y | θ) = −½ yᵀ K⁻¹ y − ½ ln |K| − (n/2) ln 2π
//! ```
//!
//! whose gradient is `½ tr((ααᵀ − K⁻¹) ∂K/∂θ_k)` with `α = K⁻¹ y` (R&W eq.
//! 5.9). Each start runs a projected BFGS search inside the bounds
//! `σ_f² ∈ [10⁻³, 10³]`, `ℓ_j ∈ [10⁻³, 10³]`, `σ_n² ∈ [10⁻⁸, 10]`; the first
//! start is `σ_f² = 1`, `ℓ_j = ½`, `σ_n² = 0.01`, the others are spread over
//! a Latin hypercube of typical values. The best start is kept. The
//! formulation (standardized response, these bounds) is the one of
//! scikit-learn's `GaussianProcessRegressor(normalize_y=True)` with a
//! constant × correlation + white-noise kernel, which the tests check
//! against. Inputs are best scaled to the unit cube first (the app does).
//!
//! **Prediction.** At `x*`, with `k*` the covariances with the training
//! inputs and `K = LLᵀ`, the mean is `k*ᵀ α` and the variance of the latent
//! `f(x*)` is `σ_f² − |L⁻¹ k*|²` (R&W eq. 2.25–2.26), returned in the
//! response's units; a new observation's variance adds `σ_n²`.
//! Leave-one-out predictions come in closed form from `K⁻¹` (R&W eq. 5.12):
//! `μ₋ᵢ = yᵢ − [K⁻¹y]ᵢ / [K⁻¹]ᵢᵢ`, `σ²₋ᵢ = 1 / [K⁻¹]ᵢᵢ`.
//!
//! **Failure modes.** Fewer than two rows, inputs of differing lengths,
//! non-finite values, more than 2,000 rows, or a covariance matrix that is
//! not positive definite at every start.

use rayon::prelude::*;
use stat_core::optimize::latin_hypercube;
use stat_core::rng::Rng;
use stat_core::{Result, StatError};

/// Most training rows (the fit costs O(n³) per likelihood evaluation).
pub const MAX_ROWS: usize = 2000;
/// Diagonal jitter added to every covariance matrix.
pub const JITTER: f64 = 1e-10;
/// Bounds of `σ_f²` (standardized response).
pub const SIGNAL_BOUNDS: (f64, f64) = (1e-3, 1e3);
/// Bounds of each length scale.
pub const LENGTH_BOUNDS: (f64, f64) = (1e-3, 1e3);
/// Bounds of an estimated nugget `σ_n²` (standardized response).
pub const NUGGET_BOUNDS: (f64, f64) = (1e-8, 10.0);

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// The correlation function of a Gaussian process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CovarianceKernel {
    /// `exp(−½ Σ d_j)`: very smooth (infinitely differentiable) surfaces.
    SquaredExponential,
    /// Matérn with ν = 5/2: twice differentiable, less prone to overshoot.
    #[default]
    Matern52,
}

impl CovarianceKernel {
    /// Both kernels.
    pub const ALL: [Self; 2] = [Self::Matern52, Self::SquaredExponential];

    /// Key in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::SquaredExponential => "squared-exponential",
            Self::Matern52 => "matern52",
        }
    }

    /// The kernel with this key.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.key() == key)
    }

    /// Name for reports.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::SquaredExponential => "Squared exponential",
            Self::Matern52 => "Matérn 5/2",
        }
    }

    /// Correlation `r` at summed scaled distance `s = Σ d_j`, and the factor
    /// `g` with `∂r/∂ ln ℓ_j = g · d_j`.
    fn correlation(self, s: f64) -> (f64, f64) {
        match self {
            Self::SquaredExponential => {
                let r = (-0.5 * s).exp();
                (r, r)
            }
            Self::Matern52 => {
                let rho5 = (5.0 * s).sqrt();
                let e = (-rho5).exp();
                ((1.0 + rho5 + 5.0 * s / 3.0) * e, 5.0 / 3.0 * (1.0 + rho5) * e)
            }
        }
    }
}

/// How the nugget (noise variance) is set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Nugget {
    /// Estimated with the other hyperparameters.
    Estimated,
    /// Fixed at this variance, in units of the standardized response.
    Fixed(f64),
}

/// Settings of a fit.
#[derive(Debug, Clone, PartialEq)]
pub struct GpOptions {
    /// Correlation function.
    pub kernel: CovarianceKernel,
    /// Nugget.
    pub nugget: Nugget,
    /// Starts of the likelihood search (at least one).
    pub starts: usize,
    /// Seed of the starts after the first.
    pub seed: u64,
    /// Most BFGS iterations per start.
    pub max_iter: usize,
}

impl Default for GpOptions {
    /// Matérn 5/2, nugget estimated, ten starts, seed 1, 500 iterations.
    fn default() -> Self {
        Self { kernel: CovarianceKernel::Matern52, nugget: Nugget::Estimated, starts: 10, seed: 1, max_iter: 500 }
    }
}

/// Hyperparameters on their natural scale (standardized response).
#[derive(Debug, Clone, PartialEq)]
pub struct GpHyperparameters {
    /// Signal variance `σ_f²`.
    pub signal_variance: f64,
    /// One length scale per input.
    pub length_scales: Vec<f64>,
    /// Nugget `σ_n²`.
    pub noise_variance: f64,
}

impl GpHyperparameters {
    /// `θ = (ln σ_f², ln ℓ…, ln σ_n²)`, the nugget included only when
    /// `with_noise`.
    #[must_use]
    pub fn theta(&self, with_noise: bool) -> Vec<f64> {
        let mut t = vec![self.signal_variance.ln()];
        t.extend(self.length_scales.iter().map(|l| l.ln()));
        if with_noise {
            t.push(self.noise_variance.ln());
        }
        t
    }

    /// The hyperparameters of `θ` (with `noise` when it holds no nugget).
    #[must_use]
    pub fn from_theta(theta: &[f64], dims: usize, noise: Option<f64>) -> Self {
        Self {
            signal_variance: theta[0].exp(),
            length_scales: theta[1..=dims].iter().map(|t| t.exp()).collect(),
            noise_variance: noise.unwrap_or_else(|| theta[dims + 1].exp()),
        }
    }
}

/// A fitted Gaussian process.
#[derive(Debug, Clone, PartialEq)]
pub struct GaussianProcess {
    /// Correlation function.
    pub kernel: CovarianceKernel,
    /// Hyperparameters (standardized response).
    pub hyper: GpHyperparameters,
    /// `true` when the nugget was estimated.
    pub nugget_estimated: bool,
    /// Training inputs.
    pub x: Vec<Vec<f64>>,
    /// Training responses (original units).
    pub y: Vec<f64>,
    /// Mean used to standardize the response.
    pub y_mean: f64,
    /// Standard deviation used to standardize the response.
    pub y_scale: f64,
    /// Log marginal likelihood of the standardized response.
    pub log_likelihood: f64,
    /// Final log likelihood of each start (−∞ where a start failed).
    pub start_log_likelihoods: Vec<f64>,
    /// Index of the start kept.
    pub best_start: usize,
    /// BFGS iterations of the kept start.
    pub iterations: usize,
    /// `false` when the kept start stopped at the iteration limit.
    pub converged: bool,
    /// Lower Cholesky factor of `K`, row-major `n × n`.
    chol: Vec<f64>,
    /// `α = K⁻¹ y` (standardized).
    alpha: Vec<f64>,
}

/// Lower Cholesky factor of the row-major `n × n` matrix `a`, or `None`.
fn cholesky(a: &[f64], n: usize) -> Option<Vec<f64>> {
    let mut l = vec![0.0; n * n];
    for i in 0..n {
        for j in 0..=i {
            let mut s = a[i * n + j];
            for k in 0..j {
                s -= l[i * n + k] * l[j * n + k];
            }
            if i == j {
                if !(s > 0.0 && s.is_finite()) {
                    return None;
                }
                l[i * n + i] = s.sqrt();
            } else {
                l[i * n + j] = s / l[j * n + j];
            }
        }
    }
    Some(l)
}

/// Solve `L z = b` (forward substitution).
fn forward(l: &[f64], n: usize, b: &[f64]) -> Vec<f64> {
    let mut z = b.to_vec();
    for i in 0..n {
        let mut s = z[i];
        for k in 0..i {
            s -= l[i * n + k] * z[k];
        }
        z[i] = s / l[i * n + i];
    }
    z
}

/// Solve `Lᵀ z = b` (back substitution).
fn backward(l: &[f64], n: usize, b: &[f64]) -> Vec<f64> {
    let mut z = b.to_vec();
    for i in (0..n).rev() {
        let mut s = z[i];
        for k in i + 1..n {
            s -= l[k * n + i] * z[k];
        }
        z[i] = s / l[i * n + i];
    }
    z
}

/// `K⁻¹` (row-major) from its Cholesky factor.
fn inverse(l: &[f64], n: usize) -> Vec<f64> {
    // L⁻¹ column by column, then K⁻¹ = L⁻ᵀ L⁻¹.
    let mut linv = vec![0.0; n * n];
    for j in 0..n {
        let mut e = vec![0.0; n];
        e[j] = 1.0;
        let z = forward(l, n, &e);
        for i in 0..n {
            linv[i * n + j] = z[i];
        }
    }
    let mut out = vec![0.0; n * n];
    for i in 0..n {
        for j in 0..=i {
            let mut s = 0.0;
            for k in i.max(j)..n {
                s += linv[k * n + i] * linv[k * n + j];
            }
            out[i * n + j] = s;
            out[j * n + i] = s;
        }
    }
    out
}

fn scaled_distance(a: &[f64], b: &[f64], inv_ls2: &[f64]) -> f64 {
    a.iter().zip(b).zip(inv_ls2).map(|((u, v), w)| (u - v) * (u - v) * w).sum()
}

/// The covariance matrix `K` (row-major) for hyperparameters `h`.
fn covariance(x: &[Vec<f64>], kernel: CovarianceKernel, h: &GpHyperparameters) -> Vec<f64> {
    let n = x.len();
    let inv: Vec<f64> = h.length_scales.iter().map(|l| 1.0 / (l * l)).collect();
    let mut k = vec![0.0; n * n];
    for i in 0..n {
        k[i * n + i] = h.signal_variance + h.noise_variance + JITTER;
        for j in 0..i {
            let v = h.signal_variance * kernel.correlation(scaled_distance(&x[i], &x[j], &inv)).0;
            k[i * n + j] = v;
            k[j * n + i] = v;
        }
    }
    k
}

/// Log marginal likelihood of the standardized response `y` at `θ` (with
/// the nugget fixed at `noise` when `θ` holds none) and its gradient with
/// respect to `θ`, or `None` when `K` is not positive definite.
#[must_use]
pub fn log_marginal_likelihood(
    x: &[Vec<f64>],
    y: &[f64],
    kernel: CovarianceKernel,
    theta: &[f64],
    noise: Option<f64>,
) -> Option<(f64, Vec<f64>)> {
    let n = x.len();
    let d = x.first().map_or(0, Vec::len);
    let h = GpHyperparameters::from_theta(theta, d, noise);
    let k = covariance(x, kernel, &h);
    let l = cholesky(&k, n)?;
    let alpha = backward(&l, n, &forward(&l, n, y));
    let fit: f64 = y.iter().zip(&alpha).map(|(a, b)| a * b).sum();
    let log_det: f64 = (0..n).map(|i| l[i * n + i].ln()).sum();
    let value = -0.5 * fit - log_det - 0.5 * n as f64 * (2.0 * std::f64::consts::PI).ln();
    let kinv = inverse(&l, n);
    // W = ααᵀ − K⁻¹; gradient_k = ½ Σ_ij W_ij ∂K_ij/∂θ_k.
    let inv: Vec<f64> = h.length_scales.iter().map(|l| 1.0 / (l * l)).collect();
    let mut grad = vec![0.0; theta.len()];
    for i in 0..n {
        let w = alpha[i] * alpha[i] - kinv[i * n + i];
        grad[0] += 0.5 * w * h.signal_variance;
        if noise.is_none() {
            grad[d + 1] += 0.5 * w * h.noise_variance;
        }
        for j in 0..i {
            // Off-diagonal pairs count twice.
            let w = alpha[i] * alpha[j] - kinv[i * n + j];
            let s = scaled_distance(&x[i], &x[j], &inv);
            let (r, g) = kernel.correlation(s);
            grad[0] += w * h.signal_variance * r;
            for m in 0..d {
                let dm = (x[i][m] - x[j][m]).powi(2) * inv[m];
                grad[1 + m] += w * h.signal_variance * g * dm;
            }
        }
    }
    Some((value, grad))
}

/// An objective's value and gradient at a point (`None`: +∞).
type ValueAndGradient<'a> = dyn Fn(&[f64]) -> Option<(f64, Vec<f64>)> + 'a;

/// One search's end: the point, its value, the iterations and whether it
/// converged.
type SearchEnd = (Vec<f64>, f64, usize, bool);

/// Projected BFGS: minimize `f` (value and gradient; `None` = +∞) inside
/// the box. Returns the point, its value, the iterations and whether it
/// converged.
fn minimize_box(
    f: &ValueAndGradient<'_>,
    x0: &[f64],
    lower: &[f64],
    upper: &[f64],
    max_iter: usize,
) -> Option<SearchEnd> {
    let p = x0.len();
    let clamp = |x: &mut [f64]| {
        for ((v, l), u) in x.iter_mut().zip(lower).zip(upper) {
            *v = v.clamp(*l, *u);
        }
    };
    let mut x = x0.to_vec();
    clamp(&mut x);
    let (mut fx, mut g) = f(&x)?;
    let mut h = identity(p);
    let free_of = |x: &[f64], g: &[f64]| -> Vec<bool> {
        (0..p).map(|i| !((x[i] <= lower[i] && g[i] > 0.0) || (x[i] >= upper[i] && g[i] < 0.0))).collect()
    };
    let mut free = free_of(&x, &g);
    for iter in 0..max_iter {
        let pg: Vec<f64> = (0..p).map(|i| if free[i] { g[i] } else { 0.0 }).collect();
        if pg.iter().all(|v| v.abs() < 1e-6) {
            return Some((x, fx, iter, true));
        }
        let mut dir: Vec<f64> = (0..p)
            .map(|i| if free[i] { -(0..p).filter(|&j| free[j]).map(|j| h[i * p + j] * g[j]).sum::<f64>() } else { 0.0 })
            .collect();
        if dir.iter().zip(&pg).map(|(a, b)| a * b).sum::<f64>() >= 0.0 {
            h = identity(p);
            dir = pg.iter().map(|v| -v).collect();
        }
        // Backtracking along the projected path.
        let mut t = 1.0;
        let mut accepted = None;
        for _ in 0..40 {
            let mut xn: Vec<f64> = x.iter().zip(&dir).map(|(a, b)| a + t * b).collect();
            clamp(&mut xn);
            let decrease: f64 = g.iter().zip(xn.iter().zip(&x)).map(|(gi, (a, b))| gi * (a - b)).sum();
            if let Some((fv, gv)) = f(&xn)
                && fv <= fx + 1e-4 * decrease
            {
                accepted = Some((xn, fv, gv));
                break;
            }
            t *= 0.5;
        }
        let Some((xn, fv, gv)) = accepted else { return Some((x, fx, iter, true)) };
        let s: Vec<f64> = xn.iter().zip(&x).map(|(a, b)| a - b).collect();
        let yv: Vec<f64> = gv.iter().zip(&g).map(|(a, b)| a - b).collect();
        let small = (fx - fv) <= 1e-12 * fx.abs().max(fv.abs()).max(1.0);
        let new_free = free_of(&xn, &gv);
        let sy: f64 = s.iter().zip(&yv).map(|(a, b)| a * b).sum();
        if new_free != free {
            h = identity(p);
        } else if sy > 1e-10 {
            // H ← (I − ρ s yᵀ) H (I − ρ y sᵀ) + ρ s sᵀ.
            let rho = 1.0 / sy;
            let hy: Vec<f64> = (0..p).map(|i| (0..p).map(|j| h[i * p + j] * yv[j]).sum()).collect();
            let yhy: f64 = yv.iter().zip(&hy).map(|(a, b)| a * b).sum();
            for i in 0..p {
                for j in 0..p {
                    h[i * p + j] += -rho * (s[i] * hy[j] + hy[i] * s[j]) + (rho * rho * yhy + rho) * s[i] * s[j];
                }
            }
        }
        (x, fx, g, free) = (xn, fv, gv, new_free);
        if small {
            return Some((x, fx, iter + 1, true));
        }
    }
    Some((x, fx, max_iter, false))
}

fn identity(p: usize) -> Vec<f64> {
    let mut h = vec![0.0; p * p];
    for i in 0..p {
        h[i * p + i] = 1.0;
    }
    h
}

fn check_data(x: &[Vec<f64>], y: &[f64]) -> Result<usize> {
    if x.len() != y.len() {
        return Err(invalid("the inputs and responses differ in length"));
    }
    if x.len() < 2 {
        return Err(invalid("a Gaussian process needs at least two rows"));
    }
    if x.len() > MAX_ROWS {
        return Err(invalid(format!("a Gaussian process is limited to {MAX_ROWS} rows")));
    }
    let d = x[0].len();
    if d == 0 || x.iter().any(|r| r.len() != d) {
        return Err(invalid("every row needs the same, non-zero number of inputs"));
    }
    if x.iter().flatten().chain(y).any(|v| !v.is_finite()) {
        return Err(invalid("the inputs and responses must be finite"));
    }
    Ok(d)
}

/// Fit a Gaussian process to `x` (rows of inputs, best scaled to the unit
/// cube) and `y` by maximum likelihood (see the module docs).
///
/// # Errors
/// See the module docs.
pub fn fit_gaussian_process(x: &[Vec<f64>], y: &[f64], options: &GpOptions) -> Result<GaussianProcess> {
    let d = check_data(x, y)?;
    if options.starts == 0 {
        return Err(invalid("use at least one start"));
    }
    if let Nugget::Fixed(v) = options.nugget
        && !(v >= 0.0 && v.is_finite())
    {
        return Err(invalid("the nugget must not be negative"));
    }
    let (y_mean, y_scale) = standardization(y);
    let ys: Vec<f64> = y.iter().map(|v| (v - y_mean) / y_scale).collect();
    let noise = match options.nugget {
        Nugget::Estimated => None,
        Nugget::Fixed(v) => Some(v),
    };
    let p = d + 1 + usize::from(noise.is_none());
    let mut lower = vec![SIGNAL_BOUNDS.0.ln()];
    let mut upper = vec![SIGNAL_BOUNDS.1.ln()];
    lower.extend(std::iter::repeat_n(LENGTH_BOUNDS.0.ln(), d));
    upper.extend(std::iter::repeat_n(LENGTH_BOUNDS.1.ln(), d));
    if noise.is_none() {
        lower.push(NUGGET_BOUNDS.0.ln());
        upper.push(NUGGET_BOUNDS.1.ln());
    }
    // First start: σ_f² = 1, ℓ = ½, σ_n² = 0.01; then a Latin hypercube of
    // σ_f² ∈ [0.1, 10], ℓ ∈ [0.05, 5], σ_n² ∈ [10⁻⁶, 0.5].
    let mut starts = vec![{
        let mut t = vec![0.0];
        t.extend(std::iter::repeat_n(0.5f64.ln(), d));
        if noise.is_none() {
            t.push(0.01f64.ln());
        }
        t
    }];
    if options.starts > 1 {
        let mut rng = Rng::new(options.seed);
        let span = |u: f64, lo: f64, hi: f64| lo.ln() + u * (hi.ln() - lo.ln());
        for u in latin_hypercube(options.starts - 1, p, &mut rng) {
            let mut t = vec![span(u[0], 0.1, 10.0)];
            t.extend((0..d).map(|j| span(u[1 + j], 0.05, 5.0)));
            if noise.is_none() {
                t.push(span(u[d + 1], 1e-6, 0.5));
            }
            starts.push(t);
        }
    }
    let objective = |t: &[f64]| -> Option<(f64, Vec<f64>)> {
        let (v, g) = log_marginal_likelihood(x, &ys, options.kernel, t, noise)?;
        v.is_finite().then(|| (-v, g.into_iter().map(|a| -a).collect()))
    };
    let runs: Vec<Option<SearchEnd>> =
        starts.par_iter().map(|t0| minimize_box(&objective, t0, &lower, &upper, options.max_iter)).collect();
    let start_log_likelihoods: Vec<f64> = runs.iter().map(|r| r.as_ref().map_or(f64::NEG_INFINITY, |r| -r.1)).collect();
    let best = (0..runs.len())
        .filter(|&i| runs[i].is_some())
        .max_by(|&a, &b| start_log_likelihoods[a].total_cmp(&start_log_likelihoods[b]))
        .ok_or_else(|| invalid("the covariance matrix is not positive definite at any start"))?;
    let (theta, _, iterations, converged) = runs[best].clone().unwrap_or_default();
    let hyper = GpHyperparameters::from_theta(&theta, d, noise);
    let mut gp = GaussianProcess::with_hyperparameters(x, y, options.kernel, hyper, (y_mean, y_scale))?;
    gp.nugget_estimated = noise.is_none();
    gp.start_log_likelihoods = start_log_likelihoods;
    gp.best_start = best;
    gp.iterations = iterations;
    gp.converged = converged;
    Ok(gp)
}

/// Mean and population standard deviation (1 when the responses are equal).
fn standardization(y: &[f64]) -> (f64, f64) {
    let n = y.len() as f64;
    let mean = y.iter().sum::<f64>() / n;
    let sd = (y.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n).sqrt();
    (mean, if sd > 0.0 { sd } else { 1.0 })
}

impl GaussianProcess {
    /// The process with given hyperparameters (no search), the response
    /// standardized by `(mean, scale)`.
    ///
    /// # Errors
    /// Invalid data, or `K` not positive definite.
    pub fn with_hyperparameters(
        x: &[Vec<f64>],
        y: &[f64],
        kernel: CovarianceKernel,
        hyper: GpHyperparameters,
        (y_mean, y_scale): (f64, f64),
    ) -> Result<Self> {
        let d = check_data(x, y)?;
        if hyper.length_scales.len() != d {
            return Err(invalid("one length scale is needed per input"));
        }
        let n = x.len();
        let ys: Vec<f64> = y.iter().map(|v| (v - y_mean) / y_scale).collect();
        let k = covariance(x, kernel, &hyper);
        let chol = cholesky(&k, n).ok_or_else(|| invalid("the covariance matrix is not positive definite"))?;
        let alpha = backward(&chol, n, &forward(&chol, n, &ys));
        let fit: f64 = ys.iter().zip(&alpha).map(|(a, b)| a * b).sum();
        let log_det: f64 = (0..n).map(|i| chol[i * n + i].ln()).sum();
        let log_likelihood = -0.5 * fit - log_det - 0.5 * n as f64 * (2.0 * std::f64::consts::PI).ln();
        Ok(Self {
            kernel,
            hyper,
            nugget_estimated: false,
            x: x.to_vec(),
            y: y.to_vec(),
            y_mean,
            y_scale,
            log_likelihood,
            start_log_likelihoods: vec![log_likelihood],
            best_start: 0,
            iterations: 0,
            converged: true,
            chol,
            alpha,
        })
    }

    /// The same hyperparameters and standardization, conditioned on the
    /// training data plus `(x, y)` (the constant-liar step of a batch).
    ///
    /// # Errors
    /// As [`Self::with_hyperparameters`].
    pub fn condition_on(&self, x: &[Vec<f64>], y: &[f64]) -> Result<Self> {
        let mut xs = self.x.clone();
        xs.extend_from_slice(x);
        let mut ys = self.y.clone();
        ys.extend_from_slice(y);
        let mut gp =
            Self::with_hyperparameters(&xs, &ys, self.kernel, self.hyper.clone(), (self.y_mean, self.y_scale))?;
        gp.nugget_estimated = self.nugget_estimated;
        Ok(gp)
    }

    /// Number of inputs.
    #[must_use]
    pub fn dims(&self) -> usize {
        self.hyper.length_scales.len()
    }

    /// Log marginal likelihood in the response's own units
    /// (`ln p(y) = ln p(y_std) − n ln s`).
    #[must_use]
    pub fn log_likelihood_original(&self) -> f64 {
        self.log_likelihood - self.y.len() as f64 * self.y_scale.ln()
    }

    /// Standard deviation of the noise in the response's units.
    #[must_use]
    pub fn noise_sd(&self) -> f64 {
        self.hyper.noise_variance.sqrt() * self.y_scale
    }

    /// Predicted mean and standard deviation of the latent mean `f(x)`, in
    /// the response's units.
    ///
    /// # Panics
    /// If `x` has the wrong number of inputs.
    #[must_use]
    pub fn predict(&self, x: &[f64]) -> (f64, f64) {
        assert_eq!(x.len(), self.dims(), "wrong number of inputs");
        let n = self.x.len();
        let inv: Vec<f64> = self.hyper.length_scales.iter().map(|l| 1.0 / (l * l)).collect();
        let ks: Vec<f64> = self
            .x
            .iter()
            .map(|xi| self.hyper.signal_variance * self.kernel.correlation(scaled_distance(xi, x, &inv)).0)
            .collect();
        let mean: f64 = ks.iter().zip(&self.alpha).map(|(a, b)| a * b).sum();
        let v = forward(&self.chol, n, &ks);
        let var = (self.hyper.signal_variance - v.iter().map(|a| a * a).sum::<f64>()).max(0.0);
        (self.y_mean + self.y_scale * mean, self.y_scale * var.sqrt())
    }

    /// Leave-one-out predicted mean and standard deviation (of a new
    /// observation) of each training row, in closed form.
    #[must_use]
    pub fn leave_one_out(&self) -> Vec<(f64, f64)> {
        let n = self.x.len();
        let kinv = inverse(&self.chol, n);
        (0..n)
            .map(|i| {
                let d = kinv[i * n + i];
                let ys = (self.y[i] - self.y_mean) / self.y_scale;
                let mean = ys - self.alpha[i] / d;
                (self.y_mean + self.y_scale * mean, self.y_scale / d.sqrt())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> (Vec<Vec<f64>>, Vec<f64>) {
        let mut rng = Rng::new(3);
        let x: Vec<Vec<f64>> = (0..20).map(|_| vec![rng.next_f64(), rng.next_f64()]).collect();
        let y = x.iter().map(|r| (4.0 * r[0]).sin() + r[1] * r[1] + 0.02 * rng.normal()).collect();
        (x, y)
    }

    #[test]
    fn gradient_matches_central_differences() {
        let (x, y) = data();
        let (m, s) = standardization(&y);
        let ys: Vec<f64> = y.iter().map(|v| (v - m) / s).collect();
        for kernel in CovarianceKernel::ALL {
            for noise in [None, Some(1e-4)] {
                let mut t = vec![0.3, -1.0, -0.4];
                if noise.is_none() {
                    t.push(-3.0);
                }
                let (_, g) = log_marginal_likelihood(&x, &ys, kernel, &t, noise).unwrap();
                for k in 0..t.len() {
                    let h = 1e-6;
                    let mut a = t.clone();
                    a[k] += h;
                    let mut b = t.clone();
                    b[k] -= h;
                    let fa = log_marginal_likelihood(&x, &ys, kernel, &a, noise).unwrap().0;
                    let fb = log_marginal_likelihood(&x, &ys, kernel, &b, noise).unwrap().0;
                    let num = (fa - fb) / (2.0 * h);
                    assert!((num - g[k]).abs() < 1e-5 * (1.0 + g[k].abs()), "{kernel:?} {k}: {num} vs {}", g[k]);
                }
            }
        }
    }

    #[test]
    fn leave_one_out_matches_refits() {
        let (x, y) = data();
        let gp = fit_gaussian_process(&x, &y, &GpOptions { starts: 3, ..GpOptions::default() }).unwrap();
        assert!(gp.converged);
        let loo = gp.leave_one_out();
        for i in [0, 7, 19] {
            let mut xs = x.clone();
            let mut ys = y.clone();
            xs.remove(i);
            ys.remove(i);
            let g =
                GaussianProcess::with_hyperparameters(&xs, &ys, gp.kernel, gp.hyper.clone(), (gp.y_mean, gp.y_scale))
                    .unwrap();
            let (m, s) = g.predict(&x[i]);
            let s_new = (s * s + gp.noise_sd().powi(2)).sqrt();
            assert!((loo[i].0 - m).abs() < 1e-7, "{} vs {m}", loo[i].0);
            assert!((loo[i].1 - s_new).abs() < 1e-7, "{} vs {s_new}", loo[i].1);
        }
        // Conditioning on a point with a tiny nugget pins the mean there.
        let fixed =
            fit_gaussian_process(&x, &y, &GpOptions { nugget: Nugget::Fixed(1e-8), starts: 2, ..GpOptions::default() })
                .unwrap();
        let c = fixed.condition_on(&[vec![0.5, 0.5]], &[3.0]).unwrap();
        let (m, s) = c.predict(&[0.5, 0.5]);
        assert!((m - 3.0).abs() < 1e-3 && s < 1e-2, "{m} {s}");
        assert!(fit_gaussian_process(&x[..1], &y[..1], &GpOptions::default()).is_err());
        assert_eq!(CovarianceKernel::from_key("matern52"), Some(CovarianceKernel::Matern52));
    }
}
