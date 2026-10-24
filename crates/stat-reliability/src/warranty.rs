//! Warranty returns: shipments by period and returns by period of shipment
//! and period of return, turned into censored life data, a fitted life
//! distribution, and a forecast of future returns from the installed base.
//!
//! # Data
//! Calendar periods are numbered `0, 1, …, P − 1`, `P − 1` being the last
//! period observed (the current one). Units of a *cohort* are shipped in
//! period `s` (`N` of them) and enter service at the start of that period; a
//! unit returned in period `r ≥ s` therefore failed at an age (in periods) in
//! `(r − s, r − s + 1]`, so it is **interval-censored**. A unit not yet
//! returned has survived `P − s` periods and is **right-censored** there —
//! or at the warranty length `W` when it has already passed it, since returns
//! after the warranty ends are not seen (Kalbfleisch, Lawless & Robinson
//! 1991, "Methods for the analysis and prediction of warranty claims",
//! *Technometrics* 33, 273–285; Meeker & Escobar, *Statistical Methods for
//! Reliability Data*, 1998, §12.7).
//!
//! # Forecast
//! With the fitted distribution `F`, a unit of a cohort of current age
//! `a = min(P − s, W)` that has not been returned is returned in future
//! period `k = 1, 2, …` (calendar period `P − 1 + k`) with probability
//!
//! ```text
//! p_k = [F(min(a + k, W)) − F(min(a + k − 1, W))] / [1 − F(a)]
//! ```
//!
//! (the conditional failure probability given survival to the current age).
//! A planned shipment of `M` units in future period `m` contributes
//! `M [F(min(k − m + 1, W)) − F(min(k − m, W))]` for `k ≥ m`. The expected
//! returns of a period are `μ_k = Σ n p_k` over cohorts (`n` units still in
//! service) and planned shipments, and of the first `K` periods together the
//! same sum of the cumulative probabilities.
//!
//! **Prediction intervals.** The returns from a cohort are binomial given the
//! parameters, so the count's variance from that source is `Σ n p (1 − p)`;
//! estimating the distribution adds the delta-method variance
//! `∇μᵀ V ∇μ` of the expected count (`V` the parameters' covariance). The
//! limits are the normal approximation `μ ± z √(Σ n p(1 − p) + ∇μᵀV∇μ)`,
//! with the lower limit kept at or above zero (Meeker & Escobar 1998, §12.7,
//! approximate prediction of the number of future failures; Escobar & Meeker
//! 1999, "Statistical prediction based on censored life data",
//! *Technometrics* 41, 113–124, describe calibrated refinements).

use stat_core::distribution_fit::FittedDistribution;
use stat_core::{ContinuousDistribution, Normal, Result, StatError};

use crate::censored_fit::CensoredFit;
use crate::life_data::{LifeData, LifeObservation};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// One shipment cohort.
#[derive(Debug, Clone, PartialEq)]
pub struct WarrantyCohort {
    /// Calendar period of shipment, `0 ≤ s < P`.
    pub period: usize,
    /// Units shipped.
    pub shipped: f64,
    /// Returns by age: `returns[a]` units returned in calendar period `s + a`
    /// (failed at an age in `(a, a + 1]` periods).
    pub returns: Vec<f64>,
}

impl WarrantyCohort {
    /// Units returned so far.
    #[must_use]
    pub fn returned(&self) -> f64 {
        self.returns.iter().sum()
    }

    /// Units still in service.
    #[must_use]
    pub fn in_service(&self) -> f64 {
        self.shipped - self.returned()
    }
}

/// Shipment cohorts observed through calendar period `P − 1`.
#[derive(Debug, Clone, PartialEq)]
pub struct WarrantyData {
    cohorts: Vec<WarrantyCohort>,
    periods: usize,
    warranty: Option<f64>,
}

/// One row of the life data built from the counts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WarrantyLifeRow {
    /// Index of the cohort in [`WarrantyData::cohorts`].
    pub cohort: usize,
    /// The censored observation (an age interval, or right-censored at the
    /// current age).
    pub observation: LifeObservation,
}

