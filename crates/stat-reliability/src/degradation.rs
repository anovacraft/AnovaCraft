//! Degradation analysis: life information from measurements of a
//! performance characteristic that drifts towards a failure threshold
//! (Meeker & Escobar, *Statistical Methods for Reliability Data*, 1998,
//! ch. 13; Lu & Meeker 1993, "Using degradation measures to estimate a
//! time-to-failure distribution", *Technometrics* 35, 161–174).
//!
//! # Repeated measures: paths per unit
//! Each unit `i` is measured at times `tᵢⱼ`; its path is one of
//!
//! * **linear** on transformed scales, `g(y) = a + b·h(t)`, with `g` and `h`
//!   each the identity, natural log or square root (so exponential growth is
//!   `g = ln`, a power law `g = h = ln`);
//! * **asymptotic**, `D(t) = D∞(1 − e^{−kt})` (a first-order chemical
//!   reaction approaching `D∞`; Meeker & Escobar §13.2);
//! * **power**, `D(t) = a·tᵇ`,
//!
//! fitted to each unit by least squares (the nonlinear forms exactly, by
//! a one-dimensional search over `k` or `b` with the linear coefficient
//! solved in closed form). A unit fails when its path reaches the threshold
//! `D_f` (rising to it, or falling to it). The **approximate method**
//! (Meeker & Escobar §13.6–13.7) solves each fitted path for its crossing
//! time, the unit's *pseudo failure time*; a path that never reaches the
//! threshold, or is moving away from it, gives a unit right-censored at its
//! last measurement, and one already past the threshold at time zero a unit
//! left-censored at its first. A life distribution (or a life–stress model,
//! with a stress per unit) is then fitted to the pseudo failure times by
//! [`crate::censored_fit`] or [`crate::life_stress`].
//!
//! # Repeated measures: random-effects path model
//! With the linear path, the units' intercepts and slopes may instead be
//! treated as random: `g(yᵢⱼ) = (β₀ + b₀ᵢ) + (β₁ + b₁ᵢ) h(tᵢⱼ) + εᵢⱼ` with
//! `(b₀ᵢ, b₁ᵢ) ~ N(0, Σ)` and `εᵢⱼ ~ N(0, σ²)`, fitted by REML (or ML) with
//! `stat_mixed`. The fraction of the population whose actual path
//! `D(t) = β₀ + b₀ + (β₁ + b₁) h(t)` is beyond the transformed threshold
//! `g(D_f)` at `t` is
//!
//! ```text
//! F(t) = Φ( ±(β₀ + β₁h(t) − g(D_f)) / √(Σ₀₀ + 2Σ₀₁h(t) + Σ₁₁h(t)²) )
//! ```
//!
//! (`+` for a rising failure direction), computed exactly (Meeker & Escobar
//! §13.5; Lu & Meeker 1993). Pointwise intervals for `F(t)` and its
//! quantiles come from a parametric bootstrap with a fixed seed: new
//! data are simulated from the fitted model at the observed design, the model
//! refitted, and the percentiles of the bootstrap values taken (Meeker &
//! Escobar §13.7).
//!
//! # Destructive degradation
//! When measuring a unit destroys it, each unit gives one value. The
//! transformed response is modelled as a location–scale regression
//! (normal, logistic or extreme value) with constant `σ` and location
//! `μ(t) = β₀ + β₁ h(t)`, or with a stress `s` acting on the degradation rate,
//! `μ(t, s) = β₀ + β₁ exp(β₂ [x(s) − x(s_U)]) h(t)`, `x(s)` the transformed
//! stress (Arrhenius `1/kT`, inverse power `ln s`, …) and `s_U` the use
//! condition, so that `β₁` is the rate at use conditions and, for Arrhenius,
//! `−β₂` the activation energy of the rate in eV (Escobar, Meeker, Kugler &
//! Kramer 2003, "Accelerated destructive degradation tests: data, models
//! and analysis", in *Mathematical and Statistical Methods in Reliability*,
//! World Scientific; Meeker & Escobar §13.9). The parameters are estimated
//! by maximum likelihood (damped Newton from a grid over `β₂` with `β₀, β₁`
//! by least squares), the covariance from the observed information. The
//! time-to-threshold distribution at stress `s` is
//! `F(t) = Φ((g(D_f) − μ(t, s))/σ)` for a falling response (`1 − Φ(·)`
//! rising), and its `p` quantile solves `μ(t_p, s) = g(D_f) − σ w`,
//! `w = Φ⁻¹(p)` falling or `Φ⁻¹(1 − p)` rising. Intervals are Wald, on log
//! time for quantiles and the logit for probabilities, from the delta method.

use stat_core::rng::Rng;
use stat_core::{ContinuousDistribution, Matrix, Normal, Result, StatError};
use stat_mixed::{MixedFamily, MixedFit, MixedModel, MixedOptions, RandomTerm, fit_mixed};

use crate::censored_fit::{IntervalEstimate, derivatives, inverse_spd, maximize};
use crate::life_data::LifeObservation;
use crate::life_regression::RegressionFamily;
use crate::life_stress::StressVariable;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// A monotone transform of a response or time scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ScaleTransform {
    /// No transform.
    #[default]
    Identity,
    /// Natural logarithm.
    Log,
    /// Square root.
    Sqrt,
}

