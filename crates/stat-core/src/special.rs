//! Special functions needed by the probability distributions.
//!
//! * [`ln_gamma`] — Lanczos approximation (Lanczos 1964, SIAM J. Numer. Anal. B 1)
//!   with the widely published `g = 7`, nine-term coefficient set; relative error
//!   about 1e-15 for `x > 0`. The reflection formula covers `x < 0.5`.
//! * [`ln_beta`] — `ln Γ(a) + ln Γ(b) − ln Γ(a + b)`.
//! * [`stirling_error`], [`saddle_deviance`] — the pieces of Loader's (2000)
//!   saddle-point form of `ln Γ`, "Fast and accurate computation of binomial
//!   probabilities". Writing `ln Γ(x + 1) = (x + ½) ln x − x + ½ ln 2π + δ(x)`
//!   keeps the large, cancelling terms of order `x ln x` out of densities and
//!   of the incomplete gamma and beta prefactors, which the incomplete
//!   functions, [`gamma_prefactor`], [`beta_prefactor`] and
//!   [`ln_gamma_difference`] use for parameters of 10 or more. Plain `ln Γ`
//!   differences lose about `x · 10⁻¹⁶` absolutely, so relative accuracy fell
//!   to 1e-10 at `x ≈ 10⁶` and 1e-6 at `x ≈ 10¹⁰`.
//! * [`regularized_incomplete_beta`] — `I_x(a, b)` evaluated with the continued
//!   fraction of DLMF §8.17(v), eq. 8.17.22, using the modified Lentz algorithm
//!   (Thompson & Barnett 1986, J. Comput. Phys. 64), and the symmetry
//!   `I_x(a, b) = 1 − I_{1−x}(b, a)` to stay in the rapidly converging region
//!   `x < (a + 1)/(a + b + 2)`.
//! * [`regularized_gamma_p`] / [`regularized_gamma_q`] — the regularized incomplete
//!   gamma functions, by the power series of DLMF 8.11.4 for `x < a + 1` and the
//!   continued fraction of DLMF 8.9.2 (modified Lentz) otherwise, so that the
//!   smaller of `P` and `Q` is always computed directly, without cancellation.
//! * [`erfc`] — the complementary error function, `erfc(x) = Q(1/2, x²)` for
//!   `x ≥ 0` (DLMF 7.11.2).
//! * [`digamma`] / [`trigamma`] — `ψ(x) = d ln Γ(x)/dx` and `ψ′(x)`, by the
//!   recurrences `ψ(x) = ψ(x + 1) − 1/x`, `ψ′(x) = ψ′(x + 1) + 1/x²` up to
//!   `x ≥ 10` (20 for `ψ′`) and then the asymptotic series of DLMF 5.15.8 and 5.11.2.

use crate::error::{Result, StatError, invalid};

const LANCZOS_G: f64 = 7.0;
const LANCZOS_COEFFS: [f64; 9] = [
    0.999_999_999_999_809_9,
    676.520_368_121_885_1,
    -1_259.139_216_722_402_8,
    771.323_428_777_653_1,
    -176.615_029_162_140_6,
    12.507_343_278_686_905,
    -0.138_571_095_265_720_12,
    9.984_369_578_019_572e-6,
    1.505_632_735_149_311_6e-7,
];

/// Natural logarithm of `|Γ(x)|`.
///
/// Returns `+∞` at the poles `x = 0, −1, −2, …` and NaN for NaN input.
#[must_use]
pub fn ln_gamma(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x <= 0.0 && x == x.floor() {
        return f64::INFINITY;
    }
    if x < 0.5 {
        // Reflection: Γ(x) Γ(1 − x) = π / sin(πx).
        let s = (std::f64::consts::PI * x).sin().abs();
        return std::f64::consts::PI.ln() - s.ln() - ln_gamma(1.0 - x);
    }
    let z = x - 1.0;
    let mut acc = LANCZOS_COEFFS[0];
    for (i, &c) in LANCZOS_COEFFS.iter().enumerate().skip(1) {
        acc += c / (z + i as f64);
    }
    let t = z + LANCZOS_G + 0.5;
    0.5 * (2.0 * std::f64::consts::PI).ln() + (z + 0.5) * t.ln() - t + acc.ln()
}

/// `ln B(a, b) = ln Γ(a) + ln Γ(b) − ln Γ(a + b)` for `a, b > 0`.
#[must_use]
pub fn ln_beta(a: f64, b: f64) -> f64 {
    ln_gamma(a) + ln_gamma(b) - ln_gamma(a + b)
}

