//! Parametric survival regression: accelerated failure time (log-location–
//! scale) models with covariates, for exact, right-, left- and
//! interval-censored times.
//!
//! # Model
//! The log of the failure time is linear in the covariates with a standard
//! error distribution scaled by `σ` (Meeker & Escobar, *Statistical Methods
//! for Reliability Data*, 1998, ch. 17; Kalbfleisch & Prentice, *The
//! Statistical Analysis of Failure Time Data*, 2nd ed., 2002, §2.3; Lawless,
//! *Statistical Models and Methods for Lifetime Data*, 2nd ed., 2003, ch. 6):
//!
//! ```text
//! ln T = μ(x) + σ W,    μ(x) = β₀ + xᵀβ
//! ```
//!
//! | distribution of `T` | `W`                       | `F_W(w)`              |
//! |---------------------|---------------------------|-----------------------|
//! | Weibull             | smallest extreme value    | `1 − exp(−eʷ)`        |
//! | exponential         | as Weibull with `σ = 1`   |                       |
//! | lognormal           | standard normal           | `Φ(w)`                |
//! | log-logistic        | standard logistic         | `1/(1 + e⁻ʷ)`         |
//!
//! A covariate change `Δx` multiplies every quantile of `T` by the **time
//! ratio** (acceleration factor) `exp(Δxᵀβ)`. For the Weibull, the shape is
//! `1/σ` and the scale `exp(μ(x))`, and the model is also a proportional
//! hazards model with hazard ratio `exp(−Δxᵀβ/σ)`.
//!
//! # Likelihood
//! With `z = (ln t − μ(x))/σ`, a unit with frequency `w` contributes
//! `w[ln f_W(z) − ln σ − ln t]` when it failed at `t` (the density of `T`),
//! `w ln S_W(z_l)` when still running at `l`, `w ln F_W(z_u)` when found failed
//! by `u`, and `w ln[F_W(z_u) − F_W(z_l)]` when it failed between inspections
//! at `l` and `u`. A lower end at or below zero counts as left censoring.
//! Tail probabilities are evaluated on the log scale, and interval
//! probabilities from whichever tail is smaller, so extreme observations keep
//! their precision.
//!
//! # Estimation and inference
//! The parameters `θ = (β₀, β, ln σ)` (no `ln σ` for the exponential) are
//! estimated by Newton–Raphson with Levenberg damping and step halving, from
//! least squares on representative log times; the score is analytic and the
//! observed information `−∇²ℓ` is its central difference. The covariance is
//! the inverse of the observed information. Each coefficient has a Wald
//! interval and a **profile-likelihood** interval
//! `{c : 2[ℓ(θ̂) − max_{θⱼ = c} ℓ(θ)] ≤ χ²₁(level)}`; `σ` (and the Weibull
//! shape `1/σ`) gets intervals from those of `ln σ`.
//! [`SurvivalRegressionFit::drop_columns_test`] gives likelihood-ratio tests of
//! sets of covariates.
//!
//! # Predictions
//! The `p` quantile at `x` is `t_p = exp(μ(x) + σ w_p)` with Wald limits
//! `exp(ln t̂_p ± z·SE)` (delta method on `ln t_p`); the failure probability
//! `F(t | x) = F_W(ẑ)` has limits `F_W(ẑ ± z·SE(ẑ))` (Meeker & Escobar
//! §8.4.3, §17.3). **Cox–Snell residuals** `rᵢ = −ln S(tᵢ | xᵢ)` are, for a
//! correct model, a censored sample from the unit exponential; censoring
//! carries over (an interval `[l, u]` maps to `[r(l), r(u)]`).
//!
//! # Failure modes
//! [`StatError::InvalidArgument`] for mismatched sizes, non-finite
//! covariates, a non-positive exact failure time, no failures of any kind,
//! or collinear covariates; [`StatError::ConvergenceFailure`] when the
//! likelihood has no finite maximum (for example a covariate that separates
//! failures from survivors).

use stat_core::{Matrix, Result, StatError};

use crate::life_data::{CensorKind, LifeObservation};
use crate::support::{
    ChiSquareTest, find_crossing, normal_quantile, normal_two_sided, quadratic, spd_inverse, spd_solve,
};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

