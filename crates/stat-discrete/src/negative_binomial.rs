//! Negative binomial regression for overdispersed counts: a log-linear model
//! `ln μᵢ = xᵢᵀβ` with `Var(Yᵢ) = μᵢ + μᵢ²/θ`, fitted by maximum likelihood
//! over `β` and the shape `θ` together (Lawless, "Negative binomial and mixed
//! Poisson regression", *Can. J. Stat.* 15, 1987; Venables & Ripley,
//! *Modern Applied Statistics with S*, 4th ed., §7.4).
//!
//! The fit alternates the two maximizations until the log-likelihood
//! settles: for fixed `θ`, iteratively reweighted least squares with
//! working weights `wᵢ = μᵢθ/(θ + μᵢ)` and working response
//! `zᵢ = ηᵢ + (yᵢ − μᵢ)/μᵢ`; for fixed `μ`, Newton's method on the score
//! `Σ[ψ(θ + yᵢ) − ψ(θ) + ln θ + 1 − ln(θ + μᵢ) − (yᵢ + θ)/(θ + μᵢ)]`
//! started from the moment estimate `n / Σ(yᵢ/μᵢ − 1)²`. The first `μ` is
//! the Poisson fit's. Coefficient standard errors come from `(XᵀWX)⁻¹`
//! (θ treated as known, as is standard; `β̂` and `θ̂` are asymptotically
//! independent), with z tests; `θ`'s from its observed information.
//!
//! The log-likelihood is
//! `ℓ = Σ[ln Γ(θ + yᵢ) − ln Γ(θ) − ln Γ(yᵢ + 1) + θ ln θ + yᵢ ln μᵢ
//!      − (θ + yᵢ) ln(θ + μᵢ)]`, `AIC = −2ℓ + 2(p + 1)`, and the deviance
//! `2Σ[yᵢ ln(yᵢ/μᵢ) − (yᵢ + θ) ln((yᵢ + θ)/(μᵢ + θ))]` at the fitted `θ`.

