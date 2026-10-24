//! Accurate floating-point summation and dot products.
//!
//! Statistical results such as residual sums of squares are sums of many terms of
//! mixed magnitude, where naive accumulation can lose most significant digits.
//! These routines use error-free transformations so the result is as accurate as
//! if it had been computed in roughly twice the working precision and then rounded.
//!
//! * `two_sum` — Knuth's error-free addition: `a + b = s + e` exactly.
//! * `two_prod` — error-free multiplication via fused multiply-add:
//!   `a · b = p + e` exactly.
//! * [`sum`] — Neumaier's improved Kahan–Babuška summation.
//! * [`dot`] — the `Dot2` algorithm of Ogita, Rump & Oishi (2005),
//!   "Accurate sum and dot product", SIAM J. Sci. Comput. 26(6).

#[inline]
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let bb = s - a;
    let e = (a - (s - bb)) + (b - bb);
    (s, e)
}

#[inline]
fn two_prod(a: f64, b: f64) -> (f64, f64) {
    let p = a * b;
    let e = a.mul_add(b, -p);
    (p, e)
}

/// Compensated sum of `values` (Neumaier's algorithm).
///
/// The error bound is `|ŝ − s| ≤ ε|s| + O(n ε²) Σ|xᵢ|`, compared with
/// `O(n ε) Σ|xᵢ|` for naive summation.
#[must_use]
pub fn sum(values: &[f64]) -> f64 {
    let mut s = 0.0;
    let mut c = 0.0;
    for &x in values {
        let (t, e) = two_sum(s, x);
        s = t;
        c += e;
    }
    s + c
}

/// Compensated sum of an iterator of values.
#[must_use]
pub fn sum_iter<I: IntoIterator<Item = f64>>(values: I) -> f64 {
    let mut s = 0.0;
    let mut c = 0.0;
    for x in values {
        let (t, e) = two_sum(s, x);
        s = t;
        c += e;
    }
    s + c
}

/// Compensated dot product `Σ aᵢ bᵢ` (`Dot2`).
///
/// The result is as accurate as a dot product computed in doubled precision and
/// rounded once to `f64`.
///
/// # Panics
/// If the slices have different lengths.
#[must_use]
pub fn dot(a: &[f64], b: &[f64]) -> f64 {
    assert_eq!(a.len(), b.len(), "dot product of slices with different lengths");
    let mut s = 0.0;
    let mut c = 0.0;
    for (&x, &y) in a.iter().zip(b) {
        let (p, pe) = two_prod(x, y);
        let (t, se) = two_sum(s, p);
        s = t;
        c += pe + se;
    }
    s + c
}

/// Compensated arithmetic mean. Returns `None` for an empty slice.
#[must_use]
pub fn mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    // Two-pass mean: the correction term removes the rounding error of the first pass.
    let n = values.len() as f64;
    let m0 = sum(values) / n;
    let correction = sum_iter(values.iter().map(|&v| v - m0)) / n;
    Some(m0 + correction)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sum_recovers_cancelled_terms() {
        // Naive left-to-right summation returns 0 here.
        let v = [1.0, 1e100, 1.0, -1e100];
        assert_eq!(sum(&v), 2.0);
        assert_eq!(sum_iter(v.iter().copied()), 2.0);
    }

    #[test]
    fn dot_is_exact_on_ill_conditioned_input() {
        let a = [1e16, 1.0, -1e16];
        let b = [1.0, 1.0, 1.0];
        assert_eq!(dot(&a, &b), 1.0);
        let x = 1.0 + f64::EPSILON;
        // x² − 1 − 2ε = ε², which naive evaluation loses entirely.
        assert_eq!(dot(&[x, -1.0, -2.0 * f64::EPSILON], &[x, 1.0, 1.0]), f64::EPSILON * f64::EPSILON);
    }

    #[test]
    fn mean_handles_large_offsets() {
        let v: Vec<f64> = (0..1000).map(|i| 1e9 + f64::from(i) * 0.1).collect();
        let m = mean(&v).unwrap();
        assert!((m - (1e9 + 49.95)).abs() < 1e-6);
        assert_eq!(mean(&[]), None);
    }
}
