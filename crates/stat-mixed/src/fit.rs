//! Fitting: REML or ML for the normal family; Laplace or adaptive
//! Gauss–Hermite approximations to the likelihood for the others.

use stat_core::distributions::{ChiSquared, ContinuousDistribution, FisherSnedecor, Normal, StudentsT};
use stat_core::quadrature::gauss_hermite;
use stat_core::{Matrix, QrDecomposition, Result, StatError};

use crate::dense::{Square, cholesky, inverse_from_cholesky, spd_inverse, symmetric_eigen};
use crate::family::MixedFamily;
use crate::optimize::{bfgs, hessian, nelder_mead};
use crate::system::{Structure, Term};

/// One random-effects term: a grouping column, and the effects that vary by
/// its levels (an intercept and/or slopes).
#[derive(Debug, Clone, PartialEq)]
pub struct RandomTerm {
    /// Name of the grouping (e.g. `"Subject"` or `"School/Class"`).
    pub group: String,
    /// Level names.
    pub levels: Vec<String>,
    /// Level of each row (an index into `levels`).
    pub codes: Vec<usize>,
    /// Names of the effects (columns of `z`), e.g. `["Intercept", "Days"]`.
    pub effects: Vec<String>,
    /// `n × q` design of the random effects (a column of ones for an
    /// intercept, the slope variable for a slope).
    pub z: Matrix,
    /// Estimate correlations between the effects (an unstructured
    /// covariance); otherwise they are independent (diagonal covariance).
    pub correlated: bool,
}

/// A mixed model's data: response, fixed-effects design, prior weights,
/// offset and random-effects terms.
#[derive(Debug, Clone, PartialEq)]
pub struct MixedModel {
    /// Response (a proportion for the binomial family).
    pub y: Vec<f64>,
    /// Fixed-effects design, including any intercept column.
    pub x: Matrix,
    /// Names of the columns of `x`.
    pub x_names: Vec<String>,
    /// Prior weights (binomial: number of trials); empty means all 1.
    pub weights: Vec<f64>,
    /// Offset on the linear-predictor scale; empty means 0.
    pub offset: Vec<f64>,
    /// Random-effects terms (at least one).
    pub random: Vec<RandomTerm>,
    /// Response distribution.
    pub family: MixedFamily,
    /// Normal family: a residual variance per level of a grouping
    /// (`σ²δ²_g`, with `δ = 1` for the first level), as `nlme`'s `varIdent`
    /// (Pinheiro & Bates 2000, §5.2); `None` for one residual variance.
    pub residual_groups: Option<ResidualGroups>,
    /// Ordinal family: the category names, lowest first (`y` holds codes
    /// into it); empty otherwise.
    pub categories: Vec<String>,
}

/// Levels of the grouping whose residual variances differ.
#[derive(Debug, Clone, PartialEq)]
pub struct ResidualGroups {
    /// Name of the grouping.
    pub name: String,
    /// Level names; the first is the reference (`δ = 1`).
    pub levels: Vec<String>,
    /// Level of each row (an index into `levels`).
    pub codes: Vec<usize>,
}

/// Denominator degrees of freedom of the normal family's t and F tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DfMethod {
    /// Satterthwaite's approximation (Kuznetsova et al. 2017).
    #[default]
    Satterthwaite,
    /// Between–within ("containment") degrees of freedom for nested
    /// groupings (Pinheiro & Bates 2000, §2.4.2): an effect that is constant
    /// within the groups of level ℓ gets `Gℓ − Gℓ₋₁ − pℓ`; one that varies
    /// within the innermost groups (and the intercept) gets `N − G_L − p`.
    BetweenWithin,
}

/// Fitting options.
#[derive(Debug, Clone, PartialEq)]
pub struct MixedOptions {
    /// Normal family: restricted maximum likelihood (REML); otherwise ML.
    pub reml: bool,
    /// Other families: quadrature points per random effect; 1 is the Laplace
    /// approximation. More than 1 (adaptive Gauss–Hermite) needs a single
    /// term with one effect.
    pub quadrature_points: usize,
    /// Confidence level of intervals.
    pub confidence_level: f64,
    /// Limit on objective evaluations per optimisation stage.
    pub max_evaluations: usize,
    /// Normal family: replace the model-based standard errors of the fixed
    /// effects by cluster-robust (sandwich) ones, clustered on the outermost
    /// grouping, with the CR2 small-sample correction and Bell–McCaffrey
    /// degrees of freedom (see [`RobustSummary`]).
    pub robust: bool,
    /// Normal family: denominator degrees of freedom of model-based tests.
    pub df_method: DfMethod,
}

impl Default for MixedOptions {
    fn default() -> Self {
        Self {
            reml: true,
            quadrature_points: 1,
            confidence_level: 0.95,
            max_evaluations: 20_000,
            robust: false,
            df_method: DfMethod::Satterthwaite,
        }
    }
}

/// A likelihood-ratio test of equal residual variances.
#[derive(Debug, Clone, PartialEq)]
pub struct VarianceTest {
    /// `−2 log Λ`: the null model's criterion minus the full model's (both
    /// REML, or both ML).
    pub statistic: f64,
    /// Degrees of freedom: levels − 1.
    pub df: usize,
    /// Upper-tail χ² p-value.
    pub p_value: f64,
}

/// Cluster-robust (sandwich) standard errors of the fixed effects.
///
/// With `V̂` the fitted marginal covariance, `M = (XᵀV̂⁻¹X)⁻¹`, residuals
/// `eⱼ = yⱼ − Xⱼβ̂` and clusters `j` (levels of the outermost grouping, which
/// every other grouping is nested in), the estimate is
/// `M [Σⱼ XⱼᵀV̂ⱼ⁻¹AⱼeⱼeⱼᵀAⱼᵀV̂ⱼ⁻¹Xⱼ] M` (Liang & Zeger 1986). The CR2
/// adjustment `Aⱼ = Dⱼᵀ Bⱼ^{−½} Dⱼ`, with `V̂ⱼ = DⱼᵀDⱼ` and
/// `Bⱼ = Dⱼ(V̂ⱼ − XⱼMXⱼᵀ)Dⱼᵀ`, makes it exactly unbiased when the working
/// model is right (Bell & McCaffrey 2002; Pustejovsky & Tipton 2018). Tests
/// of single coefficients use Satterthwaite degrees of freedom of the
/// estimate under the working model, and tests of several the Hotelling T²
/// approximation ("HTZ", Pustejovsky & Tipton 2018, §3.2).
#[derive(Debug, Clone, PartialEq)]
pub struct RobustSummary {
    /// Grouping whose levels are the clusters.
    pub cluster: String,
    /// Number of clusters.
    pub n_clusters: usize,
    /// Model-based standard errors of the fixed effects, for comparison.
    pub model_std_errors: Vec<f64>,
    /// Model-based covariance of the fixed effects.
    pub model_covariance: Matrix,
}

/// What the CR2 tests need for each cluster.
#[derive(Debug, Clone, PartialEq)]
struct RobustCluster {
    /// `Gⱼ = AⱼᵀV̂ⱼ⁻¹XⱼM` (`nⱼ × p`, row-major).
    g: Vec<f64>,
    /// `V̂ⱼ` (unscaled, `nⱼ × nⱼ`).
    v: Square,
    /// Rows of `X` (`nⱼ × p`, row-major).
    x: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RobustParts {
    /// Unscaled `M`.
    m: Square,
    clusters: Vec<RobustCluster>,
}

/// A fixed effect with its test and interval.
#[derive(Debug, Clone, PartialEq)]
pub struct FixedEffect {
    /// Column name.
    pub name: String,
    /// Estimate.
    pub estimate: f64,
    /// Standard error.
    pub std_error: f64,
    /// Denominator degrees of freedom (Satterthwaite) for the normal family;
    /// `None` for a large-sample z test.
    pub df: Option<f64>,
    /// t (with `df`) or z statistic.
    pub statistic: f64,
    /// Two-sided p-value.
    pub p_value: f64,
    /// Lower confidence limit.
    pub lower: f64,
    /// Upper confidence limit.
    pub upper: f64,
}

/// A variance of a random effect (or the residual variance).
#[derive(Debug, Clone, PartialEq)]
pub struct VarianceComponent {
    /// Grouping (`"Residual"` for the residual).
    pub group: String,
    /// Effect (`"Intercept"`, a slope, or empty for the residual).
    pub effect: String,
    /// Variance.
    pub variance: f64,
    /// Standard deviation.
    pub std_dev: f64,
    /// Large-sample (Wald) standard error of the variance, when available.
    pub std_error: Option<f64>,
    /// Confidence limits of the variance, when available: Satterthwaite's
    /// χ² limits from the estimate and its large-sample standard error (see
    /// [`satterthwaite_limits`]), which stay positive where the symmetric
    /// Wald limits `σ² ± z·se` would cross zero.
    pub interval: Option<(f64, f64)>,
}

/// A correlation between two random effects of one term.
#[derive(Debug, Clone, PartialEq)]
pub struct RandomCorrelation {
    /// Grouping.
    pub group: String,
    /// First effect.
    pub first: String,
    /// Second effect.
    pub second: String,
    /// Correlation.
    pub value: f64,
}

/// Conditional modes (BLUPs for the normal family) of one term's random
/// effects, with their conditional standard deviations.
#[derive(Debug, Clone, PartialEq)]
pub struct ConditionalModes {
    /// Grouping.
    pub group: String,
    /// Effect names.
    pub effects: Vec<String>,
    /// Level names.
    pub levels: Vec<String>,
    /// `[level][effect]`.
    pub values: Vec<Vec<f64>>,
    /// `[level][effect]`; NaN when not computed (very large models).
    pub std_errors: Vec<Vec<f64>>,
}

/// The family's extra scale parameter (Gamma dispersion, negative binomial
/// shape).
#[derive(Debug, Clone, PartialEq)]
pub struct ScaleParameter {
    /// Name.
    pub name: String,
    /// Estimate.
    pub value: f64,
    /// Large-sample standard error, when available.
    pub std_error: Option<f64>,
}

/// A test of a linear hypothesis `Lβ = 0` on the fixed effects.
#[derive(Debug, Clone, PartialEq)]
pub struct ContrastTest {
    /// Numerator degrees of freedom (rows of `L`, after dropping dependent rows).
    pub num_df: usize,
    /// Denominator degrees of freedom (Satterthwaite) for an F test; `None`
    /// for a χ² (Wald) test.
    pub den_df: Option<f64>,
    /// F (with `den_df`) or χ² statistic.
    pub statistic: f64,
    /// p-value.
    pub p_value: f64,
}

/// What Satterthwaite's approximation needs: the large-sample covariance of
/// the variance parameters and the derivatives of Cov(β̂) with respect to them.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Satterthwaite {
    a: Square,
    grads: Vec<Square>,
    /// Derivatives of each term's random-effects covariance (`q × q`), per
    /// direction of `grads`.
    g_grads: Vec<Vec<Square>>,
}