/// Remainder of Stirling's approximation,
/// `δ(x) = ln Γ(x + 1) − [(x + ½) ln x − x + ½ ln 2π]`, for `x > 0`.
///
/// For `x ≥ 10` it is the asymptotic series `Σ B₂ₖ / (2k(2k − 1) x^{2k−1})`
/// (DLMF 5.11.1) through `k = 8`, whose truncation error there is below
/// 2e-18; below 10 the recurrence `δ(x) = δ(x + 1) + (x + ½) ln(1 + 1/x) − 1`
/// carries it down (absolute error about 1e-16 per step). `δ(x) ≈ 1/(12x)` is small and smooth, so differences of
/// `δ` lose nothing, unlike differences of `ln Γ` itself.
///
/// Returns NaN unless `x > 0`.
#[must_use]
pub fn stirling_error(x: f64) -> f64 {
    if x.is_nan() || x <= 0.0 {
        return f64::NAN;
    }
    if x < 10.0 {
        // δ(x) = δ(x + 1) + (x + ½) ln(1 + 1/x) − 1, from Γ(x + 2) = (x + 1) Γ(x + 1).
        return stirling_error(x + 1.0) + (x + 0.5) * x.recip().ln_1p() - 1.0;
    }
    if x.is_infinite() {
        return 0.0;
    }
    // B₂ₖ / (2k(2k − 1)) for k = 1..8.
    const C: [f64; 8] = [
        1.0 / 12.0,
        -1.0 / 360.0,
        1.0 / 1260.0,
        -1.0 / 1680.0,
        1.0 / 1188.0,
        -691.0 / 360_360.0,
        1.0 / 156.0,
        -3617.0 / 122_400.0,
    ];
    let w = 1.0 / (x * x);
    C.iter().rev().fold(0.0, |acc, &c| acc * w + c) / x
}

/// Deviance term `D(x; m) = x ln(x/m) + m − x ≥ 0` of a Poisson count `x`
/// against mean `m` (Loader 2000, `bd0`), for `x ≥ 0`, `m > 0`.
///
/// When `x` and `m` are close, the direct form cancels to nothing; there the
/// series `D = (x − m) v + 2x Σ_{j≥1} v^{2j+1}/(2j + 1)`, `v = (x − m)/(x + m)`,
/// is summed instead, which is exact up to rounding for `|x − m| < 0.1 (x + m)`.
#[must_use]
pub fn saddle_deviance(x: f64, m: f64) -> f64 {
    if x == 0.0 {
        return m;
    }
    let d = x - m;
    if d.abs() < 0.1 * (x + m) {
        let v = d / (x + m);
        let v2 = v * v;
        let mut s = d * v;
        let mut ej = 2.0 * x * v;
        for j in 1..1000 {
            ej *= v2;
            let next = s + ej / f64::from(2 * j + 1);
            if next == s {
                break;
            }
            s = next;
        }
        s
    } else {
        x * (x / m).ln() + m - x
    }
}

/// `ln Γ(x + d) − ln Γ(x)` for `x > 0`, `x + d > 0`, without the cancellation
/// of two large `ln Γ` values.
///
/// With both arguments at least 10 it uses `ln Γ(z) = (z − ½) ln z − z +
/// ½ ln 2π + δ(z)` (Stirling's form with [`stirling_error`]), so the difference is
/// `(x + d − ½) ln1p(d/x) + d (ln x − 1) + δ(x + d) − δ(x)`, which keeps
/// full relative accuracy for `|d| ≪ x` (the case of `ln Γ((ν + 1)/2) −
/// ln Γ(ν/2)` in Student's t density at large `ν`). Otherwise the two
/// [`ln_gamma`] values are subtracted directly.
#[must_use]
pub fn ln_gamma_difference(x: f64, d: f64) -> f64 {
    let y = x + d;
    if x < 10.0 || y < 10.0 {
        return ln_gamma(y) - ln_gamma(x);
    }
    // ln Γ(z) = ln Γ(z + 1) − ln z = (z − ½) ln z − z + ½ ln 2π + δ(z).
    (y - 0.5) * (d / x).ln_1p() + d * x.ln() - d + stirling_error(y) - stirling_error(x)
}

/// Poisson probability `mᵏ e⁻ᵐ / Γ(k + 1)` for real `k ≥ 0` and `m > 0`.
///
/// For `k ≥ 10` it is evaluated as `exp(−δ(k) − D(k; m)) / √(2πk)` with
/// [`stirling_error`] and [`saddle_deviance`] (Loader 2000), so no term of
/// size `k ln k` appears; smaller `k` uses `exp(k ln m − m − ln Γ(k + 1))`.
#[must_use]
pub fn poisson_probability(k: f64, m: f64) -> f64 {
    if k == 0.0 {
        (-m).exp()
    } else if k < 10.0 {
        (k * m.ln() - m - ln_gamma(k + 1.0)).exp()
    } else {
        (-stirling_error(k) - saddle_deviance(k, m)).exp() / (2.0 * std::f64::consts::PI * k).sqrt()
    }
}

/// `xᵃ e⁻ˣ / Γ(a)`, the factor in front of the incomplete gamma series and
/// continued fraction, for `a > 0`, `x > 0`: `a` times
/// [`poisson_probability`]`(a, x)`.
#[must_use]
pub fn gamma_prefactor(a: f64, x: f64) -> f64 {
    if a < 10.0 { (a * x.ln() - x - ln_gamma(a)).exp() } else { a * poisson_probability(a, x) }
}

