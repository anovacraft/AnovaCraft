//! Trend models of a series against time, moving-average forecasts and
//! forecast accuracy measures.
//!
//! # Trend models
//! Each model regresses the series `y₁, …, yₙ` on the time index
//! `t = 1, …, n` (the textbook convention: the first observation is at
//! `t = 1`, so the intercept is the level one period before the series
//! starts; forecasts are at `t = n + 1, n + 2, …`). See Montgomery, Jennings &
//! Kulahci (2015), *Introduction to Time Series Analysis and Forecasting*,
//! 2nd ed., Wiley, chapter 3.
//!
//! * **Linear** `y = β₀ + β₁ t + ε` and **quadratic** `y = β₀ + β₁ t + β₂ t² + ε`
//!   by ordinary least squares (pivoted Householder QR). Forecasts carry
//!   prediction intervals `ŷ(t₀) ± t_{n−p, 1−α/2} · s · √(1 + x₀ᵀ (XᵀX)⁻¹ x₀)`
//!   with `s² = SSE/(n − p)`.
//! * **Exponential growth** `y = b₀ · b₁ᵗ`, fitted by least squares on the log
//!   scale, `ln y = ln b₀ + t ln b₁ + ε` (the classical linearisation; it
//!   minimises relative rather than absolute squared error, so the curve is
//!   not the least-squares fit on the original scale). Requires `y > 0`.
//!   Fitted values and forecasts are `exp` of the log-scale fit (the median,
//!   not the mean, of a log-normal error model); prediction intervals are the
//!   log-scale intervals transformed back with `exp`.
//! * **Logistic (Pearl–Reed) S-curve** `y = c / (1 + a·e^{−b t})`, by
//!   nonlinear least squares on the original scale (Levenberg–Marquardt,
//!   [`stat_core::optimize::levenberg_marquardt`]; Seber & Wild (1989),
//!   *Nonlinear Regression*, Wiley, §7.3.4). `c` is the upper asymptote, the
//!   curve's midpoint is at `t = ln(a)/b` and its scale is `1/b`. Starting
//!   values come from the linearisation `ln(c/y − 1) = ln a − b t` for several
//!   trial asymptotes above `max y`; the best converged fit is kept. Requires
//!   `y > 0`. No prediction intervals are given for this model.
//!
//! # Accuracy measures
//! For actual values `yₜ` and fitted (one-step or in-sample) values `ŷₜ`
//! with errors `eₜ = yₜ − ŷₜ` over the `m` periods that have a fitted value:
//! mean error `ME = Σe/m`, mean absolute deviation `MAD = Σ|e|/m` (also
//! called MAE), mean squared deviation `MSD = Σe²/m` (MSE with divisor `m`),
//! `RMSE = √MSD`, and mean absolute percentage error
//! `MAPE = 100 · Σ|e/y|/m` (undefined when some `yₜ = 0`).
//!
//! # Moving-average forecasts
//! The single moving average of span `k` forecasts every future period by the
//! mean of the last `k` observations; its in-sample one-step forecasts are
//! `ŷₜ = (y_{t−1} + … + y_{t−k})/k` for `t = k + 1, …, n`, and the accuracy
//! measures use those `n − k` errors (Montgomery, Jennings & Kulahci 2015,
//! §4.2).

use crate::{Result, StatError};
use stat_core::distributions::{ContinuousDistribution, StudentsT};
use stat_core::matrix::Matrix;
use stat_core::optimize::{LeastSquaresOptions, ResidualFn, levenberg_marquardt};
use stat_core::qr::QrDecomposition;

/// Least-squares fit used by the trend models and the unit-root regressions.
#[derive(Debug, Clone)]
pub(crate) struct OlsFit {
    /// Coefficients, in the order of the columns.
    pub beta: Vec<f64>,
    /// Residuals `y − Xβ`.
    pub residuals: Vec<f64>,
    /// Residual sum of squares.
    pub rss: f64,
    /// `(XᵀX)⁻¹`.
    pub xtx_inv: Matrix,
    /// Residual degrees of freedom `n − p`.
    pub df: usize,
}

impl OlsFit {
    /// Standard error of coefficient `j`, `s·√[(XᵀX)⁻¹]ⱼⱼ` with `s² = RSS/df`.
    pub fn std_error(&self, j: usize) -> f64 {
        (self.rss / self.df as f64 * self.xtx_inv[(j, j)]).sqrt()
    }
}

