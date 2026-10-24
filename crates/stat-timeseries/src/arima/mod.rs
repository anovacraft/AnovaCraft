//! Seasonal ARIMA models by exact maximum likelihood, forecasts with
//! prediction intervals, and automatic order selection.
//!
//! # Model
//!
//! A seasonal ARIMA(p,d,q)(P,D,Q)ₘ model with regression on columns `xₜ`
//! (regression with ARIMA errors) is
//!
//! ```text
//! yₜ = xₜᵀβ + uₜ,
//! φ(B) Φ(Bᵐ) (1 − B)^d (1 − Bᵐ)^D uₜ = θ(B) Θ(Bᵐ) εₜ,   εₜ ~ N(0, σ²) independent,
//! φ(B) = 1 − φ₁B − … − φₚBᵖ,       Φ(Bᵐ) = 1 − Φ₁Bᵐ − … − Φ_P B^{Pm},
//! θ(B) = 1 + θ₁B + … + θ_qB^q,     Θ(Bᵐ) = 1 + Θ₁Bᵐ + … + Θ_Q B^{Qm}
//! ```
//!
//! (Box, Jenkins, Reinsel & Ljung, *Time Series Analysis: Forecasting and
//! Control*, 5th ed., 2015, ch. 9). The optional constant is a regression
//! column: the mean (a column of ones) when `d + D = 0`, and the drift (the
//! time index `t = 1, …, n`) when `d + D = 1`. With `d + D ≥ 2` no constant
//! is allowed, since a polynomial trend of that degree is better expressed by
//! differencing.
//!
//! # Likelihood
//!
//! The differenced residual series `wₜ = Δ(B)(yₜ − xₜᵀβ)`, `t = d + mD, …, n−1`,
//! is a stationary ARMA process with expanded operators `φ*(B) = φ(B)Φ(Bᵐ)` and
//! `θ*(B) = θ(B)Θ(Bᵐ)`. Its exact Gaussian likelihood is computed by the
//! Kalman filter on the state-space form of Harvey (1989, *Forecasting,
//! Structural Time Series Models and the Kalman Filter*, §3.4) and Gardner,
//! Harvey & Phillips (1980, *Applied Statistics* 29, 311–322), started from
//! the stationary state covariance (the solution of the Lyapunov equation
//! `Q₀ = T Q₀ Tᵀ + R Rᵀ`; Jones 1980, *Technometrics* 22, 389–395). With
//! innovations `vₜ` and their variances `σ² Fₜ`, the variance σ² is
//! concentrated out:
//!
//! ```text
//! σ̂² = (1/ν) Σ vₜ² / Fₜ,    ℓ = −½ [ν ln(2π σ̂²) + Σ ln Fₜ + ν],   ν = n − d − mD.
//! ```
//!
//! This is the exact likelihood of the differenced data, i.e. the limit of a
//! diffuse prior on the `d + mD` starting values; software that uses a large
//! but finite prior variance (for example 10⁶) agrees to about the ratio of
//! the innovation variance to that prior variance.
//!
//! # Estimation
//!
//! Starting values come from the conditional sum of squares (residuals
//! computed with pre-sample values set to zero; Box et al. 2015, §7.1),
//! minimised from zero ARMA coefficients and the least-squares regression on
//! the differenced data. Exact maximum likelihood then starts from the
//! conditional estimates. The autoregressive operators are kept stationary by
//! optimising their partial autocorrelations `rₖ = tanh(uₖ)` (Jones 1980;
//! Barndorff-Nielsen & Schou 1973, *J. Multivariate Analysis* 3); a fitted
//! moving-average operator with roots inside the unit circle is replaced by
//! its invertible counterpart (roots `z ↦ 1/z̄`), which has the same
//! likelihood. The minimiser is BFGS with central-difference gradients.
//! Standard errors are the square roots of the diagonal of the inverse
//! numerical Hessian of `−ℓ` in the natural parameters.
//!
//! # Forecasts
//!
//! From the filtered state at the end of the series, the model is propagated
//! in the augmented state `(αₜ, u_{t−1}, …, u_{t−d−mD})`, so forecasts of the
//! levels and their mean squared errors include the integration of the
//! differences and the uncertainty of the final ARMA state (Harvey 1989, §3.5;
//! Brockwell & Davis, *Introduction to Time Series and Forecasting*, 2016,
//! §6.4). Regression coefficients are treated as known. Prediction intervals
//! are `ŷ ± z_{(1+level)/2} · se`.
//!
//! # Transfer-function inputs
//!
//! Inputs with a rational distributed lag `ω(B)B^b/δ(B)` ([`TransferInput`];
//! Box et al. 2015, ch. 11–12) are added to the regression part, so the
//! likelihood above is that of the transfer-function–noise model
//! `yₜ = xₜᵀβ + Σ vᵢ,ₜ + Nₜ`. Their numerator weights are estimated with the
//! regression coefficients (started from the least-squares distributed-lag
//! regression), their denominators are kept stable by the partial-
//! autocorrelation transformation; see [`transfer`] for the filter and its
//! start-up convention. Forecasts need the future values of each input up to
//! `h − b` steps ahead and treat them as known.
//!
//! # Automatic order selection
//!
//! [`auto_arima`] follows the stepwise algorithm of Hyndman & Khandakar
//! (2008, *Journal of Statistical Software* 27(3)); see its documentation.

mod auto;
mod optim;
mod polynomial;
mod statespace;
pub mod transfer;

pub use auto::{
    AutoArimaOptions, AutoArimaResult, CandidateModel, auto_arima, auto_arima_transfer, kpss_differencing_order,
    seasonal_differencing_order,
};
pub use transfer::{
    ImpulseResponseEstimate, TransferEstimate, TransferInput, impulse_response_estimate, steady_state_gain,
};

