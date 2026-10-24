//! Noncentral t, chi-square and F distributions, used for the power of t, F
//! and chi-square tests.
//!
//! Each is evaluated as a Poisson mixture of central distributions, summed
//! outward from the largest Poisson weight until the remaining terms are
//! negligible (Johnson, Kotz & Balakrishnan, *Continuous Univariate
//! Distributions*, vol. 2, chs. 29–31):
//!
//! * **Noncentral χ²** with `k` df and noncentrality `λ`:
//!   `P(X ≤ x) = Σⱼ e^{−λ/2}(λ/2)ʲ/j! · P(k/2 + j, x/2)` (regularized gamma).
//! * **Noncentral F** with `(d₁, d₂)` df and noncentrality `λ`:
//!   `P(F ≤ f) = Σⱼ e^{−λ/2}(λ/2)ʲ/j! · I_x(d₁/2 + j, d₂/2)`,
//!   `x = d₁f/(d₁f + d₂)`.
//! * **Noncentral t** with `ν` df and noncentrality `δ` (Lenth 1989, *Applied
//!   Statistics* 38, AS 243): for `t ≥ 0`, with `x = t²/(t² + ν)`,
//!   `P(T ≤ t) = Φ(−δ) + ½ Σⱼ [pⱼ I_x(j + ½, ν/2) + qⱼ I_x(j + 1, ν/2)]`,
//!   `pⱼ = e^{−δ²/2}(δ²/2)ʲ/j!`, `qⱼ = δ e^{−δ²/2}(δ²/2)ʲ / (√2 Γ(j + 3/2))`;
//!   for `t < 0`, `P(T ≤ t; δ) = 1 − P(T ≤ −t; −δ)`.
//!
//! Upper tails are summed directly from upper-tail central probabilities where
//! the mixture allows it (χ², F), so small tail probabilities keep relative
//! precision.

use crate::distributions::{ContinuousDistribution, Normal, StudentsT};
use crate::error::{Result, StatError, invalid};
use crate::special::{
    ln_gamma_difference, poisson_probability, regularized_gamma_p, regularized_gamma_q,
    regularized_incomplete_beta_pair,
};

/// Sum `Σⱼ w(j) g(j)` over a Poisson(`mean`) weight `w`, outward from the mode.
pub(crate) fn poisson_mixture(mean: f64, mut g: impl FnMut(f64) -> Result<f64>) -> Result<f64> {
    if mean == 0.0 {
        return g(0.0);
    }
    let weight = |j: f64| poisson_probability(j, mean);
    let mode = mean.floor();
    let mut total = 0.0;
    // Upward from the mode.
    let mut j = mode;
    loop {
        let w = weight(j);
        total += w * g(j)?;
        // A weight of exactly 0 ends the sum even when the total is 0 too
        // (every term underflowed), where the relative test never fires.
        if (j > mode + 5.0 && (w == 0.0 || w < 1e-17 * total.max(f64::MIN_POSITIVE))) || j > mode + 1e6 {
            break;
        }
        j += 1.0;
    }
    // Downward.
    let mut j = mode - 1.0;
    while j >= 0.0 {
        let w = weight(j);
        total += w * g(j)?;
        if w == 0.0 || w < 1e-17 * total.max(f64::MIN_POSITIVE) {
            break;
        }
        j -= 1.0;
    }
    Ok(total)
}

fn check(name: &str, value: f64, allow_zero: bool) -> Result<()> {
    let ok = value.is_finite() && if allow_zero { value >= 0.0 } else { value > 0.0 };
    if ok {
        Ok(())
    } else {
        Err(invalid(format!(
            "{name} must be {} and finite (got {value})",
            if allow_zero { "non-negative" } else { "positive" }
        )))
    }
}

/// Noncentral chi-square distribution.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoncentralChiSquared {
    df: f64,
    noncentrality: f64,
}

impl NoncentralChiSquared {
    /// `df > 0` degrees of freedom and noncentrality `λ ≥ 0`.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for invalid parameters.
    pub fn new(df: f64, noncentrality: f64) -> Result<Self> {
        check("degrees of freedom", df, false)?;
        check("noncentrality", noncentrality, true)?;
        Ok(Self { df, noncentrality })
    }