/// A fitted mixed model.
#[derive(Debug, Clone, PartialEq)]
pub struct MixedFit {
    /// Response distribution.
    pub family: MixedFamily,
    /// Estimation method, e.g. `"REML"` or `"Laplace"`.
    pub method: String,
    /// Observations used.
    pub n_obs: usize,
    /// Fixed effects.
    pub fixed: Vec<FixedEffect>,
    /// Covariance of the fixed-effect estimates.
    pub fixed_covariance: Matrix,
    /// Random-effect variances, term by term.
    pub variance_components: Vec<VarianceComponent>,
    /// Correlations within correlated terms.
    pub correlations: Vec<RandomCorrelation>,
    /// Residual variance (normal family).
    pub residual: Option<VarianceComponent>,
    /// Gamma dispersion or negative binomial shape.
    pub scale: Option<ScaleParameter>,
    /// −2 log likelihood (the REML criterion for a REML fit).
    pub minus_two_log_likelihood: f64,
    /// Akaike's information criterion.
    pub aic: f64,
    /// Schwarz's Bayesian information criterion.
    pub bic: f64,
    /// Parameters counted by the information criteria.
    pub n_parameters: usize,
    /// Number of levels of each term's grouping.
    pub n_levels: Vec<(String, usize)>,
    /// Conditional modes of the random effects.
    pub modes: Vec<ConditionalModes>,
    /// Linear predictor including the random effects.
    pub linear_predictor: Vec<f64>,
    /// Mean including the random effects.
    pub fitted: Vec<f64>,
    /// Mean from the fixed effects (and offset) only.
    pub fitted_marginal: Vec<f64>,
    /// Covariance parameters (lower-triangular factors of each term, relative
    /// to the residual SD for the normal family).
    pub theta: Vec<f64>,
    /// The optimiser met its convergence criteria.
    pub converged: bool,
    /// Problems worth reporting (singular fit, convergence, …).
    pub warnings: Vec<String>,
    /// Objective evaluations.
    pub evaluations: usize,
    /// Normal family: large-sample (delta-method) covariance of the
    /// variances in `variance_components` followed by the residual variance,
    /// in that order, from twice the inverse Hessian of the unprofiled REML
    /// (or ML) deviance. Sums of components (for example a gauge R&R total)
    /// get their standard error as `√(1ᵀ C 1)` over the summed rows. `None`
    /// for other families or when the curvature is not positive definite.
    pub variance_covariance: Option<Matrix>,
    /// Residual variances by level when they differ by a grouping (the
    /// first is `residual`); empty otherwise.
    pub residual_by_group: Vec<VarianceComponent>,
    /// Likelihood-ratio test of equal residual variances, when they differ
    /// by a grouping.
    pub equal_variance_test: Option<VarianceTest>,
    /// Cluster-robust standard errors, when asked for (`fixed` and
    /// `fixed_covariance` then hold the robust results).
    pub robust: Option<RobustSummary>,
    /// Normal family: how the denominator degrees of freedom were found
    /// (`"Satterthwaite"`, `"between–within"`, `"Bell–McCaffrey (CR2)"` or
    /// `"residual"`).
    pub df_label: String,
    /// Ordinal family: thresholds `α₁ … α_{K−1}`, named `"low|high"` after
    /// the categories they separate (Wald z tests); empty otherwise.
    pub thresholds: Vec<FixedEffect>,
    /// Ordinal family: covariance of `(α, β)`, thresholds first.
    pub ordinal_covariance: Option<Matrix>,
    /// Ordinal family: category probabilities of each row with the random
    /// effects at their conditional modes.
    pub category_probabilities: Vec<Vec<f64>>,
    /// Ordinal family: category probabilities with the random effects at 0.
    pub category_probabilities_marginal: Vec<Vec<f64>>,
    pub(crate) confidence_level: f64,
    pub(crate) satterthwaite: Option<Satterthwaite>,
    /// Between–within degrees of freedom per fixed effect, when used.
    pub(crate) between_within: Option<Vec<f64>>,
    pub(crate) robust_parts: Option<RobustParts>,
}

/// Smallest log dispersion (or log shape) the optimiser may reach.
const MIN_LOG_SCALE: f64 = -20.0;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Everything fixed during the optimisation.
pub(crate) struct Problem<'a> {
    pub model: &'a MixedModel,
    pub st: Structure<'a>,
    pub w: Vec<f64>,
    pub offset: Vec<f64>,
    pub n: usize,
    pub p: usize,
}

/// PIRLS output.
#[derive(Debug, Clone)]
pub(crate) struct Mode {
    pub u: Vec<f64>,
    pub beta: Vec<f64>,
    pub eta: Vec<f64>,
    pub mu: Vec<f64>,
    /// `Σ log f(y | μ)`.
    pub log_f: f64,
    /// `log|ΛᵀZᵀWZΛ + I|` at the mode's weights.
    pub logdet_u: f64,
    /// Working weights at the mode.
    pub weights: Vec<f64>,
}

impl Problem<'_> {
    fn family(&self) -> MixedFamily {
        self.model.family
    }

    // ---- normal family -------------------------------------------------

    /// Number of residual variance ratios estimated (levels − 1).
    fn n_ratios(&self) -> usize {
        self.model.residual_groups.as_ref().map_or(0, |g| g.levels.len().saturating_sub(1))
    }

    /// Prior weights divided by the squared variance ratios `δ²_g` of the
    /// rows' residual groups (`log_ratios` holds `log δ` of levels 2, 3, …).
    fn lmm_weights(&self, log_ratios: &[f64]) -> Vec<f64> {
        match &self.model.residual_groups {
            None => self.w.clone(),
            Some(g) => (0..self.n)
                .map(|i| {
                    let c = g.codes[i];
                    let d = if c == 0 { 0.0 } else { log_ratios[c - 1] };
                    self.w[i] * (-2.0 * d).exp()
                })
                .collect(),
        }
    }

    /// Penalized least squares at θ with weights `w`: (solution, penalized RSS r²).
    fn lmm(&self, theta: &[f64], w: &[f64]) -> Option<(crate::system::Solution, f64)> {
        let a = self.st.a_rows(theta);
        let v: Vec<f64> = (0..self.n).map(|i| self.model.y[i] - self.offset[i]).collect();
        let sol = self.st.solve(&a, w, &v, true)?;
        let zu = self.st.random_part(&a, &sol.u);
        let xb = self.st.fixed_part(&sol.beta);
        let rss: f64 = (0..self.n).map(|i| w[i] * (v[i] - zu[i] - xb[i]).powi(2)).sum();
        let r2 = rss + sol.u.iter().map(|x| x * x).sum::<f64>();
        Some((sol, r2))
    }

    /// Profiled deviance (REML criterion or −2 log likelihood) at θ.
    fn lmm_profiled(&self, theta: &[f64], w: &[f64], reml: bool) -> f64 {
        let Some((sol, r2)) = self.lmm(theta, w) else { return f64::INFINITY };
        let two_pi = 2.0 * std::f64::consts::PI;
        let (n, p) = (self.n as f64, self.p as f64);
        let sum_log_w: f64 = w.iter().map(|w| w.ln()).sum();
        if reml {
            sol.logdet_u + sol.logdet_x + (n - p) * (1.0 + (two_pi * r2 / (n - p)).ln()) - sum_log_w
        } else {
            sol.logdet_u + n * (1.0 + (two_pi * r2 / n).ln()) - sum_log_w
        }
    }

    /// Deviance at (θ, σ) without profiling σ out.
    fn lmm_deviance(&self, theta: &[f64], w: &[f64], sigma: f64, reml: bool) -> f64 {
        let Some((sol, r2)) = self.lmm(theta, w) else { return f64::INFINITY };
        if sigma.is_nan() || sigma <= 0.0 {
            return f64::INFINITY;
        }
        let two_pi = 2.0 * std::f64::consts::PI;
        let m = if reml { (self.n - self.p) as f64 } else { self.n as f64 };
        let extra = if reml { sol.logdet_x } else { 0.0 };
        let sum_log_w: f64 = w.iter().map(|w| w.ln()).sum();
        sol.logdet_u + extra + m * (two_pi * sigma * sigma).ln() + r2 / (sigma * sigma) - sum_log_w
    }

    // ---- other families ------------------------------------------------

    /// Penalized iteratively reweighted least squares: the conditional mode
    /// of u (and β when `beta` is `None`) at θ and the family's scale.
    fn pirls(&self, theta: &[f64], beta: Option<&[f64]>, scale: f64, warm: Option<&Mode>) -> Option<Mode> {
        let fam = self.family();
        let (n, y) = (self.n, &self.model.y);
        let a = self.st.a_rows(theta);
        let fixed_xb: Option<Vec<f64>> = beta.map(|b| self.st.fixed_part(b));
        // Offset that is not solved for.
        let base: Vec<f64> = (0..n).map(|i| self.offset[i] + fixed_xb.as_ref().map_or(0.0, |f| f[i])).collect();
        let eta_of = |u: &[f64], b: &[f64]| -> Vec<f64> {
            let zu = self.st.random_part(&a, u);
            let xb = if beta.is_none() { self.st.fixed_part(b) } else { vec![0.0; n] };
            (0..n).map(|i| base[i] + zu[i] + xb[i]).collect()
        };
        let objective = |eta: &[f64], u: &[f64]| -> (f64, Vec<f64>) {
            let mu: Vec<f64> = eta.iter().map(|&e| fam.inverse_link(e)).collect();
            let lf: f64 = (0..n).map(|i| fam.log_density(y[i], mu[i], self.w[i], scale)).sum();
            (-2.0 * lf + u.iter().map(|x| x * x).sum::<f64>(), mu)
        };
        // Start: the warm mode, or the family's starting means.
        let (mut u, mut b, mut eta) = match warm {
            Some(m) => {
                let b = if beta.is_none() { m.beta.clone() } else { Vec::new() };
                let eta = eta_of(&m.u, &b);
                (m.u.clone(), b, eta)
            }
            None => {
                let eta: Vec<f64> = (0..n).map(|i| fam.link(fam.start_mean(y[i], self.w[i]))).collect();
                (vec![0.0; self.st.q_total], vec![0.0; if beta.is_none() { self.p } else { 0 }], eta)
            }
        };
        let mut pdev = if warm.is_some() { objective(&eta, &u).0 } else { f64::INFINITY };
        let mut converged = false;
        for _ in 0..100 {
            let mu: Vec<f64> = eta.iter().map(|&e| fam.inverse_link(e)).collect();
            let w: Vec<f64> = (0..n).map(|i| fam.working_weight(mu[i], self.w[i], scale)).collect();
            let z: Vec<f64> = (0..n).map(|i| eta[i] - base[i] + (y[i] - mu[i]) / fam.mu_eta(mu[i])).collect();
            let sol = self.st.solve(&a, &w, &z, beta.is_none())?;
            let (mut nu, mut nb) = (sol.u, sol.beta);
            let mut neta = eta_of(&nu, &nb);
            let (mut npdev, _) = objective(&neta, &nu);
            // Step halving when the penalized deviance does not fall.
            let mut halvings = 0;
            while (npdev.is_nan() || npdev > pdev + 1e-10 * pdev.abs().max(1.0)) && pdev.is_finite() && halvings < 30 {
                halvings += 1;
                for (x, o) in nu.iter_mut().zip(&u) {
                    *x = 0.5 * (*x + o);
                }
                for (x, o) in nb.iter_mut().zip(&b) {
                    *x = 0.5 * (*x + o);
                }
                neta = eta_of(&nu, &nb);
                npdev = objective(&neta, &nu).0;
            }
            if !npdev.is_finite() {
                return None;
            }
            let change = (pdev - npdev).abs();
            let step: f64 = nu.iter().zip(&u).chain(nb.iter().zip(&b)).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
            u = nu;
            b = nb;
            eta = neta;
            pdev = npdev;
            if change <= 1e-12 * (pdev.abs() + 1.0)
                && step <= 1e-9 * (1.0 + u.iter().chain(&b).map(|v| v.abs()).fold(0.0, f64::max))
            {
                converged = true;
                break;
            }
        }
        let _ = converged;
        let mu: Vec<f64> = eta.iter().map(|&e| fam.inverse_link(e)).collect();
        let weights: Vec<f64> = (0..n).map(|i| fam.working_weight(mu[i], self.w[i], scale)).collect();
        // The determinant at the mode's own weights.
        let zeros = vec![0.0; n];
        let sol = self.st.solve(&a, &weights, &zeros, false)?;
        let log_f: f64 = (0..n).map(|i| fam.log_density(y[i], mu[i], self.w[i], scale)).sum();
        let beta_out = match beta {
            Some(bv) => bv.to_vec(),
            None => b,
        };
        Some(Mode { u, beta: beta_out, eta, mu, log_f, logdet_u: sol.logdet_u, weights })
    }

    /// Laplace approximation to −2 log likelihood at a mode.
    pub(crate) fn laplace(mode: &Mode) -> f64 {
        -2.0 * mode.log_f + mode.u.iter().map(|x| x * x).sum::<f64>() + mode.logdet_u
    }

    /// Adaptive Gauss–Hermite approximation to −2 log likelihood (one term
    /// with one effect): each level's integral is centred at its conditional
    /// mode and scaled by its conditional SD.
    fn agq(&self, theta: &[f64], mode: &Mode, scale: f64, nodes: &(Vec<f64>, Vec<f64>)) -> f64 {
        let fam = self.family();
        let log_f = |i: usize, eta: f64| fam.log_density(self.model.y[i], fam.inverse_link(eta), self.w[i], scale);
        self.agq_with(theta, mode, nodes, &log_f)
    }

    /// [`Problem::agq`] with the log density of row `i` at linear predictor
    /// `η` given by `log_f(i, η)`.
    pub(crate) fn agq_with(
        &self,
        theta: &[f64],
        mode: &Mode,
        nodes: &(Vec<f64>, Vec<f64>),
        log_f: &dyn Fn(usize, f64) -> f64,
    ) -> f64 {
        let term = &self.st.terms[0];
        let a = self.st.a_rows(theta);
        let a0 = &a[0];
        let (xs, ws) = nodes;
        let mut total = 0.0;
        for (j, rows) in self.st.rows_by_level.iter().enumerate() {
            let uj = mode.u[term.u_start + j];
            let info: f64 = 1.0 + rows.iter().map(|&i| mode.weights[i] * a0[i] * a0[i]).sum::<f64>();
            let sd = 1.0 / info.sqrt();
            let h = |u: f64| -> f64 {
                let lf: f64 = rows.iter().map(|&i| log_f(i, mode.eta[i] + a0[i] * (u - uj))).sum();
                lf - 0.5 * u * u
            };
            let terms: Vec<f64> =
                xs.iter().zip(ws).map(|(x, w)| w.ln() + x * x + h(uj + std::f64::consts::SQRT_2 * sd * x)).collect();
            let mx = terms.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let lse = mx + terms.iter().map(|t| (t - mx).exp()).sum::<f64>().ln();
            total += (std::f64::consts::SQRT_2 * sd).ln() + lse - 0.5 * (2.0 * std::f64::consts::PI).ln();
        }
        -2.0 * total
    }
}

