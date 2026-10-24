//! Bias and repeatability of an attribute (go/no-go) gauge from acceptance
//! counts at known reference values: the analytic method described in AIAG,
//! *Measurement Systems Analysis* (4th ed., 2010), ch. III §C ("Attribute
//! Measurement Systems Study — Analytic Method").
//!
//! Parts with known reference values `xᵢ` near one specification limit are
//! each checked `m` times (AIAG: `m = 20`) and the acceptances `aᵢ` counted.
//! The probability of acceptance is estimated with a half-count adjustment
//! away from 0 and 1:
//!
//! ```text
//! P′ᵢ = (aᵢ + 0.5)/m   when aᵢ/m < 0.5 and aᵢ ≠ 0
//! P′ᵢ = (aᵢ − 0.5)/m   when aᵢ/m > 0.5 and aᵢ ≠ m
//! P′ᵢ = 0.5            when aᵢ/m = 0.5
//! ```
//!
//! The gauge performance curve is taken to be a normal distribution
//! function, so `Φ⁻¹(P′)` is linear in the reference value. The parts with
//! `0 < aᵢ < m` give the least-squares line `Φ⁻¹(P′) = b₀ + b₁ x`, and
//! `x(P) = (Φ⁻¹(P) − b₀)/b₁` is the reference value accepted with
//! probability `P`. Then
//!
//! ```text
//! bias          = limit − x(0.5)
//! repeatability = |x(0.995) − x(0.005)| / 1.08
//! t             = 31.3 · |bias| / repeatability     on m − 1 df
//! ```
//!
//! with AIAG's adjustment factor 1.08 and constant 31.3 (both stated for
//! `m = 20` trials). Acceptance that rises with the reference value means the
//! gauge decides at a lower limit; falling acceptance, an upper limit. The
//! fitted curve's standard deviation `1/|b₁|` is the gauge's
//! repeatability standard deviation on the normal model.
//!
//! AIAG's design calls for at least eight parts: the part farthest inside the
//! limit accepted every time, the one farthest outside never accepted, and
//! at least six with `0 < aᵢ < m`; [`AttributeGaugeStudy::design_notes`]
//! reports which of these the data miss.
//!
//! # Failure modes
//! A reference value given twice, fewer than two reference values with `0 < aᵢ < m`, counts
//! outside `0 … m`, unequal numbers of trials, fewer than two trials, a flat
//! fit (`b₁ = 0`) or non-finite input.

use stat_core::distributions::{ContinuousDistribution, Normal, StudentsT};
use stat_core::{Result, StatError};

/// AIAG's adjustment factor for the repeatability (for 20 trials).
pub const REPEATABILITY_ADJUSTMENT: f64 = 1.08;
/// AIAG's constant of the bias t statistic (for 20 trials).
pub const BIAS_T_CONSTANT: f64 = 31.3;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Acceptances of one reference value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AcceptancePoint {
    /// Reference (true) value of the part.
    pub reference: f64,
    /// Times accepted.
    pub accepted: f64,
    /// Times checked.
    pub trials: f64,
}

/// One reference value of the analysed study.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AcceptanceEstimate {
    /// The reference value and its counts.
    pub point: AcceptancePoint,
    /// Adjusted probability of acceptance `P′`.
    pub adjusted: f64,
    /// Whether the point enters the fit (`0 < a < m`).
    pub fitted: bool,
}

/// Which specification limit the gauge decides at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitSide {
    /// Acceptance rises with the reference value.
    Lower,
    /// Acceptance falls as the reference value rises.
    Upper,
}

/// The analysed attribute gauge study.
#[derive(Debug, Clone, PartialEq)]
pub struct AttributeGaugeStudy {
    /// Reference values in increasing order.
    pub points: Vec<AcceptanceEstimate>,
    /// The specification limit.
    pub limit: f64,
    /// Side of the limit, from the sign of the slope.
    pub side: LimitSide,
    /// Intercept `b₀` of `Φ⁻¹(P′)` on the reference value.
    pub intercept: f64,
    /// Slope `b₁`.
    pub slope: f64,
    /// Reference value accepted half the time, `x(0.5)`.
    pub median_acceptance: f64,
    /// `x(0.005)` and `x(0.995)`.
    pub span: (f64, f64),
    /// `limit − x(0.5)`.
    pub bias: f64,
    /// `|x(0.995) − x(0.005)| / 1.08`.
    pub repeatability: f64,
    /// Standard deviation of the fitted normal performance curve, `1/|b₁|`.
    pub sigma: f64,
    /// `31.3 |bias| / repeatability`.
    pub t: f64,
    /// `m − 1`.
    pub df: f64,
    /// Two-sided p-value of `t`.
    pub p_value: f64,
    /// Trials per part `m`.
    pub trials: f64,
}

