//! Stress–life (S–N) curves for fatigue tests: cycles to failure `N` of
//! specimens cycled at a stress amplitude `S`, with run-outs (specimens
//! still unbroken when the test was stopped) as right-censored
//! observations, fitted by maximum likelihood with
//! [`crate::life_regression`].
//!
//! # Model
//! Log life is a location–scale variable,
//!
//! ```text
//! ln N = μ(S) + σ(S) ε,
//! ```
//!
//! with `ε` standard normal (lognormal life), smallest extreme value
//! (Weibull) or logistic (log-logistic), and one of four curves for the
//! location (ASTM E739-10, *Standard Practice for Statistical Analysis of
//! Linear or Linearized Stress-Life and Strain-Life Fatigue Data*; Meeker &
//! Escobar, *Statistical Methods for Reliability Data*, 1998, ch. 17;
//! Pascual & Meeker 1999, "Estimating fatigue curves with the random
//! fatigue-limit model", *Technometrics* 41, 277–290):
//!
//! | Form | `μ(S)` |
//! |---|---|
//! | Basquin (1910) | `β₀ + β₁ ln S` |
//! | Semi-log | `β₀ + β₁ S` |
//! | Quadratic log–log | `β₀ + β₁ ln S + β₂ (ln S)²` |
//! | Endurance limit (Stromeyer 1914) | `β₀ + β₁ ln(S − E)`, `0 ≤ E < min S` |
//!
//! The scale `σ` is constant, or optionally `ln σ = γ₀ + γ₁ ln S` (for every
//! form, including the endurance limit, the scale covariate is `ln S`), as
//! fatigue lives usually scatter more at low stress (Meeker & Escobar §17.5).
//!
//! # Endurance limit
//! For a fixed `E` the endurance-limit form is a Basquin curve in `S − E`,
//! so `E` is estimated by profile likelihood: `ℓ_p(E)` = the maximum of the
//! log-likelihood with `E` held fixed. The profile is evaluated on a grid of
//! `v = −ln(1 − E / min S)` (which spreads the grid towards `min S`), the
//! best grid point is refined by golden-section search, and the result is
//! polished by maximizing the full likelihood jointly over `(E, β, γ)`. When
//! the profile is largest at `E = 0` the estimate is `0` (no endurance limit
//! is supported by the data). The likelihood-ratio interval is
//! `{E : 2[ℓ_p(Ê) − ℓ_p(E)] ≤ χ²₁(level)}` within `[0, min S]`: a limit
//! that reaches `0` or `min S` means the profile does not fall far enough
//! before the end of the admissible range.
//!
//! The coefficients' covariance, and every interval for quantiles and
//! failure probabilities of the endurance form, are **conditional on `Ê`**:
//! `E` is treated as known, so they do not include the uncertainty of the
//! endurance limit and are somewhat too narrow.
//!
//! # Likelihood
//! Failures contribute `w ln f(N)` (the density of `N`, including the
//! `1/N` Jacobian) and run-outs `w ln S(N)` (see [`crate::life_regression`]).

use stat_core::{ChiSquared, ContinuousDistribution, Result, StatError};

use crate::censored_fit::{IntervalEstimate, contribution, maximize};
use crate::life_data::{CensorKind, LifeObservation};
use crate::life_regression::{CovariatePoint, RegressionData, RegressionFamily, RegressionFit, fit_regression};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Upper end of the profile grid in `v = −ln(1 − E / min S)`
/// (`E = min S · (1 − 10⁻⁶)`).
const V_MAX: f64 = 13.815_510_557_964_274;
/// Number of grid intervals over `[0, V_MAX]`.
const GRID: usize = 40;

/// The curve relating log life to stress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SnForm {
    /// Basquin (power law): `ln N = β₀ + β₁ ln S`.
    Basquin,
    /// Semi-log (exponential): `ln N = β₀ + β₁ S`.
    SemiLog,
    /// Quadratic log–log: `ln N = β₀ + β₁ ln S + β₂ (ln S)²`.
    QuadraticLog,
    /// Endurance limit (Stromeyer): `ln N = β₀ + β₁ ln(S − E)`, with the
    /// endurance limit `0 ≤ E < min S` estimated by profile likelihood.
    EnduranceLimit,
}