use crate::{Result, StatError};
use optim::{bfgs, hessian, invert_spd};
use polynomial::{
    ar_to_partial, ar_to_unconstrained, difference, differencing_operator, expand_ar, expand_ma, make_invertible,
    min_root_modulus, unconstrained_to_ar,
};
use stat_core::distributions::{ContinuousDistribution, Normal};
use stat_core::{Matrix, QrDecomposition};
use statespace::{conditional_sum_of_squares, kalman_filter};
use std::ops::Range;
use transfer::{InputData, filter_input};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Orders of a seasonal ARIMA(p,d,q)(P,D,Q)ₘ model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArimaSpec {
    /// Regular autoregressive order `p`.
    pub p: usize,
    /// Regular differencing order `d`.
    pub d: usize,
    /// Regular moving-average order `q`.
    pub q: usize,
    /// Seasonal autoregressive order `P`.
    pub seasonal_p: usize,
    /// Seasonal differencing order `D`.
    pub seasonal_d: usize,
    /// Seasonal moving-average order `Q`.
    pub seasonal_q: usize,
    /// Seasonal period `m` (1 for a non-seasonal model).
    pub period: usize,
    /// Include a constant: the mean when `d + D = 0`, the drift (slope on the
    /// time index) when `d + D = 1`. Not allowed when `d + D ≥ 2`.
    pub constant: bool,
}

impl ArimaSpec {
    /// Non-seasonal ARIMA(p,d,q) without a constant.
    #[must_use]
    pub const fn new(p: usize, d: usize, q: usize) -> Self {
        Self { p, d, q, seasonal_p: 0, seasonal_d: 0, seasonal_q: 0, period: 1, constant: false }
    }

    /// Add the seasonal part (P,D,Q) with period `m`.
    #[must_use]
    pub const fn seasonal(mut self, p: usize, d: usize, q: usize, period: usize) -> Self {
        self.seasonal_p = p;
        self.seasonal_d = d;
        self.seasonal_q = q;
        self.period = period;
        self
    }

    /// Include (or not) the constant term.
    #[must_use]
    pub const fn with_constant(mut self, constant: bool) -> Self {
        self.constant = constant;
        self
    }

    /// Total differencing lag `d + m·D` (observations lost to differencing).
    #[must_use]
    pub const fn differencing_lag(&self) -> usize {
        self.d.saturating_add(self.period.saturating_mul(self.seasonal_d))
    }

    /// Whether the model has a seasonal part.
    #[must_use]
    pub const fn is_seasonal(&self) -> bool {
        self.seasonal_p + self.seasonal_d + self.seasonal_q > 0
    }

    /// Number of ARMA coefficients `p + q + P + Q`.
    #[must_use]
    pub const fn arma_count(&self) -> usize {
        self.p + self.q + self.seasonal_p + self.seasonal_q
    }

    /// A label such as `ARIMA(0,1,1)(0,1,1)[12]` or `ARIMA(1,1,0) with drift`.
    #[must_use]
    pub fn label(&self) -> String {
        let mut s = format!("ARIMA({},{},{})", self.p, self.d, self.q);
        if self.is_seasonal() {
            s.push_str(&format!("({},{},{})[{}]", self.seasonal_p, self.seasonal_d, self.seasonal_q, self.period));
        }
        if self.constant {
            s.push_str(if self.d + self.seasonal_d == 0 { " with mean" } else { " with drift" });
        }
        s
    }

    fn validate(&self) -> Result<()> {
        if self.period == 0 {
            return Err(invalid("the seasonal period must be at least 1"));
        }
        if self.is_seasonal() && self.period < 2 {
            return Err(invalid("a seasonal ARIMA part needs a seasonal period of at least 2"));
        }
        if self.constant && self.d + self.seasonal_d > 1 {
            return Err(invalid(
                "a constant is allowed only when the total differencing order d + D is 0 (mean) or 1 (drift)",
            ));
        }
        Ok(())
    }
}

/// A regression column for regression with ARIMA errors.
#[derive(Debug, Clone, PartialEq)]
pub struct Regressor {
    /// Name used in the coefficient table.
    pub name: String,
    /// One value per observation.
    pub values: Vec<f64>,
}

/// How the coefficients are estimated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EstimationMethod {
    /// Conditional sum of squares for starting values, then exact maximum
    /// likelihood (the default).
    #[default]
    ConditionalThenExact,
    /// Exact maximum likelihood from zero ARMA coefficients.
    Exact,
}

/// Options for [`fit_arima`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArimaOptions {
    /// Estimation method.
    pub method: EstimationMethod,
    /// Maximum BFGS iterations for each optimisation stage.
    pub max_iterations: usize,
}

impl Default for ArimaOptions {
    fn default() -> Self {
        Self { method: EstimationMethod::default(), max_iterations: 500 }
    }
}

/// A fitted (or evaluated) ARIMA model.
#[derive(Debug, Clone)]
pub struct ArimaFit {
    /// Model orders.
    pub spec: ArimaSpec,
    /// Regular AR coefficients `φ₁…φₚ`.
    pub ar: Vec<f64>,
    /// Regular MA coefficients `θ₁…θ_q`.
    pub ma: Vec<f64>,
    /// Seasonal AR coefficients `Φ₁…Φ_P`.
    pub seasonal_ar: Vec<f64>,
    /// Seasonal MA coefficients `Θ₁…Θ_Q`.
    pub seasonal_ma: Vec<f64>,
    /// Regression coefficients: the constant (if any) first, then the regressors.
    pub regression: Vec<f64>,
    /// Fitted transfer functions, one per input (empty without inputs).
    pub transfer: Vec<TransferEstimate>,
    /// Names of all coefficients, in the order `ar1…, ma1…, sar1…, sma1…,
    /// intercept | drift, regressors`, then each input's `ω0…, δ1…`.
    pub coefficient_names: Vec<String>,
    /// All coefficients in that order.
    pub coefficients: Vec<f64>,
    /// Their standard errors (NaN when the Hessian is not positive definite or
    /// the coefficients were supplied rather than estimated).
    pub std_errors: Vec<f64>,
    /// Covariance matrix of the coefficients (row-major), when available.
    pub covariance: Option<Vec<f64>>,
    /// Maximum likelihood estimate of the innovation variance σ².
    pub sigma2: f64,
    /// Exact Gaussian log-likelihood ℓ.
    pub log_likelihood: f64,
    /// `AIC = −2ℓ + 2k`, with `k` the number of coefficients plus one for σ².
    pub aic: f64,
    /// `AICc = AIC + 2k(k+1)/(ν − k − 1)` (Hurvich & Tsai 1989), ∞ when `ν ≤ k + 1`.
    pub aicc: f64,
    /// `BIC = −2ℓ + k ln ν`.
    pub bic: f64,
    /// Number of observations in the likelihood, `ν = n − d − mD`.
    pub n_used: usize,
    /// Residuals: innovations scaled to the steady-state variance,
    /// `vₜ / √Fₜ`, so their variance is about σ². NaN for the first
    /// `d + mD` observations, which have no innovation.
    pub residuals: Vec<f64>,
    /// One-step predictions `ŷₜ|ₜ₋₁ = yₜ − vₜ` (NaN for the first `d + mD`).
    pub fitted: Vec<f64>,
    /// Whether the optimiser met its convergence criterion (true for
    /// evaluated models).
    pub converged: bool,
    state: ForecastState,
}