const MAX_ITERATIONS: usize = 300;
const LN_SQRT_2PI: f64 = 0.918_938_533_204_672_8;

/// The distribution of the failure time in an accelerated failure time model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AftDistribution {
    /// Weibull (smallest extreme value for log time).
    #[default]
    Weibull,
    /// Lognormal (normal for log time).
    Lognormal,
    /// Log-logistic (logistic for log time).
    LogLogistic,
    /// Exponential (Weibull with shape 1).
    Exponential,
}

impl AftDistribution {
    /// Every distribution.
    pub const ALL: [Self; 4] = [Self::Weibull, Self::Lognormal, Self::LogLogistic, Self::Exponential];

    /// Key in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Weibull => "weibull",
            Self::Lognormal => "lognormal",
            Self::LogLogistic => "loglogistic",
            Self::Exponential => "exponential",
        }
    }

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Weibull => "Weibull",
            Self::Lognormal => "Lognormal",
            Self::LogLogistic => "Log-logistic",
            Self::Exponential => "Exponential",
        }
    }

    /// The distribution with key `key`.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.key() == key)
    }

    /// `true` when `σ` is estimated (all but the exponential).
    #[must_use]
    pub fn has_scale(self) -> bool {
        self != Self::Exponential
    }

    /// `ln f_W(z)`.
    #[must_use]
    pub fn ln_pdf(self, z: f64) -> f64 {
        match self {
            Self::Weibull | Self::Exponential => z - z.exp(),
            Self::Lognormal => -0.5 * z * z - LN_SQRT_2PI,
            Self::LogLogistic => -z.abs() - 2.0 * (-z.abs()).exp().ln_1p(),
        }
    }

    /// `d ln f_W / dz`.
    fn dln_pdf(self, z: f64) -> f64 {
        match self {
            Self::Weibull | Self::Exponential => 1.0 - z.exp(),
            Self::Lognormal => -z,
            Self::LogLogistic => -(0.5 * z).tanh(),
        }
    }

    /// `ln S_W(z)`.
    #[must_use]
    pub fn ln_sf(self, z: f64) -> f64 {
        match self {
            Self::Weibull | Self::Exponential => -z.exp(),
            Self::Lognormal => ln_normal_sf(z),
            Self::LogLogistic => -softplus(z),
        }
    }

    /// `ln F_W(z)`.
    #[must_use]
    pub fn ln_cdf(self, z: f64) -> f64 {
        match self {
            Self::Weibull | Self::Exponential => {
                let e = z.exp();
                if e < 1e-8 { z + (-0.5 * e).ln_1p() } else { (-(-e).exp_m1()).ln() }
            }
            Self::Lognormal => ln_normal_sf(-z),
            Self::LogLogistic => -softplus(-z),
        }
    }

    /// `F_W(z)`.
    #[must_use]
    pub fn cdf(self, z: f64) -> f64 {
        self.ln_cdf(z).exp()
    }

    /// The `p` quantile `w_p` of `W`.
    #[must_use]
    pub fn standard_quantile(self, p: f64) -> f64 {
        match self {
            Self::Weibull | Self::Exponential => (-(-p).ln_1p()).ln(),
            Self::Lognormal => normal_quantile(p),
            Self::LogLogistic => (p / (1.0 - p)).ln(),
        }
    }
}

fn softplus(z: f64) -> f64 {
    if z > 0.0 { z + (-z).exp().ln_1p() } else { z.exp().ln_1p() }
}

/// `ln(1 − Φ(z))` without underflow.
fn ln_normal_sf(z: f64) -> f64 {
    if z < 30.0 {
        (0.5 * stat_core::special::erfc(z / std::f64::consts::SQRT_2)).ln()
    } else {
        // Asymptotic series of the Mills ratio.
        let z2 = z * z;
        -0.5 * z2 - LN_SQRT_2PI - z.ln() + (-1.0 / z2 + 3.0 / (z2 * z2) - 15.0 / (z2 * z2 * z2)).ln_1p()
    }
}