impl ScaleTransform {
    /// Every transform.
    pub const ALL: [Self; 3] = [Self::Identity, Self::Log, Self::Sqrt];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Identity => "None",
            Self::Log => "Log",
            Self::Sqrt => "Square root",
        }
    }

    /// Stable key.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Identity => "none",
            Self::Log => "log",
            Self::Sqrt => "sqrt",
        }
    }

    /// The transform with this key.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.key() == key)
    }

    /// The transformed value (`None` outside the domain).
    #[must_use]
    pub fn apply(self, v: f64) -> Option<f64> {
        let out = match self {
            Self::Identity => v,
            Self::Log if v > 0.0 => v.ln(),
            Self::Sqrt if v >= 0.0 => v.sqrt(),
            _ => return None,
        };
        out.is_finite().then_some(out)
    }

    /// The inverse (`None` outside the range).
    #[must_use]
    pub fn invert(self, u: f64) -> Option<f64> {
        let out = match self {
            Self::Identity => u,
            Self::Log => u.exp(),
            Self::Sqrt if u >= 0.0 => u * u,
            Self::Sqrt => return None,
        };
        out.is_finite().then_some(out)
    }

    /// `"ln(name)"`, `"√name"` or `name`.
    #[must_use]
    pub fn describe(self, name: &str) -> String {
        match self {
            Self::Identity => name.to_owned(),
            Self::Log => format!("ln({name})"),
            Self::Sqrt => format!("√({name})"),
        }
    }
}

/// The form of each unit's degradation path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PathModel {
    /// `g(y) = a + b h(t)` on transformed scales.
    Linear {
        /// Response transform `g`.
        response: ScaleTransform,
        /// Time transform `h`.
        time: ScaleTransform,
    },
    /// `D(t) = D∞(1 − e^{−kt})`.
    Asymptotic,
    /// `D(t) = a tᵇ`.
    Power,
}

impl PathModel {
    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Linear { .. } => "Linear",
            Self::Asymptotic => "Asymptotic D∞(1 − e^(−kt))",
            Self::Power => "Power a·t^b",
        }
    }

    /// Parameter names.
    #[must_use]
    pub fn parameter_names(self) -> [&'static str; 2] {
        match self {
            Self::Linear { .. } => ["Intercept a", "Slope b"],
            Self::Asymptotic => ["Limit D∞", "Rate k"],
            Self::Power => ["Coefficient a", "Exponent b"],
        }
    }

    /// The path's value at `t` with parameters `p`, on the response scale.
    #[must_use]
    pub fn value(self, p: [f64; 2], t: f64) -> f64 {
        match self {
            Self::Linear { response, time } => {
                time.apply(t).and_then(|h| response.invert(p[0] + p[1] * h)).unwrap_or(f64::NAN)
            }
            Self::Asymptotic => p[0] * -(-p[1] * t).exp_m1(),
            Self::Power => {
                if t > 0.0 {
                    p[0] * t.powf(p[1])
                } else if t == 0.0 {
                    0.0
                } else {
                    f64::NAN
                }
            }
        }
    }
}

/// Which way the response moves towards failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FailureDirection {
    /// The unit fails when the response rises to the threshold.
    #[default]
    Rising,
    /// The unit fails when the response falls to the threshold.
    Falling,
}

impl FailureDirection {
    fn sign(self) -> f64 {
        match self {
            Self::Rising => 1.0,
            Self::Falling => -1.0,
        }
    }
}

/// How a unit's pseudo failure time is known.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Crossing {
    /// The fitted path reaches the threshold at this time.
    At(f64),
    /// The path does not reach the threshold (or moves away from it):
    /// right-censored at the last measurement.
    Never(f64),
    /// The path is past the threshold at time zero: left-censored at the
    /// first measurement.
    Before(f64),
}

impl Crossing {
    /// The censored observation for a life distribution fit.
    #[must_use]
    pub fn observation(self) -> LifeObservation {
        match self {
            Self::At(t) => LifeObservation::exact(t, 1.0),
            Self::Never(t) => LifeObservation::right(t, 1.0),
            Self::Before(t) => LifeObservation::left(t, 1.0),
        }
    }

    /// The time involved.
    #[must_use]
    pub fn time(self) -> f64 {
        match self {
            Self::At(t) | Self::Never(t) | Self::Before(t) => t,
        }
    }
}

/// One unit's least-squares path.
#[derive(Debug, Clone, PartialEq)]
pub struct UnitPath {
    /// Path parameters (see [`PathModel::parameter_names`]).
    pub parameters: [f64; 2],
    /// Residual standard deviation (on the fitted scale).
    pub residual_sd: f64,
    /// Measurements used.
    pub n: usize,
    /// The crossing of the threshold.
    pub crossing: Crossing,
}

/// Minimize a unimodal-ish function of one variable on `[a, b]` by a grid
/// followed by golden-section refinement.
fn minimize_1d(f: &dyn Fn(f64) -> f64, a: f64, b: f64) -> f64 {
    let n = 80;
    let grid: Vec<f64> = (0..=n).map(|i| a + (b - a) * i as f64 / n as f64).collect();
    let vals: Vec<f64> = grid.iter().map(|&x| f(x)).collect();
    let i = (0..=n).min_by(|&i, &j| vals[i].total_cmp(&vals[j])).unwrap_or(0);
    let (mut lo, mut hi) = (grid[i.saturating_sub(1)], grid[(i + 1).min(n)]);
    let r = (5f64.sqrt() - 1.0) / 2.0;
    let mut c = hi - r * (hi - lo);
    let mut d = lo + r * (hi - lo);
    let (mut fc, mut fd) = (f(c), f(d));
    for _ in 0..200 {
        if (hi - lo).abs() < 1e-13 * (1.0 + lo.abs()) {
            break;
        }
        if fc <= fd {
            hi = d;
            d = c;
            fd = fc;
            c = hi - r * (hi - lo);
            fc = f(c);
        } else {
            lo = c;
            c = d;
            fc = fd;
            d = lo + r * (hi - lo);
            fd = f(d);
        }
    }
    0.5 * (lo + hi)
}