impl SnForm {
    /// Every form, in menu order.
    pub const ALL: [Self; 4] = [Self::Basquin, Self::SemiLog, Self::QuadraticLog, Self::EnduranceLimit];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Basquin => "Basquin (log–log line)",
            Self::SemiLog => "Semi-log (log life, linear stress)",
            Self::QuadraticLog => "Quadratic log–log",
            Self::EnduranceLimit => "Endurance limit (Stromeyer)",
        }
    }

    /// Stable key for project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Basquin => "basquin",
            Self::SemiLog => "semi-log",
            Self::QuadraticLog => "quadratic-log",
            Self::EnduranceLimit => "endurance-limit",
        }
    }

    /// The form with this key (also `"semilog"`, `"quadratic"` and
    /// `"stromeyer"`).
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        let key = match key {
            "semilog" => "semi-log",
            "quadratic" => "quadratic-log",
            "stromeyer" => "endurance-limit",
            other => other,
        };
        Self::ALL.into_iter().find(|f| f.key() == key)
    }

    /// The curve as a formula.
    #[must_use]
    pub fn formula(self) -> &'static str {
        match self {
            Self::Basquin => "ln N = β₀ + β₁ ln S",
            Self::SemiLog => "ln N = β₀ + β₁ S",
            Self::QuadraticLog => "ln N = β₀ + β₁ ln S + β₂ (ln S)²",
            Self::EnduranceLimit => "ln N = β₀ + β₁ ln(S − E)",
        }
    }

    /// Distinct stress levels the form needs (2, or 3 for the quadratic and
    /// endurance-limit forms, which have a third curve parameter).
    #[must_use]
    pub fn min_levels(self) -> usize {
        match self {
            Self::Basquin | Self::SemiLog => 2,
            Self::QuadraticLog | Self::EnduranceLimit => 3,
        }
    }

    /// Names of the location coefficients.
    #[must_use]
    pub fn coefficient_names(self) -> Vec<&'static str> {
        match self {
            Self::Basquin => vec!["Intercept β₀", "β₁ (ln S)"],
            Self::SemiLog => vec!["Intercept β₀", "β₁ (S)"],
            Self::QuadraticLog => vec!["Intercept β₀", "β₁ (ln S)", "β₂ ((ln S)²)"],
            Self::EnduranceLimit => vec!["Intercept β₀", "β₁ (ln(S − E))"],
        }
    }

    /// Location covariates at stress `s` with endurance limit `e`.
    fn location(self, s: f64, e: f64) -> Vec<f64> {
        match self {
            Self::Basquin => vec![1.0, s.ln()],
            Self::SemiLog => vec![1.0, s],
            Self::QuadraticLog => vec![1.0, s.ln(), s.ln().powi(2)],
            Self::EnduranceLimit => vec![1.0, (s - e).ln()],
        }
    }
}

/// The fatigue model: curve, life distribution and scale form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FatigueModel {
    /// The S–N curve.
    pub form: SnForm,
    /// Life distribution: [`RegressionFamily::Lognormal`],
    /// [`RegressionFamily::Weibull`] or [`RegressionFamily::LogLogistic`]
    /// (the exponential is also accepted).
    pub family: RegressionFamily,
    /// `ln σ = γ₀ + γ₁ ln S` (otherwise `σ` is constant).
    pub stress_dependent_scale: bool,
}

impl FatigueModel {
    /// The covariate point at stress `s` for endurance limit `e` (ignored
    /// by the other forms).
    #[must_use]
    pub fn point_at(&self, s: f64, e: f64) -> CovariatePoint {
        CovariatePoint {
            location: self.form.location(s, e),
            scale: if self.stress_dependent_scale { vec![1.0, s.ln()] } else { vec![1.0] },
            offset: 0.0,
        }
    }
}

