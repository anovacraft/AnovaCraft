//! Exponential smoothing in innovations state-space form.
//!
//! Each model of the ETS(error, trend, seasonal) taxonomy of Hyndman,
//! Koehler, Snyder & Grose (2002), "A state space framework for automatic
//! forecasting using exponential smoothing methods", *International Journal
//! of Forecasting* 18, and of Hyndman, Koehler, Ord & Snyder (2008),
//! *Forecasting with Exponential Smoothing: The State Space Approach*
//! (Springer), is a single-source-of-error state-space model. With level
//! `ℓ`, slope `b`, seasonal states `s` (period `m`), damping `φ` and
//!
//! * base `B_t = ℓ_{t−1} + φ b_{t−1}` (`φ = 1` for an undamped trend, no
//!   `b` term without a trend),
//! * one-step forecast `μ_t = B_t`, `B_t + s_{t−m}` or `B_t · s_{t−m}` for
//!   no, additive or multiplicative seasonality,
//!
//! the observation is `y_t = μ_t + ε_t` (additive error) or
//! `y_t = μ_t (1 + ε_t)` (multiplicative error), `ε_t ~ N(0, σ²)`, and the
//! states are corrected by the one-step error `d_t = y_t − μ_t`
//! (Hyndman et al. 2008, Table 2.3):
//!
//! ```text
//! ℓ_t = B_t + α d_t / c_t           c_t = s_{t−m} for multiplicative seasonality, else 1
//! b_t = φ b_{t−1} + β d_t / c_t
//! s_t = s_{t−m} + γ d_t             (additive seasonality)
//! s_t = s_{t−m} + γ d_t / B_t       (multiplicative seasonality)
//! ```
//!
//! The error type changes the likelihood and the forecast distribution, not
//! the point recursion.
//!
//! **Estimation** ([`fit_ets`]) maximises the Gaussian likelihood over the
//! smoothing parameters and the initial states `x₀ = (ℓ₀, b₀, s₀, …, s₋ₘ₊₁)`
//! jointly. The innovation variance is concentrated out, leaving the
//! criterion `n ln Σ ε_t² + 2 Σ ln|μ_t|` (the second term for multiplicative
//! errors only; Hyndman et al. 2008, ch. 5), minimised by box-constrained
//! Nelder–Mead ([`stat_core::optimize::minimize_in_box`]) from heuristic
//! initial states (§2.6.1: a 2×m moving average over the first cycles for
//! the seasonal indices, then a straight line through the first ten
//! seasonally adjusted values for level and slope). Parameters are kept in
//! the usual region `0 < β < α < 1`, `0 < γ < 1 − α`, `0.8 ≤ φ ≤ 0.98` (by
//! default `[10⁻⁴, 0.9999]` for α, β and γ) and, for seasonal models, in the
//! admissible (forecastable) region where every eigenvalue of the discount
//! matrix `D = F − g w′` of the linear (additive-error, additive-season)
//! form lies in the closed unit disc (Hyndman et al. 2008, ch. 10; the unit
//! eigenvalue that every unnormalised seasonal model has is allowed). The
//! seasonal initial states are normalised to sum to 0 (additive) or `m`
//! (multiplicative), so `m − 1` of them are free.
//!
//! **Forecasts** ([`EtsFit::forecast`]): point forecasts follow the
//! recursion with zero future errors. For the linear models (additive or
//! no trend, additive or no seasonality) these are the forecast means and
//! the forecast variances are analytic (Hyndman et al. 2008, ch. 6, classes
//! 1 and 2): with `c_j = w′Fʲ⁻¹g`, additive errors give
//! `v_h = σ²(1 + Σ_{j<h} c_j²)`, and multiplicative errors give
//! `v_h = (1 + σ²) θ_h − μ_h²` with `θ_h = μ_h² + σ² Σ_{j<h} c_j² θ_{h−j}`.
//! Intervals are `μ_h ± z σ_h`. Multiplicative-seasonal models have no
//! closed form; their intervals are quantiles of seeded simulated sample
//! paths ([`stat_core::rng::Rng`]).
//!
//! **Model selection** ([`select_ets`]) fits every allowed model and keeps
//! the one with the smallest AICc. Models with additive errors and
//! multiplicative seasonality are excluded by default because their
//! forecast distributions can have infinite variance (Hyndman et al. 2008,
//! ch. 6); multiplicative components need a strictly positive series, and
//! seasonal models need at least two full cycles.
//!
//! **Reporting conventions.** The log-likelihood is the full Gaussian one at
//! the maximum-likelihood variance, `ℓ = −(n/2)(ln(2π SSE/n) + 1) − Σ ln|μ_t|`
//! (SSE the sum of squared innovations). Information criteria count the
//! estimated parameters `p` (smoothing parameters and initial states) plus
//! one for σ²: `AIC = −2ℓ + 2(p+1)`, `AICc = AIC + 2(p+1)(p+2)/(n−p−2)`,
//! `BIC = −2ℓ + (p+1) ln n`. The reported residual variance (also used for
//! forecast intervals) is `σ² = SSE/(n − p)`.