/// Fit one unit's path and find its pseudo failure time.
///
/// # Errors
/// [`StatError::InvalidArgument`] for mismatched or non-finite inputs, fewer
/// than three usable measurements (two for no transform of a linear path),
/// values outside a transform's domain, a threshold outside the response
/// transform's domain, or measurements all at one time.
pub fn fit_unit_path(
    times: &[f64],
    values: &[f64],
    model: PathModel,
    threshold: f64,
    direction: FailureDirection,
) -> Result<UnitPath> {
    if times.len() != values.len() {
        return Err(invalid("each measurement needs its time"));
    }
    if times.iter().chain(values).any(|v| !v.is_finite()) || times.iter().any(|t| *t < 0.0) {
        return Err(invalid("measurement times must be non-negative and values finite"));
    }
    let n = times.len();
    if n < 3 {
        return Err(StatError::InsufficientData { required: 3, found: n });
    }
    let t_first = times.iter().copied().fold(f64::INFINITY, f64::min);
    let t_last = times.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if t_last <= t_first {
        return Err(invalid("a unit's measurements are all at one time"));
    }
    let sign = direction.sign();
    let (parameters, sse, crossing) = match model {
        PathModel::Linear { response, time } => {
            let g = response.apply(threshold).ok_or_else(|| {
                invalid(format!("the threshold {threshold} is outside the response transform's domain"))
            })?;
            let mut xs = Vec::with_capacity(n);
            let mut ys = Vec::with_capacity(n);
            for (&t, &y) in times.iter().zip(values) {
                let (Some(h), Some(v)) = (time.apply(t), response.apply(y)) else {
                    return Err(invalid(format!(
                        "a measurement ({t}, {y}) is outside the time or response transform's domain"
                    )));
                };
                xs.push(h);
                ys.push(v);
            }
            let mx = xs.iter().sum::<f64>() / n as f64;
            let my = ys.iter().sum::<f64>() / n as f64;
            let sxx: f64 = xs.iter().map(|x| (x - mx).powi(2)).sum();
            if sxx.is_nan() || sxx <= 0.0 {
                return Err(invalid("a unit's transformed times are all equal"));
            }
            let sxy: f64 = xs.iter().zip(&ys).map(|(x, y)| (x - mx) * (y - my)).sum();
            let b = sxy / sxx;
            let a = my - b * mx;
            let sse: f64 = xs.iter().zip(&ys).map(|(x, y)| (y - a - b * x).powi(2)).sum();
            let crossing = if b * sign <= 0.0 {
                Crossing::Never(t_last)
            } else {
                let h = (g - a) / b;
                match time.invert(h) {
                    Some(t) if t > 0.0 => Crossing::At(t),
                    _ => Crossing::Before(t_first.max(f64::MIN_POSITIVE)),
                }
            };
            ([a, b], sse, crossing)
        }
        PathModel::Asymptotic | PathModel::Power => {
            let basis = |s: f64, t: f64| match model {
                PathModel::Asymptotic => -(-s * t).exp_m1(),
                _ => {
                    if t > 0.0 {
                        t.powf(s)
                    } else {
                        0.0
                    }
                }
            };
            // For a nonlinear parameter s, the linear coefficient in closed form.
            let solve = |s: f64| -> (f64, f64) {
                let (mut gg, mut gy) = (0.0, 0.0);
                for (&t, &y) in times.iter().zip(values) {
                    let g = basis(s, t);
                    gg += g * g;
                    gy += g * y;
                }
                let c = if gg > 0.0 { gy / gg } else { 0.0 };
                let sse: f64 = times.iter().zip(values).map(|(&t, &y)| (y - c * basis(s, t)).powi(2)).sum();
                (c, sse)
            };
            let positive: Vec<f64> = times.iter().copied().filter(|t| *t > 0.0).collect();
            if positive.len() < 2 {
                return Err(invalid("a nonlinear path needs at least two measurements after time zero"));
            }
            let t_min = positive.iter().copied().fold(f64::INFINITY, f64::min);
            let (lo, hi) = match model {
                PathModel::Asymptotic => ((1e-3 / t_last).ln(), (50.0 / t_min).ln()),
                _ => (0.02f64.ln(), 20f64.ln()),
            };
            let ls = minimize_1d(&|u: f64| solve(u.exp()).1, lo, hi);
            let s = ls.exp();
            let (c, sse) = solve(s);
            let slope_sign = c * s;
            let crossing = if slope_sign * sign <= 0.0 {
                Crossing::Never(t_last)
            } else {
                let r = threshold / c;
                let t = match model {
                    PathModel::Asymptotic => {
                        if r > 0.0 && r < 1.0 {
                            Some(-(-r).ln_1p() / s)
                        } else {
                            None
                        }
                    }
                    _ => (r > 0.0).then(|| r.powf(1.0 / s)),
                };
                match t {
                    Some(t) if t.is_finite() && t > 0.0 => Crossing::At(t),
                    _ if threshold * sign <= 0.0 => Crossing::Before(t_first.max(f64::MIN_POSITIVE)),
                    _ => Crossing::Never(t_last),
                }
            };
            ([c, s], sse, crossing)
        }
    };
    let residual_sd = if n > 2 { (sse / (n - 2) as f64).sqrt() } else { f64::NAN };
    Ok(UnitPath { parameters, residual_sd, n, crossing })
}