/// `xᵃ yᵇ / B(a, b)` with `y = 1 − x`, the factor in front of the incomplete
/// beta continued fraction, for `a, b > 0` and `0 < x < 1`.
///
/// When `a` or `b` is at least 10 it is evaluated in Loader's (2000)
/// saddle-point form, with `n = a + b`:
/// `√(ab / (2πn)) exp(δ(n) − δ(a) − δ(b) − D(a; nx) − D(b; ny))`, using
/// [`stirling_error`] and [`saddle_deviance`]. `nx` and `ny` are both formed
/// from whichever of `x` and `y` is smaller (`nx = n − ny`), so a value of
/// `x` that rounds near 1 does not cost accuracy. Smaller parameters use
/// `exp(a ln x + b ln y − ln B(a, b))`.
#[must_use]
pub fn beta_prefactor(a: f64, b: f64, x: f64, y: f64) -> f64 {
    if a.max(b) < 10.0 {
        return (a * x.ln() + b * y.ln() - ln_beta(a, b)).exp();
    }
    let n = a + b;
    let (nx, ny) = if x <= y {
        let nx = n * x;
        (nx, n - nx)
    } else {
        let ny = n * y;
        (n - ny, ny)
    };
    let log =
        stirling_error(n) - stirling_error(a) - stirling_error(b) - saddle_deviance(a, nx) - saddle_deviance(b, ny);
    (a * b / (2.0 * std::f64::consts::PI * n)).sqrt() * log.exp()
}

/// Regularized incomplete beta function
/// `I_x(a, b) = (1 / B(a, b)) ∫₀ˣ t^{a−1} (1 − t)^{b−1} dt`.
///
/// # Errors
/// * [`StatError::InvalidArgument`] unless `a > 0`, `b > 0` and `0 ≤ x ≤ 1`.
/// * [`StatError::ConvergenceFailure`] if the continued fraction does not converge
///   (not observed for finite parameters; guards against pathological input).
pub fn regularized_incomplete_beta(a: f64, b: f64, x: f64) -> Result<f64> {
    regularized_incomplete_beta_pair(a, b, x, 1.0 - x)
}

/// [`regularized_incomplete_beta`] with `y = 1 − x` supplied by the caller.
///
/// Distribution functions can often form `1 − x` without cancellation
/// (e.g. `t²/(ν + t²)` alongside `ν/(ν + t²)`); passing both preserves accuracy in
/// the tails.
///
/// # Errors
/// As for [`regularized_incomplete_beta`]; additionally `x + y` must equal 1 to
/// within a few ulps.
pub fn regularized_incomplete_beta_pair(a: f64, b: f64, x: f64, y: f64) -> Result<f64> {
    if !(a > 0.0 && b > 0.0 && a.is_finite() && b.is_finite()) {
        return Err(invalid(format!("incomplete beta requires a, b > 0 (got a = {a}, b = {b})")));
    }
    if !((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y)) || (x + y - 1.0).abs() > 1e-12 {
        return Err(invalid(format!("incomplete beta requires 0 <= x = 1 - y <= 1 (got x = {x}, y = {y})")));
    }
    if x == 0.0 {
        return Ok(0.0);
    }
    if y == 0.0 {
        return Ok(1.0);
    }
    // Choose the smaller tail by testing the smaller of x and y: at huge `a`
    // x rounds to 1 while y still carries the information, and
    // `x < (a + 1)/(a + b + 2)` is the same condition as `y > (b + 1)/(a + b + 2)`.
    let lower = if y < x { y > (b + 1.0) / (a + b + 2.0) } else { x < (a + 1.0) / (a + b + 2.0) };
    if lower { lower_tail(a, b, x, y) } else { Ok(1.0 - lower_tail(b, a, y, x)?) }
}

/// `I_x(a, b)` for `x` below the mean region, where it is the smaller tail.
///
/// The continued fraction loses about `a · 10⁻¹⁶` relatively when `x` lies
/// within `O(b/a)` of 1, because its leading terms `1 − c x` cancel; there
/// (`a ≥ 1000`, `b ≤ 10`, `y < 0.1`, as for Student's t with large `ν`) the
/// asymptotic expansion [`beta_large_a_small_b`] is used instead.
fn lower_tail(a: f64, b: f64, x: f64, y: f64) -> Result<f64> {
    if a >= 1000.0 && b <= 10.0 && y < 0.1 {
        beta_large_a_small_b(a, b, x, y)
    } else {
        Ok(front_factor(a, b, x, y) * continued_fraction(a, b, x)? / a)
    }
}

