//! Point forecasts and prediction intervals of a fitted ETS model.

use stat_core::distributions::{ContinuousDistribution, Normal};
use stat_core::rng::Rng;

use super::recursion::State;
use super::{ErrorType, EtsFit, StateVector};
use crate::{Result, StatError};

/// How prediction intervals are computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntervalMethod {
    /// Analytic variances where they exist (no multiplicative seasonality,
    /// Hyndman et al. 2008 classes 1 and 2), simulation otherwise.
    Automatic,
    /// Always simulate sample paths.
    Simulation,
}

/// Options for [`EtsFit::forecast`].
#[derive(Debug, Clone, PartialEq)]
pub struct ForecastOptions {
    /// Coverage probabilities of the intervals, each in `(0, 1)` (default
    /// 0.80 and 0.95).
    pub levels: Vec<f64>,
    /// Interval method (default [`IntervalMethod::Automatic`]).
    pub method: IntervalMethod,
    /// Number of simulated sample paths when simulating (default 5000).
    pub paths: usize,
    /// Seed of the simulation (default 1), so results are reproducible.
    pub seed: u64,
}

impl Default for ForecastOptions {
    fn default() -> Self {
        Self { levels: vec![0.80, 0.95], method: IntervalMethod::Automatic, paths: 5000, seed: 1 }
    }
}

/// Forecasts `1..=h` steps beyond the last observation.
#[derive(Debug, Clone, PartialEq)]
pub struct EtsForecast {
    /// Point forecasts (the recursion with zero future errors; the forecast
    /// means for models without multiplicative seasonality).
    pub mean: Vec<f64>,
    /// Coverage probabilities, as requested.
    pub levels: Vec<f64>,
    /// `lower[k][h−1]`: lower limit at `levels[k]`, `h` steps ahead.
    pub lower: Vec<Vec<f64>>,
    /// `upper[k][h−1]`: upper limit at `levels[k]`, `h` steps ahead.
    pub upper: Vec<Vec<f64>>,
    /// Forecast standard deviations (analytic, or of the simulated paths).
    pub standard_errors: Vec<f64>,
    /// Means of the simulated paths, when simulated. With multiplicative
    /// seasonality the forecast mean exceeds the point forecast beyond one
    /// cycle ahead (the seasonal factor then reused is itself random and
    /// correlated with the level), so this estimates the mean while
    /// [`EtsForecast::mean`] stays the point forecast.
    pub path_means: Option<Vec<f64>>,
    /// Whether the intervals come from simulated sample paths (empirical
    /// quantiles, linear interpolation between order statistics) rather
    /// than `mean ± z · standard error`.
    pub simulated: bool,
}

/// Linear-interpolation sample quantile (Hyndman & Fan 1996, definition 7)
/// of sorted values.
fn quantile_sorted(sorted: &[f64], p: f64) -> f64 {
    let h = (sorted.len() - 1) as f64 * p;
    let lo = h.floor() as usize;
    let hi = (lo + 1).min(sorted.len() - 1);
    sorted[lo] + (h - lo as f64) * (sorted[hi] - sorted[lo])
}

