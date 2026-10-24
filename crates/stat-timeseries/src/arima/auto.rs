//! Automatic ARIMA order selection (Hyndman & Khandakar 2008, "Automatic time
//! series forecasting: the forecast package for R", *Journal of Statistical
//! Software* 27(3)).
//!
//! 1. The seasonal differencing order `D` (0 or 1 by default) is chosen from
//!    the strength of seasonality `F_S = max(0, 1 − var(R)/var(S + R))` of an
//!    STL decomposition (Wang, Smith & Hyndman 2006, *Data Mining and
//!    Knowledge Discovery* 13): difference seasonally while `F_S > 0.64`.
//! 2. The regular differencing order `d` is chosen by repeated KPSS tests
//!    (Kwiatkowski, Phillips, Schmidt & Shin 1992) of level stationarity at
//!    the 5% level on the seasonally differenced series: difference while the
//!    test rejects. The Bartlett truncation lag is `⌊3√n / 13⌋` (Hobijn,
//!    Franses & Ooms 2004, *Statistica Neerlandica* 58), the rule that
//!    reproduces the reference software's choices.
//! 3. Stepwise search over (p, q, P, Q) and the constant, minimising AICc.
//!    The search starts from ARIMA(2,d,2)(1,D,1), (0,d,0)(0,D,0),
//!    (1,d,0)(1,D,0) and (0,d,1)(0,D,1), each with a constant when
//!    `d + D ≤ 1`, and the (0,d,0)(0,D,0) model without it. From the current
//!    best model it then tries, in order, P ∓ 1 and Q ∓ 1 singly and together,
//!    p ∓ 1 and q ∓ 1 singly and together, and toggling the constant; the first
//!    model with lower AICc becomes the current model and the neighbours are
//!    tried again, until none improves. The neighbours carry the constant of
//!    the current search point, which starts as "included" (when allowed) and
//!    changes only through the toggle move, even when the starting model
//!    without a constant is the best of the starting set.
//!
//! A candidate is rejected (AICc = ∞) when it cannot be fitted or when a
//! root of its expanded AR or MA operator lies within 1.01 of the origin
//! (modulus < 1.01), since such fits are close to non-stationarity or
//! non-invertibility and forecast poorly.

use std::collections::HashMap;

use super::{ArimaFit, ArimaOptions, ArimaSpec, Regressor, TransferInput, fit_arima_transfer};
use crate::decompose::{StlOptions, seasonal_strength, stl};
use crate::unit_root::{KpssNull, kpss};
use crate::{Result, StatError};
use stat_core::{Matrix, QrDecomposition};

/// Options for [`auto_arima`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoArimaOptions {
    /// Seasonal period `m` (1 for non-seasonal data).
    pub period: usize,
    /// Regular differencing order; `None` chooses it by KPSS tests.
    pub d: Option<usize>,
    /// Seasonal differencing order; `None` chooses it by seasonal strength.
    pub seasonal_d: Option<usize>,
    /// Largest `d` the KPSS rule may choose (default 2).
    pub max_d: usize,
    /// Largest `D` the seasonal-strength rule may choose (default 1).
    pub max_seasonal_d: usize,
    /// Largest `p` (default 5).
    pub max_p: usize,
    /// Largest `q` (default 5).
    pub max_q: usize,
    /// Largest `P` (default 2).
    pub max_seasonal_p: usize,
    /// Largest `Q` (default 2).
    pub max_seasonal_q: usize,
    /// Largest `p + q + P + Q` in the exhaustive search (default 5). The
    /// stepwise search, as published, bounds each order separately only.
    pub max_order: usize,
    /// Allow a mean (`d + D = 0`) or drift (`d + D = 1`) term (default true).
    pub allow_constant: bool,
    /// Use the stepwise search (default true); otherwise every model within
    /// the bounds is fitted.
    pub stepwise: bool,
    /// Largest number of models fitted by the stepwise search (default 94;
    /// the exhaustive search fits every model).
    pub max_models: usize,
    /// Options passed to each fit.
    pub fit: ArimaOptions,
}

impl AutoArimaOptions {
    /// Defaults for a series with seasonal period `period`.
    #[must_use]
    pub fn new(period: usize) -> Self {
        Self {
            period,
            d: None,
            seasonal_d: None,
            max_d: 2,
            max_seasonal_d: 1,
            max_p: 5,
            max_q: 5,
            max_seasonal_p: 2,
            max_seasonal_q: 2,
            max_order: 5,
            allow_constant: true,
            stepwise: true,
            max_models: 94,
            fit: ArimaOptions::default(),
        }
    }
}