/// Fit a linear (normal family) or generalized linear mixed model.
///
/// # Errors
/// * [`StatError::DimensionMismatch`] when inputs disagree in length.
/// * [`StatError::InvalidArgument`] for responses outside the family's range,
///   no random-effects terms, bad level codes, or quadrature asked for a
///   model with more than one random effect per level.
/// * [`StatError::NonFiniteInput`] for NaN or infinite inputs.
/// * [`StatError::InsufficientData`] unless there are more rows than fixed effects.
/// * [`StatError::RankDeficient`] for collinear fixed effects.
pub fn fit_mixed(model: &MixedModel, options: &MixedOptions) -> Result<MixedFit> {
    let n = model.y.len();
    let p = model.x.ncols();
    if model.x.nrows() != n {
        return Err(StatError::DimensionMismatch {
            context: "fixed-effects rows vs. response",
            expected: n,
            found: model.x.nrows(),
        });
    }
    if model.x_names.len() != p {
        return Err(StatError::DimensionMismatch {
            context: "fixed-effect names",
            expected: p,
            found: model.x_names.len(),
        });
    }
    let w = if model.weights.is_empty() { vec![1.0; n] } else { model.weights.clone() };
    let offset = if model.offset.is_empty() { vec![0.0; n] } else { model.offset.clone() };
    if w.len() != n || offset.len() != n {
        return Err(StatError::DimensionMismatch {
            context: "weights or offset vs. response",
            expected: n,
            found: w.len().min(offset.len()),
        });
    }
    if !(options.confidence_level > 0.0 && options.confidence_level < 1.0) {
        return Err(invalid("the confidence level must lie in (0, 1)"));
    }
    if !model.x.is_finite() || offset.iter().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "mixed-model design" });
    }
    for i in 0..n {
        if let Some(why) = model.family.check(model.y[i], w[i]) {
            return Err(invalid(why));
        }
    }
    let constant = model.y.iter().all(|v| *v == model.y[0]);
    if model.family == MixedFamily::Ordinal {
        let k = model.categories.len();
        if k < 2 {
            return Err(invalid("an ordinal response needs two or more categories"));
        }
        if model.y.iter().any(|&y| y as usize >= k) {
            return Err(invalid("an ordinal response code is beyond the last category"));
        }
        if (0..p).any(|j| (0..n).all(|i| (model.x[(i, j)] - model.x[(0, j)]).abs() <= 1e-12)) {
            return Err(invalid(
                "an ordinal model has no intercept or other constant fixed effect: the thresholds take its place",
            ));
        }
    }
    if let Some(g) = &model.residual_groups {
        if model.family != MixedFamily::Normal {
            return Err(invalid("residual variances by group apply to normal responses only"));
        }
        if g.codes.len() != n {
            return Err(StatError::DimensionMismatch {
                context: "residual groups vs. response",
                expected: n,
                found: g.codes.len(),
            });
        }
        if g.levels.len() < 2 || g.codes.iter().any(|&c| c >= g.levels.len()) {
            return Err(invalid(format!("{} needs two or more levels for separate residual variances", g.name)));
        }
        if (0..g.levels.len()).any(|l| !g.codes.contains(&l)) {
            return Err(invalid(format!("every level of {} needs observations", g.name)));
        }
    }
    if constant && matches!(model.family, MixedFamily::Normal | MixedFamily::Gamma) {
        return Err(invalid("the response is constant, so its variance cannot be estimated"));
    }
    if model.random.is_empty() {
        return Err(invalid("a mixed model needs at least one random-effects term"));
    }
    if n <= p {
        return Err(StatError::InsufficientData { required: p + 1, found: n });
    }
    if p > 0 {
        let qr = QrDecomposition::new(model.x.clone())?;
        let rank = qr.rank(qr.default_rank_tolerance());
        if rank < p {
            return Err(StatError::RankDeficient { rank, columns: p });
        }
    }
    let mut terms = Vec::with_capacity(model.random.len());
    let (mut theta_start, mut u_start) = (0, 0);
    for t in &model.random {
        let q = t.effects.len();
        if q == 0 || t.z.ncols() != q {
            return Err(invalid(format!("the random term for {} has no effects or a mismatched design", t.group)));
        }
        if t.z.nrows() != n || t.codes.len() != n {
            return Err(StatError::DimensionMismatch {
                context: "random-effects rows vs. response",
                expected: n,
                found: t.codes.len(),
            });
        }
        if t.levels.is_empty() || t.codes.iter().any(|&c| c >= t.levels.len()) {
            return Err(invalid(format!("level codes of {} are out of range", t.group)));
        }
        if !t.z.is_finite() {
            return Err(StatError::NonFiniteInput { context: "random-effects design" });
        }
        let term = Term {
            q,
            g: t.levels.len(),
            codes: &t.codes,
            z: &t.z,
            correlated: t.correlated && q > 1,
            theta_start,
            u_start,
        };
        theta_start += term.n_theta();
        u_start += term.g * q;
        terms.push(term);
    }
    let agq = model.family != MixedFamily::Normal && options.quadrature_points > 1;
    if agq && (terms.len() != 1 || terms[0].q != 1) {
        return Err(invalid(
            "adaptive quadrature needs a single random-effects term with one effect; use the Laplace approximation",
        ));
    }
    if options.quadrature_points == 0 || options.quadrature_points > 25 {
        return Err(invalid("use 1 (Laplace) to 25 quadrature points"));
    }
    let st = Structure::new(n, &model.x, terms);
    let problem = Problem { model, st, w, offset, n, p };
    match model.family {
        MixedFamily::Normal => {
            let mut fit = fit_lmm(&problem, options)?;
            if let Some(g) = &model.residual_groups {
                // Likelihood ratio against one residual variance (same fixed
                // effects, so valid for REML too).
                let null_model = MixedModel { residual_groups: None, ..model.clone() };
                let null_options =
                    MixedOptions { robust: false, df_method: DfMethod::Satterthwaite, ..options.clone() };
                let null = fit_mixed(&null_model, &null_options)?;
                let df = g.levels.len() - 1;
                let statistic = (null.minus_two_log_likelihood - fit.minus_two_log_likelihood).max(0.0);
                let p_value = ChiSquared::new(df as f64).and_then(|d| d.sf(statistic)).unwrap_or(f64::NAN);
                fit.equal_variance_test = Some(VarianceTest { statistic, df, p_value });
            }
            Ok(fit)
        }
        MixedFamily::Ordinal => crate::ordinal::fit_ordinal(&problem, options),
        _ => fit_glmm(&problem, options),
    }
}