/// One estimated parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct RegressionParameter {
    /// Name.
    pub name: String,
    /// Estimate.
    pub estimate: f64,
    /// Standard error.
    pub std_error: f64,
    /// Wald `z` (NaN for the scale).
    pub z: f64,
    /// Two-sided p-value of `z` (NaN for the scale).
    pub p_value: f64,
    /// Wald interval.
    pub wald: (f64, f64),
    /// Profile-likelihood interval (`None` when not computed; an infinite end
    /// when the likelihood does not fall far enough).
    pub profile: Option<(f64, f64)>,
}

/// A prediction with Wald limits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LimitedPrediction {
    /// Estimate.
    pub estimate: f64,
    /// Standard error on the scale the limits are formed (`ln t_p`, or `z`).
    pub std_error: f64,
    /// Lower limit.
    pub lower: f64,
    /// Upper limit.
    pub upper: f64,
}

/// Settings of a survival regression fit.
#[derive(Debug, Clone, PartialEq)]
pub struct SurvivalRegressionOptions {
    /// Distribution of the failure time.
    pub distribution: AftDistribution,
    /// Covariate names (default `x1`, `x2`, …).
    pub names: Vec<String>,
    /// Confidence level.
    pub level: f64,
    /// Compute profile-likelihood intervals.
    pub profile_intervals: bool,
}

impl Default for SurvivalRegressionOptions {
    fn default() -> Self {
        Self { distribution: AftDistribution::Weibull, names: Vec::new(), level: 0.95, profile_intervals: true }
    }
}

/// The likelihood over a fixed dataset.
#[derive(Debug, Clone)]
struct Likelihood {
    distribution: AftDistribution,
    obs: Vec<LifeObservation>,
    /// Design rows with a leading 1.
    rows: Vec<Vec<f64>>,
}

impl Likelihood {
    fn n_params(&self) -> usize {
        self.rows.first().map_or(1, Vec::len) + usize::from(self.distribution.has_scale())
    }

    fn sigma(&self, theta: &[f64]) -> f64 {
        if self.distribution.has_scale() { theta[theta.len() - 1].exp() } else { 1.0 }
    }

    fn mu(&self, i: usize, theta: &[f64]) -> f64 {
        self.rows[i].iter().zip(theta).map(|(x, b)| x * b).sum()
    }

    /// Log-likelihood and score at `theta` (columns in `active`, the others
    /// held at their values in `theta`).
    fn evaluate(&self, theta: &[f64]) -> (f64, Vec<f64>) {
        let d = self.distribution;
        let k = self.n_params();
        let pb = k - usize::from(d.has_scale());
        let sigma = self.sigma(theta);
        let mut ll = 0.0;
        let mut grad = vec![0.0; k];
        for (i, o) in self.obs.iter().enumerate() {
            let mu = self.mu(i, theta);
            let w = o.weight;
            let lo = if o.lower > 0.0 { o.lower.ln() } else { f64::NEG_INFINITY };
            let hi = if o.upper.is_finite() { o.upper.ln() } else { f64::INFINITY };
            let (term, dmu, ds) = match o.kind() {
                CensorKind::Exact => {
                    let z = (lo - mu) / sigma;
                    let g = d.dln_pdf(z);
                    (d.ln_pdf(z) - sigma.ln() - lo, -g / sigma, -g * z - 1.0)
                }
                _ if hi == f64::INFINITY => {
                    if lo == f64::NEG_INFINITY {
                        continue;
                    }
                    let z = (lo - mu) / sigma;
                    let h = (d.ln_pdf(z) - d.ln_sf(z)).exp();
                    (d.ln_sf(z), h / sigma, h * z)
                }
                _ if lo == f64::NEG_INFINITY => {
                    let z = (hi - mu) / sigma;
                    let r = (d.ln_pdf(z) - d.ln_cdf(z)).exp();
                    (d.ln_cdf(z), -r / sigma, -r * z)
                }
                _ => {
                    let (zl, zu) = ((lo - mu) / sigma, (hi - mu) / sigma);
                    let ln_p = if zl > 0.0 {
                        let (a, b) = (d.ln_sf(zl), d.ln_sf(zu));
                        a + (-(b - a).exp_m1()).ln()
                    } else {
                        let (a, b) = (d.ln_cdf(zu), d.ln_cdf(zl));
                        a + (-(b - a).exp_m1()).ln()
                    };
                    let fu = (d.ln_pdf(zu) - ln_p).exp();
                    let fl = (d.ln_pdf(zl) - ln_p).exp();
                    (ln_p, -(fu - fl) / sigma, -(fu * zu - fl * zl))
                }
            };
            ll += w * term;
            for (j, x) in self.rows[i].iter().enumerate() {
                grad[j] += w * dmu * x;
            }
            if d.has_scale() {
                grad[pb] += w * ds;
            }
        }
        (ll, grad)
    }