#[derive(Debug, Clone)]
struct ForecastState {
    n: usize,
    phi: Vec<f64>,
    theta: Vec<f64>,
    delta: Vec<f64>,
    a: Vec<f64>,
    p: Vec<f64>,
    /// `u_{n−1}, …, u_{n−nd}`.
    lags: Vec<f64>,
    /// Kind of constant column, if any.
    constant: Option<ConstantKind>,
    regressors: usize,
    /// Per input: centred history `x̃`, filtered history `v`, delay, ω, δ, mean.
    inputs: Vec<InputState>,
}

#[derive(Debug, Clone)]
struct InputState {
    xc: Vec<f64>,
    mean: f64,
    delay: usize,
    omega: Vec<f64>,
    delta: Vec<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConstantKind {
    Mean,
    Drift,
}

/// Point forecasts with standard errors and prediction intervals.
#[derive(Debug, Clone, PartialEq)]
pub struct ArimaForecast {
    /// Forecast `ŷ_{n+h}`, `h = 1…H`.
    pub mean: Vec<f64>,
    /// Forecast standard error (root mean squared error given the model).
    pub std_error: Vec<f64>,
    /// One interval per requested level.
    pub intervals: Vec<PredictionInterval>,
}

/// Prediction interval `ŷ ± z_{(1+level)/2} · se`.
#[derive(Debug, Clone, PartialEq)]
pub struct PredictionInterval {
    /// Coverage level in (0, 1).
    pub level: f64,
    /// Lower limits.
    pub lower: Vec<f64>,
    /// Upper limits.
    pub upper: Vec<f64>,
}

/// The data and layout of one estimation problem.
struct Problem {
    spec: ArimaSpec,
    y: Vec<f64>,
    /// Regression columns (constant first).
    x: Vec<Vec<f64>>,
    names: Vec<String>,
    constant: Option<ConstantKind>,
    delta: Vec<f64>,
    wy: Vec<f64>,
    wx: Vec<Vec<f64>>,
    inputs: Vec<InputData>,
    /// Coefficient ranges kept stationary: ar, sar and each input's δ.
    ar_groups: Vec<Range<usize>>,
}

/// Natural coefficients split by role.
struct Coefs<'a> {
    ar: &'a [f64],
    ma: &'a [f64],
    sar: &'a [f64],
    sma: &'a [f64],
    beta: &'a [f64],
    tf: &'a [f64],
}

impl Problem {
    fn new(y: &[f64], spec: &ArimaSpec, xreg: &[Regressor], inputs: &[TransferInput]) -> Result<Self> {
        spec.validate()?;
        if y.iter().any(|v| !v.is_finite()) {
            return Err(StatError::NonFiniteInput { context: "time series" });
        }
        let n = y.len();
        let mut x = Vec::new();
        let mut names = Vec::new();
        let mut constant = None;
        if spec.constant {
            if spec.d + spec.seasonal_d == 0 {
                x.push(vec![1.0; n]);
                names.push("intercept".to_string());
                constant = Some(ConstantKind::Mean);
            } else {
                x.push((1..=n).map(|t| t as f64).collect());
                names.push("drift".to_string());
                constant = Some(ConstantKind::Drift);
            }
        }
        for r in xreg {
            if r.values.len() != n {
                return Err(StatError::DimensionMismatch {
                    context: "regressor length vs. series length",
                    expected: n,
                    found: r.values.len(),
                });
            }
            if r.values.iter().any(|v| !v.is_finite()) {
                return Err(StatError::NonFiniteInput { context: "regressor" });
            }
            x.push(r.values.clone());
            names.push(r.name.clone());
        }
        let inputs = inputs.iter().map(|i| InputData::new(i, n)).collect::<Result<Vec<_>>>()?;
        // The longest lag of any operator, in saturating arithmetic, so that a
        // huge period is refused before its polynomial is allocated.
        let longest = [(spec.d, spec.seasonal_d), (spec.p, spec.seasonal_p), (spec.q, spec.seasonal_q)]
            .iter()
            .map(|&(regular, seasonal)| regular.saturating_add(spec.period.saturating_mul(seasonal)))
            .max()
            .unwrap_or(0);
        if longest >= n {
            return Err(StatError::InsufficientData { required: longest.saturating_add(1), found: n });
        }
        let delta = differencing_operator(spec.d, spec.seasonal_d, spec.period);
        let nd = delta.len();
        let k = spec.arma_count() + x.len() + inputs.iter().map(InputData::n_par).sum::<usize>();
        let required = nd + k + 2;
        if n < required {
            return Err(StatError::InsufficientData { required, found: n });
        }
        let wy = difference(y, &delta);
        let wx: Vec<Vec<f64>> = x.iter().map(|c| difference(c, &delta)).collect();
        for ((c, orig), name) in wx.iter().zip(&x).zip(&names) {
            let scale = c.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
            let orig = orig.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
            if scale <= 1e-12 * orig.max(1e-300) {
                return Err(invalid(format!("regressor '{name}' is removed entirely by the differencing")));
            }
        }
        let sar = spec.p + spec.q;
        let mut ar_groups: Vec<Range<usize>> = Vec::with_capacity(2 + inputs.len());
        ar_groups.push(0..spec.p);
        ar_groups.push(sar..sar + spec.seasonal_p);
        let mut at = spec.arma_count() + x.len();
        for i in &inputs {
            at += i.numerator + 1;
            ar_groups.push(at..at + i.denominator);
            at += i.denominator;
        }
        Ok(Self { spec: *spec, y: y.to_vec(), x, names, constant, delta, wy, wx, inputs, ar_groups })
    }