impl WarrantyData {
    /// Cohorts observed through calendar period `periods − 1`, with an
    /// optional warranty length `W` (in periods).
    ///
    /// # Errors
    /// No cohorts or no units, a shipment period at or after `periods`,
    /// returns in periods not yet observed, negative or non-finite counts,
    /// more returns than shipments, returns at ages past the warranty, or a
    /// warranty length that is not positive.
    pub fn new(cohorts: Vec<WarrantyCohort>, periods: usize, warranty: Option<f64>) -> Result<Self> {
        if cohorts.is_empty() {
            return Err(invalid("no shipment periods"));
        }
        if let Some(w) = warranty {
            if !(w.is_finite() && w > 0.0) {
                return Err(invalid("the warranty length must be positive"));
            }
        }
        for c in &cohorts {
            if c.period >= periods {
                return Err(invalid("a shipment is after the last observed period"));
            }
            if !(c.shipped.is_finite() && c.shipped >= 0.0) || c.returns.iter().any(|r| !(r.is_finite() && *r >= 0.0)) {
                return Err(invalid("shipments and returns must be non-negative counts"));
            }
            if c.returns.len() > periods - c.period {
                return Err(invalid("returns are recorded for periods after the last observed period"));
            }
            if c.returned() > c.shipped * (1.0 + 1e-12) {
                return Err(invalid(format!(
                    "shipment period {}: {} units returned but only {} shipped",
                    c.period + 1,
                    c.returned(),
                    c.shipped
                )));
            }
            if let Some(w) = warranty {
                if c.returns.iter().enumerate().any(|(a, &r)| r > 0.0 && a as f64 >= w) {
                    return Err(invalid(format!(
                        "shipment period {}: returns are recorded after the warranty ends",
                        c.period + 1
                    )));
                }
            }
        }
        if cohorts.iter().map(|c| c.shipped).sum::<f64>() <= 0.0 {
            return Err(invalid("no units were shipped"));
        }
        Ok(Self { cohorts, periods, warranty })
    }

    /// The cohorts.
    #[must_use]
    pub fn cohorts(&self) -> &[WarrantyCohort] {
        &self.cohorts
    }

    /// Number of calendar periods observed, `P`.
    #[must_use]
    pub fn periods(&self) -> usize {
        self.periods
    }

    /// Warranty length in periods.
    #[must_use]
    pub fn warranty(&self) -> Option<f64> {
        self.warranty
    }

    /// Age (periods in service) of a cohort at the end of observation,
    /// capped at the warranty length.
    #[must_use]
    pub fn current_age(&self, cohort: &WarrantyCohort) -> f64 {
        let age = (self.periods - cohort.period) as f64;
        self.warranty.map_or(age, |w| age.min(w))
    }

    /// The censored life data: one interval per cohort and age with returns,
    /// and the units still in service right-censored at their current age.
    #[must_use]
    pub fn life_rows(&self) -> Vec<WarrantyLifeRow> {
        let mut out = Vec::new();
        for (i, c) in self.cohorts.iter().enumerate() {
            for (a, &r) in c.returns.iter().enumerate() {
                if r > 0.0 {
                    out.push(WarrantyLifeRow {
                        cohort: i,
                        observation: LifeObservation::interval(a as f64, a as f64 + 1.0, r),
                    });
                }
            }
            let left = c.in_service();
            if left > 0.0 {
                out.push(WarrantyLifeRow { cohort: i, observation: LifeObservation::right(self.current_age(c), left) });
            }
        }
        out
    }

    /// [`life_rows`](Self::life_rows) as [`LifeData`] for fitting.
    ///
    /// # Errors
    /// See [`LifeData::new`].
    pub fn life_data(&self) -> Result<LifeData> {
        LifeData::new(self.life_rows().into_iter().map(|r| r.observation).collect())
    }

    /// Observed returns in each calendar period `0..P`.
    #[must_use]
    pub fn returns_by_period(&self) -> Vec<f64> {
        let mut out = vec![0.0; self.periods];
        for c in &self.cohorts {
            for (a, &r) in c.returns.iter().enumerate() {
                out[c.period + a] += r;
            }
        }
        out
    }

    /// Expected returns in each observed calendar period under `d`,
    /// `Σ N [F(r − s + 1) − F(r − s)]` (capped at the warranty), for comparing
    /// the fit with the observed returns.
    #[must_use]
    pub fn fitted_by_period(&self, d: &FittedDistribution) -> Vec<f64> {
        let cap = |a: f64| self.warranty.map_or(a, |w| a.min(w));
        let mut out = vec![0.0; self.periods];
        for c in &self.cohorts {
            for (r, slot) in out.iter_mut().enumerate().skip(c.period) {
                let a = (r - c.period) as f64;
                *slot += c.shipped * (d.cdf(cap(a + 1.0)) - d.cdf(cap(a)));
            }
        }
        out
    }
}

