//! Seasonal decomposition: classical moving-average decomposition and STL.
//!
//! # Classical decomposition
//!
//! The trend is a centred moving average of order `m` (Kendall & Stuart,
//! *The Advanced Theory of Statistics* vol. 3, §46; Makridakis, Wheelwright &
//! Hyndman, *Forecasting*, §3.4): for odd `m` the plain average of the `m`
//! values centred on `t`, for even `m` the `2×m` average with weights
//! `(½, 1, …, 1, ½)/m` over `m + 1` values. It is undefined for the first and
//! last `⌊m/2⌋` observations. The detrended series (`y − T` additive, `y / T`
//! multiplicative) is averaged by position within the cycle, and those
//! averages are normalised to sum to 0 (additive) or to average 1
//! (multiplicative) to give the seasonal indices. The remainder is
//! `y − T − S` or `y / (T S)`.
//!
//! # STL
//!
//! Seasonal–trend decomposition by LOESS (Cleveland, Cleveland, McRae &
//! Terpenning 1990, *J. Official Statistics* 6(1):3–73). Each pass of the
//! inner loop, starting from trend `T = 0`:
//!
//! 1. detrend, `y − T`;
//! 2. smooth each cycle-subseries (all January values, …) by LOESS with span
//!    `n_s` and degree `d_s`, extending it one period before and after the
//!    data, giving `C` of length `n + 2m`;
//! 3. low-pass filter `C`: moving averages of lengths `m`, `m`, 3, then LOESS
//!    with span `n_l` and degree `d_l`, giving `L`;
//! 4. seasonal `S = C − L` (over the data's time points);
//! 5. deseasonalise, `y − S`;
//! 6. trend `T` = LOESS of `y − S` with span `n_t` and degree `d_t`.
//!
//! The outer loop computes robustness weights `ρ_t = B(|R_t| / (6 · median|R|))`
//! from the remainder `R = y − S − T`, with the bisquare `B(u) = (1 − u²)²`
//! for `u < 1` and 0 otherwise, and repeats the inner loop with the LOESS
//! neighbourhood weights of steps 2 and 6 multiplied by `ρ`.
//!
//! LOESS (Cleveland 1979, *JASA* 74:829) at a point `x` fits a weighted
//! polynomial of degree 0 or 1 to the `q` nearest points with tricube weights
//! `(1 − (|xᵢ − x|/λ)³)³`, `λ` the distance to the `q`-th nearest point. For
//! `q > n` the paper scales `λ_n` by `q/n`; here the neighbourhood is instead
//! widened by `⌊(q − n)/2⌋` on each side, as if the missing points were
//! there. This reproduces R's `stl` and statsmodels' `STL` (used as reference
//! software in the tests) on periodic fits, where the scaled form differs
//! from them by about 10⁻⁶ relative. To save time the smoother may be
//! evaluated at every `j`-th point only (and at the last), with linear
//! interpolation in between (the paper's `n_jump`).
//!
//! Defaults are those of the paper with the choices of R's `stats::stl` where
//! the paper leaves a choice open: seasonal degree 0, trend and low-pass
//! degree 1, `n_l` the smallest odd integer `≥ m`,
//! `n_t = nextodd(⌈1.5 m / (1 − 1.5 / n_s)⌉)`, jumps `⌈span / 10⌉`, two inner
//! passes without robustness or one inner pass with fifteen robustness
//! iterations. The periodic option (as R's `s.window = "periodic"`) smooths
//! the cycle-subseries at degree 0 with span `10n + 1`, so that the tricube
//! weights are within about 10⁻⁴ of uniform and each smooth is close to the
//! (robustness-weighted) subseries mean, the limit of an infinite span; at
//! the end the seasonal values at each position in the cycle are replaced by
//! their mean, so every cycle is identical.

use stat_core::descriptive::median;