    /// `−∇²ℓ` over the parameters in `free`, by central differences of the score.
    fn information(&self, theta: &[f64], free: &[usize]) -> Vec<Vec<f64>> {
        let m = free.len();
        let mut h = vec![vec![0.0; m]; m];
        for (a, &j) in free.iter().enumerate() {
            let step = 1e-5 * (1.0 + theta[j].abs());
            let mut tp = theta.to_vec();
            tp[j] += step;
            let mut tm = theta.to_vec();
            tm[j] -= step;
            let (_, gp) = self.evaluate(&tp);
            let (_, gm) = self.evaluate(&tm);
            for (b, &k) in free.iter().enumerate() {
                h[b][a] = -(gp[k] - gm[k]) / (2.0 * step);
            }
        }
        for a in 0..m {
            for b in 0..a {
                let v = 0.5 * (h[a][b] + h[b][a]);
                h[a][b] = v;
                h[b][a] = v;
            }
        }
        h
    }

    /// Maximize over the parameters in `free` from `start` (others fixed).
    fn maximize(&self, start: &[f64], free: &[usize]) -> Result<(Vec<f64>, f64, usize)> {
        let mut theta = start.to_vec();
        let (mut ll, mut grad) = self.evaluate(&theta);
        if !ll.is_finite() {
            return Err(invalid("the starting values give an impossible likelihood"));
        }
        if free.is_empty() {
            return Ok((theta, ll, 0));
        }
        let mut lambda = 0.0_f64;
        for iteration in 1..=MAX_ITERATIONS {
            let g: Vec<f64> = free.iter().map(|&j| grad[j]).collect();
            let info = self.information(&theta, free);
            let scale = (0..free.len()).map(|a| info[a][a].abs()).fold(0.0_f64, f64::max).max(1e-12);
            // Levenberg damping until the system is positive definite.
            let mut step = None;
            let mut damp = lambda;
            for _ in 0..40 {
                let mut a = info.clone();
                for (k, row) in a.iter_mut().enumerate() {
                    row[k] += damp * scale;
                }
                if let Some(s) = spd_solve(&a, &g) {
                    step = Some(s);
                    break;
                }
                damp = if damp == 0.0 { 1e-6 } else { damp * 10.0 };
            }
            let Some(step) = step else {
                return Err(invalid("the information matrix is singular (collinear covariates?)"));
            };
            // Limit very large steps (in log-time units).
            let largest = step.iter().fold(0.0_f64, |m, s| m.max(s.abs()));
            let cap = if largest > 5.0 { 5.0 / largest } else { 1.0 };
            let mut t = cap;
            let mut accepted = None;
            for _ in 0..60 {
                let mut cand = theta.clone();
                for (k, &j) in free.iter().enumerate() {
                    cand[j] += t * step[k];
                }
                let (lc, gc) = self.evaluate(&cand);
                if lc.is_finite() && lc >= ll - 1e-12 * (1.0 + ll.abs()) {
                    accepted = Some((cand, lc, gc));
                    break;
                }
                t *= 0.5;
            }
            let Some((cand, lc, gc)) = accepted else {
                return Ok((theta, ll, iteration));
            };
            lambda = if t >= cap && damp > 0.0 {
                damp / 10.0
            } else if t < cap {
                damp.max(1e-6)
            } else {
                0.0
            };
            if lambda < 1e-12 {
                lambda = 0.0;
            }
            let moved = free.iter().enumerate().all(|(k, &j)| (t * step[k]).abs() <= 1e-10 * (1.0 + cand[j].abs()));
            let gain = lc - ll;
            theta = cand;
            ll = lc;
            grad = gc;
            if theta.iter().any(|v| !v.is_finite() || v.abs() > 1e4) {
                return Err(StatError::ConvergenceFailure {
                    routine: "survival regression (estimates diverge: the likelihood has no finite maximum)",
                    iterations: iteration,
                });
            }
            let gnorm = free.iter().map(|&j| grad[j].abs()).fold(0.0_f64, f64::max);
            if (moved && gain.abs() <= 1e-10 * (1.0 + ll.abs())) || gnorm < 1e-10 {
                return Ok((theta, ll, iteration));
            }
        }
        Err(StatError::ConvergenceFailure { routine: "survival regression", iterations: MAX_ITERATIONS })
    }