/// One model visited by the search.
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateModel {
    /// Its orders and constant.
    pub spec: ArimaSpec,
    /// Its AICc (∞ when the fit failed or was rejected).
    pub aicc: f64,
}

/// Result of [`auto_arima`].
#[derive(Debug, Clone)]
pub struct AutoArimaResult {
    /// The selected model.
    pub fit: ArimaFit,
    /// The regular differencing order used.
    pub d: usize,
    /// The seasonal differencing order used.
    pub seasonal_d: usize,
    /// Every model fitted, in the order visited.
    pub candidates: Vec<CandidateModel>,
}

fn is_constant(x: &[f64]) -> bool {
    x.iter().all(|&v| v == x[0])
}

fn diff_lag(x: &[f64], lag: usize) -> Vec<f64> {
    (lag..x.len()).map(|t| x[t] - x[t - lag]).collect()
}

/// Number of regular differences by repeated KPSS level-stationarity tests at
/// the 5% level, with truncation lag `⌊3√n / 13⌋`: difference while the test
/// rejects, up to `max_d`. Stops early
/// when the series becomes constant or too short to test.
///
/// # Errors
/// [`StatError::NonFiniteInput`] for NaN or ±∞ in `y`.
pub fn kpss_differencing_order(y: &[f64], max_d: usize) -> Result<usize> {
    if y.iter().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "time series" });
    }
    let mut x = y.to_vec();
    let mut d = 0;
    while d < max_d && x.len() >= 4 && !is_constant(&x) {
        let Ok(test) = kpss(&x, KpssNull::Level, Some((3.0 * (x.len() as f64).sqrt() / 13.0) as usize)) else { break };
        if test.p_value >= 0.05 {
            break;
        }
        x = diff_lag(&x, 1);
        d += 1;
    }
    Ok(d)
}

/// Number of seasonal differences by seasonal strength: difference at lag
/// `period` while the STL seasonal strength exceeds 0.64, up to
/// `max_seasonal_d`. Returns 0 for `period < 2` or at most two full cycles
/// (as R's `forecast::nsdiffs`, whose STL fit needs more than two periods).
///
/// # Errors
/// [`StatError::NonFiniteInput`] for NaN or ±∞ in `y`.
pub fn seasonal_differencing_order(y: &[f64], period: usize, max_seasonal_d: usize) -> Result<usize> {
    if y.iter().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "time series" });
    }
    if period < 2 {
        return Ok(0);
    }
    let mut x = y.to_vec();
    let mut big_d = 0;
    while big_d < max_seasonal_d && x.len() > 2 * period && !is_constant(&x) {
        let Ok(dec) = stl(&x, &StlOptions::new(period)) else { break };
        if seasonal_strength(&dec) <= 0.64 {
            break;
        }
        x = diff_lag(&x, period);
        big_d += 1;
    }
    Ok(big_d)
}

type Key = (usize, usize, usize, usize, bool);

struct Search<'a> {
    y: &'a [f64],
    xreg: &'a [Regressor],
    inputs: &'a [TransferInput],
    options: &'a AutoArimaOptions,
    d: usize,
    big_d: usize,
    seasonal: bool,
    visited: HashMap<Key, f64>,
    candidates: Vec<CandidateModel>,
    best: Option<(f64, ArimaFit)>,
}

impl Search<'_> {
    fn spec(&self, k: Key) -> ArimaSpec {
        let mut s = ArimaSpec::new(k.0, self.d, k.1).with_constant(k.4);
        if self.seasonal {
            s = s.seasonal(k.2, self.big_d, k.3, self.options.period);
        }
        s
    }

    fn within(&self, k: Key) -> bool {
        let o = self.options;
        k.0 <= o.max_p
            && k.1 <= o.max_q
            && k.2 <= o.max_seasonal_p
            && k.3 <= o.max_seasonal_q
            && (self.seasonal || (k.2 == 0 && k.3 == 0))
    }

    /// Fit `k` unless already visited or out of bounds; true when it becomes
    /// the new best model.
    fn try_model(&mut self, k: Key) -> bool {
        let full = self.options.stepwise && self.candidates.len() >= self.options.max_models;
        if !self.within(k) || self.visited.contains_key(&k) || full {
            return false;
        }
        let spec = self.spec(k);
        let mut improves = false;
        let aicc = match fit_arima_transfer(self.y, &spec, self.xreg, self.inputs, &self.options.fit) {
            Ok(fit) if fit.aicc.is_finite() && fit.min_root_modulus() >= 1.01 => {
                let a = fit.aicc;
                improves = self.best.as_ref().is_none_or(|(b, _)| a < *b);
                if improves {
                    self.best = Some((a, fit));
                }
                a
            }
            _ => f64::INFINITY,
        };
        self.visited.insert(k, aicc);
        self.candidates.push(CandidateModel { spec, aicc });
        improves
    }
}