use crate::{Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Options for STL decomposition (Cleveland et al. 1990).
#[derive(Debug, Clone)]
pub struct StlOptions {
    /// Number of observations per seasonal cycle (≥ 2).
    pub period: usize,
    /// Seasonal smoothing span (odd, ≥ 7 recommended; an even span is
    /// rounded up), or `None` for the periodic seasonal (every cycle the
    /// same), as R's `s.window = "periodic"`.
    pub seasonal_window: Option<usize>,
    /// Trend smoothing span (odd), or `None` for the default
    /// `nextodd(⌈1.5 · period / (1 − 1.5 / seasonal_window)⌉)` (with
    /// `1.5 / seasonal_window = 0` for the periodic seasonal).
    pub trend_window: Option<usize>,
    /// Use robustness iterations (outer loop) against outliers.
    pub robust: bool,
    /// Low-pass filter span, or `None` for the smallest odd integer ≥ period.
    pub lowpass_window: Option<usize>,
    /// Degree (0 or 1) of the cycle-subseries LOESS; default 0 (always 0 for
    /// the periodic seasonal).
    pub seasonal_degree: usize,
    /// Degree (0 or 1) of the trend LOESS; default 1.
    pub trend_degree: usize,
    /// Degree (0 or 1) of the low-pass LOESS, or `None` for the trend degree.
    pub lowpass_degree: Option<usize>,
    /// Evaluate the seasonal smoother at every `j`-th point; `None` for
    /// `⌈seasonal_window / 10⌉`.
    pub seasonal_jump: Option<usize>,
    /// Trend smoother jump; `None` for `⌈trend_window / 10⌉`.
    pub trend_jump: Option<usize>,
    /// Low-pass smoother jump; `None` for `⌈lowpass_window / 10⌉`.
    pub lowpass_jump: Option<usize>,
    /// Inner-loop passes; `None` for 2 without robustness, 1 with.
    pub inner_iterations: Option<usize>,
    /// Robustness iterations; `None` for 0 without robustness, 15 with.
    pub outer_iterations: Option<usize>,
}

impl StlOptions {
    /// Periodic seasonal, default trend span, no robustness iterations.
    pub fn new(period: usize) -> Self {
        Self {
            period,
            seasonal_window: None,
            trend_window: None,
            robust: false,
            lowpass_window: None,
            seasonal_degree: 0,
            trend_degree: 1,
            lowpass_degree: None,
            seasonal_jump: None,
            trend_jump: None,
            lowpass_jump: None,
            inner_iterations: None,
            outer_iterations: None,
        }
    }
}

/// A series split as `y = trend + seasonal + remainder` (additive) or
/// `y = trend · seasonal · remainder` (multiplicative classical only).
#[derive(Debug, Clone)]
pub struct Decomposition {
    /// Observations per cycle.
    pub period: usize,
    /// Trend-cycle component (NaN where a classical moving average is undefined).
    pub trend: Vec<f64>,
    /// Seasonal component.
    pub seasonal: Vec<f64>,
    /// Remainder (irregular) component.
    pub remainder: Vec<f64>,
}

/// Resolved STL spans and the final robustness weights.
#[derive(Debug, Clone)]
pub struct StlFit {
    /// The additive decomposition.
    pub decomposition: Decomposition,
    /// Robustness weights `ρ_t` used in the last inner-loop pass, computed
    /// from the previous pass's remainder (all 1 without robustness).
    pub weights: Vec<f64>,
    /// Seasonal span used (`None` when periodic).
    pub seasonal_window: Option<usize>,
    /// Trend span used.
    pub trend_window: usize,
    /// Low-pass span used.
    pub lowpass_window: usize,
}

fn next_odd(x: usize) -> usize {
    if x % 2 == 0 { x + 1 } else { x }
}

fn check_finite(y: &[f64]) -> Result<()> {
    if y.iter().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "series" });
    }
    Ok(())
}

/// LOESS fit at position `x` of the values `y` observed at `0, 1, …, n−1`.
/// Returns `None` when every neighbourhood weight is zero.
fn loess_at(y: &[f64], span: usize, degree: usize, x: f64, rw: Option<&[f64]>) -> Option<f64> {
    let n = y.len();
    let (lo, hi, lambda) = if span >= n {
        let far = x.max((n - 1) as f64 - x);
        let far = if n == 1 { x.abs() } else { far };
        (0, n - 1, far + ((span - n) / 2) as f64)
    } else {
        let start = (x - (span - 1) as f64 / 2.0).round().clamp(0.0, (n - span) as f64) as usize;
        let end = start + span - 1;
        (start, end, (x - start as f64).max(end as f64 - x))
    };
    let mut sw = 0.0;
    let mut sx = 0.0;
    let mut sy = 0.0;
    let mut weights = Vec::with_capacity(hi - lo + 1);
    for i in lo..=hi {
        let d = (i as f64 - x).abs();
        let mut w = if lambda > 0.0 {
            let u = d / lambda;
            if u < 1.0 { (1.0 - u * u * u).powi(3) } else { 0.0 }
        } else if d == 0.0 {
            1.0
        } else {
            0.0
        };
        if let Some(r) = rw {
            w *= r[i];
        }
        weights.push(w);
        sw += w;
        sx += w * i as f64;
        sy += w * y[i];
    }
    if sw <= 0.0 {
        return None;
    }
    let (xm, ym) = (sx / sw, sy / sw);
    if degree == 0 {
        return Some(ym);
    }
    let mut sxx = 0.0;
    let mut sxy = 0.0;
    for (k, w) in weights.iter().enumerate() {
        let dx = (lo + k) as f64 - xm;
        sxx += w * dx * dx;
        sxy += w * dx * (y[lo + k] - ym);
    }
    // Fall back to a local constant when the weighted x values are
    // (numerically) all at one point.
    let range = (hi - lo) as f64;
    if sxx / sw <= (1e-3 * range).powi(2) {
        return Some(ym);
    }
    Some(ym + sxy / sxx * (x - xm))
}