/// Satterthwaite (1946) confidence limits of a variance `v` with
/// large-sample standard error `se`: treating `ν·v̂/v` as χ² with
/// `ν = 2(v/se)²` degrees of freedom gives
/// `[ν·v / χ²₁₋α/₂(ν), ν·v / χ²α/₂(ν)]` (Burdick & Graybill 1992, §2;
/// Milliken & Johnson 2009, ch. 20). Unlike the Wald limits `v ± z·se` they
/// are positive and skewed to the right, as a variance's sampling
/// distribution is. `None` when `v` or `se` is not positive and finite (a
/// variance estimated at zero has no such interval).
#[must_use]
pub fn satterthwaite_limits(v: f64, se: f64, confidence: f64) -> Option<(f64, f64)> {
    if !(v > 0.0 && se > 0.0 && v.is_finite() && se.is_finite()) {
        return None;
    }
    let df = 2.0 * (v / se).powi(2);
    let chi = ChiSquared::new(df).ok()?;
    let a = (1.0 - confidence) / 2.0;
    let (lo_q, hi_q) = (chi.quantile(1.0 - a).ok()?, chi.quantile(a).ok()?);
    (lo_q > 0.0 && hi_q > 0.0).then(|| (df * v / lo_q, df * v / hi_q))
}

/// Wald test statistic `p` value and limits for one coefficient.
pub(crate) fn coefficient(name: &str, est: f64, se: f64, df: Option<f64>, level: f64) -> FixedEffect {
    let stat = est / se;
    let alpha = 1.0 - level;
    let (p, crit) = match df.and_then(|d| StudentsT::new(d).ok()) {
        Some(t) => (2.0 * t.sf(stat.abs()).unwrap_or(f64::NAN), t.quantile(1.0 - alpha / 2.0).unwrap_or(f64::NAN)),
        None => {
            let z = Normal::new(0.0, 1.0).expect("standard normal");
            (2.0 * z.sf(stat.abs()).unwrap_or(f64::NAN), z.quantile(1.0 - alpha / 2.0).unwrap_or(f64::NAN))
        }
    };
    FixedEffect {
        name: name.to_owned(),
        estimate: est,
        std_error: se,
        df,
        statistic: stat,
        p_value: p,
        lower: est - crit * se,
        upper: est + crit * se,
    }
}

/// Variance components and correlations from θ (scaled by `sigma2`), and
/// the variances alone as a flat list (for delta-method standard errors).
pub(crate) fn components(
    problem: &Problem<'_>,
    theta: &[f64],
    sigma2: f64,
) -> (Vec<VarianceComponent>, Vec<RandomCorrelation>, Vec<f64>) {
    let mut comps = Vec::new();
    let mut cors = Vec::new();
    let mut flat = Vec::new();
    for (k, term) in problem.st.terms.iter().enumerate() {
        let rt = &problem.model.random[k];
        let f = term.factor(theta);
        let q = term.q;
        let cov = |a: usize, b: usize| -> f64 { (0..q).map(|c| f[a * q + c] * f[b * q + c]).sum::<f64>() * sigma2 };
        for a in 0..q {
            let v = cov(a, a);
            flat.push(v);
            comps.push(VarianceComponent {
                group: rt.group.clone(),
                effect: rt.effects[a].clone(),
                variance: v,
                std_dev: v.max(0.0).sqrt(),
                std_error: None,
                interval: None,
            });
        }
        if term.correlated {
            for a in 0..q {
                for b in a + 1..q {
                    let d = (cov(a, a) * cov(b, b)).sqrt();
                    cors.push(RandomCorrelation {
                        group: rt.group.clone(),
                        first: rt.effects[a].clone(),
                        second: rt.effects[b].clone(),
                        value: if d > 0.0 { cov(a, b) / d } else { f64::NAN },
                    });
                }
            }
        }
    }
    (comps, cors, flat)
}

/// Delta-method standard errors: `J A Jᵀ` diagonal for `g(params)`.
pub(crate) fn delta_se(g: &dyn Fn(&[f64]) -> Vec<f64>, params: &[f64], vary: &[usize], a: &Square) -> Vec<f64> {
    let cov = delta_cov(g, params, vary, a);
    (0..cov.n).map(|i| cov.get(i, i).max(0.0).sqrt()).collect()
}

/// Delta-method covariance `J A Jᵀ` of `g(params)`, with the Jacobian `J`
/// by central differences in the directions `vary`.
fn delta_cov(g: &dyn Fn(&[f64]) -> Vec<f64>, params: &[f64], vary: &[usize], a: &Square) -> Square {
    let base = g(params);
    let m = vary.len();
    let mut jac = vec![vec![0.0; m]; base.len()];
    for (c, &i) in vary.iter().enumerate() {
        let h = 1e-6 * params[i].abs().max(1e-3);
        let mut p1 = params.to_vec();
        p1[i] += h;
        let mut p0 = params.to_vec();
        p0[i] -= h;
        let (g1, g0) = (g(&p1), g(&p0));
        for r in 0..base.len() {
            jac[r][c] = (g1[r] - g0[r]) / (2.0 * h);
        }
    }
    let k = base.len();
    let mut out = Square::zeros(k);
    for r in 0..k {
        for c in 0..k {
            let mut s = 0.0;
            for a_ in 0..m {
                for b in 0..m {
                    s += jac[r][a_] * a.get(a_, b) * jac[c][b];
                }
            }
            *out.at(r, c) = s;
        }
    }
    out
}

/// Indices of θ that are free to vary for second-order inference: entries of
/// columns of `T` whose diagonal is not at its bound.
pub(crate) fn interior_theta(problem: &Problem<'_>, theta: &[f64]) -> Vec<usize> {
    let mut out = Vec::new();
    for term in &problem.st.terms {
        let diag = term.diagonal_indices();
        if term.correlated {
            let mut k = term.theta_start;
            for (j, &d) in diag.iter().enumerate() {
                let len = term.q - j;
                if theta[d] > 1e-6 {
                    out.extend(k..k + len);
                }
                k += len;
            }
        } else {
            out.extend(diag.into_iter().filter(|&d| theta[d] > 1e-6));
        }
    }
    out
}

pub(crate) fn singular_warning(problem: &Problem<'_>, theta: &[f64]) -> Option<String> {
    let lower = problem.st.theta_lower();
    let at_zero = lower.iter().zip(theta).any(|(l, t)| *l == 0.0 && *t <= 1e-6);
    at_zero.then(|| {
        "Singular fit: a random-effect variance is zero or two random effects are perfectly correlated; consider \
         fewer random effects."
            .to_owned()
    })
}

pub(crate) fn modes_of(
    problem: &Problem<'_>,
    theta: &[f64],
    u: &[f64],
    blocks: &[Vec<Square>],
    sigma2: f64,
) -> Vec<ConditionalModes> {
    problem
        .st
        .terms
        .iter()
        .enumerate()
        .map(|(k, term)| {
            let rt = &problem.model.random[k];
            let f = term.factor(theta);
            let q = term.q;
            let mut values = Vec::with_capacity(term.g);
            let mut ses = Vec::with_capacity(term.g);
            for j in 0..term.g {
                let uj = &u[term.u_start + j * q..term.u_start + (j + 1) * q];
                values.push((0..q).map(|a| (0..q).map(|c| f[a * q + c] * uj[c]).sum()).collect());
                let blk = &blocks[k][j];
                ses.push(
                    (0..q)
                        .map(|a| {
                            let mut s = 0.0;
                            for c in 0..q {
                                for d in 0..q {
                                    s += f[a * q + c] * blk.get(c, d) * f[a * q + d];
                                }
                            }
                            (s * sigma2).max(0.0).sqrt()
                        })
                        .collect(),
                );
            }
            ConditionalModes {
                group: rt.group.clone(),
                effects: rt.effects.clone(),
                levels: rt.levels.clone(),
                values,
                std_errors: ses,
            }
        })
        .collect()
}

pub(crate) fn n_levels(problem: &Problem<'_>) -> Vec<(String, usize)> {
    problem.model.random.iter().map(|t| (t.group.clone(), t.levels.len())).collect()
}

pub(crate) fn to_matrix(s: &Square) -> Matrix {
    let mut m = Matrix::zeros(s.n, s.n);
    for i in 0..s.n {
        for j in 0..s.n {
            m[(i, j)] = s.get(i, j);
        }
    }
    m
}

/// The `q × q` random-effects covariance of each term at θ, scaled by `sigma2`.
fn term_covariances(problem: &Problem<'_>, theta: &[f64], sigma2: f64) -> Vec<Square> {
    problem
        .st
        .terms
        .iter()
        .map(|t| {
            let f = t.factor(theta);
            let q = t.q;
            let mut g = Square::zeros(q);
            for a in 0..q {
                for b in 0..q {
                    *g.at(a, b) = (0..q).map(|c| f[a * q + c] * f[b * q + c]).sum::<f64>() * sigma2;
                }
            }
            g
        })
        .collect()
}

