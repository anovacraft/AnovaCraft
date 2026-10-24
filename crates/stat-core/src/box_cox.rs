//! Box–Cox power transformation (Box & Cox 1964, *JRSS B* 26:211; NIST/SEMATECH
//! e-Handbook §1.3.3.6 and §6.5.2).
//!
//! For positive data the transform is `y(λ) = (y^λ − 1)/λ`, with `log y` at
//! `λ = 0`. Under normality of `y(λ)` the profile log-likelihood (up to a
//! constant) is
//!
//! ```text
//! ℓ(λ) = −(n/2) · log σ̂²(λ) + (λ − 1) · Σ log yᵢ,
//! ```
//!
//! where `σ̂²(λ)` is the variance (divisor `n`) of the transformed values. The
//! estimate `λ̂` maximizes `ℓ`; an approximate `100(1 − α)%` confidence interval
//! is `{λ : ℓ(λ) ≥ ℓ(λ̂) − ½ χ²₁,₁₋α}`.

use crate::distributions::{ChiSquared, ContinuousDistribution};
use crate::{Result, StatError};

/// Search range for `λ̂` and the interval ends.
const LAMBDA_RANGE: (f64, f64) = (-5.0, 5.0);
/// Grid points for locating the maximum before refining it.
const GRID: usize = 200;

/// Result of [`box_cox_fit`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxCoxFit {
    /// Maximum-likelihood `λ̂` (within [−5, 5]).
    pub lambda: f64,
    /// `ℓ(λ̂)`.
    pub log_likelihood: f64,
    /// Confidence level of the interval, e.g. 0.95.
    pub confidence_level: f64,
    /// Interval for `λ`; an end is `None` if it lies beyond the search range.
    pub interval: (Option<f64>, Option<f64>),
}

impl BoxCoxFit {
    /// A commonly used "round" power inside the interval, preferring 1 (no
    /// transformation), then 0 (log), 0.5, −0.5, −1, 2, closest to `λ̂`.
    #[must_use]
    pub fn convenient_lambda(&self) -> Option<f64> {
        let lo = self.interval.0.unwrap_or(f64::NEG_INFINITY);
        let hi = self.interval.1.unwrap_or(f64::INFINITY);
        [1.0, 0.0, 0.5, -0.5, -1.0, 2.0]
            .into_iter()
            .filter(|l| (lo..=hi).contains(l))
            .min_by(|a, b| (a - self.lambda).abs().total_cmp(&(b - self.lambda).abs()))
    }
}

/// `(y^λ − 1)/λ`, or `log y` at `λ = 0` (computed as `expm1(λ log y)/λ`, which
/// is accurate near zero).
///
/// # Errors
/// [`StatError::InvalidArgument`] unless every value is positive and finite.
pub fn box_cox_transform(values: &[f64], lambda: f64) -> Result<Vec<f64>> {
    if values.iter().any(|v| !(v.is_finite() && *v > 0.0)) {
        return Err(invalid("the Box–Cox transformation needs positive values"));
    }
    Ok(values.iter().map(|&y| transform(y.ln(), lambda)).collect())
}

/// Inverse transform: `(λz + 1)^{1/λ}`, or `exp z` at `λ = 0`. Returns NaN
/// where `λz + 1 ≤ 0` (outside the transform's range).
#[must_use]
pub fn box_cox_inverse(z: f64, lambda: f64) -> f64 {
    if lambda == 0.0 {
        z.exp()
    } else {
        let base = lambda * z + 1.0;
        // ln_1p keeps full accuracy for λ near zero.
        if base > 0.0 { ((lambda * z).ln_1p() / lambda).exp() } else { f64::NAN }
    }
}

fn transform(log_y: f64, lambda: f64) -> f64 {
    if lambda == 0.0 { log_y } else { (lambda * log_y).exp_m1() / lambda }
}

/// Profile log-likelihood `ℓ(λ)`.
///
/// # Errors
/// [`StatError::InvalidArgument`] for fewer than two values or a value that is
/// not positive and finite.
pub fn box_cox_log_likelihood(values: &[f64], lambda: f64) -> Result<f64> {
    check_positive(values)?;
    let logs: Vec<f64> = values.iter().map(|y| y.ln()).collect();
    Ok(profile(&logs, lambda))
}