// ---------------------------------------------------------------------------
// Random-effects linear paths
// ---------------------------------------------------------------------------

/// One unit's measurements.
#[derive(Debug, Clone, PartialEq)]
pub struct UnitSeries {
    /// Measurement times.
    pub times: Vec<f64>,
    /// Measured values.
    pub values: Vec<f64>,
}

/// The random-effects linear path model and its implied life distribution.
#[derive(Debug, Clone)]
pub struct RandomPathFit {
    /// Fixed intercept and slope `(β₀, β₁)`.
    pub beta: [f64; 2],
    /// Random-effects covariance `[Σ₀₀, Σ₀₁, Σ₁₁]`.
    pub sigma: [f64; 3],
    /// Residual standard deviation.
    pub residual_sd: f64,
    /// The mixed-model fit.
    pub mixed: MixedFit,
    /// Response transform.
    pub response: ScaleTransform,
    /// Time transform.
    pub time: ScaleTransform,
    /// Threshold on the transformed response scale.
    pub threshold: f64,
    /// Failure direction.
    pub direction: FailureDirection,
    /// REML (else ML).
    pub reml: bool,
    design: Vec<(usize, f64)>,
}

/// Fit `g(y) = (β₀ + b₀ᵢ) + (β₁ + b₁ᵢ) h(t) + ε` with correlated random
/// intercepts and slopes per unit (see the module docs).
///
/// # Errors
/// [`StatError::InvalidArgument`] for fewer than three units, values outside a
/// transform's domain or a threshold outside it; errors of `fit_mixed`.
pub fn fit_random_paths(
    units: &[UnitSeries],
    response: ScaleTransform,
    time: ScaleTransform,
    threshold: f64,
    direction: FailureDirection,
    reml: bool,
) -> Result<RandomPathFit> {
    if units.len() < 3 {
        return Err(invalid("a random-effects path model needs at least three units"));
    }
    let threshold_g = response
        .apply(threshold)
        .ok_or_else(|| invalid(format!("the threshold {threshold} is outside the response transform's domain")))?;
    let mut y = Vec::new();
    let mut design = Vec::new();
    for (u, s) in units.iter().enumerate() {
        if s.times.len() != s.values.len() {
            return Err(invalid("each measurement needs its time"));
        }
        for (&t, &v) in s.times.iter().zip(&s.values) {
            let (Some(h), Some(g)) = (time.apply(t), response.apply(v)) else {
                return Err(invalid(format!(
                    "a measurement ({t}, {v}) is outside the time or response transform's domain"
                )));
            };
            y.push(g);
            design.push((u, h));
        }
    }
    let probe = RandomPathFit::refit(&y, &design, units.len(), reml)?;
    let (beta, sigma, residual_sd) = RandomPathFit::summarize(&probe)?;
    Ok(RandomPathFit {
        beta,
        sigma,
        residual_sd,
        mixed: probe,
        response,
        time,
        threshold: threshold_g,
        direction,
        reml,
        design,
    })
}

/// Pointwise bootstrap limits.
#[derive(Debug, Clone, PartialEq)]
pub struct BootstrapBands {
    /// Times of the band.
    pub times: Vec<f64>,
    /// Lower limits of `F(t)`.
    pub lower: Vec<f64>,
    /// Upper limits of `F(t)`.
    pub upper: Vec<f64>,
    /// Requested probabilities and the `(lower, upper)` limits of their quantiles.
    pub quantiles: Vec<(f64, f64, f64)>,
    /// Replicates that refitted successfully.
    pub replicates: usize,
}

impl RandomPathFit {
    fn refit(y: &[f64], design: &[(usize, f64)], n_units: usize, reml: bool) -> Result<MixedFit> {
        let n = y.len();
        let x = Matrix::from_columns(&[vec![1.0; n], design.iter().map(|d| d.1).collect::<Vec<f64>>()])?;
        let z = x.clone();
        let model = MixedModel {
            y: y.to_vec(),
            x,
            x_names: vec!["Intercept".to_owned(), "Time".to_owned()],
            weights: Vec::new(),
            offset: Vec::new(),
            random: vec![RandomTerm {
                group: "Unit".to_owned(),
                levels: (0..n_units).map(|i| i.to_string()).collect(),
                codes: design.iter().map(|d| d.0).collect(),
                effects: vec!["Intercept".to_owned(), "Time".to_owned()],
                z,
                correlated: true,
            }],
            family: MixedFamily::Normal,
            residual_groups: None,
            categories: Vec::new(),
        };
        fit_mixed(&model, &MixedOptions { reml, ..MixedOptions::default() })
    }

