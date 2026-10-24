//! # stat-timeseries
//!
//! Analysis of a single equally spaced series `y₁, …, yₙ` (and pairs of
//! series for cross-correlation). Missing values are not allowed inside a
//! series; callers drop leading and trailing gaps first.
//!
//! * [`autocorrelation`] — sample ACF and PACF with confidence limits,
//!   Ljung–Box and Box–Pierce portmanteau tests, cross-correlation with
//!   optional prewhitening.
//! * [`transform`] — regular and seasonal differencing and its inverse, and
//!   Box–Cox transformation of a series with Guerrero's choice of λ.
//! * [`decompose`] — classical (moving-average) and STL (Cleveland et al.
//!   1990) seasonal decomposition.
//! * [`trend`] — linear, quadratic, exponential-growth and logistic trend
//!   fits, moving-average forecasts and accuracy measures (MAPE, MAD, MSD).
//! * [`spectral`] — periodogram and smoothed spectral density estimates.
//! * [`unit_root`] — augmented Dickey–Fuller and KPSS tests.
//! * [`smoothing`] — exponential smoothing (simple, Holt, damped,
//!   Holt–Winters additive and multiplicative) fitted by maximum likelihood
//!   in innovations state-space form (Hyndman et al. 2008).
//! * [`arima`] — seasonal ARIMA by exact maximum likelihood through the
//!   Kalman filter, forecasts with prediction intervals, and the stepwise
//!   automatic order search of Hyndman & Khandakar (2008).

pub mod arima;
pub mod autocorrelation;
pub mod decompose;
pub mod smoothing;
pub mod spectral;
pub mod transform;
pub mod trend;
pub mod unit_root;

pub use stat_core::error::{Result, StatError};
