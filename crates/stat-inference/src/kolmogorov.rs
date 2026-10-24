//! The two-sample Kolmogorov–Smirnov test (Hollander, Wolfe & Chicken,
//! *Nonparametric Statistical Methods*, 3rd ed., §5.4; Conover, *Practical
//! Nonparametric Statistics*, 3rd ed., §6.3).
//!
//! With empirical distribution functions `F₁` (first sample, `m` values) and
//! `F₂` (second, `n` values), the statistics are
//! `D = max |F₁(x) − F₂(x)|` (two-sided), `D⁺ = max (F₂(x) − F₁(x))` for the
//! alternative that the first sample's values tend to be larger (its
//! distribution function lies below the second's), and
//! `D⁻ = max (F₁(x) − F₂(x))` for smaller.
//!
//! * **Exact** p-values (when `mn < 10 000` and there are no ties): every
//!   arrangement of the pooled sample is equally likely under the null, so
//!   `P(D < d)` is the probability that a random lattice path from `(0, 0)`
//!   to `(m, n)` keeps `|i/m − j/n| < d` at every point (`i/m − j/n` or its
//!   negative for one side), found by a forward recursion over the grid with
//!   transition probabilities `(m − i)/(m + n − i − j)` (Hodges 1957).
//! * **Asymptotic** p-values otherwise, from `z = √(mn/(m + n))·D`:
//!   `P = 2 Σₖ₌₁^∞ (−1)^{k−1} e^{−2k²z²}` two-sided (Kolmogorov's limit) and
//!   `P = e^{−2z²}` one-sided (Smirnov 1939).

use crate::common::{Alternative, TestResult, check_finite};
use stat_core::{Result, StatError};

/// Largest `m·n` for exact p-values (as R's `ks.test`).
pub const EXACT_PRODUCT_LIMIT: usize = 10_000;

/// Kolmogorov's limiting upper tail `P(√N·D > z) = 2Σ(−1)^{k−1}e^{−2k²z²}`;
/// for small `z` the equivalent theta-function series
/// `1 − √(2π)/z Σ e^{−(2k−1)²π²/(8z²)}`.
pub fn kolmogorov_upper_tail(z: f64) -> f64 {
    if z <= 0.0 {
        return 1.0;
    }
    if z < 1.0 {
        let a = -std::f64::consts::PI * std::f64::consts::PI / (8.0 * z * z);
        let s: f64 = (1..=10).map(|k| (((2 * k - 1) * (2 * k - 1)) as f64 * a).exp()).sum();
        (1.0 - (2.0 * std::f64::consts::PI).sqrt() / z * s).clamp(0.0, 1.0)
    } else {
        let mut s = 0.0;
        for k in 1..=100 {
            let term = (-2.0 * (k * k) as f64 * z * z).exp();
            s += if k % 2 == 1 { term } else { -term };
            if term < 1e-300 {
                break;
            }
        }
        (2.0 * s).clamp(0.0, 1.0)
    }
}

/// Probability that a random path to `(m, n)` keeps `allowed(i, j)` true at
/// every point (the starting point included).
fn path_probability(m: usize, n: usize, allowed: impl Fn(usize, usize) -> bool) -> f64 {
    // u[j]: probability of reaching (i, j) without leaving the region.
    let mut u = vec![0.0; n + 1];
    for i in 0..=m {
        for j in 0..=n {
            if !allowed(i, j) {
                u[j] = 0.0;
                continue;
            }
            if i == 0 && j == 0 {
                u[0] = 1.0;
                continue;
            }
            let left = m + n - i - j + 1; // values remaining before the last step
            let from_x = if i > 0 { u[j] * (m - i + 1) as f64 / left as f64 } else { 0.0 };
            let from_y = if j > 0 { u[j - 1] * (n - j + 1) as f64 / left as f64 } else { 0.0 };
            u[j] = from_x + from_y;
        }
    }
    u[n]
}

