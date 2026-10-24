//! Generalized linear models fitted by iteratively reweighted least squares.
//!
//! # Model
//! `g(E[y]) = η = β₀ + xᵀβ` with a response distribution (family) and a link
//! function `g` ([`Link`]):
//!
//! | Family | Links (default first) | Variance `V(μ)` | Typical use |
//! |---|---|---|---|
//! | binomial | logit, probit, complementary log-log | `μ(1−μ)` | yes/no outcomes, dose–response |
//! | Poisson | log, identity | `μ` | counts |
//! | Gaussian | identity, log | `1` | continuous (identity equals OLS) |
//! | Gamma | inverse, log, identity | `μ²` | positive, skewed, constant CV |
//!
//! Optional prior weights `wᵢ` multiply each observation's contribution to
//! the likelihood. For the binomial family they are the numbers of trials,
//! with the response the observed proportion `yᵢ = rᵢ/nᵢ` (grouped binary
//! data, as in probit analysis).
//!
//! # Fitting
//! Fisher scoring as iteratively reweighted least squares (Nelder & Wedderburn
//! 1972, *JRSS A* 135; McCullagh & Nelder, *Generalized Linear Models*, 2nd ed.,
//! §2.5): with working weights `w = wᵢ (dμ/dη)² / V(μ)` and working response
//! `z = η + (y − μ)/(dμ/dη)`, each step solves the weighted least-squares
//! problem `min Σ w (z − xᵀβ)²` by Householder QR of `√w·X` (never forming
//! `XᵀWX`). A step that gives a non-finite deviance (e.g. a negative mean
//! under the inverse link) is halved. Iteration stops when the deviance
//! changes by less than `10⁻¹⁰ (|D| + 0.1)` and every estimate by less than
//! `10⁻¹⁰ (|β| + 10⁻⁶)` (non-canonical links converge only linearly);
//! estimates still drifting after five flat steps, as under separation, stop.
//!
//! # Inference
//! `Cov(β̂) = φ (XᵀWX)⁻¹` with dispersion `φ = 1` (binomial, Poisson) or the
//! Pearson estimate `φ = Σ wᵢ(yᵢ − μ̂ᵢ)²/V(μ̂ᵢ) / (n − p)` (Gaussian, Gamma; for
//! the Gaussian family with identity link this is `RSS/(n − p)`). Wald tests
//! use `z = β̂/se` against the normal distribution (t on `n − p` df when `φ` is
//! estimated). [`likelihood_ratio_test`] compares nested fits by the change in
//! deviance against χ². With the logit link `exp(β)` is an odds ratio; with
//! the log link, a rate (or mean) ratio.
//!
//! Deviance `D = 2(ℓ_saturated − ℓ)`; `AIC = −2ℓ + 2k`, `BIC = −2ℓ + k ln n`,
//! with `k` coefficients (plus one for an estimated dispersion). For the Gamma
//! family `ℓ` is evaluated at the shape `1/φ̃` with `φ̃ = D / Σwᵢ` (the
//! convention of R's `Gamma()$aic`).

use stat_core::distributions::{ChiSquared, ContinuousDistribution, Normal, StudentsT};
use stat_core::special::{erfc, ln_gamma};
use stat_core::{Matrix, QrDecomposition, Result, StatError};

/// Response distribution. Each family has a default (canonical) link, and
/// [`GlmOptions::link`] can choose another from [`Family::links`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    /// 0/1 response (or a proportion, with the number of trials as the prior
    /// weight); logit, probit or complementary log-log link.
    Binomial,
    /// Count response; log or identity link.
    Poisson,
    /// Continuous response; identity or log link.
    Gaussian,
    /// Positive continuous response with a constant coefficient of variation,
    /// `V(μ) = μ²`; inverse (canonical), log or identity link.
    Gamma,
}

impl Family {
    /// Every family, in menu order.
    pub const ALL: [Self; 4] = [Self::Binomial, Self::Poisson, Self::Gaussian, Self::Gamma];

