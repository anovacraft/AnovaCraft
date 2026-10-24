//! Bayesian generalized linear and hierarchical models, sampled with
//! [`crate::nuts`].
//!
//! **Model.** `ηᵢ = offsetᵢ + β₀ + Σⱼ βⱼxᵢⱼ + Σₖ Σₑ zᵢₖₑ bₖ[gᵢₖ]ₑ`, with a
//! normal (identity link), binomial (logit), Poisson (log) or negative
//! binomial NB2 (log; `Var = μ + μ²/φ`) response. A normal response may
//! instead have a known standard deviation per row (meta-analysis, e.g.
//! the eight schools of Rubin 1981). Group effects are independent normal
//! with one standard deviation per grouping and effect, sampled in the
//! non-centred form `b = τ·u`, `u ~ N(0, 1)` (Papaspiliopoulos, Roberts &
//! Sköld 2007, *Stat. Sci.* 22; Betancourt & Girolami 2015, "Hamiltonian
//! Monte Carlo for hierarchical models"), which avoids the funnel
//! geometry of weakly identified groups.
//!
//! **Priors** (weakly informative; Gelman, Jakulin, Pittau & Su 2008, *Ann.
//! Appl. Stat.* 2; Gelman 2006, *Bayesian Analysis* 1). Predictors are
//! centred and scaled internally. With autoscaling (the default) the
//! coefficient of predictor `j` has a normal, Student-t or Cauchy prior
//! centred at 0 with scale `s·s_y/sd(xⱼ)` (`s = 2.5`; `s_y = sd(y)` for a
//! normal response, 1 otherwise); the intercept, defined at the predictors'
//! means, has a normal prior centred at the response's mean on the link
//! scale with scale `2.5·s_y`; the residual SD and group SDs have
//! exponential (mean `s_y`), half-normal, half-Cauchy or half-t(3) priors;
//! the negative binomial `φ` an exponential(1) prior. Without autoscaling the
//! scales are used as given. Positive parameters are sampled on the log
//! scale with the Jacobian added.
//!
//! **Output**: draws of every parameter on its natural scale, summaries
//! with equal-tailed and HPD intervals, R̂, bulk/tail ESS and MCSE
//! ([`crate::diagnostics`]), PSIS-LOO and WAIC ([`crate::loo`]) from the
//! pointwise log likelihood, and posterior predictive replicates.

use crate::diagnostics::{ParameterDiagnostics, diagnose};
use crate::loo::{LooResult, loo};
use crate::nuts::{Chain, NutsSettings, Target, sample};
use crate::univariate::hpd_from_draws;
use stat_core::catalog::{Distribution, Family};
use stat_core::rng::Rng;
use stat_core::special::{digamma, ln_gamma};
use stat_core::{Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

const LN_2PI: f64 = 1.837_877_066_409_345_5;

/// Response distribution and link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlmFamily {
    /// Normal with identity link.
    Normal,
    /// Binomial with logit link (response a proportion of `weights` trials).
    Binomial,
    /// Poisson with log link.
    Poisson,
    /// Negative binomial (NB2) with log link.
    NegativeBinomial,
}

/// Prior family of the coefficients.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CoefficientPrior {
    /// Normal.
    Normal,
    /// Student t with `df` degrees of freedom.
    StudentT(f64),
    /// Cauchy.
    Cauchy,
}

/// Prior family of standard deviations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalePrior {
    /// Exponential with mean equal to the scale.
    Exponential,
    /// Half-normal.
    HalfNormal,
    /// Half-Cauchy.
    HalfCauchy,
    /// Half Student t with 3 degrees of freedom.
    HalfT3,
}

/// Priors of a Bayesian GLM.
#[derive(Debug, Clone, PartialEq)]
pub struct Priors {
    /// Coefficient prior family.
    pub coefficient: CoefficientPrior,
    /// Coefficient prior scale (before autoscaling).
    pub coefficient_scale: f64,
    /// Intercept prior location on the link scale (`None`: the response's
    /// mean on the link scale).
    pub intercept_location: Option<f64>,
    /// Intercept prior scale (before autoscaling).
    pub intercept_scale: f64,
    /// Prior family of the residual and group SDs.
    pub sd: ScalePrior,
    /// Prior scale of the SDs (before autoscaling).
    pub sd_scale: f64,
    /// Scale priors to the data (see the module documentation).
    pub autoscale: bool,
}

