//! Maximum-likelihood fits of life distributions to censored data: exact
//! failures with right-, left- and interval-censored observations, for the
//! fourteen families of [`stat_core::distribution_fit`] (exponential,
//! Weibull, lognormal, log-logistic, gamma, normal, logistic, smallest and
//! largest extreme value, and the threshold forms), plus the Weibull with a
//! known shape for data with few or no failures.
//!
//! # Likelihood
//! With the family's density `f`, distribution function `F` and survival
//! function `S = 1 − F` (taken from [`FittedDistribution`], so the family
//! definitions are shared with distribution identification and capability),
//! observations weighted by their frequencies `wᵢ` contribute
//!
//! ```text
//! ℓ(θ) = Σ_exact w ln f(t) + Σ_right w ln S(l) + Σ_left w ln F(u) + Σ_interval w ln[F(u) − F(l)]
//! ```
//!
//! (Meeker & Escobar, *Statistical Methods for Reliability Data*, 1998,
//! §2.4, ch. 8; Lawless, *Statistical Models and Methods for Lifetime Data*,
//! 2nd ed., 2003, §2.2). `F(u) − F(l)` is computed from whichever of `F`
//! and `S` is smaller, so long intervals far in either tail keep their
//! precision.
//!
//! # Estimation
//! The likelihood is maximized over unconstrained parameters — `(μ, ln σ)`
//! for the location–scale families (on `ln(t − γ)` for the Weibull,
//! lognormal and log-logistic), `ln θ` for the exponential, `(ln k, ln θ)`
//! for the gamma — by damped Newton (Levenberg–Marquardt) iterations with
//! central-difference derivatives, from moment estimates computed as if every
//! observation were a failure at a representative time (the time itself, the
//! censoring time, the interval's geometric or arithmetic midpoint).
//!
//! A threshold `γ` must lie below `m`, the earliest time by which some unit
//! is known to have failed. As for complete data, the profile likelihood
//! `ℓ_p(γ)` is searched on `γ = m − r·eᵗ` (`r` the range of the data) over
//! a grid of `t`, the largest interior local maximum is refined by
//! golden-section search and then polished jointly. The likelihood can rise
//! without bound as `γ → m` when that earliest time is an exact failure
//! (shapes below one); such boundary solutions are not reported. The
//! exponential with a threshold is the exception: its likelihood rises to a
//! finite maximum at `γ = m` when `m` is an exact failure, so that boundary
//! value is the estimate (with no standard error).
//!
//! # Intervals
//! The covariance of the estimates is the inverse of the observed
//! information `−∇²ℓ` (central differences). **Wald** intervals are formed
//! on the scale where the estimate is unconstrained: `exp(ln θ̂ ± z·SE)` for
//! positive parameters and quantiles, `logit⁻¹(logit F̂ ± z·SE)` for
//! probabilities, the delta method supplying the standard error.
//! **Likelihood-ratio (profile)** intervals are the sets
//! `{c : 2[ℓ(θ̂) − max_{g(θ)=c} ℓ(θ)] ≤ χ²₁(1 − α)}` (Meeker & Escobar §8.3,
//! Appendix B.6.6; Cox & Hinkley 1974, §9.3): for a parameter the other
//! parameters are re-maximized with it held fixed; for the quantile `t_p`,
//! and for the failure probability `F(t)` (the quantile constraint read the
//! other way round), the location (or the gamma scale) is solved from the
//! constraint and the rest re-maximized. Each end is located by bracketing
//! from the Wald interval and Illinois-modified regula falsi.
//!
//! # Weibull with known shape
//! With the shape `β` known, `T^β` is exponential with mean `η = α^β`, so for
//! failures and right-censored times (Nelson 1985, "Weibull analysis of
//! reliability data with few or no failures", *J. Quality Technology* 17,
//! 140–146; also Abernethy, *The New Weibull Handbook*) the estimate is
//! `η̂ = Σ wᵢ tᵢ^β / r` with `r` failures, and the chi-square limits for an
//! exponential mean give `η_L = 2Σ wᵢ tᵢ^β / χ²(C; 2r + 2)` (a one-sided lower
//! bound with confidence `C`, defined even when `r = 0`) and
//! `η_U = 2Σ wᵢ tᵢ^β / χ²(1 − C; 2r)`. The scale limits are `η^{1/β}`;
//! percentiles `α(−ln(1 − p))^{1/β}` and reliabilities `exp(−(t/α)^β)` follow
//! from the scale bound.

use stat_core::distribution_fit::{FitFamily, FittedDistribution};
use stat_core::{ChiSquared, ContinuousDistribution, Normal, Result, StatError};

use crate::life_data::{CensorKind, LifeData, LifeObservation};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

const LN_2: f64 = std::f64::consts::LN_2;
const EULER: f64 = 0.577_215_664_901_532_9;

/// How a family's unconstrained parameters are laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    /// `[μ, ln σ]`, on `ln(t − γ)` when `log`.
    LocationScale { log: bool },
    /// `[ln θ]`.
    Exponential,
    /// `[ln k, ln θ]`.
    Gamma,
}

fn layout(family: FitFamily) -> Layout {
    match family {
        FitFamily::Normal | FitFamily::Logistic | FitFamily::SmallestExtremeValue | FitFamily::LargestExtremeValue => {
            Layout::LocationScale { log: false }
        }
        FitFamily::Lognormal
        | FitFamily::Lognormal3
        | FitFamily::LogLogistic
        | FitFamily::LogLogistic3
        | FitFamily::Weibull
        | FitFamily::Weibull3 => Layout::LocationScale { log: true },
        FitFamily::Exponential | FitFamily::Exponential2 => Layout::Exponential,
        FitFamily::Gamma | FitFamily::Gamma3 => Layout::Gamma,
    }
}

/// Number of unconstrained parameters of the family without its threshold.
fn base_dim(family: FitFamily) -> usize {
    if layout(family) == Layout::Exponential { 1 } else { 2 }
}

/// The distribution with unconstrained parameters `th` (threshold last).
fn build(family: FitFamily, th: &[f64]) -> FittedDistribution {
    let threshold = if family.has_threshold() { th[th.len() - 1] } else { 0.0 };
    let mut d =
        FittedDistribution { family, threshold, location: 0.0, scale: 1.0, shape: 1.0, log_likelihood: f64::NAN, n: 0 };
    match layout(family) {
        Layout::LocationScale { .. } => {
            d.location = th[0];
            d.scale = th[1].exp();
        }
        Layout::Exponential => d.scale = th[0].exp(),
        Layout::Gamma => {
            d.shape = th[0].exp();
            d.scale = th[1].exp();
        }
    }
    d
}