/// The forecast for one future period (or for the first `k` together).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ForecastValue {
    /// Future period index `k = 1, 2, …` (calendar period `P − 1 + k`).
    pub period: usize,
    /// Expected returns.
    pub expected: f64,
    /// Expected returns from units already in service.
    pub from_installed: f64,
    /// Expected returns from planned future shipments.
    pub from_planned: f64,
    /// Standard deviation of the prediction (binomial and parameter parts).
    pub std_error: f64,
    /// Standard error of the expected count from parameter uncertainty alone.
    pub parameter_std_error: f64,
    /// Lower prediction limit.
    pub lower: f64,
    /// Upper prediction limit.
    pub upper: f64,
}

/// Forecasts per future period and cumulative.
#[derive(Debug, Clone, PartialEq)]
pub struct WarrantyForecast {
    /// Expected returns in each future period.
    pub periods: Vec<ForecastValue>,
    /// Expected returns over the first `k` future periods together.
    pub cumulative: Vec<ForecastValue>,
}

/// A contribution `(units, lower age, upper age, conditioning age)`: the
/// units fail in `(lower, upper]` with probability
/// `[F(upper) − F(lower)] / [1 − F(cond)]`.
type Exposure = (f64, f64, f64, f64);

fn probability(d: &FittedDistribution, lower: f64, upper: f64, cond: f64) -> f64 {
    if upper <= lower {
        return 0.0;
    }
    let s = 1.0 - d.cdf(cond);
    if s.is_nan() || s <= 0.0 {
        return 0.0;
    }
    ((d.cdf(upper) - d.cdf(lower)) / s).clamp(0.0, 1.0)
}

fn value(fit: &CensoredFit, exposures: &[(Exposure, bool)], z: f64, period: usize) -> ForecastValue {
    let d = &fit.distribution;
    let (mut expected, mut installed, mut planned, mut var) = (0.0, 0.0, 0.0, 0.0);
    for &((n, lo, hi, cond), is_installed) in exposures {
        let p = probability(d, lo, hi, cond);
        expected += n * p;
        if is_installed {
            installed += n * p;
        } else {
            planned += n * p;
        }
        var += n * p * (1.0 - p);
    }
    let g = |dist: &FittedDistribution| -> f64 {
        exposures.iter().map(|&((n, lo, hi, cond), _)| n * probability(dist, lo, hi, cond)).sum()
    };
    let parameter_std_error = fit.delta_std_error(&g);
    let param_var = if parameter_std_error.is_finite() { parameter_std_error.powi(2) } else { 0.0 };
    let std_error = (var + param_var).sqrt();
    ForecastValue {
        period,
        expected,
        from_installed: installed,
        from_planned: planned,
        std_error,
        parameter_std_error,
        lower: (expected - z * std_error).max(0.0),
        upper: expected + z * std_error,
    }
}