impl Default for Priors {
    fn default() -> Self {
        Self {
            coefficient: CoefficientPrior::Normal,
            coefficient_scale: 2.5,
            intercept_location: None,
            intercept_scale: 2.5,
            sd: ScalePrior::Exponential,
            sd_scale: 1.0,
            autoscale: true,
        }
    }
}

/// One grouping's effects.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupTerm {
    /// Grouping name.
    pub name: String,
    /// Level names.
    pub levels: Vec<String>,
    /// Level of each row.
    pub codes: Vec<usize>,
    /// Effect names, e.g. `["Intercept", "Days"]`.
    pub effects: Vec<String>,
    /// Effect columns (`n` values each; ones for an intercept).
    pub z: Vec<Vec<f64>>,
}

/// A Bayesian GLM's data.
#[derive(Debug, Clone, PartialEq)]
pub struct BayesGlmModel {
    /// Response (a proportion for the binomial family).
    pub y: Vec<f64>,
    /// Predictor columns (without the intercept).
    pub x: Vec<Vec<f64>>,
    /// Their names.
    pub x_names: Vec<String>,
    /// Binomial trials (empty: 1 each; ignored for other families).
    pub trials: Vec<f64>,
    /// Offset on the link scale (empty: 0).
    pub offset: Vec<f64>,
    /// Normal family: known standard deviation of each row (no residual SD
    /// is estimated).
    pub known_sd: Option<Vec<f64>>,
    /// Response distribution.
    pub family: GlmFamily,
    /// Group effects.
    pub groups: Vec<GroupTerm>,
}

/// What a reported parameter is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterKind {
    /// Intercept or slope.
    Coefficient,
    /// Residual SD or negative binomial φ.
    Auxiliary,
    /// Standard deviation of a group effect.
    GroupSd,
    /// One level's group effect.
    GroupEffect,
}

/// Summary of one parameter's draws.
#[derive(Debug, Clone, PartialEq)]
pub struct ParameterSummary {
    /// Name.
    pub name: String,
    /// Kind.
    pub kind: ParameterKind,
    /// Mean.
    pub mean: f64,
    /// SD.
    pub sd: f64,
    /// Median.
    pub median: f64,
    /// Equal-tailed interval.
    pub interval: (f64, f64),
    /// HPD interval.
    pub hpd: (f64, f64),
    /// Convergence diagnostics.
    pub diagnostics: ParameterDiagnostics,
}

/// A fitted Bayesian GLM.
#[derive(Debug, Clone, PartialEq)]
pub struct BayesGlmFit {
    /// Parameter summaries (coefficients, auxiliary, group SDs, group effects).
    pub parameters: Vec<ParameterSummary>,
    /// Draws on the natural scale: `[parameter][chain][draw]`.
    pub draws: Vec<Vec<Vec<f64>>>,
    /// Sampler output per chain (unconstrained draws and statistics).
    pub chains: Vec<Chain>,
    /// PSIS-LOO and WAIC.
    pub loo: LooResult,
    /// Posterior mean of each row's mean response.
    pub fitted: Vec<f64>,
    /// Posterior predictive replicates (a few hundred sets of `n` values),
    /// on the response's scale (counts of successes divided by trials for
    /// the binomial).
    pub replicates: Vec<Vec<f64>>,
    /// Credible level.
    pub level: f64,
    /// Effective prior scales actually used: `(label, description)`.
    pub prior_notes: Vec<(String, String)>,
}

struct Layout {
    p: usize,
    sigma: Option<usize>,
    phi: Option<usize>,
    /// Index of `ln τ` for each (group, effect).
    tau: Vec<Vec<usize>>,
    /// Index of `u` for each (group, level, effect).
    u: Vec<Vec<Vec<usize>>>,
    dim: usize,
}

struct GlmTarget<'a> {
    m: &'a BayesGlmModel,
    xs: Vec<Vec<f64>>,
    trials: Vec<f64>,
    offset: Vec<f64>,
    layout: Layout,
    intercept: (f64, f64),
    coef_scales: Vec<f64>,
    coef: CoefficientPrior,
    sd_prior: ScalePrior,
    sd_scale: f64,
}

