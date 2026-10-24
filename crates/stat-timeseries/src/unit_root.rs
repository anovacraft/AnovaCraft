//! Unit-root and stationarity tests: KPSS and augmented Dickey–Fuller.
//!
//! # KPSS (Kwiatkowski, Phillips, Schmidt & Shin 1992)
//! "Testing the null hypothesis of stationarity against the alternative of a
//! unit root", *Journal of Econometrics* 54, 159–178. With `eₜ` the residuals
//! of `y` on a constant (level stationarity) or on a constant and the time
//! index (trend stationarity), partial sums `Sₜ = e₁ + … + eₜ` and the
//! Bartlett-window long-run variance with truncation lag `l`
//!
//! ```text
//! s²(l) = (1/n) Σₜ eₜ² + (2/n) Σ_{s=1}^{l} (1 − s/(l+1)) Σ_{t=s+1}^{n} eₜ e_{t−s},
//! η = Σₜ Sₜ² / (n² s²(l)).
//! ```
//!
//! Large `η` rejects stationarity. The p-value is interpolated linearly in the
//! paper's table of asymptotic upper-tail critical values (level: 0.347,
//! 0.463, 0.574, 0.739; trend: 0.119, 0.146, 0.176, 0.216 at 10, 5, 2.5 and
//! 1 %), and clipped to `[0.01, 0.10]` outside the table.
//! The default lag is the "short" rule `l = ⌊4 (n/100)^{1/4}⌋`; the "long"
//! rule `⌊12 (n/100)^{1/4}⌋` is available through [`kpss_lag`] (both from
//! Schwert 1989, *J. Business & Economic Statistics* 7, used in the paper).
//!
//! # Augmented Dickey–Fuller
//! Dickey & Fuller (1979), "Distribution of the estimators for autoregressive
//! time series with a unit root", *JASA* 74, 427–431; Said & Dickey (1984),
//! "Testing for unit roots in autoregressive-moving average models of
//! unknown order", *Biometrika* 71, 599–607. The regression
//!
//! ```text
//! Δyₜ = [α] + [β t] + γ y_{t−1} + Σ_{i=1}^{k} δᵢ Δy_{t−i} + εₜ
//! ```
//!
//! is fitted by least squares and the test statistic is `τ = γ̂ / se(γ̂)`
//! (a unit root means `γ = 0`; large negative `τ` rejects it). The number of
//! lagged differences `k` is fixed or chosen to minimise AIC
//! (`m ln(RSS/m) + 2p`) or BIC (`m ln(RSS/m) + p ln m`) over `k = 0, …, K` (or from a
//! given smallest lag),
//! every candidate fitted on the same (common) sample, the observations
//! available with `K` lags. The chosen regression is then reported either on
//! that common sample or refitted on all observations its `k` allows
//! ([`AdfSample`]).
//!
//! p-values use MacKinnon's approximate asymptotic distribution functions
//! (MacKinnon 1994, "Approximate asymptotic distribution functions for
//! unit-root and cointegration tests", *J. Business & Economic Statistics* 12,
//! 167–176; the approach of MacKinnon 1996, *J. Applied Econometrics* 11,
//! 601–618): `p = Φ(γ₀ + γ₁τ + γ₂τ²)` for `τ ≤ τ*` and
//! `p = Φ(γ₀ + γ₁τ + γ₂τ² + γ₃τ³)` above, 0 below `τ_min` and 1 above
//! `τ_max`. Critical values use the finite-sample response surfaces of
//! MacKinnon (2010), "Critical values for cointegration tests", Queen's
//! Economics Department Working Paper 1227, Tables 2–3 (`N = 1`):
//! `C(T) = β∞ + β₁/T + β₂/T² + β₃/T³` with `T` the number of observations in
//! the regression.

use crate::trend::{check_finite, ols};
use crate::{Result, StatError};
use stat_core::distributions::{ContinuousDistribution, Normal};

/// Null hypothesis of the KPSS test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KpssNull {
    /// Stationary around a constant level.
    Level,
    /// Stationary around a linear trend.
    Trend,
}