fn fit_lmm(problem: &Problem<'_>, options: &MixedOptions) -> Result<MixedFit> {
    let reml = options.reml;
    let st = &problem.st;
    let (n, p, nt) = (problem.n, problem.p, st.n_theta);
    let nr = problem.n_ratios();
    // Parameters: θ, then log δ of residual groups 2, 3, ….
    let mut lower = st.theta_lower();
    lower.extend(std::iter::repeat_n(f64::NEG_INFINITY, nr));
    let mut start = st.theta_start();
    start.extend(std::iter::repeat_n(0.0, nr));
    let steps: Vec<f64> = start
        .iter()
        .enumerate()
        .map(|(i, v)| {
            if i >= nt {
                0.3
            } else if *v == 1.0 {
                0.5
            } else {
                0.25
            }
        })
        .collect();
    let mut evals = 0;
    let mut objective = |x: &[f64]| {
        evals += 1;
        problem.lmm_profiled(&x[..nt], &problem.lmm_weights(&x[nt..]), reml)
    };
    let mut min = nelder_mead(&mut objective, &start, &lower, &steps, options.max_evaluations);
    if nr > 0 {
        // A restart from the optimum: more parameters, more chance to stall.
        let steps: Vec<f64> = min.x.iter().map(|v| 0.1 * v.abs().max(0.1)).collect();
        let again = nelder_mead(&mut objective, &min.x, &lower, &steps, options.max_evaluations);
        if again.f <= min.f {
            min = again;
        }
    }
    let theta = min.x[..nt].to_vec();
    let w_hat = problem.lmm_weights(&min.x[nt..]);
    let (sol, r2) = problem
        .lmm(&theta, &w_hat)
        .ok_or(StatError::ConvergenceFailure { routine: "linear mixed model", iterations: evals })?;
    let denom = if reml { (n - p) as f64 } else { n as f64 };
    let sigma2 = r2 / denom;
    let sigma = sigma2.sqrt();
    let unscaled = st.beta_unscaled_covariance(&sol);
    let mut cov_beta = unscaled.clone();
    cov_beta.a.iter_mut().for_each(|v| *v *= sigma2);

    // Satterthwaite: ψ = (θ, log δ, σ); A = 2 H⁻¹ of the unprofiled deviance.
    let mut psi = min.x.clone();
    psi.push(sigma);
    let mut psi_lower = lower.clone();
    psi_lower.push(0.0);
    let mut vary = interior_theta(problem, &theta);
    vary.extend(nt..nt + nr);
    vary.push(nt + nr);
    let split = |x: &[f64]| -> (Vec<f64>, Vec<f64>, f64) {
        (x[..nt].to_vec(), problem.lmm_weights(&x[nt..nt + nr]), x[nt + nr])
    };
    let mut dev = |x: &[f64]| {
        let (th, w, s) = split(x);
        problem.lmm_deviance(&th, &w, s, reml)
    };
    let h = hessian(&mut dev, &psi, &psi_lower, &vary);
    let mut warnings = Vec::new();
    let a = spd_inverse(&h).map(|mut inv| {
        inv.a.iter_mut().for_each(|v| *v *= 2.0);
        inv
    });
    let cov_of = |x: &[f64]| -> Option<Square> {
        let (th, w, s) = split(x);
        let (sol, _) = problem.lmm(&th, &w)?;
        let mut c = st.beta_unscaled_covariance(&sol);
        c.a.iter_mut().for_each(|v| *v *= s * s);
        Some(c)
    };
    let satterthwaite = a.as_ref().and_then(|a| {
        let mut grads = Vec::with_capacity(vary.len());
        let mut g_grads = Vec::with_capacity(vary.len());
        for &i in &vary {
            let hstep = 1e-4 * psi[i].abs().max(0.05);
            let mut up = psi.clone();
            up[i] += hstep;
            let mut dn = psi.clone();
            dn[i] -= hstep;
            let (cu, cd) = (cov_of(&up)?, cov_of(&dn)?);
            let mut g = Square::zeros(p);
            for (k, v) in g.a.iter_mut().enumerate() {
                *v = (cu.a[k] - cd.a[k]) / (2.0 * hstep);
            }
            grads.push(g);
            let gu = term_covariances(problem, &up[..nt], up[nt + nr] * up[nt + nr]);
            let gd = term_covariances(problem, &dn[..nt], dn[nt + nr] * dn[nt + nr]);
            g_grads.push(
                gu.iter()
                    .zip(&gd)
                    .map(|(u, d)| Square {
                        n: u.n,
                        a: u.a.iter().zip(&d.a).map(|(x, y)| (x - y) / (2.0 * hstep)).collect(),
                    })
                    .collect(),
            );
        }
        Some(Satterthwaite { a: a.clone(), grads, g_grads })
    });
    if satterthwaite.is_none() {
        warnings.push(
            "The curvature of the likelihood is not positive definite: denominator degrees of freedom use the residual \
             degrees of freedom, and variance-component standard errors are not shown."
                .to_owned(),
        );
    }
    let mut df_label = if satterthwaite.is_some() { "Satterthwaite" } else { "residual" };
    let mut between_within = None;
    if options.df_method == DfMethod::BetweenWithin {
        match between_within_df(problem) {
            Some(d) => {
                between_within = Some(d);
                df_label = "between–within";
            }
            None => warnings.push(
                "Between–within degrees of freedom need groupings nested in one another; Satterthwaite's are used."
                    .to_owned(),
            ),
        }
    }
    let mut fixed: Vec<FixedEffect> = (0..p)
        .map(|j| {
            let se = cov_beta.get(j, j).sqrt();
            let df = match (&between_within, &satterthwaite) {
                (Some(bw), _) => Some(bw[j]),
                (None, Some(s)) => satterthwaite_df(s, &cov_beta, &unit(p, j)),
                (None, None) => Some((n - p) as f64),
            };
            coefficient(&problem.model.x_names[j], sol.beta[j], se, df, options.confidence_level)
        })
        .collect();

    let (mut comps, cors, _) = components(problem, &theta, sigma2);
    let groups = problem.model.residual_groups.as_ref();
    let mut residual = VarianceComponent {
        group: "Residual".to_owned(),
        effect: groups.map_or_else(String::new, |g| g.levels[0].clone()),
        variance: sigma2,
        std_dev: sigma,
        std_error: None,
        interval: None,
    };
    let mut residual_by_group: Vec<VarianceComponent> = match groups {
        None => Vec::new(),
        Some(g) => g
            .levels
            .iter()
            .enumerate()
            .map(|(l, name)| {
                let v = if l == 0 { sigma2 } else { sigma2 * (2.0 * min.x[nt + l - 1]).exp() };
                VarianceComponent {
                    group: "Residual".to_owned(),
                    effect: name.clone(),
                    variance: v,
                    std_dev: v.sqrt(),
                    std_error: None,
                    interval: None,
                }
            })
            .collect(),
    };
    let mut variance_covariance = None;
    if let Some(a) = &a {
        let g = |x: &[f64]| -> Vec<f64> {
            let s = x[nt + nr];
            let (_, _, mut flat) = components(problem, &x[..nt], s * s);
            flat.push(s * s);
            flat.extend((0..nr).map(|r| s * s * (2.0 * x[nt + r]).exp()));
            flat
        };
        let cov = delta_cov(&g, &psi, &vary, a);
        let ses: Vec<f64> = (0..cov.n).map(|i| cov.get(i, i).max(0.0).sqrt()).collect();
        for (c, se) in comps.iter_mut().zip(&ses) {
            c.std_error = Some(*se);
            c.interval = satterthwaite_limits(c.variance, *se, options.confidence_level);
        }
        let k = comps.len();
        residual.std_error = Some(ses[k]);
        residual.interval = satterthwaite_limits(sigma2, ses[k], options.confidence_level);
        for (l, c) in residual_by_group.iter_mut().enumerate() {
            c.std_error = Some(ses[k + l]);
            c.interval = satterthwaite_limits(c.variance, ses[k + l], options.confidence_level);
        }
        variance_covariance = Some(to_matrix(&cov));
    }
    // Cluster-robust standard errors.
    let mut robust = None;
    let mut robust_parts = None;
    let mut fixed_covariance = to_matrix(&cov_beta);
    if options.robust {
        match cr2_parts(problem, &theta, &w_hat, &unscaled, &sol.beta) {
            Some((parts, cluster, meat)) => {
                let rcov = sandwich(&parts.m, &meat);
                let model_std_errors: Vec<f64> = fixed.iter().map(|f| f.std_error).collect();
                for (j, f) in fixed.iter_mut().enumerate() {
                    let df = bell_mccaffrey_df(&parts, &unit(p, j));
                    *f = coefficient(&f.name, f.estimate, rcov.get(j, j).max(0.0).sqrt(), df, options.confidence_level);
                }
                robust = Some(RobustSummary {
                    cluster,
                    n_clusters: parts.clusters.len(),
                    model_std_errors,
                    model_covariance: fixed_covariance.clone(),
                });
                fixed_covariance = to_matrix(&rcov);
                robust_parts = Some(parts);
                between_within = None;
                df_label = "Bell–McCaffrey (CR2)";
            }
            None => warnings.push(
                "Robust standard errors need one grouping that every other grouping is nested in (the clusters); \
                 model-based standard errors are shown."
                    .to_owned(),
            ),
        }
    }
    // Conditional modes and their covariance.
    let a_rows = st.a_rows(&theta);
    let zeros = vec![0.0; n];
    let sol_u = st
        .solve(&a_rows, &w_hat, &zeros, false)
        .ok_or(StatError::ConvergenceFailure { routine: "linear mixed model", iterations: evals })?;
    let blocks = st.conditional_blocks(&sol_u);
    let modes = modes_of(problem, &theta, &sol.u, &blocks, sigma2);
    let zu = st.random_part(&a_rows, &sol.u);
    let xb = st.fixed_part(&sol.beta);
    let marginal: Vec<f64> = (0..n).map(|i| problem.offset[i] + xb[i]).collect();
    let eta: Vec<f64> = (0..n).map(|i| marginal[i] + zu[i]).collect();
    if let Some(wn) = singular_warning(problem, &theta) {
        warnings.push(wn);
    }
    if !min.converged {
        warnings.push("The optimizer reached its evaluation limit before converging.".to_owned());
    }
    let crit = problem.lmm_profiled(&theta, &w_hat, reml);
    let k = p + st.n_theta + 1 + nr;
    Ok(MixedFit {
        family: MixedFamily::Normal,
        method: if reml { "REML" } else { "Maximum likelihood" }.to_owned(),
        n_obs: n,
        fixed,
        fixed_covariance,
        variance_components: comps,
        correlations: cors,
        residual: Some(residual),
        scale: None,
        minus_two_log_likelihood: crit,
        aic: crit + 2.0 * k as f64,
        bic: crit + (n as f64).ln() * k as f64,
        n_parameters: k,
        n_levels: n_levels(problem),
        modes,
        linear_predictor: eta.clone(),
        fitted: eta,
        fitted_marginal: marginal,
        theta,
        converged: min.converged,
        warnings,
        evaluations: evals,
        variance_covariance,
        residual_by_group,
        equal_variance_test: None,
        robust,
        df_label: df_label.to_owned(),
        thresholds: Vec::new(),
        ordinal_covariance: None,
        category_probabilities: Vec::new(),
        category_probabilities_marginal: Vec::new(),
        confidence_level: options.confidence_level,
        satterthwaite,
        between_within,
        robust_parts,
    })
}

/// `true` when every level of `inner` lies within one level of `outer`.
fn nested_in(inner: &RandomTerm, outer: &RandomTerm) -> bool {
    let mut map = vec![usize::MAX; inner.levels.len()];
    for (&a, &b) in inner.codes.iter().zip(&outer.codes) {
        if map[a] == usize::MAX {
            map[a] = b;
        } else if map[a] != b {
            return false;
        }
    }
    true
}

/// Between–within degrees of freedom of each fixed-effect column (Pinheiro
/// & Bates 2000, §2.4.2, as `nlme`): the groupings, fewest levels first,
/// must each be nested in the one before. A column constant within every
/// group of level ℓ (and not of an outer level) belongs to level ℓ, with
/// `Gℓ − Gℓ₋₁ − pℓ` degrees of freedom (`G₀ = 1` for the intercept); columns
/// varying within the innermost groups, and constant columns, get
/// `N − G_L − p_{L+1}`. `None` for crossed groupings.
fn between_within_df(problem: &Problem<'_>) -> Option<Vec<f64>> {
    let model = problem.model;
    let (n, p) = (problem.n, problem.p);
    let mut groups: Vec<&RandomTerm> = Vec::new();
    for t in &model.random {
        if !groups.iter().any(|g| g.group == t.group) {
            groups.push(t);
        }
    }
    groups.sort_by_key(|t| t.levels.len());
    if groups.windows(2).any(|w| !nested_in(w[1], w[0])) {
        return None;
    }
    let count = |t: &RandomTerm| {
        let mut seen = vec![false; t.levels.len()];
        t.codes.iter().for_each(|&c| seen[c] = true);
        seen.iter().filter(|&&s| s).count() as f64
    };
    let sizes: Vec<f64> = groups.iter().map(|t| count(t)).collect();
    let depth = groups.len();
    let level: Vec<usize> = (0..p)
        .map(|j| {
            let x0 = model.x[(0, j)];
            if (0..n).all(|i| (model.x[(i, j)] - x0).abs() <= 1e-12 * x0.abs().max(1.0)) {
                return depth;
            }
            groups
                .iter()
                .position(|t| {
                    let mut first = vec![f64::NAN; t.levels.len()];
                    (0..n).all(|i| {
                        let c = t.codes[i];
                        let v = model.x[(i, j)];
                        if first[c].is_nan() {
                            first[c] = v;
                            true
                        } else {
                            (first[c] - v).abs() <= 1e-12 * v.abs().max(1.0)
                        }
                    })
                })
                .unwrap_or(depth)
        })
        .collect();
    let constant = |j: usize| (0..n).all(|i| (model.x[(i, j)] - model.x[(0, j)]).abs() <= 1e-12);
    let mut df = vec![0.0; depth + 1];
    for l in 0..=depth {
        let used = (0..p).filter(|&j| level[j] == l && !constant(j)).count() as f64;
        let outer = if l == 0 { 1.0 } else { sizes[l - 1] };
        let this = if l == depth { n as f64 } else { sizes[l] };
        df[l] = (this - outer - used).max(1.0);
    }
    Some(level.iter().map(|&l| df[l]).collect())
}