    fn summarize(fit: &MixedFit) -> Result<([f64; 2], [f64; 3], f64)> {
        if fit.fixed.len() != 2 {
            return Err(invalid("the path model could not be fitted"));
        }
        let beta = [fit.fixed[0].estimate, fit.fixed[1].estimate];
        let var = |effect: &str| {
            fit.variance_components.iter().find(|c| c.effect == effect).map_or(0.0, |c| c.variance.max(0.0))
        };
        let (s00, s11) = (var("Intercept"), var("Time"));
        let rho = fit.correlations.first().map_or(0.0, |c| if c.value.is_finite() { c.value } else { 0.0 });
        let residual_sd = fit.residual.as_ref().map_or(f64::NAN, |r| r.std_dev);
        Ok((beta, [s00, rho * (s00 * s11).sqrt(), s11], residual_sd))
    }

    fn probability(beta: [f64; 2], sigma: [f64; 3], h: f64, threshold: f64, sign: f64) -> f64 {
        let m = beta[0] + beta[1] * h - threshold;
        let v = sigma[0] + 2.0 * sigma[1] * h + sigma[2] * h * h;
        if v > 0.0 {
            Normal::standard().cdf(sign * m / v.sqrt()).unwrap_or(f64::NAN)
        } else if sign * m > 0.0 {
            1.0
        } else {
            0.0
        }
    }

    /// The fraction of units whose path is past the threshold at `t`.
    #[must_use]
    pub fn failure_probability(&self, t: f64) -> f64 {
        match self.time.apply(t) {
            Some(h) => Self::probability(self.beta, self.sigma, h, self.threshold, self.direction.sign()),
            None => f64::NAN,
        }
    }

    /// The smallest time at which `F(t) = p` (NaN when `F` never reaches
    /// `p`), searched up to `horizon`.
    #[must_use]
    pub fn quantile(&self, p: f64, horizon: f64) -> f64 {
        Self::quantile_of(&|t| self.failure_probability(t), p, horizon)
    }

    fn quantile_of(f: &dyn Fn(f64) -> f64, p: f64, horizon: f64) -> f64 {
        if !(p > 0.0 && p < 1.0 && horizon > 0.0) {
            return f64::NAN;
        }
        let n = 400;
        let lo = horizon * 1e-6;
        let mut prev = lo;
        if f(lo) >= p {
            return lo;
        }
        for i in 1..=n {
            let t = lo * (horizon / lo).powf(i as f64 / n as f64);
            if f(t) >= p {
                let (mut a, mut b) = (prev, t);
                for _ in 0..100 {
                    let c = 0.5 * (a + b);
                    if f(c) >= p {
                        b = c;
                    } else {
                        a = c;
                    }
                }
                return 0.5 * (a + b);
            }
            prev = t;
        }
        f64::NAN
    }

    /// Pointwise parametric-bootstrap limits of `F(t)` at `times` and of the
    /// quantiles at `probabilities` (searched up to `horizon`), from
    /// `replicates` simulated datasets with random-number seed `seed`.
    #[must_use]
    pub fn bootstrap(
        &self,
        times: &[f64],
        probabilities: &[f64],
        horizon: f64,
        replicates: usize,
        seed: u64,
        level: f64,
    ) -> BootstrapBands {
        let mut rng = Rng::new(seed);
        let n_units = self.design.iter().map(|d| d.0).max().map_or(0, |m| m + 1);
        let [s00, s01, s11] = self.sigma;
        // Cholesky factor of Σ.
        let l00 = s00.max(0.0).sqrt();
        let l10 = if l00 > 0.0 { s01 / l00 } else { 0.0 };
        let l11 = (s11 - l10 * l10).max(0.0).sqrt();
        let sign = self.direction.sign();
        let mut f_values: Vec<Vec<f64>> = vec![Vec::new(); times.len()];
        let mut q_values: Vec<Vec<f64>> = vec![Vec::new(); probabilities.len()];
        let mut ok = 0;
        for _ in 0..replicates {
            let effects: Vec<(f64, f64)> = (0..n_units)
                .map(|_| {
                    let (z0, z1) = (rng.normal(), rng.normal());
                    (l00 * z0, l10 * z0 + l11 * z1)
                })
                .collect();
            let y: Vec<f64> = self
                .design
                .iter()
                .map(|&(u, h)| {
                    let (b0, b1) = effects[u];
                    self.beta[0] + b0 + (self.beta[1] + b1) * h + self.residual_sd * rng.normal()
                })
                .collect();
            let Ok(fit) = Self::refit(&y, &self.design, n_units, self.reml) else { continue };
            let Ok((beta, sigma, _)) = Self::summarize(&fit) else { continue };
            ok += 1;
            let f = |t: f64| {
                self.time.apply(t).map_or(f64::NAN, |h| Self::probability(beta, sigma, h, self.threshold, sign))
            };
            for (k, &t) in times.iter().enumerate() {
                f_values[k].push(f(t));
            }
            for (k, &p) in probabilities.iter().enumerate() {
                q_values[k].push(Self::quantile_of(&f, p, horizon));
            }
        }
        let alpha = (1.0 - level) / 2.0;
        let percentile = |v: &mut Vec<f64>, q: f64| -> f64 {
            // Unreached quantiles count as +∞.
            for x in v.iter_mut() {
                if x.is_nan() {
                    *x = f64::INFINITY;
                }
            }
            v.sort_by(f64::total_cmp);
            if v.is_empty() {
                return f64::NAN;
            }
            let pos = q * (v.len() - 1) as f64;
            let (i, frac) = (pos.floor() as usize, pos - pos.floor());
            let j = (i + 1).min(v.len() - 1);
            if frac == 0.0 || v[i] == v[j] { v[i] } else { v[i] + frac * (v[j] - v[i]) }
        };
        let (mut lower, mut upper) = (Vec::new(), Vec::new());
        for v in &mut f_values {
            lower.push(percentile(v, alpha));
            upper.push(percentile(v, 1.0 - alpha));
        }
        let quantiles = probabilities
            .iter()
            .zip(&mut q_values)
            .map(|(&p, v)| (p, percentile(v, alpha), percentile(v, 1.0 - alpha)))
            .collect();
        BootstrapBands { times: times.to_vec(), lower, upper, quantiles, replicates: ok }
    }
}