/// `I_x(a, b)` for large `a` and small `b` by the asymptotic expansion of
/// DiDonato & Morris (1992), "Algorithm 708: significant digit computation of
/// the incomplete beta function ratios", *ACM TOMS* 18(3), §9 (eqs. 9.1–9.6;
/// their BGRAT).
///
/// With `T = a + (b − 1)/2` and `u = −T ln x`,
/// `I_x(a, b) = [Γ(a + b) / (Γ(a) Tᵇ)] Σₙ pₙ Jₙ`, where `J₀ = Γ(b)
/// Q(b, u) / (uᵇe⁻ᵘ)` (times the prefactor `uᵇe⁻ᵘ/Γ(b)`), the `pₙ` follow
/// the recursion of eq. 9.4 and the `Jₙ` that of eq. 9.6, and the terms fall
/// like `(ln x / 2)²ⁿ / T²ⁿ`. Only `y = 1 − x` and `ln x = ln1p(−y)` enter,
/// so `x` near 1 costs no accuracy.
fn beta_large_a_small_b(a: f64, b: f64, x: f64, y: f64) -> Result<f64> {
    const TERMS: usize = 30;
    let bm1 = b - 1.0;
    let t = a + 0.5 * bm1;
    let lx = if y < 0.35 { (-y).ln_1p() } else { x.ln() };
    let u = -t * lx;
    let h = gamma_prefactor(b, u);
    if h == 0.0 {
        return Ok(0.0);
    }
    let prefix = h * (ln_gamma_difference(a, b) - b * t.ln()).exp();
    // (2m + 1)! for m = 0, 1, …
    let mut odd_factorial = [1.0_f64; TERMS + 1];
    for m in 1..=TERMS {
        odd_factorial[m] = odd_factorial[m - 1] * (2 * m) as f64 * (2 * m + 1) as f64;
    }
    let mut p = [0.0_f64; TERMS];
    p[0] = 1.0;
    let mut j = regularized_gamma_q(b, u)? / h;
    let mut sum = prefix * j;
    let lx2 = (0.5 * lx).powi(2);
    let mut lxp = 1.0;
    let t4 = 4.0 * t * t;
    let mut b2n = b;
    for n in 1..TERMS {
        let nf = n as f64;
        let mut pn = 0.0;
        for m in 1..n {
            pn += (m as f64 * b - nf) * p[n - m] / odd_factorial[m];
        }
        p[n] = pn / nf + bm1 / odd_factorial[n];
        j = (b2n * (b2n + 1.0) * j + (u + b2n + 1.0) * lxp) / t4;
        lxp *= lx2;
        b2n += 2.0;
        let r = prefix * p[n] * j;
        sum += r;
        if r.abs() <= f64::EPSILON * sum.abs() {
            return Ok(sum);
        }
    }
    Err(StatError::ConvergenceFailure { routine: "incomplete beta large-a expansion", iterations: TERMS })
}

/// `xᵃ yᵇ / B(a, b)`.
fn front_factor(a: f64, b: f64, x: f64, y: f64) -> f64 {
    beta_prefactor(a, b, x, y)
}

/// Continued fraction for `I_x(a, b)` (DLMF 8.17.22), modified Lentz evaluation.
fn continued_fraction(a: f64, b: f64, x: f64) -> Result<f64> {
    const TINY: f64 = 1e-300;
    // The number of terms grows like √(a + b) near the mean.
    let max_iter = 20_000 + extra_iterations(a + b);
    let eps = f64::EPSILON;

    let qab = a + b;
    let qap = a + 1.0;
    let qam = a - 1.0;
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < TINY {
        d = TINY;
    }
    d = 1.0 / d;
    let mut h = d;
    for m in 1..=max_iter {
        let m = m as f64;
        let m2 = 2.0 * m;
        // Even step: d_{2m} = m (b − m) x / ((a + 2m − 1)(a + 2m)).
        let aa = m * (b - m) * x / ((qam + m2) * (a + m2));
        d = 1.0 + aa * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1.0 + aa / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        h *= d * c;
        // Odd step: d_{2m+1} = −(a + m)(a + b + m) x / ((a + 2m)(a + 2m + 1)).
        let aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2));
        d = 1.0 + aa * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1.0 + aa / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        let delta = d * c;
        h *= delta;
        if (delta - 1.0).abs() <= eps {
            return Ok(h);
        }
    }
    Err(StatError::ConvergenceFailure { routine: "incomplete beta continued fraction", iterations: max_iter })
}

/// Extra iterations for parameters as large as `size`: `20 √size`, at most
/// two million (enough for parameters up to 10¹⁰).
fn extra_iterations(size: f64) -> usize {
    (20.0 * size.sqrt()).min(2e6) as usize
}

/// Regularized lower incomplete gamma function `P(a, x) = γ(a, x) / Γ(a)`.
///
/// # Errors
/// * [`StatError::InvalidArgument`] unless `a > 0` and `x ≥ 0` (both finite).
/// * [`StatError::ConvergenceFailure`] if the series or continued fraction fails
///   to converge (not observed for finite arguments).
pub fn regularized_gamma_p(a: f64, x: f64) -> Result<f64> {
    check_gamma_args(a, x)?;
    if x == 0.0 {
        return Ok(0.0);
    }
    if x < a + 1.0 { gamma_series(a, x) } else { Ok(1.0 - gamma_continued_fraction(a, x)?) }
}

/// Regularized upper incomplete gamma function `Q(a, x) = Γ(a, x) / Γ(a) = 1 − P(a, x)`.
///
/// # Errors
/// As for [`regularized_gamma_p`].
pub fn regularized_gamma_q(a: f64, x: f64) -> Result<f64> {
    check_gamma_args(a, x)?;
    if x == 0.0 {
        return Ok(1.0);
    }
    if x < a + 1.0 { Ok(1.0 - gamma_series(a, x)?) } else { gamma_continued_fraction(a, x) }
}

