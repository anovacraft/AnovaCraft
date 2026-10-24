//! Censored location–scale regression: a life (or response) distribution
//! whose location, and optionally whose log scale, are linear in covariates,
//! fitted by maximum likelihood to exact, right-, left- and
//! interval-censored observations with frequencies. It is the engine behind
//! the life–stress models of [`crate::life_stress`] and the destructive
//! degradation model of [`crate::degradation`].
//!
//! # Model
//! For observation `i` with location covariates `xᵢ` (first entry 1), scale
//! covariates `zᵢ` (first entry 1) and a known offset `oᵢ`,
//!
//! ```text
//! Y = μᵢ + σᵢ ε,   μᵢ = xᵢᵀβ + oᵢ,   ln σᵢ = zᵢᵀγ,
//! ```
//!
//! where `Y = ln T` for the log-location-scale families (Weibull: `ε` smallest
//! extreme value; lognormal: normal; log-logistic: logistic; exponential: the
//! Weibull with `σ = 1`) and `Y = T` for the normal, logistic and extreme-value
//! families (Meeker & Escobar, *Statistical Methods for Reliability Data*,
//! 1998, ch. 17; Lawless, *Statistical Models and Methods for Lifetime Data*,
//! 2nd ed., 2003, ch. 6; Nelson, *Accelerated Testing*, 1990, ch. 5). The
//! `p` quantile at a covariate point is `y_p = μ + w_p σ` with `w_p` the
//! standard quantile of `ε`, so `t_p = exp(y_p)` for the log families.
//!
//! # Likelihood
//! Each observation contributes as in [`crate::censored_fit`]:
//! `w ln f(t)` for an exact time, `w ln S(t)` right-censored, `w ln F(t)`
//! left-censored and `w ln[F(u) − F(l)]` interval-censored, with the
//! observation's own `μᵢ, σᵢ`; the log-likelihood includes the Jacobian `1/t`
//! of the log families, so it is the likelihood of the times themselves.
//!
//! # Estimation
//! Covariates other than the intercept are centred and scaled internally, so
//! every coefficient is of order one; the likelihood is maximized over the
//! standardized coefficients by damped Newton (Levenberg–Marquardt) steps
//! with central-difference derivatives from least-squares starting values
//! that treat every observation as a failure at a representative time. The
//! covariance of the estimates is the inverse observed information, mapped
//! back to the original coefficients linearly.
//!
//! # Intervals
//! **Wald** limits use the delta-method standard error on the scale where
//! the quantity is unconstrained (log time for quantiles of the log
//! families, logit for probabilities). **Likelihood-ratio** limits are the
//! sets `{c : 2[ℓ̂ − max_{g(θ) = c} ℓ(θ)] ≤ χ²₁(1 − α)}` (Meeker & Escobar
//! §8.3, §17.4; Cox & Hinkley 1974 §9.3), where for a linear function of the
//! coefficients one coefficient is solved from the constraint, and for a
//! quantile or failure probability at a covariate point the intercept is
//! solved from `μ + w_p σ = y` and the rest re-maximized.

use stat_core::distribution_fit::{FitFamily, FittedDistribution};
use stat_core::{ChiSquared, ContinuousDistribution, Normal, Result, StatError};

use crate::censored_fit::{IntervalEstimate, contribution, derivatives, inverse_spd, maximize};
use crate::life_data::{CensorKind, LifeObservation};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

const EULER: f64 = 0.577_215_664_901_532_9;

/// The distribution of the standardized error of a location–scale regression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegressionFamily {
    /// Weibull life (smallest extreme value on log time).
    Weibull,
    /// Lognormal life (normal on log time).
    Lognormal,
    /// Log-logistic life (logistic on log time).
    LogLogistic,
    /// Exponential life (the Weibull with `σ = 1`).
    Exponential,
    /// Normal.
    Normal,
    /// Logistic.
    Logistic,
    /// Smallest extreme value.
    SmallestExtremeValue,
    /// Largest extreme value.
    LargestExtremeValue,
}

impl RegressionFamily {
    /// The life distributions (log-location-scale families and the exponential).
    pub const LIFE: [Self; 4] = [Self::Weibull, Self::Lognormal, Self::LogLogistic, Self::Exponential];