/// Ordinary least squares of `y` on the given columns by pivoted QR.
///
/// # Errors
/// * [`StatError::InsufficientData`] unless there are more rows than columns.
/// * [`StatError::RankDeficient`] when the columns are numerically collinear.
pub(crate) fn ols(columns: &[Vec<f64>], y: &[f64]) -> Result<OlsFit> {
    let n = y.len();
    let p = columns.len();
    if n <= p {
        return Err(StatError::InsufficientData { required: p + 1, found: n });
    }
    let x = Matrix::from_columns(columns)?;
    let qr = QrDecomposition::new(x.clone())?;
    let rank = qr.rank(1e-11);
    if rank < p {
        return Err(StatError::RankDeficient { rank, columns: p });
    }
    let beta = qr.solve_least_squares(y)?;
    let fitted = x.mul_vec(&beta)?;
    let residuals: Vec<f64> = y.iter().zip(&fitted).map(|(a, b)| a - b).collect();
    let rss = stat_core::summation::dot(&residuals, &residuals);
    let xtx_inv = qr.inverse_gram()?;
    Ok(OlsFit { beta, residuals, rss, xtx_inv, df: n - p })
}

pub(crate) fn check_finite(y: &[f64], context: &'static str) -> Result<()> {
    if y.iter().all(|v| v.is_finite()) { Ok(()) } else { Err(StatError::NonFiniteInput { context }) }
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Forecast accuracy measures of fitted values against actual values (see
/// the module documentation for the formulas).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AccuracyMeasures {
    /// Number of periods compared `m`.
    pub n: usize,
    /// Mean error `Σe/m` (bias; positive when the fit is too low on average).
    pub mean_error: f64,
    /// Mean absolute deviation `Σ|e|/m` (MAE).
    pub mad: f64,
    /// Mean squared deviation `Σe²/m`.
    pub msd: f64,
    /// Root mean squared error `√MSD`.
    pub rmse: f64,
    /// Mean absolute percentage error `100·Σ|e/y|/m`; `None` when an actual
    /// value is zero.
    pub mape: Option<f64>,
}

/// Accuracy measures of `fitted` against `actual` (equal lengths, at least one
/// pair).
///
/// # Errors
/// * [`StatError::DimensionMismatch`] when the lengths differ.
/// * [`StatError::InsufficientData`] when they are empty.
/// * [`StatError::NonFiniteInput`] for NaN or infinite values.
pub fn accuracy(actual: &[f64], fitted: &[f64]) -> Result<AccuracyMeasures> {
    if actual.len() != fitted.len() {
        return Err(StatError::DimensionMismatch {
            context: "actual vs. fitted values",
            expected: actual.len(),
            found: fitted.len(),
        });
    }
    if actual.is_empty() {
        return Err(StatError::InsufficientData { required: 1, found: 0 });
    }
    check_finite(actual, "actual values")?;
    check_finite(fitted, "fitted values")?;
    let m = actual.len() as f64;
    let errors: Vec<f64> = actual.iter().zip(fitted).map(|(a, f)| a - f).collect();
    let mean_error = stat_core::summation::sum(&errors) / m;
    let mad = stat_core::summation::sum_iter(errors.iter().map(|e| e.abs())) / m;
    let msd = stat_core::summation::dot(&errors, &errors) / m;
    let mape = if actual.contains(&0.0) {
        None
    } else {
        Some(100.0 * stat_core::summation::sum_iter(errors.iter().zip(actual).map(|(e, a)| (e / a).abs())) / m)
    };
    Ok(AccuracyMeasures { n: actual.len(), mean_error, mad, msd, rmse: msd.sqrt(), mape })
}

/// The trend curve fitted by [`fit_trend`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrendModel {
    /// `y = β₀ + β₁ t`.
    Linear,
    /// `y = β₀ + β₁ t + β₂ t²`.
    Quadratic,
    /// `y = b₀ · b₁ᵗ`, least squares on `ln y`.
    Exponential,
    /// `y = c / (1 + a·e^{−b t})`, nonlinear least squares.
    Logistic,
}