fn check_gamma_args(a: f64, x: f64) -> Result<()> {
    if a > 0.0 && a.is_finite() && x >= 0.0 && x.is_finite() {
        Ok(())
    } else {
        Err(invalid(format!("incomplete gamma requires a > 0 and x >= 0 (got a = {a}, x = {x})")))
    }
}

/// `P(a, x)` by the series `xᵃe⁻ˣ/Γ(a+1) · Σₖ xᵏ / ((a+1)(a+2)…(a+k))` (DLMF 8.11.4).
fn gamma_series(a: f64, x: f64) -> Result<f64> {
    // The number of terms grows like √a when x is near a.
    let max_iter = 10_000 + extra_iterations(a);
    let mut term = 1.0 / a;
    let mut sum = term;
    // Kahan compensation: the terms are all positive and can number many
    // thousands.
    let mut carry = 0.0;
    let mut denom = a;
    for _ in 0..max_iter {
        denom += 1.0;
        term *= x / denom;
        let y = term - carry;
        let next = sum + y;
        carry = (next - sum) - y;
        sum = next;
        // The remaining terms fall at least geometrically with ratio
        // r = x/(denom + 1), so their total is below term · r/(1 − r); near
        // x ≈ a that ratio is close to 1 and the tail is far larger than the
        // last term.
        let ratio = x / (denom + 1.0);
        if term * ratio < sum * f64::EPSILON * (1.0 - ratio) {
            return Ok(sum * gamma_prefactor(a, x));
        }
    }
    Err(StatError::ConvergenceFailure { routine: "incomplete gamma series", iterations: max_iter })
}

/// `Q(a, x)` by the continued fraction of DLMF 8.9.2, modified Lentz evaluation.
fn gamma_continued_fraction(a: f64, x: f64) -> Result<f64> {
    const TINY: f64 = 1e-300;
    // The number of terms grows like √a when x is near a.
    let max_iter = 10_000 + extra_iterations(a);
    let mut b = x + 1.0 - a;
    let mut c = 1.0 / TINY;
    let mut d = 1.0 / b;
    let mut h = d;
    for i in 1..=max_iter {
        let i = i as f64;
        let an = -i * (i - a);
        b += 2.0;
        d = an * d + b;
        if d.abs() < TINY {
            d = TINY;
        }
        c = b + an / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        let delta = d * c;
        h *= delta;
        if (delta - 1.0).abs() <= f64::EPSILON {
            return Ok(h * gamma_prefactor(a, x));
        }
    }
    Err(StatError::ConvergenceFailure { routine: "incomplete gamma continued fraction", iterations: max_iter })
}

/// Complementary error function `erfc(x) = (2/√π) ∫ₓ^∞ e^{−t²} dt`.
///
/// Evaluated as `Q(1/2, x²)` for `x ≥ 0` and `2 − erfc(−x)` for `x < 0`, so the
/// upper tail keeps full relative precision. Returns NaN for NaN input.
#[must_use]
pub fn erfc(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    // erfc(40) ≈ 1e-697 underflows to 0, and beyond ~1.3e154 `x²` itself
    // would overflow, so the limits are returned directly.
    if x.abs() > 40.0 {
        return if x > 0.0 { 0.0 } else { 2.0 };
    }
    // For finite x the arguments are always valid, so the computation cannot fail.
    let upper = regularized_gamma_q(0.5, x * x).unwrap_or(f64::NAN);
    if x >= 0.0 { upper } else { 2.0 - upper }
}

/// Digamma function `ψ(x) = Γ′(x)/Γ(x)` for `x > 0` (NaN otherwise).
///
/// The recurrence `ψ(x) = ψ(x + 1) − 1/x` moves the argument to `x ≥ 10`,
/// where the asymptotic expansion
/// `ψ(x) ~ ln x − 1/(2x) − Σ B₂ₖ/(2k x²ᵏ)` (DLMF 5.11.2) is accurate to
/// double precision with six terms.
#[must_use]
pub fn digamma(x: f64) -> f64 {
    if x.is_nan() || x <= 0.0 || x.is_infinite() {
        return if x == f64::INFINITY { f64::INFINITY } else { f64::NAN };
    }
    let (mut x, mut acc) = (x, 0.0);
    while x < 10.0 {
        acc -= 1.0 / x;
        x += 1.0;
    }
    let r = 1.0 / (x * x);
    let series = r
        * (1.0 / 12.0
            - r * (1.0 / 120.0 - r * (1.0 / 252.0 - r * (1.0 / 240.0 - r * (1.0 / 132.0 - r * 691.0 / 32760.0)))));
    acc + x.ln() - 0.5 / x - series
}