/// `ln(1 − eˣ)` for `x ≤ 0` (`−∞` at `x ≥ 0`).
fn ln_one_minus_exp(x: f64) -> f64 {
    if x >= 0.0 {
        f64::NEG_INFINITY
    } else if x > -LN_2 {
        (-x.exp_m1()).ln()
    } else {
        (-x.exp()).ln_1p()
    }
}

/// `ln[F(u) − F(l)]`.
fn ln_interval_probability(d: &FittedDistribution, l: f64, u: f64) -> f64 {
    let fu = d.ln_cdf(u);
    if fu < -LN_2 {
        fu + ln_one_minus_exp(d.ln_cdf(l) - fu)
    } else {
        let sl = d.ln_sf(l);
        sl + ln_one_minus_exp(d.ln_sf(u) - sl)
    }
}

/// One observation's log-likelihood contribution (unweighted).
pub(crate) fn contribution(d: &FittedDistribution, o: &LifeObservation) -> f64 {
    match o.kind() {
        CensorKind::Exact => d.ln_pdf(o.lower),
        CensorKind::Right => d.ln_sf(o.lower),
        CensorKind::Left => d.ln_cdf(o.upper),
        CensorKind::Interval => ln_interval_probability(d, o.lower, o.upper),
    }
}

/// The censored-data log-likelihood of `d` (see the module docs); `−∞` when
/// an observation is impossible under `d`.
#[must_use]
pub fn censored_log_likelihood(data: &LifeData, d: &FittedDistribution) -> f64 {
    let mut total = 0.0;
    for o in data.observations() {
        let c = contribution(d, o);
        if !c.is_finite() {
            return if c == f64::INFINITY { f64::INFINITY } else { f64::NEG_INFINITY };
        }
        total += o.weight * c;
    }
    if total.is_nan() { f64::NEG_INFINITY } else { total }
}

// ----------------------------------------------------------------------------
// Numerical maximization
// ----------------------------------------------------------------------------

/// Value, central-difference gradient and Hessian of `f` at `x` with steps `h`.
pub(crate) fn derivatives(f: &dyn Fn(&[f64]) -> f64, x: &[f64], h: &[f64]) -> (f64, Vec<f64>, Vec<Vec<f64>>) {
    let d = x.len();
    let f0 = f(x);
    let mut g = vec![0.0; d];
    let mut hess = vec![vec![0.0; d]; d];
    let mut at = x.to_vec();
    let mut plus = vec![0.0; d];
    let mut minus = vec![0.0; d];
    for i in 0..d {
        at[i] = x[i] + h[i];
        plus[i] = f(&at);
        at[i] = x[i] - h[i];
        minus[i] = f(&at);
        // Gradient: Richardson extrapolation of central differences at h
        // and h/2 (error O(h⁴)), so the maximizer is located far more
        // precisely than the step alone would allow.
        at[i] = x[i] + 0.5 * h[i];
        let half_plus = f(&at);
        at[i] = x[i] - 0.5 * h[i];
        let half_minus = f(&at);
        at[i] = x[i];
        let coarse = (plus[i] - minus[i]) / (2.0 * h[i]);
        let fine = (half_plus - half_minus) / h[i];
        g[i] = (4.0 * fine - coarse) / 3.0;
        hess[i][i] = (plus[i] - 2.0 * f0 + minus[i]) / (h[i] * h[i]);
    }
    for i in 0..d {
        for j in i + 1..d {
            let mut eval = |si: f64, sj: f64| {
                at[i] = x[i] + si * h[i];
                at[j] = x[j] + sj * h[j];
                let v = f(&at);
                at[i] = x[i];
                at[j] = x[j];
                v
            };
            let v = (eval(1.0, 1.0) - eval(1.0, -1.0) - eval(-1.0, 1.0) + eval(-1.0, -1.0)) / (4.0 * h[i] * h[j]);
            hess[i][j] = v;
            hess[j][i] = v;
        }
    }
    (f0, g, hess)
}

/// Solve `a·x = b` for a small symmetric positive definite `a` by Cholesky;
/// `None` when `a` is not positive definite.
fn solve_spd(a: &[Vec<f64>], b: &[f64]) -> Option<Vec<f64>> {
    let n = b.len();
    let mut l = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..=i {
            let s: f64 = (0..j).map(|k| l[i][k] * l[j][k]).sum();
            if i == j {
                let v = a[i][i] - s;
                if !(v > 0.0 && v.is_finite()) {
                    return None;
                }
                l[i][i] = v.sqrt();
            } else {
                l[i][j] = (a[i][j] - s) / l[j][j];
            }
        }
    }
    let mut y = vec![0.0; n];
    for i in 0..n {
        y[i] = (b[i] - (0..i).map(|k| l[i][k] * y[k]).sum::<f64>()) / l[i][i];
    }
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        x[i] = (y[i] - (i + 1..n).map(|k| l[k][i] * x[k]).sum::<f64>()) / l[i][i];
    }
    Some(x)
}

/// Inverse of a small symmetric positive definite matrix.
pub(crate) fn inverse_spd(a: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let n = a.len();
    let mut cols = Vec::with_capacity(n);
    for j in 0..n {
        let mut e = vec![0.0; n];
        e[j] = 1.0;
        cols.push(solve_spd(a, &e)?);
    }
    Some((0..n).map(|i| (0..n).map(|j| 0.5 * (cols[j][i] + cols[i][j])).collect()).collect())
}

/// Maximize `f` from `x0` by Levenberg–Marquardt-damped Newton steps with
/// numerical derivatives (`steps` gives the difference step of each
/// coordinate at a point). Returns the maximizer and the maximum.
pub(crate) fn maximize(
    f: &dyn Fn(&[f64]) -> f64,
    x0: &[f64],
    steps: &dyn Fn(&[f64]) -> Vec<f64>,
) -> Option<(Vec<f64>, f64)> {
    let d = x0.len();
    let mut x = x0.to_vec();
    let mut value = f(&x);
    if d == 0 {
        return value.is_finite().then_some((x, value));
    }
    if !value.is_finite() {
        return None;
    }
    let mut damping = 1e-6;
    for _ in 0..300 {
        let (_, g, h) = derivatives(f, &x, &steps(&x));
        if g.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let neg: Vec<Vec<f64>> = h.iter().map(|r| r.iter().map(|v| -v).collect()).collect();
        let mut accepted = false;
        for _ in 0..40 {
            let a: Vec<Vec<f64>> = (0..d)
                .map(|i| {
                    (0..d)
                        .map(|j| if i == j { neg[i][i] + damping * (neg[i][i].abs() + 1e-8) } else { neg[i][j] })
                        .collect()
                })
                .collect();
            let Some(step) = solve_spd(&a, &g) else {
                damping = (damping * 10.0).max(1e-4);
                continue;
            };
            let trial: Vec<f64> = x.iter().zip(&step).map(|(a, b)| a + b).collect();
            let v = f(&trial);
            if v.is_finite() && v >= value - 1e-12 * (1.0 + value.abs()) {
                let decrement: f64 = step.iter().zip(&g).map(|(s, gi)| s * gi).sum();
                x = trial;
                value = v.max(value);
                damping = (damping * 0.1).max(1e-12);
                accepted = true;
                if decrement.abs() < 1e-11 * (1.0 + value.abs()) {
                    return Some((x, value));
                }
                break;
            }
            damping = (damping * 10.0).max(1e-4);
        }
        if !accepted {
            // No ascent direction left at the resolution of the differences.
            let small = g.iter().zip(steps(&x)).all(|(gi, hi)| (gi * hi).abs() < 1e-7 * (1.0 + value.abs()));
            return small.then_some((x, value));
        }
    }
    None
}

