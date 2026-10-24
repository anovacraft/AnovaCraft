//! Small dense linear algebra on row-major `Vec<f64>` matrices.

/// Lower Cholesky factor of the `p × p` symmetric matrix `a` (row-major),
/// or `None` if it is not positive definite.
pub(crate) fn cholesky(a: &[f64], p: usize) -> Option<Vec<f64>> {
    let mut l = vec![0.0; p * p];
    for i in 0..p {
        for j in 0..=i {
            let mut s = a[i * p + j];
            for k in 0..j {
                s -= l[i * p + k] * l[j * p + k];
            }
            if i == j {
                if s.is_nan() || s <= 0.0 || !s.is_finite() {
                    return None;
                }
                l[i * p + i] = s.sqrt();
            } else {
                l[i * p + j] = s / l[j * p + j];
            }
        }
    }
    Some(l)
}

/// Solve `L Lᵀ x = b` given the Cholesky factor `l`.
pub(crate) fn cholesky_solve(l: &[f64], p: usize, b: &[f64]) -> Vec<f64> {
    let mut z = b.to_vec();
    for i in 0..p {
        let mut s = z[i];
        for k in 0..i {
            s -= l[i * p + k] * z[k];
        }
        z[i] = s / l[i * p + i];
    }
    for i in (0..p).rev() {
        let mut s = z[i];
        for k in (i + 1)..p {
            s -= l[k * p + i] * z[k];
        }
        z[i] = s / l[i * p + i];
    }
    z
}

/// `ln |A|` from its Cholesky factor.
pub(crate) fn cholesky_log_det(l: &[f64], p: usize) -> f64 {
    (0..p).map(|i| 2.0 * l[i * p + i].ln()).sum()
}

/// The inverse of `A` from its Cholesky factor (row-major).
pub(crate) fn cholesky_inverse(l: &[f64], p: usize) -> Vec<f64> {
    let mut inv = vec![0.0; p * p];
    for j in 0..p {
        let mut e = vec![0.0; p];
        e[j] = 1.0;
        let c = cholesky_solve(l, p, &e);
        for i in 0..p {
            inv[i * p + j] = c[i];
        }
    }
    inv
}