    /// Least-squares start on representative log times.
    fn start(&self) -> Vec<f64> {
        let k = self.n_params();
        let pb = self.rows.first().map_or(1, Vec::len);
        let y: Vec<f64> = self
            .obs
            .iter()
            .map(|o| match (o.lower > 0.0, o.upper.is_finite()) {
                (true, true) => 0.5 * (o.lower.ln() + o.upper.ln()),
                (true, false) => o.lower.ln(),
                (false, true) => o.upper.ln(),
                (false, false) => f64::NAN,
            })
            .collect();
        let used: Vec<usize> = (0..y.len()).filter(|&i| y[i].is_finite()).collect();
        let mut xtx = vec![vec![0.0; pb]; pb];
        let mut xty = vec![0.0; pb];
        for &i in &used {
            let w = self.obs[i].weight;
            for a in 0..pb {
                xty[a] += w * self.rows[i][a] * y[i];
                for b in 0..pb {
                    xtx[a][b] += w * self.rows[i][a] * self.rows[i][b];
                }
            }
        }
        let wsum: f64 = used.iter().map(|&i| self.obs[i].weight).sum();
        let ybar = used.iter().map(|&i| self.obs[i].weight * y[i]).sum::<f64>() / wsum.max(1e-300);
        let beta = spd_solve(&xtx, &xty).filter(|b| b.iter().all(|v| v.is_finite())).unwrap_or_else(|| {
            let mut b = vec![0.0; pb];
            b[0] = ybar;
            b
        });
        let mut theta = beta;
        if self.distribution.has_scale() {
            let rss: f64 = used
                .iter()
                .map(|&i| {
                    let r = y[i] - self.mu(i, &theta);
                    self.obs[i].weight * r * r
                })
                .sum();
            let sd = (rss / wsum.max(1.0)).sqrt();
            theta.push(sd.clamp(0.1, 3.0).ln());
        }
        debug_assert_eq!(theta.len(), k);
        theta
    }
}

/// A fitted accelerated failure time model.
#[derive(Debug, Clone)]
pub struct SurvivalRegressionFit {
    likelihood: Likelihood,
    options: SurvivalRegressionOptions,
    theta: Vec<f64>,
    /// Covariance of `θ = (β₀, β, ln σ)`.
    pub covariance: Vec<Vec<f64>>,
    /// Maximized log-likelihood.
    pub log_likelihood: f64,
    /// Intercept and covariate coefficients (on the log-time scale).
    pub coefficients: Vec<RegressionParameter>,
    /// `σ` with intervals (`None` for the exponential).
    pub scale: Option<RegressionParameter>,
    /// Iterations used.
    pub iterations: usize,
}