/// A fitted trend model (see the module documentation).
#[derive(Debug, Clone)]
pub struct TrendFit {
    /// The model.
    pub model: TrendModel,
    /// Coefficients: `[β₀, β₁]` (linear), `[β₀, β₁, β₂]` (quadratic),
    /// `[b₀, b₁]` (exponential growth, `y = b₀ b₁ᵗ`) or `[c, a, b]`
    /// (logistic).
    pub coefficients: Vec<f64>,
    /// Standard errors of the coefficients: least-squares standard errors for
    /// the linear and quadratic models, those of `ln b₀` and `ln b₁` on the
    /// log scale for the exponential model, and the linearisation
    /// (`s²(JᵀJ)⁻¹`) standard errors of `c, a, b` for the logistic model.
    pub std_errors: Vec<f64>,
    /// Fitted values at `t = 1, …, n`.
    pub fitted: Vec<f64>,
    /// Residuals `y − ŷ` on the original scale.
    pub residuals: Vec<f64>,
    /// Residual standard deviation `√(SSE/(n − p))` of the fitting scale
    /// (the log scale for the exponential model).
    pub sigma: f64,
    /// Residual degrees of freedom `n − p`.
    pub df: usize,
    /// Accuracy measures of the fitted values (original scale).
    pub accuracy: AccuracyMeasures,
    /// `(XᵀX)⁻¹` of the least-squares models (log scale for exponential);
    /// `None` for the logistic model.
    xtx_inv: Option<Matrix>,
    n: usize,
}

/// Forecasts of a trend model.
#[derive(Debug, Clone, PartialEq)]
pub struct TrendForecast {
    /// Time index of each forecast, `n + 1, …, n + h`.
    pub time: Vec<f64>,
    /// Point forecasts.
    pub mean: Vec<f64>,
    /// Lower prediction limits (`None` for the logistic model).
    pub lower: Option<Vec<f64>>,
    /// Upper prediction limits (`None` for the logistic model).
    pub upper: Option<Vec<f64>>,
    /// Coverage of the intervals, e.g. 0.95.
    pub level: f64,
}

fn design_row(model: TrendModel, t: f64) -> Vec<f64> {
    match model {
        TrendModel::Quadratic => vec![1.0, t, t * t],
        _ => vec![1.0, t],
    }
}

fn logistic(theta: &[f64], t: f64) -> f64 {
    theta[0] / (1.0 + theta[1] * (-theta[2] * t).exp())
}

/// Fit a trend model to `y` on `t = 1, …, n`.
///
/// # Errors
/// * [`StatError::NonFiniteInput`] for NaN or infinite values.
/// * [`StatError::InsufficientData`] with fewer than `p + 1` observations
///   (`p` = number of coefficients).
/// * [`StatError::InvalidArgument`] for a non-positive value in the
///   exponential or logistic model.
/// * [`StatError::ConvergenceFailure`] when no logistic fit converges.
pub fn fit_trend(y: &[f64], model: TrendModel) -> Result<TrendFit> {
    check_finite(y, "series")?;
    let n = y.len();
    let t: Vec<f64> = (1..=n).map(|i| i as f64).collect();
    let p = match model {
        TrendModel::Quadratic | TrendModel::Logistic => 3,
        _ => 2,
    };
    if n <= p {
        return Err(StatError::InsufficientData { required: p + 1, found: n });
    }
    if matches!(model, TrendModel::Exponential | TrendModel::Logistic) && y.iter().any(|&v| v <= 0.0) {
        return Err(invalid("exponential and logistic trends need every value to be positive"));
    }
    match model {
        TrendModel::Linear | TrendModel::Quadratic | TrendModel::Exponential => {
            let mut columns = vec![vec![1.0; n], t.clone()];
            if model == TrendModel::Quadratic {
                columns.push(t.iter().map(|v| v * v).collect());
            }
            let response: Vec<f64> =
                if model == TrendModel::Exponential { y.iter().map(|v| v.ln()).collect() } else { y.to_vec() };
            let fit = ols(&columns, &response)?;
            let std_errors: Vec<f64> = (0..p).map(|j| fit.std_error(j)).collect();
            let (coefficients, fitted): (Vec<f64>, Vec<f64>) = if model == TrendModel::Exponential {
                let f = t.iter().map(|&ti| (fit.beta[0] + fit.beta[1] * ti).exp()).collect();
                (fit.beta.iter().map(|b| b.exp()).collect(), f)
            } else {
                let f = response.iter().zip(&fit.residuals).map(|(a, r)| a - r).collect();
                (fit.beta.clone(), f)
            };
            let residuals: Vec<f64> = y.iter().zip(&fitted).map(|(a, f)| a - f).collect();
            let accuracy = accuracy(y, &fitted)?;
            Ok(TrendFit {
                model,
                coefficients,
                std_errors,
                fitted,
                residuals,
                sigma: (fit.rss / fit.df as f64).sqrt(),
                df: fit.df,
                accuracy,
                xtx_inv: Some(fit.xtx_inv),
                n,
            })
        }
        TrendModel::Logistic => fit_logistic(y, &t),
    }
}