/// Row-major `r × c` product of `a` (`r × k`) and `b` (`k × c`).
fn matmul(a: &[f64], b: &[f64], r: usize, k: usize, c: usize) -> Vec<f64> {
    let mut out = vec![0.0; r * c];
    for i in 0..r {
        for m in 0..k {
            let v = a[i * k + m];
            if v != 0.0 {
                for j in 0..c {
                    out[i * c + j] += v * b[m * c + j];
                }
            }
        }
    }
    out
}

/// `s^{−½}` of a symmetric positive semi-definite matrix, as a generalized
/// inverse square root (eigenvalues below 10⁻¹⁰ of the largest dropped).
fn inverse_sqrt(s: &Square) -> Square {
    let (vals, vecs) = symmetric_eigen(s);
    let top = vals.iter().copied().fold(0.0, f64::max);
    let n = s.n;
    let mut out = Square::zeros(n);
    for (k, &v) in vals.iter().enumerate() {
        if v > 1e-10 * top {
            let f = 1.0 / v.sqrt();
            for i in 0..n {
                for j in 0..n {
                    *out.at(i, j) += vecs.get(i, k) * f * vecs.get(j, k);
                }
            }
        }
    }
    out
}

/// The CR2 pieces for the clusters of the outermost grouping, its name, and
/// the meat `Σⱼ XⱼᵀV̂ⱼ⁻¹AⱼeⱼeⱼᵀAⱼᵀV̂ⱼ⁻¹Xⱼ`. Everything is in units of σ²
/// (which cancels). `None` when no grouping contains every other.
fn cr2_parts(
    problem: &Problem<'_>,
    theta: &[f64],
    w: &[f64],
    m: &Square,
    beta: &[f64],
) -> Option<(RobustParts, String, Square)> {
    let (st, model, n, p) = (&problem.st, problem.model, problem.n, problem.p);
    let k = (0..model.random.len())
        .filter(|&k| model.random.iter().all(|t| nested_in(t, &model.random[k])))
        .min_by_key(|&k| model.random[k].levels.len())?;
    let cluster = &model.random[k];
    let mut rows_of = vec![Vec::new(); cluster.levels.len()];
    for i in 0..n {
        rows_of[cluster.codes[i]].push(i);
    }
    let a = st.a_rows(theta);
    let xb = st.fixed_part(beta);
    let e: Vec<f64> = (0..n).map(|i| model.y[i] - problem.offset[i] - xb[i]).collect();
    let mut meat = Square::zeros(p);
    let mut clusters = Vec::new();
    for rows in rows_of.iter().filter(|r| !r.is_empty()) {
        let nj = rows.len();
        let mut v = Square::zeros(nj);
        for (r1, &i) in rows.iter().enumerate() {
            for (r2, &j) in rows.iter().enumerate() {
                let mut s = if r1 == r2 { 1.0 / w[i] } else { 0.0 };
                for (kk, t) in st.terms.iter().enumerate() {
                    if t.codes[i] == t.codes[j] {
                        let q = t.q;
                        s += (0..q).map(|c| a[kk][i * q + c] * a[kk][j * q + c]).sum::<f64>();
                    }
                }
                *v.at(r1, r2) = s;
            }
        }
        let l = cholesky(&v)?;
        let winv = inverse_from_cholesky(&l);
        let xj: Vec<f64> = rows.iter().flat_map(|&i| (0..p).map(move |c| model.x[(i, c)])).collect();
        // S = V − X M Xᵀ, then B = Lᵀ S L with V = L Lᵀ (Dⱼ = Lᵀ).
        let xm = matmul(&xj, &m.a, nj, p, p);
        let mut s_mat = v.clone();
        for r1 in 0..nj {
            for r2 in 0..nj {
                *s_mat.at(r1, r2) -= (0..p).map(|c| xm[r1 * p + c] * xj[r2 * p + c]).sum::<f64>();
            }
        }
        let lt: Vec<f64> = (0..nj * nj).map(|idx| l.get(idx % nj, idx / nj)).collect();
        let b = Square { n: nj, a: matmul(&matmul(&lt, &s_mat.a, nj, nj, nj), &l.a, nj, nj, nj) };
        let b_isqrt = inverse_sqrt(&b);
        // A = L B^{−½} Lᵀ.
        let adj = matmul(&matmul(&l.a, &b_isqrt.a, nj, nj, nj), &lt, nj, nj, nj);
        let adj_t: Vec<f64> = (0..nj * nj).map(|idx| adj[(idx % nj) * nj + idx / nj]).collect();
        // AᵀV⁻¹X, Gⱼ = AᵀV⁻¹XM and the meat term XᵀV⁻¹Ae = (AᵀV⁻¹X)ᵀe.
        let awx = matmul(&adj_t, &matmul(&winv.a, &xj, nj, nj, p), nj, nj, p);
        let g = matmul(&awx, &m.a, nj, p, p);
        let vj: Vec<f64> = (0..p).map(|c| (0..nj).map(|r| awx[r * p + c] * e[rows[r]]).sum()).collect();
        for r in 0..p {
            for c in 0..p {
                *meat.at(r, c) += vj[r] * vj[c];
            }
        }
        clusters.push(RobustCluster { g, v, x: xj });
    }
    Some((RobustParts { m: m.clone(), clusters }, cluster.group.clone(), meat))
}

/// `M meat M`.
fn sandwich(m: &Square, meat: &Square) -> Square {
    let p = m.n;
    Square { n: p, a: matmul(&matmul(&m.a, &meat.a, p, p, p), &m.a, p, p, p) }
}

/// For contrast `c`: each cluster's `uⱼ = Gⱼc`, `sⱼ`-ready pieces and
/// `aⱼ = Xⱼᵀuⱼ`.
fn cr2_vectors(parts: &RobustParts, c: &[f64]) -> Vec<(Vec<f64>, Vec<f64>)> {
    let p = c.len();
    parts
        .clusters
        .iter()
        .map(|cl| {
            let nj = cl.v.n;
            let u: Vec<f64> = (0..nj).map(|r| (0..p).map(|k| cl.g[r * p + k] * c[k]).sum()).collect();
            let a: Vec<f64> = (0..p).map(|k| (0..nj).map(|r| cl.x[r * p + k] * u[r]).sum()).collect();
            (u, a)
        })
        .collect()
}

/// `P_ij` of the CR2 tests for contrasts `s` and `t`:
/// `δᵢⱼ uₛⱼᵀV̂ⱼuₜⱼ − aₛᵢᵀMaₜⱼ` (the working-model covariances of the
/// clusters' contributions).
fn cr2_p(parts: &RobustParts, s: &[(Vec<f64>, Vec<f64>)], t: &[(Vec<f64>, Vec<f64>)]) -> Vec<Vec<f64>> {
    let j_n = s.len();
    let p = parts.m.n;
    let ma: Vec<Vec<f64>> =
        t.iter().map(|(_, a)| (0..p).map(|r| (0..p).map(|c| parts.m.get(r, c) * a[c]).sum()).collect()).collect();
    (0..j_n)
        .map(|i| {
            (0..j_n)
                .map(|j| {
                    let cross: f64 = s[i].1.iter().zip(&ma[j]).map(|(x, y)| x * y).sum();
                    let diag = if i == j {
                        let v = &parts.clusters[i].v;
                        let (us, ut) = (&s[i].0, &t[i].0);
                        (0..v.n).map(|r| us[r] * (0..v.n).map(|c| v.get(r, c) * ut[c]).sum::<f64>()).sum()
                    } else {
                        0.0
                    };
                    diag - cross
                })
                .collect()
        })
        .collect()
}

/// Bell–McCaffrey (Satterthwaite) degrees of freedom of `cᵀβ̂` with the CR2
/// covariance: `(Σⱼ Pⱼⱼ)² / Σᵢⱼ Pᵢⱼ²` (Pustejovsky & Tipton 2018, §3.1).
fn bell_mccaffrey_df(parts: &RobustParts, c: &[f64]) -> Option<f64> {
    let v = cr2_vectors(parts, c);
    let pm = cr2_p(parts, &v, &v);
    let trace: f64 = (0..pm.len()).map(|j| pm[j][j]).sum();
    let ss: f64 = pm.iter().flatten().map(|x| x * x).sum();
    let df = trace * trace / ss;
    (df.is_finite() && df > 0.0).then_some(df)
}

/// The HTZ test's degrees of freedom `η` for the contrast rows `l`
/// (Pustejovsky & Tipton 2018, §3.2): with `Ω = Σⱼ Pⱼⱼ` (`q × q`) and
/// `Bᵢⱼ = Ω^{−½} Pᵢⱼ Ω^{−½}`, `η = q(q+1) / Σₛₜ Σᵢⱼ (Bᵢⱼ[s,t] Bᵢⱼ[t,s] + Bᵢⱼ[s,s] Bᵢⱼ[t,t])`.
fn htz_eta(parts: &RobustParts, l: &[Vec<f64>]) -> Option<f64> {
    let q = l.len();
    let vecs: Vec<Vec<(Vec<f64>, Vec<f64>)>> = l.iter().map(|c| cr2_vectors(parts, c)).collect();
    let mut pa: Vec<Vec<Vec<Vec<f64>>>> = Vec::with_capacity(q);
    for s in 0..q {
        pa.push((0..q).map(|t| cr2_p(parts, &vecs[s], &vecs[t])).collect());
    }
    let j_n = parts.clusters.len();
    let mut omega = Square::zeros(q);
    for s in 0..q {
        for t in 0..q {
            *omega.at(s, t) = (0..j_n).map(|j| pa[s][t][j][j]).sum();
        }
    }
    let on = inverse_sqrt(&omega);
    let mut total = 0.0;
    let mut b = Square::zeros(q);
    for i in 0..j_n {
        for j in 0..j_n {
            for s in 0..q {
                for t in 0..q {
                    let mut acc = 0.0;
                    for a in 0..q {
                        for c in 0..q {
                            acc += on.get(s, a) * pa[a][c][i][j] * on.get(c, t);
                        }
                    }
                    *b.at(s, t) = acc;
                }
            }
            for s in 0..q {
                for t in 0..q {
                    total += b.get(s, t) * b.get(t, s) + b.get(s, s) * b.get(t, t);
                }
            }
        }
    }
    let eta = (q * (q + 1)) as f64 / total;
    (eta.is_finite() && eta > 0.0).then_some(eta)
}