mod admissible;
mod estimate;
mod forecast;
mod recursion;

#[cfg(test)]
mod tests;

pub use forecast::{EtsForecast, ForecastOptions, IntervalMethod};

use crate::{Result, StatError};
use recursion::{Coefficients, FilterOutput, State, filter};

/// Form of the error term of an ETS model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorType {
    /// `y_t = μ_t + ε_t`.
    Additive,
    /// `y_t = μ_t (1 + ε_t)`: the error is relative to the one-step forecast.
    Multiplicative,
}

/// Form of the trend component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrendType {
    /// No slope state.
    None,
    /// Additive (linear) trend `ℓ + h b`.
    Additive,
    /// Damped additive trend `ℓ + (φ + φ² + … + φʰ) b`, `0 < φ < 1`.
    DampedAdditive,
}

/// Form of the seasonal component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SeasonalType {
    /// No seasonal states.
    None,
    /// Seasonal effects added to the level and trend.
    Additive,
    /// Seasonal factors multiplying the level and trend.
    Multiplicative,
}

/// One model of the ETS(error, trend, seasonal) taxonomy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EtsSpec {
    /// Error form.
    pub error: ErrorType,
    /// Trend form.
    pub trend: TrendType,
    /// Seasonal form.
    pub seasonal: SeasonalType,
}

impl EtsSpec {
    /// A model with the given components.
    #[must_use]
    pub const fn new(error: ErrorType, trend: TrendType, seasonal: SeasonalType) -> Self {
        Self { error, trend, seasonal }
    }

    /// Simple exponential smoothing, ETS(A,N,N).
    #[must_use]
    pub const fn simple() -> Self {
        Self::new(ErrorType::Additive, TrendType::None, SeasonalType::None)
    }

    /// Holt's linear trend method, ETS(A,A,N).
    #[must_use]
    pub const fn holt_linear() -> Self {
        Self::new(ErrorType::Additive, TrendType::Additive, SeasonalType::None)
    }

    /// Damped trend method (Gardner & McKenzie 1985), ETS(A,Ad,N).
    #[must_use]
    pub const fn damped_trend() -> Self {
        Self::new(ErrorType::Additive, TrendType::DampedAdditive, SeasonalType::None)
    }

    /// Additive Holt–Winters method, ETS(A,A,A).
    #[must_use]
    pub const fn holt_winters_additive() -> Self {
        Self::new(ErrorType::Additive, TrendType::Additive, SeasonalType::Additive)
    }

    /// Multiplicative Holt–Winters method with multiplicative errors,
    /// ETS(M,A,M). (The additive-error form ETS(A,A,M) has the same point
    /// forecasts but can have infinite forecast variance; build it with
    /// [`EtsSpec::new`] if wanted.)
    #[must_use]
    pub const fn holt_winters_multiplicative() -> Self {
        Self::new(ErrorType::Multiplicative, TrendType::Additive, SeasonalType::Multiplicative)
    }

    /// The taxonomy label, such as `ETS(M,Ad,A)`.
    #[must_use]
    pub fn label(&self) -> String {
        let e = match self.error {
            ErrorType::Additive => "A",
            ErrorType::Multiplicative => "M",
        };
        let t = match self.trend {
            TrendType::None => "N",
            TrendType::Additive => "A",
            TrendType::DampedAdditive => "Ad",
        };
        let s = match self.seasonal {
            SeasonalType::None => "N",
            SeasonalType::Additive => "A",
            SeasonalType::Multiplicative => "M",
        };
        format!("ETS({e},{t},{s})")
    }

