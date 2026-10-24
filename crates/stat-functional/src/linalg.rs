//! Small dense helpers for matrices stored as rows.

use stat_core::eigen::cholesky;

/// `A x`.
pub(crate) fn mat_vec(a: &[Vec<f64>], x: &[f64]) -> Vec<f64> {
    a.iter().map(|r| dot(r, x)).collect()
}

pub(crate) fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// `xᵀ A y`.
pub(crate) fn quad_form(a: &[Vec<f64>], x: &[f64], y: &[f64]) -> f64 {
    dot(x, &mat_vec(a, y))
}

/// Solve `L Lᵀ x = b` given the lower Cholesky factor.
pub(crate) fn chol_solve(l: &[Vec<f64>], b: &[f64]) -> Vec<f64> {
    let n = b.len();
    let mut y = b.to_vec();
    for i in 0..n {
        let s: f64 = (0..i).map(|k| l[i][k] * y[k]).sum();
        y[i] = (y[i] - s) / l[i][i];
    }
    for i in (0..n).rev() {
        let s: f64 = (i + 1..n).map(|k| l[k][i] * y[k]).sum();
        y[i] = (y[i] - s) / l[i][i];
    }
    y
}

/// Inverse of a symmetric positive-definite matrix, or `None` when it is
/// not numerically positive definite.
pub(crate) fn spd_inverse(a: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let l = cholesky(a)?;
    let n = a.len();
    let cols: Vec<Vec<f64>> = (0..n)
        .map(|j| {
            let mut e = vec![0.0; n];
            e[j] = 1.0;
            chol_solve(&l, &e)
        })
        .collect();
    Some((0..n).map(|i| (0..n).map(|j| 0.5 * (cols[j][i] + cols[i][j])).collect()).collect())
}

#[allow(dead_code)]
/// Solve `L x = b` (lower triangular).
pub(crate) fn forward(l: &[Vec<f64>], b: &[f64]) -> Vec<f64> {
    let n = b.len();
    let mut y = b.to_vec();
    for i in 0..n {
        let s: f64 = (0..i).map(|k| l[i][k] * y[k]).sum();
        y[i] = (y[i] - s) / l[i][i];
    }
    y
}

/// Solve `Lᵀ x = b` (`L` lower triangular).
pub(crate) fn backward_transpose(l: &[Vec<f64>], b: &[f64]) -> Vec<f64> {
    let n = b.len();
    let mut y = b.to_vec();
    for i in (0..n).rev() {
        let s: f64 = (i + 1..n).map(|k| l[k][i] * y[k]).sum();
        y[i] = (y[i] - s) / l[i][i];
    }
    y
}

/// Composite Simpson weights for `n` (odd, ≥ 3) equally spaced points on
/// `[a, b]`; trapezoid weights when `n` is even.
pub(crate) fn simpson_weights(n: usize, a: f64, b: f64) -> Vec<f64> {
    let h = (b - a) / (n - 1) as f64;
    if n % 2 == 1 && n >= 3 {
        (0..n)
            .map(|i| {
                let c = if i == 0 || i == n - 1 {
                    1.0
                } else if i % 2 == 1 {
                    4.0
                } else {
                    2.0
                };
                c * h / 3.0
            })
            .collect()
    } else {
        (0..n).map(|i| if i == 0 || i == n - 1 { 0.5 * h } else { h }).collect()
    }
}

/// `n` equally spaced points from `a` to `b`.
pub(crate) fn linspace(a: f64, b: f64, n: usize) -> Vec<f64> {
    if n == 1 {
        return vec![a];
    }
    (0..n).map(|i| if i == n - 1 { b } else { a + (b - a) * i as f64 / (n - 1) as f64 }).collect()
}

/// Linear interpolation of `(xs, ys)` (`xs` increasing) at `x`, clamped.
pub(crate) fn interp(xs: &[f64], ys: &[f64], x: f64) -> f64 {
    let n = xs.len();
    if x <= xs[0] {
        return ys[0];
    }
    if x >= xs[n - 1] {
        return ys[n - 1];
    }
    let i = xs.partition_point(|v| *v <= x).saturating_sub(1).min(n - 2);
    let f = (x - xs[i]) / (xs[i + 1] - xs[i]);
    ys[i] + f * (ys[i + 1] - ys[i])
}
