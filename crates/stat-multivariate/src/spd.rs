//! Small dense helpers for symmetric positive-definite matrices (covariance
//! and correlation matrices of a few dozen variables): sample moments, the
//! Cholesky factor `A = L Lᵀ`, quadratic forms `dᵀ A⁻¹ d` by one triangular
//! solve, and the inverse. Matrices are row vectors (`Vec<Vec<f64>>`).

use stat_core::summation::sum_iter;

/// Column means of row vectors (`p` columns).
pub(crate) fn mean_vector(rows: &[Vec<f64>], p: usize) -> Vec<f64> {
    let n = rows.len() as f64;
    (0..p).map(|j| sum_iter(rows.iter().map(|r| r[j])) / n).collect()
}

/// Sample covariance matrix (divisor `n − 1`) about `mean`, with compensated
/// sums of centred cross-products.
pub(crate) fn covariance(rows: &[Vec<f64>], mean: &[f64]) -> Vec<Vec<f64>> {
    let p = mean.len();
    let d = rows.len() as f64 - 1.0;
    let mut c = vec![vec![0.0; p]; p];
    for a in 0..p {
        for b in a..p {
            let s = sum_iter(rows.iter().map(|r| (r[a] - mean[a]) * (r[b] - mean[b]))) / d;
            c[a][b] = s;
            c[b][a] = s;
        }
    }
    c
}

/// Lower Cholesky factor of a symmetric matrix, or `None` when it is not
/// numerically positive definite (a pivot at or below `1e-12` of the
/// largest diagonal element, so near-singular matrices are refused rather
/// than inverted inaccurately).
pub(crate) fn cholesky(a: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let p = a.len();
    let scale = a.iter().enumerate().map(|(i, r)| r[i].abs()).fold(0.0_f64, f64::max);
    if !(scale > 0.0 && scale.is_finite()) {
        return None;
    }
    let mut l = vec![vec![0.0; p]; p];
    for i in 0..p {
        for j in 0..=i {
            let s = a[i][j] - (0..j).map(|k| l[i][k] * l[j][k]).sum::<f64>();
            if i == j {
                if s <= 1e-12 * scale || !s.is_finite() {
                    return None;
                }
                l[i][i] = s.sqrt();
            } else {
                l[i][j] = s / l[j][j];
            }
        }
    }
    Some(l)
}

/// Solve `L z = d` (forward substitution).
pub(crate) fn forward(l: &[Vec<f64>], d: &[f64]) -> Vec<f64> {
    let mut z = vec![0.0; d.len()];
    for i in 0..d.len() {
        z[i] = (d[i] - (0..i).map(|k| l[i][k] * z[k]).sum::<f64>()) / l[i][i];
    }
    z
}

/// `dᵀ A⁻¹ d` from the Cholesky factor `L` of `A`.
pub(crate) fn quadratic_form(l: &[Vec<f64>], d: &[f64]) -> f64 {
    forward(l, d).iter().map(|z| z * z).sum()
}

/// `A⁻¹` from the Cholesky factor `L` of `A`: column `j` solves
/// `L Lᵀ x = eⱼ`.
pub(crate) fn inverse(l: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let p = l.len();
    let mut inv = vec![vec![0.0; p]; p];
    for j in 0..p {
        let mut e = vec![0.0; p];
        e[j] = 1.0;
        let z = forward(l, &e);
        // Back substitution Lᵀ x = z.
        let mut x = vec![0.0; p];
        for i in (0..p).rev() {
            x[i] = (z[i] - (i + 1..p).map(|k| l[k][i] * x[k]).sum::<f64>()) / l[i][i];
        }
        for i in 0..p {
            inv[i][j] = x[i];
        }
    }
    inv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_and_quadratic_form() {
        // A = [[4, 2], [2, 3]]: A⁻¹ = [[3, −2], [−2, 4]] / 8 (exact).
        let a = vec![vec![4.0, 2.0], vec![2.0, 3.0]];
        let l = cholesky(&a).unwrap();
        let inv = inverse(&l);
        let want = [[0.375, -0.25], [-0.25, 0.5]];
        for i in 0..2 {
            for j in 0..2 {
                assert!((inv[i][j] - want[i][j]).abs() < 1e-15);
            }
        }
        // [1, 1] A⁻¹ [1, 1]ᵀ = (3 − 4 + 4) / 8 = 0.375.
        assert!((quadratic_form(&l, &[1.0, 1.0]) - 0.375).abs() < 1e-15);
        assert!(cholesky(&[vec![1.0, 1.0], vec![1.0, 1.0]]).is_none(), "singular");
    }
}