    /// A descriptive method name for the five classical methods (simple
    /// exponential smoothing, Holt linear trend, damped trend, additive and
    /// multiplicative Holt–Winters), otherwise the taxonomy label.
    #[must_use]
    pub fn name(&self) -> String {
        match (self.error, self.trend, self.seasonal) {
            (ErrorType::Additive, TrendType::None, SeasonalType::None) => "Simple exponential smoothing".into(),
            (ErrorType::Additive, TrendType::Additive, SeasonalType::None) => "Holt linear trend".into(),
            (ErrorType::Additive, TrendType::DampedAdditive, SeasonalType::None) => "Damped trend".into(),
            (ErrorType::Additive, TrendType::Additive, SeasonalType::Additive) => "Additive Holt–Winters".into(),
            (ErrorType::Multiplicative, TrendType::Additive, SeasonalType::Multiplicative) => {
                "Multiplicative Holt–Winters".into()
            }
            _ => self.label(),
        }
    }

    /// Whether the forecast distribution has the analytic moments of
    /// Hyndman et al. (2008) classes 1 and 2: no multiplicative seasonality.
    #[must_use]
    pub fn has_linear_states(&self) -> bool {
        self.seasonal != SeasonalType::Multiplicative
    }

    fn has_trend(&self) -> bool {
        self.trend != TrendType::None
    }

    fn has_season(&self) -> bool {
        self.seasonal != SeasonalType::None
    }

    fn is_multiplicative(&self) -> bool {
        self.error == ErrorType::Multiplicative || self.seasonal == SeasonalType::Multiplicative
    }
}

/// Smoothing parameters of a fitted model. Parameters a model does not have
/// are `None`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SmoothingParameters {
    /// Level smoothing `α`.
    pub alpha: f64,
    /// Slope smoothing `β` (trend models).
    pub beta: Option<f64>,
    /// Seasonal smoothing `γ` (seasonal models).
    pub gamma: Option<f64>,
    /// Damping `φ` (damped-trend models).
    pub phi: Option<f64>,
}

/// A state vector: level, slope and seasonal states.
///
/// As initial states, `seasonal[j]` is the seasonal component used for the
/// observation at index `j` (the first cycle, in time order), i.e.
/// `s_{j+1−m}` in the book's notation. As final states, `seasonal[j]` is the
/// component used for the forecast `j + 1` steps ahead.
#[derive(Debug, Clone, PartialEq)]
pub struct StateVector {
    /// Level `ℓ`.
    pub level: f64,
    /// Slope `b` (0 for models without a trend).
    pub slope: f64,
    /// Seasonal states (empty for non-seasonal models; `m` values otherwise).
    pub seasonal: Vec<f64>,
}

/// Bounds of the parameter search.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParameterBounds {
    /// Lower and upper limits for `α`, `β` and `γ` (default `[10⁻⁴, 0.9999]`);
    /// in addition `β ≤ α` and `γ ≤ 1 − α`.
    pub smoothing: (f64, f64),
    /// Lower and upper limits for `φ` (default `[0.8, 0.98]`).
    pub damping: (f64, f64),
    /// Also require the admissible region for seasonal models (default true).
    pub admissible: bool,
}

impl Default for ParameterBounds {
    fn default() -> Self {
        Self { smoothing: (1e-4, 0.9999), damping: (0.8, 0.98), admissible: true }
    }
}

/// Options for [`fit_ets`]. Every `Some` value is held fixed instead of
/// estimated.
#[derive(Debug, Clone, PartialEq)]
pub struct EtsOptions {
    /// Seasonal period `m` (1 for non-seasonal data; must be ≥ 2 for a
    /// seasonal model).
    pub period: usize,
    /// Fixed `α`.
    pub alpha: Option<f64>,
    /// Fixed `β`.
    pub beta: Option<f64>,
    /// Fixed `γ`.
    pub gamma: Option<f64>,
    /// Fixed `φ`.
    pub phi: Option<f64>,
    /// Fixed initial states (see [`StateVector`] for the order; all `m`
    /// seasonal values are given and used as they are).
    pub initial: Option<StateVector>,
    /// Count supplied (fixed) values as estimated parameters in σ², AIC,
    /// AICc and BIC. Use it to reproduce a fit whose parameters were
    /// estimated elsewhere; supplied initial seasonal states then count
    /// `m − 1`.
    pub count_supplied_as_estimated: bool,
    /// Search bounds.
    pub bounds: ParameterBounds,
    /// Objective evaluations allowed per optimisation start (default 4000 per
    /// free parameter).
    pub max_evaluations: Option<usize>,
}

