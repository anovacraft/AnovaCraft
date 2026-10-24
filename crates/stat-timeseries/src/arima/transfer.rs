//! Transfer-function inputs for ARIMA models (Box, Jenkins, Reinsel & Ljung,
//! *Time Series Analysis: Forecasting and Control*, 5th ed., 2015, ch. 11–12).
//!
//! # Model
//!
//! Each input series `xₜ` enters the response through a rational distributed
//! lag with delay `b`, numerator order `s` and denominator order `r`:
//!
//! ```text
//! yₜ = xₜᵀβ + Σᵢ vᵢ,ₜ + Nₜ,     vₜ = [ω(B) / δ(B)] B^b x̃ₜ,
//! ω(B) = ω₀ − ω₁B − … − ω_sB^s,  δ(B) = 1 − δ₁B − … − δ_rB^r,
//! ```
//!
//! where `Nₜ` is the seasonal ARIMA noise of the model and `x̃ₜ = xₜ − x̄` is
//! the input in deviations from its mean (BJR §12.2). The filtered input is
//! computed by the recursion
//!
//! ```text
//! vₜ = δ₁v_{t−1} + … + δ_rv_{t−r} + ω₀x̃_{t−b} − ω₁x̃_{t−b−1} − … − ω_sx̃_{t−b−s},
//! ```
//!
//! with pre-sample values of `x̃` and `v` set to zero, their unconditional
//! expectation (BJR §12.2.1). Centring the input keeps that start-up
//! transient small; the mean or drift of the model absorbs the level. The
//! denominator is kept stable (roots of `δ(B)` outside the unit circle) by
//! the same partial-autocorrelation transformation as the autoregressive
//! operators.
//!
//! The impulse response weights `v₀, v₁, …` of an input (the response of `y`
//! to a unit pulse in `x`) satisfy `δ(B) v(B) = ω(B) B^b`; the steady-state
//! gain (the eventual change in `y` per unit step in `x`) is
//! `g = ω(1)/δ(1) = (ω₀ − Σω_j) / (1 − Σδᵢ)`.
//!
//! # Identification
//!
//! [`impulse_response_estimate`] prewhitens the (suitably differenced) input
//! by an autoregression and filters the response with the same operator; the
//! cross-correlations `r_αβ(k)` of the two filtered series at lags
//! `k = 0, 1, …` (input leading) estimate the impulse response weights by
//! `v̂ₖ = r_αβ(k) s_β / s_α` (BJR §11.2.2). The delay `b` is the first lag
//! with a significant correlation; the decay pattern after it suggests `r`
//! and the number of irregular weights `s`.

use super::{Result, StatError, invalid};
use crate::autocorrelation::{PrewhitenedCrossCorrelation, prewhitened_cross_correlation};
use stat_core::distributions::{ContinuousDistribution, Normal};

/// An input series with its transfer-function structure.
#[derive(Debug, Clone, PartialEq)]
pub struct TransferInput {
    /// Name used in the coefficient table.
    pub name: String,
    /// One value per observation of the response.
    pub values: Vec<f64>,
    /// Delay `b`: the input first affects the response `b` steps later.
    pub delay: usize,
    /// Numerator order `s` (`s + 1` weights `ω₀…ω_s`).
    pub numerator: usize,
    /// Denominator order `r` (`r` weights `δ₁…δ_r`).
    pub denominator: usize,
}

impl TransferInput {
    /// An input with delay 0 and a single weight `ω₀` (a plain regression on
    /// the centred input).
    #[must_use]
    pub fn new(name: impl Into<String>, values: Vec<f64>) -> Self {
        Self { name: name.into(), values, delay: 0, numerator: 0, denominator: 0 }
    }

    /// Set the delay `b`, numerator order `s` and denominator order `r`.
    #[must_use]
    pub fn with_orders(mut self, delay: usize, numerator: usize, denominator: usize) -> Self {
        self.delay = delay;
        self.numerator = numerator;
        self.denominator = denominator;
        self
    }

    /// Number of transfer coefficients `s + 1 + r`.
    #[must_use]
    pub const fn parameter_count(&self) -> usize {
        self.numerator + 1 + self.denominator
    }

    /// Coefficient names `"<name> ω0" …, "<name> δ1" …`.
    #[must_use]
    pub fn coefficient_names(&self) -> Vec<String> {
        let mut out: Vec<String> = (0..=self.numerator).map(|j| format!("{} ω{j}", self.name)).collect();
        out.extend((1..=self.denominator).map(|i| format!("{} δ{i}", self.name)));
        out
    }
}