/// `ℓ(λ)`, evaluated relative to the geometric mean `ẏ = exp(ḡ)`, `ḡ = mean(log y)`.
///
/// With `vᵢ = (exp(λ(log yᵢ − ḡ)) − 1)/λ` (`log yᵢ − ḡ` at `λ = 0`), the
/// transformed values are `y(λ)ᵢ = ẏ^λ vᵢ + const`, so
/// `σ̂²(λ) = ẏ^{2λ} var(v)` and `ℓ(λ) = −(n/2) log var(v) − Σ log yᵢ` exactly.
/// Forming `y^λ − 1` directly would round to `−1` for large `y` and negative
/// `λ` (e.g. `y ≈ 10⁴`, `λ < −3`), leaving `σ̂²` as pure rounding noise.
fn profile(logs: &[f64], lambda: f64) -> f64 {
    let n = logs.len() as f64;
    let centre = logs.iter().sum::<f64>() / n;
    let v: Vec<f64> = logs.iter().map(|&l| transform(l - centre, lambda)).collect();
    let mean = v.iter().sum::<f64>() / n;
    let var = v.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / n;
    // An overflowed or vanishing variance has no likelihood; −∞ keeps such a
    // λ from being chosen (a NaN would rank by its sign bit, which differs
    // between platforms).
    if !(var.is_finite() && var > 0.0) {
        return f64::NEG_INFINITY;
    }
    -0.5 * n * var.ln() - n * centre
}

/// Maximum-likelihood `λ̂` with a `confidence_level` interval.
///
/// # Errors
/// [`StatError::InvalidArgument`] for fewer than two values, a value that is
/// not positive and finite, values that are all equal, or a confidence level
/// outside `(0, 1)`.
pub fn box_cox_fit(values: &[f64], confidence_level: f64) -> Result<BoxCoxFit> {
    check_positive(values)?;
    if !(confidence_level > 0.0 && confidence_level < 1.0) {
        return Err(invalid("the confidence level must be between 0 and 1"));
    }
    let logs: Vec<f64> = values.iter().map(|y| y.ln()).collect();
    if logs.iter().all(|&l| l == logs[0]) {
        return Err(invalid("the values are all equal, so no transformation can be estimated"));
    }
    let f = |l: f64| profile(&logs, l);
    let (a, b) = LAMBDA_RANGE;
    let step = (b - a) / GRID as f64;
    let best = (0..=GRID).map(|i| a + step * i as f64).max_by(|x, y| f(*x).total_cmp(&f(*y))).unwrap_or(1.0);
    // Golden-section refinement on the bracketing cells.
    let (mut lo, mut hi) = ((best - step).max(a), (best + step).min(b));
    let g = 0.5 * (5.0_f64.sqrt() - 1.0);
    for _ in 0..200 {
        if hi - lo < 1e-12 {
            break;
        }
        let (c, d) = (hi - g * (hi - lo), lo + g * (hi - lo));
        if f(c) > f(d) {
            hi = d;
        } else {
            lo = c;
        }
    }
    let lambda = 0.5 * (lo + hi);
    let log_likelihood = f(lambda);
    let cut = log_likelihood - 0.5 * ChiSquared::new(1.0)?.quantile(confidence_level)?;
    // ℓ is unimodal in practice: bisect for the crossing on each side.
    let crossing = |inside: f64, outside: f64| -> Option<f64> {
        if f(outside) >= cut {
            return None;
        }
        let (mut i, mut o) = (inside, outside);
        for _ in 0..200 {
            let m = 0.5 * (i + o);
            if f(m) >= cut {
                i = m;
            } else {
                o = m;
            }
        }
        Some(0.5 * (i + o))
    };
    let interval = (crossing(lambda, a), crossing(lambda, b));
    Ok(BoxCoxFit { lambda, log_likelihood, confidence_level, interval })
}