use stat_core::distributions::{ContinuousDistribution, Normal};
use stat_core::special::{digamma, ln_gamma, trigamma};
use stat_core::{Matrix, QrDecomposition, Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Settings of [`fit_negative_binomial`].
#[derive(Debug, Clone, PartialEq)]
pub struct NegativeBinomialOptions {
    /// Include an intercept. Default `true`.
    pub intercept: bool,
    /// Names of the columns of `X` (default `x1, x2, …`).
    pub names: Vec<String>,
    /// Confidence level of the Wald intervals. Default 0.95.
    pub confidence_level: f64,
    /// Limit on the outer (θ, β) iterations. Default 50.
    pub max_iterations: usize,
}

impl Default for NegativeBinomialOptions {
    fn default() -> Self {
        Self { intercept: true, names: Vec::new(), confidence_level: 0.95, max_iterations: 50 }
    }
}

/// One coefficient with its Wald z test.
#[derive(Debug, Clone, PartialEq)]
pub struct NegativeBinomialCoefficient {
    /// Name (`Intercept` first when present).
    pub name: String,
    /// Estimate on the log scale.
    pub estimate: f64,
    /// Standard error.
    pub std_error: f64,
    /// `z = estimate / std_error`.
    pub z: f64,
    /// Two-sided p-value.
    pub p_value: f64,
    /// Wald interval.
    pub interval: (f64, f64),
}

/// A fitted negative binomial regression.
#[derive(Debug, Clone, PartialEq)]
pub struct NegativeBinomialFit {
    /// Coefficients, intercept first.
    pub coefficients: Vec<NegativeBinomialCoefficient>,
    /// Covariance of the coefficients.
    pub covariance: Matrix,
    /// Shape `θ` (larger is closer to Poisson).
    pub theta: f64,
    /// Standard error of `θ`.
    pub theta_std_error: f64,
    /// Maximized log-likelihood.
    pub log_likelihood: f64,
    /// `−2ℓ + 2(p + 1)`.
    pub aic: f64,
    /// Residual deviance at `θ̂`.
    pub deviance: f64,
    /// Fitted means.
    pub fitted: Vec<f64>,
    /// Residual degrees of freedom `n − p`.
    pub df_residual: usize,
    /// Warnings (e.g. θ growing without bound: no overdispersion).
    pub warnings: Vec<String>,
}

/// Largest shape accepted before the data are judged not overdispersed.
const MAX_THETA: f64 = 1e8;

/// Fit `ln μ = Xβ` (plus an intercept) to non-negative counts `y`.
///
/// # Errors
/// Mismatched sizes, non-finite or negative responses, no more rows than
/// coefficients, collinear columns, or failure to converge.
pub fn fit_negative_binomial(x: &Matrix, y: &[f64], options: &NegativeBinomialOptions) -> Result<NegativeBinomialFit> {
    let (n, q) = (x.nrows(), x.ncols());
    if y.len() != n {
        return Err(StatError::DimensionMismatch { context: "response vs. design rows", expected: n, found: y.len() });
    }
    if !options.names.is_empty() && options.names.len() != q {
        return Err(StatError::DimensionMismatch {
            context: "predictor names",
            expected: q,
            found: options.names.len(),
        });
    }
    if !x.is_finite() || y.iter().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "negative binomial data" });
    }
    if y.iter().any(|&v| v < 0.0) {
        return Err(invalid("a negative binomial response must be a non-negative count"));
    }
    if !(options.confidence_level > 0.0 && options.confidence_level < 1.0) {
        return Err(invalid("the confidence level must lie in (0, 1)"));
    }
    let off = usize::from(options.intercept);
    let p = q + off;
    if p == 0 {
        return Err(invalid("the model has no terms"));
    }
    if n <= p {
        return Err(StatError::InsufficientData { required: p + 1, found: n });
    }
    if y.iter().all(|&v| v == 0.0) {
        return Err(invalid("every count is zero"));
    }
    let design = |i: usize, j: usize| if options.intercept && j == 0 { 1.0 } else { x[(i, j - off)] };

    // Poisson start (θ = ∞), then alternate θ and β.
    let (mut beta, mut mu) = irls(&design, y, n, p, f64::INFINITY, None)?;
    let mut theta = theta_ml(y, &mu, moment_theta(y, &mu, p))?;
    let mut ll = log_likelihood(y, &mu, theta);
    let mut converged = false;
    for _ in 0..options.max_iterations {
        let (b, m) = irls(&design, y, n, p, theta, Some(&beta))?;
        beta = b;
        mu = m;
        theta = theta_ml(y, &mu, theta)?;
        let next = log_likelihood(y, &mu, theta);
        if (next - ll).abs() <= 1e-10 * (1.0 + next.abs()) {
            ll = next;
            converged = true;
            break;
        }
        ll = next;
    }
    if !converged {
        return Err(StatError::ConvergenceFailure {
            routine: "negative binomial regression",
            iterations: options.max_iterations,
        });
    }
    let mut warnings = Vec::new();
    if theta >= MAX_THETA {
        warnings.push("θ is very large: the counts show no overdispersion, so a Poisson model fits as well".into());
    }
    // Covariance (XᵀWX)⁻¹ at the solution.
    let mut wx = Matrix::zeros(n, p);
    for i in 0..n {
        let w = (mu[i] * theta / (theta + mu[i])).sqrt();
        for j in 0..p {
            wx[(i, j)] = w * design(i, j);
        }
    }
    let covariance = QrDecomposition::new(wx)?.inverse_gram()?;
    let z_crit = Normal::standard().quantile(0.5 + options.confidence_level / 2.0)?;
    let names: Vec<String> = options
        .intercept
        .then(|| "Intercept".to_owned())
        .into_iter()
        .chain((0..q).map(|j| options.names.get(j).cloned().unwrap_or_else(|| format!("x{}", j + 1))))
        .collect();
    let coefficients = names
        .into_iter()
        .enumerate()
        .map(|(j, name)| {
            let se = covariance[(j, j)].max(0.0).sqrt();
            let z = beta[j] / se;
            Ok(NegativeBinomialCoefficient {
                name,
                estimate: beta[j],
                std_error: se,
                z,
                p_value: 2.0 * Normal::standard().sf(z.abs())?,
                interval: (beta[j] - z_crit * se, beta[j] + z_crit * se),
            })
        })
        .collect::<Result<_>>()?;
    let info = theta_information(y, &mu, theta);
    let deviance = 2.0
        * y.iter()
            .zip(&mu)
            .map(|(&yi, &m)| {
                let a = if yi > 0.0 { yi * (yi / m).ln() } else { 0.0 };
                a - (yi + theta) * ((yi + theta) / (m + theta)).ln()
            })
            .sum::<f64>();
    Ok(NegativeBinomialFit {
        coefficients,
        covariance,
        theta,
        theta_std_error: if info > 0.0 { 1.0 / info.sqrt() } else { f64::NAN },
        log_likelihood: ll,
        aic: -2.0 * ll + 2.0 * (p + 1) as f64,
        deviance,
        fitted: mu,
        df_residual: n - p,
        warnings,
    })
}