/// The fitted transfer function of one input.
#[derive(Debug, Clone, PartialEq)]
pub struct TransferEstimate {
    /// Input name.
    pub name: String,
    /// Delay `b`.
    pub delay: usize,
    /// Numerator weights `ω₀…ω_s` (sign convention of the module docs).
    pub omega: Vec<f64>,
    /// Denominator weights `δ₁…δ_r`.
    pub delta: Vec<f64>,
    /// Mean `x̄` subtracted from the input.
    pub input_mean: f64,
    /// The filtered input `vₜ` for each observation (the input's contribution
    /// to the response, in deviations from its mean effect).
    pub effect: Vec<f64>,
}

impl TransferEstimate {
    /// Impulse response weights `v₀ … v_{lags}`.
    #[must_use]
    pub fn impulse_response(&self, lags: usize) -> Vec<f64> {
        let mut pulse = vec![0.0; lags + 1];
        pulse[0] = 1.0;
        filter_input(&pulse, self.delay, &self.omega, &self.delta)
    }

    /// Steady-state gain `ω(1)/δ(1)` (NaN when `δ(1) = 0`).
    #[must_use]
    pub fn steady_state_gain(&self) -> f64 {
        steady_state_gain(&self.omega, &self.delta)
    }
}

/// `ω(1)/δ(1) = (ω₀ − Σω_j)/(1 − Σδᵢ)`; NaN when the denominator vanishes.
#[must_use]
pub fn steady_state_gain(omega: &[f64], delta: &[f64]) -> f64 {
    let num = omega.first().copied().unwrap_or(0.0) - omega.iter().skip(1).sum::<f64>();
    let den = 1.0 - delta.iter().sum::<f64>();
    if den == 0.0 { f64::NAN } else { num / den }
}

/// Filter `x̃` (pre-sample zeros) through `ω(B)B^b / δ(B)`; returns one value per element of `x̃`.
pub(crate) fn filter_input(xc: &[f64], delay: usize, omega: &[f64], delta: &[f64]) -> Vec<f64> {
    let n = xc.len();
    let mut v = vec![0.0; n];
    for t in 0..n {
        let mut s = 0.0;
        for (i, &d) in delta.iter().enumerate() {
            if t > i {
                s += d * v[t - 1 - i];
            }
        }
        for (j, &w) in omega.iter().enumerate() {
            let lag = delay + j;
            if t >= lag {
                let x = xc[t - lag];
                s += if j == 0 { w * x } else { -w * x };
            }
        }
        v[t] = s;
    }
    v
}

/// An input prepared for estimation: centred values and orders.
#[derive(Debug, Clone)]
pub(crate) struct InputData {
    pub name: String,
    pub xc: Vec<f64>,
    pub mean: f64,
    pub delay: usize,
    pub numerator: usize,
    pub denominator: usize,
}

impl InputData {
    pub(crate) fn new(input: &TransferInput, n: usize) -> Result<Self> {
        if input.values.len() != n {
            return Err(StatError::DimensionMismatch {
                context: "input series length vs. response length",
                expected: n,
                found: input.values.len(),
            });
        }
        if input.values.iter().any(|v| !v.is_finite()) {
            return Err(StatError::NonFiniteInput { context: "transfer-function input" });
        }
        let mean = input.values.iter().sum::<f64>() / n.max(1) as f64;
        let xc: Vec<f64> = input.values.iter().map(|v| v - mean).collect();
        let spread = xc.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
        if spread <= 1e-12 * mean.abs().max(1e-300) {
            return Err(invalid(format!("input '{}' is constant", input.name)));
        }
        if input.delay + input.numerator >= n {
            return Err(invalid(format!(
                "input '{}': the delay plus the numerator order must be shorter than the series",
                input.name
            )));
        }
        Ok(Self {
            name: input.name.clone(),
            xc,
            mean,
            delay: input.delay,
            numerator: input.numerator,
            denominator: input.denominator,
        })
    }

    pub(crate) const fn n_par(&self) -> usize {
        self.numerator + 1 + self.denominator
    }

    /// Split this input's coefficients into (ω, δ).
    pub(crate) fn split<'a>(&self, c: &'a [f64]) -> (&'a [f64], &'a [f64]) {
        c.split_at(self.numerator + 1)
    }

    /// The input lagged by `lag` with pre-sample zeros.
    pub(crate) fn lagged(&self, lag: usize) -> Vec<f64> {
        (0..self.xc.len()).map(|t| if t >= lag { self.xc[t - lag] } else { 0.0 }).collect()
    }
}