    fn n_arma(&self) -> usize {
        self.spec.arma_count()
    }

    fn n_coef(&self) -> usize {
        self.n_arma() + self.x.len() + self.inputs.iter().map(InputData::n_par).sum::<usize>()
    }

    /// Each input's (ω, δ) from the transfer part of the coefficients.
    fn input_parts<'a>(&self, tf: &'a [f64]) -> Vec<(&'a [f64], &'a [f64])> {
        let mut rest = tf;
        self.inputs
            .iter()
            .map(|i| {
                let (mine, next) = rest.split_at(i.n_par());
                rest = next;
                i.split(mine)
            })
            .collect()
    }

    /// Each input's filtered series `vₜ`.
    fn input_effects(&self, tf: &[f64]) -> Vec<Vec<f64>> {
        self.inputs
            .iter()
            .zip(self.input_parts(tf))
            .map(|(i, (omega, delta))| filter_input(&i.xc, i.delay, omega, delta))
            .collect()
    }

    /// Whether every operator kept stationary (AR, seasonal AR, each δ) is.
    fn stationary(&self, coefs: &[f64]) -> bool {
        self.ar_groups.iter().all(|g| ar_to_partial(&coefs[g.clone()]).is_some())
    }

    fn split<'a>(&self, c: &'a [f64]) -> Coefs<'a> {
        let s = &self.spec;
        let (ar, rest) = c.split_at(s.p);
        let (ma, rest) = rest.split_at(s.q);
        let (sar, rest) = rest.split_at(s.seasonal_p);
        let (sma, rest) = rest.split_at(s.seasonal_q);
        let (beta, tf) = rest.split_at(self.x.len());
        Coefs { ar, ma, sar, sma, beta, tf }
    }

    fn expanded(&self, c: &Coefs<'_>) -> (Vec<f64>, Vec<f64>) {
        (expand_ar(c.ar, c.sar, self.spec.period), expand_ma(c.ma, c.sma, self.spec.period))
    }

    fn w(&self, beta: &[f64], tf: &[f64]) -> Vec<f64> {
        let mut w = self.wy.clone();
        for (col, &b) in self.wx.iter().zip(beta) {
            for (wi, ci) in w.iter_mut().zip(col) {
                *wi -= b * ci;
            }
        }
        if !self.inputs.is_empty() {
            let mut total = vec![0.0; self.y.len()];
            for e in self.input_effects(tf) {
                for (t, v) in total.iter_mut().zip(e) {
                    *t += v;
                }
            }
            for (wi, di) in w.iter_mut().zip(difference(&total, &self.delta)) {
                *wi -= di;
            }
        }
        w
    }

    /// ½ ln σ̂² + ½ Σ ln Fₜ / ν at natural coefficients (∞ when not stationary).
    fn ml_objective(&self, coefs: &[f64]) -> f64 {
        if !self.stationary(coefs) {
            return f64::INFINITY;
        }
        let c = self.split(coefs);
        let (phi, theta) = self.expanded(&c);
        let w = self.w(c.beta, c.tf);
        match kalman_filter(&w, &phi, &theta, false) {
            Some(f) if f.ssq > 0.0 => 0.5 * (f.ssq / f.nu as f64).ln() + 0.5 * f.sumlog / f.nu as f64,
            _ => f64::INFINITY,
        }
    }

    /// Whether the model at `coefs` leaves no residual variation (an exactly
    /// deterministic series), which makes the likelihood unbounded.
    fn fits_exactly(&self, coefs: &[f64]) -> bool {
        if !self.stationary(coefs) {
            return false;
        }
        let c = self.split(coefs);
        let (phi, theta) = self.expanded(&c);
        let w = self.w(c.beta, c.tf);
        let scale = self.y.iter().fold(0.0_f64, |m, v| m.max(v.abs())).max(1.0);
        kalman_filter(&w, &phi, &theta, false).is_some_and(|f| f.ssq <= 1e-24 * scale * scale * f.nu as f64)
    }

    fn css_objective(&self, coefs: &[f64]) -> f64 {
        let c = self.split(coefs);
        let (phi, theta) = self.expanded(&c);
        let w = self.w(c.beta, c.tf);
        let (ssq, count) = conditional_sum_of_squares(&w, &phi, &theta);
        if count == 0 || !ssq.is_finite() || ssq <= 0.0 { f64::INFINITY } else { 0.5 * (ssq / count as f64).ln() }
    }

    /// Starting point and scales of every coefficient: zero ARMA and
    /// denominator coefficients (scale 1), and the least-squares regression of
    /// the differenced response on the differenced columns and the lagged
    /// inputs `x̃_{t−b−j}`, `j = 0…s` (a distributed-lag regression), for the
    /// regression and numerator coefficients, with their standard errors as
    /// scales.
    fn start_values(&self) -> Result<(Vec<f64>, Vec<f64>)> {
        let na = self.n_arma();
        let mut cols: Vec<Vec<f64>> = self.wx.clone();
        for i in &self.inputs {
            for j in 0..=i.numerator {
                cols.push(difference(&i.lagged(i.delay + j), &self.delta));
            }
        }
        let k = cols.len();
        let (beta, scales) = if k == 0 {
            (Vec::new(), Vec::new())
        } else {
            let m = self.wy.len();
            let a = Matrix::from_columns(&cols)?;
            let qr = QrDecomposition::new(a)?;
            let rank = qr.rank(qr.default_rank_tolerance());
            if rank < k {
                return Err(StatError::RankDeficient { rank, columns: k });
            }
            let beta = qr.solve_least_squares(&self.wy)?;
            let rss: f64 = (0..m)
                .map(|t| {
                    let fit: f64 = cols.iter().zip(&beta).map(|(c, b)| c[t] * b).sum();
                    (self.wy[t] - fit).powi(2)
                })
                .sum();
            let s2 = if m > k { rss / (m - k) as f64 } else { 1.0 };
            let g = qr.inverse_gram()?;
            let scales: Vec<f64> = (0..k)
                .map(|j| {
                    let s = (s2 * g[(j, j)]).sqrt();
                    if s.is_finite() && s > 0.0 { s } else { beta[j].abs().max(1.0) }
                })
                .collect();
            (beta, scales)
        };
        let mut offset = vec![0.0; na];
        let mut scale = vec![1.0; na];
        let nx = self.wx.len();
        offset.extend_from_slice(&beta[..nx]);
        scale.extend_from_slice(&scales[..nx]);
        let mut at = nx;
        for i in &self.inputs {
            for j in 0..=i.numerator {
                // ω(B) = ω₀ − ω₁B − …: the lag-j regression weight is −ω_j for j ≥ 1.
                offset.push(if j == 0 { beta[at] } else { -beta[at] });
                scale.push(scales[at]);
                at += 1;
            }
            offset.extend(std::iter::repeat_n(0.0, i.denominator));
            scale.extend(std::iter::repeat_n(1.0, i.denominator));
        }
        Ok((offset, scale))
    }
}