// ----------------------------------------------------------------------------
// Fits
// ----------------------------------------------------------------------------

/// A maximum-likelihood fit to censored data.
#[derive(Debug, Clone, PartialEq)]
pub struct CensoredFit {
    /// The fitted distribution (its `log_likelihood` is the censored-data
    /// maximum and `n` the total frequency, rounded).
    pub distribution: FittedDistribution,
    /// Covariance of the unconstrained parameters (`None` when the observed
    /// information is not positive definite).
    pub covariance: Option<Vec<Vec<f64>>>,
    /// `true` when the threshold sits at its upper limit (the exponential
    /// with a threshold whose earliest time is a failure); it then has no
    /// standard error and is held fixed in profile intervals.
    pub threshold_at_boundary: bool,
    internal: Vec<f64>,
    data: LifeData,
}

/// An estimate with its standard error and Wald and likelihood-ratio limits.
#[derive(Debug, Clone, PartialEq)]
pub struct IntervalEstimate {
    /// Display name (for parameters).
    pub name: &'static str,
    /// Point estimate.
    pub estimate: f64,
    /// Standard error (delta method), NaN when unavailable.
    pub std_error: f64,
    /// Wald limits.
    pub wald: (f64, f64),
    /// Likelihood-ratio limits (`None` when not requested; NaN ends where
    /// the profile could not be followed).
    pub likelihood_ratio: Option<(f64, f64)>,
}

/// How a reported parameter depends on its unconstrained coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Transform {
    Identity,
    Exp,
    /// `exp(−x)` (Weibull shape `β = 1/σ`).
    ExpNeg,
}

impl Transform {
    fn apply(self, x: f64) -> f64 {
        match self {
            Self::Identity => x,
            Self::Exp => x.exp(),
            Self::ExpNeg => (-x).exp(),
        }
    }
}

/// The reported parameters as `(name, coordinate, transform)`, in the order
/// of [`FittedDistribution::parameters`].
fn reported(family: FitFamily) -> Vec<(&'static str, usize, Transform)> {
    let mut out = match family {
        FitFamily::Normal | FitFamily::Logistic | FitFamily::SmallestExtremeValue | FitFamily::LargestExtremeValue => {
            vec![("Location μ", 0, Transform::Identity), ("Scale σ", 1, Transform::Exp)]
        }
        FitFamily::Lognormal | FitFamily::Lognormal3 | FitFamily::LogLogistic | FitFamily::LogLogistic3 => {
            vec![("Location μ (log scale)", 0, Transform::Identity), ("Scale σ (log scale)", 1, Transform::Exp)]
        }
        FitFamily::Weibull | FitFamily::Weibull3 => {
            vec![("Shape β", 1, Transform::ExpNeg), ("Scale α", 0, Transform::Exp)]
        }
        FitFamily::Exponential | FitFamily::Exponential2 => vec![("Scale θ (mean)", 0, Transform::Exp)],
        FitFamily::Gamma | FitFamily::Gamma3 => vec![("Shape k", 0, Transform::Exp), ("Scale θ", 1, Transform::Exp)],
    };
    if family.has_threshold() {
        out.push(("Threshold γ", base_dim(family), Transform::Identity));
    }
    out
}

/// Representative "as if failed" times for starting values.
fn pseudo_values(data: &LifeData, positive: bool, shift: f64) -> Vec<(f64, f64)> {
    let (lo, hi) = data.finite_range();
    let spread = (hi - lo).abs().max(1e-3 * hi.abs()).max(1e-12);
    data.observations()
        .iter()
        .filter_map(|o| {
            let v = match o.kind() {
                CensorKind::Exact | CensorKind::Right => o.lower,
                CensorKind::Left if positive => 0.5 * (o.upper + shift),
                CensorKind::Left => o.upper - 0.5 * spread,
                CensorKind::Interval if positive && o.lower > shift => {
                    shift + ((o.lower - shift) * (o.upper - shift)).sqrt()
                }
                CensorKind::Interval => 0.5 * (o.lower + o.upper),
            };
            let v = v - shift;
            (!positive || v > 0.0).then_some((v, o.weight))
        })
        .collect()
}

fn weighted_moments(values: &[(f64, f64)]) -> Option<(f64, f64)> {
    let w: f64 = values.iter().map(|v| v.1).sum();
    if w.is_nan() || w <= 0.0 {
        return None;
    }
    let mean = values.iter().map(|(x, wi)| x * wi).sum::<f64>() / w;
    let var = values.iter().map(|(x, wi)| wi * (x - mean).powi(2)).sum::<f64>() / w;
    Some((mean, var.sqrt()))
}

/// Moment starting values of the non-threshold coordinates, with threshold `gamma`.
fn start_values(data: &LifeData, family: FitFamily, gamma: f64) -> Option<Vec<f64>> {
    let lay = layout(family);
    let positive = !matches!(lay, Layout::LocationScale { log: false });
    let values = pseudo_values(data, positive, gamma);
    match lay {
        Layout::LocationScale { log } => {
            let y: Vec<(f64, f64)> = values.iter().map(|&(v, w)| (if log { v.ln() } else { v }, w)).collect();
            let (mean, mut sd) = weighted_moments(&y)?;
            if !(sd > 0.0 && sd.is_finite()) {
                sd = if log { 0.5 } else { mean.abs().max(1.0) * 0.1 };
            }
            let pi = std::f64::consts::PI;
            let (mu, sigma) = match family {
                FitFamily::Normal | FitFamily::Lognormal | FitFamily::Lognormal3 => (mean, sd),
                FitFamily::Logistic | FitFamily::LogLogistic | FitFamily::LogLogistic3 => (mean, sd * 3f64.sqrt() / pi),
                FitFamily::LargestExtremeValue => {
                    let s = sd * 6f64.sqrt() / pi;
                    (mean - EULER * s, s)
                }
                _ => {
                    let s = sd * 6f64.sqrt() / pi;
                    (mean + EULER * s, s)
                }
            };
            Some(vec![mu, sigma.ln()])
        }
        Layout::Exponential => {
            let (mean, _) = weighted_moments(&values)?;
            (mean > 0.0).then(|| vec![mean.ln()])
        }
        Layout::Gamma => {
            let (mean, mut sd) = weighted_moments(&values)?;
            if sd.is_nan() || sd <= 0.0 {
                sd = 0.5 * mean;
            }
            let k = (mean / sd).powi(2);
            (mean > 0.0).then(|| vec![k.ln(), (mean / k).ln()])
        }
    }
}

