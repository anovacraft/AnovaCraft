//! Sample autocorrelation, partial autocorrelation, portmanteau tests,
//! Yule–Walker autoregression and cross-correlation.
//!
//! # Definitions
//!
//! For a series `y₀, …, y_{n−1}` with mean `ȳ` the sample autocovariance at
//! lag `k` uses the divisor `n` (Box, Jenkins & Reinsel, *Time Series
//! Analysis*, §2.1.4), which keeps the sequence positive semi-definite:
//!
//! ```text
//! c_k = (1/n) Σ_{t=0}^{n−1−k} (y_{t+k} − ȳ)(y_t − ȳ),      r_k = c_k / c_0.
//! ```
//!
//! The partial autocorrelations `φ_kk` follow from `r_k` by the
//! Durbin–Levinson recursion (Durbin 1960, *Rev. Int. Statist. Inst.* 28:233):
//!
//! ```text
//! φ_kk = (r_k − Σ_{j<k} φ_{k−1,j} r_{k−j}) / (1 − Σ_{j<k} φ_{k−1,j} r_j),
//! φ_kj = φ_{k−1,j} − φ_kk φ_{k−1,k−j}.
//! ```
//!
//! Approximate confidence limits under white noise are `±z/√n`; for the ACF of
//! a moving average of order `k − 1` Bartlett's (1946) formula gives
//! `Var r_k ≈ (1 + 2 Σ_{j=1}^{k−1} r_j²)/n`.
//!
//! The portmanteau statistics are Box–Pierce (1970, *JASA* 65:1509)
//! `Q = n Σ_{k=1}^{h} r_k²` and Ljung–Box (1978, *Biometrika* 65:297)
//! `Q = n(n+2) Σ_{k=1}^{h} r_k²/(n−k)`, referred to χ² with `h − p` degrees
//! of freedom when `p` parameters were fitted to the series the residuals come
//! from.
//!
//! The cross-correlation of `x` and `y` at lag `k` (positive or negative) is
//!
//! ```text
//! r_xy(k) = (1/n) Σ_t (x_{t+k} − x̄)(y_t − ȳ) / √(c_xx(0) c_yy(0)),
//! ```
//!
//! the sum running over the `t` with both indices in range, so a peak at a
//! positive lag means `x` *follows* `y` (Box, Jenkins & Reinsel §12.1). This is
//! the convention of R's `ccf(x, y)`. Prewhitening (Box, Jenkins & Reinsel
//! §12.2) fits an autoregression `φ(B)` to `x` and filters both series by it
//! before cross-correlating, so that autocorrelation within `x` no longer
//! inflates the cross-correlations.

use stat_core::distributions::{ChiSquared, ContinuousDistribution, Normal};

use crate::{Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn check_series(y: &[f64], context: &'static str, required: usize) -> Result<()> {
    if y.len() < required {
        return Err(StatError::InsufficientData { required, found: y.len() });
    }
    if y.iter().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context });
    }
    Ok(())
}

fn mean(y: &[f64]) -> f64 {
    y.iter().sum::<f64>() / y.len() as f64
}

fn normal_quantile(level: f64) -> Result<f64> {
    if !(level > 0.0 && level < 1.0) {
        return Err(invalid(format!("the confidence level must lie strictly between 0 and 1 (got {level})")));
    }
    Normal::standard().quantile(0.5 + level / 2.0)
}

/// Default maximum lag for a series of length `n`: `⌊10 · log₁₀(n / s)⌋`
/// for `s` series analysed together (1 for an ACF, 2 for a cross-correlation),
/// capped at `n − 1` and at least 1 (when `n ≥ 2`).
#[must_use]
pub fn default_max_lag(n: usize, series: usize) -> usize {
    if n < 2 {
        return 0;
    }
    let lag = (10.0 * (n as f64 / series.max(1) as f64).log10()).floor();
    let lag = if lag.is_finite() && lag >= 1.0 { lag as usize } else { 1 };
    lag.min(n - 1)
}