fn fit_logistic(y: &[f64], t: &[f64]) -> Result<TrendFit> {
    let n = y.len();
    let ymax = y.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let mut best: Option<stat_core::optimize::LeastSquaresResult> = None;
    for factor in [1.01, 1.05, 1.2, 1.5, 2.0, 3.0, 5.0] {
        let c0 = ymax * factor;
        // ln(c/y − 1) = ln a − b t
        let z: Vec<f64> = y.iter().map(|&v| (c0 / v - 1.0).ln()).collect();
        let Ok(lin) = ols(&[vec![1.0; n], t.to_vec()], &z) else { continue };
        let start = [c0, lin.beta[0].exp(), -lin.beta[1]];
        if !start.iter().all(|v| v.is_finite()) {
            continue;
        }
        let mut problem = ResidualFn::new(n, |theta: &[f64], out: &mut [f64]| {
            for i in 0..n {
                out[i] = y[i] - logistic(theta, t[i]);
            }
            true
        });
        let Ok(res) = levenberg_marquardt(&mut problem, &start, &LeastSquaresOptions::default()) else { continue };
        if !res.converged() {
            continue;
        }
        if best.as_ref().is_none_or(|b| res.sse < b.sse) {
            best = Some(res);
        }
    }
    let res = best.ok_or(StatError::ConvergenceFailure { routine: "logistic trend fit", iterations: 200 })?;
    let theta = res.theta.clone();
    let fitted: Vec<f64> = t.iter().map(|&ti| logistic(&theta, ti)).collect();
    let residuals: Vec<f64> = y.iter().zip(&fitted).map(|(a, f)| a - f).collect();
    let df = n - 3;
    let s2 = res.sse / df as f64;
    // Linearisation covariance s²(JᵀJ)⁻¹ from the QR of the Jacobian.
    let std_errors = match QrDecomposition::new(res.jacobian.clone()).and_then(|q| q.inverse_gram()) {
        Ok(inv) => (0..3).map(|j| (s2 * inv[(j, j)]).sqrt()).collect(),
        Err(_) => vec![f64::NAN; 3],
    };
    let accuracy = accuracy(y, &fitted)?;
    Ok(TrendFit {
        model: TrendModel::Logistic,
        coefficients: theta,
        std_errors,
        fitted,
        residuals,
        sigma: s2.sqrt(),
        df,
        accuracy,
        xtx_inv: None,
        n,
    })
}

impl TrendFit {
    /// Value of the fitted curve at time `t` (`t = 1` is the first
    /// observation).
    #[must_use]
    pub fn evaluate(&self, t: f64) -> f64 {
        let c = &self.coefficients;
        match self.model {
            TrendModel::Linear => c[0] + c[1] * t,
            TrendModel::Quadratic => c[0] + c[1] * t + c[2] * t * t,
            TrendModel::Exponential => c[0] * c[1].powf(t),
            TrendModel::Logistic => logistic(c, t),
        }
    }

    /// Forecasts `h` periods ahead (`t = n + 1, …, n + h`) with prediction
    /// intervals of coverage `level` for the least-squares models (see the
    /// module documentation; none for the logistic model).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] when `level` is not in (0, 1).
    pub fn forecast(&self, h: usize, level: f64) -> Result<TrendForecast> {
        if !(level > 0.0 && level < 1.0) {
            return Err(invalid("the prediction interval level must be between 0 and 1"));
        }
        let time: Vec<f64> = (1..=h).map(|i| (self.n + i) as f64).collect();
        let mean: Vec<f64> = time.iter().map(|&t| self.evaluate(t)).collect();
        let Some(inv) = &self.xtx_inv else {
            return Ok(TrendForecast { time, mean, lower: None, upper: None, level });
        };
        let q = StudentsT::new(self.df as f64)?.quantile(0.5 + level / 2.0)?;
        let (mut lower, mut upper) = (Vec::with_capacity(h), Vec::with_capacity(h));
        for &t in &time {
            let x = design_row(self.model, t);
            let mut quad = 0.0;
            for i in 0..x.len() {
                for j in 0..x.len() {
                    quad += x[i] * inv[(i, j)] * x[j];
                }
            }
            let half = q * self.sigma * (1.0 + quad).sqrt();
            if self.model == TrendModel::Exponential {
                let centre = self.coefficients[0].ln() + self.coefficients[1].ln() * t;
                lower.push((centre - half).exp());
                upper.push((centre + half).exp());
            } else {
                let centre = self.evaluate(t);
                lower.push(centre - half);
                upper.push(centre + half);
            }
        }
        Ok(TrendForecast { time, mean, lower: Some(lower), upper: Some(upper), level })
    }
}