/// Difference steps for the unconstrained coordinates at `th`.
fn difference_steps(family: FitFamily, th: &[f64], bound: f64) -> Vec<f64> {
    let mut h = match layout(family) {
        Layout::LocationScale { .. } => vec![1e-4 * th[1].exp(), 1e-4],
        Layout::Exponential => vec![1e-4],
        Layout::Gamma => vec![1e-4, 1e-4],
    };
    if family.has_threshold() {
        let gamma = th[th.len() - 1];
        let gap = (bound - gamma).abs();
        h.push(1e-4 * gap.max(1e-300));
    }
    h
}

fn check_support(data: &LifeData, family: FitFamily) -> Result<()> {
    if data.earliest_failure_bound().is_none() {
        return Err(invalid(
            "every unit is right-censored, so a distribution cannot be estimated (a Weibull with a known shape can be)",
        ));
    }
    if family.needs_positive() {
        for o in data.observations() {
            let bad = match o.kind() {
                CensorKind::Exact => o.lower <= 0.0,
                CensorKind::Left | CensorKind::Interval => o.upper <= 0.0,
                CensorKind::Right => false,
            };
            if bad {
                return Err(invalid(format!("the {} distribution needs positive failure times", family.label())));
            }
        }
    }
    Ok(())
}

fn no_maximum(family: FitFamily) -> StatError {
    invalid(format!(
        "the {} likelihood has no maximum for these data (too few failures, or the failures pin the distribution \
         down too little)",
        family.label()
    ))
}

/// Fit `family` to censored life data by maximum likelihood (see the module docs).
///
/// # Errors
/// [`StatError::InvalidArgument`] when every unit is right-censored, when a
/// family on positive times meets a failure at or before zero, or when the
/// likelihood has no maximum (e.g. one failure for a two-parameter family);
/// [`StatError::ConvergenceFailure`] when a threshold form's profile
/// likelihood has no interior maximum.
pub fn fit_censored(data: &LifeData, family: FitFamily) -> Result<CensoredFit> {
    check_support(data, family)?;
    let ll = |th: &[f64]| censored_log_likelihood(data, &build(family, th));
    let bound = data.earliest_failure_bound().unwrap_or(f64::INFINITY);
    let (internal, value, at_boundary) = if family.has_threshold() {
        fit_threshold(data, family, bound)?
    } else {
        let start = start_values(data, family, 0.0).ok_or_else(|| no_maximum(family))?;
        let steps = |th: &[f64]| difference_steps(family, th, bound);
        let mut best: Option<(Vec<f64>, f64)> = None;
        for factor in [0.0, 0.7, -0.7, 1.5, -1.5] {
            let mut x0 = start.clone();
            // Vary the spread of the start (ln σ, ln θ or ln k).
            let k = if layout(family) == Layout::Exponential { 0 } else { x0.len() - 1 };
            x0[k] += factor;
            if let Some(m) = maximize(&ll, &x0, &steps) {
                if best.as_ref().is_none_or(|b| m.1 > b.1 + 1e-9) {
                    best = Some(m);
                }
                if factor == 0.0 {
                    // A clean start usually suffices; confirm with one other.
                    continue;
                }
                break;
            }
        }
        let (x, v) = best.ok_or_else(|| no_maximum(family))?;
        (x, v, false)
    };
    finish(data, family, internal, value, at_boundary, bound)
}

fn finish(
    data: &LifeData,
    family: FitFamily,
    internal: Vec<f64>,
    value: f64,
    at_boundary: bool,
    bound: f64,
) -> Result<CensoredFit> {
    let free = if at_boundary { internal.len() - 1 } else { internal.len() };
    let ll_free = |x: &[f64]| {
        let mut th = internal.clone();
        th[..free].copy_from_slice(x);
        censored_log_likelihood(data, &build(family, &th))
    };
    let h: Vec<f64> = difference_steps(family, &internal, bound).iter().map(|s| s * 3.0).take(free).collect();
    let (_, _, hess) = derivatives(&ll_free, &internal[..free], &h);
    let neg: Vec<Vec<f64>> = hess.iter().map(|r| r.iter().map(|v| -v).collect()).collect();
    let covariance = inverse_spd(&neg).map(|mut v| {
        if at_boundary {
            for row in &mut v {
                row.push(f64::NAN);
            }
            v.push(vec![f64::NAN; free + 1]);
        }
        v
    });
    let mut distribution = build(family, &internal);
    distribution.log_likelihood = value;
    distribution.n = data.total_weight().round() as usize;
    if !value.is_finite() {
        return Err(no_maximum(family));
    }
    Ok(CensoredFit { distribution, covariance, threshold_at_boundary: at_boundary, internal, data: data.clone() })
}