/// Rule for the Bartlett truncation lag of the KPSS long-run variance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KpssLagRule {
    /// `⌊4 (n/100)^{1/4}⌋` (the default of [`kpss`]).
    Short,
    /// `⌊12 (n/100)^{1/4}⌋`.
    Long,
}

/// Truncation lag of `rule` for a series of length `n`.
#[must_use]
pub fn kpss_lag(n: usize, rule: KpssLagRule) -> usize {
    let c = match rule {
        KpssLagRule::Short => 4.0,
        KpssLagRule::Long => 12.0,
    };
    (c * (n as f64 / 100.0).powf(0.25)).trunc() as usize
}

/// Result of a KPSS test (Kwiatkowski, Phillips, Schmidt & Shin 1992).
#[derive(Debug, Clone)]
pub struct KpssTest {
    /// Null hypothesis tested.
    pub null: KpssNull,
    /// The KPSS statistic η.
    pub statistic: f64,
    /// Bartlett-window truncation lag used for the long-run variance.
    pub lag: usize,
    /// p-value interpolated in the published table (clipped to [0.01, 0.10]).
    pub p_value: f64,
    /// Long-run variance estimate `s²(l)`.
    pub long_run_variance: f64,
    /// Asymptotic critical values `(significance level, value)` at 10, 5,
    /// 2.5 and 1 %.
    pub critical_values: [(f64, f64); 4],
    /// Number of observations.
    pub n: usize,
}

const KPSS_LEVELS: [f64; 4] = [0.10, 0.05, 0.025, 0.01];
const KPSS_LEVEL_CRIT: [f64; 4] = [0.347, 0.463, 0.574, 0.739];
const KPSS_TREND_CRIT: [f64; 4] = [0.119, 0.146, 0.176, 0.216];

/// KPSS test of `y` for level or trend stationarity (see the module
/// documentation). `lag = None` uses the short rule `⌊4 (n/100)^{1/4}⌋`
/// ([`kpss_lag`] with [`KpssLagRule::Short`]).
///
/// # Errors
/// * [`StatError::InsufficientData`] with fewer than 4 observations.
/// * [`StatError::NonFiniteInput`] for NaN or infinite values.
/// * [`StatError::InvalidArgument`] for a lag of `n` or more, or a constant
///   series (zero long-run variance).
pub fn kpss(y: &[f64], null: KpssNull, lag: Option<usize>) -> Result<KpssTest> {
    let n = y.len();
    if n < 4 {
        return Err(StatError::InsufficientData { required: 4, found: n });
    }
    check_finite(y, "series")?;
    let lag = lag.unwrap_or_else(|| kpss_lag(n, KpssLagRule::Short));
    if lag >= n {
        return Err(StatError::InvalidArgument {
            message: format!("the KPSS lag ({lag}) must be smaller than the series length ({n})"),
        });
    }
    let mut columns = vec![vec![1.0; n]];
    if null == KpssNull::Trend {
        columns.push((1..=n).map(|t| t as f64).collect());
    }
    let e = ols(&columns, y)?.residuals;
    let scale = y.iter().fold(0.0f64, |m, v| m.max(v.abs())).max(f64::MIN_POSITIVE);
    if e.iter().all(|v| v.abs() <= 1e-12 * scale) {
        return Err(StatError::InvalidArgument {
            message: "the series is exactly constant (or exactly linear) under the KPSS null".into(),
        });
    }
    let nf = n as f64;
    let mut s2 = stat_core::summation::dot(&e, &e) / nf;
    for s in 1..=lag {
        let w = 1.0 - s as f64 / (lag as f64 + 1.0);
        s2 += 2.0 / nf * w * stat_core::summation::dot(&e[s..], &e[..n - s]);
    }
    if s2.is_nan() || s2 <= 0.0 {
        return Err(StatError::InvalidArgument {
            message: "the long-run variance is not positive (constant series or too long a lag)".into(),
        });
    }
    let mut partial = 0.0;
    let mut ss = 0.0;
    for v in &e {
        partial += v;
        ss += partial * partial;
    }
    let statistic = ss / (nf * nf * s2);
    let table = if null == KpssNull::Level { KPSS_LEVEL_CRIT } else { KPSS_TREND_CRIT };
    let p_value = interpolate_p(statistic, &table);
    let mut critical_values = [(0.0, 0.0); 4];
    for i in 0..4 {
        critical_values[i] = (KPSS_LEVELS[i], table[i]);
    }
    Ok(KpssTest { null, statistic, lag, p_value, long_run_variance: s2, critical_values, n })
}