impl EtsOptions {
    /// Estimate everything, with default bounds, for period `period`.
    #[must_use]
    pub fn new(period: usize) -> Self {
        Self {
            period,
            alpha: None,
            beta: None,
            gamma: None,
            phi: None,
            initial: None,
            count_supplied_as_estimated: false,
            bounds: ParameterBounds::default(),
            max_evaluations: None,
        }
    }
}

/// In-sample accuracy of the one-step forecasts, from `d_t = y_t − μ_t`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Accuracy {
    /// Root mean squared error `√(Σd²/n)`.
    pub rmse: f64,
    /// Mean absolute error `Σ|d|/n`.
    pub mae: f64,
    /// Mean absolute percentage error `100 Σ|d/y|/n`; `None` when a value is 0.
    pub mape: Option<f64>,
}

/// A fitted ETS model.
#[derive(Debug, Clone, PartialEq)]
pub struct EtsFit {
    /// The model.
    pub spec: EtsSpec,
    /// Seasonal period `m`.
    pub period: usize,
    /// Number of observations `n`.
    pub n: usize,
    /// Smoothing parameters.
    pub parameters: SmoothingParameters,
    /// Initial states `x₀`.
    pub initial: StateVector,
    /// States after the last observation (the forecast origin).
    pub final_state: StateVector,
    /// Number of estimated parameters `p` (smoothing parameters and initial
    /// states; σ² not included).
    pub estimated_parameters: usize,
    /// Gaussian log-likelihood at the maximum-likelihood σ².
    pub log_likelihood: f64,
    /// Akaike information criterion.
    pub aic: f64,
    /// Small-sample corrected AIC; `None` when `n ≤ p + 2`.
    pub aicc: Option<f64>,
    /// Bayesian information criterion.
    pub bic: f64,
    /// Innovation variance `SSE/(n − p)` (relative-error variance for
    /// multiplicative errors); `SSE/n` when `n ≤ p`.
    pub sigma2: f64,
    /// One-step-ahead fitted values `μ_t`.
    pub fitted: Vec<f64>,
    /// Innovations `ε_t`: `y_t − μ_t` (additive errors) or `(y_t − μ_t)/μ_t`
    /// (multiplicative errors).
    pub residuals: Vec<f64>,
    /// In-sample accuracy of the fitted values.
    pub accuracy: Accuracy,
}

impl EtsFit {
    /// Innovation standard deviation `√σ²`.
    #[must_use]
    pub fn sigma(&self) -> f64 {
        self.sigma2.sqrt()
    }

    /// Response residuals `y_t − μ_t`, whatever the error form.
    #[must_use]
    pub fn response_residuals(&self) -> Vec<f64> {
        match self.spec.error {
            ErrorType::Additive => self.residuals.clone(),
            ErrorType::Multiplicative => self.residuals.iter().zip(&self.fitted).map(|(e, m)| e * m).collect(),
        }
    }

    fn coefficients(&self) -> Coefficients {
        Coefficients::from_parameters(&self.spec, &self.parameters)
    }
}

/// Validate the series and model against the period.
fn validate(y: &[f64], spec: &EtsSpec, period: usize) -> Result<()> {
    if y.iter().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "time series" });
    }
    if period == 0 {
        return Err(StatError::InvalidArgument { message: "the seasonal period must be at least 1".into() });
    }
    if spec.has_season() {
        if period < 2 {
            return Err(StatError::InvalidArgument { message: "a seasonal model needs a period of at least 2".into() });
        }
        if y.len() < 2 * period {
            return Err(StatError::InsufficientData { required: 2 * period, found: y.len() });
        }
    }
    let minimum = 3 + usize::from(spec.has_trend());
    if y.len() < minimum {
        return Err(StatError::InsufficientData { required: minimum, found: y.len() });
    }
    if spec.is_multiplicative() && y.iter().any(|&v| v <= 0.0) {
        return Err(StatError::InvalidArgument {
            message: "multiplicative errors or seasonality need a strictly positive series".into(),
        });
    }
    Ok(())
}