/// Profile-likelihood search over the threshold (see the module docs).
fn fit_threshold(data: &LifeData, family: FitFamily, bound: f64) -> Result<(Vec<f64>, f64, bool)> {
    let k = base_dim(family);
    let (lo, hi) = data.finite_range();
    let range = if hi > lo { hi - lo } else { bound.abs().max(1.0) };
    let inner = |gamma: f64, warm: Option<&[f64]>| -> Option<(Vec<f64>, f64)> {
        let ll = |x: &[f64]| {
            let mut th = x.to_vec();
            th.push(gamma);
            censored_log_likelihood(data, &build(family, &th))
        };
        let steps = |x: &[f64]| {
            let mut th = x.to_vec();
            th.push(gamma);
            difference_steps(family, &th, bound)[..k].to_vec()
        };
        let mut starts: Vec<Vec<f64>> = Vec::new();
        if let Some(w) = warm {
            starts.push(w.to_vec());
        }
        if let Some(s) = start_values(data, family, gamma) {
            starts.push(s);
        }
        let mut best: Option<(Vec<f64>, f64)> = None;
        for s in starts {
            if let Some(m) = maximize(&ll, &s, &steps) {
                if best.as_ref().is_none_or(|b| m.1 > b.1) {
                    best = Some(m);
                }
                if warm.is_some() {
                    break;
                }
            }
        }
        best
    };
    let gamma_of = |t: f64| bound - range * t.exp();
    let grid: Vec<f64> = (0..=32).map(|i| 5.0 - 0.5 * f64::from(i)).collect();
    let mut values: Vec<Option<(Vec<f64>, f64)>> = Vec::with_capacity(grid.len());
    let mut warm: Option<Vec<f64>> = None;
    for &t in &grid {
        let r = inner(gamma_of(t), warm.as_deref());
        if let Some((x, _)) = &r {
            warm = Some(x.clone());
        }
        values.push(r);
    }
    let ll = |i: usize| values[i].as_ref().map_or(f64::NEG_INFINITY, |v| v.1);
    // The exponential's likelihood rises to a finite value at γ = m.
    if layout(family) == Layout::Exponential {
        let earliest_exact =
            data.observations().iter().filter(|o| o.kind() == CensorKind::Exact).any(|o| o.lower == bound);
        if earliest_exact {
            if let Some((x, v)) = inner(bound, warm.as_deref()) {
                if (0..grid.len()).all(|i| ll(i) <= v + 1e-12 * v.abs()) {
                    let mut th = x;
                    th.push(bound);
                    return Ok((th, v, true));
                }
            }
        }
    }
    // Interior local maxima (grid runs from far below m towards m).
    let mut best: Option<usize> = None;
    for i in 1..grid.len() - 1 {
        if ll(i).is_finite() && ll(i) >= ll(i - 1) && ll(i) >= ll(i + 1) && best.is_none_or(|b| ll(i) > ll(b)) {
            best = Some(i);
        }
    }
    let Some(i) = best else {
        return Err(StatError::ConvergenceFailure { routine: "threshold profile likelihood", iterations: grid.len() });
    };
    // Golden-section refinement over t in [t_{i+1}, t_{i−1}].
    let (mut a, mut b) = (grid[i + 1], grid[i - 1]);
    let ratio = (5f64.sqrt() - 1.0) / 2.0;
    let mut warm = values[i].as_ref().map(|v| v.0.clone());
    let eval = |t: f64, warm: &mut Option<Vec<f64>>| -> (f64, Option<Vec<f64>>) {
        match inner(gamma_of(t), warm.as_deref()) {
            Some((x, v)) => {
                *warm = Some(x.clone());
                (v, Some(x))
            }
            None => (f64::NEG_INFINITY, None),
        }
    };
    let mut c = b - ratio * (b - a);
    let mut d = a + ratio * (b - a);
    let (mut fc, mut fd) = (eval(c, &mut warm).0, eval(d, &mut warm).0);
    for _ in 0..100 {
        if (b - a).abs() < 1e-10 {
            break;
        }
        if fc >= fd {
            b = d;
            d = c;
            fd = fc;
            c = b - ratio * (b - a);
            fc = eval(c, &mut warm).0;
        } else {
            a = c;
            c = d;
            fc = fd;
            d = a + ratio * (b - a);
            fd = eval(d, &mut warm).0;
        }
    }
    let t = 0.5 * (a + b);
    let (v, x) = eval(t, &mut warm);
    let (mut th, mut value) = match x {
        Some(x) if v >= ll(i) => {
            let mut th = x;
            th.push(gamma_of(t));
            (th, v)
        }
        _ => {
            let (x, v) = values[i].clone().ok_or(StatError::ConvergenceFailure {
                routine: "threshold profile likelihood",
                iterations: grid.len(),
            })?;
            let mut th = x;
            th.push(gamma_of(grid[i]));
            (th, v)
        }
    };
    // Joint polish, kept only when it stays near the profile solution.
    let ll_full = |x: &[f64]| censored_log_likelihood(data, &build(family, x));
    let steps = |x: &[f64]| difference_steps(family, x, bound);
    if let Some((x, v)) = maximize(&ll_full, &th, &steps) {
        let gap = bound - th[k];
        if v >= value && (x[k] - th[k]).abs() < 0.1 * gap && x[k] < bound {
            th = x;
            value = v;
        }
    }
    Ok((th, value, false))
}

impl CensoredFit {
    /// The family.
    #[must_use]
    pub fn family(&self) -> FitFamily {
        self.distribution.family
    }

    /// Maximized log-likelihood.
    #[must_use]
    pub fn log_likelihood(&self) -> f64 {
        self.distribution.log_likelihood
    }

    /// Akaike's criterion `−2ℓ + 2p`.
    #[must_use]
    pub fn aic(&self) -> f64 {
        self.distribution.aic()
    }

    /// Schwarz's criterion `−2ℓ + p ln W` with `W` the total frequency.
    #[must_use]
    pub fn bic(&self) -> f64 {
        -2.0 * self.log_likelihood() + self.family().n_parameters() as f64 * self.data.total_weight().ln()
    }

    /// The data the fit was made to.
    #[must_use]
    pub fn data(&self) -> &LifeData {
        &self.data
    }

    /// The unconstrained parameter vector.
    #[must_use]
    pub fn internal(&self) -> &[f64] {
        &self.internal
    }

    /// Hazard `h(t) = f(t)/S(t)`.
    #[must_use]
    pub fn hazard(&self, t: f64) -> f64 {
        (self.distribution.ln_pdf(t) - self.distribution.ln_sf(t)).exp()
    }

    fn bound(&self) -> f64 {
        self.data.earliest_failure_bound().unwrap_or(f64::INFINITY)
    }

    /// Delta-method standard error of `g(θ)` (θ unconstrained), NaN when the
    /// covariance is unavailable.
    fn delta_se(&self, g: &dyn Fn(&FittedDistribution) -> f64) -> f64 {
        let Some(v) = &self.covariance else { return f64::NAN };
        let family = self.family();
        let h = difference_steps(family, &self.internal, self.bound());
        let free = if self.threshold_at_boundary { self.internal.len() - 1 } else { self.internal.len() };
        let mut grad = vec![0.0; free];
        let mut th = self.internal.clone();
        for i in 0..free {
            th[i] = self.internal[i] + h[i];
            let up = g(&build(family, &th));
            th[i] = self.internal[i] - h[i];
            let down = g(&build(family, &th));
            th[i] = self.internal[i];
            grad[i] = (up - down) / (2.0 * h[i]);
        }
        let mut var = 0.0;
        for i in 0..free {
            for j in 0..free {
                var += grad[i] * v[i][j] * grad[j];
            }
        }
        if var >= 0.0 { var.sqrt() } else { f64::NAN }
    }

    /// The unconstrained vector with `x` in the free slots (every slot but
    /// `eliminated`), the eliminated slots holding the estimates.
    fn assemble(&self, eliminated: &[usize], x: &[f64]) -> Vec<f64> {
        let mut th = self.internal.clone();
        let mut it = x.iter();
        for (i, slot) in th.iter_mut().enumerate() {
            if !eliminated.contains(&i) {
                if let Some(v) = it.next() {
                    *slot = *v;
                }
            }
        }
        th
    }