/// Cycles to failure (or run-out) with the stress of each specimen.
#[derive(Debug, Clone, PartialEq)]
pub struct FatigueData {
    /// Cycles: exact for failures, right-censored for run-outs (left- and
    /// interval-censored cycles are also accepted).
    pub observations: Vec<LifeObservation>,
    /// Stress amplitude of each observation (`> 0`).
    pub stresses: Vec<f64>,
}

/// The estimated endurance limit.
#[derive(Debug, Clone, PartialEq)]
pub struct EnduranceLimit {
    /// Maximum-likelihood estimate `Ê` (0 when the profile is largest there).
    pub estimate: f64,
    /// Confidence level of [`Self::likelihood_ratio`].
    pub level: f64,
    /// Profile-likelihood limits, within `[0, min S]`.
    pub likelihood_ratio: (f64, f64),
    /// `true` when the estimate is at the lower boundary `E = 0`.
    pub at_zero: bool,
    /// The profile `(E, ℓ_p(E))` on the search grid, in increasing `E`
    /// (`−∞` where the fit failed).
    pub profile: Vec<(f64, f64)>,
}

/// Specimens tested at one stress.
#[derive(Debug, Clone, PartialEq)]
pub struct FatigueLevel {
    /// The stress.
    pub stress: f64,
    /// Frequency of failures (observations that are not right-censored).
    pub failures: f64,
    /// Frequency of run-outs.
    pub run_outs: f64,
}

/// A fitted S–N curve.
#[derive(Debug, Clone)]
pub struct FatigueFit {
    /// The model.
    pub model: FatigueModel,
    /// The location–scale regression (at `Ê` for the endurance form).
    pub fit: RegressionFit,
    /// The data.
    pub data: FatigueData,
    /// The endurance limit (endurance form only).
    pub endurance_limit: Option<EnduranceLimit>,
    /// Distinct stresses in increasing order.
    pub levels: Vec<FatigueLevel>,
}

fn regression_data(data: &FatigueData, model: &FatigueModel, e: f64) -> RegressionData {
    RegressionData {
        observations: data.observations.clone(),
        points: data.stresses.iter().map(|&s| model.point_at(s, e)).collect(),
    }
}

fn e_of_v(v: f64, min_s: f64) -> f64 {
    if v <= 0.0 { 0.0 } else { -min_s * (-v).exp_m1() }
}

fn v_of_e(e: f64, min_s: f64) -> f64 {
    -(-e / min_s).ln_1p()
}

/// Profile log-likelihood at endurance limit `e`.
fn profile_at(data: &FatigueData, model: &FatigueModel, e: f64) -> f64 {
    fit_regression(&regression_data(data, model, e), model.family).map_or(f64::NEG_INFINITY, |f| f.log_likelihood)
}