    /// Display name with the default link, e.g. `"binomial (logit link)"`.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Binomial => "binomial (logit link)",
            Self::Poisson => "Poisson (log link)",
            Self::Gaussian => "normal (identity link)",
            Self::Gamma => "Gamma (inverse link)",
        }
    }

    /// Family name without the link, e.g. `"binomial"`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Binomial => "binomial",
            Self::Poisson => "Poisson",
            Self::Gaussian => "normal",
            Self::Gamma => "Gamma",
        }
    }

    /// The canonical link, used when [`GlmOptions::link`] is `None`.
    #[must_use]
    pub fn default_link(self) -> Link {
        match self {
            Self::Binomial => Link::Logit,
            Self::Poisson => Link::Log,
            Self::Gaussian => Link::Identity,
            Self::Gamma => Link::Inverse,
        }
    }

    /// Links this family can be fitted with (the default first).
    #[must_use]
    pub fn links(self) -> &'static [Link] {
        match self {
            Self::Binomial => &[Link::Logit, Link::Probit, Link::ComplementaryLogLog],
            Self::Poisson => &[Link::Log, Link::Identity],
            Self::Gaussian => &[Link::Identity, Link::Log],
            Self::Gamma => &[Link::Inverse, Link::Log, Link::Identity],
        }
    }

    /// `true` when the dispersion `φ` is estimated (normal and Gamma), so Wald
    /// tests use t on `n − p` df.
    #[must_use]
    pub fn estimates_dispersion(self) -> bool {
        matches!(self, Self::Gaussian | Self::Gamma)
    }

    fn variance(self, mu: f64) -> f64 {
        match self {
            Self::Binomial => mu * (1.0 - mu),
            Self::Poisson => mu,
            Self::Gaussian => 1.0,
            Self::Gamma => mu * mu,
        }
    }

    /// Unit deviance contribution (before the prior weight).
    fn unit_deviance(self, y: f64, mu: f64) -> f64 {
        let ylog = |a: f64, b: f64| if a > 0.0 { a * (a / b).ln() } else { 0.0 };
        match self {
            Self::Binomial => 2.0 * (ylog(y, mu) + ylog(1.0 - y, 1.0 - mu)),
            Self::Poisson => 2.0 * (ylog(y, mu) - (y - mu)),
            Self::Gaussian => (y - mu) * (y - mu),
            Self::Gamma => 2.0 * (-(y / mu).ln() + (y - mu) / mu),
        }
    }

    /// Keep a mean strictly inside the family's range.
    fn clamp_mean(self, m: f64) -> f64 {
        match self {
            Self::Binomial => m.clamp(1e-15, 1.0 - 1e-15),
            Self::Poisson | Self::Gamma => m.max(1e-300),
            Self::Gaussian => m,
        }
    }
}

/// Link function `g` with `g(μ) = η`.
///
/// | Link | `g(μ)` | `μ = g⁻¹(η)` |
/// |---|---|---|
/// | logit | `ln(μ/(1−μ))` | `1/(1+e^(−η))` |
/// | probit | `Φ⁻¹(μ)` | `Φ(η)`, the standard normal cdf |
/// | complementary log-log | `ln(−ln(1−μ))` | `1 − exp(−e^η)` |
/// | log | `ln μ` | `e^η` |
/// | identity | `μ` | `η` |
/// | inverse | `1/μ` | `1/η` |
///
/// The probit link is the model of probit analysis (Finney, *Probit
/// Analysis*, 3rd ed., 1971); the complementary log-log link is the
/// extreme-value (one-hit) model, asymmetric about μ = ½ (McCullagh & Nelder
/// 1989, §4.3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Link {
    /// Log-odds.
    Logit,
    /// Inverse standard normal cdf.
    Probit,
    /// `ln(−ln(1 − μ))`.
    ComplementaryLogLog,
    /// Natural log.
    Log,
    /// No transformation.
    Identity,
    /// Reciprocal.
    Inverse,
}

impl Link {
    /// Every link.
    pub const ALL: [Self; 6] =
        [Self::Logit, Self::Probit, Self::ComplementaryLogLog, Self::Log, Self::Identity, Self::Inverse];