/// Fit an accelerated failure time model to the observations `obs` with
/// covariate matrix `x` (`n × p`, no intercept column; `p` may be zero).
///
/// # Errors
/// See the module documentation.
pub fn fit_survival_regression(
    obs: &[LifeObservation],
    x: &Matrix,
    options: &SurvivalRegressionOptions,
) -> Result<SurvivalRegressionFit> {
    let n = obs.len();
    if x.nrows() != n {
        return Err(StatError::DimensionMismatch {
            context: "observations and covariate rows",
            expected: n,
            found: x.nrows(),
        });
    }
    if !x.is_finite() {
        return Err(StatError::NonFiniteInput { context: "covariates" });
    }
    if !(options.level > 0.0 && options.level < 1.0) {
        return Err(invalid("the confidence level must be between 0 and 1"));
    }
    if obs.iter().any(|o| o.kind() == CensorKind::Exact && o.lower <= 0.0) {
        return Err(invalid("exact failure times must be positive for a log-time model"));
    }
    if !obs.iter().any(|o| o.upper.is_finite()) {
        return Err(invalid("there are no failures (every unit is still running)"));
    }
    crate::life_data::LifeData::new(obs.to_vec())?;
    let p = x.ncols();
    let rows: Vec<Vec<f64>> =
        (0..n).map(|i| std::iter::once(1.0).chain((0..p).map(|j| x.col(j)[i])).collect()).collect();
    let likelihood = Likelihood { distribution: options.distribution, obs: obs.to_vec(), rows };
    let k = likelihood.n_params();
    let start = likelihood.start();
    let free: Vec<usize> = (0..k).collect();
    let (theta, ll, iterations) = likelihood.maximize(&start, &free)?;
    let info = likelihood.information(&theta, &free);
    let covariance = spd_inverse(&info)
        .ok_or_else(|| invalid("the information matrix is singular at the estimates (collinear covariates?)"))?;
    let mut fit = SurvivalRegressionFit {
        likelihood,
        options: options.clone(),
        theta,
        covariance,
        log_likelihood: ll,
        coefficients: Vec::new(),
        scale: None,
        iterations,
    };
    let z = normal_quantile(0.5 + options.level / 2.0);
    let names: Vec<String> = std::iter::once("Intercept".to_owned())
        .chain((0..p).map(|j| options.names.get(j).cloned().unwrap_or_else(|| format!("x{}", j + 1))))
        .collect();
    fit.coefficients = (0..=p)
        .map(|j| {
            let est = fit.theta[j];
            let se = fit.covariance[j][j].max(0.0).sqrt();
            RegressionParameter {
                name: names[j].clone(),
                estimate: est,
                std_error: se,
                z: est / se,
                p_value: normal_two_sided(est / se),
                wald: (est - z * se, est + z * se),
                profile: options.profile_intervals.then(|| fit.profile_interval(j, se)),
            }
        })
        .collect();
    if options.distribution.has_scale() {
        let j = k - 1;
        let ls = fit.theta[j];
        let se = fit.covariance[j][j].max(0.0).sqrt();
        let sigma = ls.exp();
        fit.scale = Some(RegressionParameter {
            name: "σ (scale of log time)".to_owned(),
            estimate: sigma,
            std_error: sigma * se,
            z: f64::NAN,
            p_value: f64::NAN,
            wald: ((ls - z * se).exp(), (ls + z * se).exp()),
            profile: options.profile_intervals.then(|| {
                let (a, b) = fit.profile_interval(j, se);
                (a.exp(), b.exp())
            }),
        });
    }
    Ok(fit)
}

impl SurvivalRegressionFit {
    /// The distribution.
    #[must_use]
    pub fn distribution(&self) -> AftDistribution {
        self.options.distribution
    }

    /// The options used.
    #[must_use]
    pub fn options(&self) -> &SurvivalRegressionOptions {
        &self.options
    }

    /// The estimates `θ = (β₀, β, ln σ)`.
    #[must_use]
    pub fn theta(&self) -> &[f64] {
        &self.theta
    }

    /// The observations fitted.
    #[must_use]
    pub fn observations(&self) -> &[LifeObservation] {
        &self.likelihood.obs
    }

    /// Number of estimated parameters.
    #[must_use]
    pub fn n_parameters(&self) -> usize {
        self.theta.len()
    }

    /// Total weight (units).
    #[must_use]
    pub fn n_units(&self) -> f64 {
        self.likelihood.obs.iter().map(|o| o.weight).sum()
    }

    /// `−2ℓ + 2k`.
    #[must_use]
    pub fn aic(&self) -> f64 {
        -2.0 * self.log_likelihood + 2.0 * self.n_parameters() as f64
    }

    /// `−2ℓ + k ln n` (`n` the number of units).
    #[must_use]
    pub fn bic(&self) -> f64 {
        -2.0 * self.log_likelihood + self.n_parameters() as f64 * self.n_units().ln()
    }

    /// `σ`.
    #[must_use]
    pub fn sigma(&self) -> f64 {
        self.likelihood.sigma(&self.theta)
    }