fn resolve_lag(n: usize, max_lag: Option<usize>, series: usize) -> Result<usize> {
    match max_lag {
        None => Ok(default_max_lag(n, series)),
        Some(0) => Err(invalid("the maximum lag must be at least 1")),
        Some(k) if k >= n => Err(invalid(format!("the maximum lag must be below the series length {n} (got {k})"))),
        Some(k) => Ok(k),
    }
}

/// Sample autocovariances `c₀, …, c_K` (divisor `n`, mean removed).
///
/// # Errors
/// [`StatError::InsufficientData`] for fewer than 2 values,
/// [`StatError::NonFiniteInput`] for NaN or ±∞, and
/// [`StatError::InvalidArgument`] when `max_lag ≥ n`.
pub fn autocovariance(y: &[f64], max_lag: usize) -> Result<Vec<f64>> {
    check_series(y, "series", 2)?;
    if max_lag >= y.len() {
        return Err(invalid(format!("the maximum lag must be below the series length {} (got {max_lag})", y.len())));
    }
    Ok(autocovariance_unchecked(y, max_lag))
}

fn autocovariance_unchecked(y: &[f64], max_lag: usize) -> Vec<f64> {
    let n = y.len();
    let m = mean(y);
    let d: Vec<f64> = y.iter().map(|v| v - m).collect();
    (0..=max_lag).map(|k| (k..n).map(|t| d[t] * d[t - k]).sum::<f64>() / n as f64).collect()
}

/// Sample autocorrelation function.
#[derive(Debug, Clone)]
pub struct Autocorrelation {
    /// Series length `n`.
    pub n: usize,
    /// `r₀ = 1, r₁, …, r_K`, indexed by lag.
    pub values: Vec<f64>,
}

impl Autocorrelation {
    /// Maximum lag `K`.
    #[must_use]
    pub fn max_lag(&self) -> usize {
        self.values.len() - 1
    }

    /// Half-width `z_{(1+level)/2} / √n` of the white-noise limits.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] unless `0 < level < 1`.
    pub fn white_noise_limit(&self, level: f64) -> Result<f64> {
        Ok(normal_quantile(level)? / (self.n as f64).sqrt())
    }

    /// Bartlett half-widths for lags `1..=K`: at lag `k`,
    /// `z · √((1 + 2 Σ_{j=1}^{k−1} r_j²)/n)`, the limit for `r_k` when the
    /// series is a moving average of order `k − 1`.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] unless `0 < level < 1`.
    pub fn bartlett_limits(&self, level: f64) -> Result<Vec<f64>> {
        let z = normal_quantile(level)?;
        let mut sum = 0.0;
        let mut out = Vec::with_capacity(self.max_lag());
        for k in 1..=self.max_lag() {
            out.push(z * ((1.0 + 2.0 * sum) / self.n as f64).sqrt());
            sum += self.values[k] * self.values[k];
        }
        Ok(out)
    }
}

/// Sample autocorrelation `r₀, …, r_K` of `y`; `max_lag = None` uses
/// [`default_max_lag`]`(n, 1)`.
///
/// # Errors
/// [`StatError::InsufficientData`] for fewer than 2 values,
/// [`StatError::NonFiniteInput`] for NaN or ±∞, and
/// [`StatError::InvalidArgument`] when the lag is 0 or `≥ n`, or the series is
/// constant (`c₀ = 0`).
pub fn acf(y: &[f64], max_lag: Option<usize>) -> Result<Autocorrelation> {
    check_series(y, "series", 2)?;
    let k = resolve_lag(y.len(), max_lag, 1)?;
    let c = autocovariance_unchecked(y, k);
    if c[0] <= 0.0 {
        return Err(invalid("the autocorrelation of a constant series is undefined"));
    }
    Ok(Autocorrelation { n: y.len(), values: c.iter().map(|v| v / c[0]).collect() })
}