fn interpolate_p(stat: f64, crit: &[f64; 4]) -> f64 {
    if stat <= crit[0] {
        return KPSS_LEVELS[0];
    }
    if stat >= crit[3] {
        return KPSS_LEVELS[3];
    }
    for i in 0..3 {
        if stat <= crit[i + 1] {
            let f = (stat - crit[i]) / (crit[i + 1] - crit[i]);
            return KPSS_LEVELS[i] + f * (KPSS_LEVELS[i + 1] - KPSS_LEVELS[i]);
        }
    }
    KPSS_LEVELS[3]
}

/// Deterministic terms of the Dickey–Fuller regression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdfDeterministic {
    /// No constant (random walk without drift under the null).
    None,
    /// A constant.
    Constant,
    /// A constant and a linear time trend.
    ConstantTrend,
}

impl AdfDeterministic {
    fn count(self) -> usize {
        match self {
            Self::None => 0,
            Self::Constant => 1,
            Self::ConstantTrend => 2,
        }
    }
}

/// How the number of lagged differences is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdfLags {
    /// Exactly this many lagged differences.
    Fixed(usize),
    /// Minimise AIC over `0..=max`; `None` uses `⌊12 (n/100)^{1/4}⌋`.
    Aic(Option<usize>),
    /// Minimise BIC over `0..=max`; `None` uses `⌊12 (n/100)^{1/4}⌋`.
    Bic(Option<usize>),
}

/// Sample on which the selected Dickey–Fuller regression is reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdfSample {
    /// Refit the chosen lag on every observation it allows (`n − 1 − k`
    /// regression rows). For fixed lags the two choices coincide.
    Maximal,
    /// Keep the common sample of the lag search (`n − 1 − K` rows).
    Common,
}

/// Settings of [`adf`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdfOptions {
    /// Deterministic terms (default: constant).
    pub deterministic: AdfDeterministic,
    /// Lag choice (default: AIC with the default maximum).
    pub lags: AdfLags,
    /// Sample of the reported regression (default: maximal).
    pub sample: AdfSample,
    /// Smallest lag the AIC/BIC search considers (default 0; urca's `ur.df`
    /// searches from 1). Ignored for a fixed lag.
    pub min_lag: usize,
}

impl Default for AdfOptions {
    fn default() -> Self {
        Self {
            deterministic: AdfDeterministic::Constant,
            lags: AdfLags::Aic(None),
            sample: AdfSample::Maximal,
            min_lag: 0,
        }
    }
}

/// Result of an augmented Dickey–Fuller test.
#[derive(Debug, Clone, PartialEq)]
pub struct AdfTest {
    /// Deterministic terms of the regression.
    pub deterministic: AdfDeterministic,
    /// Number of lagged differences `k` used.
    pub lag: usize,
    /// The τ statistic `γ̂ / se(γ̂)`.
    pub statistic: f64,
    /// MacKinnon approximate asymptotic p-value.
    pub p_value: f64,
    /// Critical values at 1, 5 and 10 % (MacKinnon 2010 response surface at
    /// `T = nobs`).
    pub critical_values: [f64; 3],
    /// Estimated `γ` (coefficient of `y_{t−1}`).
    pub gamma: f64,
    /// Standard error of `γ̂`.
    pub gamma_std_error: f64,
    /// Number of observations in the regression.
    pub nobs: usize,
    /// Information criterion of each candidate lag `min_lag..=K` on the
    /// common sample (empty for a fixed lag).
    pub criterion: Vec<f64>,
}

struct AdfRegression {
    gamma: f64,
    se: f64,
    rss: f64,
    nobs: usize,
    params: usize,
}