// ---------------------------------------------------------------------------
// Destructive degradation
// ---------------------------------------------------------------------------

/// A destructive degradation model.
#[derive(Debug, Clone, PartialEq)]
pub struct DestructiveModel {
    /// Distribution of the transformed response (one of
    /// [`RegressionFamily::REAL_LINE`]).
    pub family: RegressionFamily,
    /// Response transform `g`.
    pub response: ScaleTransform,
    /// Time transform `h`.
    pub time: ScaleTransform,
    /// Optional stress acting on the degradation rate.
    pub stress: Option<StressVariable>,
    /// Use condition of the stress (the reference at which `β₁` is the rate).
    pub use_stress: Option<f64>,
    /// Failure threshold on the response's own scale.
    pub threshold: f64,
    /// Failure direction.
    pub direction: FailureDirection,
}

/// One measurement per unit.
#[derive(Debug, Clone, PartialEq)]
pub struct DestructiveData {
    /// Measurement times.
    pub times: Vec<f64>,
    /// Measured values.
    pub values: Vec<f64>,
    /// Stress of each unit (when the model has a stress).
    pub stresses: Vec<f64>,
}

/// A fitted destructive degradation model.
#[derive(Debug, Clone)]
pub struct DestructiveFit {
    /// The model.
    pub model: DestructiveModel,
    /// Estimates `(β₀, β₁, [β₂], ln σ)`.
    pub parameters: Vec<f64>,
    /// Covariance of the estimates (`None` when the observed information is
    /// not positive definite).
    pub covariance: Option<Vec<Vec<f64>>>,
    /// Maximized log-likelihood (of the transformed responses).
    pub log_likelihood: f64,
    /// The threshold on the transformed scale.
    pub threshold: f64,
    /// Number of measurements.
    pub n: usize,
    x_ref: f64,
}

/// Least squares of `y` on `(1, w)`: intercept, slope and residual sum of squares.
fn simple_regression(w: &[f64], y: &[f64]) -> Option<(f64, f64, f64)> {
    let n = w.len() as f64;
    let mw = w.iter().sum::<f64>() / n;
    let my = y.iter().sum::<f64>() / n;
    let sww: f64 = w.iter().map(|v| (v - mw).powi(2)).sum();
    if !(sww > 0.0 && sww.is_finite()) {
        return None;
    }
    let b = w.iter().zip(y).map(|(a, c)| (a - mw) * (c - my)).sum::<f64>() / sww;
    let a = my - b * mw;
    let sse = w.iter().zip(y).map(|(x, c)| (c - a - b * x).powi(2)).sum();
    Some((a, b, sse))
}