    /// `P(X ≤ x)`.
    ///
    /// # Errors
    /// [`StatError::NonFiniteInput`] for NaN; numerical failures.
    pub fn cdf(&self, x: f64) -> Result<f64> {
        if x.is_nan() {
            return Err(StatError::NonFiniteInput { context: "noncentral chi-square value" });
        }
        if x <= 0.0 {
            return Ok(0.0);
        }
        if x.is_infinite() {
            return Ok(1.0);
        }
        poisson_mixture(self.noncentrality / 2.0, |j| regularized_gamma_p(self.df / 2.0 + j, x / 2.0))
            .map(|v| v.min(1.0))
    }

    /// `P(X > x)`.
    ///
    /// # Errors
    /// As [`cdf`](Self::cdf).
    pub fn sf(&self, x: f64) -> Result<f64> {
        if x.is_nan() {
            return Err(StatError::NonFiniteInput { context: "noncentral chi-square value" });
        }
        if x <= 0.0 {
            return Ok(1.0);
        }
        if x.is_infinite() {
            return Ok(0.0);
        }
        poisson_mixture(self.noncentrality / 2.0, |j| regularized_gamma_q(self.df / 2.0 + j, x / 2.0))
            .map(|v| v.min(1.0))
    }
}

/// Noncentral F distribution.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoncentralF {
    df1: f64,
    df2: f64,
    noncentrality: f64,
}

impl NoncentralF {
    /// Degrees of freedom `d₁, d₂ > 0` and noncentrality `λ ≥ 0`.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for invalid parameters.
    pub fn new(df1: f64, df2: f64, noncentrality: f64) -> Result<Self> {
        check("numerator degrees of freedom", df1, false)?;
        check("denominator degrees of freedom", df2, false)?;
        check("noncentrality", noncentrality, true)?;
        Ok(Self { df1, df2, noncentrality })
    }

    fn xy(&self, f: f64) -> (f64, f64) {
        let a = self.df1 * f;
        if !(a + self.df2).is_finite() {
            // d₁f overflows: the pair is (1, d₂/(d₁f)) to working precision.
            return (1.0, self.df2 / self.df1 / f);
        }
        (a / (a + self.df2), self.df2 / (a + self.df2))
    }

    /// `P(F ≤ f)`.
    ///
    /// # Errors
    /// [`StatError::NonFiniteInput`] for NaN; numerical failures.
    pub fn cdf(&self, f: f64) -> Result<f64> {
        if f.is_nan() {
            return Err(StatError::NonFiniteInput { context: "noncentral F value" });
        }
        if f <= 0.0 {
            return Ok(0.0);
        }
        if f.is_infinite() {
            return Ok(1.0);
        }
        let (x, y) = self.xy(f);
        let (a, b) = (self.df1 / 2.0, self.df2 / 2.0);
        poisson_mixture(self.noncentrality / 2.0, |j| regularized_incomplete_beta_pair(a + j, b, x, y))
            .map(|v| v.min(1.0))
    }

    /// `P(F > f)`.
    ///
    /// # Errors
    /// As [`cdf`](Self::cdf).
    pub fn sf(&self, f: f64) -> Result<f64> {
        if f.is_nan() {
            return Err(StatError::NonFiniteInput { context: "noncentral F value" });
        }
        if f <= 0.0 {
            return Ok(1.0);
        }
        if f.is_infinite() {
            return Ok(0.0);
        }
        let (x, y) = self.xy(f);
        let (a, b) = (self.df1 / 2.0, self.df2 / 2.0);
        poisson_mixture(self.noncentrality / 2.0, |j| regularized_incomplete_beta_pair(b, a + j, y, x))
            .map(|v| v.min(1.0))
    }
}

/// Noncentral t distribution.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoncentralT {
    df: f64,
    noncentrality: f64,
}

impl NoncentralT {
    /// `ν > 0` degrees of freedom and noncentrality `δ` (any finite value).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for invalid parameters.
    pub fn new(df: f64, noncentrality: f64) -> Result<Self> {
        check("degrees of freedom", df, false)?;
        if !noncentrality.is_finite() {
            return Err(invalid("noncentrality must be finite"));
        }
        Ok(Self { df, noncentrality })
    }