/// Mapping between the optimiser's unconstrained vector and the natural
/// coefficients: linear coefficients as `c = offset + scale ⊙ z` (identity
/// for the ARMA and denominator coefficients), and, when `transform` is set,
/// each stationary operator (AR, seasonal AR, input denominators) through its
/// partial autocorrelations.
struct Param<'a> {
    problem: &'a Problem,
    offset: Vec<f64>,
    scale: Vec<f64>,
    transform: bool,
}

impl Param<'_> {
    fn unconstrained_to_natural(&self, x: &[f64]) -> Vec<f64> {
        let mut out: Vec<f64> = x.iter().zip(&self.offset).zip(&self.scale).map(|((z, o), s)| o + s * z).collect();
        if self.transform {
            for g in &self.problem.ar_groups {
                out[g.clone()].copy_from_slice(&unconstrained_to_ar(&x[g.clone()]));
            }
        }
        out
    }

    /// Inverse of [`Self::unconstrained_to_natural`]; `None` when a stationary operator is not.
    fn natural_to_unconstrained(&self, c: &[f64]) -> Option<Vec<f64>> {
        let mut out: Vec<f64> = c.iter().zip(&self.offset).zip(&self.scale).map(|((v, o), s)| (v - o) / s).collect();
        if self.transform {
            for g in &self.problem.ar_groups {
                out[g.clone()].copy_from_slice(&ar_to_unconstrained(&c[g.clone()])?);
            }
        }
        Some(out)
    }
}

/// Replace non-invertible regular or seasonal MA parts by their invertible
/// counterparts. Returns whether anything changed.
fn invert_ma_parts(problem: &Problem, coefs: &mut [f64]) -> bool {
    let s = problem.spec;
    let mut changed = false;
    let start = s.p;
    let (ma, c1) = make_invertible(&coefs[start..start + s.q]);
    coefs[start..start + s.q].copy_from_slice(&ma);
    changed |= c1;
    let start = s.p + s.q + s.seasonal_p;
    let (sma, c2) = make_invertible(&coefs[start..start + s.seasonal_q]);
    coefs[start..start + s.seasonal_q].copy_from_slice(&sma);
    changed |= c2;
    changed
}

/// Fit a seasonal ARIMA model with optional regressors to `y` (equally spaced,
/// no missing values) by exact maximum likelihood.
///
/// # Errors
/// * [`StatError::NonFiniteInput`] for NaN or ±∞ in `y` or a regressor.
/// * [`StatError::DimensionMismatch`] when a regressor's length differs from `y`.
/// * [`StatError::InsufficientData`] when `n < d + mD + (number of coefficients) + 2`.
/// * [`StatError::InvalidArgument`] for an invalid specification (a constant
///   with `d + D ≥ 2`, seasonal orders with period < 2) or a regressor that the
///   differencing removes (such as a constant column with `d ≥ 1`).
/// * [`StatError::RankDeficient`] for collinear regressors.
/// * [`StatError::InvalidArgument`] when the model fits the series exactly
///   (for example `(0,1,0)` on constant data), so the likelihood is unbounded.
/// * [`StatError::ConvergenceFailure`] when no finite likelihood is found.
pub fn fit_arima(y: &[f64], spec: &ArimaSpec, xreg: &[Regressor], options: &ArimaOptions) -> Result<ArimaFit> {
    fit_arima_transfer(y, spec, xreg, &[], options)
}