/// Fit a destructive degradation model (see the module docs).
///
/// # Errors
/// [`StatError::InvalidArgument`] for a family that is not on the real line,
/// mismatched lengths, values outside a transform's domain, a threshold
/// outside the response transform's domain, a stress without a valid use
/// condition, fewer than four measurements, all measurements at one time,
/// or a likelihood without a maximum.
pub fn fit_destructive(data: &DestructiveData, model: &DestructiveModel) -> Result<DestructiveFit> {
    if model.family.is_log() {
        return Err(invalid(
            "destructive degradation models the transformed response with a normal, logistic or extreme value \
             distribution",
        ));
    }
    let n = data.times.len();
    if data.values.len() != n || (model.stress.is_some() && data.stresses.len() != n) {
        return Err(invalid("each measurement needs its time (and stress)"));
    }
    if n < 4 {
        return Err(StatError::InsufficientData { required: 4, found: n });
    }
    let threshold = model.response.apply(model.threshold).ok_or_else(|| {
        invalid(format!("the threshold {} is outside the response transform's domain", model.threshold))
    })?;
    let x_ref = match (&model.stress, model.use_stress) {
        (None, _) => 0.0,
        (Some(v), Some(u)) if v.valid(u) => v.transformed(u),
        (Some(_), _) => return Err(invalid("a stress needs a valid use condition")),
    };
    let mut g = Vec::with_capacity(n);
    let mut h = Vec::with_capacity(n);
    let mut dx = Vec::with_capacity(n);
    for i in 0..n {
        let (Some(gi), Some(hi)) = (model.response.apply(data.values[i]), model.time.apply(data.times[i])) else {
            return Err(invalid(format!(
                "the measurement ({}, {}) is outside the time or response transform's domain",
                data.times[i], data.values[i]
            )));
        };
        let x = match &model.stress {
            Some(v) if v.valid(data.stresses[i]) => v.transformed(data.stresses[i]) - x_ref,
            Some(_) => return Err(invalid(format!("the stress {} does not suit its relationship", data.stresses[i]))),
            None => 0.0,
        };
        g.push(gi);
        h.push(hi);
        dx.push(x);
    }
    let with_stress = model.stress.is_some();
    let family = model.family;
    let ll = |th: &[f64]| -> f64 {
        let k = th.len();
        let sigma = th[k - 1].exp();
        if !(sigma > 0.0 && sigma.is_finite()) {
            return f64::NEG_INFINITY;
        }
        let mut total = 0.0;
        for i in 0..n {
            let rate = if with_stress { th[1] * (th[2] * dx[i]).exp() } else { th[1] };
            let mu = th[0] + rate * h[i];
            total += family.distribution(mu, sigma).ln_pdf(g[i]);
        }
        if total.is_finite() { total } else { f64::NEG_INFINITY }
    };
    // Starting values: least squares over a grid of β₂.
    let span = dx.iter().map(|v| v.abs()).fold(0.0, f64::max);
    let grid: Vec<f64> = if with_stress && span > 0.0 {
        (0..=60).map(|i| (-6.0 + 0.2 * f64::from(i)) / span).collect()
    } else {
        vec![0.0]
    };
    let mut best: Option<(f64, f64, f64, f64)> = None;
    for &b2 in &grid {
        let w: Vec<f64> = h.iter().zip(&dx).map(|(hi, xi)| hi * (b2 * xi).exp()).collect();
        if let Some((a, b, sse)) = simple_regression(&w, &g) {
            if best.is_none_or(|bb| sse < bb.3) {
                best = Some((a, b, b2, sse));
            }
        }
    }
    let (a, b, b2, sse) = best.ok_or_else(|| invalid("the measurements are all at one time"))?;
    let sigma0 = (sse / n as f64).sqrt().max(1e-8 * (1.0 + a.abs()));
    let mut start = vec![a, b];
    if with_stress {
        start.push(b2);
    }
    start.push(sigma0.ln());
    let mean_h = h.iter().map(|v| v.abs()).sum::<f64>() / n as f64;
    let steps = |th: &[f64]| -> Vec<f64> {
        let sigma = th[th.len() - 1].exp().clamp(1e-12, 1e12);
        let mut s = vec![1e-4 * sigma, 1e-4 * sigma / mean_h.max(1e-12)];
        if with_stress {
            s.push(1e-4 / span.max(1e-12));
        }
        s.push(1e-4);
        s
    };
    let (parameters, value) = maximize(&ll, &start, &steps)
        .ok_or_else(|| invalid("the destructive degradation likelihood could not be maximized"))?;
    let hstep: Vec<f64> = steps(&parameters).iter().map(|v| v * 3.0).collect();
    let (_, _, hess) = derivatives(&ll, &parameters, &hstep);
    let neg: Vec<Vec<f64>> = hess.iter().map(|r| r.iter().map(|v| -v).collect()).collect();
    Ok(DestructiveFit {
        model: model.clone(),
        covariance: inverse_spd(&neg),
        parameters,
        log_likelihood: value,
        threshold,
        n,
        x_ref,
    })
}

impl DestructiveFit {
    fn dx(&self, s: Option<f64>) -> f64 {
        match (&self.model.stress, s) {
            (Some(v), Some(s)) => v.transformed(s) - self.x_ref,
            _ => 0.0,
        }
    }

    /// The degradation rate on the transformed scales at stress `s`.
    fn rate(&self, th: &[f64], s: Option<f64>) -> f64 {
        if self.model.stress.is_some() { th[1] * (th[2] * self.dx(s)).exp() } else { th[1] }
    }

    fn time_quantile(&self, th: &[f64], p: f64, s: Option<f64>) -> f64 {
        let sigma = th[th.len() - 1].exp();
        let w = match self.model.direction {
            FailureDirection::Falling => self.model.family.standard_quantile(p),
            FailureDirection::Rising => self.model.family.standard_quantile(1.0 - p),
        };
        let rate = self.rate(th, s);
        if rate * self.model.direction.sign() <= 0.0 {
            return f64::NAN;
        }
        let h = (self.threshold - sigma * w - th[0]) / rate;
        self.model.time.invert(h).filter(|t| *t > 0.0).unwrap_or(f64::NAN)
    }

    fn time_probability(&self, th: &[f64], t: f64, s: Option<f64>) -> f64 {
        let Some(h) = self.model.time.apply(t) else { return f64::NAN };
        let mu = th[0] + self.rate(th, s) * h;
        let z = (self.threshold - mu) / th[th.len() - 1].exp();
        match self.model.direction {
            FailureDirection::Falling => self.model.family.standard_cdf(z),
            FailureDirection::Rising => 1.0 - self.model.family.standard_cdf(z),
        }
    }

    /// Delta-method standard error of `g(θ)`.
    #[must_use]
    pub fn delta_std_error(&self, g: &dyn Fn(&[f64]) -> f64) -> f64 {
        let Some(v) = &self.covariance else { return f64::NAN };
        let k = self.parameters.len();
        let mut th = self.parameters.clone();
        let mut grad = vec![0.0; k];
        for i in 0..k {
            let step = 1e-6 * (1.0 + self.parameters[i].abs());
            th[i] = self.parameters[i] + step;
            let up = g(&th);
            th[i] = self.parameters[i] - step;
            let down = g(&th);
            th[i] = self.parameters[i];
            grad[i] = (up - down) / (2.0 * step);
        }
        let var: f64 = (0..k).map(|i| (0..k).map(|j| grad[i] * v[i][j] * grad[j]).sum::<f64>()).sum();
        if var >= 0.0 { var.sqrt() } else { f64::NAN }
    }