    /// Display name, e.g. `"complementary log-log"`.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Logit => "logit",
            Self::Probit => "probit",
            Self::ComplementaryLogLog => "complementary log-log",
            Self::Log => "log",
            Self::Identity => "identity",
            Self::Inverse => "inverse",
        }
    }

    /// Stable key for project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Logit => "logit",
            Self::Probit => "probit",
            Self::ComplementaryLogLog => "cloglog",
            Self::Log => "log",
            Self::Identity => "identity",
            Self::Inverse => "inverse",
        }
    }

    /// The link with this key.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.key() == key)
    }

    /// Mean from the linear predictor, `g⁻¹(η)`.
    #[must_use]
    pub fn inverse(self, eta: f64) -> f64 {
        match self {
            Self::Logit => 1.0 / (1.0 + (-eta).exp()),
            Self::Probit => 0.5 * erfc(-eta / std::f64::consts::SQRT_2),
            Self::ComplementaryLogLog => -(-eta.exp()).exp_m1(),
            Self::Log => eta.exp(),
            Self::Identity => eta,
            Self::Inverse => 1.0 / eta,
        }
    }

    /// Linear predictor from the mean, `g(μ)`. For the probit link a mean of
    /// exactly 0 or 1 maps to ∓40 rather than ∓∞.
    #[must_use]
    pub fn link(self, mu: f64) -> f64 {
        match self {
            Self::Logit => (mu / (1.0 - mu)).ln(),
            Self::Probit => Normal::standard().quantile(mu).unwrap_or(if mu < 0.5 { -40.0 } else { 40.0 }),
            Self::ComplementaryLogLog => (-(-mu).ln_1p()).ln(),
            Self::Log => mu.ln(),
            Self::Identity => mu,
            Self::Inverse => 1.0 / mu,
        }
    }

    /// `dμ/dη` at `η`.
    #[must_use]
    pub fn mu_eta(self, eta: f64) -> f64 {
        match self {
            Self::Logit => {
                let m = 1.0 / (1.0 + (-eta).exp());
                m * (1.0 - m)
            }
            Self::Probit => (-0.5 * eta * eta).exp() / (2.0 * std::f64::consts::PI).sqrt(),
            Self::ComplementaryLogLog => (eta - eta.exp()).exp(),
            Self::Log => eta.exp(),
            Self::Identity => 1.0,
            Self::Inverse => -1.0 / (eta * eta),
        }
    }
}

/// Options for [`fit_glm`].
#[derive(Debug, Clone, PartialEq)]
pub struct GlmOptions {
    /// Response distribution.
    pub family: Family,
    /// Link function; `None` uses [`Family::default_link`].
    pub link: Option<Link>,
    /// Prior weights, one per row (empty for all 1). For the binomial family
    /// they are the numbers of trials and the response is the proportion of
    /// successes.
    pub weights: Vec<f64>,
    /// Include an intercept. Default `true`.
    pub intercept: bool,
    /// Names of the columns of `X` (default `x1, x2, …`).
    pub names: Vec<String>,
    /// Confidence level for Wald intervals. Default 0.95.
    pub confidence_level: f64,
    /// Iteration limit. Default 100.
    pub max_iterations: usize,
}

impl GlmOptions {
    /// Defaults for `family`.
    #[must_use]
    pub fn new(family: Family) -> Self {
        Self {
            family,
            link: None,
            weights: Vec::new(),
            intercept: true,
            names: Vec::new(),
            confidence_level: 0.95,
            max_iterations: 100,
        }
    }
}

/// One coefficient with Wald inference.
#[derive(Debug, Clone, PartialEq)]
pub struct GlmCoefficient {
    /// Term name (`Intercept` first when present).
    pub name: String,
    /// Estimate on the link scale.
    pub estimate: f64,
    /// Standard error.
    pub std_error: f64,
    /// Wald statistic `estimate / std_error` (z, or t when φ is estimated).
    pub statistic: f64,
    /// Two-sided p-value.
    pub p_value: f64,
    /// Wald confidence interval on the link scale.
    pub confidence_interval: (f64, f64),
}

/// A fitted generalized linear model.
#[derive(Debug, Clone, PartialEq)]
pub struct GlmFit {
    /// Family used.
    pub family: Family,
    /// Link used.
    pub link: Link,
    /// Coefficients, intercept first.
    pub coefficients: Vec<GlmCoefficient>,
    /// Covariance of the estimates.
    pub covariance: Matrix,
    /// Residual deviance.
    pub deviance: f64,
    /// Pearson's statistic `Σ wᵢ (yᵢ − μ̂ᵢ)² / V(μ̂ᵢ)`.
    pub pearson_chi_square: f64,
    /// Deviance of the intercept-only model; without an intercept, of the
    /// model `η = 0` (`μ = g⁻¹(0)`, as R's `glm`), which is `∞` when
    /// `g⁻¹(0)` is not a possible mean (the inverse link, or an identity
    /// link for Gamma data).
    pub null_deviance: f64,
    /// Residual degrees of freedom `n − p`.
    pub df_residual: usize,
    /// Null-model degrees of freedom.
    pub df_null: usize,
    /// Maximized log-likelihood.
    pub log_likelihood: f64,
    /// Akaike information criterion.
    pub aic: f64,
    /// Bayesian information criterion.
    pub bic: f64,
    /// Dispersion `φ`.
    pub dispersion: f64,
    /// Fitted means `μ̂`.
    pub fitted: Vec<f64>,
    /// IRLS iterations used.
    pub iterations: usize,
    /// Warnings (e.g. fitted probabilities of 0 or 1).
    pub warnings: Vec<String>,
    /// Confidence level of the Wald intervals.
    pub confidence_level: f64,
    intercept: bool,
    /// Critical value of the Wald intervals (normal, or t when φ is estimated).
    critical: f64,
}