/// Fit a transfer-function–noise model: [`fit_arima`] with input series
/// entering through rational distributed lags (see [`transfer`]).
///
/// # Errors
/// As [`fit_arima`]; also for an input of the wrong length, with non-finite
/// values, constant, or with a delay plus numerator order at least the series
/// length, and [`StatError::RankDeficient`] when the lagged inputs are collinear.
pub fn fit_arima_transfer(
    y: &[f64],
    spec: &ArimaSpec,
    xreg: &[Regressor],
    inputs: &[TransferInput],
    options: &ArimaOptions,
) -> Result<ArimaFit> {
    let problem = Problem::new(y, spec, xreg, inputs)?;
    let (offset, scale) = problem.start_values()?;
    let na = problem.n_arma();
    let k = problem.n_coef();
    let mut start_natural: Vec<f64> = offset.clone();
    let mut iterations_used = 0;
    if options.method == EstimationMethod::ConditionalThenExact && k > 0 {
        let raw = Param { problem: &problem, offset: offset.clone(), scale: scale.clone(), transform: false };
        let mut f = |x: &[f64]| problem.css_objective(&raw.unconstrained_to_natural(x));
        let x0 = vec![0.0; k];
        let m = bfgs(&mut f, &x0, options.max_iterations, 1e-8);
        iterations_used += m.iterations;
        let css = raw.unconstrained_to_natural(&m.x);
        if css.iter().all(|v| v.is_finite()) {
            let mut candidate = css.clone();
            if !problem.stationary(&css) {
                // Keep the regression and numerator parts; restart the ARMA
                // part and the denominators at zero.
                for v in &mut candidate[..na] {
                    *v = 0.0;
                }
                for g in &problem.ar_groups {
                    for v in &mut candidate[g.clone()] {
                        *v = 0.0;
                    }
                }
            } else {
                invert_ma_parts(&problem, &mut candidate);
            }
            start_natural = candidate;
        }
    }
    let param = Param { problem: &problem, offset: offset.clone(), scale: scale.clone(), transform: true };
    let x0 = param.natural_to_unconstrained(&start_natural).unwrap_or_else(|| vec![0.0; k]);
    let mut f = |x: &[f64]| problem.ml_objective(&param.unconstrained_to_natural(x));
    let mut m = bfgs(&mut f, &x0, options.max_iterations, 1e-8);
    iterations_used += m.iterations;
    if !m.value.is_finite() {
        if problem.fits_exactly(&param.unconstrained_to_natural(&m.x)) || problem.fits_exactly(&start_natural) {
            return Err(invalid(
                "the model fits the series exactly (no residual variation is left), so its likelihood has no \
                 maximum; the series may be constant or follow an exact pattern",
            ));
        }
        return Err(StatError::ConvergenceFailure { routine: "ARIMA maximum likelihood", iterations: iterations_used });
    }
    let mut coefs = param.unconstrained_to_natural(&m.x);
    if invert_ma_parts(&problem, &mut coefs) {
        if let Some(x1) = param.natural_to_unconstrained(&coefs) {
            let m2 = bfgs(&mut f, &x1, options.max_iterations, 1e-8);
            let mut c2 = param.unconstrained_to_natural(&m2.x);
            invert_ma_parts(&problem, &mut c2);
            if m2.value.is_finite() && m2.value <= m.value + 1e-10 {
                coefs = c2;
                m = m2;
            }
        }
    }
    // Hessian of −ℓ in natural ARMA and denominator coefficients and scaled
    // regression and numerator units.
    let nu = (problem.wy.len()) as f64;
    let raw = Param { problem: &problem, offset, scale: scale.clone(), transform: false };
    let hpoint = raw.natural_to_unconstrained(&coefs).unwrap_or_else(|| vec![0.0; k]);
    let mut negll = |h: &[f64]| nu * problem.ml_objective(&raw.unconstrained_to_natural(h));
    let steps: Vec<f64> = vec![1e-4; k];
    let covariance = hessian(&mut negll, &hpoint, &steps).and_then(|h| invert_spd(&h, k)).map(|mut cov| {
        for i in 0..k {
            for j in 0..k {
                cov[i * k + j] *= scale[i] * scale[j];
            }
        }
        cov
    });
    let mut fit = assemble(&problem, &coefs, covariance)?;
    fit.converged = m.converged;
    Ok(fit)
}

/// Evaluate a model at given coefficients (no estimation): exact
/// log-likelihood, σ̂², information criteria, residuals and the forecast state.
/// `coefficients` are in the order of [`ArimaFit::coefficient_names`]
/// (`ar, ma, sar, sma, constant, regressors`). Standard errors are NaN.
///
/// # Errors
/// As [`fit_arima`]; also [`StatError::DimensionMismatch`] for a wrong number
/// of coefficients and [`StatError::InvalidArgument`] for a non-stationary AR part.
pub fn evaluate_arima(y: &[f64], spec: &ArimaSpec, xreg: &[Regressor], coefficients: &[f64]) -> Result<ArimaFit> {
    evaluate_arima_transfer(y, spec, xreg, &[], coefficients)
}

/// [`evaluate_arima`] for a transfer-function–noise model; each input's
/// `ω0…, δ1…` follow the regression coefficients.
///
/// # Errors
/// As [`evaluate_arima`] and [`fit_arima_transfer`]; also an unstable denominator.
pub fn evaluate_arima_transfer(
    y: &[f64],
    spec: &ArimaSpec,
    xreg: &[Regressor],
    inputs: &[TransferInput],
    coefficients: &[f64],
) -> Result<ArimaFit> {
    let problem = Problem::new(y, spec, xreg, inputs)?;
    if coefficients.len() != problem.n_coef() {
        return Err(StatError::DimensionMismatch {
            context: "number of ARIMA coefficients",
            expected: problem.n_coef(),
            found: coefficients.len(),
        });
    }
    if coefficients.iter().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "ARIMA coefficients" });
    }
    assemble(&problem, coefficients, None)
}