    /// The parameters `(β₀, β₁, [β₂], σ)` with Wald limits (`σ` on the log scale).
    #[must_use]
    pub fn parameter_estimates(&self, level: f64) -> Vec<IntervalEstimate> {
        let z = Normal::standard().quantile(0.5 + level / 2.0).unwrap_or(f64::NAN);
        let k = self.parameters.len();
        let names: &[&'static str] =
            if self.model.stress.is_some() { &["β₀", "β₁", "β₂", "σ"] } else { &["β₀", "β₁", "σ"] };
        (0..k)
            .map(|i| {
                let se = self.covariance.as_ref().map_or(f64::NAN, |c| c[i][i].max(0.0).sqrt());
                let x = self.parameters[i];
                if i == k - 1 {
                    let est = x.exp();
                    IntervalEstimate {
                        name: names[i],
                        estimate: est,
                        std_error: est * se,
                        wald: ((x - z * se).exp(), (x + z * se).exp()),
                        likelihood_ratio: None,
                    }
                } else {
                    IntervalEstimate {
                        name: names[i],
                        estimate: x,
                        std_error: se,
                        wald: (x - z * se, x + z * se),
                        likelihood_ratio: None,
                    }
                }
            })
            .collect()
    }

    /// The `p` quantile of the time to reach the threshold at stress `s`,
    /// with Wald limits formed on log time.
    #[must_use]
    pub fn quantile(&self, p: f64, s: Option<f64>, level: f64) -> IntervalEstimate {
        let estimate = self.time_quantile(&self.parameters, p, s);
        let se = self.delta_std_error(&|th| self.time_quantile(th, p, s).ln());
        let z = Normal::standard().quantile(0.5 + level / 2.0).unwrap_or(f64::NAN);
        let wald = ((estimate.ln() - z * se).exp(), (estimate.ln() + z * se).exp());
        IntervalEstimate { name: "Quantile", estimate, std_error: estimate * se, wald, likelihood_ratio: None }
    }

    /// The probability of having reached the threshold by `t` at stress `s`,
    /// with Wald limits formed on the logit scale.
    #[must_use]
    pub fn failure_probability(&self, t: f64, s: Option<f64>, level: f64) -> IntervalEstimate {
        let estimate = self.time_probability(&self.parameters, t, s);
        let logit = |f: f64| f.ln() - (-f).ln_1p();
        let se = self.delta_std_error(&|th| logit(self.time_probability(th, t, s)));
        let z = Normal::standard().quantile(0.5 + level / 2.0).unwrap_or(f64::NAN);
        let expit = |v: f64| 1.0 / (1.0 + (-v).exp());
        let s_hat = logit(estimate);
        let wald = if estimate > 0.0 && estimate < 1.0 {
            (expit(s_hat - z * se), expit(s_hat + z * se))
        } else {
            (f64::NAN, f64::NAN)
        };
        IntervalEstimate {
            name: "Failure probability",
            estimate,
            std_error: estimate * (1.0 - estimate) * se,
            wald,
            likelihood_ratio: None,
        }
    }

    /// The fitted location of the response at time `t` and stress `s`, on
    /// the response's own scale.
    #[must_use]
    pub fn location_at(&self, t: f64, s: Option<f64>) -> f64 {
        let Some(h) = self.model.time.apply(t) else { return f64::NAN };
        let mu = self.parameters[0] + self.rate(&self.parameters, s) * h;
        self.model.response.invert(mu).unwrap_or(f64::NAN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_transforms_invert() {
        for t in ScaleTransform::ALL {
            let u = t.apply(4.0).unwrap();
            assert!((t.invert(u).unwrap() - 4.0).abs() < 1e-12);
            assert_eq!(ScaleTransform::from_key(t.key()), Some(t));
        }
        assert!(ScaleTransform::Log.apply(0.0).is_none() && ScaleTransform::Sqrt.apply(-1.0).is_none());
    }

    /// An exact straight line crosses its threshold where the line says;
    /// a path that moves away is censored at its last time, and one whose
    /// fitted path is already past the threshold at time zero is
    /// left-censored at the first time.
    #[test]
    fn crossings_are_classified() {
        let times = [1.0, 2.0, 3.0, 4.0];
        let linear = PathModel::Linear { response: ScaleTransform::Identity, time: ScaleTransform::Identity };
        let rising: Vec<f64> = times.iter().map(|t| 1.0 + 2.0 * t).collect();
        let p = fit_unit_path(&times, &rising, linear, 15.0, FailureDirection::Rising).unwrap();
        assert!(matches!(p.crossing, Crossing::At(t) if (t - 7.0).abs() < 1e-10), "{:?}", p.crossing);
        let p = fit_unit_path(&times, &rising, linear, 2.0, FailureDirection::Falling).unwrap();
        assert!(matches!(p.crossing, Crossing::Never(4.0)), "falls never: {:?}", p.crossing);
        let p = fit_unit_path(&times, &rising, linear, 0.5, FailureDirection::Rising).unwrap();
        assert!(matches!(p.crossing, Crossing::Before(1.0)), "already past: {:?}", p.crossing);
        // A power path a·t^b with a = 2, b = 0.5 reaches 6 at t = 9.
        let power: Vec<f64> = times.iter().map(|t| 2.0 * t.sqrt()).collect();
        let p = fit_unit_path(&times, &power, PathModel::Power, 6.0, FailureDirection::Rising).unwrap();
        assert!(matches!(p.crossing, Crossing::At(t) if (t - 9.0).abs() < 1e-5), "{:?}", p.crossing);
    }
}