fn unit(p: usize, j: usize) -> Vec<f64> {
    let mut v = vec![0.0; p];
    v[j] = 1.0;
    v
}

/// Satterthwaite degrees of freedom for `lᵀβ̂`:
/// `2 (lᵀCl)² / (gᵀ A g)` with `gₘ = lᵀ (∂C/∂ψₘ) l`.
fn satterthwaite_df(s: &Satterthwaite, cov: &Square, l: &[f64]) -> Option<f64> {
    let quad = |m: &Square| -> f64 {
        let p = l.len();
        let mut acc = 0.0;
        for i in 0..p {
            for j in 0..p {
                acc += l[i] * m.get(i, j) * l[j];
            }
        }
        acc
    };
    let v = quad(cov);
    let g: Vec<f64> = s.grads.iter().map(quad).collect();
    let mut denom = 0.0;
    for a in 0..g.len() {
        for b in 0..g.len() {
            denom += g[a] * s.a.get(a, b) * g[b];
        }
    }
    let df = 2.0 * v * v / denom;
    if df.is_finite() && df > 0.0 { Some(df) } else { None }
}

/// Denominator degrees of freedom of an F statistic that is the mean of `r`
/// independent squared t statistics with `nus[m]` degrees of freedom (`None`:
/// unbounded), by matching means (Fai & Cornelius 1996; Kuznetsova et al.
/// 2017, §3.3): with `E = Σ νₘ/(νₘ − 2)`, `ν = 2E/(E − r)`. A t² with
/// `νₘ ≤ 2` has no finite mean, so then `E` is unbounded and `ν = 2`. One
/// direction keeps its own df, and equal df are returned unchanged.
fn combined_ddf(nus: &[Option<f64>]) -> f64 {
    let df: Vec<f64> = nus.iter().map(|nu| nu.unwrap_or(f64::INFINITY)).collect();
    let r = df.len() as f64;
    if df.len() == 1 {
        return df[0];
    }
    if df.windows(2).all(|w| w[0] == w[1] || (w[0].is_finite() && (w[0] - w[1]).abs() <= 1e-8 * w[0].max(1.0))) {
        return df[0];
    }
    if df.iter().any(|&v| v <= 2.0) {
        return 2.0;
    }
    let e: f64 = df.iter().map(|&v| if v.is_finite() { v / (v - 2.0) } else { 1.0 }).sum();
    if e > r { 2.0 * e / (e - r) } else { f64::INFINITY }
}

fn fit_glmm(problem: &Problem<'_>, options: &MixedOptions) -> Result<MixedFit> {
    let fam = problem.family();
    let st = &problem.st;
    let (n, p, nt) = (problem.n, problem.p, st.n_theta);
    let has_scale = fam.has_scale();
    let y = &problem.model.y;
    // Starting scale: Gamma dispersion from the squared CV, NB shape by moments.
    let mean = y.iter().sum::<f64>() / n as f64;
    let var = y.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n as f64 - 1.0).max(1.0);
    let scale0 = match fam {
        MixedFamily::Gamma => (var / (mean * mean)).clamp(1e-3, 1e3),
        MixedFamily::NegativeBinomial => {
            if var > mean * 1.01 {
                (mean * mean / (var - mean)).clamp(0.05, 1e3)
            } else {
                100.0
            }
        }
        _ => 1.0,
    };
    let scale_of = |x: &[f64]| if has_scale { x[x.len() - 1].exp() } else { 1.0 };
    let nodes = gauss_hermite(options.quadrature_points);
    let agq = options.quadrature_points > 1;
    let evals = std::cell::Cell::new(0usize);

    // Stage 1: θ (and log scale), with β solved jointly with u.
    let mut warm: Option<Mode> = None;
    let mut x1 = st.theta_start();
    let mut lower1 = st.theta_lower();
    if has_scale {
        x1.push(scale0.ln());
        lower1.push(MIN_LOG_SCALE);
    }
    let steps: Vec<f64> = (0..x1.len()).map(|i| if i < nt { 0.5 } else { 0.3 }).collect();
    let mut stage1 = |x: &[f64]| -> f64 {
        evals.set(evals.get() + 1);
        let th = &x[..nt];
        match problem.pirls(th, None, scale_of(x), warm.as_ref()) {
            Some(m) => {
                let d = Problem::laplace(&m);
                if d.is_finite() {
                    warm = Some(m);
                }
                d
            }
            None => f64::INFINITY,
        }
    };
    let m1 = nelder_mead(&mut stage1, &x1, &lower1, &steps, options.max_evaluations);
    let base = problem
        .pirls(&m1.x[..nt], None, scale_of(&m1.x), None)
        .ok_or(StatError::ConvergenceFailure { routine: "penalized IRLS", iterations: evals.get() })?;

    // Stage 2: θ, β (and log scale) together, u by PIRLS for each β.
    let mut x2: Vec<f64> = m1.x[..nt].to_vec();
    x2.extend(&base.beta);
    let mut lower2 = st.theta_lower();
    lower2.extend(std::iter::repeat_n(f64::NEG_INFINITY, p));
    if has_scale {
        x2.push(m1.x[nt]);
        lower2.push(MIN_LOG_SCALE);
    }
    let warm2 = std::cell::RefCell::new(Some(base.clone()));
    let mut stage2 = |x: &[f64]| -> f64 {
        evals.set(evals.get() + 1);
        let th = &x[..nt];
        let beta = &x[nt..nt + p];
        let sc = scale_of(x);
        let found = problem.pirls(th, Some(beta), sc, warm2.borrow().as_ref());
        match found.or_else(|| problem.pirls(th, Some(beta), sc, None)) {
            Some(m) => {
                let d = if agq { problem.agq(th, &m, sc, &nodes) } else { Problem::laplace(&m) };
                if d.is_finite() {
                    *warm2.borrow_mut() = Some(m);
                }
                d
            }
            None => f64::INFINITY,
        }
    };
    let m2 = bfgs(&mut stage2, &x2, &lower2, options.max_evaluations);
    // Polish with Nelder–Mead from the quasi-Newton point (cheap insurance
    // against a stalled line search).
    let steps2: Vec<f64> = m2.x.iter().map(|v| 0.05 * v.abs().max(0.1)).collect();
    let m3 = nelder_mead(&mut stage2, &m2.x, &lower2, &steps2, options.max_evaluations / 4);
    let best = if m3.f < m2.f { m3.clone() } else { m2.clone() };
    let converged = m1.converged && (m2.converged || m3.converged);
    let xhat = best.x.clone();
    let theta = xhat[..nt].to_vec();
    let beta = xhat[nt..nt + p].to_vec();
    let scale = scale_of(&xhat);
    let mode = problem
        .pirls(&theta, Some(&beta), scale, warm2.borrow().as_ref())
        .ok_or(StatError::ConvergenceFailure { routine: "penalized IRLS", iterations: evals.get() })?;
    let crit = if agq { problem.agq(&theta, &mode, scale, &nodes) } else { Problem::laplace(&mode) };

    // Inference from the Hessian over (θ, β, log scale), as lme4 does.
    let mut vary = interior_theta(problem, &theta);
    vary.extend(nt..nt + p);
    if has_scale {
        vary.push(nt + p);
    }
    let h = hessian(&mut stage2, &xhat, &lower2, &vary);
    let mut warnings = Vec::new();
    let inv = spd_inverse(&h).map(|mut m| {
        m.a.iter_mut().for_each(|v| *v *= 2.0);
        m
    });
    let beta_pos: Vec<usize> = (0..p).map(|j| vary.iter().position(|&v| v == nt + j).expect("β varies")).collect();
    let cov_beta = match &inv {
        Some(c) => c.sub(&beta_pos),
        None => {
            warnings.push(
                "The curvature of the likelihood is not positive definite: fixed-effect standard errors ignore the \
                 uncertainty in the variance components."
                    .to_owned(),
            );
            let joint = problem.pirls(&theta, None, scale, Some(&mode));
            let a_rows = st.a_rows(&theta);
            let z = vec![0.0; n];
            match joint.and_then(|m| st.solve(&a_rows, &m.weights, &z, true)) {
                Some(sol) => st.beta_unscaled_covariance(&sol),
                None => Square::zeros(p),
            }
        }
    };
    let fixed: Vec<FixedEffect> = (0..p)
        .map(|j| {
            coefficient(
                &problem.model.x_names[j],
                beta[j],
                cov_beta.get(j, j).max(0.0).sqrt(),
                None,
                options.confidence_level,
            )
        })
        .collect();
    let (mut comps, cors, _) = components(problem, &theta, 1.0);
    let mut scale_param = None;
    if let Some(c) = &inv {
        let theta_pos: Vec<usize> = (0..vary.len()).filter(|&i| vary[i] < nt).collect();
        let a = c.sub(&theta_pos);
        let theta_vary: Vec<usize> = theta_pos.iter().map(|&i| vary[i]).collect();
        let g = |x: &[f64]| components(problem, x, 1.0).2;
        let ses = delta_se(&g, &theta, &theta_vary, &a);
        for (comp, se) in comps.iter_mut().zip(&ses) {
            comp.std_error = Some(*se);
            comp.interval = satterthwaite_limits(comp.variance, *se, options.confidence_level);
        }
    }
    if has_scale {
        let se = inv.as_ref().map(|c| scale * c.get(vary.len() - 1, vary.len() - 1).max(0.0).sqrt());
        scale_param = Some(ScaleParameter {
            name: if fam == MixedFamily::Gamma { "Dispersion".to_owned() } else { "Shape (k)".to_owned() },
            value: scale,
            std_error: se,
        });
    }
    let blocks = {
        let a_rows = st.a_rows(&theta);
        let z = vec![0.0; n];
        st.solve(&a_rows, &mode.weights, &z, false).map(|s| st.conditional_blocks(&s))
    }
    .ok_or(StatError::ConvergenceFailure { routine: "penalized IRLS", iterations: evals.get() })?;
    let modes = modes_of(problem, &theta, &mode.u, &blocks, 1.0);
    let xb = st.fixed_part(&beta);
    let marginal: Vec<f64> = (0..n).map(|i| fam.inverse_link(problem.offset[i] + xb[i])).collect();
    if let Some(wn) = singular_warning(problem, &theta) {
        warnings.push(wn);
    }
    if !converged {
        warnings.push("The optimizer reached its evaluation limit before converging.".to_owned());
    }
    if options.robust {
        warnings.push("Robust standard errors are available for normal responses only.".to_owned());
    }
    let k = p + nt + usize::from(has_scale);
    Ok(MixedFit {
        family: fam,
        method: if agq {
            format!("Adaptive Gauss–Hermite quadrature ({} points)", options.quadrature_points)
        } else {
            "Laplace approximation".to_owned()
        },
        n_obs: n,
        fixed,
        fixed_covariance: to_matrix(&cov_beta),
        variance_components: comps,
        correlations: cors,
        residual: None,
        scale: scale_param,
        minus_two_log_likelihood: crit,
        aic: crit + 2.0 * k as f64,
        bic: crit + (n as f64).ln() * k as f64,
        n_parameters: k,
        n_levels: n_levels(problem),
        modes,
        linear_predictor: mode.eta.clone(),
        fitted: mode.mu.clone(),
        fitted_marginal: marginal,
        theta,
        converged,
        warnings,
        evaluations: evals.get(),
        confidence_level: options.confidence_level,
        variance_covariance: None,
        residual_by_group: Vec::new(),
        equal_variance_test: None,
        robust: None,
        df_label: String::new(),
        thresholds: Vec::new(),
        ordinal_covariance: None,
        category_probabilities: Vec::new(),
        category_probabilities_marginal: Vec::new(),
        satterthwaite: None,
        between_within: None,
        robust_parts: None,
    })
}