/// Durbin–Levinson recursion on autocorrelations `r₀ = 1, r₁, …, r_K`.
/// Returns the partial autocorrelations `φ_11, …, φ_KK` and, for each order
/// `p = 0..=K`, the coefficients `φ_p1, …, φ_pp` and the prediction-variance
/// ratio `v_p = Π_{k≤p} (1 − φ_kk²)`.
fn durbin_levinson(r: &[f64]) -> (Vec<f64>, Vec<Vec<f64>>, Vec<f64>) {
    let kmax = r.len() - 1;
    let mut pacf = Vec::with_capacity(kmax);
    let mut coefs: Vec<Vec<f64>> = vec![Vec::new()];
    let mut ratios = vec![1.0];
    let mut phi: Vec<f64> = Vec::new();
    let mut v = 1.0;
    for k in 1..=kmax {
        let num = r[k] - (0..k - 1).map(|j| phi[j] * r[k - 1 - j]).sum::<f64>();
        let akk = if v > 0.0 { num / v } else { 0.0 };
        let mut next = vec![0.0; k];
        for j in 0..k - 1 {
            next[j] = phi[j] - akk * phi[k - 2 - j];
        }
        next[k - 1] = akk;
        v *= 1.0 - akk * akk;
        phi = next;
        pacf.push(akk);
        coefs.push(phi.clone());
        ratios.push(v);
    }
    (pacf, coefs, ratios)
}

/// Sample partial autocorrelation function.
#[derive(Debug, Clone)]
pub struct PartialAutocorrelation {
    /// Series length `n`.
    pub n: usize,
    /// `φ_11, …, φ_KK`; element `k − 1` is lag `k`.
    pub values: Vec<f64>,
}

impl PartialAutocorrelation {
    /// Half-width `z / √n` of the white-noise limits (Quenouille 1949).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] unless `0 < level < 1`.
    pub fn white_noise_limit(&self, level: f64) -> Result<f64> {
        Ok(normal_quantile(level)? / (self.n as f64).sqrt())
    }
}

/// Sample partial autocorrelation at lags `1..=K` by the Durbin–Levinson
/// recursion on the sample ACF; `max_lag = None` uses [`default_max_lag`].
///
/// # Errors
/// As [`acf`].
pub fn pacf(y: &[f64], max_lag: Option<usize>) -> Result<PartialAutocorrelation> {
    let r = acf(y, max_lag)?;
    Ok(PartialAutocorrelation { n: r.n, values: durbin_levinson(&r.values).0 })
}

/// Which portmanteau statistic to compute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortmanteauKind {
    /// Ljung–Box (1978): `n(n+2) Σ r_k²/(n−k)`.
    LjungBox,
    /// Box–Pierce (1970): `n Σ r_k²`.
    BoxPierce,
}

/// One portmanteau test of no autocorrelation up to a lag.
#[derive(Debug, Clone)]
pub struct PortmanteauTest {
    /// Statistic used.
    pub kind: PortmanteauKind,
    /// Number of lags `h` pooled.
    pub lag: usize,
    /// The statistic `Q`.
    pub statistic: f64,
    /// Degrees of freedom `h − p`.
    pub df: usize,
    /// Upper-tail χ² probability of `Q`.
    pub p_value: f64,
}

/// Portmanteau tests of `y` at each lag in `lags`, with `fitted_parameters`
/// (`p`) subtracted from the degrees of freedom (for residuals of an ARMA(p, q)
/// model pass `p + q`).
///
/// # Errors
/// As [`acf`], plus [`StatError::InvalidArgument`] when `lags` is empty or a
/// lag is not greater than `fitted_parameters` or not below `n`.
pub fn portmanteau(
    y: &[f64],
    lags: &[usize],
    fitted_parameters: usize,
    kind: PortmanteauKind,
) -> Result<Vec<PortmanteauTest>> {
    let &max = lags.iter().max().ok_or_else(|| invalid("at least one lag is needed"))?;
    if let Some(&bad) = lags.iter().find(|&&h| h <= fitted_parameters) {
        return Err(invalid(format!(
            "each lag must exceed the number of fitted parameters {fitted_parameters} (got {bad})"
        )));
    }
    let r = acf(y, Some(max))?;
    let n = y.len() as f64;
    lags.iter()
        .map(|&h| {
            let statistic = match kind {
                PortmanteauKind::BoxPierce => n * (1..=h).map(|k| r.values[k].powi(2)).sum::<f64>(),
                PortmanteauKind::LjungBox => {
                    n * (n + 2.0) * (1..=h).map(|k| r.values[k].powi(2) / (n - k as f64)).sum::<f64>()
                }
            };
            let df = h - fitted_parameters;
            let p_value = ChiSquared::new(df as f64)?.sf(statistic)?;
            Ok(PortmanteauTest { kind, lag: h, statistic, df, p_value })
        })
        .collect()
}