fn check_positive(values: &[f64]) -> Result<()> {
    if values.len() < 2 {
        return Err(invalid("at least two values are needed"));
    }
    if values.iter().any(|v| !(v.is_finite() && *v > 0.0)) {
        return Err(invalid("the Box–Cox transformation needs positive values"));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Positive, right-skewed test data; reference values from SciPy
    /// (`boxcox_normmax(method="mle")`, `boxcox_llf`, `boxcox(alpha=0.05)`).
    const SKEWED: [f64; 42] = [
        0.15, 0.09, 0.18, 0.10, 0.05, 0.12, 0.08, 0.05, 0.08, 0.10, 0.07, 0.02, 0.01, 0.10, 0.10, 0.10, 0.02, 0.10,
        0.01, 0.40, 0.10, 0.05, 0.03, 0.05, 0.15, 0.10, 0.15, 0.09, 0.08, 0.18, 0.10, 0.20, 0.11, 0.30, 0.02, 0.20,
        0.20, 0.30, 0.30, 0.40, 0.30, 0.05,
    ];

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol, "{a} vs {b}");
    }

    #[test]
    fn matches_scipy() {
        close(box_cox_log_likelihood(&SKEWED, 1.0).unwrap(), 97.103_089_924_536_1, 1e-9);
        let fit = box_cox_fit(&SKEWED, 0.95).unwrap();
        close(fit.lambda, 0.275_937_292_783_248_4, 1e-7);
        close(fit.log_likelihood, 106.519_231_464_937, 1e-9);
        close(fit.interval.0.unwrap(), -0.017_786_831_792_912_92, 1e-7);
        close(fit.interval.1.unwrap(), 0.586_381_245_442_393_8, 1e-7);
        // 0.5 is the closest convenient power in the interval (1 is excluded).
        assert_eq!(fit.convenient_lambda(), Some(0.5));
    }

    #[test]
    fn transform_inverse_and_limits() {
        let y = [0.5, 1.0, 2.0, 7.5];
        for lambda in [-1.0, 0.0, 1e-12, 0.5, 2.0] {
            let z = box_cox_transform(&y, lambda).unwrap();
            for (v, t) in y.iter().zip(&z) {
                close(box_cox_inverse(*t, lambda), *v, 1e-12);
            }
        }
        // λ → 0 is continuous with the log.
        let near = box_cox_transform(&[3.0], 1e-12).unwrap()[0];
        close(near, 3.0_f64.ln(), 1e-11);
        // λ = 1 is a shift: y − 1 (to within rounding: exp_m1 and ln differ
        // in the last bit between platform maths libraries, e.g. macOS gives
        // 2.0000000000000004).
        close(box_cox_transform(&[3.0], 1.0).unwrap()[0], 2.0, 1e-15);
        assert!(box_cox_inverse(-2.0, 1.0).is_nan());
    }

    #[test]
    fn log_normal_data_choose_log() {
        // exp of evenly spaced normal quantiles: the log transform makes them exactly normal-shaped.
        let z = crate::distributions::Normal::standard();
        let y: Vec<f64> = (1..=99).map(|i| (z.quantile(f64::from(i) / 100.0).unwrap() * 0.8 + 1.0).exp()).collect();
        let fit = box_cox_fit(&y, 0.95).unwrap();
        assert!(fit.lambda.abs() < 0.05, "{}", fit.lambda);
        assert_eq!(fit.convenient_lambda(), Some(0.0));
    }

    /// λ̂ does not depend on the units of the data. Reference: SciPy 1.18
    /// `boxcox_normmax(x, method="mle")` gives −0.104 354 8 for `BASE` times
    /// 1, 10³, 10⁴ and 10⁶ (agreeing to 1e-7).
    #[test]
    fn lambda_is_scale_invariant() {
        const BASE: [f64; 10] = [1.2, 1.5, 1.9, 2.4, 3.1, 3.3, 4.0, 5.2, 6.8, 9.5];
        let reference = box_cox_fit(&BASE, 0.95).unwrap();
        close(reference.lambda, -0.104_354_8, 1e-6);
        for scale in [1e3, 1e4, 1e6, 1e9] {
            let x: Vec<f64> = BASE.iter().map(|v| v * scale).collect();
            let fit = box_cox_fit(&x, 0.95).unwrap();
            close(fit.lambda, reference.lambda, 1e-6);
            close(fit.interval.0.unwrap(), reference.interval.0.unwrap(), 1e-6);
            close(fit.interval.1.unwrap(), reference.interval.1.unwrap(), 1e-6);
            // ℓ shifts by −n·log(scale) (the Jacobian), whatever λ is.
            let shift = -10.0 * scale.ln();
            for lambda in [-5.0, -1.0, 0.0, 2.0] {
                let l = box_cox_log_likelihood(&x, lambda).unwrap();
                close(l, box_cox_log_likelihood(&BASE, lambda).unwrap() + shift, 1e-8 * (1.0 + l.abs()));
            }
        }
    }

    #[test]
    fn rejects_bad_input() {
        assert!(box_cox_fit(&[1.0], 0.95).is_err());
        assert!(box_cox_fit(&[1.0, -2.0], 0.95).is_err());
        assert!(box_cox_fit(&[1.0, 0.0], 0.95).is_err());
        assert!(box_cox_fit(&[2.0, 2.0, 2.0], 0.95).is_err());
        assert!(box_cox_fit(&SKEWED, 1.0).is_err());
    }
}