/// IRLS for the log link with variance `μ + μ²/θ` (`θ = ∞`: Poisson).
fn irls(
    design: &dyn Fn(usize, usize) -> f64,
    y: &[f64],
    n: usize,
    p: usize,
    theta: f64,
    start: Option<&[f64]>,
) -> Result<(Vec<f64>, Vec<f64>)> {
    let eta_of = |b: &[f64]| -> Vec<f64> { (0..n).map(|i| (0..p).map(|j| design(i, j) * b[j]).sum()).collect() };
    let mut eta: Vec<f64> = match start {
        Some(b) => eta_of(b),
        None => y.iter().map(|&v| (v + 0.1).ln()).collect(),
    };
    let mut beta = vec![0.0; p];
    let mut last = f64::INFINITY;
    for _ in 0..100 {
        let mu: Vec<f64> = eta.iter().map(|e| e.exp()).collect();
        let mut wx = Matrix::zeros(n, p);
        let mut wz = vec![0.0; n];
        for i in 0..n {
            let w = if theta.is_finite() { mu[i] * theta / (theta + mu[i]) } else { mu[i] };
            let sw = w.sqrt();
            for j in 0..p {
                wx[(i, j)] = sw * design(i, j);
            }
            wz[i] = sw * (eta[i] + (y[i] - mu[i]) / mu[i]);
        }
        beta = QrDecomposition::new(wx)?.solve_least_squares(&wz)?;
        eta = eta_of(&beta);
        let mu: Vec<f64> = eta.iter().map(|e| e.exp()).collect();
        let dev: f64 = y
            .iter()
            .zip(&mu)
            .map(|(&yi, &m)| {
                let a = if yi > 0.0 { yi * (yi / m).ln() } else { 0.0 };
                if theta.is_finite() { a - (yi + theta) * ((yi + theta) / (m + theta)).ln() } else { a - (yi - m) }
            })
            .sum();
        if (dev - last).abs() <= 1e-12 * (dev.abs() + 0.1) {
            return Ok((beta, mu));
        }
        last = dev;
    }
    Err(StatError::ConvergenceFailure { routine: "negative binomial IRLS", iterations: 100 })
}

fn moment_theta(y: &[f64], mu: &[f64], p: usize) -> f64 {
    let s: f64 = y.iter().zip(mu).map(|(&yi, &m)| (yi / m - 1.0).powi(2)).sum();
    let n = y.len().saturating_sub(p).max(1) as f64;
    if s > 0.0 { n / s } else { 1.0 }
}

fn theta_score(y: &[f64], mu: &[f64], t: f64) -> f64 {
    y.iter()
        .zip(mu)
        .map(|(&yi, &m)| digamma(t + yi) - digamma(t) + t.ln() + 1.0 - (t + m).ln() - (yi + t) / (m + t))
        .sum()
}

fn theta_information(y: &[f64], mu: &[f64], t: f64) -> f64 {
    y.iter()
        .zip(mu)
        .map(|(&yi, &m)| -trigamma(t + yi) + trigamma(t) - 1.0 / t + 2.0 / (m + t) - (yi + t) / (m + t).powi(2))
        .sum()
}

/// Maximum-likelihood shape for fixed means (Newton's method, halving
/// steps that leave θ ≤ 0).
fn theta_ml(y: &[f64], mu: &[f64], start: f64) -> Result<f64> {
    let mut t = start.clamp(1e-6, MAX_THETA);
    for _ in 0..200 {
        let info = theta_information(y, mu, t);
        let score = theta_score(y, mu, t);
        let mut step = if info > 0.0 { score / info } else { t };
        while t + step <= 0.0 {
            step /= 2.0;
        }
        let next = (t + step).min(MAX_THETA);
        if (next - t).abs() <= 1e-10 * t {
            return Ok(next);
        }
        t = next;
        if t >= MAX_THETA {
            return Ok(t);
        }
    }
    Err(StatError::ConvergenceFailure { routine: "negative binomial shape", iterations: 200 })
}

fn log_likelihood(y: &[f64], mu: &[f64], t: f64) -> f64 {
    y.iter()
        .zip(mu)
        .map(|(&yi, &m)| {
            ln_gamma(t + yi) - ln_gamma(t) - ln_gamma(yi + 1.0) + t * t.ln() + if yi > 0.0 { yi * m.ln() } else { 0.0 }
                - (t + yi) * (t + m).ln()
        })
        .sum()
}

/// Likelihood-ratio test of a reduced model against a fuller one (each
/// with its own θ): `χ² = 2(ℓ_full − ℓ_reduced)` on the difference in the
/// number of coefficients.
///
/// # Errors
/// A reduced model with at least as many coefficients.
pub fn negative_binomial_lr_test(
    full: &NegativeBinomialFit,
    reduced: &NegativeBinomialFit,
) -> Result<(f64, usize, f64)> {
    let df = full.coefficients.len().checked_sub(reduced.coefficients.len()).filter(|&d| d > 0);
    let Some(df) = df else {
        return Err(invalid("the reduced model must have fewer coefficients"));
    };
    let chi2 = (2.0 * (full.log_likelihood - reduced.log_likelihood)).max(0.0);
    let p = stat_core::distributions::ChiSquared::new(df as f64)?.sf(chi2)?;
    Ok((chi2, df, p))
}