fn key_of(s: &ArimaSpec) -> Key {
    (s.p, s.q, s.seasonal_p, s.seasonal_q, s.constant)
}

/// Select and fit a seasonal ARIMA model automatically (Hyndman & Khandakar
/// 2008; see the module documentation for the rules). The differencing
/// orders can be supplied through `options.d` and `options.seasonal_d`;
/// otherwise `D` is chosen by seasonal strength and then `d` by KPSS tests on
/// the seasonally differenced series. Models are compared by AICc on the same
/// differenced data, so the differencing orders are fixed before the search.
///
/// # Errors
/// * [`StatError::NonFiniteInput`] for NaN or ±∞ in `y`.
/// * [`StatError::InvalidArgument`] for a period of 0.
/// * [`StatError::ConvergenceFailure`] when no candidate model could be fitted.
/// * Errors of [`fit_arima`] that concern the data (regressor lengths).
pub fn auto_arima(y: &[f64], xreg: &[Regressor], options: &AutoArimaOptions) -> Result<AutoArimaResult> {
    auto_arima_transfer(y, xreg, &[], options)
}

/// [`auto_arima`] for a transfer-function–noise model: the inputs keep their
/// given delays and orders while the search chooses the noise model. The
/// differencing tests are applied to the response with the input effects
/// removed (estimated with an AR(1) noise model), so that a nonstationary
/// input does not decide the differencing of the noise.
///
/// # Errors
/// As [`auto_arima`] and [`fit_arima_transfer`].
pub fn auto_arima_transfer(
    y: &[f64],
    xreg: &[Regressor],
    inputs: &[TransferInput],
    options: &AutoArimaOptions,
) -> Result<AutoArimaResult> {
    if options.period == 0 {
        return Err(StatError::InvalidArgument { message: "the seasonal period must be at least 1".into() });
    }
    if y.iter().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "time series" });
    }
    let m = options.period;
    let tested = if inputs.is_empty() { y.to_vec() } else { input_residuals(y, inputs)? };
    let big_d = match options.seasonal_d {
        Some(v) => v,
        None => seasonal_differencing_order(&tested, m, options.max_seasonal_d)?,
    };
    let d = match options.d {
        Some(v) => v,
        None => {
            let x = if big_d > 0 && m > 1 {
                (0..big_d).fold(tested.clone(), |x, _| diff_lag(&x, m))
            } else {
                tested.clone()
            };
            kpss_differencing_order(&x, options.max_d)?
        }
    };
    let seasonal = m > 1 && (big_d > 0 || options.max_seasonal_p + options.max_seasonal_q > 0);
    let constant = options.allow_constant && d + big_d <= 1;
    // Check the regressors once, so a data error is reported rather than every
    // candidate failing.
    let probe = ArimaSpec::new(0, d, 0).with_constant(false);
    let probe = if seasonal { probe.seasonal(0, big_d, 0, m) } else { probe };
    super::Problem::new(y, &probe, xreg, inputs)?;
    let mut search = Search {
        y,
        xreg,
        inputs,
        options,
        d,
        big_d,
        seasonal,
        visited: HashMap::new(),
        candidates: Vec::new(),
        best: None,
    };
    if options.stepwise {
        let o = options;
        let sp = |v: usize, max: usize| v.min(max);
        let (sp1, sq1) = if seasonal { (sp(1, o.max_seasonal_p), sp(1, o.max_seasonal_q)) } else { (0, 0) };
        search.try_model((sp(2, o.max_p), sp(2, o.max_q), sp1, sq1, constant));
        search.try_model((0, 0, 0, 0, constant));
        search.try_model((sp(1, o.max_p), 0, sp1, 0, constant));
        search.try_model((0, sp(1, o.max_q), 0, sq1, constant));
        if constant {
            search.try_model((0, 0, 0, 0, false));
        }
        let mut current = match &search.best {
            Some((_, fit)) => {
                let k = key_of(&fit.spec);
                (k.0, k.1, k.2, k.3, constant)
            }
            None => (0, 0, 0, 0, constant),
        };
        while search.best.is_some() {
            let (p, q, bp, bq, c) = current;
            let dec = |v: usize| v.checked_sub(1);
            let mut moves: Vec<Key> = Vec::new();
            let mut push = |k: Option<Key>| {
                if let Some(k) = k {
                    moves.push(k);
                }
            };
            if seasonal {
                push(dec(bp).map(|v| (p, q, v, bq, c)));
                push(dec(bq).map(|v| (p, q, bp, v, c)));
                push(Some((p, q, bp + 1, bq, c)));
                push(Some((p, q, bp, bq + 1, c)));
                push(dec(bp).zip(dec(bq)).map(|(a, b)| (p, q, a, b, c)));
                push(dec(bp).map(|a| (p, q, a, bq + 1, c)));
                push(dec(bq).map(|b| (p, q, bp + 1, b, c)));
                push(Some((p, q, bp + 1, bq + 1, c)));
            }
            push(dec(p).map(|v| (v, q, bp, bq, c)));
            push(dec(q).map(|v| (p, v, bp, bq, c)));
            push(Some((p + 1, q, bp, bq, c)));
            push(Some((p, q + 1, bp, bq, c)));
            push(dec(p).zip(dec(q)).map(|(a, b)| (a, b, bp, bq, c)));
            push(dec(p).map(|a| (a, q + 1, bp, bq, c)));
            push(dec(q).map(|b| (p + 1, b, bp, bq, c)));
            push(Some((p + 1, q + 1, bp, bq, c)));
            if constant {
                push(Some((p, q, bp, bq, !c)));
            }
            let mut improved = false;
            for k in moves {
                if search.try_model(k) {
                    current = k;
                    improved = true;
                    break;
                }
            }
            if !improved || search.candidates.len() >= options.max_models {
                break;
            }
        }
    } else {
        let (mp, mq) = if seasonal { (options.max_seasonal_p, options.max_seasonal_q) } else { (0, 0) };
        let consts: &[bool] = if constant { &[true, false] } else { &[false] };
        let mut all = Vec::new();
        for p in 0..=options.max_p {
            for q in 0..=options.max_q {
                for bp in 0..=mp {
                    for bq in 0..=mq {
                        if p + q + bp + bq <= options.max_order {
                            for &c in consts {
                                all.push((p, q, bp, bq, c));
                            }
                        }
                    }
                }
            }
        }
        for k in all {
            search.try_model(k);
        }
    }
    let Search { best, candidates, .. } = search;
    finish(best, candidates, d, big_d)
}