/// Fit the ADF regression with `k` lags on rows `t = start..n` (0-based
/// index of `y`, `start ≥ k + 1`).
fn adf_regression(y: &[f64], det: AdfDeterministic, k: usize, start: usize) -> Result<AdfRegression> {
    let n = y.len();
    let rows = start..n;
    let dy = |t: usize| y[t] - y[t - 1];
    let mut columns: Vec<Vec<f64>> = vec![rows.clone().map(|t| y[t - 1]).collect()];
    if det != AdfDeterministic::None {
        columns.push(vec![1.0; n - start]);
    }
    if det == AdfDeterministic::ConstantTrend {
        columns.push(rows.clone().map(|t| t as f64).collect());
    }
    for i in 1..=k {
        columns.push(rows.clone().map(|t| dy(t - i)).collect());
    }
    let response: Vec<f64> = rows.map(dy).collect();
    let fit = ols(&columns, &response)?;
    Ok(AdfRegression { gamma: fit.beta[0], se: fit.std_error(0), rss: fit.rss, nobs: n - start, params: columns.len() })
}

/// Augmented Dickey–Fuller test of `y` (see the module documentation).
///
/// # Errors
/// * [`StatError::NonFiniteInput`] for NaN or infinite values.
/// * [`StatError::InsufficientData`] when the regression would have no
///   residual degrees of freedom: it uses the `n − K − 1` rows after the
///   largest lag `K` and `K + d + 1` regressors (`d` deterministic terms), so
///   `2K + d + 3` observations are needed.
/// * [`StatError::RankDeficient`] for a degenerate (e.g. constant) series.
/// * [`StatError::InvalidArgument`] when the regression fits the differences
///   exactly (an exact line, say), so the statistic is undefined.
pub fn adf(y: &[f64], options: &AdfOptions) -> Result<AdfTest> {
    check_finite(y, "series")?;
    let n = y.len();
    let det = options.deterministic;
    let default_max = || kpss_lag(n, KpssLagRule::Long);
    let (max_lag, use_bic) = match options.lags {
        AdfLags::Fixed(k) => (k, None),
        AdfLags::Aic(m) => (m.unwrap_or_else(default_max), Some(false)),
        AdfLags::Bic(m) => (m.unwrap_or_else(default_max), Some(true)),
    };
    let required = 2 * max_lag + det.count() + 3;
    if n < required {
        return Err(StatError::InsufficientData { required, found: n });
    }
    let mut criterion = Vec::new();
    let lag = match use_bic {
        None => max_lag,
        Some(bic) => {
            let mut best = (f64::INFINITY, options.min_lag);
            if options.min_lag > max_lag {
                return Err(StatError::InvalidArgument {
                    message: format!("the smallest lag ({}) exceeds the largest ({max_lag})", options.min_lag),
                });
            }
            for k in options.min_lag..=max_lag {
                let r = adf_regression(y, det, k, max_lag + 1)?;
                let m = r.nobs as f64;
                let penalty = if bic { m.ln() } else { 2.0 };
                let ic = m * (r.rss / m).ln() + penalty * r.params as f64;
                if ic < best.0 {
                    best = (ic, k);
                }
                criterion.push(ic);
            }
            best.1
        }
    };
    let start = match options.sample {
        AdfSample::Maximal => lag + 1,
        AdfSample::Common => max_lag + 1,
    };
    let r = adf_regression(y, det, lag, start)?;
    let statistic = r.gamma / r.se;
    if !statistic.is_finite() {
        return Err(StatError::InvalidArgument {
            message: "the test regression fits the series' differences exactly (the series is an exact line or \
                      follows an exact recursion), so the Dickey–Fuller statistic is undefined"
                .to_owned(),
        });
    }
    let critical_values = [
        mackinnon_critical_value(0.01, det, Some(r.nobs))?,
        mackinnon_critical_value(0.05, det, Some(r.nobs))?,
        mackinnon_critical_value(0.10, det, Some(r.nobs))?,
    ];
    Ok(AdfTest {
        deterministic: det,
        lag,
        statistic,
        p_value: mackinnon_p_value(statistic, det),
        critical_values,
        gamma: r.gamma,
        gamma_std_error: r.se,
        nobs: r.nobs,
        criterion,
    })
}