impl MixedFit {
    /// Test `Lβ = 0` for the rows of `L` (each of length = number of fixed
    /// effects). Normal family: F with Satterthwaite denominator degrees of
    /// freedom (Kuznetsova, Brockhoff & Christensen, "lmerTest", *J. Stat.
    /// Softw.* 82(13), 2017, §3); other families: Wald χ².
    ///
    /// # Errors
    /// [`StatError::DimensionMismatch`] for rows of the wrong length;
    /// [`StatError::InvalidArgument`] when `L` has no independent rows.
    pub fn test_contrast(&self, l: &[Vec<f64>]) -> Result<ContrastTest> {
        let p = self.fixed.len();
        if let Some(bad) = l.iter().find(|r| r.len() != p) {
            return Err(StatError::DimensionMismatch { context: "contrast row", expected: p, found: bad.len() });
        }
        let cov = Square { n: p, a: self.fixed_covariance.transpose().as_slice().to_vec() };
        // M = L C Lᵀ, then its eigen decomposition (dropping null directions).
        let q = l.len();
        let mut m = Square::zeros(q);
        for i in 0..q {
            for j in 0..q {
                let mut s = 0.0;
                for a in 0..p {
                    for b in 0..p {
                        s += l[i][a] * cov.get(a, b) * l[j][b];
                    }
                }
                *m.at(i, j) = s;
            }
        }
        let (vals, vecs) = symmetric_eigen(&m);
        let top = vals.iter().copied().fold(0.0, f64::max);
        let keep: Vec<usize> = (0..q).filter(|&k| vals[k] > 1e-10 * top.max(f64::MIN_POSITIVE)).collect();
        if keep.is_empty() {
            return Err(invalid("the hypothesis has no estimable rows"));
        }
        let beta: Vec<f64> = self.fixed.iter().map(|f| f.estimate).collect();
        let lb: Vec<f64> = l.iter().map(|r| r.iter().zip(&beta).map(|(a, b)| a * b).sum()).collect();
        let mut chi2 = 0.0;
        let mut nus = Vec::new();
        for &k in &keep {
            let proj: f64 = (0..q).map(|i| vecs.get(i, k) * lb[i]).sum();
            chi2 += proj * proj / vals[k];
            if let Some(s) = &self.satterthwaite {
                // Contrast along this eigenvector.
                let lk: Vec<f64> = (0..p).map(|a| (0..q).map(|i| vecs.get(i, k) * l[i][a]).sum()).collect();
                nus.push(satterthwaite_df(s, &cov, &lk));
            }
        }
        let r = keep.len();
        if let Some(parts) = &self.robust_parts {
            // Hotelling T² approximation (HTZ) with the CR2 covariance.
            let rows: Vec<Vec<f64>> = keep
                .iter()
                .map(|&k| (0..p).map(|a| (0..q).map(|i| vecs.get(i, k) * l[i][a]).sum()).collect())
                .collect();
            let eta = htz_eta(parts, &rows).unwrap_or(f64::NAN);
            let qf = r as f64;
            let den = eta - qf + 1.0;
            let f = ((eta - qf + 1.0) / eta).max(0.0) * chi2 / qf;
            let pval = if den > 0.0 {
                FisherSnedecor::new(qf, den).and_then(|d| d.sf(f)).unwrap_or(f64::NAN)
            } else {
                f64::NAN
            };
            return Ok(ContrastTest { num_df: r, den_df: Some(den), statistic: f, p_value: pval });
        }
        if let Some(bw) = &self.between_within {
            // The smallest between–within df of the columns involved.
            let den =
                (0..p).filter(|&a| l.iter().any(|row| row[a] != 0.0)).map(|a| bw[a]).fold(f64::INFINITY, f64::min);
            let f = chi2 / r as f64;
            let pval = FisherSnedecor::new(r as f64, den).and_then(|d| d.sf(f)).unwrap_or(f64::NAN);
            return Ok(ContrastTest { num_df: r, den_df: Some(den), statistic: f, p_value: pval });
        }
        if self.family == MixedFamily::Normal {
            let f = chi2 / r as f64;
            let den = if self.satterthwaite.is_some() { combined_ddf(&nus) } else { (self.n_obs - p) as f64 };
            let pval = FisherSnedecor::new(r as f64, den).and_then(|d| d.sf(f)).unwrap_or(f64::NAN);
            Ok(ContrastTest { num_df: r, den_df: Some(den), statistic: f, p_value: pval })
        } else {
            let pval = ChiSquared::new(r as f64).and_then(|d| d.sf(chi2)).unwrap_or(f64::NAN);
            Ok(ContrastTest { num_df: r, den_df: None, statistic: chi2, p_value: pval })
        }
    }

    /// Confidence level of the intervals.
    #[must_use]
    pub fn confidence_level(&self) -> f64 {
        self.confidence_level
    }

    /// Variance of `lᵀβ̂` (with the model-based covariance), plus, with
    /// `random = Some((k, z))`, the variance `zᵀGₖz` of term `k`'s random
    /// effects at design `z` (for predicting a new level of the grouping),
    /// and Satterthwaite's degrees of freedom of that sum,
    /// `2v² / (∇vᵀ A ∇v)` (normal family; `None` otherwise or when the
    /// likelihood's curvature was not positive definite).
    ///
    /// # Errors
    /// [`StatError::DimensionMismatch`] for `l` or `z` of the wrong length,
    /// [`StatError::InvalidArgument`] for an unknown term.
    pub fn prediction_variance(&self, l: &[f64], random: Option<(usize, &[f64])>) -> Result<(f64, Option<f64>)> {
        let p = self.fixed.len();
        if l.len() != p {
            return Err(StatError::DimensionMismatch { context: "prediction row", expected: p, found: l.len() });
        }
        let cov_m = self.robust.as_ref().map_or(&self.fixed_covariance, |r| &r.model_covariance);
        let quad = |m: &dyn Fn(usize, usize) -> f64, v: &[f64]| -> f64 {
            let mut acc = 0.0;
            for i in 0..v.len() {
                for j in 0..v.len() {
                    acc += v[i] * m(i, j) * v[j];
                }
            }
            acc
        };
        let mut var = quad(&|i, j| cov_m[(i, j)], l);
        let mut term = None;
        if let Some((k, z)) = random {
            let Some(m) = self.modes.get(k) else { return Err(invalid(format!("no random-effects term {k}"))) };
            let q = m.effects.len();
            if z.len() != q {
                return Err(StatError::DimensionMismatch {
                    context: "random-effects row",
                    expected: q,
                    found: z.len(),
                });
            }
            // The term's covariance from its variances and correlations.
            let start: usize = self.modes[..k].iter().map(|m| m.effects.len()).sum();
            let comps = &self.variance_components[start..start + q];
            let corr = |a: usize, b: usize| -> f64 {
                if a == b {
                    return 1.0;
                }
                self.correlations
                    .iter()
                    .find(|c| {
                        c.group == m.group
                            && ((c.first == m.effects[a] && c.second == m.effects[b])
                                || (c.first == m.effects[b] && c.second == m.effects[a]))
                    })
                    .map_or(0.0, |c| if c.value.is_finite() { c.value } else { 0.0 })
            };
            var += quad(&|a, b| corr(a, b) * comps[a].std_dev * comps[b].std_dev, z);
            term = Some((k, z));
        }
        let df = self.satterthwaite.as_ref().and_then(|s| {
            let g: Vec<f64> = (0..s.grads.len())
                .map(|m| {
                    let mut d = quad(&|i, j| s.grads[m].get(i, j), l);
                    if let Some((k, z)) = term {
                        d += quad(&|a, b| s.g_grads[m][k].get(a, b), z);
                    }
                    d
                })
                .collect();
            let mut denom = 0.0;
            for a in 0..g.len() {
                for b in 0..g.len() {
                    denom += g[a] * s.a.get(a, b) * g[b];
                }
            }
            let df = 2.0 * var * var / denom;
            (df.is_finite() && df > 0.0).then_some(df)
        });
        Ok((var, df))
    }
}

#[cfg(test)]
mod tests {
    use super::combined_ddf;

    /// Fai & Cornelius's mean-matching rule, by hand: for ν = (10, 30),
    /// E = 10/8 + 30/28 = 2.32143, so ν = 2E/(E − 2) = 14.4444; one or equal
    /// directions keep their df; a direction with ν ≤ 2 (a t² without a finite
    /// mean) makes E unbounded, so ν = 2 (as lmerTest's `get_Fstat_ddf`).
    #[test]
    fn combined_denominator_df() {
        let e = 10.0 / 8.0 + 30.0 / 28.0;
        assert!((combined_ddf(&[Some(10.0), Some(30.0)]) - 2.0 * e / (e - 2.0)).abs() < 1e-12);
        assert!((combined_ddf(&[Some(7.5)]) - 7.5).abs() < 1e-12);
        assert!((combined_ddf(&[Some(1.5)]) - 1.5).abs() < 1e-12);
        assert!((combined_ddf(&[Some(12.0), Some(12.0), Some(12.0)]) - 12.0).abs() < 1e-9);
        // Previously the ν = 1.5 direction counted as if its df were infinite,
        // giving 2·2.0417/0.0417 ≈ 98.
        assert!((combined_ddf(&[Some(1.5), Some(50.0)]) - 2.0).abs() < 1e-12);
        // An unbounded direction contributes 1 to E.
        let e = 1.0 + 20.0 / 18.0;
        assert!((combined_ddf(&[None, Some(20.0)]) - 2.0 * e / (e - 2.0)).abs() < 1e-9);
    }
}