/// Single moving-average forecasts (see the module documentation).
#[derive(Debug, Clone, PartialEq)]
pub struct MovingAverageForecast {
    /// Span `k`.
    pub span: usize,
    /// One-step in-sample forecasts: `None` for the first `k` periods, then
    /// the mean of the previous `k` observations.
    pub fitted: Vec<Option<f64>>,
    /// Forecast of every future period: the mean of the last `k` observations.
    pub forecast: f64,
    /// Accuracy of the `n − k` one-step forecasts.
    pub accuracy: AccuracyMeasures,
}

/// Single moving-average forecasts of span `span` for `y`.
///
/// # Errors
/// * [`StatError::InvalidArgument`] for a span of zero.
/// * [`StatError::InsufficientData`] unless `n > span` (at least one
///   one-step forecast to score).
/// * [`StatError::NonFiniteInput`] for NaN or infinite values.
pub fn moving_average_forecast(y: &[f64], span: usize) -> Result<MovingAverageForecast> {
    if span == 0 {
        return Err(invalid("the moving-average span must be at least 1"));
    }
    check_finite(y, "series")?;
    let n = y.len();
    if n <= span {
        return Err(StatError::InsufficientData { required: span + 1, found: n });
    }
    let k = span as f64;
    let mut fitted = vec![None; n];
    for t in span..n {
        fitted[t] = Some(stat_core::summation::sum(&y[t - span..t]) / k);
    }
    let forecast = stat_core::summation::sum(&y[n - span..]) / k;
    let f: Vec<f64> = fitted.iter().flatten().copied().collect();
    let accuracy = accuracy(&y[span..], &f)?;
    Ok(MovingAverageForecast { span, fitted, forecast, accuracy })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_line_is_recovered() {
        let y: Vec<f64> = (1..=10).map(|t| 3.0 + 2.0 * t as f64).collect();
        let fit = fit_trend(&y, TrendModel::Linear).unwrap();
        assert!((fit.coefficients[0] - 3.0).abs() < 1e-12 && (fit.coefficients[1] - 2.0).abs() < 1e-12);
        let f = fit.forecast(2, 0.95).unwrap();
        assert!((f.mean[0] - 25.0).abs() < 1e-10 && (f.mean[1] - 27.0).abs() < 1e-10);
    }

    #[test]
    fn exact_growth_and_logistic_curves() {
        let y: Vec<f64> = (1..=12).map(|t| 5.0 * 1.1f64.powi(t)).collect();
        let fit = fit_trend(&y, TrendModel::Exponential).unwrap();
        assert!((fit.coefficients[0] - 5.0).abs() < 1e-10 && (fit.coefficients[1] - 1.1).abs() < 1e-12);
        let y: Vec<f64> = (1..=30).map(|t| 100.0 / (1.0 + 50.0 * (-0.3 * t as f64).exp())).collect();
        let fit = fit_trend(&y, TrendModel::Logistic).unwrap();
        for (got, want) in fit.coefficients.iter().zip([100.0, 50.0, 0.3]) {
            assert!((got - want).abs() < 1e-6 * want, "{got} vs {want}");
        }
    }

    #[test]
    fn moving_average_and_accuracy() {
        let y = [1.0, 2.0, 3.0, 4.0, 5.0];
        let ma = moving_average_forecast(&y, 2).unwrap();
        assert_eq!(ma.fitted, vec![None, None, Some(1.5), Some(2.5), Some(3.5)]);
        assert!((ma.forecast - 4.5).abs() < 1e-15);
        assert!((ma.accuracy.mad - 1.5).abs() < 1e-15 && (ma.accuracy.msd - 2.25).abs() < 1e-15);
        let acc = accuracy(&[0.0, 1.0], &[1.0, 1.0]).unwrap();
        assert_eq!(acc.mape, None);
    }

    #[test]
    fn bad_input_is_rejected() {
        assert!(fit_trend(&[1.0, 2.0], TrendModel::Linear).is_err());
        assert!(fit_trend(&[1.0, f64::NAN, 3.0, 4.0], TrendModel::Linear).is_err());
        assert!(fit_trend(&[1.0, -2.0, 3.0, 4.0], TrendModel::Exponential).is_err());
        assert!(moving_average_forecast(&[1.0, 2.0], 2).is_err());
        assert!(moving_average_forecast(&[1.0, 2.0], 0).is_err());
        let fit = fit_trend(&[1.0, 2.0, 4.0, 3.0, 5.0], TrendModel::Linear).unwrap();
        assert!(fit.forecast(1, 1.5).is_err());
    }
}