    fn profile_interval(&self, j: usize, se: f64) -> (f64, f64) {
        let crit = stat_core::ChiSquared::new(1.0)
            .and_then(|c| stat_core::ContinuousDistribution::quantile(&c, self.options.level))
            .unwrap_or(f64::NAN);
        let k = self.theta.len();
        let free: Vec<usize> = (0..k).filter(|&i| i != j).collect();
        let warm = std::cell::RefCell::new(self.theta.clone());
        let g = |c: f64| -> Option<f64> {
            let mut start = warm.borrow().clone();
            start[j] = c;
            let (th, ll, _) = self.likelihood.maximize(&start, &free).ok()?;
            *warm.borrow_mut() = th;
            Some(2.0 * (self.log_likelihood - ll) - crit)
        };
        let step = if se.is_finite() && se > 0.0 { se } else { 0.1 };
        let lower = find_crossing(&g, self.theta[j], -step);
        *warm.borrow_mut() = self.theta.clone();
        let upper = find_crossing(&g, self.theta[j], step);
        (lower.unwrap_or(f64::NEG_INFINITY), upper.unwrap_or(f64::INFINITY))
    }

    /// Likelihood-ratio test of dropping covariates `drop` (indices among the
    /// covariates, not counting the intercept).
    ///
    /// # Errors
    /// If the reduced model cannot be fitted.
    pub fn drop_columns_test(&self, drop: &[usize]) -> Result<ChiSquareTest> {
        let pb = self.coefficients.len();
        let keep: Vec<usize> = (0..pb).filter(|&j| j == 0 || !drop.contains(&(j - 1))).collect();
        let df = pb - keep.len();
        if df == 0 {
            return Ok(ChiSquareTest::new(0.0, 0));
        }
        let mut reduced = self.likelihood.clone();
        reduced.rows = reduced.rows.iter().map(|r| keep.iter().map(|&j| r[j]).collect()).collect();
        let mut start: Vec<f64> = keep.iter().map(|&j| self.theta[j]).collect();
        if self.distribution().has_scale() {
            start.push(self.theta[self.theta.len() - 1]);
        }
        let free: Vec<usize> = (0..start.len()).collect();
        let fallback = reduced.start();
        let ll = match reduced.maximize(&start, &free) {
            Ok((_, ll, _)) => ll,
            Err(_) => reduced.maximize(&fallback, &free)?.1,
        };
        Ok(ChiSquareTest::new(2.0 * (self.log_likelihood - ll), df))
    }

    fn design(&self, x: &[f64]) -> Vec<f64> {
        std::iter::once(1.0).chain(x.iter().copied()).collect()
    }

    /// `μ(x) = β₀ + xᵀβ` for a covariate row `x` (no intercept entry).
    #[must_use]
    pub fn location(&self, x: &[f64]) -> f64 {
        self.design(x).iter().zip(&self.theta).map(|(a, b)| a * b).sum()
    }

    /// The `p` quantile of the failure time at covariates `x`, with Wald limits.
    #[must_use]
    pub fn quantile(&self, x: &[f64], p: f64, level: f64) -> LimitedPrediction {
        let d = self.distribution();
        let w = d.standard_quantile(p);
        let sigma = self.sigma();
        let mut grad = self.design(x);
        if d.has_scale() {
            grad.push(sigma * w);
        }
        let log_q = self.location(x) + sigma * w;
        let se = quadratic(&self.covariance, &grad, &grad).max(0.0).sqrt();
        let z = normal_quantile(0.5 + level / 2.0);
        LimitedPrediction {
            estimate: log_q.exp(),
            std_error: se,
            lower: (log_q - z * se).exp(),
            upper: (log_q + z * se).exp(),
        }
    }

    /// The failure probability `F(t | x)` with limits formed on the `z` scale.
    #[must_use]
    pub fn failure_probability(&self, x: &[f64], t: f64, level: f64) -> LimitedPrediction {
        let d = self.distribution();
        if t <= 0.0 {
            return LimitedPrediction { estimate: 0.0, std_error: 0.0, lower: 0.0, upper: 0.0 };
        }
        let sigma = self.sigma();
        let zt = (t.ln() - self.location(x)) / sigma;
        let mut grad: Vec<f64> = self.design(x).iter().map(|v| -v / sigma).collect();
        if d.has_scale() {
            grad.push(-zt);
        }
        let se = quadratic(&self.covariance, &grad, &grad).max(0.0).sqrt();
        let z = normal_quantile(0.5 + level / 2.0);
        LimitedPrediction { estimate: d.cdf(zt), std_error: se, lower: d.cdf(zt - z * se), upper: d.cdf(zt + z * se) }
    }