    /// Coordinates held out of a profile: `own`, and the threshold when it
    /// sits at its boundary.
    fn held(&self, own: usize) -> Vec<usize> {
        let mut e = vec![own];
        if self.threshold_at_boundary {
            e.push(self.internal.len() - 1);
        }
        e.sort_unstable();
        e.dedup();
        e
    }

    /// The profile log-likelihood: the maximum over the free coordinates of
    /// `ℓ(complete(assemble(x)))`, warm-started at `warm` (updated).
    fn profile(
        &self,
        eliminated: &[usize],
        complete: &dyn Fn(Vec<f64>) -> Option<Vec<f64>>,
        warm: &mut Vec<f64>,
    ) -> f64 {
        let family = self.family();
        let bound = self.bound();
        let ll = |x: &[f64]| match complete(self.assemble(eliminated, x)) {
            Some(th) => censored_log_likelihood(&self.data, &build(family, &th)),
            None => f64::NEG_INFINITY,
        };
        let steps = |x: &[f64]| -> Vec<f64> {
            let th = complete(self.assemble(eliminated, x)).unwrap_or_else(|| self.internal.clone());
            difference_steps(family, &th, bound)
                .into_iter()
                .enumerate()
                .filter(|(i, _)| !eliminated.contains(i))
                .map(|(_, v)| v)
                .collect()
        };
        match maximize(&ll, warm, &steps) {
            Some((x, v)) => {
                *warm = x;
                v
            }
            None => f64::NEG_INFINITY,
        }
    }

    fn free_start(&self, eliminated: &[usize]) -> Vec<f64> {
        self.internal.iter().enumerate().filter(|(i, _)| !eliminated.contains(i)).map(|(_, v)| *v).collect()
    }