fn ln_coef_prior(kind: CoefficientPrior, x: f64, scale: f64) -> (f64, f64) {
    let z = x / scale;
    match kind {
        CoefficientPrior::Normal => (-0.5 * z * z, -z / scale),
        CoefficientPrior::StudentT(nu) => {
            (-0.5 * (nu + 1.0) * (z * z / nu).ln_1p(), -(nu + 1.0) * z / (scale * (nu + z * z)))
        }
        CoefficientPrior::Cauchy => (-(z * z).ln_1p(), -2.0 * z / (scale * (1.0 + z * z))),
    }
}

/// Log prior of a positive `τ = eᵛ` (with Jacobian) and its derivative in `v`.
fn ln_scale_prior(kind: ScalePrior, v: f64, scale: f64) -> (f64, f64) {
    let t = v.exp();
    let z = t / scale;
    match kind {
        ScalePrior::Exponential => (-z + v, -z + 1.0),
        ScalePrior::HalfNormal => (-0.5 * z * z + v, -z * z + 1.0),
        ScalePrior::HalfCauchy => (-(z * z).ln_1p() + v, -2.0 * z * z / (1.0 + z * z) + 1.0),
        ScalePrior::HalfT3 => (-2.0 * (z * z / 3.0).ln_1p() + v, -4.0 * z * z / (3.0 + z * z) + 1.0),
    }
}

impl GlmTarget<'_> {
    fn eta(&self, theta: &[f64]) -> Vec<f64> {
        let n = self.m.y.len();
        let p = self.layout.p;
        let mut eta: Vec<f64> = (0..n).map(|i| self.offset[i] + theta[0]).collect();
        for j in 1..p {
            let b = theta[j];
            for (e, x) in eta.iter_mut().zip(&self.xs[j - 1]) {
                *e += b * x;
            }
        }
        for (k, g) in self.m.groups.iter().enumerate() {
            for (e, z) in g.z.iter().enumerate() {
                let tau = theta[self.layout.tau[k][e]].exp();
                for i in 0..n {
                    eta[i] += z[i] * tau * theta[self.layout.u[k][g.codes[i]][e]];
                }
            }
        }
        eta
    }

    fn sigma(&self, theta: &[f64], i: usize) -> f64 {
        match (&self.m.known_sd, self.layout.sigma) {
            (Some(s), _) => s[i],
            (None, Some(j)) => theta[j].exp(),
            (None, None) => 1.0,
        }
    }

    /// Pointwise log likelihood (with constants) and `dℓ/dη`.
    fn pointwise(&self, theta: &[f64], eta: &[f64], i: usize) -> (f64, f64) {
        let y = self.m.y[i];
        let e = eta[i];
        match self.m.family {
            GlmFamily::Normal => {
                let s = self.sigma(theta, i);
                let r = (y - e) / s;
                (-0.5 * LN_2PI - s.ln() - 0.5 * r * r, r / s)
            }
            GlmFamily::Binomial => {
                let n = self.trials[i];
                let k = y * n;
                let softplus = if e > 0.0 { e + (-e).exp().ln_1p() } else { e.exp().ln_1p() };
                let ln_c = ln_gamma(n + 1.0) - ln_gamma(k + 1.0) - ln_gamma(n - k + 1.0);
                let pr = 1.0 / (1.0 + (-e).exp());
                (ln_c + k * e - n * softplus, k - n * pr)
            }
            GlmFamily::Poisson => {
                let mu = e.exp();
                (y * e - mu - ln_gamma(y + 1.0), y - mu)
            }
            GlmFamily::NegativeBinomial => {
                let phi = theta[self.layout.phi.unwrap_or(0)].exp();
                let mu = e.exp();
                let ll = ln_gamma(y + phi) - ln_gamma(phi) - ln_gamma(y + 1.0)
                    + phi * (phi / (phi + mu)).ln()
                    + y * (mu / (phi + mu)).ln();
                (ll, y - (y + phi) * mu / (phi + mu))
            }
        }
    }
}