/// MacKinnon (1994) coefficients for one variable (`N = 1`):
/// `(τ_min, τ*, τ_max, small-p γ₀..γ₂, large-p γ₀..γ₃)`.
struct MacKinnonTau {
    tau_min: f64,
    tau_star: f64,
    tau_max: f64,
    small: [f64; 3],
    large: [f64; 4],
}

fn mackinnon_tau(det: AdfDeterministic) -> MacKinnonTau {
    match det {
        AdfDeterministic::None => MacKinnonTau {
            tau_min: -19.04,
            tau_star: -1.04,
            tau_max: f64::INFINITY,
            small: [0.6344, 1.2378, 0.032496],
            large: [0.4797, 0.93557, -0.06999, 0.033066],
        },
        AdfDeterministic::Constant => MacKinnonTau {
            tau_min: -18.83,
            tau_star: -1.61,
            tau_max: 2.74,
            small: [2.1659, 1.4412, 0.038269],
            large: [1.7339, 0.93202, -0.12745, -0.010368],
        },
        AdfDeterministic::ConstantTrend => MacKinnonTau {
            tau_min: -16.18,
            tau_star: -2.89,
            tau_max: 0.70,
            small: [3.2512, 1.6047, 0.049588],
            large: [2.5261, 0.61654, -0.37956, -0.060285],
        },
    }
}

/// MacKinnon's approximate asymptotic p-value of a Dickey–Fuller τ statistic
/// (see the module documentation). Returns NaN for a NaN statistic.
#[must_use]
pub fn mackinnon_p_value(tau: f64, det: AdfDeterministic) -> f64 {
    if tau.is_nan() {
        return f64::NAN;
    }
    let c = mackinnon_tau(det);
    if tau > c.tau_max {
        return 1.0;
    }
    if tau < c.tau_min {
        return 0.0;
    }
    let z = if tau <= c.tau_star {
        c.small[0] + tau * (c.small[1] + tau * c.small[2])
    } else {
        c.large[0] + tau * (c.large[1] + tau * (c.large[2] + tau * c.large[3]))
    };
    Normal::standard().cdf(z).unwrap_or(f64::NAN)
}