    /// Likelihood-ratio limits on a search scale `s` (estimate `s_hat`,
    /// Wald standard error `se`), where `profile(s, warm)` is the profile
    /// log-likelihood. Unbounded sides give `∓∞`; a side whose profile
    /// cannot be followed gives NaN.
    fn lr_limits(
        &self,
        s_hat: f64,
        se: f64,
        level: f64,
        eliminated: &[usize],
        profile: &dyn Fn(f64, &mut Vec<f64>) -> f64,
    ) -> (f64, f64) {
        let crit = ChiSquared::new(1.0).and_then(|c| c.quantile(level)).unwrap_or(f64::NAN);
        let ll_hat = self.log_likelihood();
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
                    // A higher likelihood than the estimate's: the profile left
                    // the maximum's neighbourhood.
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

    /// The fitted parameters in their usual form (as in
    /// [`FittedDistribution::parameters`]) with standard errors, Wald limits
    /// formed on the unconstrained scale, and, when `likelihood_ratio`,
    /// profile-likelihood limits, at confidence `level`.
    #[must_use]
    pub fn parameters(&self, level: f64, likelihood_ratio: bool) -> Vec<IntervalEstimate> {
        let z = Normal::standard().quantile(0.5 + level / 2.0).unwrap_or(f64::NAN);
        let threshold_index = self.family().has_threshold().then(|| self.internal.len() - 1);
        reported(self.family())
            .into_iter()
            .map(|(name, c, tr)| {
                let x = self.internal[c];
                let estimate = tr.apply(x);
                let fixed = self.threshold_at_boundary && Some(c) == threshold_index;
                let se_x = match &self.covariance {
                    Some(v) if !fixed && v[c][c] >= 0.0 => v[c][c].sqrt(),
                    _ => f64::NAN,
                };
                let order = |a: f64, b: f64| if a <= b { (a, b) } else { (b, a) };
                let wald = order(tr.apply(x - z * se_x), tr.apply(x + z * se_x));
                let std_error = match tr {
                    Transform::Identity => se_x,
                    Transform::Exp | Transform::ExpNeg => estimate * se_x,
                };
                let lr = (likelihood_ratio && !fixed).then(|| {
                    let held = self.held(c);
                    let profile = |s: f64, warm: &mut Vec<f64>| {
                        let complete = |mut th: Vec<f64>| {
                            th[c] = s;
                            Some(th)
                        };
                        self.profile(&held, &complete, warm)
                    };
                    let (lo, hi) = self.lr_limits(x, se_x, level, &held, &profile);
                    order(tr.apply(lo), tr.apply(hi))
                });
                IntervalEstimate { name, estimate, std_error, wald, likelihood_ratio: lr }
            })
            .collect()
    }

    /// The coordinate solved from a quantile constraint.
    fn quantile_slot(&self) -> usize {
        if layout(self.family()) == Layout::Gamma { 1 } else { 0 }
    }

    /// Solve the eliminated coordinate of `th` so that `F(t) = p`.
    fn impose_quantile(&self, mut th: Vec<f64>, p: f64, t: f64) -> Option<Vec<f64>> {
        let family = self.family();
        if !(p > 0.0 && p < 1.0) {
            return None;
        }
        let gamma = if family.has_threshold() { th[th.len() - 1] } else { 0.0 };
        match layout(family) {
            Layout::LocationScale { log } => {
                let w = build(family, &th).standard_quantile(p);
                let y = if log {
                    if t <= gamma {
                        return None;
                    }
                    (t - gamma).ln()
                } else {
                    t
                };
                th[0] = y - th[1].exp() * w;
            }
            Layout::Exponential => {
                if t <= gamma {
                    return None;
                }
                th[0] = ((t - gamma) / -(-p).ln_1p()).ln();
            }
            Layout::Gamma => {
                if t <= gamma {
                    return None;
                }
                let q = build(family, &th).standard_quantile(p);
                if !(q > 0.0 && q.is_finite()) {
                    return None;
                }
                th[1] = ((t - gamma) / q).ln();
            }
        }
        th.iter().all(|v| v.is_finite()).then_some(th)
    }

    /// `true` when quantiles are positive and estimated on the log scale.
    fn log_quantiles(&self) -> bool {
        self.family().needs_positive()
    }

    /// The `p` quantile `t_p` with Wald limits (on the log scale for the
    /// families on positive times without a threshold) and, when
    /// `likelihood_ratio`, profile limits.
    #[must_use]
    pub fn quantile(&self, p: f64, level: f64, likelihood_ratio: bool) -> IntervalEstimate {
        let z = Normal::standard().quantile(0.5 + level / 2.0).unwrap_or(f64::NAN);
        let estimate = self.distribution.quantile(p);
        let log = self.log_quantiles();
        let to_s = |t: f64| if log { t.ln() } else { t };
        let from_s = |s: f64| if log { s.exp() } else { s };
        let s_hat = to_s(estimate);
        let se_s = self.delta_se(&|d| to_s(d.quantile(p)));
        let wald = (from_s(s_hat - z * se_s), from_s(s_hat + z * se_s));
        let std_error = if log { estimate * se_s } else { se_s };
        let lr = likelihood_ratio.then(|| {
            let held = self.held(self.quantile_slot());
            let profile = |s: f64, warm: &mut Vec<f64>| {
                let t = from_s(s);
                let complete = |th: Vec<f64>| self.impose_quantile(th, p, t);
                self.profile(&held, &complete, warm)
            };
            let (lo, hi) = self.lr_limits(s_hat, se_s, level, &held, &profile);
            (from_s(lo), from_s(hi))
        });
        IntervalEstimate { name: "Quantile", estimate, std_error, wald, likelihood_ratio: lr }
    }

    /// The failure probability `F(t)` with Wald limits on the logit scale
    /// and, when `likelihood_ratio`, profile limits.
    #[must_use]
    pub fn failure_probability(&self, t: f64, level: f64, likelihood_ratio: bool) -> IntervalEstimate {
        let z = Normal::standard().quantile(0.5 + level / 2.0).unwrap_or(f64::NAN);
        let estimate = self.distribution.cdf(t);
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
        let se_s = self.delta_se(&|d| logit(d.cdf(t)));
        let wald = (expit(s_hat - z * se_s), expit(s_hat + z * se_s));
        let lr = likelihood_ratio.then(|| {
            let held = self.held(self.quantile_slot());
            let profile = |s: f64, warm: &mut Vec<f64>| {
                let p = expit(s);
                let complete = |th: Vec<f64>| self.impose_quantile(th, p, t);
                self.profile(&held, &complete, warm)
            };
            let (lo, hi) = self.lr_limits(s_hat, se_s, level, &held, &profile);
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

    /// Delta-method standard error of any smooth function `g` of the fitted
    /// distribution: `√(∇gᵀ V ∇g)` with `V` the covariance of the
    /// unconstrained parameters and `∇g` by central differences. NaN when the
    /// covariance is unavailable. Used, for example, for forecasts that sum
    /// conditional failure probabilities over many units.
    #[must_use]
    pub fn delta_std_error(&self, g: &dyn Fn(&FittedDistribution) -> f64) -> f64 {
        self.delta_se(g)
    }

    /// Pointwise Wald limits of `F(t)` (logit scale), for confidence bands.
    #[must_use]
    pub fn probability_band(&self, t: f64, level: f64) -> (f64, f64) {
        self.failure_probability(t, level, false).wald
    }

    /// Pointwise Wald limits of the hazard `h(t)`, formed on the log scale.
    #[must_use]
    pub fn hazard_band(&self, t: f64, level: f64) -> (f64, f64) {
        let z = Normal::standard().quantile(0.5 + level / 2.0).unwrap_or(f64::NAN);
        let ln_h = |d: &FittedDistribution| d.ln_pdf(t) - d.ln_sf(t);
        let est = ln_h(&self.distribution);
        let se = self.delta_se(&ln_h);
        ((est - z * se).exp(), (est + z * se).exp())
    }
}

/// A likelihood-ratio comparison of a family with one it contains.
#[derive(Debug, Clone, PartialEq)]
pub struct NestedComparison {
    /// The simpler family.
    pub reduced: FitFamily,
    /// The family that contains it.
    pub full: FitFamily,
    /// `2(ℓ_full − ℓ_reduced)` (at least zero).
    pub statistic: f64,
    /// Difference in the number of parameters.
    pub df: usize,
    /// Upper tail of χ² with `df` degrees of freedom.
    pub p_value: f64,
}

/// Pairs `(reduced, full)` where the first family is a special or limiting
/// case of the second: the exponential is the Weibull with `β = 1` and the
/// gamma with `k = 1`; each two-parameter form is its threshold form with
/// `γ = 0`.
pub const NESTED_FAMILIES: [(FitFamily, FitFamily); 7] = [
    (FitFamily::Exponential, FitFamily::Weibull),
    (FitFamily::Exponential, FitFamily::Gamma),
    (FitFamily::Exponential, FitFamily::Exponential2),
    (FitFamily::Weibull, FitFamily::Weibull3),
    (FitFamily::Lognormal, FitFamily::Lognormal3),
    (FitFamily::LogLogistic, FitFamily::LogLogistic3),
    (FitFamily::Gamma, FitFamily::Gamma3),
];

/// The likelihood-ratio test of `reduced` within `full`: `2(ℓ_full − ℓ_red)`
/// referred to χ² with the difference in parameter counts (Wilks 1938). For
/// a threshold the reduced model `γ = 0` is an interior point, so the usual
/// reference distribution applies; it is approximate in small samples.
///
/// # Errors
/// [`StatError::InvalidArgument`] when the pair is not in [`NESTED_FAMILIES`]
/// or the fits are to different data.
pub fn nested_comparison(reduced: &CensoredFit, full: &CensoredFit) -> Result<NestedComparison> {
    let pair = (reduced.family(), full.family());
    if !NESTED_FAMILIES.contains(&pair) {
        return Err(invalid(format!("{} is not a special case of {}", pair.0.label(), pair.1.label())));
    }
    if reduced.data != full.data {
        return Err(invalid("the two fits are to different data"));
    }
    let statistic = (2.0 * (full.log_likelihood() - reduced.log_likelihood())).max(0.0);
    let df = full.family().n_parameters() - reduced.family().n_parameters();
    let p_value = ChiSquared::new(df as f64)?.sf(statistic)?;
    Ok(NestedComparison { reduced: pair.0, full: pair.1, statistic, df, p_value })
}

/// A Weibull fit with a known shape (see the module docs).
#[derive(Debug, Clone, PartialEq)]
pub struct KnownShapeWeibull {
    /// The known shape `β`.
    pub shape: f64,
    /// Number of failures `r` (total frequency of exact failures).
    pub failures: f64,
    /// `Σ wᵢ tᵢ^β` over failures and censored units.
    pub scaled_time: f64,
    /// Confidence level `C`.
    pub level: f64,
    /// Maximum-likelihood scale `α̂ = (Σ wᵢ tᵢ^β / r)^{1/β}` (NaN without failures).
    pub scale: f64,
    /// Two-sided limits for the scale at confidence `C` (upper `+∞` without failures).
    pub scale_interval: (f64, f64),
    /// One-sided lower confidence bound for the scale at confidence `C`.
    pub scale_lower_bound: f64,
}

impl KnownShapeWeibull {
    /// The `p` quantile at the estimate (NaN without failures).
    #[must_use]
    pub fn quantile(&self, p: f64) -> f64 {
        self.scale * (-(-p).ln_1p()).powf(1.0 / self.shape)
    }

    /// One-sided lower confidence bound for the `p` quantile.
    #[must_use]
    pub fn quantile_lower_bound(&self, p: f64) -> f64 {
        self.scale_lower_bound * (-(-p).ln_1p()).powf(1.0 / self.shape)
    }

    /// Reliability `exp(−(t/α̂)^β)` at the estimate (NaN without failures).
    #[must_use]
    pub fn reliability(&self, t: f64) -> f64 {
        (-(t / self.scale).powf(self.shape)).exp()
    }

    /// One-sided lower confidence bound for the reliability at `t`.
    #[must_use]
    pub fn reliability_lower_bound(&self, t: f64) -> f64 {
        (-(t / self.scale_lower_bound).powf(self.shape)).exp()
    }
}

/// Fit a Weibull with known shape `β` to failures and right-censored times
/// (Nelson 1985; see the module docs).
///
/// # Errors
/// [`StatError::InvalidArgument`] for a shape that is not finite and
/// positive, a level outside `(0, 1)`, left- or interval-censored
/// observations, or a time that is not positive.
pub fn weibull_known_shape(data: &LifeData, shape: f64, level: f64) -> Result<KnownShapeWeibull> {
    if !(shape > 0.0 && shape.is_finite()) {
        return Err(invalid("the known Weibull shape must be positive"));
    }
    if !(level > 0.0 && level < 1.0) {
        return Err(invalid("the confidence level must be between 0 and 1"));
    }
    if !data.is_right_censored_only() {
        return Err(invalid(
            "a Weibull with a known shape needs failure times and right-censored times (no left- or interval-censored \
             observations)",
        ));
    }
    let mut failures = 0.0;
    let mut scaled_time = 0.0;
    for o in data.observations() {
        if o.lower <= 0.0 {
            return Err(invalid("a Weibull with a known shape needs positive times"));
        }
        scaled_time += o.weight * o.lower.powf(shape);
        if o.kind() == CensorKind::Exact {
            failures += o.weight;
        }
    }
    let root = |eta: f64| eta.powf(1.0 / shape);
    let chi = |df: f64, p: f64| ChiSquared::new(df).and_then(|c| c.quantile(p));
    let scale = if failures > 0.0 { root(scaled_time / failures) } else { f64::NAN };
    let lower = root(2.0 * scaled_time / chi(2.0 * failures + 2.0, 0.5 + level / 2.0)?);
    let upper =
        if failures > 0.0 { root(2.0 * scaled_time / chi(2.0 * failures, 0.5 - level / 2.0)?) } else { f64::INFINITY };
    let scale_lower_bound = root(2.0 * scaled_time / chi(2.0 * failures + 2.0, level)?);
    Ok(KnownShapeWeibull {
        shape,
        failures,
        scaled_time,
        level,
        scale,
        scale_interval: (lower, upper),
        scale_lower_bound,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_core::distribution_fit::fit_distribution;

    fn complete(x: &[f64]) -> LifeData {
        LifeData::new(x.iter().map(|&t| LifeObservation::exact(t, 1.0)).collect()).unwrap()
    }

    const SAMPLE: [f64; 15] =
        [12.1, 15.3, 18.2, 20.5, 22.4, 25.0, 26.6, 29.9, 31.2, 35.7, 38.8, 43.1, 47.5, 55.2, 64.9];

    /// On complete data the censored likelihood is the ordinary one, so the
    /// fits agree with distribution identification's complete-data fits.
    #[test]
    fn complete_data_matches_complete_fits() {
        let d = complete(&SAMPLE);
        for family in [
            FitFamily::Exponential,
            FitFamily::Weibull,
            FitFamily::Lognormal,
            FitFamily::LogLogistic,
            FitFamily::Gamma,
            FitFamily::Logistic,
            FitFamily::SmallestExtremeValue,
            FitFamily::LargestExtremeValue,
            FitFamily::Weibull3,
            FitFamily::Lognormal3,
        ] {
            let a = fit_censored(&d, family).unwrap();
            let b = fit_distribution(&SAMPLE, family).unwrap();
            assert!((a.log_likelihood() - b.log_likelihood).abs() < 1e-7 * b.log_likelihood.abs(), "{family:?}");
            for (p, q) in a.distribution.parameters().iter().zip(b.parameters()) {
                assert!(
                    (p.value - q.value).abs() < 1e-5 * (1.0 + q.value.abs()),
                    "{family:?} {}: {} vs {}",
                    p.name,
                    p.value,
                    q.value
                );
            }
        }
        // The normal's maximum-likelihood σ uses divisor n.
        let n = fit_censored(&d, FitFamily::Normal).unwrap();
        let mean = SAMPLE.iter().sum::<f64>() / 15.0;
        let var = SAMPLE.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / 15.0;
        assert!((n.distribution.location - mean).abs() < 1e-8 * mean);
        assert!((n.distribution.scale - var.sqrt()).abs() < 1e-7 * var.sqrt());
    }

    /// Exponential with right censoring: θ̂ = total time / failures, with
    /// SE θ̂/√r, and its exact profile interval in closed form.
    #[test]
    fn exponential_closed_form() {
        let mut obs: Vec<LifeObservation> =
            [30.0, 70.0, 110.0, 160.0].iter().map(|&t| LifeObservation::exact(t, 1.0)).collect();
        obs.push(LifeObservation::right(200.0, 6.0));
        let d = LifeData::new(obs).unwrap();
        let fit = fit_censored(&d, FitFamily::Exponential).unwrap();
        let theta = (370.0 + 1200.0) / 4.0;
        assert!((fit.distribution.scale - theta).abs() < 1e-8 * theta);
        let p = fit.parameters(0.95, true);
        assert!((p[0].std_error - theta / 2.0).abs() < 1e-5 * theta);
        // Profile: ℓ(θ) = −r ln θ − T/θ; check the limits solve 2(ℓ̂ − ℓ) = χ²₁.
        let (lo, hi) = p[0].likelihood_ratio.unwrap();
        let ll = |t: f64| -4.0 * t.ln() - 1570.0 / t;
        let crit = ChiSquared::new(1.0).unwrap().quantile(0.95).unwrap();
        for v in [lo, hi] {
            assert!((2.0 * (ll(theta) - ll(v)) - crit).abs() < 1e-7, "{v}");
        }
    }

    #[test]
    fn rejects_unusable_data() {
        let all_right = LifeData::new(vec![LifeObservation::right(5.0, 3.0)]).unwrap();
        assert!(fit_censored(&all_right, FitFamily::Weibull).is_err());
        let negative = complete(&[-1.0, 2.0, 3.0]);
        assert!(fit_censored(&negative, FitFamily::Weibull).is_err());
        assert!(fit_censored(&negative, FitFamily::Normal).is_ok());
        let interval = LifeData::new(vec![LifeObservation::interval(1.0, 2.0, 1.0)]).unwrap();
        assert!(weibull_known_shape(&interval, 2.0, 0.9).is_err());
    }
}