/// LOESS smooth of `y` at every point, evaluated at every `jump`-th point and
/// the last, linearly interpolated in between.
fn loess_smooth(y: &[f64], span: usize, degree: usize, jump: usize, rw: Option<&[f64]>) -> Vec<f64> {
    let n = y.len();
    let mut out = vec![0.0; n];
    if n == 0 {
        return out;
    }
    let jump = jump.max(1);
    let mut points: Vec<usize> = (0..n).step_by(jump).collect();
    if *points.last().unwrap_or(&0) != n - 1 {
        points.push(n - 1);
    }
    for &i in &points {
        out[i] = loess_at(y, span, degree, i as f64, rw).unwrap_or(y[i]);
    }
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let slope = (out[b] - out[a]) / (b - a) as f64;
        for i in a + 1..b {
            out[i] = out[a] + slope * (i - a) as f64;
        }
    }
    out
}

fn moving_average(x: &[f64], len: usize) -> Vec<f64> {
    if x.len() < len {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(x.len() - len + 1);
    let mut sum: f64 = x[..len].iter().sum();
    out.push(sum / len as f64);
    for i in len..x.len() {
        sum += x[i] - x[i - len];
        out.push(sum / len as f64);
    }
    out
}

struct Resolved {
    period: usize,
    periodic: bool,
    seasonal: usize,
    trend: usize,
    lowpass: usize,
    s_degree: usize,
    t_degree: usize,
    l_degree: usize,
    s_jump: usize,
    t_jump: usize,
    l_jump: usize,
    inner: usize,
    outer: usize,
}

fn resolve(n: usize, o: &StlOptions) -> Result<Resolved> {
    let m = o.period;
    if m < 2 {
        return Err(invalid(format!("STL needs a seasonal period of at least 2 (got {m})")));
    }
    if n < 2 * m {
        return Err(StatError::InsufficientData { required: 2 * m, found: n });
    }
    let degree = |d: usize, what: &str| {
        if d <= 1 { Ok(d) } else { Err(invalid(format!("the {what} LOESS degree must be 0 or 1 (got {d})"))) }
    };
    let s_degree = degree(o.seasonal_degree, "seasonal")?;
    let t_degree = degree(o.trend_degree, "trend")?;
    let l_degree = degree(o.lowpass_degree.unwrap_or(t_degree), "low-pass")?;
    let span = |w: usize, what: &str| {
        if w < 3 { Err(invalid(format!("the {what} span must be at least 3 (got {w})"))) } else { Ok(next_odd(w)) }
    };
    let periodic = o.seasonal_window.is_none();
    let seasonal = match o.seasonal_window {
        Some(w) => span(w, "seasonal")?,
        None => 10 * n + 1,
    };
    let s_degree = if periodic { 0 } else { s_degree };
    let trend = match o.trend_window {
        Some(w) => span(w, "trend")?,
        None => next_odd((1.5 * m as f64 / (1.0 - 1.5 / seasonal as f64)).ceil() as usize),
    };
    let lowpass = match o.lowpass_window {
        Some(w) => span(w, "low-pass")?,
        None => next_odd(m),
    };
    let jump = |j: Option<usize>, w: usize| j.unwrap_or_else(|| w.div_ceil(10)).max(1);
    Ok(Resolved {
        period: m,
        periodic,
        seasonal,
        trend,
        lowpass,
        s_degree,
        t_degree,
        l_degree,
        s_jump: jump(o.seasonal_jump, seasonal),
        t_jump: jump(o.trend_jump, trend),
        l_jump: jump(o.lowpass_jump, lowpass),
        inner: o.inner_iterations.unwrap_or(if o.robust { 1 } else { 2 }).max(1),
        outer: o.outer_iterations.unwrap_or(if o.robust { 15 } else { 0 }),
    })
}

/// One inner-loop pass; updates `seasonal` and `trend` in place.
fn inner_pass(y: &[f64], r: &Resolved, rw: Option<&[f64]>, seasonal: &mut [f64], trend: &mut [f64]) {
    let n = y.len();
    let m = r.period;
    let detrended: Vec<f64> = y.iter().zip(trend.iter()).map(|(a, b)| a - b).collect();
    // Cycle-subseries smoothing into C (index t + m for time t ∈ [−m, n + m)).
    let mut c = vec![0.0; n + 2 * m];
    for j in 0..m {
        let idx: Vec<usize> = (j..n).step_by(m).collect();
        let sub: Vec<f64> = idx.iter().map(|&t| detrended[t]).collect();
        let sub_rw: Option<Vec<f64>> = rw.map(|w| idx.iter().map(|&t| w[t]).collect());
        let k = sub.len();
        let w = sub_rw.as_deref();
        let inner = loess_smooth(&sub, r.seasonal, r.s_degree, r.s_jump, w);
        let before = loess_at(&sub, r.seasonal, r.s_degree, -1.0, w).unwrap_or(inner[0]);
        let after = loess_at(&sub, r.seasonal, r.s_degree, k as f64, w).unwrap_or(inner[k - 1]);
        let mut smoothed = Vec::with_capacity(k + 2);
        smoothed.push(before);
        smoothed.extend(inner);
        smoothed.push(after);
        for (s, value) in smoothed.into_iter().enumerate() {
            // Subseries index s − 1 is time j + (s − 1) m, stored at that + m.
            c[j + s * m] = value;
        }
    }
    // Low-pass filter of C.
    let l1 = moving_average(&c, m);
    let l2 = moving_average(&l1, m);
    let l3 = moving_average(&l2, 3);
    let low = loess_smooth(&l3, r.lowpass, r.l_degree, r.l_jump, None);
    for t in 0..n {
        seasonal[t] = c[t + m] - low[t];
    }
    let deseasonal: Vec<f64> = y.iter().zip(seasonal.iter()).map(|(a, b)| a - b).collect();
    let new_trend = loess_smooth(&deseasonal, r.trend, r.t_degree, r.t_jump, rw);
    trend.copy_from_slice(&new_trend);
}

fn robustness_weights(remainder: &[f64]) -> Result<Vec<f64>> {
    let abs: Vec<f64> = remainder.iter().map(|v| v.abs()).collect();
    let h = 6.0 * median(&abs)?;
    Ok(abs
        .iter()
        .map(|&a| {
            if h > 0.0 {
                let u = a / h;
                if u < 1.0 { (1.0 - u * u).powi(2) } else { 0.0 }
            } else if a == 0.0 {
                1.0
            } else {
                0.0
            }
        })
        .collect())
}

/// STL decomposition with its resolved spans and robustness weights; see the
/// module docs for the algorithm and defaults.
///
/// # Errors
/// [`StatError::InsufficientData`] with fewer than two full cycles;
/// [`StatError::NonFiniteInput`] for NaN or ±∞; [`StatError::InvalidArgument`]
/// for a period below 2, a span below 3 or a degree above 1.
pub fn stl_fit(y: &[f64], options: &StlOptions) -> Result<StlFit> {
    check_finite(y)?;
    let r = resolve(y.len(), options)?;
    let n = y.len();
    let mut seasonal = vec![0.0; n];
    let mut trend = vec![0.0; n];
    let mut weights: Option<Vec<f64>> = None;
    for outer in 0..=r.outer {
        for _ in 0..r.inner {
            inner_pass(y, &r, weights.as_deref(), &mut seasonal, &mut trend);
        }
        if outer < r.outer {
            let rem: Vec<f64> = (0..n).map(|t| y[t] - seasonal[t] - trend[t]).collect();
            weights = Some(robustness_weights(&rem)?);
        }
    }
    if r.periodic {
        // Make every cycle identical: each position's seasonal values are
        // replaced by their mean.
        for j in 0..r.period {
            let values: Vec<f64> = seasonal.iter().skip(j).step_by(r.period).copied().collect();
            let mean = values.iter().sum::<f64>() / values.len() as f64;
            seasonal.iter_mut().skip(j).step_by(r.period).for_each(|v| *v = mean);
        }
    }
    let remainder: Vec<f64> = (0..n).map(|t| y[t] - seasonal[t] - trend[t]).collect();
    let weights = weights.unwrap_or_else(|| vec![1.0; n]);
    Ok(StlFit {
        decomposition: Decomposition { period: r.period, trend, seasonal, remainder },
        weights,
        seasonal_window: (!r.periodic).then_some(r.seasonal),
        trend_window: r.trend,
        lowpass_window: r.lowpass,
    })
}

/// STL decomposition of `y`.
///
/// # Errors
/// Fails when the series has fewer than two full cycles or contains non-finite
/// values, or the options are invalid (see [`stl_fit`]).
pub fn stl(y: &[f64], options: &StlOptions) -> Result<Decomposition> {
    Ok(stl_fit(y, options)?.decomposition)
}

fn variance(x: &[f64]) -> f64 {
    let n = x.len() as f64;
    let m = x.iter().sum::<f64>() / n;
    x.iter().map(|v| (v - m).powi(2)).sum::<f64>() / (n - 1.0)
}

/// Strength of seasonality, `max(0, 1 − var(R) / var(S + R))`
/// (Wang, Smith & Hyndman 2006), from an additive decomposition. Time points
/// where any component is NaN (the ends of a classical decomposition) are
/// skipped. Returns NaN with fewer than two usable points and 0 when
/// `var(S + R) = 0`.
pub fn seasonal_strength(decomposition: &Decomposition) -> f64 {
    let (r, sr): (Vec<f64>, Vec<f64>) = decomposition
        .remainder
        .iter()
        .zip(&decomposition.seasonal)
        .filter(|(r, s)| r.is_finite() && s.is_finite())
        .map(|(r, s)| (*r, r + s))
        .unzip();
    if r.len() < 2 {
        return f64::NAN;
    }
    let denominator = variance(&sr);
    if denominator <= 0.0 {
        return 0.0;
    }
    (1.0 - variance(&r) / denominator).max(0.0)
}

/// Additive or multiplicative classical decomposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecompositionType {
    /// `y = T + S + R`; seasonal indices sum to 0.
    Additive,
    /// `y = T · S · R`; seasonal indices average 1.
    Multiplicative,
}