    /// The location–scale families on the whole real line.
    pub const REAL_LINE: [Self; 4] =
        [Self::Normal, Self::Logistic, Self::SmallestExtremeValue, Self::LargestExtremeValue];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Weibull => "Weibull",
            Self::Lognormal => "Lognormal",
            Self::LogLogistic => "Log-logistic",
            Self::Exponential => "Exponential",
            Self::Normal => "Normal",
            Self::Logistic => "Logistic",
            Self::SmallestExtremeValue => "Smallest extreme value",
            Self::LargestExtremeValue => "Largest extreme value",
        }
    }

    /// Stable key for project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Weibull => "weibull",
            Self::Lognormal => "lognormal",
            Self::LogLogistic => "log-logistic",
            Self::Exponential => "exponential",
            Self::Normal => "normal",
            Self::Logistic => "logistic",
            Self::SmallestExtremeValue => "smallest-extreme-value",
            Self::LargestExtremeValue => "largest-extreme-value",
        }
    }

    /// The family with this key (also `"loglogistic"`, `"sev"` and `"lev"`).
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        let key = match key {
            "loglogistic" => "log-logistic",
            "sev" => "smallest-extreme-value",
            "lev" => "largest-extreme-value",
            other => other,
        };
        Self::LIFE.into_iter().chain(Self::REAL_LINE).find(|f| f.key() == key)
    }

    /// `true` when the model is for `ln T` (times must be positive).
    #[must_use]
    pub fn is_log(self) -> bool {
        matches!(self, Self::Weibull | Self::Lognormal | Self::LogLogistic | Self::Exponential)
    }

    /// `true` when the scale is fixed at one (the exponential).
    #[must_use]
    pub fn fixed_scale(self) -> bool {
        self == Self::Exponential
    }

    /// The [`FitFamily`] whose density is used (the exponential uses the
    /// Weibull with `σ = 1`).
    #[must_use]
    pub fn fit_family(self) -> FitFamily {
        match self {
            Self::Weibull | Self::Exponential => FitFamily::Weibull,
            Self::Lognormal => FitFamily::Lognormal,
            Self::LogLogistic => FitFamily::LogLogistic,
            Self::Normal => FitFamily::Normal,
            Self::Logistic => FitFamily::Logistic,
            Self::SmallestExtremeValue => FitFamily::SmallestExtremeValue,
            Self::LargestExtremeValue => FitFamily::LargestExtremeValue,
        }
    }

    /// The single-sample family of [`crate::censored_fit`] with the same
    /// distribution (the exponential is [`FitFamily::Exponential`]).
    #[must_use]
    pub fn single_sample_family(self) -> FitFamily {
        if self == Self::Exponential { FitFamily::Exponential } else { self.fit_family() }
    }

    /// The distribution with location `mu` and scale `sigma`.
    #[must_use]
    pub fn distribution(self, mu: f64, sigma: f64) -> FittedDistribution {
        FittedDistribution {
            family: self.fit_family(),
            threshold: 0.0,
            location: mu,
            scale: if self.fixed_scale() { 1.0 } else { sigma },
            shape: 1.0,
            log_likelihood: f64::NAN,
            n: 0,
        }
    }

    /// Standard quantile `w_p` of the error distribution.
    #[must_use]
    pub fn standard_quantile(self, p: f64) -> f64 {
        self.distribution(0.0, 1.0).standard_quantile(p)
    }

    /// Standard distribution function `Φ(z)` of the error.
    #[must_use]
    pub fn standard_cdf(self, z: f64) -> f64 {
        let d = FittedDistribution {
            family: match self.fit_family() {
                FitFamily::Weibull => FitFamily::SmallestExtremeValue,
                FitFamily::Lognormal => FitFamily::Normal,
                FitFamily::LogLogistic => FitFamily::Logistic,
                f => f,
            },
            threshold: 0.0,
            location: 0.0,
            scale: 1.0,
            shape: 1.0,
            log_likelihood: f64::NAN,
            n: 0,
        };
        d.cdf(z)
    }
}

/// Covariates of one point: location covariates (first entry 1), scale
/// covariates (first entry 1) and the location offset.
#[derive(Debug, Clone, PartialEq)]
pub struct CovariatePoint {
    /// Location covariates `x`.
    pub location: Vec<f64>,
    /// Scale covariates `z`.
    pub scale: Vec<f64>,
    /// Location offset `o`.
    pub offset: f64,
}

/// Observations with their covariates.
#[derive(Debug, Clone, PartialEq)]
pub struct RegressionData {
    /// The observations (times, or responses for the real-line families).
    pub observations: Vec<LifeObservation>,
    /// Covariates of each observation.
    pub points: Vec<CovariatePoint>,
}