impl AttributeGaugeStudy {
    /// Probability of acceptance at reference value `x` on the fitted curve.
    #[must_use]
    pub fn acceptance_probability(&self, x: f64) -> f64 {
        Normal::standard().cdf(self.intercept + self.slope * x).unwrap_or(f64::NAN)
    }

    /// Ways in which the data depart from AIAG's recommended design.
    #[must_use]
    pub fn design_notes(&self) -> Vec<String> {
        let mut notes = Vec::new();
        let m = self.trials;
        if self.points.len() < 8 {
            notes.push(format!("{} reference values were checked; AIAG recommends at least eight.", self.points.len()));
        }
        let inside = self.points.iter().filter(|p| p.fitted).count();
        if inside < 6 {
            notes.push(format!(
                "{inside} reference values were accepted some but not all of the time; AIAG recommends at least six."
            ));
        }
        let (first, last) = (self.points[0].point.accepted, self.points[self.points.len() - 1].point.accepted);
        let (low_end, high_end) = match self.side {
            LimitSide::Lower => (0.0, m),
            LimitSide::Upper => (m, 0.0),
        };
        if first != low_end || last != high_end {
            notes.push(
                "The smallest and largest reference values should be accepted never and always (or always and \
                 never at an upper limit)."
                    .to_owned(),
            );
        }
        if (m - 20.0).abs() > 0.0 {
            notes.push(format!(
                "Each part was checked {m} times; the constants 1.08 and 31.3 are AIAG's for 20 trials."
            ));
        }
        notes
    }
}

/// Adjusted probability of acceptance `P′` of `a` acceptances in `m` trials.
#[must_use]
pub fn adjusted_acceptance(a: f64, m: f64) -> f64 {
    let p = a / m;
    if a <= 0.0 {
        0.0
    } else if a >= m {
        1.0
    } else if p < 0.5 {
        (a + 0.5) / m
    } else if p > 0.5 {
        (a - 0.5) / m
    } else {
        0.5
    }
}