fn assemble(problem: &Problem, coefs: &[f64], covariance: Option<Vec<f64>>) -> Result<ArimaFit> {
    if !problem.stationary(coefs) {
        return Err(invalid("the autoregressive part or a transfer-function denominator is not stationary"));
    }
    let c = problem.split(coefs);
    let (phi, theta) = problem.expanded(&c);
    let w = problem.w(c.beta, c.tf);
    let effects = problem.input_effects(c.tf);
    let out = kalman_filter(&w, &phi, &theta, true)
        .ok_or_else(|| invalid("the model has no finite likelihood at these coefficients"))?;
    let nu = out.nu;
    let n = problem.y.len();
    let nd = problem.delta.len();
    let sigma2 = out.ssq / nu as f64;
    let ln2pi = (2.0 * std::f64::consts::PI).ln();
    let log_likelihood = -0.5 * (nu as f64 * (ln2pi + sigma2.ln()) + out.sumlog + nu as f64);
    let kpar = problem.n_coef() + 1;
    let aic = -2.0 * log_likelihood + 2.0 * kpar as f64;
    let aicc =
        if nu > kpar + 1 { aic + 2.0 * (kpar * (kpar + 1)) as f64 / (nu - kpar - 1) as f64 } else { f64::INFINITY };
    let bic = -2.0 * log_likelihood + kpar as f64 * (nu as f64).ln();
    let mut residuals = vec![f64::NAN; n];
    let mut fitted = vec![f64::NAN; n];
    for (i, (&v, &g)) in out.innovations.iter().zip(&out.gains).enumerate() {
        residuals[nd + i] = v / g.sqrt();
        fitted[nd + i] = problem.y[nd + i] - v;
    }
    let u: Vec<f64> = (0..n)
        .map(|t| {
            problem.y[t]
                - problem.x.iter().zip(c.beta).map(|(col, b)| col[t] * b).sum::<f64>()
                - effects.iter().map(|e| e[t]).sum::<f64>()
        })
        .collect();
    let lags: Vec<f64> = (0..nd).map(|j| u[n - 1 - j]).collect();
    let k = problem.n_coef();
    let std_errors = match &covariance {
        Some(cov) => (0..k).map(|i| cov[i * k + i].sqrt()).collect(),
        None => vec![f64::NAN; k],
    };
    let s = problem.spec;
    let mut names = Vec::with_capacity(k);
    names.extend((1..=s.p).map(|i| format!("ar{i}")));
    names.extend((1..=s.q).map(|i| format!("ma{i}")));
    names.extend((1..=s.seasonal_p).map(|i| format!("sar{i}")));
    names.extend((1..=s.seasonal_q).map(|i| format!("sma{i}")));
    names.extend(problem.names.iter().cloned());
    let parts = problem.input_parts(c.tf);
    for i in &problem.inputs {
        names.extend((0..=i.numerator).map(|j| format!("{} ω{j}", i.name)));
        names.extend((1..=i.denominator).map(|j| format!("{} δ{j}", i.name)));
    }
    let transfer: Vec<TransferEstimate> = problem
        .inputs
        .iter()
        .zip(&parts)
        .zip(effects)
        .map(|((i, (omega, delta)), effect)| TransferEstimate {
            name: i.name.clone(),
            delay: i.delay,
            omega: omega.to_vec(),
            delta: delta.to_vec(),
            input_mean: i.mean,
            effect,
        })
        .collect();
    let input_states: Vec<InputState> = problem
        .inputs
        .iter()
        .zip(&parts)
        .map(|(i, (omega, delta))| InputState {
            xc: i.xc.clone(),
            mean: i.mean,
            delay: i.delay,
            omega: omega.to_vec(),
            delta: delta.to_vec(),
        })
        .collect();
    Ok(ArimaFit {
        transfer,
        spec: s,
        ar: c.ar.to_vec(),
        ma: c.ma.to_vec(),
        seasonal_ar: c.sar.to_vec(),
        seasonal_ma: c.sma.to_vec(),
        regression: c.beta.to_vec(),
        coefficient_names: names,
        coefficients: coefs.to_vec(),
        std_errors,
        covariance,
        sigma2,
        log_likelihood,
        aic,
        aicc,
        bic,
        n_used: nu,
        residuals,
        fitted,
        converged: true,
        state: ForecastState {
            n,
            phi,
            theta,
            delta: problem.delta.clone(),
            a: out.state,
            p: out.state_cov,
            lags,
            constant: problem.constant,
            regressors: problem.x.len() - usize::from(problem.constant.is_some()),
            inputs: input_states,
        },
    })
}

impl ArimaFit {
    /// Number of estimated parameters `k` used by the information criteria
    /// (coefficients plus σ²).
    #[must_use]
    pub fn parameter_count(&self) -> usize {
        self.coefficients.len() + 1
    }

    /// Smallest modulus of the roots of the expanded AR and MA operators
    /// (∞ when there are none); values near 1 indicate a near-unit root.
    #[must_use]
    pub fn min_root_modulus(&self) -> f64 {
        min_root_modulus(&self.state.phi, -1.0).min(min_root_modulus(&self.state.theta, 1.0))
    }

    /// Forecast `horizon` steps ahead with prediction intervals at each of
    /// `levels` (e.g. `[0.8, 0.95]`). `future_xreg` gives the future values of
    /// each user regressor (in the order they were supplied), each at least
    /// `horizon` long; the mean and drift columns are extended automatically.
    ///
    /// # Errors
    /// * [`StatError::DimensionMismatch`] when the number of future regressors
    ///   differs from the model's, or one is shorter than `horizon`.
    /// * [`StatError::NonFiniteInput`] for non-finite future regressor values.
    /// * [`StatError::InvalidArgument`] for a level outside (0, 1).
    pub fn forecast(&self, horizon: usize, future_xreg: &[Vec<f64>], levels: &[f64]) -> Result<ArimaForecast> {
        self.forecast_with_inputs(horizon, future_xreg, &[], levels)
    }

    /// Number of future values of each transfer-function input that a
    /// forecast `horizon` steps ahead needs: `horizon − b` (zero when the
    /// delay covers the horizon).
    #[must_use]
    pub fn future_inputs_needed(&self, horizon: usize) -> Vec<usize> {
        self.state.inputs.iter().map(|i| horizon.saturating_sub(i.delay)).collect()
    }