impl Target for GlmTarget<'_> {
    fn dim(&self) -> usize {
        self.layout.dim
    }

    fn log_density_gradient(&self, theta: &[f64], grad: &mut [f64]) -> f64 {
        grad.iter_mut().for_each(|g| *g = 0.0);
        let n = self.m.y.len();
        let eta = self.eta(theta);
        let mut lp = 0.0;
        let mut d_eta = vec![0.0; n];
        let mut d_sigma = 0.0;
        let mut d_phi = 0.0;
        let phi = self.layout.phi.map(|j| theta[j].exp());
        for i in 0..n {
            let (ll, de) = self.pointwise(theta, &eta, i);
            lp += ll;
            d_eta[i] = de;
            if self.m.family == GlmFamily::Normal && self.layout.sigma.is_some() {
                let s = self.sigma(theta, i);
                let r = (self.m.y[i] - eta[i]) / s;
                d_sigma += r * r - 1.0;
            }
            if let Some(phi) = phi {
                let y = self.m.y[i];
                let mu = eta[i].exp();
                d_phi +=
                    phi * (digamma(y + phi) - digamma(phi) + (phi / (phi + mu)).ln() + 1.0 - (y + phi) / (phi + mu));
            }
        }
        if !lp.is_finite() {
            return f64::NEG_INFINITY;
        }
        // Coefficients.
        grad[0] = d_eta.iter().sum::<f64>();
        for j in 1..self.layout.p {
            grad[j] = self.xs[j - 1].iter().zip(&d_eta).map(|(x, d)| x * d).sum();
        }
        let (loc, sc) = self.intercept;
        let z = (theta[0] - loc) / sc;
        lp -= 0.5 * z * z;
        grad[0] -= z / sc;
        for j in 1..self.layout.p {
            let (l, d) = ln_coef_prior(self.coef, theta[j], self.coef_scales[j - 1]);
            lp += l;
            grad[j] += d;
        }
        if let Some(js) = self.layout.sigma {
            let (l, d) = ln_scale_prior(self.sd_prior, theta[js], self.sd_scale);
            lp += l;
            grad[js] += d_sigma + d;
        }
        if let Some(jp) = self.layout.phi {
            // φ ~ Exponential(1), sampled on the log scale.
            let (l, d) = ln_scale_prior(ScalePrior::Exponential, theta[jp], 1.0);
            lp += l;
            grad[jp] += d_phi + d;
        }
        for (k, g) in self.m.groups.iter().enumerate() {
            for (e, zc) in g.z.iter().enumerate() {
                let jt = self.layout.tau[k][e];
                let tau = theta[jt].exp();
                let mut d_tau = 0.0;
                for i in 0..n {
                    let ju = self.layout.u[k][g.codes[i]][e];
                    let c = d_eta[i] * zc[i];
                    grad[ju] += c * tau;
                    d_tau += c * tau * theta[ju];
                }
                let (l, d) = ln_scale_prior(self.sd_prior, theta[jt], self.sd_scale);
                lp += l;
                grad[jt] += d_tau + d;
            }
            for lv in &self.layout.u[k] {
                for &ju in lv {
                    lp -= 0.5 * theta[ju] * theta[ju];
                    grad[ju] -= theta[ju];
                }
            }
        }
        lp
    }
}

fn mean_sd(x: &[f64]) -> (f64, f64) {
    let n = x.len() as f64;
    let m = x.iter().sum::<f64>() / n;
    (m, (x.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / (n - 1.0).max(1.0)).sqrt())
}