/// Fit an S–N curve by maximum likelihood (see the module docs).
///
/// # Errors
/// [`StatError::InvalidArgument`] when the lengths differ, a stress is not
/// positive and finite, cycles are not positive, the family is not a life
/// (log-location-scale) family, there are fewer distinct stresses than the
/// form needs (2, or 3 for the quadratic and endurance-limit forms), every
/// specimen is a run-out, or the likelihood has no maximum.
pub fn fit_fatigue(data: &FatigueData, model: &FatigueModel) -> Result<FatigueFit> {
    if !model.family.is_log() {
        return Err(invalid("an S–N curve needs a life distribution (lognormal, Weibull or log-logistic)"));
    }
    if data.observations.is_empty() || data.observations.len() != data.stresses.len() {
        return Err(invalid("every observation needs its stress"));
    }
    if data.stresses.iter().any(|s| !(s.is_finite() && *s > 0.0)) {
        return Err(invalid("stresses must be positive"));
    }
    for o in &data.observations {
        let ok = match o.kind() {
            CensorKind::Exact | CensorKind::Right => o.lower > 0.0,
            CensorKind::Left => o.upper > 0.0,
            CensorKind::Interval => o.lower >= 0.0 && o.upper > 0.0,
        };
        if !ok {
            return Err(invalid("cycles must be positive"));
        }
    }
    if data.observations.iter().all(|o| o.kind() == CensorKind::Right) {
        return Err(invalid("every specimen is a run-out, so no S–N curve can be estimated"));
    }
    let mut levels: Vec<FatigueLevel> = Vec::new();
    for (o, &s) in data.observations.iter().zip(&data.stresses) {
        let level = match levels.iter_mut().find(|l| l.stress == s) {
            Some(l) => l,
            None => {
                levels.push(FatigueLevel { stress: s, failures: 0.0, run_outs: 0.0 });
                levels.last_mut().expect("just pushed")
            }
        };
        if o.kind() == CensorKind::Right {
            level.run_outs += o.weight;
        } else {
            level.failures += o.weight;
        }
    }
    levels.sort_by(|a, b| a.stress.total_cmp(&b.stress));
    if levels.len() < model.form.min_levels() {
        return Err(invalid(format!(
            "the {} curve needs at least {} different stresses",
            model.form.label(),
            model.form.min_levels()
        )));
    }
    if model.form != SnForm::EnduranceLimit {
        let fit = fit_regression(&regression_data(data, model, 0.0), model.family)?;
        return Ok(FatigueFit { model: *model, fit, data: data.clone(), endurance_limit: None, levels });
    }
    let min_s = levels[0].stress;
    // Profile on a grid of v.
    let grid: Vec<(f64, f64)> = (0..=GRID)
        .map(|j| {
            let v = V_MAX * j as f64 / GRID as f64;
            (v, profile_at(data, model, e_of_v(v, min_s)))
        })
        .collect();
    let (j_best, _) = grid
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.1.total_cmp(&b.1.1))
        .filter(|(_, g)| g.1.is_finite())
        .ok_or_else(|| invalid("the endurance-limit likelihood has no maximum for these data"))?;
    // Golden-section refinement between the neighbours of the best point.
    let (mut a, mut b) = (grid[j_best.saturating_sub(1)].0, grid[(j_best + 1).min(GRID)].0);
    let f = |v: f64| profile_at(data, model, e_of_v(v, min_s));
    let ratio = 0.5 * (5f64.sqrt() - 1.0);
    let (mut c, mut d) = (b - ratio * (b - a), a + ratio * (b - a));
    let (mut fc, mut fd) = (f(c), f(d));
    for _ in 0..80 {
        if b - a < 1e-10 {
            break;
        }
        if fc >= fd {
            (b, d, fd) = (d, c, fc);
            c = b - ratio * (b - a);
            fc = f(c);
        } else {
            (a, c, fc) = (c, d, fd);
            d = a + ratio * (b - a);
            fd = f(d);
        }
    }
    let mut v_hat = 0.5 * (a + b);
    let mut l_hat = f(v_hat);
    if grid[0].1 >= l_hat {
        (v_hat, l_hat) = (0.0, grid[0].1);
    }
    let at_zero = v_hat < 1e-8;
    if at_zero {
        v_hat = 0.0;
    } else if let Some((v, l)) = polish(data, model, min_s, v_hat).filter(|&(v, l)| l > l_hat && v > 0.0 && v < V_MAX) {
        (v_hat, l_hat) = (v, l);
    }
    let e_hat = e_of_v(v_hat, min_s);
    let fit = fit_regression(&regression_data(data, model, e_hat), model.family)?;
    let l_hat = l_hat.max(fit.log_likelihood);
    let level = 0.95;
    let likelihood_ratio = lr_interval(data, model, min_s, v_hat, l_hat, &grid, level);
    let profile = grid.iter().map(|&(v, l)| (e_of_v(v, min_s), l)).collect();
    Ok(FatigueFit {
        model: *model,
        fit,
        data: data.clone(),
        endurance_limit: Some(EnduranceLimit { estimate: e_hat, level, likelihood_ratio, at_zero, profile }),
        levels,
    })
}