/// Two-sample Kolmogorov–Smirnov test of `a` against `b`. `exact`: `None`
/// chooses exact p-values when `mn < 10 000` without ties; `Some(true)`
/// asks for them (ignored with ties); `Some(false)` uses the asymptotic
/// distribution. The estimate and statistic are `D` (or `D⁺`, `D⁻`); no
/// confidence interval.
///
/// # Errors
/// [`StatError::InsufficientData`] for an empty sample;
/// [`StatError::NonFiniteInput`] for NaN/±∞.
pub fn ks_two_sample(a: &[f64], b: &[f64], alternative: Alternative, exact: Option<bool>) -> Result<TestResult> {
    check_finite(a, "sample values")?;
    check_finite(b, "sample values")?;
    if a.is_empty() || b.is_empty() {
        return Err(StatError::InsufficientData { required: 1, found: 0 });
    }
    let (m, n) = (a.len(), b.len());
    let mut x = a.to_vec();
    let mut y = b.to_vec();
    x.sort_by(f64::total_cmp);
    y.sort_by(f64::total_cmp);
    // Walk the distinct pooled values; after each, F₁ − F₂.
    let (mut i, mut j) = (0, 0);
    let (mut d_minus, mut d_plus) = (0.0_f64, 0.0_f64);
    let mut ties = false;
    while i < m || j < n {
        let v = match (x.get(i), y.get(j)) {
            (Some(&p), Some(&q)) => p.min(q),
            (Some(&p), None) => p,
            (None, Some(&q)) => q,
            (None, None) => unreachable!(),
        };
        let (i0, j0) = (i, j);
        while i < m && x[i] == v {
            i += 1;
        }
        while j < n && y[j] == v {
            j += 1;
        }
        if (i - i0) + (j - j0) > 1 {
            ties = true;
        }
        let diff = i as f64 / m as f64 - j as f64 / n as f64;
        d_minus = d_minus.max(diff);
        d_plus = d_plus.max(-diff);
    }
    let d = match alternative {
        Alternative::TwoSided => d_minus.max(d_plus),
        Alternative::Greater => d_plus,
        Alternative::Less => d_minus,
    };
    let use_exact = !ties && exact.unwrap_or(m * n < EXACT_PRODUCT_LIMIT);
    let (p_value, method) = if use_exact {
        let (mf, nf) = (m as f64, n as f64);
        // As R's psmirnov: values within rounding of d count as reaching it.
        let q = (0.5 + (d * mf * nf - 1e-7).floor()) / (mf * nf);
        let stat = |i: usize, j: usize| i as f64 / mf - j as f64 / nf;
        let stay = match alternative {
            Alternative::TwoSided => path_probability(m, n, |i, j| stat(i, j).abs() <= q),
            Alternative::Greater => path_probability(m, n, |i, j| -stat(i, j) <= q),
            Alternative::Less => path_probability(m, n, |i, j| stat(i, j) <= q),
        };
        ((1.0 - stay).clamp(0.0, 1.0), "Kolmogorov–Smirnov (exact)")
    } else {
        let z = (m as f64 * n as f64 / (m + n) as f64).sqrt() * d;
        let p = match alternative {
            Alternative::TwoSided => kolmogorov_upper_tail(z),
            Alternative::Greater | Alternative::Less => (-2.0 * z * z).exp(),
        };
        (p, "Kolmogorov–Smirnov (asymptotic)")
    };
    Ok(TestResult {
        method,
        estimate: d,
        null_value: 0.0,
        std_error: f64::NAN,
        statistic: d,
        statistic_name: "D",
        df: None,
        df2: None,
        p_value,
        confidence_interval: (f64::NAN, f64::NAN),
        level: f64::NAN,
        alternative,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::means::tests::close;

    const A: [f64; 8] = [0.61, 0.29, 0.06, 0.59, -1.73, -0.74, 0.51, -0.56];
    const B: [f64; 10] = [2.2, 1.22, 0.32, 2.98, 0.69, 1.55, -0.08, 1.06, 0.86, 1.82];

    /// R 4.3.3 `ks.test(a, b, alternative = ...)` (`tests/data/kolmogorov_reference.R`);
    /// R's "less" is our `Greater` (first sample larger) and vice versa.
    #[test]
    fn exact_matches_r() {
        let t = ks_two_sample(&A, &B, Alternative::TwoSided, None).unwrap();
        close(t.statistic, 0.8, 1e-12);
        close(t.p_value, 0.002_422_414_187_120_069_8, 1e-9);
        assert!(t.method.contains("exact"));
        let lo = ks_two_sample(&A, &B, Alternative::Less, None).unwrap();
        close(lo.statistic, 0.8, 1e-12);
        close(lo.p_value, 0.001_211_207_093_560_034_9, 1e-9);
        let hi = ks_two_sample(&A, &B, Alternative::Greater, None).unwrap();
        assert!(hi.statistic.abs() < 1e-12);
        close(hi.p_value, 1.0, 1e-9);
    }

    /// R `ks.test(a, b, exact = FALSE)`, and a sample with ties.
    #[test]
    fn asymptotic_matches_r() {
        let t = ks_two_sample(&A, &B, Alternative::TwoSided, Some(false)).unwrap();
        close(t.p_value, 0.006_766_700_057_164_51, 1e-9);
        let lo = ks_two_sample(&A, &B, Alternative::Less, Some(false)).unwrap();
        close(lo.p_value, 0.003_383_350_159_617_437_6, 1e-9);
        let x = [1.0, 2.0, 2.0, 3.0, 4.0, 5.0];
        let y = [2.0, 3.0, 5.0, 6.0, 7.0, 7.0, 8.0];
        let t = ks_two_sample(&x, &y, Alternative::TwoSided, None).unwrap();
        assert!(t.method.contains("asymptotic"));
        close(t.statistic, 0.571_428_571_428_571_4, 1e-12);
        close(t.p_value, 0.242_070_301_090_674_73, 1e-9);
        assert!(ks_two_sample(&[], &y, Alternative::TwoSided, None).is_err());
    }

    #[test]
    fn kolmogorov_tail_series_agree() {
        // The two series meet at z = 1.
        let below = {
            let a = -std::f64::consts::PI.powi(2) / 8.0;
            let s: f64 = (1..=10).map(|k| (((2 * k - 1) * (2 * k - 1)) as f64 * a).exp()).sum();
            1.0 - (2.0 * std::f64::consts::PI).sqrt() * s
        };
        close(kolmogorov_upper_tail(1.0), below, 1e-12);
        // The 95% point of Kolmogorov's distribution is 1.358099 (Smirnov 1948).
        close(kolmogorov_upper_tail(1.358_098_6), 0.05, 1e-5);
    }
}