    /// `P(T ≤ t)`.
    ///
    /// # Errors
    /// [`StatError::NonFiniteInput`] for NaN; numerical failures.
    pub fn cdf(&self, t: f64) -> Result<f64> {
        if t.is_nan() {
            return Err(StatError::NonFiniteInput { context: "noncentral t value" });
        }
        if t.is_infinite() {
            return Ok(if t > 0.0 { 1.0 } else { 0.0 });
        }
        if t >= 0.0 {
            lower_probability(t, self.df, self.noncentrality)
        } else {
            upper_half_sf(-t, self.df, -self.noncentrality)
        }
    }

    /// `P(T > t)`.
    ///
    /// # Errors
    /// As [`cdf`](Self::cdf).
    pub fn sf(&self, t: f64) -> Result<f64> {
        if t.is_nan() {
            return Err(StatError::NonFiniteInput { context: "noncentral t value" });
        }
        if t.is_infinite() {
            return Ok(if t > 0.0 { 0.0 } else { 1.0 });
        }
        if t >= 0.0 {
            upper_half_sf(t, self.df, self.noncentrality)
        } else {
            lower_probability(-t, self.df, -self.noncentrality)
        }
    }
}

/// `P(T ≤ t)` for `t ≥ 0`: `1 −` the directly summed upper tail when that tail
/// is the smaller side (so the cdf reaches 1 exactly in the far tail),
/// otherwise Lenth's series.
fn lower_probability(t: f64, df: f64, delta: f64) -> Result<f64> {
    let sf = upper_half_sf(t, df, delta)?;
    if sf < 0.5 { Ok(1.0 - sf) } else { upper_half_cdf(t, df, delta) }
}

/// Lenth's series for `t ≥ 0`.
fn upper_half_cdf(t: f64, df: f64, delta: f64) -> Result<f64> {
    if delta == 0.0 {
        return StudentsT::new(df)?.cdf(t);
    }
    let phi = Normal::standard().cdf(-delta)?;
    if t == 0.0 {
        return Ok(phi);
    }
    let (x, y) = beta_pair(t, df);
    let b = 0.5 * df;
    let total = lenth_series(delta, |a| regularized_incomplete_beta_pair(a, b, x, y))?;
    Ok((phi + 0.5 * total).clamp(0.0, 1.0))
}

/// `P(T > t)` for `t ≥ 0`, summed directly so that far upper tails keep
/// relative precision. Since `Σⱼ pⱼ = 1` and `Σⱼ qⱼ = 2Φ(δ) − 1`, subtracting
/// Lenth's series from 1 leaves `½ Σⱼ [pⱼ I_{1−x}(ν/2, j + ½) + qⱼ I_{1−x}(ν/2, j + 1)]`.
fn upper_half_sf(t: f64, df: f64, delta: f64) -> Result<f64> {
    if delta == 0.0 {
        return StudentsT::new(df)?.sf(t);
    }
    if t == 0.0 {
        return Normal::standard().sf(-delta);
    }
    let (x, y) = beta_pair(t, df);
    let b = 0.5 * df;
    let total = lenth_series(delta, |a| regularized_incomplete_beta_pair(b, a, y, x))?;
    Ok((0.5 * total).clamp(0.0, 1.0))
}

/// `(t²/(t² + ν), ν/(t² + ν))`; when `t²` overflows, `(1, ν/t²)` to working precision.
fn beta_pair(t: f64, df: f64) -> (f64, f64) {
    if (t * t + df).is_finite() { (t * t / (t * t + df), df / (t * t + df)) } else { (1.0, df / t / t) }
}