/// Maximize the full likelihood jointly over `(v, β, γ)` from the profile
/// maximum at `v0`.
fn polish(data: &FatigueData, model: &FatigueModel, min_s: f64, v0: f64) -> Option<(f64, f64)> {
    let start = fit_regression(&regression_data(data, model, e_of_v(v0, min_s)), model.family).ok()?;
    let p = start.location_coefficients.len();
    let mut x0 = vec![v0];
    x0.extend(&start.location_coefficients);
    x0.extend(&start.scale_coefficients);
    let fixed = model.family.fixed_scale();
    let ll = |th: &[f64]| -> f64 {
        let e = e_of_v(th[0], min_s);
        let mut total = 0.0;
        for (o, &s) in data.observations.iter().zip(&data.stresses) {
            let pt = model.point_at(s, e);
            let mu: f64 = pt.location.iter().zip(&th[1..=p]).map(|(a, b)| a * b).sum();
            let sigma =
                if fixed { 1.0 } else { pt.scale.iter().zip(&th[p + 1..]).map(|(a, b)| a * b).sum::<f64>().exp() };
            if !(mu.is_finite() && sigma > 0.0 && sigma.is_finite()) {
                return f64::NEG_INFINITY;
            }
            let c = contribution(&model.family.distribution(mu, sigma), o);
            if !c.is_finite() {
                return f64::NEG_INFINITY;
            }
            total += o.weight * c;
        }
        if total.is_nan() { f64::NEG_INFINITY } else { total }
    };
    let sigma0 = start.sigma(&model.point_at(min_s, e_of_v(v0, min_s)));
    let steps = |th: &[f64]| -> Vec<f64> {
        (0..th.len())
            .map(|i| {
                if i == 0 {
                    1e-5
                } else if i <= p {
                    1e-5 * sigma0 * (1.0 + th[i].abs()).min(1e3)
                } else {
                    1e-5
                }
            })
            .collect()
    };
    let (x, value) = maximize(&ll, &x0, &steps)?;
    Some((x[0], value))
}

/// Profile-likelihood limits for `E` from the grid and bisection in `v`.
fn lr_interval(
    data: &FatigueData,
    model: &FatigueModel,
    min_s: f64,
    v_hat: f64,
    l_hat: f64,
    grid: &[(f64, f64)],
    level: f64,
) -> (f64, f64) {
    let crit = ChiSquared::new(1.0).and_then(|c| c.quantile(level)).unwrap_or(f64::NAN);
    let g = |l: f64| if l.is_finite() { 2.0 * (l_hat - l) - crit } else { f64::INFINITY };
    let h = |v: f64| g(profile_at(data, model, e_of_v(v, min_s)));
    let bisect = |mut inside: f64, mut outside: f64| -> f64 {
        for _ in 0..100 {
            if (outside - inside).abs() < 1e-11 * (1.0 + inside.abs()) {
                break;
            }
            let mid = 0.5 * (inside + outside);
            if h(mid) > 0.0 {
                outside = mid;
            } else {
                inside = mid;
            }
        }
        0.5 * (inside + outside)
    };
    let lower = {
        let mut inside = v_hat;
        let mut found = None;
        for &(v, l) in grid.iter().rev().filter(|(v, _)| *v < v_hat) {
            if g(l) > 0.0 {
                found = Some(v);
                break;
            }
            inside = v;
        }
        found.map_or(0.0, |out| e_of_v(bisect(inside, out), min_s))
    };
    let upper = {
        let mut inside = v_hat;
        let mut found = None;
        for &(v, l) in grid.iter().filter(|(v, _)| *v > v_hat) {
            if g(l) > 0.0 {
                found = Some(v);
                break;
            }
            inside = v;
        }
        found.map_or(min_s, |out| e_of_v(bisect(inside, out), min_s))
    };
    (lower, upper)
}

impl FatigueFit {
    /// The endurance limit used by the curve (`Ê`, or 0 for the other
    /// forms).
    #[must_use]
    pub fn endurance(&self) -> f64 {
        self.endurance_limit.as_ref().map_or(0.0, |e| e.estimate)
    }