/// A fitted location–scale regression.
#[derive(Debug, Clone, PartialEq)]
pub struct RegressionFit {
    /// The family.
    pub family: RegressionFamily,
    /// Location coefficients `β` (intercept first).
    pub location_coefficients: Vec<f64>,
    /// Log-scale coefficients `γ` (empty for the exponential).
    pub scale_coefficients: Vec<f64>,
    /// Covariance of `(β, γ)` (`None` when the observed information is not
    /// positive definite).
    pub covariance: Option<Vec<Vec<f64>>>,
    /// Maximized log-likelihood.
    pub log_likelihood: f64,
    internal: Vec<f64>,
    internal_covariance: Option<Vec<Vec<f64>>>,
    /// Centring and scaling of each location and scale covariate.
    location_std: Vec<(f64, f64)>,
    scale_std: Vec<(f64, f64)>,
    data: RegressionData,
}

/// Centre and scale of a column (`(0, 1)` for the intercept).
fn standardization(points: &[CovariatePoint], weights: &[f64], col: usize, location: bool) -> Result<(f64, f64)> {
    if col == 0 {
        return Ok((0.0, 1.0));
    }
    let v = |p: &CovariatePoint| if location { p.location[col] } else { p.scale[col] };
    let w: f64 = weights.iter().sum();
    let mean = points.iter().zip(weights).map(|(p, w)| w * v(p)).sum::<f64>() / w;
    let var = points.iter().zip(weights).map(|(p, w)| w * (v(p) - mean).powi(2)).sum::<f64>() / w;
    let sd = var.sqrt();
    if sd.is_nan() || sd <= 1e-12 * (1.0 + mean.abs()) || !sd.is_finite() {
        return Err(invalid("a covariate takes a single value, so its coefficient cannot be estimated"));
    }
    Ok((mean, sd))
}