/// The noise series `N̂ = y − Σ v̂ᵢ` whose differencing the tests decide: the
/// transfer weights are estimated with a stationary AR(1) noise model and a
/// mean (an AR coefficient near one stands in for a unit root), and the
/// fitted input effects removed. Falls back to the residuals of the
/// least-squares regression of `y` on the lagged inputs `x̃_{t−b−j}`,
/// `j = 0…s`, when that fit fails.
fn input_residuals(y: &[f64], inputs: &[TransferInput]) -> Result<Vec<f64>> {
    let spec = ArimaSpec::new(1, 0, 0).with_constant(true);
    if let Ok(fit) = fit_arima_transfer(y, &spec, &[], inputs, &ArimaOptions::default()) {
        return Ok((0..y.len()).map(|t| y[t] - fit.transfer.iter().map(|e| e.effect[t]).sum::<f64>()).collect());
    }
    let n = y.len();
    let mut cols = vec![vec![1.0; n]];
    for input in inputs {
        let data = super::transfer::InputData::new(input, n)?;
        for j in 0..=input.numerator {
            cols.push(data.lagged(input.delay + j));
        }
    }
    let k = cols.len();
    let qr = QrDecomposition::new(Matrix::from_columns(&cols)?)?;
    let rank = qr.rank(qr.default_rank_tolerance());
    if rank < k {
        return Err(StatError::RankDeficient { rank, columns: k });
    }
    let beta = qr.solve_least_squares(y)?;
    Ok((0..n).map(|t| y[t] - cols.iter().zip(&beta).map(|(c, b)| c[t] * b).sum::<f64>()).collect())
}

fn finish(
    best: Option<(f64, ArimaFit)>,
    candidates: Vec<CandidateModel>,
    d: usize,
    seasonal_d: usize,
) -> Result<AutoArimaResult> {
    match best {
        Some((_, fit)) => Ok(AutoArimaResult { fit, d, seasonal_d, candidates }),
        None => Err(StatError::ConvergenceFailure { routine: "automatic ARIMA search", iterations: candidates.len() }),
    }
}