/// Result of a classical decomposition.
#[derive(Debug, Clone)]
pub struct ClassicalDecomposition {
    /// Additive or multiplicative.
    pub kind: DecompositionType,
    /// Components; trend and remainder are NaN for the first and last
    /// `⌊m/2⌋` observations.
    pub components: Decomposition,
    /// Seasonal indices by position in the cycle, position 0 being the first
    /// observation's.
    pub indices: Vec<f64>,
}

/// Centred moving average of order `m` (the `2×m` average for even `m`), NaN
/// where it is undefined.
///
/// # Errors
/// [`StatError::InvalidArgument`] for `m < 2`; [`StatError::InsufficientData`]
/// when the series is shorter than the average's window.
pub fn centered_moving_average(y: &[f64], m: usize) -> Result<Vec<f64>> {
    if m < 2 {
        return Err(invalid("the moving-average order must be at least 2"));
    }
    check_finite(y)?;
    let n = y.len();
    let half = m / 2;
    if n < 2 * half + 1 {
        return Err(StatError::InsufficientData { required: 2 * half + 1, found: n });
    }
    let mut out = vec![f64::NAN; n];
    for t in half..n - half {
        let window = &y[t - half..=t + half];
        out[t] = if m % 2 == 1 {
            window.iter().sum::<f64>() / m as f64
        } else {
            (window[1..m].iter().sum::<f64>() + 0.5 * (window[0] + window[m])) / m as f64
        };
    }
    Ok(out)
}