/// An autoregression fitted by the Yule–Walker equations.
#[derive(Debug, Clone)]
pub struct ArFit {
    /// Order `p`.
    pub order: usize,
    /// Coefficients `φ₁, …, φ_p` of `y_t − μ = Σ φ_i (y_{t−i} − μ) + e_t`.
    pub coefficients: Vec<f64>,
    /// Series mean `μ` removed before fitting.
    pub mean: f64,
    /// Innovation variance `c₀ Π_{k≤p}(1 − φ_kk²) · n/(n − (p+1))`
    /// (the Yule–Walker prediction variance with a degrees-of-freedom
    /// correction for the `p` coefficients and the mean).
    pub innovation_variance: f64,
    /// `AIC_k − min AIC` for orders `k = 0..=max_order`, where
    /// `AIC_k = n log(c₀ Π_{j≤k}(1 − φ_jj²)) + 2k`.
    pub aic: Vec<f64>,
}

impl ArFit {
    /// Apply the filter `φ(B) = 1 − φ₁B − … − φ_pB^p` to `x`:
    /// `e_t = x_t − Σ φ_i x_{t−i}` for `t = p..n`, returning `n − p` values
    /// (no mean is removed; a constant shift of `x` shifts every `e_t` by the
    /// same amount).
    #[must_use]
    pub fn filter(&self, x: &[f64]) -> Vec<f64> {
        let p = self.order;
        (p..x.len()).map(|t| x[t] - (1..=p).map(|i| self.coefficients[i - 1] * x[t - i]).sum::<f64>()).collect()
    }
}

/// Fit an autoregression to `y` by the Yule–Walker equations (solved by
/// Durbin–Levinson on the sample ACF). With `order = Some(p)` that order is
/// fitted; with `None` the order in `0..=max_order` minimizing the AIC is
/// chosen, `max_order` defaulting to [`default_max_lag`]`(n, 1)`, capped at
/// `n − 2` so that short series (n = 3 to 11) have a usable default.
///
/// # Errors
/// As [`acf`]; [`StatError::InvalidArgument`] when the order is not below
/// `n − 1`.
pub fn yule_walker(y: &[f64], order: Option<usize>, max_order: Option<usize>) -> Result<ArFit> {
    check_series(y, "series", 3)?;
    let n = y.len();
    let kmax = match order {
        Some(p) => p,
        None => max_order.unwrap_or_else(|| default_max_lag(n, 1).min(n - 2)),
    };
    if kmax + 1 >= n {
        return Err(invalid(format!("the autoregressive order must be below n − 1 = {} (got {kmax})", n - 1)));
    }
    let c = autocovariance_unchecked(y, kmax.max(1));
    if c[0] <= 0.0 {
        return Err(invalid("cannot fit an autoregression to a constant series"));
    }
    let r: Vec<f64> = c[..=kmax].iter().map(|v| v / c[0]).collect();
    let (_, coefs, ratios) = durbin_levinson(&r);
    let raw: Vec<f64> = ratios.iter().enumerate().map(|(k, v)| n as f64 * (c[0] * v).ln() + 2.0 * k as f64).collect();
    let best = raw.iter().copied().fold(f64::INFINITY, f64::min);
    let aic: Vec<f64> = raw.iter().map(|a| a - best).collect();
    let p = match order {
        Some(p) => p,
        None => aic.iter().position(|&a| a == 0.0).unwrap_or(0),
    };
    let innovation_variance = c[0] * ratios[p] * n as f64 / (n - (p + 1)) as f64;
    Ok(ArFit { order: p, coefficients: coefs[p].clone(), mean: mean(y), innovation_variance, aic })
}