    /// `true` when stress `s` gives a finite life under the curve (always,
    /// except at or below the endurance limit).
    #[must_use]
    pub fn finite_life(&self, s: f64) -> bool {
        s > 0.0 && (self.model.form != SnForm::EnduranceLimit || s > self.endurance())
    }

    /// The covariate point at stress `s` (at `Ê` for the endurance form;
    /// its location covariate is NaN at or below the endurance limit).
    #[must_use]
    pub fn point(&self, s: f64) -> CovariatePoint {
        self.model.point_at(s, self.endurance())
    }

    /// Location `μ(S)` of log life (`+∞` at or below the endurance limit).
    #[must_use]
    pub fn location(&self, s: f64) -> f64 {
        if self.finite_life(s) { self.fit.location(&self.point(s)) } else { f64::INFINITY }
    }

    /// Scale `σ(S)` of log life (the Weibull shape is `1/σ`).
    #[must_use]
    pub fn sigma(&self, s: f64) -> f64 {
        self.fit.sigma(&self.model.point_at(s, 0.0))
    }

    /// Number of estimated parameters (including `E` for the endurance form).
    #[must_use]
    pub fn n_parameters(&self) -> usize {
        self.fit.n_parameters() + usize::from(self.endurance_limit.is_some())
    }

    /// Maximized log-likelihood.
    #[must_use]
    pub fn log_likelihood(&self) -> f64 {
        self.fit.log_likelihood
    }

    /// Akaike's criterion `−2ℓ + 2k` (counting `E` for the endurance form).
    #[must_use]
    pub fn aic(&self) -> f64 {
        -2.0 * self.fit.log_likelihood + 2.0 * self.n_parameters() as f64
    }

    /// The `p` quantile of cycles to failure at stress `s`,
    /// `N_p = exp(μ(S) + w_p σ(S))`, with Wald limits (on log cycles) and,
    /// when `likelihood_ratio`, profile-likelihood limits. For the
    /// endurance form these are conditional on `Ê`; at or below the
    /// endurance limit the estimate is `+∞` and the limits NaN.
    #[must_use]
    pub fn quantile(&self, s: f64, p: f64, level: f64, likelihood_ratio: bool) -> IntervalEstimate {
        if !self.finite_life(s) {
            let nan = (f64::NAN, f64::NAN);
            return IntervalEstimate {
                name: "Quantile",
                estimate: f64::INFINITY,
                std_error: f64::NAN,
                wald: nan,
                likelihood_ratio: likelihood_ratio.then_some(nan),
            };
        }
        self.fit.quantile(&self.point(s), p, level, likelihood_ratio)
    }

    /// The probability that a specimen at stress `s` fails by `n` cycles,
    /// with Wald limits (logit scale) and, when `likelihood_ratio`, profile
    /// limits (conditional on `Ê` for the endurance form; 0 at or below the
    /// endurance limit).
    #[must_use]
    pub fn failure_probability(&self, s: f64, n: f64, level: f64, likelihood_ratio: bool) -> IntervalEstimate {
        if !self.finite_life(s) {
            return IntervalEstimate {
                name: "Failure probability",
                estimate: 0.0,
                std_error: f64::NAN,
                wald: (f64::NAN, f64::NAN),
                likelihood_ratio: likelihood_ratio.then_some((f64::NAN, f64::NAN)),
            };
        }
        self.fit.failure_probability(&self.point(s), n, level, likelihood_ratio)
    }