/// Trigamma function `ψ′(x)` for `x > 0` (NaN otherwise).
///
/// The recurrence `ψ′(x) = ψ′(x + 1) + 1/x²` moves the argument to `x ≥ 20`,
/// where `ψ′(x) ~ 1/x + 1/(2x²) + Σ B₂ₖ/x²ᵏ⁺¹` (DLMF 5.15.8).
#[must_use]
pub fn trigamma(x: f64) -> f64 {
    if x.is_nan() || x <= 0.0 || x.is_infinite() {
        return if x == f64::INFINITY { 0.0 } else { f64::NAN };
    }
    let (mut x, mut acc) = (x, 0.0);
    while x < 20.0 {
        acc += 1.0 / (x * x);
        x += 1.0;
    }
    let r = 1.0 / (x * x);
    let series = r * (1.0 / 6.0 - r * (1.0 / 30.0 - r * (1.0 / 42.0 - r * (1.0 / 30.0 - r * 5.0 / 66.0))));
    acc + 1.0 / x + 0.5 * r + series / x
}

/// Lambert W function, principal branch `W₀(x)`: the solution `w ≥ −1` of
/// `w·eʷ = x` for `x ≥ −1/e` (NaN below −1/e and for NaN).
///
/// For `x > 0` it is [`wright_omega`]`(ln x)`; for `−1/e ≤ x ≤ 0` Halley's
/// iteration on `w·eʷ − x` (Corless, Gonnet, Hare, Jeffrey & Knuth, "On the
/// Lambert W function", *Adv. Comput. Math.* 5, 1996, eq. 5.9) starts from the
/// branch-point series `−1 + p − p²/3 + 11p³/72`, `p = √(2(e·x + 1))`
/// (their eq. 4.22). Accurate to a few ulps.
#[must_use]
pub fn lambert_w(x: f64) -> f64 {
    const BRANCH: f64 = -1.0 / std::f64::consts::E;
    if x.is_nan() || x < BRANCH - 1e-17 {
        return f64::NAN;
    }
    if x == 0.0 {
        return 0.0;
    }
    if x > 0.0 {
        return wright_omega(x.ln());
    }
    let p = (2.0 * (std::f64::consts::E * x + 1.0)).max(0.0).sqrt();
    if p == 0.0 {
        return -1.0;
    }
    let mut w = if x < -0.25 { -1.0 + p - p * p / 3.0 + 11.0 / 72.0 * p * p * p } else { x * (1.0 - x) };
    for _ in 0..50 {
        let e = w.exp();
        let f = w * e - x;
        let denom = e * (w + 1.0) - (w + 2.0) * f / (2.0 * w + 2.0);
        if denom == 0.0 || !denom.is_finite() {
            break;
        }
        let next = (w - f / denom).max(-1.0);
        let done = (next - w).abs() <= 4.0 * f64::EPSILON * next.abs().max(1e-300);
        w = next;
        if done {
            break;
        }
    }
    w
}