/// Dickey–Fuller τ critical value at significance `level` (0.01, 0.05 or
/// 0.10) for a regression with `nobs` observations (`None`: asymptotic), from
/// MacKinnon's (2010) response surfaces.
///
/// # Errors
/// [`StatError::InvalidArgument`] for any other level or `nobs = Some(0)`.
pub fn mackinnon_critical_value(level: f64, det: AdfDeterministic, nobs: Option<usize>) -> Result<f64> {
    let row = [0.01, 0.05, 0.10].iter().position(|&l| (l - level).abs() < 1e-12).ok_or_else(|| {
        StatError::InvalidArgument { message: "critical values are tabulated at the 1, 5 and 10% levels".into() }
    })?;
    let table: [[f64; 4]; 3] = match det {
        AdfDeterministic::None => {
            [[-2.56574, -2.2358, -3.627, 0.0], [-1.94100, -0.2686, -3.365, 31.223], [-1.61682, 0.2656, -2.714, 25.364]]
        }
        AdfDeterministic::Constant => [
            [-3.43035, -6.5393, -16.786, -79.433],
            [-2.86154, -2.8903, -4.234, -40.040],
            [-2.56677, -1.5384, -2.809, 0.0],
        ],
        AdfDeterministic::ConstantTrend => [
            [-3.95877, -9.0531, -28.428, -134.155],
            [-3.41049, -4.3904, -9.036, -45.374],
            [-3.12705, -2.5856, -3.925, -22.380],
        ],
    };
    let b = table[row];
    match nobs {
        None => Ok(b[0]),
        Some(0) => Err(StatError::InvalidArgument { message: "the sample size must be positive".into() }),
        Some(t) => {
            let u = 1.0 / t as f64;
            Ok(b[0] + u * (b[1] + u * (b[2] + u * b[3])))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The precheck names the real number of observations the regression
    /// needs, 2K + d + 3 (rows after the largest lag K, minus K + d + 1
    /// regressors, leaving one residual degree of freedom), and an exact
    /// line gives a clear error instead of a NaN statistic.
    #[test]
    fn adf_required_observations_and_exact_fit() {
        let opts = AdfOptions { lags: AdfLags::Fixed(3), ..AdfOptions::default() };
        let y: Vec<f64> = (0..12).map(|i| ((i * 7) % 5) as f64 + 0.3 * i as f64).collect();
        match adf(&y[..9], &opts) {
            Err(StatError::InsufficientData { required, found }) => assert_eq!((required, found), (10, 9)),
            other => panic!("{other:?}"),
        }
        assert!(adf(&y[..10], &opts).is_ok());
        let line: Vec<f64> = (0..20).map(|i| 2.0 + 0.5 * i as f64).collect();
        let e = adf(&line, &AdfOptions { lags: AdfLags::Fixed(0), ..AdfOptions::default() }).unwrap_err();
        assert!(e.to_string().contains("exactly"), "{e}");
    }

    fn lcg(n: usize, seed: u64) -> Vec<f64> {
        // Approximately normal noise from a sum of uniforms.
        let mut s = seed;
        (0..n)
            .map(|_| {
                let mut acc = 0.0;
                for _ in 0..12 {
                    s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                    acc += (s >> 11) as f64 / (1u64 << 53) as f64;
                }
                acc - 6.0
            })
            .collect()
    }

    #[test]
    fn lag_rules() {
        assert_eq!(kpss_lag(114, KpssLagRule::Short), 4);
        assert_eq!(kpss_lag(114, KpssLagRule::Long), 12);
        assert_eq!(kpss_lag(100, KpssLagRule::Long), 12);
    }

    #[test]
    fn white_noise_versus_random_walk() {
        let e = lcg(400, 7);
        let mut walk = Vec::with_capacity(e.len());
        let mut acc = 0.0;
        for v in &e {
            acc += v;
            walk.push(acc);
        }
        let opts = AdfOptions::default();
        assert!(adf(&e, &opts).unwrap().p_value < 0.01);
        assert!(adf(&walk, &opts).unwrap().p_value > 0.05);
        assert!(kpss(&e, KpssNull::Level, None).unwrap().p_value >= 0.05);
        assert!((kpss(&walk, KpssNull::Level, None).unwrap().p_value - 0.01).abs() < 1e-15);
    }

    #[test]
    fn p_values_are_monotone_and_bounded() {
        for det in [AdfDeterministic::None, AdfDeterministic::Constant, AdfDeterministic::ConstantTrend] {
            let mut last = 0.0;
            for i in 0..=400 {
                let tau = -20.0 + i as f64 * 0.06;
                let p = mackinnon_p_value(tau, det);
                assert!((0.0..=1.0).contains(&p));
                // The two pieces meet only approximately at τ*.
                assert!(p >= last - 2e-3, "{det:?} {tau}");
                last = p;
            }
            // At the 5% critical value the p-value is close to 0.05.
            let c = mackinnon_critical_value(0.05, det, None).unwrap();
            assert!((mackinnon_p_value(c, det) - 0.05).abs() < 5e-3);
        }
        assert!(mackinnon_critical_value(0.02, AdfDeterministic::None, None).is_err());
    }

    #[test]
    fn bad_input() {
        assert!(kpss(&[1.0, 2.0, 3.0], KpssNull::Level, None).is_err());
        assert!(kpss(&[1.0; 10], KpssNull::Level, None).is_err());
        assert!(kpss(&[1.0, 2.0, 3.0, 5.0], KpssNull::Level, Some(4)).is_err());
        assert!(kpss(&[1.0, f64::INFINITY, 3.0, 5.0], KpssNull::Level, None).is_err());
        assert!(adf(&[1.0, 2.0, 4.0], &AdfOptions::default()).is_err());
        assert!(adf(&[3.0; 30], &AdfOptions { lags: AdfLags::Fixed(1), ..AdfOptions::default() }).is_err());
    }
}
