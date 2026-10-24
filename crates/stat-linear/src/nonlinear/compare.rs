//! Comparing two least-squares fits of the same data.
//!
//! # Extra sum-of-squares F test (Bates & Watts 1988, §3.10; Draper & Smith,
//! *Applied Regression Analysis*, 3rd ed., §24.3)
//! For a simpler model nested in a more complex one (the simpler is the
//! complex model with some parameters fixed or set equal), with residual sums
//! of squares `S₀ > S₁` on `ν₀ > ν₁` residual degrees of freedom,
//! `F = ((S₀ − S₁)/(ν₀ − ν₁)) / (S₁/ν₁)` on `(ν₀ − ν₁, ν₁)` df; a small p
//! says the extra parameters reduce the sum of squares by more than chance.
//! For nonlinear models the F distribution is approximate (exact when the
//! models are linear). It is not valid for models that are not nested.
//!
//! # AICc (Burnham & Anderson, *Model Selection and Multimodel Inference*,
//! 2nd ed., Springer 2002, §2.4 and §2.9)
//! For a least-squares fit with `P` estimated parameters to `n` observations,
//! `K = P + 1` (the error variance counts) and
//! `AICc = n·ln(S/n) + 2K + 2K(K + 1)/(n − K − 1)`. The model with the lower
//! AICc is better supported; with `Δ = AICc₀ − AICc₁`, the probability that
//! model 1 is the better of the two is `e^{Δ/2} / (1 + e^{Δ/2})` (Akaike
//! weights). Unlike the F test it does not need nested models, but both fits
//! must use the same observations (and the same weights).

use stat_core::distributions::{ContinuousDistribution, FisherSnedecor};
use stat_core::{Result, StatError};

/// One fit's summary for a comparison.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FitScore {
    /// Residual (weighted) sum of squares.
    pub sse: f64,
    /// Number of observations.
    pub n: usize,
    /// Number of estimated parameters.
    pub parameters: usize,
}

impl FitScore {
    /// Residual degrees of freedom `n − P`.
    #[must_use]
    pub fn df(&self) -> usize {
        self.n.saturating_sub(self.parameters)
    }

    /// AICc (module documentation); `NaN` when `n ≤ K + 1` or `S ≤ 0`.
    #[must_use]
    pub fn aicc(&self) -> f64 {
        let n = self.n as f64;
        let k = (self.parameters + 1) as f64;
        if n - k - 1.0 <= 0.0 || self.sse.is_nan() || self.sse <= 0.0 {
            return f64::NAN;
        }
        n * (self.sse / n).ln() + 2.0 * k + 2.0 * k * (k + 1.0) / (n - k - 1.0)
    }
}

/// An extra sum-of-squares F test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExtraSumOfSquares {
    /// `F`.
    pub f: f64,
    /// Numerator df `ν₀ − ν₁`.
    pub df_numerator: usize,
    /// Denominator df `ν₁`.
    pub df_denominator: usize,
    /// Upper-tail p-value.
    pub p_value: f64,
}

/// The extra sum-of-squares F test of `simple` against `complex` (module
/// documentation).
///
/// # Errors
/// [`StatError::InvalidArgument`] when the fits have different numbers of
/// observations, `simple` does not have more residual df than `complex`, or
/// `complex` has no residual df or a zero sum of squares.
pub fn extra_sum_of_squares(simple: FitScore, complex: FitScore) -> Result<ExtraSumOfSquares> {
    if simple.n != complex.n {
        return Err(StatError::InvalidArgument { message: "the two fits must use the same observations".into() });
    }
    let (d0, d1) = (simple.df(), complex.df());
    if d0 <= d1 {
        return Err(StatError::InvalidArgument {
            message: "the simpler model must have fewer parameters than the more complex one".into(),
        });
    }
    if d1 == 0 || complex.sse.is_nan() || complex.sse <= 0.0 {
        return Err(StatError::InvalidArgument {
            message: "the more complex model fits exactly, so there is no error variance to test against".into(),
        });
    }
    let num = (simple.sse - complex.sse).max(0.0) / (d0 - d1) as f64;
    let den = complex.sse / d1 as f64;
    let f = num / den;
    let p_value = FisherSnedecor::new((d0 - d1) as f64, d1 as f64)?.sf(f)?;
    Ok(ExtraSumOfSquares { f, df_numerator: d0 - d1, df_denominator: d1, p_value })
}

/// Probability (Akaike weight) that model `b` is the better of `a` and `b`
/// by AICc; `NaN` when either AICc is undefined.
#[must_use]
pub fn aicc_probability(a: FitScore, b: FitScore) -> f64 {
    let delta = a.aicc() - b.aicc();
    if !delta.is_finite() {
        return f64::NAN;
    }
    // e^{Δ/2} / (1 + e^{Δ/2}) written to avoid overflow.
    1.0 / (1.0 + (-delta / 2.0).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aicc_and_f_by_hand() {
        let a = FitScore { sse: 10.0, n: 20, parameters: 2 };
        let b = FitScore { sse: 6.0, n: 20, parameters: 4 };
        // n ln(S/n) + 2K + 2K(K+1)/(n−K−1), K = P + 1.
        let ea = 20.0 * (0.5f64).ln() + 6.0 + 24.0 / 16.0;
        let eb = 20.0 * (0.3f64).ln() + 10.0 + 60.0 / 14.0;
        assert!((a.aicc() - ea).abs() < 1e-12 && (b.aicc() - eb).abs() < 1e-12);
        let pr = aicc_probability(a, b);
        assert!((pr - 1.0 / (1.0 + (-(ea - eb) / 2.0).exp())).abs() < 1e-12);
        let t = extra_sum_of_squares(a, b).unwrap();
        assert_eq!((t.df_numerator, t.df_denominator), (2, 16));
        assert!((t.f - (4.0 / 2.0) / (6.0 / 16.0)).abs() < 1e-12);
        // F(2, 16) upper tail at 5.3333: (1 + 2F/16)^(-8) for 2 numerator df.
        let exact = (1.0 + 2.0 * t.f / 16.0f64).powf(-8.0);
        assert!((t.p_value - exact).abs() < 1e-12, "{} vs {exact}", t.p_value);
        assert!(extra_sum_of_squares(b, a).is_err());
    }
}