    /// The stress whose `p` quantile of life is `n` cycles (the fatigue
    /// strength at `n` cycles), found by scanning `ln(S − E₀)` (with
    /// `E₀ = Ê` for the endurance form, else 0) from `2 · max S` down to
    /// `min S / 2` (or to just above `Ê`) for the first sign change of
    /// `μ(S) + w_p σ(S) − ln n`, then bisecting; with a non-monotone curve
    /// this is the highest such stress. `None` when the life is not reached
    /// in that range, or for `n ≤ 0` or `p` outside `(0, 1)`.
    #[must_use]
    pub fn stress_at_life(&self, n: f64, p: f64) -> Option<f64> {
        if !(n > 0.0 && n.is_finite() && p > 0.0 && p < 1.0) {
            return None;
        }
        let w = self.model.family.standard_quantile(p);
        let target = n.ln();
        let e0 = self.endurance();
        let min_s = self.levels.first()?.stress;
        let max_s = self.levels.last()?.stress;
        let lo = if self.model.form == SnForm::EnduranceLimit { 1e-6 * (min_s - e0) } else { min_s / 2.0 };
        let hi = 2.0 * max_s - e0;
        let h = |u: f64| {
            let s = e0 + u.exp();
            let pt = self.point(s);
            self.fit.location(&pt) + w * self.fit.sigma(&pt) - target
        };
        let (ul, uh) = (lo.ln(), hi.ln());
        let steps = 400;
        let mut prev = (uh, h(uh));
        for i in (0..steps).rev() {
            let u = ul + (uh - ul) * i as f64 / steps as f64;
            let v = h(u);
            if v == 0.0 {
                return Some(e0 + u.exp());
            }
            if v.is_finite() && prev.1.is_finite() && (v > 0.0) != (prev.1 > 0.0) {
                let (mut a, mut fa, mut b) = (u, v, prev.0);
                for _ in 0..200 {
                    let m = 0.5 * (a + b);
                    if (b - a).abs() < 1e-15 * (1.0 + m.abs()) {
                        break;
                    }
                    let fm = h(m);
                    if (fm > 0.0) == (fa > 0.0) {
                        (a, fa) = (m, fm);
                    } else {
                        b = m;
                    }
                }
                return Some(e0 + (0.5 * (a + b)).exp());
            }
            prev = (u, v);
        }
        None
    }

    /// Standardized residuals `z = (ln N − μ(S))/σ(S)` in data order, with a
    /// flag that is `true` for run-outs (right-censored, whose `z` is a lower
    /// bound). `N` is the observation's lower bound (the cycles reached for a
    /// failure or run-out), or its upper bound when left-censored.
    #[must_use]
    pub fn standardized_residuals(&self) -> Vec<(f64, bool)> {
        self.data
            .observations
            .iter()
            .zip(&self.data.stresses)
            .map(|(o, &s)| {
                let kind = o.kind();
                let n = if kind == CensorKind::Left || o.lower <= 0.0 { o.upper } else { o.lower };
                let pt = self.point(s);
                ((n.ln() - self.fit.location(&pt)) / self.fit.sigma(&pt), kind == CensorKind::Right)
            })
            .collect()
    }

    /// Basquin's coefficients `(A, b)` of `S = A · N^b` (Basquin form only),
    /// from inverting `ln N = β₀ + β₁ ln S`: `A = exp(−β₀/β₁)`, `b = 1/β₁`.
    /// Here `N = exp(μ)`, the median life for the lognormal and
    /// log-logistic families and the 63.2 % quantile (characteristic life)
    /// for the Weibull. `None` for the other forms or `β₁ = 0`.
    #[must_use]
    pub fn basquin_coefficients(&self) -> Option<(f64, f64)> {
        if self.model.form != SnForm::Basquin {
            return None;
        }
        let (b0, b1) = (self.fit.location_coefficients[0], self.fit.location_coefficients[1]);
        (b1 != 0.0).then(|| ((-b0 / b1).exp(), 1.0 / b1))
    }