/// Fit an exponential smoothing model by maximum likelihood.
///
/// Parameters and initial states given in `options` are held fixed; the
/// rest are estimated jointly (see the module documentation). With
/// everything fixed, the model is only evaluated, which reproduces fitted
/// values, likelihood and forecasts of a fit made elsewhere.
///
/// # Errors
/// * [`StatError::NonFiniteInput`] for NaN or infinite values.
/// * [`StatError::InsufficientData`] for fewer than 3 observations (4 with a
///   trend), or fewer than two full cycles for a seasonal model.
/// * [`StatError::InvalidArgument`] for a zero period, a seasonal model with
///   period 1, multiplicative components with a non-positive value, fixed
///   values outside `[0, 1]` (`φ` in `(0, 1]`), fixed parameters given for
///   components the model lacks, fixed initial states of the wrong shape, or
///   fixed values at which the multiplicative recursions break down
///   (non-positive one-step forecasts).
/// * [`StatError::ConvergenceFailure`] when no admissible start is found.
pub fn fit_ets(y: &[f64], spec: EtsSpec, options: &EtsOptions) -> Result<EtsFit> {
    validate(y, &spec, options.period)?;
    let (coefficients, initial, estimated) = estimate::estimate(y, &spec, options)?;
    let state = State::from_vector(&initial, &spec);
    let out = filter(y, &spec, &coefficients, state).ok_or_else(|| StatError::InvalidArgument {
        message: "the multiplicative recursions break down (a one-step forecast or state is not positive)".into(),
    })?;
    Ok(assemble(y, spec, options.period, &coefficients, initial, estimated, out))
}

/// Package a filter run as a fit.
fn assemble(
    y: &[f64],
    spec: EtsSpec,
    period: usize,
    c: &Coefficients,
    initial: StateVector,
    estimated: usize,
    out: FilterOutput,
) -> EtsFit {
    let n = y.len();
    let nf = n as f64;
    let sse = out.sse;
    let log_likelihood = -0.5 * nf * ((std::f64::consts::TAU * sse / nf).ln() + 1.0) - 0.5 * out.log_mu_twice;
    let k = (estimated + 1) as f64;
    let aic = -2.0 * log_likelihood + 2.0 * k;
    let aicc = (nf - k - 1.0 > 0.0).then(|| aic + 2.0 * k * (k + 1.0) / (nf - k - 1.0));
    let bic = -2.0 * log_likelihood + k * nf.ln();
    let sigma2 = if n > estimated { sse / (n - estimated) as f64 } else { sse / nf };
    let d: Vec<f64> = y.iter().zip(&out.fitted).map(|(v, m)| v - m).collect();
    let rmse = (d.iter().map(|v| v * v).sum::<f64>() / nf).sqrt();
    let mae = d.iter().map(|v| v.abs()).sum::<f64>() / nf;
    let mape =
        y.iter().all(|&v| v != 0.0).then(|| 100.0 * d.iter().zip(y).map(|(e, v)| (e / v).abs()).sum::<f64>() / nf);
    EtsFit {
        spec,
        period,
        n,
        parameters: c.to_parameters(&spec),
        initial,
        final_state: out.final_state.to_vector(),
        estimated_parameters: estimated,
        log_likelihood,
        aic,
        aicc,
        bic,
        sigma2,
        fitted: out.fitted,
        residuals: out.innovations,
        accuracy: Accuracy { rmse, mae, mape },
    }
}

/// Options for [`select_ets`].
#[derive(Debug, Clone, PartialEq)]
pub struct SelectionOptions {
    /// Seasonal period `m` (1 for non-seasonal data).
    pub period: usize,
    /// Consider multiplicative errors and seasonality (default true; used
    /// only when the series is strictly positive).
    pub allow_multiplicative: bool,
    /// Consider damped trends (default true).
    pub allow_damped: bool,
    /// Consider seasonal models (default true; needs `m ≥ 2` and two full
    /// cycles).
    pub allow_seasonal: bool,
    /// Exclude the models with possibly infinite forecast variance,
    /// additive errors with multiplicative seasonality (default true).
    pub exclude_unstable: bool,
    /// Search bounds for every candidate.
    pub bounds: ParameterBounds,
}

