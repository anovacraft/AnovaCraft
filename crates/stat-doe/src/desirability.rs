//! Desirability functions for optimizing several responses at once
//! (Derringer & Suich, "Simultaneous optimization of several response
//! variables", *Journal of Quality Technology* 12(4), 1980).
//!
//! Each response `ŷ` is mapped to a desirability `d ∈ [0, 1]`:
//!
//! * **maximize** with lower limit `L` and upper limit `U`:
//!   `d = 0` for `ŷ ≤ L`, `((ŷ − L)/(U − L))^s` between, `1` for `ŷ ≥ U`;
//! * **minimize**: `d = 1` for `ŷ ≤ L`, `((U − ŷ)/(U − L))^s` between, `0`
//!   for `ŷ ≥ U`;
//! * **hit a target** `T` with `L < T < U`: `((ŷ − L)/(T − L))^s` from `L`
//!   to `T`, `((U − ŷ)/(U − T))^t` from `T` to `U`, `0` outside `[L, U]`.
//!
//! The exponents (`s`, `t` > 0) shape the curve: 1 is linear, larger values
//! demand values close to the best, smaller ones accept values near the
//! limit. The overall desirability is the weighted geometric mean
//! `D = (Π dᵢ^wᵢ)^(1/Σwᵢ)` over the responses with a goal (importance `wᵢ`);
//! it is 0 as soon as any one response is unacceptable.

use stat_core::{Result, StatError};

/// What is wanted of a response.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Goal {
    /// Larger is better.
    Maximize,
    /// Smaller is better.
    Minimize,
    /// Closest to this value is best.
    Target(f64),
    /// The response is shown but does not enter the overall desirability.
    None,
}

/// A response's desirability function.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Desirability {
    /// What is wanted.
    pub goal: Goal,
    /// Lower limit `L`.
    pub low: f64,
    /// Upper limit `U`.
    pub high: f64,
    /// Exponent `s` (the whole curve for maximize and minimize, the part
    /// below the target otherwise).
    pub shape: f64,
    /// Exponent `t` above a target (unused otherwise).
    pub shape_above: f64,
    /// Importance `w > 0` in the overall desirability.
    pub importance: f64,
}

impl Desirability {
    /// A linear (`s = t = 1`), equally important desirability.
    #[must_use]
    pub fn new(goal: Goal, low: f64, high: f64) -> Self {
        Self { goal, low, high, shape: 1.0, shape_above: 1.0, importance: 1.0 }
    }

    /// Check the limits, target, exponents and importance.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] unless `L < U` (and `L < T < U` for a
    /// target), the exponents are positive and the importance is positive,
    /// all finite.
    pub fn validate(&self) -> Result<()> {
        let bad = |m: &str| Err(StatError::InvalidArgument { message: m.to_owned() });
        if self.goal == Goal::None {
            return Ok(());
        }
        if !(self.low.is_finite() && self.high.is_finite() && self.low < self.high) {
            return bad("desirability: the lower limit must be below the upper limit");
        }
        if let Goal::Target(t) = self.goal
            && !(t > self.low && t < self.high)
        {
            return bad("desirability: the target must lie between the limits");
        }
        if !(self.shape > 0.0 && self.shape.is_finite() && self.shape_above > 0.0 && self.shape_above.is_finite()) {
            return bad("desirability: the shape exponents must be positive");
        }
        if !(self.importance > 0.0 && self.importance.is_finite()) {
            return bad("desirability: the importance must be positive");
        }
        Ok(())
    }

    /// The desirability of response value `y` (NaN for a NaN `y` or a
    /// response without a goal).
    #[must_use]
    pub fn of(&self, y: f64) -> f64 {
        if y.is_nan() {
            return f64::NAN;
        }
        let (l, u) = (self.low, self.high);
        match self.goal {
            Goal::None => f64::NAN,
            Goal::Maximize => {
                if y <= l {
                    0.0
                } else if y >= u {
                    1.0
                } else {
                    ((y - l) / (u - l)).powf(self.shape)
                }
            }
            Goal::Minimize => {
                if y <= l {
                    1.0
                } else if y >= u {
                    0.0
                } else {
                    ((u - y) / (u - l)).powf(self.shape)
                }
            }
            Goal::Target(t) => {
                if y < l || y > u {
                    0.0
                } else if y <= t {
                    ((y - l) / (t - l)).powf(self.shape)
                } else {
                    ((u - y) / (u - t)).powf(self.shape_above)
                }
            }
        }
    }
}

/// Overall desirability `D = (Π dᵢ^wᵢ)^(1/Σwᵢ)` of individual
/// desirabilities `d` with importances `weights`, skipping NaN entries
/// (responses without a goal). NaN when no response has a goal.
#[must_use]
pub fn overall_desirability(d: &[f64], weights: &[f64]) -> f64 {
    let (mut log_sum, mut total) = (0.0, 0.0);
    for (&di, &w) in d.iter().zip(weights) {
        if di.is_nan() {
            continue;
        }
        if di <= 0.0 {
            return 0.0;
        }
        log_sum += w * di.ln();
        total += w;
    }
    if total > 0.0 { (log_sum / total).exp() } else { f64::NAN }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-12, "{a} vs {b}");
    }

    #[test]
    fn one_sided_and_target_functions() {
        // Limits like Derringer & Suich's abrasion index (maximize from 120 to 170).
        let up = Desirability::new(Goal::Maximize, 120.0, 170.0);
        close(up.of(110.0), 0.0);
        close(up.of(145.0), 0.5);
        close(up.of(180.0), 1.0);
        let curved = Desirability { shape: 2.0, ..up };
        close(curved.of(145.0), 0.25);
        let down = Desirability::new(Goal::Minimize, 10.0, 20.0);
        close(down.of(12.5), 0.75);
        close(down.of(5.0), 1.0);
        close(down.of(25.0), 0.0);
        // Target 500 within [400, 600] (their elongation response); asymmetric shapes.
        let target =
            Desirability { shape: 1.0, shape_above: 0.5, ..Desirability::new(Goal::Target(500.0), 400.0, 600.0) };
        close(target.of(450.0), 0.5);
        close(target.of(500.0), 1.0);
        close(target.of(575.0), 0.5);
        close(target.of(399.0), 0.0);
        close(target.of(601.0), 0.0);
        assert!(target.validate().is_ok());
        assert!(Desirability::new(Goal::Target(700.0), 400.0, 600.0).validate().is_err());
        assert!(Desirability::new(Goal::Maximize, 3.0, 3.0).validate().is_err());
        assert!(Desirability::new(Goal::None, 0.0, 0.0).of(1.0).is_nan());
    }

    #[test]
    fn overall_is_the_weighted_geometric_mean() {
        close(overall_desirability(&[0.25, 1.0], &[1.0, 1.0]), 0.5);
        // Importance 3 on the first: (0.25³ · 1)^(1/4).
        close(overall_desirability(&[0.25, 1.0], &[3.0, 1.0]), 0.25_f64.powf(0.75));
        close(overall_desirability(&[0.5, f64::NAN], &[1.0, 1.0]), 0.5);
        close(overall_desirability(&[0.9, 0.0, 0.8], &[1.0, 1.0, 1.0]), 0.0);
        assert!(overall_desirability(&[f64::NAN], &[1.0]).is_nan());
    }
}