/// Classical decomposition of `y` with period `m` (see the module docs).
///
/// # Errors
/// [`StatError::InvalidArgument`] for `m < 2`, or non-positive values in the
/// multiplicative case; [`StatError::InsufficientData`] with fewer than two
/// full cycles; [`StatError::NonFiniteInput`] for NaN or ±∞.
pub fn classical_decomposition(y: &[f64], period: usize, kind: DecompositionType) -> Result<ClassicalDecomposition> {
    if period < 2 {
        return Err(invalid(format!("the seasonal period must be at least 2 (got {period})")));
    }
    check_finite(y)?;
    let n = y.len();
    if n < 2 * period {
        return Err(StatError::InsufficientData { required: 2 * period, found: n });
    }
    let multiplicative = kind == DecompositionType::Multiplicative;
    if multiplicative && y.iter().any(|&v| v <= 0.0) {
        return Err(invalid("a multiplicative decomposition needs positive values"));
    }
    let trend = centered_moving_average(y, period)?;
    let detrended: Vec<f64> = y.iter().zip(&trend).map(|(v, t)| if multiplicative { v / t } else { v - t }).collect();
    let mut indices: Vec<f64> = (0..period)
        .map(|j| {
            let vals: Vec<f64> = detrended.iter().skip(j).step_by(period).copied().filter(|v| v.is_finite()).collect();
            vals.iter().sum::<f64>() / vals.len() as f64
        })
        .collect();
    let centre = indices.iter().sum::<f64>() / period as f64;
    for v in &mut indices {
        if multiplicative {
            *v /= centre;
        } else {
            *v -= centre;
        }
    }
    let seasonal: Vec<f64> = (0..n).map(|t| indices[t % period]).collect();
    let remainder: Vec<f64> = (0..n)
        .map(|t| if multiplicative { y[t] / (trend[t] * seasonal[t]) } else { y[t] - trend[t] - seasonal[t] })
        .collect();
    Ok(ClassicalDecomposition { kind, components: Decomposition { period, trend, seasonal, remainder }, indices })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic(n: usize, m: usize) -> Vec<f64> {
        (0..n)
            .map(|t| {
                let t = t as f64;
                10.0 + 0.05 * t + 3.0 * (2.0 * std::f64::consts::PI * t / m as f64).sin() + 0.3 * (t * 1.7).sin()
            })
            .collect()
    }

    #[test]
    fn stl_components_add_up_and_periodic_is_periodic() {
        let y = synthetic(96, 12);
        let fit = stl_fit(&y, &StlOptions::new(12)).unwrap();
        let d = &fit.decomposition;
        for t in 0..y.len() {
            assert!((d.trend[t] + d.seasonal[t] + d.remainder[t] - y[t]).abs() < 1e-12);
        }
        for t in 12..y.len() {
            assert!((d.seasonal[t] - d.seasonal[t - 12]).abs() < 1e-9);
        }
        assert!(seasonal_strength(d) > 0.9);
        assert_eq!(fit.weights, vec![1.0; y.len()]);
    }

    #[test]
    fn robust_stl_downweights_an_outlier() {
        let mut y = synthetic(120, 12);
        y[50] += 40.0;
        let options = StlOptions { robust: true, seasonal_window: Some(7), ..StlOptions::new(12) };
        let fit = stl_fit(&y, &options).unwrap();
        assert_eq!(fit.weights[50], 0.0);
        assert!(fit.decomposition.remainder[50] > 35.0);
    }

    #[test]
    fn classical_indices_normalised() {
        let y = synthetic(60, 12);
        let add = classical_decomposition(&y, 12, DecompositionType::Additive).unwrap();
        assert!(add.indices.iter().sum::<f64>().abs() < 1e-12);
        assert!(add.components.trend[5].is_nan() && add.components.trend[6].is_finite());
        let mul = classical_decomposition(&y, 12, DecompositionType::Multiplicative).unwrap();
        assert!((mul.indices.iter().sum::<f64>() / 12.0 - 1.0).abs() < 1e-12);
        let odd = centered_moving_average(&[1.0, 2.0, 3.0, 4.0, 5.0], 3).unwrap();
        assert_eq!(odd[1], 2.0);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(stl(&[1.0; 20], &StlOptions::new(12)).is_err());
        assert!(stl(&[1.0; 30], &StlOptions::new(1)).is_err());
        assert!(stl(&[f64::NAN; 30], &StlOptions::new(4)).is_err());
        let bad_degree = StlOptions { trend_degree: 2, ..StlOptions::new(4) };
        assert!(stl(&[1.0; 30], &bad_degree).is_err());
        assert!(classical_decomposition(&[1.0; 10], 12, DecompositionType::Additive).is_err());
        assert!(classical_decomposition(&[0.0; 30], 4, DecompositionType::Multiplicative).is_err());
        assert!(
            seasonal_strength(&Decomposition { period: 2, trend: vec![], seasonal: vec![], remainder: vec![] })
                .is_nan()
        );
    }
}