/// Solve a small dense system by Gaussian elimination with partial pivoting.
fn solve_dense(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for k in 0..n {
        let piv = (k..n).max_by(|&i, &j| a[i][k].abs().total_cmp(&a[j][k].abs()))?;
        if a[piv][k].abs() < 1e-300 {
            return None;
        }
        a.swap(k, piv);
        b.swap(k, piv);
        for i in k + 1..n {
            let f = a[i][k] / a[k][k];
            for j in k..n {
                a[i][j] -= f * a[k][j];
            }
            b[i] -= f * b[k];
        }
    }
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        x[i] = (b[i] - (i + 1..n).map(|j| a[i][j] * x[j]).sum::<f64>()) / a[i][i];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// Fit a location–scale regression by maximum likelihood (see the module
/// docs).
///
/// # Errors
/// [`StatError::InvalidArgument`] when there are no observations or they do
/// not match the covariates, covariate rows differ in length or lack the
/// leading 1, a log family meets a failure at or before zero, every unit is
/// right-censored, a covariate is constant, or the likelihood has no
/// maximum (too few failures for the parameters).
pub fn fit_regression(data: &RegressionData, family: RegressionFamily) -> Result<RegressionFit> {
    let n = data.observations.len();
    if n == 0 || data.points.len() != n {
        return Err(invalid("every observation needs its covariates"));
    }
    let p = data.points[0].location.len();
    let q = if family.fixed_scale() { 0 } else { data.points[0].scale.len() };
    if p == 0 || (!family.fixed_scale() && q == 0) {
        return Err(invalid("the location and scale need an intercept"));
    }
    for pt in &data.points {
        if pt.location.len() != p
            || pt.location[0] != 1.0
            || (!family.fixed_scale() && (pt.scale.len() != q || pt.scale[0] != 1.0))
            || pt.location.iter().chain(&pt.scale).chain(std::iter::once(&pt.offset)).any(|v| !v.is_finite())
        {
            return Err(invalid("covariates must be finite, with the intercept first"));
        }
    }
    let obs = &data.observations;
    if obs.iter().all(|o| o.kind() == CensorKind::Right) {
        return Err(invalid("every unit is still running, so a life–stress model cannot be estimated"));
    }
    if family.is_log() {
        for o in obs {
            let bad = match o.kind() {
                CensorKind::Exact => o.lower <= 0.0,
                CensorKind::Left | CensorKind::Interval => o.upper <= 0.0,
                CensorKind::Right => false,
            };
            if bad {
                return Err(invalid(format!("the {} distribution needs positive times", family.label())));
            }
        }
    }
    let weights: Vec<f64> = obs.iter().map(|o| o.weight).collect();
    let location_std = (0..p).map(|j| standardization(&data.points, &weights, j, true)).collect::<Result<Vec<_>>>()?;
    let scale_std = (0..q).map(|j| standardization(&data.points, &weights, j, false)).collect::<Result<Vec<_>>>()?;
    let mut fit = RegressionFit {
        family,
        location_coefficients: vec![0.0; p],
        scale_coefficients: vec![0.0; q],
        covariance: None,
        log_likelihood: f64::NAN,
        internal: vec![0.0; p + q],
        internal_covariance: None,
        location_std,
        scale_std,
        data: data.clone(),
    };
    // Starting values: weighted least squares of representative values.
    let y: Vec<f64> = obs.iter().map(|o| fit.representative(o)).collect();
    let xs: Vec<Vec<f64>> = data.points.iter().map(|pt| fit.standard_location(pt)).collect();
    let mut xtx = vec![vec![0.0; p]; p];
    let mut xty = vec![0.0; p];
    for ((x, yi), (w, pt)) in xs.iter().zip(&y).zip(weights.iter().zip(&data.points)) {
        for a in 0..p {
            for b in 0..p {
                xtx[a][b] += w * x[a] * x[b];
            }
            xty[a] += w * x[a] * (yi - pt.offset);
        }
    }
    let beta = solve_dense(xtx, xty).ok_or_else(|| invalid("the covariates are collinear"))?;
    let resid: f64 = xs
        .iter()
        .zip(&y)
        .zip(weights.iter().zip(&data.points))
        .map(|((x, yi), (w, pt))| w * (yi - pt.offset - x.iter().zip(&beta).map(|(a, b)| a * b).sum::<f64>()).powi(2))
        .sum::<f64>()
        / weights.iter().sum::<f64>();
    let mut sd = resid.sqrt();
    if !(sd > 0.0 && sd.is_finite()) {
        sd = if family.is_log() { 0.5 } else { y.iter().map(|v| v.abs()).fold(0.0, f64::max).max(1.0) * 0.1 };
    }
    let pi = std::f64::consts::PI;
    let (shift, s) = match FamilyKind::from(family) {
        FamilyKind::Normal => (0.0, sd),
        FamilyKind::Logistic => (0.0, sd * 3f64.sqrt() / pi),
        FamilyKind::Lev => {
            let s = sd * 6f64.sqrt() / pi;
            (-EULER * s, s)
        }
        FamilyKind::Sev => {
            let s = sd * 6f64.sqrt() / pi;
            (EULER * s, s)
        }
    };
    let mut start = beta;
    start[0] += shift;
    if q > 0 {
        start.push(s.ln());
        start.extend(std::iter::repeat_n(0.0, q - 1));
    }
    let ll = |th: &[f64]| fit.log_likelihood_internal(th);
    let steps = |th: &[f64]| fit.steps(th);
    let mut best: Option<(Vec<f64>, f64)> = None;
    let censored_share = obs.iter().filter(|o| o.kind() == CensorKind::Right).map(|o| o.weight).sum::<f64>()
        / weights.iter().sum::<f64>();
    let mut starts = vec![start.clone()];
    for (dl, ds) in [(0.0, 0.7), (0.0, -0.7), (1.0 + 2.0 * censored_share, 0.0), (2.0 * s, 0.5)] {
        let mut x0 = start.clone();
        x0[0] += dl;
        if q > 0 {
            x0[p] += ds;
        }
        starts.push(x0);
    }
    for (i, x0) in starts.iter().enumerate() {
        if let Some(m) = maximize(&ll, x0, &steps) {
            if best.as_ref().is_none_or(|b| m.1 > b.1 + 1e-9) {
                best = Some(m);
            }
            if i >= 1 && best.is_some() {
                break;
            }
        }
    }
    let (internal, value) = best.ok_or_else(|| {
        invalid(format!(
            "the {} likelihood has no maximum for these data (too few failures for the model's parameters)",
            family.label()
        ))
    })?;
    if !value.is_finite() {
        return Err(invalid("the likelihood could not be maximized"));
    }
    let h: Vec<f64> = fit.steps(&internal).iter().map(|s| s * 3.0).collect();
    let (_, _, hess) = derivatives(&ll, &internal, &h);
    let neg: Vec<Vec<f64>> = hess.iter().map(|r| r.iter().map(|v| -v).collect()).collect();
    let internal_covariance = inverse_spd(&neg);
    fit.internal = internal;
    fit.log_likelihood = value;
    fit.internal_covariance = internal_covariance;
    fit.sync_original();
    Ok(fit)
}

/// Starting-value conventions of the error distributions.
#[derive(Debug, Clone, Copy)]
enum FamilyKind {
    Normal,
    Logistic,
    Sev,
    Lev,
}

impl From<RegressionFamily> for FamilyKind {
    fn from(f: RegressionFamily) -> Self {
        match f {
            RegressionFamily::Lognormal | RegressionFamily::Normal => Self::Normal,
            RegressionFamily::LogLogistic | RegressionFamily::Logistic => Self::Logistic,
            RegressionFamily::LargestExtremeValue => Self::Lev,
            _ => Self::Sev,
        }
    }
}

impl RegressionFit {
    /// The data the model was fitted to.
    #[must_use]
    pub fn data(&self) -> &RegressionData {
        &self.data
    }

    /// Number of estimated parameters.
    #[must_use]
    pub fn n_parameters(&self) -> usize {
        self.internal.len()
    }

    /// Akaike's criterion `−2ℓ + 2k`.
    #[must_use]
    pub fn aic(&self) -> f64 {
        -2.0 * self.log_likelihood + 2.0 * self.n_parameters() as f64
    }

    /// Total frequency.
    #[must_use]
    pub fn total_weight(&self) -> f64 {
        self.data.observations.iter().map(|o| o.weight).sum()
    }

    fn p(&self) -> usize {
        self.location_std.len()
    }

    fn standard_location(&self, pt: &CovariatePoint) -> Vec<f64> {
        pt.location.iter().zip(&self.location_std).map(|(v, (m, s))| (v - m) / s).collect()
    }

    fn standard_scale(&self, pt: &CovariatePoint) -> Vec<f64> {
        pt.scale.iter().zip(&self.scale_std).map(|(v, (m, s))| (v - m) / s).collect()
    }

    /// `(μ, σ)` at a point under internal parameters `th`.
    fn mu_sigma(&self, th: &[f64], pt: &CovariatePoint) -> (f64, f64) {
        let p = self.p();
        let x = self.standard_location(pt);
        let mu = pt.offset + x.iter().zip(&th[..p]).map(|(a, b)| a * b).sum::<f64>();
        let sigma = if self.family.fixed_scale() {
            1.0
        } else {
            let z = self.standard_scale(pt);
            z.iter().zip(&th[p..]).map(|(a, b)| a * b).sum::<f64>().exp()
        };
        (mu, sigma)
    }

    fn log_likelihood_internal(&self, th: &[f64]) -> f64 {
        let mut total = 0.0;
        for (o, pt) in self.data.observations.iter().zip(&self.data.points) {
            let (mu, sigma) = self.mu_sigma(th, pt);
            if !(sigma > 0.0 && sigma.is_finite() && mu.is_finite()) {
                return f64::NEG_INFINITY;
            }
            let c = contribution(&self.family.distribution(mu, sigma), o);
            if !c.is_finite() {
                return f64::NEG_INFINITY;
            }
            total += o.weight * c;
        }
        if total.is_nan() { f64::NEG_INFINITY } else { total }
    }

    fn steps(&self, th: &[f64]) -> Vec<f64> {
        let p = self.p();
        let base = if self.family.fixed_scale() { 1.0 } else { th[p].exp().clamp(1e-6, 1e6) };
        (0..th.len()).map(|i| if i < p { 1e-4 * base } else { 1e-4 }).collect()
    }

    /// Representative value of an observation on the model scale (for starts).
    fn representative(&self, o: &LifeObservation) -> f64 {
        let log = self.family.is_log();
        let v = match o.kind() {
            CensorKind::Exact | CensorKind::Right => o.lower,
            CensorKind::Left if log => 0.5 * o.upper,
            CensorKind::Left => o.upper - 0.5 * o.upper.abs().max(1.0),
            CensorKind::Interval if log && o.lower > 0.0 => (o.lower * o.upper).sqrt(),
            CensorKind::Interval => 0.5 * (o.lower + o.upper),
        };
        if log { v.max(1e-300).ln() } else { v }
    }

    /// The linear map `A` with original = `A` · internal.
    fn transform(&self) -> Vec<Vec<f64>> {
        let p = self.p();
        let q = self.scale_std.len();
        let mut a = vec![vec![0.0; p + q]; p + q];
        for (block, std) in [(0, &self.location_std), (p, &self.scale_std)] {
            for (j, (m, s)) in std.iter().enumerate() {
                a[block + j][block + j] = 1.0 / s;
                if j > 0 {
                    a[block][block + j] = -m / s;
                }
            }
            if !std.is_empty() {
                a[block][block] = 1.0;
            }
        }
        a
    }

    fn sync_original(&mut self) {
        let a = self.transform();
        let n = self.internal.len();
        let orig: Vec<f64> = (0..n).map(|i| (0..n).map(|j| a[i][j] * self.internal[j]).sum()).collect();
        let p = self.p();
        self.location_coefficients = orig[..p].to_vec();
        self.scale_coefficients = orig[p..].to_vec();
        self.covariance = self.internal_covariance.as_ref().map(|c| {
            (0..n)
                .map(|i| {
                    (0..n)
                        .map(|j| (0..n).map(|k| (0..n).map(|l| a[i][k] * c[k][l] * a[j][l]).sum::<f64>()).sum())
                        .collect()
                })
                .collect()
        });
    }

    /// Location `μ` at a point.
    #[must_use]
    pub fn location(&self, pt: &CovariatePoint) -> f64 {
        self.mu_sigma(&self.internal, pt).0
    }

    /// Scale `σ` at a point.
    #[must_use]
    pub fn sigma(&self, pt: &CovariatePoint) -> f64 {
        self.mu_sigma(&self.internal, pt).1
    }

    /// The fitted distribution at a point.
    #[must_use]
    pub fn distribution_at(&self, pt: &CovariatePoint) -> FittedDistribution {
        let (mu, sigma) = self.mu_sigma(&self.internal, pt);
        self.family.distribution(mu, sigma)
    }

    /// Delta-method standard error of `g(θ)` (internal θ).
    fn delta_se(&self, g: &dyn Fn(&[f64]) -> f64) -> f64 {
        let Some(v) = &self.internal_covariance else { return f64::NAN };
        let h = self.steps(&self.internal);
        let mut th = self.internal.clone();
        let mut grad = vec![0.0; th.len()];
        for i in 0..th.len() {
            th[i] = self.internal[i] + h[i];
            let up = g(&th);
            th[i] = self.internal[i] - h[i];
            let down = g(&th);
            th[i] = self.internal[i];
            grad[i] = (up - down) / (2.0 * h[i]);
        }
        let mut var = 0.0;
        for i in 0..grad.len() {
            for j in 0..grad.len() {
                var += grad[i] * v[i][j] * grad[j];
            }
        }
        if var >= 0.0 { var.sqrt() } else { f64::NAN }
    }

    /// Maximum of the log-likelihood over every coordinate but `eliminated`,
    /// which `complete` fills in from the others (`None`: infeasible).
    fn profile(&self, eliminated: usize, complete: &dyn Fn(&mut Vec<f64>) -> bool, warm: &mut Vec<f64>) -> f64 {
        let assemble = |x: &[f64]| -> Option<Vec<f64>> {
            let mut th = Vec::with_capacity(x.len() + 1);
            th.extend_from_slice(&x[..eliminated]);
            th.push(0.0);
            th.extend_from_slice(&x[eliminated..]);
            complete(&mut th).then_some(th)
        };
        let ll = |x: &[f64]| assemble(x).map_or(f64::NEG_INFINITY, |th| self.log_likelihood_internal(&th));
        let steps = |x: &[f64]| {
            let th = assemble(x).unwrap_or_else(|| self.internal.clone());
            let mut h = self.steps(&th);
            h.remove(eliminated);
            h
        };
        match maximize(&ll, warm, &steps) {
            Some((x, v)) => {
                *warm = x;
                v
            }
            None => f64::NEG_INFINITY,
        }
    }

    fn free_start(&self, eliminated: usize) -> Vec<f64> {
        let mut v = self.internal.clone();
        v.remove(eliminated);
        v
    }

    /// Likelihood-ratio limits on a search scale (see `censored_fit`).
    fn lr_limits(
        &self,
        s_hat: f64,
        se: f64,
        level: f64,
        eliminated: usize,
        profile: &dyn Fn(f64, &mut Vec<f64>) -> f64,
    ) -> (f64, f64) {
        let crit = ChiSquared::new(1.0).and_then(|c| c.quantile(level)).unwrap_or(f64::NAN);
        let ll_hat = self.log_likelihood;
        let base = if se.is_finite() && se > 0.0 { se } else { 0.1 * (1.0 + s_hat.abs()) };
        let side = |dir: f64| -> f64 {
            let mut warm = self.free_start(eliminated);
            let h = |s: f64, warm: &mut Vec<f64>| -> f64 {
                let v = profile(s, warm);
                if v.is_nan() || v == f64::NEG_INFINITY { f64::INFINITY } else { 2.0 * (ll_hat - v) - crit }
            };
            let (mut a, mut fa) = (s_hat, -crit);
            let mut delta = base;
            let mut outer = None;
            for _ in 0..60 {
                let s = s_hat + dir * delta;
                let v = h(s, &mut warm);
                if v > 0.0 {
                    outer = Some((s, v));
                    break;
                }
                if v < -crit - 1e-6 * (1.0 + crit) {
                    return f64::NAN;
                }
                (a, fa) = (s, v);
                delta *= 1.6;
            }
            let Some((mut b, mut fb)) = outer else { return dir * f64::INFINITY };
            let tol = 1e-10 * (1.0 + s_hat.abs() + (b - a).abs());
            let mut last_kept = 0i8;
            for _ in 0..200 {
                let c = if fb.is_finite() { (a * fb - b * fa) / (fb - fa) } else { 0.5 * (a + b) };
                let c = if c.is_finite() && (c - a) * (c - b) < 0.0 { c } else { 0.5 * (a + b) };
                let fc = h(c, &mut warm);
                if fc.abs() < 1e-9 || (b - a).abs() < tol {
                    return c;
                }
                if fc > 0.0 {
                    (b, fb) = (c, fc);
                    if last_kept == -1 {
                        fa *= 0.5;
                    }
                    last_kept = -1;
                } else {
                    (a, fa) = (c, fc);
                    if last_kept == 1 && fb.is_finite() {
                        fb *= 0.5;
                    }
                    last_kept = 1;
                }
            }
            0.5 * (a + b)
        };
        (side(-1.0), side(1.0))
    }

    fn z(level: f64) -> f64 {
        Normal::standard().quantile(0.5 + level / 2.0).unwrap_or(f64::NAN)
    }

    /// The linear function `aᵀ(β, γ) + c` of the original coefficients with
    /// its standard error, Wald limits and, when `likelihood_ratio`,
    /// profile-likelihood limits.
    ///
    /// `a` has one entry per coefficient (location first, then log scale);
    /// missing trailing entries are zero.
    #[must_use]
    pub fn linear_function(&self, a: &[f64], c: f64, level: f64, likelihood_ratio: bool) -> IntervalEstimate {
        let n = self.internal.len();
        let t = self.transform();
        let a_full: Vec<f64> = (0..n).map(|i| a.get(i).copied().unwrap_or(0.0)).collect();
        // Internal weights b = Aᵀa.
        let b: Vec<f64> = (0..n).map(|j| (0..n).map(|i| a_full[i] * t[i][j]).sum()).collect();
        let value = |th: &[f64]| c + b.iter().zip(th).map(|(x, y)| x * y).sum::<f64>();
        let estimate = value(&self.internal);
        let se = match &self.internal_covariance {
            Some(v) => {
                let var: f64 = (0..n).map(|i| (0..n).map(|j| b[i] * v[i][j] * b[j]).sum::<f64>()).sum();
                if var >= 0.0 { var.sqrt() } else { f64::NAN }
            }
            None => f64::NAN,
        };
        let z = Self::z(level);
        let wald = (estimate - z * se, estimate + z * se);
        let lr = likelihood_ratio.then(|| {
            let Some(k) = (0..n).filter(|&k| b[k] != 0.0).max_by(|&i, &j| b[i].abs().total_cmp(&b[j].abs())) else {
                return (f64::NAN, f64::NAN);
            };
            let profile = |s: f64, warm: &mut Vec<f64>| {
                let complete = |th: &mut Vec<f64>| {
                    let rest: f64 = (0..n).filter(|&j| j != k).map(|j| b[j] * th[j]).sum();
                    th[k] = (s - c - rest) / b[k];
                    th[k].is_finite()
                };
                self.profile(k, &complete, warm)
            };
            self.lr_limits(estimate, se, level, k, &profile)
        });
        IntervalEstimate { name: "Linear function", estimate, std_error: se, wald, likelihood_ratio: lr }
    }

    /// Set the intercept of `th` so that `μ + w σ = y` at `pt`.
    fn impose(&self, th: &mut [f64], pt: &CovariatePoint, w: f64, y: f64) -> bool {
        th[0] = 0.0;
        let (mu0, sigma) = self.mu_sigma(th, pt);
        th[0] = y - w * sigma - mu0;
        th[0].is_finite()
    }

    /// The `p` quantile `t_p` at a point, with Wald limits (on log time for
    /// the log families) and, when `likelihood_ratio`, profile limits.
    #[must_use]
    pub fn quantile(&self, pt: &CovariatePoint, p: f64, level: f64, likelihood_ratio: bool) -> IntervalEstimate {
        let log = self.family.is_log();
        let w = self.family.standard_quantile(p);
        let y_of = |th: &[f64]| {
            let (mu, sigma) = self.mu_sigma(th, pt);
            mu + w * sigma
        };
        let from_s = |s: f64| if log { s.exp() } else { s };
        let s_hat = y_of(&self.internal);
        let se_s = self.delta_se(&y_of);
        let z = Self::z(level);
        let wald = (from_s(s_hat - z * se_s), from_s(s_hat + z * se_s));
        let lr = likelihood_ratio.then(|| {
            let profile = |s: f64, warm: &mut Vec<f64>| {
                let complete = |th: &mut Vec<f64>| self.impose(th, pt, w, s);
                self.profile(0, &complete, warm)
            };
            let (lo, hi) = self.lr_limits(s_hat, se_s, level, 0, &profile);
            (from_s(lo), from_s(hi))
        });
        let estimate = from_s(s_hat);
        IntervalEstimate {
            name: "Quantile",
            estimate,
            std_error: if log { estimate * se_s } else { se_s },
            wald,
            likelihood_ratio: lr,
        }
    }

    /// The failure probability `F(t)` at a point, with Wald limits on the
    /// logit scale and, when `likelihood_ratio`, profile limits.
    #[must_use]
    pub fn failure_probability(
        &self,
        pt: &CovariatePoint,
        t: f64,
        level: f64,
        likelihood_ratio: bool,
    ) -> IntervalEstimate {
        let log = self.family.is_log();
        let y = if log { if t > 0.0 { t.ln() } else { f64::NEG_INFINITY } } else { t };
        let logit = |f: f64| f.ln() - (-f).ln_1p();
        let expit = |s: f64| {
            if s == f64::INFINITY {
                1.0
            } else if s == f64::NEG_INFINITY {
                0.0
            } else {
                1.0 / (1.0 + (-s).exp())
            }
        };
        let f_of = |th: &[f64]| {
            let (mu, sigma) = self.mu_sigma(th, pt);
            self.family.standard_cdf((y - mu) / sigma)
        };
        let estimate = f_of(&self.internal);
        if !(estimate > 0.0 && estimate < 1.0) {
            let nan = (f64::NAN, f64::NAN);
            return IntervalEstimate {
                name: "Failure probability",
                estimate,
                std_error: f64::NAN,
                wald: nan,
                likelihood_ratio: likelihood_ratio.then_some(nan),
            };
        }
        let s_hat = logit(estimate);
        let se_s = self.delta_se(&|th| logit(f_of(th)));
        let z = Self::z(level);
        let wald = (expit(s_hat - z * se_s), expit(s_hat + z * se_s));
        let lr = likelihood_ratio.then(|| {
            let profile = |s: f64, warm: &mut Vec<f64>| {
                let w = self.family.standard_quantile(expit(s));
                let complete = |th: &mut Vec<f64>| w.is_finite() && self.impose(th, pt, w, y);
                self.profile(0, &complete, warm)
            };
            let (lo, hi) = self.lr_limits(s_hat, se_s, level, 0, &profile);
            (expit(lo), expit(hi))
        });
        IntervalEstimate {
            name: "Failure probability",
            estimate,
            std_error: estimate * (1.0 - estimate) * se_s,
            wald,
            likelihood_ratio: lr,
        }
    }

    /// Delta-method standard error of a function of the original
    /// coefficients `(β, γ)`.
    #[must_use]
    pub fn delta_std_error(&self, g: &dyn Fn(&[f64], &[f64]) -> f64) -> f64 {
        let t = self.transform();
        let p = self.p();
        let n = self.internal.len();
        self.delta_se(&|th: &[f64]| {
            let orig: Vec<f64> = (0..n).map(|i| (0..n).map(|j| t[i][j] * th[j]).sum()).collect();
            g(&orig[..p], &orig[p..])
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With complete data and the normal family, maximum likelihood is
    /// ordinary least squares, with σ̂² = SSE / n.
    #[test]
    fn normal_regression_on_complete_data_is_least_squares() {
        let x = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let y = [2.1, 3.9, 6.2, 7.8, 10.1, 12.2];
        let data = RegressionData {
            observations: y.iter().map(|&v| LifeObservation::exact(v, 1.0)).collect(),
            points: x
                .iter()
                .map(|&v| CovariatePoint { location: vec![1.0, v], scale: vec![1.0], offset: 0.0 })
                .collect(),
        };
        let fit = fit_regression(&data, RegressionFamily::Normal).unwrap();
        let n = x.len() as f64;
        let mx = x.iter().sum::<f64>() / n;
        let my = y.iter().sum::<f64>() / n;
        let sxx: f64 = x.iter().map(|v| (v - mx).powi(2)).sum();
        let b = x.iter().zip(&y).map(|(a, c)| (a - mx) * (c - my)).sum::<f64>() / sxx;
        let a = my - b * mx;
        let sse: f64 = x.iter().zip(&y).map(|(u, v)| (v - a - b * u).powi(2)).sum();
        assert!((fit.location_coefficients[0] - a).abs() < 1e-7);
        assert!((fit.location_coefficients[1] - b).abs() < 1e-7);
        assert!((fit.sigma(&data.points[0]) - (sse / n).sqrt()).abs() < 1e-7);
    }

    #[test]
    fn keys_round_trip() {
        for f in RegressionFamily::LIFE.into_iter().chain(RegressionFamily::REAL_LINE) {
            assert_eq!(RegressionFamily::from_key(f.key()), Some(f));
        }
        assert_eq!(RegressionFamily::from_key("loglogistic"), Some(RegressionFamily::LogLogistic));
        assert!(RegressionFamily::Exponential.fixed_scale() && RegressionFamily::Weibull.is_log());
    }
}