/// Sample cross-correlation function of two series.
#[derive(Debug, Clone)]
pub struct CrossCorrelation {
    /// Number of paired observations `n`.
    pub n: usize,
    /// Lags `−K, …, K`.
    pub lags: Vec<i64>,
    /// `r_xy(k)` for each lag: the correlation of `x_{t+k}` with `y_t`.
    pub values: Vec<f64>,
}

impl CrossCorrelation {
    /// Half-width `z / √n` of the limits for two independent white-noise series.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] unless `0 < level < 1`.
    pub fn white_noise_limit(&self, level: f64) -> Result<f64> {
        Ok(normal_quantile(level)? / (self.n as f64).sqrt())
    }
}

/// Cross-correlation `r_xy(k)` for `k = −K..=K` of two equal-length series
/// (see the module docs for the sign convention); `max_lag = None` uses
/// [`default_max_lag`]`(n, 2)`.
///
/// # Errors
/// [`StatError::DimensionMismatch`] for unequal lengths, otherwise as [`acf`]
/// for each series.
pub fn cross_correlation(x: &[f64], y: &[f64], max_lag: Option<usize>) -> Result<CrossCorrelation> {
    if x.len() != y.len() {
        return Err(StatError::DimensionMismatch {
            context: "cross-correlation series lengths",
            expected: x.len(),
            found: y.len(),
        });
    }
    check_series(x, "first series", 2)?;
    check_series(y, "second series", 2)?;
    let n = x.len();
    let k = resolve_lag(n, max_lag, 2)?;
    let (mx, my) = (mean(x), mean(y));
    let dx: Vec<f64> = x.iter().map(|v| v - mx).collect();
    let dy: Vec<f64> = y.iter().map(|v| v - my).collect();
    let sx = dx.iter().map(|v| v * v).sum::<f64>();
    let sy = dy.iter().map(|v| v * v).sum::<f64>();
    if sx <= 0.0 || sy <= 0.0 {
        return Err(invalid("the cross-correlation of a constant series is undefined"));
    }
    let scale = (sx * sy).sqrt();
    let k = k as i64;
    let lags: Vec<i64> = (-k..=k).collect();
    let values = lags
        .iter()
        .map(|&lag| {
            let s: f64 = if lag >= 0 {
                let l = lag as usize;
                (0..n - l).map(|t| dx[t + l] * dy[t]).sum()
            } else {
                let l = (-lag) as usize;
                (0..n - l).map(|t| dx[t] * dy[t + l]).sum()
            };
            s / scale
        })
        .collect();
    Ok(CrossCorrelation { n, lags, values })
}

/// Cross-correlation after prewhitening, with the autoregression used.
#[derive(Debug, Clone)]
pub struct PrewhitenedCrossCorrelation {
    /// Autoregression fitted to `x` and applied to both series.
    pub filter: ArFit,
    /// Cross-correlation of the filtered series (length `n − p`).
    pub ccf: CrossCorrelation,
}