/// Analyse acceptance counts at reference values against `limit` by the AIAG
/// analytic method (see the module documentation).
///
/// # Errors
/// [`StatError::InvalidArgument`] for non-finite values, counts outside
/// `0 … trials`, a reference value given twice, unequal or fewer than two
/// trials, fewer than two
/// reference values accepted some but not all of the time, or a flat fit.
pub fn attribute_gauge_study(points: &[AcceptancePoint], limit: f64) -> Result<AttributeGaugeStudy> {
    if !limit.is_finite() || points.iter().any(|p| !(p.reference.is_finite() && p.accepted.is_finite())) {
        return Err(StatError::NonFiniteInput { context: "attribute gauge study" });
    }
    let m = points.first().map_or(0.0, |p| p.trials);
    if points.iter().any(|p| p.trials != m) {
        return Err(invalid("every reference value must be checked the same number of times"));
    }
    if !(m >= 2.0 && m.fract() == 0.0) {
        return Err(invalid("each reference value needs at least two checks"));
    }
    if points.iter().any(|p| p.accepted < 0.0 || p.accepted > p.trials || p.accepted.fract() != 0.0) {
        return Err(invalid("acceptances must be whole numbers between 0 and the number of checks"));
    }
    let mut merged = points.to_vec();
    merged.sort_by(|a, b| a.reference.total_cmp(&b.reference));
    if let Some(w) = merged.windows(2).find(|w| w[0].reference == w[1].reference) {
        return Err(invalid(format!(
            "reference value {} appears twice; give each reference value one count",
            w[0].reference
        )));
    }
    let estimates: Vec<AcceptanceEstimate> = merged
        .iter()
        .map(|&point| AcceptanceEstimate {
            point,
            adjusted: adjusted_acceptance(point.accepted, point.trials),
            fitted: point.accepted > 0.0 && point.accepted < point.trials,
        })
        .collect();
    let normal = Normal::standard();
    let mut xs = Vec::new();
    let mut zs = Vec::new();
    for e in estimates.iter().filter(|e| e.fitted) {
        xs.push(e.point.reference);
        zs.push(normal.quantile(e.adjusted)?);
    }
    if xs.len() < 2 {
        return Err(invalid(
            "at least two reference values must be accepted some but not all of the time to fit the gauge curve",
        ));
    }
    let k = xs.len() as f64;
    let (mx, mz) = (xs.iter().sum::<f64>() / k, zs.iter().sum::<f64>() / k);
    let sxx: f64 = xs.iter().map(|x| (x - mx).powi(2)).sum();
    let sxz: f64 = xs.iter().zip(&zs).map(|(x, z)| (x - mx) * (z - mz)).sum();
    if sxx <= 0.0 || sxz == 0.0 {
        return Err(invalid("the probability of acceptance does not change with the reference value"));
    }
    let slope = sxz / sxx;
    let intercept = mz - slope * mx;
    let at = |p: f64| normal.quantile(p).map(|z| (z - intercept) / slope);
    let median_acceptance = -intercept / slope;
    let span = (at(0.005)?, at(0.995)?);
    let repeatability = (span.1 - span.0).abs() / REPEATABILITY_ADJUSTMENT;
    let bias = limit - median_acceptance;
    let t = BIAS_T_CONSTANT * bias.abs() / repeatability;
    let df = m - 1.0;
    let p_value = (2.0 * StudentsT::new(df)?.sf(t)?).min(1.0);
    Ok(AttributeGaugeStudy {
        points: estimates,
        limit,
        side: if slope > 0.0 { LimitSide::Lower } else { LimitSide::Upper },
        intercept,
        slope,
        median_acceptance,
        span,
        bias,
        repeatability,
        sigma: 1.0 / slope.abs(),
        t,
        df,
        p_value,
        trials: m,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjustment_rules() {
        assert_eq!(adjusted_acceptance(0.0, 20.0), 0.0);
        assert_eq!(adjusted_acceptance(20.0, 20.0), 1.0);
        assert!((adjusted_acceptance(3.0, 20.0) - 0.175).abs() < 1e-15);
        assert!((adjusted_acceptance(15.0, 20.0) - 0.725).abs() < 1e-15);
        assert_eq!(adjusted_acceptance(10.0, 20.0), 0.5);
    }

    /// Counts generated from a known normal curve recover its centre and
    /// spread when the counts are the expected ones.
    #[test]
    fn recovers_an_exact_probit_line() {
        // Points chosen so that P′ is exactly Φ((x − 0.5)/0.1): only the
        // half-count adjustment matters, so build them from P′ directly.
        let normal = Normal::standard();
        let pts: Vec<AcceptancePoint> = (0..=9)
            .map(|i| {
                let x = 0.3 + 0.045 * f64::from(i);
                let a = (20.0 * normal.cdf((x - 0.5) / 0.1).unwrap()).round();
                AcceptancePoint { reference: x, accepted: a, trials: 20.0 }
            })
            .collect();
        let s = attribute_gauge_study(&pts, 0.5).unwrap();
        assert_eq!(s.side, LimitSide::Lower);
        assert!((s.median_acceptance - 0.5).abs() < 0.02, "{}", s.median_acceptance);
        assert!((s.sigma - 0.1).abs() < 0.03, "{}", s.sigma);
        assert!(s.p_value > 0.0 && s.p_value <= 1.0);
        let falling: Vec<AcceptancePoint> =
            pts.iter().map(|p| AcceptancePoint { reference: -p.reference, ..*p }).collect();
        assert_eq!(attribute_gauge_study(&falling, -0.5).unwrap().side, LimitSide::Upper);
    }

    #[test]
    fn refuses_bad_studies() {
        let p = |x: f64, a: f64| AcceptancePoint { reference: x, accepted: a, trials: 20.0 };
        assert!(attribute_gauge_study(&[p(1.0, 0.0), p(2.0, 5.0), p(3.0, 20.0)], 2.0).is_err(), "one inside");
        assert!(attribute_gauge_study(&[p(1.0, 3.0), p(2.0, 25.0)], 2.0).is_err());
        assert!(attribute_gauge_study(&[p(1.0, 3.0), p(1.0, 5.0)], 2.0).is_err());
        let uneven = [p(1.0, 3.0), AcceptancePoint { reference: 2.0, accepted: 5.0, trials: 10.0 }];
        assert!(attribute_gauge_study(&uneven, 2.0).is_err());
        assert!(attribute_gauge_study(&[p(1.0, 3.0), p(2.0, 3.0)], 2.0).is_err(), "flat");
        assert!(attribute_gauge_study(&[p(1.0, 3.0), p(2.0, 6.0)], f64::NAN).is_err());
    }
}