    /// Survival `S(t | x)`.
    #[must_use]
    pub fn survival(&self, x: &[f64], t: f64) -> f64 {
        if t <= 0.0 {
            return 1.0;
        }
        self.distribution().ln_sf((t.ln() - self.location(x)) / self.sigma()).exp()
    }

    /// Cox–Snell residual intervals `[−ln S(l | xᵢ), −ln S(u | xᵢ)]` of every
    /// observation (equal ends for an exact failure; infinite upper end when
    /// right-censored; zero lower end when left-censored).
    #[must_use]
    pub fn cox_snell_residuals(&self) -> Vec<(f64, f64)> {
        let d = self.distribution();
        let sigma = self.sigma();
        self.likelihood
            .obs
            .iter()
            .enumerate()
            .map(|(i, o)| {
                let mu = self.likelihood.mu(i, &self.theta);
                let r = |t: f64| -> f64 {
                    if t <= 0.0 {
                        0.0
                    } else if t.is_infinite() {
                        f64::INFINITY
                    } else {
                        -d.ln_sf((t.ln() - mu) / sigma)
                    }
                };
                (r(o.lower), r(o.upper))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs() -> (Vec<LifeObservation>, Matrix) {
        // Two stress levels; right censoring at 1000.
        let raw = [
            (0.0, 410.0, true),
            (0.0, 520.0, true),
            (0.0, 640.0, true),
            (0.0, 790.0, true),
            (0.0, 1000.0, false),
            (0.0, 1000.0, false),
            (1.0, 120.0, true),
            (1.0, 180.0, true),
            (1.0, 260.0, true),
            (1.0, 300.0, true),
            (1.0, 410.0, true),
            (1.0, 1000.0, false),
        ];
        let o = raw
            .iter()
            .map(|&(_, t, e)| if e { LifeObservation::exact(t, 1.0) } else { LifeObservation::right(t, 1.0) })
            .collect();
        let x = Matrix::from_columns(&[raw.iter().map(|r| r.0).collect::<Vec<f64>>()]).unwrap();
        (o, x)
    }

    #[test]
    fn score_is_zero_and_intervals_bracket() {
        let (o, x) = obs();
        for d in AftDistribution::ALL {
            let f =
                fit_survival_regression(&o, &x, &SurvivalRegressionOptions { distribution: d, ..Default::default() })
                    .unwrap();
            let (_, g) = f.likelihood.evaluate(f.theta());
            assert!(g.iter().all(|v| v.abs() < 1e-6), "{d:?} {g:?}");
            for c in &f.coefficients {
                let (a, b) = c.profile.unwrap();
                assert!(a < c.estimate && c.estimate < b, "{d:?} {c:?}");
            }
            assert!(f.coefficients[1].estimate < 0.0, "higher stress shortens life");
            let q = f.quantile(&[0.0], 0.5, 0.95);
            assert!(q.lower < q.estimate && q.estimate < q.upper);
            let fp = f.failure_probability(&[0.0], q.estimate, 0.95);
            assert!((fp.estimate - 0.5).abs() < 1e-9, "{d:?} {}", fp.estimate);
            let test = f.drop_columns_test(&[0]).unwrap();
            assert_eq!(test.df, 1);
            assert!(test.statistic > 0.0);
        }
    }

    #[test]
    fn tails_are_consistent() {
        for d in AftDistribution::ALL {
            for z in [-40.0, -5.0, -0.3, 0.0, 0.7, 3.0, 35.0] {
                let s = d.ln_sf(z).exp();
                let f = d.ln_cdf(z).exp();
                assert!((s + f - 1.0).abs() < 1e-12, "{d:?} {z}: {s} + {f}");
            }
            let w = d.standard_quantile(0.3);
            assert!((d.cdf(w) - 0.3).abs() < 1e-12);
        }
    }
}