/// Wright omega function `ω(z) = W₀(eᶻ)`: the solution `w > 0` of
/// `w + ln w = z` for real `z`.
///
/// Evaluated without forming `eᶻ`, so it holds for any finite `z`
/// (`ω(z) ≈ eᶻ` far below zero, `≈ z − ln z` far above). Starting values
/// follow Lawrence, Corless & Jeffrey, "Algorithm 917: Complex Double-Precision
/// Evaluation of the Wright ω Function", *ACM TOMS* 38(3), 2012; the root is
/// refined by Halley's iteration on `w + ln w − z`. Used for the integrated
/// Michaelis–Menten progress curve, `P(t) = S₀ − Km·ω(ln(S₀/Km) + (S₀ − Vmax·t)/Km)`.
/// NaN for NaN; `+∞` for `+∞`; 0 for `−∞`.
#[must_use]
pub fn wright_omega(z: f64) -> f64 {
    if z.is_nan() {
        return f64::NAN;
    }
    if z == f64::INFINITY {
        return f64::INFINITY;
    }
    if z == f64::NEG_INFINITY {
        return 0.0;
    }
    if z < -36.0 {
        // ω = eᶻ(1 − eᶻ + …): the correction is below one ulp.
        return z.exp();
    }
    let mut w = if z > 2.0 {
        let l = z.ln();
        z - l + l / z
    } else if z < -1.0 {
        let e = z.exp();
        e * (1.0 - e)
    } else {
        // Series about z = 1 (ω(1) = 1): ω ≈ 1 + (z − 1)/2 + (z − 1)²/16.
        let d = z - 1.0;
        (1.0 + d / 2.0 + d * d / 16.0).max(0.05)
    };
    for _ in 0..50 {
        // f(w) = w + ln w − z, f′ = 1 + 1/w, f″ = −1/w².
        let f = w + w.ln() - z;
        let d1 = 1.0 + 1.0 / w;
        let d2 = -1.0 / (w * w);
        let step = f / (d1 - f * d2 / (2.0 * d1));
        let mut next = w - step;
        if next <= 0.0 {
            next = w / 4.0;
        }
        let done = (next - w).abs() <= 4.0 * f64::EPSILON * next;
        w = next;
        if done {
            break;
        }
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digamma_and_trigamma_reference_values() {
        // ψ(1) = −γ, ψ(½) = −γ − 2 ln 2, ψ′(1) = π²/6, ψ′(½) = π²/2 (DLMF 5.4.12–13, 5.15.2–3).
        let euler = 0.577_215_664_901_532_9;
        let pi2 = std::f64::consts::PI * std::f64::consts::PI;
        assert!((digamma(1.0) + euler).abs() < 1e-14);
        assert!((digamma(0.5) + euler + 2.0 * std::f64::consts::LN_2).abs() < 1e-14);
        assert!(rel(trigamma(1.0), pi2 / 6.0) < 1e-14);
        assert!(rel(trigamma(0.5), pi2 / 2.0) < 1e-14);
        // Recurrences away from the reduction threshold, and the derivative of ln Γ.
        for &x in &[0.013, 0.7, 3.3, 9.99, 10.0, 47.5, 1e4] {
            assert!((digamma(x + 1.0) - digamma(x) - 1.0 / x).abs() < 1e-12 * (1.0 + 1.0 / x), "x = {x}");
            assert!(rel(trigamma(x) - trigamma(x + 1.0), 1.0 / (x * x)) < 1e-10, "x = {x}");
            let h = 1e-5 * x.max(1.0);
            let numeric = (ln_gamma(x + h) - ln_gamma(x - h.min(x / 2.0))) / (h + h.min(x / 2.0));
            assert!((digamma(x) - numeric).abs() < 1e-5 * (1.0 + digamma(x).abs()), "x = {x}");
        }
        assert!(digamma(0.0).is_nan() && trigamma(-1.0).is_nan());
    }

    fn rel(a: f64, b: f64) -> f64 {
        ((a - b) / b).abs()
    }

    #[test]
    fn saddle_point_pieces_match_mpmath() {
        // Reference values from mpmath at 30 digits: δ(x) = lnΓ(x+1) − (x+½)ln x + x − ½ln 2π,
        // D(x; m) = x ln(x/m) + m − x, and lnΓ(x+d) − lnΓ(x).
        for (x, want) in [
            (0.5, 0.153_426_409_720_027_36),
            (3.0, 0.027_677_925_684_998_34),
            (9.99, 0.008_338_896_797_734_33),
            (10.0, 0.008_330_563_433_362_87),
            (25.0, 0.003_333_155_636_728_093),
            (1e3, 8.333_333_055_555_635e-5),
            (1e8, 8.333_333_333_333_106e-10),
        ] {
            assert!(rel(stirling_error(x), want) < 1e-13, "δ({x}) = {}", stirling_error(x));
        }
        for (x, m, want) in [
            (1e9, 1e9 + 3e4, 0.449_991_000_202_495_14),
            (1e6 + 0.5, 1e6, 1.249_999_791_666_719e-7),
            (50.0, 10.0, 40.471_895_621_705_016),
        ] {
            assert!(rel(saddle_deviance(x, m), want) < 1e-12, "D({x}; {m}) = {}", saddle_deviance(x, m));
        }
        assert_eq!(saddle_deviance(0.0, 2.5), 2.5);
        for (x, d, want) in [
            (5e9, 0.5, 11.166_351_874_665_256),
            (1e6, 2.5, 34.538_778_269_909_436),
            (12.0, 0.5, 1.232_039_666_062_559_9),
            (3.0, 0.5, 0.507_826_421_787_128_9),
        ] {
            assert!(rel(ln_gamma_difference(x, d), want) < 1e-14, "lnΓ({x}+{d}) − lnΓ({x})");
        }
        // The naive difference of two lnΓ values near 1.1e11 keeps only about 5 digits.
        assert!(rel(ln_gamma(5e9 + 0.5) - ln_gamma(5e9), 11.166_351_874_665_256) > 1e-9);
    }

    #[test]
    fn ln_gamma_matches_factorials_and_half_integers() {
        let mut fact = 1.0_f64;
        for n in 1..=25 {
            // Γ(n) = (n − 1)!
            assert!(rel(ln_gamma(f64::from(n)).exp(), fact) < 1e-13, "n = {n}");
            fact *= f64::from(n);
        }
        // Γ(1/2) = √π
        assert!(rel(ln_gamma(0.5), 0.5 * std::f64::consts::PI.ln()) < 1e-14);
        // Γ(−1/2) = −2√π  ⇒  ln|Γ| = ln(2√π)
        assert!(rel(ln_gamma(-0.5), (2.0 * std::f64::consts::PI.sqrt()).ln()) < 1e-14);
        assert_eq!(ln_gamma(0.0), f64::INFINITY);
        assert_eq!(ln_gamma(-3.0), f64::INFINITY);
        assert!(ln_gamma(f64::NAN).is_nan());
    }

    #[test]
    fn incomplete_beta_closed_forms() {
        // I_x(1, 1) = x; I_x(a, 1) = xᵃ; I_x(1, b) = 1 − (1 − x)ᵇ.
        for &x in &[0.01, 0.2, 0.5, 0.77, 0.999] {
            assert!(rel(regularized_incomplete_beta(1.0, 1.0, x).unwrap(), x) < 1e-14);
            assert!(rel(regularized_incomplete_beta(3.5, 1.0, x).unwrap(), x.powf(3.5)) < 1e-13);
            let expected = 1.0 - (1.0 - x).powf(2.5);
            assert!(rel(regularized_incomplete_beta(1.0, 2.5, x).unwrap(), expected) < 1e-13);
        }
        // I_x(1/2, 1/2) = (2/π) asin(√x).
        let x: f64 = 0.3;
        let expected = 2.0 / std::f64::consts::PI * x.sqrt().asin();
        assert!(rel(regularized_incomplete_beta(0.5, 0.5, x).unwrap(), expected) < 1e-14);
    }

    #[test]
    fn incomplete_beta_symmetry_and_bounds() {
        let (a, b, x) = (12.3, 4.7, 0.62);
        let lhs = regularized_incomplete_beta(a, b, x).unwrap();
        let rhs = 1.0 - regularized_incomplete_beta(b, a, 1.0 - x).unwrap();
        assert!((lhs - rhs).abs() < 1e-14);
        assert_eq!(regularized_incomplete_beta(a, b, 0.0).unwrap(), 0.0);
        assert_eq!(regularized_incomplete_beta(a, b, 1.0).unwrap(), 1.0);
        assert!(regularized_incomplete_beta(0.0, 1.0, 0.5).is_err());
        assert!(regularized_incomplete_beta(1.0, 1.0, 1.5).is_err());
    }

    #[test]
    fn incomplete_beta_large_parameters_converges() {
        // Symmetric case: I_{1/2}(a, a) = 1/2 exactly.
        let v = regularized_incomplete_beta(5000.0, 5000.0, 0.5).unwrap();
        assert!((v - 0.5).abs() < 1e-10);
    }

    #[test]
    fn incomplete_gamma_closed_forms() {
        // P(1, x) = 1 − e^{−x};  Q(1, x) = e^{−x}.
        for &x in &[0.01, 0.5, 2.0, 7.5, 40.0] {
            assert!(rel(regularized_gamma_q(1.0, x).unwrap(), (-x).exp()) < 1e-13, "x = {x}");
            assert!(rel(regularized_gamma_p(1.0, x).unwrap(), -(-x).exp_m1()) < 1e-13, "x = {x}");
        }
        // Q(n, x) = e^{−x} Σ_{k<n} xᵏ/k! for integer n.
        let (n, x) = (4.0, 3.2_f64);
        let expected = (-x).exp() * (1.0 + x + x * x / 2.0 + x.powi(3) / 6.0);
        assert!(rel(regularized_gamma_q(n, x).unwrap(), expected) < 1e-13);
        assert!((regularized_gamma_p(n, x).unwrap() + regularized_gamma_q(n, x).unwrap() - 1.0).abs() < 1e-15);
        assert!(regularized_gamma_p(0.0, 1.0).is_err());
        assert!(regularized_gamma_q(1.0, -1.0).is_err());
    }

    #[test]
    fn erfc_reference_values() {
        // Reference values: DLMF / Abramowitz & Stegun Table 7.1 (to the digits shown there,
        // extended with standard high-precision values).
        let cases = [
            (0.0, 1.0),
            (0.5, 0.479_500_122_186_953_5),
            (1.0, 0.157_299_207_050_285_1),
            (2.0, 0.004_677_734_981_047_266),
            (5.0, 1.537_459_794_428_035e-12),
            (-1.0, 1.842_700_792_949_715),
        ];
        for (x, v) in cases {
            assert!(rel(erfc(x), v) < 1e-13, "erfc({x}) = {} vs {v}", erfc(x));
        }
        assert_eq!(erfc(f64::INFINITY), 0.0);
        assert_eq!(erfc(f64::NEG_INFINITY), 2.0);
    }

    #[test]
    fn lambert_w_and_wright_omega() {
        // Reference values: Corless et al. (1996); Ω = W(1) = 0.5671432904097838.
        let cases = [
            (1.0, 0.567_143_290_409_783_8),
            (std::f64::consts::E, 1.0),
            (10.0, 1.745_528_002_740_699_4),
            (-0.2, -0.259_171_101_819_073_8),
            (1e-10, 9.999_999_999e-11),
            (1e300, 684.247_208_629_760_8),
        ];
        for (x, w) in cases {
            assert!(rel(lambert_w(x), w) < 1e-14, "W({x}) = {} vs {w}", lambert_w(x));
        }
        assert!((lambert_w(-1.0 / std::f64::consts::E) + 1.0).abs() < 1e-7);
        assert!(lambert_w(-0.5).is_nan());
        assert_eq!(lambert_w(0.0), 0.0);
        // Defining equations, across the whole range.
        for k in -300..=300 {
            let x = -0.367 + 0.367 * f64::from(k + 300) / 600.0;
            let w = lambert_w(x);
            assert!((w * w.exp() - x).abs() <= 1e-15 * x.abs().max(1e-3), "x = {x}");
        }
        for k in -140..=140 {
            let z = 5.0 * f64::from(k);
            let w = wright_omega(z);
            assert!(w > 0.0 && rel(w + w.ln(), z) < 1e-14 || (w + w.ln() - z).abs() < 1e-13, "z = {z}");
        }
        assert!(rel(wright_omega(0.0), 0.567_143_290_409_783_8) < 1e-15);
        assert!(rel(wright_omega(1.0), 1.0) < 1e-15);
        assert!(rel(wright_omega(-700.0), (-700.0_f64).exp()) < 1e-15);
    }
}