    /// [`forecast`](Self::forecast) for a model with transfer-function
    /// inputs: `future_inputs` gives each input's values after the end of
    /// the series (at least [`future_inputs_needed`](Self::future_inputs_needed)
    /// of them), which are treated as known.
    ///
    /// # Errors
    /// As [`forecast`](Self::forecast); also [`StatError::DimensionMismatch`]
    /// when the number of inputs differs from the model's or one is too short,
    /// and [`StatError::NonFiniteInput`] for non-finite future input values.
    pub fn forecast_with_inputs(
        &self,
        horizon: usize,
        future_xreg: &[Vec<f64>],
        future_inputs: &[Vec<f64>],
        levels: &[f64],
    ) -> Result<ArimaForecast> {
        let st = &self.state;
        if future_inputs.len() != st.inputs.len() {
            return Err(StatError::DimensionMismatch {
                context: "future inputs vs. model inputs",
                expected: st.inputs.len(),
                found: future_inputs.len(),
            });
        }
        for (i, col) in st.inputs.iter().zip(future_inputs) {
            let need = horizon.saturating_sub(i.delay);
            if col.len() < need {
                return Err(StatError::DimensionMismatch {
                    context: "future input length vs. horizon minus delay",
                    expected: need,
                    found: col.len(),
                });
            }
            if col[..need].iter().any(|v| !v.is_finite()) {
                return Err(StatError::NonFiniteInput { context: "future input" });
            }
        }
        if future_xreg.len() != st.regressors {
            return Err(StatError::DimensionMismatch {
                context: "future regressors vs. model regressors",
                expected: st.regressors,
                found: future_xreg.len(),
            });
        }
        for col in future_xreg {
            if col.len() < horizon {
                return Err(StatError::DimensionMismatch {
                    context: "future regressor length vs. horizon",
                    expected: horizon,
                    found: col.len(),
                });
            }
            if col[..horizon].iter().any(|v| !v.is_finite()) {
                return Err(StatError::NonFiniteInput { context: "future regressor" });
            }
        }
        for &l in levels {
            if !(l > 0.0 && l < 1.0) {
                return Err(invalid(format!("prediction interval level must lie in (0, 1) (got {l})")));
            }
        }
        let (umean, uvar) = forecast_integrated(st, horizon);
        let mut mean = umean;
        for (h, m) in mean.iter_mut().enumerate() {
            let mut b = self.regression.iter();
            if let Some(kind) = st.constant {
                let beta = b.next().copied().unwrap_or(0.0);
                *m += match kind {
                    ConstantKind::Mean => beta,
                    ConstantKind::Drift => beta * (st.n + h + 1) as f64,
                };
            }
            for (col, beta) in future_xreg.iter().zip(b) {
                *m += beta * col[h];
            }
        }
        for (i, future) in st.inputs.iter().zip(future_inputs) {
            let need = horizon.saturating_sub(i.delay);
            let mut xc = i.xc.clone();
            xc.extend(future[..need].iter().map(|v| v - i.mean));
            xc.resize(st.n + horizon, 0.0);
            let v = filter_input(&xc, i.delay, &i.omega, &i.delta);
            for (m, vh) in mean.iter_mut().zip(&v[st.n..]) {
                *m += vh;
            }
        }
        let std_error: Vec<f64> = uvar.iter().map(|v| (v * self.sigma2).max(0.0).sqrt()).collect();
        let normal = Normal::standard();
        let intervals = levels
            .iter()
            .map(|&level| {
                let z = normal.quantile(0.5 + level / 2.0)?;
                Ok(PredictionInterval {
                    level,
                    lower: mean.iter().zip(&std_error).map(|(m, s)| m - z * s).collect(),
                    upper: mean.iter().zip(&std_error).map(|(m, s)| m + z * s).collect(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(ArimaForecast { mean, std_error, intervals })
    }
}

/// Forecasts of the regression error `u` and their variances (σ² units) by
/// propagating the augmented state `(α, u_{t−1}, …, u_{t−nd})`.
fn forecast_integrated(st: &ForecastState, horizon: usize) -> (Vec<f64>, Vec<f64>) {
    let r = st.a.len();
    let nd = st.delta.len();
    let s = r + nd;
    let mut phi = st.phi.clone();
    phi.resize(r, 0.0);
    let mut rvec = vec![1.0];
    rvec.extend_from_slice(&st.theta);
    rvec.resize(r, 0.0);
    // Transition matrix (row-major).
    let mut t = vec![0.0; s * s];
    for i in 0..r {
        t[i * s] = phi[i];
        if i + 1 < r {
            t[i * s + i + 1] = 1.0;
        }
    }
    if nd > 0 {
        t[r * s] = 1.0;
        for j in 0..nd {
            t[r * s + r + j] = st.delta[j];
        }
        for j in 1..nd {
            t[(r + j) * s + r + j - 1] = 1.0;
        }
    }
    let mut z = vec![0.0; s];
    z[0] = 1.0;
    z[r..].copy_from_slice(&st.delta);
    let mut a = st.a.clone();
    a.extend_from_slice(&st.lags);
    let mut p = vec![0.0; s * s];
    for i in 0..r {
        for j in 0..r {
            p[i * s + j] = st.p[i * r + j];
        }
    }
    let mut mean = Vec::with_capacity(horizon);
    let mut var = Vec::with_capacity(horizon);
    let mut tp = vec![0.0; s * s];
    for _ in 0..horizon {
        mean.push(z.iter().zip(&a).map(|(zi, ai)| zi * ai).sum());
        let mut v = 0.0;
        for i in 0..s {
            if z[i] == 0.0 {
                continue;
            }
            for j in 0..s {
                v += z[i] * p[i * s + j] * z[j];
            }
        }
        var.push(v);
        let an: Vec<f64> = (0..s).map(|i| (0..s).map(|j| t[i * s + j] * a[j]).sum()).collect();
        a = an;
        for i in 0..s {
            for j in 0..s {
                tp[i * s + j] = (0..s).filter(|&k| t[i * s + k] != 0.0).map(|k| t[i * s + k] * p[k * s + j]).sum();
            }
        }
        for i in 0..s {
            for j in 0..s {
                let mut v: f64 = (0..s).filter(|&k| t[j * s + k] != 0.0).map(|k| tp[i * s + k] * t[j * s + k]).sum();
                if i < r && j < r {
                    v += rvec[i] * rvec[j];
                }
                p[i * s + j] = v;
            }
        }
    }
    (mean, var)
}

#[cfg(test)]
mod tests;