/// Prewhitened cross-correlation: fit an autoregression to `x` by
/// [`yule_walker`] (`ar_order = None` chooses the order by AIC), filter both
/// series with [`ArFit::filter`], then [`cross_correlation`] of the results.
///
/// # Errors
/// As [`yule_walker`] and [`cross_correlation`]; also when the filtered series
/// are too short for the requested lag.
pub fn prewhitened_cross_correlation(
    x: &[f64],
    y: &[f64],
    max_lag: Option<usize>,
    ar_order: Option<usize>,
) -> Result<PrewhitenedCrossCorrelation> {
    if x.len() != y.len() {
        return Err(StatError::DimensionMismatch {
            context: "cross-correlation series lengths",
            expected: x.len(),
            found: y.len(),
        });
    }
    check_series(y, "second series", 3)?;
    let filter = yule_walker(x, ar_order, None)?;
    let ccf = cross_correlation(&filter.filter(x), &filter.filter(y), max_lag)?;
    Ok(PrewhitenedCrossCorrelation { filter, ccf })
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_core::rng::Rng;

    /// Short series (n = 3 to 11) have a usable default order (capped at
    /// n − 2), as R's `ar.yw` fits them: R 4.3.3 chooses order 0 with
    /// innovation variance 2.5 for (1, 3, 2, 5, 4) and 3.2380952380952386 for
    /// (1, 3, 2, 5, 4, 6, 5). Prewhitening a short series works too.
    #[test]
    fn short_series_default_order() {
        let a = yule_walker(&[1.0, 3.0, 2.0, 5.0, 4.0], None, None).unwrap();
        assert_eq!(a.order, 0);
        assert!((a.innovation_variance - 2.5).abs() < 1e-12);
        let b = yule_walker(&[1.0, 3.0, 2.0, 5.0, 4.0, 6.0, 5.0], None, None).unwrap();
        assert_eq!(b.order, 0);
        assert!((b.innovation_variance - 3.238_095_238_095_238_6).abs() < 1e-12);
        for n in 3..=11 {
            let y: Vec<f64> = (0..n).map(|i| ((i * 7) % 5) as f64 + 0.1 * i as f64).collect();
            assert!(yule_walker(&y, None, None).is_ok(), "n = {n}");
        }
        let x = [1.0, 3.0, 2.0, 5.0, 4.0, 6.0];
        let y = [2.0, 1.0, 4.0, 3.0, 6.0, 5.0];
        assert!(prewhitened_cross_correlation(&x, &y, None, None).is_ok());
    }

    #[test]
    fn white_noise_acf_within_limits_mostly() {
        let mut rng = Rng::new(7);
        let y: Vec<f64> = (0..2000).map(|_| rng.normal()).collect();
        let r = acf(&y, Some(20)).unwrap();
        let lim = r.white_noise_limit(0.99).unwrap();
        assert_eq!(r.values[0], 1.0);
        assert!(r.values[1..].iter().filter(|v| v.abs() > lim).count() <= 2);
        let q = portmanteau(&y, &[10], 0, PortmanteauKind::LjungBox).unwrap();
        assert!(q[0].p_value > 0.001);
    }

    #[test]
    fn ar1_pacf_cuts_off() {
        let mut rng = Rng::new(3);
        let mut y = vec![0.0; 3000];
        for t in 1..y.len() {
            y[t] = 0.7 * y[t - 1] + rng.normal();
        }
        let p = pacf(&y, Some(5)).unwrap();
        assert!((p.values[0] - 0.7).abs() < 0.05);
        assert!(p.values[1..].iter().all(|v| v.abs() < 0.06));
        let fit = yule_walker(&y, None, Some(8)).unwrap();
        assert_eq!(fit.order, 1);
        assert!((fit.innovation_variance - 1.0).abs() < 0.1);
    }

    #[test]
    fn ccf_sign_convention() {
        // x is y delayed by 2: x_t = y_{t−2}, so x_{t+2} = y_t and the peak is at lag +2.
        let mut rng = Rng::new(11);
        let y: Vec<f64> = (0..300).map(|_| rng.normal()).collect();
        let x: Vec<f64> = (0..300).map(|t| if t >= 2 { y[t - 2] } else { 0.0 }).collect();
        let c = cross_correlation(&x, &y, Some(5)).unwrap();
        let best = c.values.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap().0;
        assert_eq!(c.lags[best], 2);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(acf(&[1.0], None).is_err());
        assert!(acf(&[1.0, f64::NAN, 2.0], None).is_err());
        assert!(acf(&[2.0; 10], None).is_err());
        assert!(acf(&[1.0, 2.0, 3.0], Some(3)).is_err());
        assert!(portmanteau(&[1.0, 2.0, 4.0, 3.0, 5.0], &[2], 2, PortmanteauKind::LjungBox).is_err());
        assert!(cross_correlation(&[1.0, 2.0], &[1.0, 2.0, 3.0], None).is_err());
    }
}