/// `Σⱼ [pⱼ B(j + ½) + qⱼ B(j + 1)]` for Lenth's weights `pⱼ`, `qⱼ` (module
/// docs), summed outward from the Poisson mode; `beta(a)` is the central beta
/// probability with first index `a`.
fn lenth_series(delta: f64, beta: impl Fn(f64) -> Result<f64>) -> Result<f64> {
    let lambda = 0.5 * delta * delta;
    let ln_delta = delta.abs().ln() - 0.5 * std::f64::consts::LN_2;
    let sign = delta.signum();
    let term = |j: f64| -> Result<(f64, f64)> {
        let p = poisson_probability(j, lambda);
        // λʲe^{−λ}/Γ(j + 3/2) · δ/√2 = p · exp(ln(δ/√2) − [ln Γ(j + 3/2) − ln Γ(j + 1)]).
        let q = sign * p * (ln_delta - ln_gamma_difference(j + 1.0, 0.5)).exp();
        let v = p * beta(j + 0.5)? + q * beta(j + 1.0)?;
        Ok((v, p + q.abs()))
    };
    let mode = lambda.floor();
    let mut total = 0.0;
    let mut weights = 0.0;
    let mut j = mode;
    loop {
        let (v, w) = term(j)?;
        total += v;
        weights += w;
        if (j > mode + 5.0 && (w == 0.0 || w < 1e-17 * weights)) || j > mode + 1e6 {
            break;
        }
        j += 1.0;
    }
    let mut j = mode - 1.0;
    while j >= 0.0 {
        let (v, w) = term(j)?;
        total += v;
        weights += w;
        if w == 0.0 || w < 1e-17 * weights {
            break;
        }
        j -= 1.0;
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol * b.abs().max(1e-300), "{a} vs {b}");
    }

    /// Reference values: SciPy 1.18 `stats.nct`, `stats.ncx2`, `stats.ncf`.
    #[test]
    fn noncentral_t_matches_reference() {
        let cases = [
            (2.0, 10.0, 1.5, 0.659_154_072_442_190_9),
            (-1.0, 5.0, 0.5, 0.082_444_091_056_723_37),
            (3.5, 20.0, 4.0, 0.316_128_376_905_411_85),
            (0.5, 3.0, -1.0, 0.924_273_881_288_619_1),
            (12.0, 40.0, 10.0, 0.876_148_781_779_460_8),
            // Direct quadrature of ∫ Φ(ts − δ) f_S(s) ds (SciPy's nct differs by 2e-10 here).
            (1.96, 1e6, 2.8, 0.200_454_282_010_537_03),
        ];
        for (t, df, nc, cdf) in cases {
            let d = NoncentralT::new(df, nc).unwrap();
            close(d.cdf(t).unwrap(), cdf, 1e-9);
            close(d.sf(t).unwrap(), 1.0 - cdf, 1e-9);
        }
        // δ = 0 is the central t.
        let c = NoncentralT::new(7.0, 0.0).unwrap().cdf(1.3).unwrap();
        close(c, StudentsT::new(7.0).unwrap().cdf(1.3).unwrap(), 1e-15);
    }

    #[test]
    fn noncentral_chi_square_matches_reference() {
        let cases = [
            (5.0, 3.0, 2.0, 0.593_405_180_083_155_5, 0.406_594_819_916_844_4),
            (30.0, 10.0, 15.0, 0.735_400_871_492_951, 0.264_599_128_507_049_2),
            (1.0, 1.0, 0.5, 0.571_297_010_386_746, 0.428_702_989_613_254_04),
            (200.0, 4.0, 150.0, 0.962_519_766_789_031, 0.037_480_233_210_968_96),
        ];
        for (x, df, nc, cdf, sf) in cases {
            let d = NoncentralChiSquared::new(df, nc).unwrap();
            close(d.cdf(x).unwrap(), cdf, 1e-11);
            close(d.sf(x).unwrap(), sf, 1e-10);
        }
    }

    #[test]
    fn noncentral_f_matches_reference() {
        let cases = [
            (2.5, 3.0, 20.0, 5.0, 0.516_354_999_255_266_5, 0.483_645_000_744_733_66),
            (4.0, 2.0, 30.0, 12.0, 0.220_270_055_679_636_8, 0.779_729_944_320_363_4),
            (1.0, 5.0, 10.0, 1.0, 0.442_751_056_348_337_3, 0.557_248_943_651_662_9),
            (3.0, 3.0, 100.0, 40.0, 2.547_611_847_493_034_7e-4, 0.999_745_238_815_250_5),
        ];
        for (x, d1, d2, nc, cdf, sf) in cases {
            let d = NoncentralF::new(d1, d2, nc).unwrap();
            close(d.cdf(x).unwrap(), cdf, 1e-9);
            close(d.sf(x).unwrap(), sf, 1e-11);
        }
        assert!(NoncentralF::new(0.0, 1.0, 1.0).is_err());
        assert!(NoncentralChiSquared::new(1.0, -1.0).is_err());
    }
}