impl GlmFit {
    /// Display name of the family and link, e.g. `"binomial (probit link)"`.
    #[must_use]
    pub fn label(&self) -> String {
        format!("{} ({} link)", self.family.name(), self.link.label())
    }

    /// Predicted mean for new predictor rows (same columns as the fit).
    ///
    /// # Errors
    /// [`StatError::DimensionMismatch`] for the wrong number of columns.
    pub fn predict(&self, x: &Matrix) -> Result<Vec<f64>> {
        let off = usize::from(self.intercept);
        let p = self.coefficients.len() - off;
        if x.ncols() != p {
            return Err(StatError::DimensionMismatch {
                context: "GLM prediction columns",
                expected: p,
                found: x.ncols(),
            });
        }
        Ok((0..x.nrows())
            .map(|i| {
                let eta = if self.intercept { self.coefficients[0].estimate } else { 0.0 }
                    + (0..p).map(|j| x[(i, j)] * self.coefficients[j + off].estimate).sum::<f64>();
                self.link.inverse(eta)
            })
            .collect())
    }

    /// Predicted mean with Wald confidence limits for new predictor rows:
    /// `η̂ = x̃ᵀβ̂` with `se(η̂)² = x̃ᵀ V x̃` (`x̃` with a leading 1 when the
    /// model has an intercept, `V` the estimates' covariance), the interval
    /// `η̂ ± c·se` on the link scale (`c` the normal quantile, or t when φ is
    /// estimated) mapped through the inverse link. The limits are returned in
    /// increasing order (the inverse link is decreasing), and for the
    /// probability and log links they respect the response's range.
    /// With the inverse link, a link-scale interval that contains `η = 0`
    /// maps onto no interval of means, and both limits are NaN.
    ///
    /// # Errors
    /// [`StatError::DimensionMismatch`] for the wrong number of columns.
    pub fn predict_with_limits(&self, x: &Matrix) -> Result<Vec<(f64, f64, f64)>> {
        let off = usize::from(self.intercept);
        let p = self.coefficients.len() - off;
        if x.ncols() != p {
            return Err(StatError::DimensionMismatch {
                context: "GLM prediction columns",
                expected: p,
                found: x.ncols(),
            });
        }
        Ok((0..x.nrows())
            .map(|i| {
                let row: Vec<f64> = (0..off).map(|_| 1.0).chain((0..p).map(|j| x[(i, j)])).collect();
                let eta: f64 = row.iter().zip(&self.coefficients).map(|(v, c)| v * c.estimate).sum();
                let var: f64 = (0..row.len())
                    .flat_map(|a| (0..row.len()).map(move |b| (a, b)))
                    .map(|(a, b)| row[a] * self.covariance[(a, b)] * row[b])
                    .sum();
                let half = self.critical * var.max(0.0).sqrt();
                let inv = |e: f64| self.link.inverse(e);
                // μ = 1/η maps an interval containing η = 0 onto two
                // unbounded rays, not an interval: no finite limits exist.
                if self.link == Link::Inverse && (eta - half) * (eta + half) <= 0.0 {
                    return (inv(eta), f64::NAN, f64::NAN);
                }
                let (a, b) = (inv(eta - half), inv(eta + half));
                (inv(eta), a.min(b), a.max(b))
            })
            .collect())
    }

    /// Number of coefficients `p`.
    #[must_use]
    pub fn n_parameters(&self) -> usize {
        self.coefficients.len()
    }

    /// Critical value used for the Wald intervals.
    #[must_use]
    pub fn critical_value(&self) -> f64 {
        self.critical
    }
}