    /// Profile-likelihood limits for the endurance limit at another
    /// confidence `level` (endurance form only).
    #[must_use]
    pub fn endurance_limit_interval(&self, level: f64) -> Option<(f64, f64)> {
        let el = self.endurance_limit.as_ref()?;
        let min_s = self.levels.first()?.stress;
        let grid: Vec<(f64, f64)> = el.profile.iter().map(|&(e, l)| (v_of_e(e, min_s), l)).collect();
        let v_hat = v_of_e(el.estimate, min_s);
        Some(lr_interval(&self.data, &self.model, min_s, v_hat, self.fit.log_likelihood, &grid, level))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip() {
        for f in SnForm::ALL {
            assert_eq!(SnForm::from_key(f.key()), Some(f));
            assert_eq!(f.coefficient_names().len(), f.location(300.0, 100.0).len());
        }
        assert_eq!(SnForm::from_key("stromeyer"), Some(SnForm::EnduranceLimit));
        assert!((e_of_v(v_of_e(150.0, 200.0), 200.0) - 150.0).abs() < 1e-9);
    }

    fn sample() -> FatigueData {
        let mut observations = Vec::new();
        let mut stresses = Vec::new();
        for (s, lives) in
            [(200.0, [9.0e6, 7.0e6, 6.0e6]), (300.0, [3.0e5, 4.0e5, 2.2e5]), (400.0, [5.0e4, 9.0e4, 7.0e4])]
        {
            for n in lives {
                observations.push(LifeObservation::exact(n, 1.0));
                stresses.push(s);
            }
        }
        observations.push(LifeObservation::right(1e7, 1.0));
        stresses.push(200.0);
        FatigueData { observations, stresses }
    }

    #[test]
    fn invalid_inputs_are_rejected() {
        let model =
            FatigueModel { form: SnForm::Basquin, family: RegressionFamily::Lognormal, stress_dependent_scale: false };
        let mut bad = sample();
        bad.stresses[0] = 0.0;
        assert!(fit_fatigue(&bad, &model).is_err());
        let runouts = FatigueData {
            observations: vec![LifeObservation::right(1e7, 1.0); 4],
            stresses: vec![100.0, 100.0, 200.0, 200.0],
        };
        assert!(fit_fatigue(&runouts, &model).is_err());
        let normal = FatigueModel { family: RegressionFamily::Normal, ..model };
        assert!(fit_fatigue(&sample(), &normal).is_err());
        let two = FatigueData {
            observations: vec![LifeObservation::exact(1e5, 1.0); 4],
            stresses: vec![100.0, 100.0, 200.0, 200.0],
        };
        assert!(fit_fatigue(&two, &FatigueModel { form: SnForm::QuadraticLog, ..model }).is_err());
    }

    /// With complete data, a lognormal Basquin fit is least squares of
    /// `ln N` on `ln S`; stress at life inverts the median curve, and
    /// Basquin's coefficients reproduce it.
    #[test]
    fn complete_basquin_is_least_squares() {
        let mut data = sample();
        data.observations.pop();
        data.stresses.pop();
        let model =
            FatigueModel { form: SnForm::Basquin, family: RegressionFamily::Lognormal, stress_dependent_scale: false };
        let fit = fit_fatigue(&data, &model).unwrap();
        let x: Vec<f64> = data.stresses.iter().map(|s| s.ln()).collect();
        let y: Vec<f64> = data.observations.iter().map(|o| o.lower.ln()).collect();
        let n = x.len() as f64;
        let (mx, my) = (x.iter().sum::<f64>() / n, y.iter().sum::<f64>() / n);
        let sxx: f64 = x.iter().map(|v| (v - mx).powi(2)).sum();
        let b1 = x.iter().zip(&y).map(|(a, b)| (a - mx) * (b - my)).sum::<f64>() / sxx;
        let b0 = my - b1 * mx;
        assert!((fit.fit.location_coefficients[1] - b1).abs() < 1e-6);
        assert!((fit.fit.location_coefficients[0] - b0).abs() < 1e-5);
        let s = fit.stress_at_life(1e6, 0.5).unwrap();
        assert!((fit.location(s) - 1e6f64.ln()).abs() < 1e-9);
        let (a, b) = fit.basquin_coefficients().unwrap();
        assert!((a * 1e6f64.powf(b) - s).abs() < 1e-8 * s);
        assert_eq!(fit.levels.len(), 3);
        assert!(fit.standardized_residuals().iter().all(|(z, r)| z.is_finite() && !r));
    }
}