impl SelectionOptions {
    /// Defaults for period `period`.
    #[must_use]
    pub fn new(period: usize) -> Self {
        Self {
            period,
            allow_multiplicative: true,
            allow_damped: true,
            allow_seasonal: true,
            exclude_unstable: true,
            bounds: ParameterBounds::default(),
        }
    }
}

/// One candidate of an automatic selection.
#[derive(Debug, Clone, PartialEq)]
pub struct EtsCandidate {
    /// The model.
    pub spec: EtsSpec,
    /// Its AICc, or `None` when it could not be fitted or its AICc is
    /// undefined (`n ≤ p + 2`).
    pub aicc: Option<f64>,
}

/// Result of [`select_ets`].
#[derive(Debug, Clone, PartialEq)]
pub struct EtsSelection {
    /// The fit with the smallest AICc.
    pub best: EtsFit,
    /// Every candidate considered, in the order tried.
    pub candidates: Vec<EtsCandidate>,
}

/// The models [`select_ets`] considers for a series, in the order tried.
#[must_use]
pub fn candidate_models(y: &[f64], options: &SelectionOptions) -> Vec<EtsSpec> {
    let positive = y.iter().all(|&v| v > 0.0);
    let multiplicative = options.allow_multiplicative && positive;
    let seasonal = options.allow_seasonal && options.period >= 2 && y.len() >= 2 * options.period;
    let errors: &[ErrorType] =
        if multiplicative { &[ErrorType::Additive, ErrorType::Multiplicative] } else { &[ErrorType::Additive] };
    let trends: &[TrendType] = if options.allow_damped {
        &[TrendType::None, TrendType::Additive, TrendType::DampedAdditive]
    } else {
        &[TrendType::None, TrendType::Additive]
    };
    let seasons: Vec<SeasonalType> = if !seasonal {
        vec![SeasonalType::None]
    } else if multiplicative {
        vec![SeasonalType::None, SeasonalType::Additive, SeasonalType::Multiplicative]
    } else {
        vec![SeasonalType::None, SeasonalType::Additive]
    };
    let mut out = Vec::new();
    for &error in errors {
        for &trend in trends {
            for &seasonal in &seasons {
                if options.exclude_unstable && error == ErrorType::Additive && seasonal == SeasonalType::Multiplicative
                {
                    continue;
                }
                out.push(EtsSpec::new(error, trend, seasonal));
            }
        }
    }
    out
}

/// Fit every allowed ETS model ([`candidate_models`]) and keep the one with
/// the smallest AICc (Hyndman et al. 2002, §3).
///
/// # Errors
/// * [`StatError::NonFiniteInput`] for NaN or infinite values.
/// * [`StatError::InvalidArgument`] for a zero period.
/// * [`StatError::InsufficientData`] when no candidate can be fitted with a
///   defined AICc.
pub fn select_ets(y: &[f64], options: &SelectionOptions) -> Result<EtsSelection> {
    if y.iter().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "time series" });
    }
    if options.period == 0 {
        return Err(StatError::InvalidArgument { message: "the seasonal period must be at least 1".into() });
    }
    let mut candidates = Vec::new();
    let mut best: Option<EtsFit> = None;
    for spec in candidate_models(y, options) {
        let mut fit_options = EtsOptions::new(options.period);
        fit_options.bounds = options.bounds;
        let fit = fit_ets(y, spec, &fit_options).ok();
        let aicc = fit.as_ref().and_then(|f| f.aicc);
        candidates.push(EtsCandidate { spec, aicc });
        if let (Some(fit), Some(a)) = (fit, aicc) {
            if best.as_ref().and_then(|b| b.aicc).is_none_or(|b| a < b) {
                best = Some(fit);
            }
        }
    }
    let best = best.ok_or(StatError::InsufficientData { required: 4, found: y.len() })?;
    Ok(EtsSelection { best, candidates })
}