/// A likelihood-ratio (deviance) test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LikelihoodRatioTest {
    /// `D_reduced − D_full` (divided by φ when φ is estimated).
    pub chi_square: f64,
    /// Difference in the number of parameters.
    pub df: usize,
    /// Upper-tail χ² p-value.
    pub p_value: f64,
}

/// Fit a generalized linear model of `y` on the columns of `x`.
///
/// # Errors
/// * [`StatError::DimensionMismatch`] if `y`, the weights and `x` differ in
///   rows, or the names do not match the columns.
/// * [`StatError::InvalidArgument`] for responses outside the family's range
///   (binomial: 0 or 1, or a proportion in [0, 1] with weights; Poisson:
///   non-negative; Gamma: positive), a link the family does not offer,
///   non-positive weights, or a bad confidence level.
/// * [`StatError::InsufficientData`] unless `n > p`.
/// * [`StatError::RankDeficient`] for collinear predictors.
/// * [`StatError::ConvergenceFailure`] if IRLS does not converge.
pub fn fit_glm(x: &Matrix, y: &[f64], options: &GlmOptions) -> Result<GlmFit> {
    let family = options.family;
    let link = options.link.unwrap_or_else(|| family.default_link());
    let (n, q) = (x.nrows(), x.ncols());
    if y.len() != n {
        return Err(StatError::DimensionMismatch { context: "response vs. design rows", expected: n, found: y.len() });
    }
    if !options.weights.is_empty() && options.weights.len() != n {
        return Err(StatError::DimensionMismatch {
            context: "prior weights vs. design rows",
            expected: n,
            found: options.weights.len(),
        });
    }
    if !options.names.is_empty() && options.names.len() != q {
        return Err(StatError::DimensionMismatch {
            context: "predictor names",
            expected: q,
            found: options.names.len(),
        });
    }
    if !(options.confidence_level > 0.0 && options.confidence_level < 1.0) {
        return Err(StatError::InvalidArgument { message: "the confidence level must lie in (0, 1)".into() });
    }
    if !family.links().contains(&link) {
        return Err(StatError::InvalidArgument {
            message: format!("the {} family does not offer the {} link", family.name(), link.label()),
        });
    }
    if !x.is_finite() || y.iter().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "GLM data" });
    }
    if options.weights.iter().any(|&w| !(w.is_finite() && w > 0.0)) {
        return Err(StatError::InvalidArgument { message: "prior weights must be positive and finite".into() });
    }
    let weights: Vec<f64> = if options.weights.is_empty() { vec![1.0; n] } else { options.weights.clone() };
    let grouped = !options.weights.is_empty();
    match family {
        Family::Binomial if !grouped && y.iter().any(|&v| v != 0.0 && v != 1.0) => {
            return Err(StatError::InvalidArgument { message: "a binomial response must be 0 or 1".into() });
        }
        Family::Binomial if y.iter().any(|&v| !(0.0..=1.0).contains(&v)) => {
            return Err(StatError::InvalidArgument {
                message: "a binomial response with trials as weights must be a proportion in [0, 1]".into(),
            });
        }
        Family::Poisson if y.iter().any(|&v| v < 0.0) => {
            return Err(StatError::InvalidArgument { message: "a Poisson response must be non-negative".into() });
        }
        Family::Gamma if y.iter().any(|&v| v <= 0.0) => {
            return Err(StatError::InvalidArgument { message: "a Gamma response must be positive".into() });
        }
        _ => {}
    }
    let off = usize::from(options.intercept);
    let p = q + off;
    if p == 0 {
        return Err(StatError::InvalidArgument { message: "the model has no terms".into() });
    }
    if n <= p {
        return Err(StatError::InsufficientData { required: p + 1, found: n });
    }
    let design = |i: usize, j: usize| if options.intercept && j == 0 { 1.0 } else { x[(i, j - off)] };

    let model = Model { family, link, y, weights: &weights, n, p, design: &design };
    let (beta, fitted, iterations, final_weights) = model.irls(options.max_iterations)?;
    let deviance = model.deviance(&fitted);
    let pearson_chi_square: f64 =
        (0..n).map(|i| weights[i] * (y[i] - fitted[i]).powi(2) / family.variance(fitted[i]).max(1e-300)).sum();

    // Covariance from the QR of √w·X at the solution.
    let mut wx = Matrix::zeros(n, p);
    for i in 0..n {
        let sw = final_weights[i].sqrt();
        for j in 0..p {
            wx[(i, j)] = sw * design(i, j);
        }
    }
    let qr = QrDecomposition::new(wx)?;
    let df_residual = n - p;
    let dispersion = if family.estimates_dispersion() { pearson_chi_square / df_residual as f64 } else { 1.0 };
    let inv = qr.inverse_gram()?;
    let mut covariance = Matrix::zeros(p, p);
    for i in 0..p {
        for j in 0..p {
            covariance[(i, j)] = dispersion * inv[(i, j)];
        }
    }
    let names: Vec<String> = (if options.intercept { Some("Intercept".to_owned()) } else { None })
        .into_iter()
        .chain((0..q).map(|j| options.names.get(j).cloned().unwrap_or_else(|| format!("x{}", j + 1))))
        .collect();
    let alpha = 1.0 - options.confidence_level;
    let (crit, p_of): (f64, Box<dyn Fn(f64) -> Result<f64>>) = if family.estimates_dispersion() {
        let t = StudentsT::new(df_residual as f64)?;
        let c = t.quantile(1.0 - alpha / 2.0)?;
        (c, Box::new(move |s: f64| t.two_sided_p_value(s)))
    } else {
        let z = Normal::standard();
        (z.quantile(1.0 - alpha / 2.0)?, Box::new(move |s: f64| Ok(2.0 * z.sf(s.abs())?)))
    };
    let coefficients = names
        .into_iter()
        .enumerate()
        .map(|(j, name)| {
            let estimate = beta[j];
            let std_error = covariance[(j, j)].sqrt();
            let statistic = estimate / std_error;
            Ok(GlmCoefficient {
                name,
                estimate,
                std_error,
                statistic,
                p_value: p_of(statistic)?,
                confidence_interval: (estimate - crit * std_error, estimate + crit * std_error),
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let log_likelihood = model.log_likelihood(&fitted, deviance);
    let k = p as f64 + if family.estimates_dispersion() { 1.0 } else { 0.0 };
    let total_weight: f64 = weights.iter().sum();
    let null_mean = if options.intercept {
        weights.iter().zip(y).map(|(w, v)| w * v).sum::<f64>() / total_weight
    } else {
        link.inverse(0.0)
    };
    // Without an intercept the null model is η = 0. When g⁻¹(0) is not a
    // possible mean (μ = 1/0 for the inverse link, μ = 0 for an identity link
    // on positive data) that model cannot describe the data: its deviance is
    // infinite rather than the NaN of ∞ − ∞.
    let null_deviance = match model.deviance(&vec![null_mean; n]) {
        d if d.is_nan() && !options.intercept => f64::INFINITY,
        d => d,
    };
    let mut warnings = Vec::new();
    if family == Family::Binomial && fitted.iter().any(|&m| !(1e-10..=1.0 - 1e-10).contains(&m)) {
        warnings.push(
            "Some fitted probabilities are 0 or 1: the classes may be (quasi-)separated and estimates unreliable."
                .to_owned(),
        );
    }
    Ok(GlmFit {
        family,
        link,
        coefficients,
        covariance,
        deviance,
        pearson_chi_square,
        null_deviance,
        df_residual,
        df_null: n - off,
        log_likelihood,
        aic: -2.0 * log_likelihood + 2.0 * k,
        bic: -2.0 * log_likelihood + k * (n as f64).ln(),
        dispersion,
        fitted,
        iterations,
        warnings,
        confidence_level: options.confidence_level,
        critical: crit,
        intercept: options.intercept,
    })
}

type IrlsResult = (Vec<f64>, Vec<f64>, usize, Vec<f64>);

/// The data and model specification shared by the fitting steps.
struct Model<'a> {
    family: Family,
    link: Link,
    y: &'a [f64],
    weights: &'a [f64],
    n: usize,
    p: usize,
    design: &'a dyn Fn(usize, usize) -> f64,
}

impl Model<'_> {
    fn deviance(&self, mu: &[f64]) -> f64 {
        (0..self.n).map(|i| self.weights[i] * self.family.unit_deviance(self.y[i], mu[i])).sum()
    }

    /// Linear predictors and means of `beta`. A mean outside the family's
    /// range (μ ≤ 0 for Poisson and Gamma, μ outside [0, 1] for binomial),
    /// which links such as the identity can produce, comes back as NaN so the
    /// step is halved, as R's `glm.fit` does with `validmu`; means on the
    /// boundary only through rounding are clamped inside it.
    fn means(&self, beta: &[f64]) -> (Vec<f64>, Vec<f64>) {
        let eta: Vec<f64> = (0..self.n).map(|i| (0..self.p).map(|j| (self.design)(i, j) * beta[j]).sum()).collect();
        let mu = self.means_of(&eta);
        (eta, mu)
    }

    /// Means of the linear predictors `eta`, NaN outside the family's range
    /// as in [`means`](Self::means).
    fn means_of(&self, eta: &[f64]) -> Vec<f64> {
        eta.iter()
            .map(|&e| {
                let m = self.link.inverse(e);
                let outside = match self.family {
                    Family::Binomial => !(0.0..=1.0).contains(&m),
                    Family::Poisson | Family::Gamma => m <= 0.0 && !matches!(self.link, Link::Log),
                    Family::Gaussian => false,
                };
                if outside { f64::NAN } else { self.family.clamp_mean(m) }
            })
            .collect()
    }

    fn working_weight(&self, i: usize, eta: f64, mu: f64) -> f64 {
        let d = self.link.mu_eta(eta);
        self.weights[i] * d * d / self.family.variance(mu).max(1e-300)
    }

    fn irls(&self, max_iterations: usize) -> Result<IrlsResult> {
        let (family, link, n, p, y) = (self.family, self.link, self.n, self.p, self.y);
        // Starting means inside the range of the link. A Gaussian response
        // may be zero or negative, which the log link cannot take: start such
        // rows at a tenth of the mean absolute response instead.
        let floor = {
            let scale = y.iter().map(|v| v.abs()).sum::<f64>() / n as f64;
            if scale > 0.0 { 0.1 * scale } else { 0.1 }
        };
        let mut mu: Vec<f64> = (0..n)
            .map(|i| match family {
                Family::Binomial => (self.weights[i] * y[i] + 0.5) / (self.weights[i] + 1.0),
                Family::Poisson => y[i] + 0.1,
                Family::Gaussian if link == Link::Log => y[i].max(floor),
                Family::Gaussian | Family::Gamma => y[i],
            })
            .collect();
        let mut eta: Vec<f64> = mu.iter().map(|&m| link.link(m)).collect();
        let mut deviance_old = f64::INFINITY;
        let mut beta: Option<Vec<f64>> = None;
        let mut weights = vec![1.0; n];
        let mut flat_steps = 0;
        for iteration in 1..=max_iterations.max(1) {
            let mut wx = Matrix::zeros(n, p);
            let mut wz = vec![0.0; n];
            for i in 0..n {
                let d = link.mu_eta(eta[i]);
                let d = if d.abs() < 1e-300 { 1e-300f64.copysign(d) } else { d };
                let w = self.working_weight(i, eta[i], mu[i]).max(1e-300);
                weights[i] = w;
                let z = eta[i] + (y[i] - mu[i]) / d;
                let sw = w.sqrt();
                for j in 0..p {
                    wx[(i, j)] = sw * (self.design)(i, j);
                }
                wz[i] = sw * z;
            }
            let qr = QrDecomposition::new(wx)?;
            if qr.rank(qr.default_rank_tolerance()) < p {
                return Err(StatError::RankDeficient { rank: qr.rank(qr.default_rank_tolerance()), columns: p });
            }
            let mut candidate = qr.solve_least_squares(&wz)?;
            let (mut new_eta, mut new_mu) = self.means(&candidate);
            let mut deviance = self.deviance(&new_mu);
            // Halve steps that leave the link's domain or the family's mean
            // range (non-finite deviance or mean), and steps that raise the
            // deviance, so the iterations descend (Marschner 2011, the
            // `glm2` step-halving rule); canonical links rarely need either.
            // Before any estimate exists there is no previous β to halve
            // toward, so the linear predictors are pulled back toward the
            // starting ones (valid means) instead, and the next iteration
            // restarts from there.
            let mut halvings = 0;
            let mut pulled_back = false;
            while !deviance.is_finite()
                || new_mu.iter().any(|m| !m.is_finite())
                || deviance > deviance_old * (1.0 + 1e-12) + 1e-300
            {
                if halvings == 30 {
                    break;
                }
                halvings += 1;
                if let Some(previous) = &beta {
                    candidate = candidate.iter().zip(previous).map(|(a, b)| 0.5 * (a + b)).collect();
                    (new_eta, new_mu) = self.means(&candidate);
                } else {
                    new_eta = new_eta.iter().zip(&eta).map(|(a, b)| 0.5 * (a + b)).collect();
                    new_mu = self.means_of(&new_eta);
                    pulled_back = true;
                }
                deviance = self.deviance(&new_mu);
            }
            if !deviance.is_finite() {
                return Err(StatError::ConvergenceFailure {
                    routine: "GLM iteratively reweighted least squares",
                    iterations: iteration,
                });
            }
            eta = new_eta;
            mu = new_mu;
            if pulled_back {
                // The pulled-back point is not a fit of the model (it may lie
                // nearer the data than any fit does), so the next step need
                // only be valid, not lower its deviance.
                continue;
            }
            // Non-canonical links converge linearly, so the estimates must
            // also have settled, not only the deviance.
            let settled = beta
                .as_ref()
                .is_some_and(|old| old.iter().zip(&candidate).all(|(a, b)| (a - b).abs() <= 1e-10 * (b.abs() + 1e-6)));
            beta = Some(candidate);
            let flat = (deviance - deviance_old).abs() < 1e-10 * (deviance.abs() + 0.1);
            flat_steps = if flat { flat_steps + 1 } else { 0 };
            // Estimates drifting off with a flat deviance (separation) stop
            // after a few steps; the caller's warning flags them.
            if flat && (settled || flat_steps >= 5) {
                // Weights at the solution, for the covariance.
                for i in 0..n {
                    weights[i] = self.working_weight(i, eta[i], mu[i]).max(1e-300);
                }
                return Ok((beta.unwrap_or_default(), mu, iteration, weights));
            }
            deviance_old = deviance;
        }
        Err(StatError::ConvergenceFailure {
            routine: "GLM iteratively reweighted least squares",
            iterations: max_iterations,
        })
    }

    fn log_likelihood(&self, mu: &[f64], deviance: f64) -> f64 {
        let (y, w) = (self.y, self.weights);
        match self.family {
            Family::Binomial => (0..self.n)
                .map(|i| {
                    let (m, r) = (w[i], w[i] * y[i]);
                    let choose = ln_gamma(m + 1.0) - ln_gamma(r + 1.0) - ln_gamma(m - r + 1.0);
                    let a = if r > 0.0 { r * mu[i].ln() } else { 0.0 };
                    let b = if m - r > 0.0 { (m - r) * (1.0 - mu[i]).ln() } else { 0.0 };
                    choose + a + b
                })
                .sum(),
            Family::Poisson => (0..self.n).map(|i| w[i] * (y[i] * mu[i].ln() - mu[i] - ln_gamma(y[i] + 1.0))).sum(),
            Family::Gaussian => {
                let n = self.n as f64;
                let rss = deviance;
                -0.5 * n * ((2.0 * std::f64::consts::PI * rss / n).ln() + 1.0)
                    + 0.5 * w.iter().map(|v| v.ln()).sum::<f64>()
            }
            Family::Gamma => {
                // Shape 1/φ̃ with φ̃ = D / Σw (R's Gamma()$aic).
                let shape = w.iter().sum::<f64>() / deviance.max(1e-300);
                (0..self.n)
                    .map(|i| {
                        let rate = shape / mu[i];
                        w[i] * (shape * rate.ln() - ln_gamma(shape) + (shape - 1.0) * y[i].ln() - rate * y[i])
                    })
                    .sum()
            }
        }
    }
}

/// Likelihood-ratio test of a reduced model nested in a full one (same data).
///
/// # Errors
/// [`StatError::InvalidArgument`] if the families or links differ or the
/// reduced model does not have fewer parameters.
pub fn likelihood_ratio_test(full: &GlmFit, reduced: &GlmFit) -> Result<LikelihoodRatioTest> {
    if full.family != reduced.family || full.link != reduced.link || reduced.n_parameters() >= full.n_parameters() {
        return Err(StatError::InvalidArgument {
            message: "the reduced model must be nested in the full model".into(),
        });
    }
    let df = full.n_parameters() - reduced.n_parameters();
    let chi_square = ((reduced.deviance - full.deviance) / full.dispersion).max(0.0);
    Ok(LikelihoodRatioTest { chi_square, df, p_value: ChiSquared::new(df as f64)?.sf(chi_square)? })
}

#[cfg(test)]
mod tests;