/// Forecast returns for `horizon` future periods from the units still in
/// service and from `planned[m − 1]` units to be shipped in future period
/// `m`, with prediction limits at confidence `level` (see the module docs).
///
/// # Errors
/// A horizon of zero, a confidence level outside (0, 1), or a negative or
/// non-finite planned shipment.
pub fn forecast_returns(
    data: &WarrantyData,
    fit: &CensoredFit,
    horizon: usize,
    planned: &[f64],
    level: f64,
) -> Result<WarrantyForecast> {
    if horizon == 0 {
        return Err(invalid("the forecast needs at least one future period"));
    }
    if !(level > 0.0 && level < 1.0) {
        return Err(invalid("the confidence level must be between 0 and 1"));
    }
    if planned.iter().any(|m| !(m.is_finite() && *m >= 0.0)) {
        return Err(invalid("planned shipments must be non-negative"));
    }
    let z = Normal::standard().quantile(0.5 + level / 2.0)?;
    let cap = |a: f64| data.warranty.map_or(a, |w| a.min(w));
    // Exposures of future period k (from = first period counted).
    let exposures = |from: usize, k: usize| -> Vec<(Exposure, bool)> {
        let mut out = Vec::new();
        for c in &data.cohorts {
            let n = c.in_service();
            if n <= 0.0 {
                continue;
            }
            let age = (data.periods - c.period) as f64;
            let cond = cap(age);
            out.push(((n, cap(age + (from - 1) as f64), cap(age + k as f64), cond), true));
        }
        for (m0, &units) in planned.iter().enumerate() {
            let m = m0 + 1;
            if units <= 0.0 || m > k {
                continue;
            }
            let lo = from.max(m) - m;
            out.push(((units, cap(lo as f64), cap((k - m + 1) as f64), 0.0), false));
        }
        out
    };
    let periods = (1..=horizon).map(|k| value(fit, &exposures(k, k), z, k)).collect();
    let cumulative = (1..=horizon).map(|k| value(fit, &exposures(1, k), z, k)).collect();
    Ok(WarrantyForecast { periods, cumulative })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::censored_fit::fit_censored;
    use stat_core::distribution_fit::FitFamily;

    fn data() -> WarrantyData {
        WarrantyData::new(
            vec![
                WarrantyCohort { period: 0, shipped: 1000.0, returns: vec![5.0, 9.0, 12.0, 14.0] },
                WarrantyCohort { period: 1, shipped: 1200.0, returns: vec![6.0, 11.0, 15.0] },
                WarrantyCohort { period: 2, shipped: 900.0, returns: vec![4.0, 8.0] },
                WarrantyCohort { period: 3, shipped: 1100.0, returns: vec![6.0] },
            ],
            4,
            None,
        )
        .unwrap()
    }

    #[test]
    fn life_rows_and_period_totals() {
        let d = data();
        let rows = d.life_rows();
        assert_eq!(rows.len(), 4 + 3 + 2 + 1 + 4);
        let total: f64 = rows.iter().map(|r| r.observation.weight).sum();
        assert_eq!(total, 4200.0);
        assert_eq!(d.returns_by_period(), vec![5.0, 15.0, 27.0, 43.0]);
        // Cohort 1 survivors are right-censored at age 3.
        let censored = rows.iter().find(|r| r.cohort == 1 && r.observation.upper.is_infinite()).unwrap();
        assert_eq!(censored.observation.lower, 3.0);
        assert_eq!(censored.observation.weight, 1200.0 - 32.0);
    }

    #[test]
    fn exponential_forecast_by_hand() {
        // For the exponential with unit-width intervals, the MLE is geometric:
        // q = e^{−λ} = T / (T + R) with T the periods survived and R returns.
        let d = data();
        let fit = fit_censored(&d.life_data().unwrap(), FitFamily::Exponential).unwrap();
        let mut survived = 0.0;
        let mut returned = 0.0;
        for c in d.cohorts() {
            for (a, &r) in c.returns.iter().enumerate() {
                survived += a as f64 * r;
                returned += r;
            }
            survived += c.in_service() * (d.periods() - c.period) as f64;
        }
        let q = survived / (survived + returned);
        let theta = -1.0 / q.ln();
        assert!((fit.distribution.scale - theta).abs() < 1e-6 * theta, "{} vs {theta}", fit.distribution.scale);
        // Memoryless: every unit in service returns in future period k with
        // probability q^{k−1}(1 − q) (computed with the fitted q).
        let qf = (-1.0 / fit.distribution.scale).exp();
        let installed: f64 = d.cohorts().iter().map(WarrantyCohort::in_service).sum();
        let f = forecast_returns(&d, &fit, 3, &[500.0], 0.95).unwrap();
        for (k, v) in f.periods.iter().enumerate() {
            let p = qf.powi(k as i32) * (1.0 - qf);
            // Shipped at the start of future period 1, so returnable from then.
            let planned = 500.0 * p;
            assert!((v.from_installed - installed * p).abs() < 1e-9 * installed, "{v:?}");
            assert!((v.from_planned - planned).abs() < 1e-9 * 500.0);
            assert!(v.lower < v.expected && v.expected < v.upper);
        }
        // Cumulative over three periods.
        let c = &f.cumulative[2];
        let want = (installed + 500.0) * (1.0 - qf.powi(3));
        assert!((c.expected - want).abs() < 1e-9 * want);
        let sum: f64 = f.periods.iter().map(|v| v.expected).sum();
        assert!((sum - c.expected).abs() < 1e-9 * want);
    }

    #[test]
    fn warranty_caps_ages_and_errors() {
        let mut cohorts = data().cohorts().to_vec();
        assert!(WarrantyData::new(cohorts.clone(), 4, Some(3.0)).is_err());
        cohorts[0].returns.pop();
        let d = WarrantyData::new(cohorts.clone(), 4, Some(3.0)).unwrap();
        // The oldest cohort is censored at the warranty end, not its age 4.
        let rows = d.life_rows();
        let oldest = rows.iter().find(|r| r.cohort == 0 && r.observation.upper.is_infinite()).unwrap();
        assert_eq!(oldest.observation.lower, 3.0);
        assert!(WarrantyData::new(cohorts.clone(), 4, Some(2.0)).is_err());
        cohorts[0].returns[0] = 2000.0;
        assert!(WarrantyData::new(cohorts, 4, None).is_err());
        assert!(WarrantyData::new(Vec::new(), 4, None).is_err());
    }
}