/// Fit a Bayesian GLM by NUTS.
///
/// # Errors
/// Inconsistent lengths, non-finite values, a response unsuited to the
/// family, a constant predictor, too few rows, invalid priors, or sampler
/// failures.
pub fn fit_bayes_glm(m: &BayesGlmModel, priors: &Priors, settings: &NutsSettings, level: f64) -> Result<BayesGlmFit> {
    let n = m.y.len();
    if n < 2 {
        return Err(StatError::InsufficientData { required: 2, found: n });
    }
    if m.x.iter().any(|c| c.len() != n)
        || m.x.len() != m.x_names.len()
        || (!m.trials.is_empty() && m.trials.len() != n)
        || (!m.offset.is_empty() && m.offset.len() != n)
        || m.known_sd.as_ref().is_some_and(|s| s.len() != n)
        || m.groups
            .iter()
            .any(|g| g.codes.len() != n || g.z.iter().any(|z| z.len() != n) || g.z.len() != g.effects.len())
    {
        return Err(invalid("every column of the model needs a value for every row"));
    }
    if m.y.iter().chain(m.x.iter().flatten()).chain(&m.offset).any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "model data" });
    }
    if !(level > 0.0 && level < 1.0) {
        return Err(invalid("the credible level must lie between 0 and 1"));
    }
    let trials = if m.trials.is_empty() { vec![1.0; n] } else { m.trials.clone() };
    match m.family {
        GlmFamily::Binomial => {
            if m.y
                .iter()
                .zip(&trials)
                .any(|(&y, &t)| !(t > 0.0 && (0.0..=1.0).contains(&y) && ((y * t) - (y * t).round()).abs() < 1e-6))
            {
                return Err(invalid("a binomial response must be a proportion of a whole number of trials"));
            }
        }
        GlmFamily::Poisson | GlmFamily::NegativeBinomial => {
            if m.y.iter().any(|&y| !(y >= 0.0 && (y - y.round()).abs() < 1e-9)) {
                return Err(invalid("a count response must hold non-negative whole numbers"));
            }
        }
        GlmFamily::Normal => {
            if let Some(s) = &m.known_sd {
                if s.iter().any(|&v| !(v > 0.0 && v.is_finite())) {
                    return Err(invalid("known standard deviations must be positive"));
                }
            }
        }
    }
    if m.known_sd.is_some() && m.family != GlmFamily::Normal {
        return Err(invalid("known standard deviations apply to a normal response only"));
    }
    if !(priors.coefficient_scale > 0.0 && priors.intercept_scale > 0.0 && priors.sd_scale > 0.0) {
        return Err(invalid("prior scales must be positive"));
    }
    if let CoefficientPrior::StudentT(df) = priors.coefficient {
        if df.is_nan() || df <= 0.0 {
            return Err(invalid("the Student t prior needs positive degrees of freedom"));
        }
    }
    let offset = if m.offset.is_empty() { vec![0.0; n] } else { m.offset.clone() };
    // Standardize predictors.
    let mut xs = Vec::with_capacity(m.x.len());
    let mut centres = Vec::new();
    let mut spreads = Vec::new();
    for (c, name) in m.x.iter().zip(&m.x_names) {
        let (mu, sd) = mean_sd(c);
        if sd.is_nan() || sd <= 0.0 {
            return Err(invalid(format!("the predictor {name:?} does not vary")));
        }
        xs.push(c.iter().map(|v| (v - mu) / sd).collect::<Vec<_>>());
        centres.push(mu);
        spreads.push(sd);
    }
    // Layout.
    let p = m.x.len() + 1;
    let mut dim = p;
    let sigma = if m.family == GlmFamily::Normal && m.known_sd.is_none() {
        dim += 1;
        Some(dim - 1)
    } else {
        None
    };
    let phi = if m.family == GlmFamily::NegativeBinomial {
        dim += 1;
        Some(dim - 1)
    } else {
        None
    };
    let mut tau = Vec::new();
    for g in &m.groups {
        tau.push((0..g.effects.len()).map(|e| dim + e).collect::<Vec<_>>());
        dim += g.effects.len();
    }
    let mut u = Vec::new();
    for g in &m.groups {
        let mut lv = Vec::new();
        for _ in &g.levels {
            lv.push((0..g.effects.len()).map(|e| dim + e).collect::<Vec<_>>());
            dim += g.effects.len();
        }
        u.push(lv);
    }
    if m.groups.iter().any(|g| g.codes.iter().any(|&c| c >= g.levels.len())) {
        return Err(invalid("a group code refers to a missing level"));
    }
    let layout = Layout { p, sigma, phi, tau, u, dim };
    // Effective prior scales.
    let (ym, ysd) = mean_sd(&m.y);
    let sy = if m.family == GlmFamily::Normal && priors.autoscale { if ysd > 0.0 { ysd } else { 1.0 } } else { 1.0 };
    let default_location = match m.family {
        GlmFamily::Normal => ym,
        GlmFamily::Binomial => {
            let k: f64 = m.y.iter().zip(&trials).map(|(y, t)| y * t).sum();
            let t: f64 = trials.iter().sum();
            let pr = ((k + 0.5) / (t + 1.0)).clamp(1e-6, 1.0 - 1e-6);
            (pr / (1.0 - pr)).ln()
        }
        GlmFamily::Poisson | GlmFamily::NegativeBinomial => {
            (ym.max(0.5 / n as f64)).ln() - offset.iter().sum::<f64>() / n as f64
        }
    };
    let intercept = (priors.intercept_location.unwrap_or(default_location), priors.intercept_scale * sy);
    let coef_scales: Vec<f64> = spreads
        .iter()
        .map(|&sd| if priors.autoscale { priors.coefficient_scale * sy } else { priors.coefficient_scale * sd })
        .collect();
    let sd_scale = priors.sd_scale * sy;
    let target = GlmTarget {
        m,
        xs,
        trials: trials.clone(),
        offset: offset.clone(),
        layout,
        intercept,
        coef_scales: coef_scales.clone(),
        coef: priors.coefficient,
        sd_prior: priors.sd,
        sd_scale,
    };
    let chains = sample(&target, settings)?;
    // Natural-scale draws.
    let mut names: Vec<(String, ParameterKind)> = vec![("Intercept".to_owned(), ParameterKind::Coefficient)];
    names.extend(m.x_names.iter().map(|x| (x.clone(), ParameterKind::Coefficient)));
    if sigma.is_some() {
        names.push(("Residual SD".to_owned(), ParameterKind::Auxiliary));
    }
    if phi.is_some() {
        names.push(("Dispersion φ".to_owned(), ParameterKind::Auxiliary));
    }
    for g in &m.groups {
        for e in &g.effects {
            names.push((format!("SD({e} | {})", g.name), ParameterKind::GroupSd));
        }
    }
    for g in &m.groups {
        for l in &g.levels {
            for e in &g.effects {
                names.push((format!("{}[{l}]: {e}", g.name), ParameterKind::GroupEffect));
            }
        }
    }
    let natural = |th: &[f64]| -> Vec<f64> {
        let mut out = Vec::with_capacity(names.len());
        let slopes: Vec<f64> = (1..p).map(|j| th[j] / spreads[j - 1]).collect();
        out.push(th[0] - slopes.iter().zip(&centres).map(|(b, c)| b * c).sum::<f64>());
        out.extend(&slopes);
        if let Some(j) = sigma {
            out.push(th[j].exp());
        }
        if let Some(j) = phi {
            out.push(th[j].exp());
        }
        for t in &target.layout.tau {
            out.extend(t.iter().map(|&j| th[j].exp()));
        }
        for (k, lv) in target.layout.u.iter().enumerate() {
            for l in lv {
                for (e, &j) in l.iter().enumerate() {
                    out.push(th[target.layout.tau[k][e]].exp() * th[j]);
                }
            }
        }
        out
    };
    let k = names.len();
    let mut draws = vec![vec![Vec::with_capacity(settings.draws); chains.len()]; k];
    for (c, ch) in chains.iter().enumerate() {
        for th in &ch.draws {
            for (j, v) in natural(th).into_iter().enumerate() {
                draws[j][c].push(v);
            }
        }
    }
    let tail = 0.5 * (1.0 - level);
    let parameters: Vec<ParameterSummary> = names
        .iter()
        .zip(&draws)
        .map(|((name, kind), per)| {
            let mut all: Vec<f64> = per.iter().flatten().copied().collect();
            let (mean, sd) = mean_sd(&all);
            all.sort_by(f64::total_cmp);
            let q = |p: f64| {
                let h = (all.len() - 1) as f64 * p;
                let lo = h.floor() as usize;
                let hi = (lo + 1).min(all.len() - 1);
                all[lo] + (h - lo as f64) * (all[hi] - all[lo])
            };
            ParameterSummary {
                name: name.clone(),
                kind: *kind,
                mean,
                sd,
                median: q(0.5),
                interval: (q(tail), q(1.0 - tail)),
                hpd: hpd_from_draws(&all, level),
                diagnostics: diagnose(per),
            }
        })
        .collect();
    // Pointwise log likelihood, fitted means and replicates.
    let total: usize = chains.iter().map(|c| c.draws.len()).sum();
    let mut log_lik = Vec::with_capacity(total);
    let mut fitted = vec![0.0; n];
    let mut replicates = Vec::new();
    let every = (total / 200).max(1);
    let mut rng = Rng::new(settings.seed.wrapping_mul(31).wrapping_add(17));
    let mut count = 0;
    for ch in &chains {
        for th in &ch.draws {
            let eta = target.eta(th);
            log_lik.push((0..n).map(|i| target.pointwise(th, &eta, i).0).collect::<Vec<_>>());
            for i in 0..n {
                fitted[i] += match m.family {
                    GlmFamily::Normal => eta[i],
                    GlmFamily::Binomial => 1.0 / (1.0 + (-eta[i]).exp()),
                    GlmFamily::Poisson | GlmFamily::NegativeBinomial => eta[i].exp(),
                };
            }
            if count % every == 0 {
                let rep: Vec<f64> = (0..n)
                    .map(|i| {
                        let e = eta[i];
                        let draw = |family: Family, params: &[f64], rng: &mut Rng| {
                            Distribution::new(family, params).map_or(f64::NAN, |d| d.sample(rng))
                        };
                        match m.family {
                            GlmFamily::Normal => e + target.sigma(th, i) * rng.normal(),
                            GlmFamily::Binomial => {
                                let pr = 1.0 / (1.0 + (-e).exp());
                                draw(Family::Binomial, &[trials[i], pr], &mut rng) / trials[i]
                            }
                            GlmFamily::Poisson => draw(Family::Poisson, &[e.exp()], &mut rng),
                            GlmFamily::NegativeBinomial => {
                                let ph = th[phi.unwrap_or(0)].exp();
                                let lam = draw(Family::Gamma, &[ph, e.exp() / ph, 0.0], &mut rng);
                                draw(Family::Poisson, &[lam.max(1e-300)], &mut rng)
                            }
                        }
                    })
                    .collect();
                replicates.push(rep);
            }
            count += 1;
        }
    }
    fitted.iter_mut().for_each(|f| *f /= total as f64);
    let loo = loo(&log_lik, None);
    let fam = |x: CoefficientPrior| match x {
        CoefficientPrior::Normal => "Normal".to_owned(),
        CoefficientPrior::StudentT(df) => format!("Student t ({df} df)"),
        CoefficientPrior::Cauchy => "Cauchy".to_owned(),
    };
    let sdf = match priors.sd {
        ScalePrior::Exponential => "Exponential",
        ScalePrior::HalfNormal => "Half-normal",
        ScalePrior::HalfCauchy => "Half-Cauchy",
        ScalePrior::HalfT3 => "Half-t (3 df)",
    };
    let g4 = |v: f64| format!("{}", (v * 1e4).round() / 1e4);
    let mut prior_notes = vec![(
        "Intercept (at the predictors' means)".to_owned(),
        format!("Normal(location {}, scale {})", g4(intercept.0), g4(intercept.1)),
    )];
    for ((name, s), sd) in m.x_names.iter().zip(&coef_scales).zip(&spreads) {
        prior_notes.push((name.clone(), format!("{}(0, scale {})", fam(priors.coefficient), g4(s / sd))));
    }
    if sigma.is_some() || !m.groups.is_empty() {
        let text = if priors.sd == ScalePrior::Exponential {
            format!("Exponential with mean {}", g4(sd_scale))
        } else {
            format!("{sdf}, scale {}", g4(sd_scale))
        };
        prior_notes.push(("Standard deviations".to_owned(), text));
    }
    if phi.is_some() {
        prior_notes.push(("Dispersion φ".to_owned(), "Exponential (mean 1)".to_owned()));
    }
    Ok(BayesGlmFit { parameters, draws, chains, loo, fitted, replicates, level, prior_notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient_check(m: &BayesGlmModel, priors: &Priors) {
        let n = m.y.len();
        let trials = if m.trials.is_empty() { vec![1.0; n] } else { m.trials.clone() };
        let fit = fit_bayes_glm(
            m,
            priors,
            &NutsSettings { chains: 1, warmup: 20, draws: 10, ..NutsSettings::default() },
            0.9,
        );
        assert!(fit.is_ok(), "{fit:?}");
        // Rebuild a target the same way and compare gradients numerically.
        let xs: Vec<Vec<f64>> =
            m.x.iter()
                .map(|c| {
                    let (mu, sd) = mean_sd(c);
                    c.iter().map(|v| (v - mu) / sd).collect()
                })
                .collect();
        let p = m.x.len() + 1;
        let mut dim = p;
        let sigma = (m.family == GlmFamily::Normal && m.known_sd.is_none()).then(|| {
            dim += 1;
            dim - 1
        });
        let phi = (m.family == GlmFamily::NegativeBinomial).then(|| {
            dim += 1;
            dim - 1
        });
        let mut tau = Vec::new();
        for g in &m.groups {
            tau.push((0..g.effects.len()).map(|e| dim + e).collect::<Vec<_>>());
            dim += g.effects.len();
        }
        let mut u = Vec::new();
        for g in &m.groups {
            let mut lv = Vec::new();
            for _ in &g.levels {
                lv.push((0..g.effects.len()).map(|e| dim + e).collect::<Vec<_>>());
                dim += g.effects.len();
            }
            u.push(lv);
        }
        let t = GlmTarget {
            m,
            xs,
            trials,
            offset: vec![0.0; n],
            layout: Layout { p, sigma, phi, tau, u, dim },
            intercept: (0.1, 2.0),
            coef_scales: vec![1.5; m.x.len()],
            coef: CoefficientPrior::StudentT(4.0),
            sd_prior: ScalePrior::HalfCauchy,
            sd_scale: 1.3,
        };
        let theta: Vec<f64> = (0..dim).map(|j| 0.3 * ((j * 7 % 5) as f64 - 2.0) / 2.0).collect();
        let mut g = vec![0.0; dim];
        let f0 = t.log_density_gradient(&theta, &mut g);
        assert!(f0.is_finite());
        for j in 0..dim {
            let h = 1e-6;
            let mut a = theta.clone();
            a[j] += h;
            let mut b = theta.clone();
            b[j] -= h;
            let mut tmp = vec![0.0; dim];
            let num = (t.log_density_gradient(&a, &mut tmp) - t.log_density_gradient(&b, &mut tmp)) / (2.0 * h);
            assert!((num - g[j]).abs() < 1e-5 * (1.0 + g[j].abs()), "param {j}: {num} vs {}", g[j]);
        }
    }

    #[test]
    fn gradients_match_finite_differences() {
        let n = 12;
        let x1: Vec<f64> = (0..n).map(|i| i as f64 * 0.5).collect();
        let x2: Vec<f64> = (0..n).map(|i| ((i * 5) % 7) as f64).collect();
        let codes: Vec<usize> = (0..n).map(|i| i % 3).collect();
        let group = GroupTerm {
            name: "G".into(),
            levels: vec!["a".into(), "b".into(), "c".into()],
            codes,
            effects: vec!["Intercept".into(), "x1".into()],
            z: vec![vec![1.0; n], x1.clone()],
        };
        let base = BayesGlmModel {
            y: (0..n).map(|i| ((i * 3) % 5) as f64).collect(),
            x: vec![x1, x2],
            x_names: vec!["x1".into(), "x2".into()],
            trials: vec![],
            offset: vec![],
            known_sd: None,
            family: GlmFamily::Poisson,
            groups: vec![group],
        };
        let pr = Priors::default();
        gradient_check(&base, &pr);
        gradient_check(&BayesGlmModel { family: GlmFamily::NegativeBinomial, ..base.clone() }, &pr);
        gradient_check(&BayesGlmModel { family: GlmFamily::Normal, ..base.clone() }, &pr);
        let y: Vec<f64> = (0..n).map(|i| f64::from(u8::from(i % 3 == 0))).collect();
        gradient_check(&BayesGlmModel { family: GlmFamily::Binomial, y, ..base.clone() }, &pr);
        gradient_check(&BayesGlmModel { known_sd: Some(vec![0.7; n]), family: GlmFamily::Normal, ..base }, &pr);
    }
}