impl EtsFit {
    /// Forecast `horizon` steps ahead with prediction intervals.
    ///
    /// For models without multiplicative seasonality the forecast variance
    /// is analytic (see the module documentation), with `σ² = SSE/(n − p)`;
    /// intervals are `μ_h ± z_{(1+level)/2} σ_h`. Otherwise (or with
    /// [`IntervalMethod::Simulation`]) `paths` sample paths are simulated
    /// from the final state with `ε ~ N(0, σ²)` and the intervals are their
    /// empirical quantiles; point forecasts stay the zero-error recursion.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for a zero horizon, a level outside
    /// `(0, 1)`, or fewer than 2 paths when simulating.
    pub fn forecast(&self, horizon: usize, options: &ForecastOptions) -> Result<EtsForecast> {
        if horizon == 0 {
            return Err(StatError::InvalidArgument { message: "the forecast horizon must be at least 1".into() });
        }
        if options.levels.iter().any(|l| !(*l > 0.0 && *l < 1.0)) {
            return Err(StatError::InvalidArgument { message: "interval levels must lie in (0, 1)".into() });
        }
        let c = self.coefficients();
        let state = State::from_vector(&self.final_state, &self.spec);
        let mean = state.point_forecasts(&c, horizon);
        let simulate = options.method == IntervalMethod::Simulation || !self.spec.has_linear_states();
        let sigma2 = self.sigma2;
        if !simulate {
            // Impulse responses c_j = w′F^{j−1}g of a unit one-step error.
            let zero = StateVector { level: 0.0, slope: 0.0, seasonal: vec![0.0; self.final_state.seasonal.len()] };
            let mut s = State::from_vector(&zero, &self.spec);
            let step = s.one_step(&c);
            s.update(&c, step, 1.0);
            let mut cj = vec![0.0];
            cj.extend(s.point_forecasts(&c, horizon.saturating_sub(1)));
            let variance: Vec<f64> = match self.spec.error {
                ErrorType::Additive => {
                    let mut acc = 0.0;
                    (0..horizon)
                        .map(|h| {
                            if h > 0 {
                                acc += cj[h] * cj[h];
                            }
                            sigma2 * (1.0 + acc)
                        })
                        .collect()
                }
                ErrorType::Multiplicative => {
                    let mut theta: Vec<f64> = Vec::with_capacity(horizon);
                    for h in 0..horizon {
                        let mut t = mean[h] * mean[h];
                        for j in 1..=h {
                            t += sigma2 * cj[j] * cj[j] * theta[h - j];
                        }
                        theta.push(t);
                    }
                    theta.iter().zip(&mean).map(|(t, m)| (1.0 + sigma2) * t - m * m).collect()
                }
            };
            let se: Vec<f64> = variance.iter().map(|v| v.max(0.0).sqrt()).collect();
            let normal = Normal::standard();
            let mut lower = Vec::new();
            let mut upper = Vec::new();
            for &level in &options.levels {
                let z = normal.quantile(0.5 + level / 2.0)?;
                lower.push(mean.iter().zip(&se).map(|(m, s)| m - z * s).collect());
                upper.push(mean.iter().zip(&se).map(|(m, s)| m + z * s).collect());
            }
            return Ok(EtsForecast {
                mean,
                levels: options.levels.clone(),
                lower,
                upper,
                standard_errors: se,
                path_means: None,
                simulated: false,
            });
        }
        if options.paths < 2 {
            return Err(StatError::InvalidArgument { message: "simulation needs at least 2 sample paths".into() });
        }
        let sigma = sigma2.sqrt();
        let mut rng = Rng::new(options.seed);
        let mut draws = vec![Vec::with_capacity(options.paths); horizon];
        for _ in 0..options.paths {
            let mut s = state.clone();
            for slot in draws.iter_mut() {
                let step = s.one_step(&c);
                let e = sigma * rng.normal();
                let y = match self.spec.error {
                    ErrorType::Additive => step.mu + e,
                    ErrorType::Multiplicative => step.mu * (1.0 + e),
                };
                slot.push(y);
                s.update(&c, step, y - step.mu);
            }
        }
        let mut se = Vec::with_capacity(horizon);
        let mut path_means = Vec::with_capacity(horizon);
        for d in &mut draws {
            let k = d.len() as f64;
            let avg = d.iter().sum::<f64>() / k;
            path_means.push(avg);
            se.push((d.iter().map(|v| (v - avg) * (v - avg)).sum::<f64>() / (k - 1.0)).sqrt());
            d.sort_by(f64::total_cmp);
        }
        let mut lower = Vec::new();
        let mut upper = Vec::new();
        for &level in &options.levels {
            let a = (1.0 - level) / 2.0;
            lower.push(draws.iter().map(|d| quantile_sorted(d, a)).collect());
            upper.push(draws.iter().map(|d| quantile_sorted(d, 1.0 - a)).collect());
        }
        Ok(EtsForecast {
            mean,
            levels: options.levels.clone(),
            lower,
            upper,
            standard_errors: se,
            path_means: Some(path_means),
            simulated: true,
        })
    }
}