/// Impulse response weights estimated from the prewhitened cross-correlation.
#[derive(Debug, Clone)]
pub struct ImpulseResponseEstimate {
    /// The prewhitening autoregression and the full cross-correlation.
    pub prewhitened: PrewhitenedCrossCorrelation,
    /// Lags `k = 0 … K` (input leading the response).
    pub lags: Vec<usize>,
    /// Cross-correlations `r_αβ(k)` of the prewhitened input with the filtered response.
    pub correlations: Vec<f64>,
    /// Weight estimates `v̂ₖ = r_αβ(k) s_β / s_α`.
    pub weights: Vec<f64>,
    /// Half-width `z / √n` of the 95% limits for the correlations.
    pub limit: f64,
    /// First lag whose correlation is outside the limits (the suggested delay `b`).
    pub suggested_delay: Option<usize>,
}

/// Estimate impulse response weights of `y` on `x` by prewhitening (see the
/// module docs). Both series should already be differenced to stationarity.
/// `ar_order = None` chooses the prewhitening order by AIC.
///
/// # Errors
/// As [`prewhitened_cross_correlation`].
pub fn impulse_response_estimate(
    x: &[f64],
    y: &[f64],
    max_lag: Option<usize>,
    ar_order: Option<usize>,
) -> Result<ImpulseResponseEstimate> {
    let pw = prewhitened_cross_correlation(x, y, max_lag, ar_order)?;
    let alpha = pw.filter.filter(x);
    let beta = pw.filter.filter(y);
    let sd = |s: &[f64]| {
        let m = s.iter().sum::<f64>() / s.len() as f64;
        (s.iter().map(|v| (v - m).powi(2)).sum::<f64>() / s.len() as f64).sqrt()
    };
    let ratio = sd(&beta) / sd(&alpha);
    let mut lags = Vec::new();
    let mut correlations = Vec::new();
    for (&lag, &r) in pw.ccf.lags.iter().zip(&pw.ccf.values) {
        if lag <= 0 {
            lags.push((-lag) as usize);
            correlations.push(r);
        }
    }
    // Lags were −K…0; put them in increasing order of k.
    lags.reverse();
    correlations.reverse();
    let weights = correlations.iter().map(|r| r * ratio).collect();
    let limit = Normal::standard().quantile(0.975)? / (pw.ccf.n as f64).sqrt();
    let suggested_delay = lags.iter().zip(&correlations).find(|(_, r)| r.abs() > limit).map(|(&k, _)| k);
    Ok(ImpulseResponseEstimate { prewhitened: pw, lags, correlations, weights, limit, suggested_delay })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impulse_response_of_first_order_denominator() {
        // v(B) = 2 B² / (1 − 0.5B): v₂ = 2, v₃ = 1, v₄ = 0.5, …
        let e = TransferEstimate {
            name: "x".into(),
            delay: 2,
            omega: vec![2.0],
            delta: vec![0.5],
            input_mean: 0.0,
            effect: Vec::new(),
        };
        let v = e.impulse_response(5);
        let want = [0.0, 0.0, 2.0, 1.0, 0.5, 0.25];
        for (a, b) in v.iter().zip(want) {
            assert!((a - b).abs() < 1e-15);
        }
        assert!((e.steady_state_gain() - 4.0).abs() < 1e-15);
        // Numerator weights enter with a minus sign: (3 − B)x: v₀ = 3, v₁ = −1.
        let v = filter_input(&[1.0, 0.0, 0.0], 0, &[3.0, 1.0], &[]);
        assert_eq!(v, vec![3.0, -1.0, 0.0]);
        assert!((steady_state_gain(&[3.0, 1.0], &[]) - 2.0).abs() < 1e-15);
    }

    #[test]
    fn prewhitened_weights_recover_a_delay() {
        let mut rng = stat_core::rng::Rng::new(5);
        let n = 400;
        let mut x = vec![0.0; n];
        for t in 1..n {
            x[t] = 0.6 * x[t - 1] + rng.normal();
        }
        let y: Vec<f64> = (0..n).map(|t| if t >= 3 { 1.5 * x[t - 3] } else { 0.0 } + 0.3 * rng.normal()).collect();
        let est = impulse_response_estimate(&x, &y, Some(10), None).unwrap();
        assert_eq!(est.suggested_delay, Some(3));
        assert_eq!(est.lags[0], 0);
        assert!((est.weights[3] - 1.5).abs() < 0.15, "{}", est.weights[3]);
        assert!(est.weights[5].abs() < 0.3);
    }
}
