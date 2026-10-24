//! Eigen-decompositions of small dense symmetric matrices (a few hundred
//! rows at most): covariance, correlation and SSCP matrices, and the
//! symmetric-definite generalized problems of multivariate analysis.
//!
//! * [`symmetric_eigen`]: `A = V Λ Vᵀ` by cyclic Jacobi rotations (Golub &
//!   Van Loan, *Matrix Computations*, 4th ed., §8.5). Jacobi is slower than
//!   tridiagonal QR but delivers small eigenvalues to high relative accuracy,
//!   which matters for nearly singular covariance matrices.
//! * [`generalized_symmetric_eigen`]: `A v = λ B v` with `A` symmetric and
//!   `B` symmetric positive definite, reduced to a standard problem through
//!   the Cholesky factor `B = L Lᵀ`: `(L⁻¹ A L⁻ᵀ) w = λ w`, `v = L⁻ᵀ w`
//!   (Golub & Van Loan §8.7.2). The eigenvectors are `B`-orthonormal:
//!   `Vᵀ B V = I`. This is the eigen-analysis of `E⁻¹H` in MANOVA and of
//!   `W⁻¹B` in canonical discriminant analysis.
//!
//! Matrices are row vectors (`Vec<Vec<f64>>`, each inner vector a row).

use crate::matrix::{scale_by_power_of_two, unit_scale_exponent};
use crate::{Result, StatError};

/// Eigenvalues (largest first) and unit eigenvectors of a symmetric matrix.
///
/// Returns `(values, vectors)` where `vectors[k]` is the eigenvector of
/// `values[k]`. Each eigenvector's largest-magnitude component is made
/// positive, so the signs are reproducible. Only the lower triangle need be
/// exact; the matrix is symmetrized as `(A + Aᵀ)/2` first.
///
/// # Errors
/// A non-square matrix or a non-finite element.
pub fn symmetric_eigen(a: &[Vec<f64>]) -> Result<(Vec<f64>, Vec<Vec<f64>>)> {
    let n = a.len();
    if a.iter().any(|r| r.len() != n) {
        return Err(invalid("symmetric_eigen: the matrix must be square"));
    }
    if a.iter().flatten().any(|x| !x.is_finite()) {
        return Err(invalid("symmetric_eigen: the matrix has a non-finite element"));
    }
    // Entries beyond 10^{±150} are brought to unit scale by an exact power of
    // two, since the rotations square them.
    let max_abs = a.iter().flatten().fold(0.0_f64, |m, x| m.max(x.abs()));
    let exponent = unit_scale_exponent(max_abs).unwrap_or(0);
    let mut m: Vec<Vec<f64>> = (0..n)
        .map(|i| {
            (0..n)
                .map(|j| 0.5 * (scale_by_power_of_two(a[i][j], exponent) + scale_by_power_of_two(a[j][i], exponent)))
                .collect()
        })
        .collect();
    let mut v: Vec<Vec<f64>> = (0..n).map(|i| (0..n).map(|j| f64::from(u8::from(i == j))).collect()).collect();
    for _sweep in 0..100 {
        let off: f64 = (0..n)
            .flat_map(|i| (0..n).filter(move |&j| j != i).map(move |j| (i, j)))
            .map(|(i, j)| m[i][j] * m[i][j])
            .sum();
        let total: f64 = m.iter().flatten().map(|x| x * x).sum();
        if off <= 1e-30 * total.max(f64::MIN_POSITIVE) {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                if m[p][q] == 0.0 {
                    continue;
                }
                let theta = (m[q][q] - m[p][p]) / (2.0 * m[p][q]);
                let t = if theta == 0.0 { 1.0 } else { theta.signum() / (theta.abs() + theta.hypot(1.0)) };
                let cos = 1.0 / t.hypot(1.0);
                let sin = t * cos;
                for row in &mut m {
                    let (mkp, mkq) = (row[p], row[q]);
                    row[p] = cos * mkp - sin * mkq;
                    row[q] = sin * mkp + cos * mkq;
                }
                for k in 0..n {
                    let (mpk, mqk) = (m[p][k], m[q][k]);
                    m[p][k] = cos * mpk - sin * mqk;
                    m[q][k] = sin * mpk + cos * mqk;
                }
                for row in &mut v {
                    let (vp, vq) = (row[p], row[q]);
                    row[p] = cos * vp - sin * vq;
                    row[q] = sin * vp + cos * vq;
                }
            }
        }
    }
    let mut pairs: Vec<(f64, Vec<f64>)> = (0..n)
        .map(|i| {
            let mut vec: Vec<f64> = v.iter().map(|row| row[i]).collect();
            orient(&mut vec);
            (scale_by_power_of_two(m[i][i], -exponent), vec)
        })
        .collect();
    pairs.sort_by(|a, b| b.0.total_cmp(&a.0));
    Ok(pairs.into_iter().unzip())
}

/// Eigenvalues (largest first) and `B`-orthonormal eigenvectors of the
/// symmetric-definite problem `A v = λ B v`.
///
/// Returns `(values, vectors)` with `vectors[k]ᵀ B vectors[k] = 1`, so for
/// `B = E/ν` (an error covariance) each vector is a linear combination of
/// the variables with unit error variance. Each vector's largest-magnitude
/// component is made positive.
///
/// # Errors
/// Mismatched or non-square matrices, non-finite elements, or a `B` that is
/// not numerically positive definite (a Cholesky pivot at or below `1e-12`
/// of its largest diagonal element).
pub fn generalized_symmetric_eigen(a: &[Vec<f64>], b: &[Vec<f64>]) -> Result<(Vec<f64>, Vec<Vec<f64>>)> {
    let n = a.len();
    if b.len() != n || a.iter().chain(b).any(|r| r.len() != n) {
        return Err(invalid("generalized_symmetric_eigen: the matrices must be square and of the same size"));
    }
    if a.iter().chain(b).flatten().any(|x| !x.is_finite()) {
        return Err(invalid("generalized_symmetric_eigen: a matrix has a non-finite element"));
    }
    let l = cholesky(b)
        .ok_or_else(|| invalid("generalized_symmetric_eigen: the second matrix is not positive definite"))?;
    // C = L⁻¹ A L⁻ᵀ: solve L X = A, then L Cᵀ = Xᵀ.
    let x: Vec<Vec<f64>> = (0..n).map(|j| forward(&l, &(0..n).map(|i| a[i][j]).collect::<Vec<_>>())).collect();
    // x[j] is column j of L⁻¹A; C = (L⁻¹A) L⁻ᵀ = (L⁻¹ (L⁻¹A)ᵀ)ᵀ.
    let rows_of_l_inv_a: Vec<Vec<f64>> = (0..n).map(|i| (0..n).map(|j| x[j][i]).collect()).collect();
    let c_cols: Vec<Vec<f64>> = (0..n).map(|i| forward(&l, &rows_of_l_inv_a[i])).collect();
    let c: Vec<Vec<f64>> = (0..n).map(|i| (0..n).map(|j| c_cols[j][i]).collect()).collect();
    let (values, w) = symmetric_eigen(&c)?;
    let vectors = w
        .iter()
        .map(|wk| {
            let mut v = backward_transpose(&l, wk);
            orient(&mut v);
            v
        })
        .collect();
    Ok((values, vectors))
}

/// Lower Cholesky factor `L` of a symmetric positive-definite matrix
/// (`A = L Lᵀ`), or `None` when a pivot is at or below `1e-12` of the
/// largest diagonal element.
#[must_use]
pub fn cholesky(a: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let p = a.len();
    if a.iter().any(|r| r.len() != p) {
        return None;
    }
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

/// Solve `L z = d` for lower-triangular `L`.
fn forward(l: &[Vec<f64>], d: &[f64]) -> Vec<f64> {
    let mut z = vec![0.0; d.len()];
    for i in 0..d.len() {
        z[i] = (d[i] - (0..i).map(|k| l[i][k] * z[k]).sum::<f64>()) / l[i][i];
    }
    z
}

/// Solve `Lᵀ z = d` for lower-triangular `L`.
fn backward_transpose(l: &[Vec<f64>], d: &[f64]) -> Vec<f64> {
    let n = d.len();
    let mut z = vec![0.0; n];
    for i in (0..n).rev() {
        z[i] = (d[i] - (i + 1..n).map(|k| l[k][i] * z[k]).sum::<f64>()) / l[i][i];
    }
    z
}

/// Make the largest-magnitude component positive.
fn orient(v: &mut [f64]) {
    let big = v.iter().copied().fold(0.0_f64, |best, x| if x.abs() > best.abs() { x } else { best });
    if big < 0.0 {
        v.iter_mut().for_each(|x| *x = -*x);
    }
}

fn invalid(message: &str) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * (1.0 + b.abs())
    }

    /// Eigenvalues of a known matrix (reference: NumPy `eigh`), and the
    /// decomposition reconstructs the matrix.
    #[test]
    fn symmetric_known_values_and_reconstruction() {
        let a = vec![vec![4.0, 1.0, 2.0], vec![1.0, 3.0, 0.5], vec![2.0, 0.5, 5.0]];
        let (l, v) = symmetric_eigen(&a).unwrap();
        // numpy.linalg.eigh([[4,1,2],[1,3,.5],[2,.5,5]])
        let expected = [6.831254430698_f64, 3.072223952005, 2.096521617297];
        for (x, e) in l.iter().zip(expected) {
            assert!(close(*x, e, 1e-10), "{x} vs {e}");
        }
        for i in 0..3 {
            for j in 0..3 {
                let r: f64 = (0..3).map(|k| l[k] * v[k][i] * v[k][j]).sum();
                assert!(close(r, a[i][j], 1e-12));
                let ortho: f64 = (0..3).map(|k| v[i][k] * v[j][k]).sum();
                assert!(close(ortho, f64::from(u8::from(i == j)), 1e-12));
            }
        }
        assert!(symmetric_eigen(&[vec![1.0, 2.0]]).is_err());
    }

    /// `A v = λ B v`, `vᵀ B v = 1`, and the eigenvalues match those of
    /// `B⁻¹A` (NumPy `scipy.linalg.eigh(A, B)`).
    #[test]
    fn generalized_problem() {
        let a = vec![vec![2.0, 1.0, 0.0], vec![1.0, 3.0, 1.0], vec![0.0, 1.0, 1.0]];
        let b = vec![vec![4.0, 1.0, 0.5], vec![1.0, 2.0, 0.3], vec![0.5, 0.3, 1.5]];
        let (l, v) = generalized_symmetric_eigen(&a, &b).unwrap();
        for k in 0..3 {
            let av: Vec<f64> = (0..3).map(|i| (0..3).map(|j| a[i][j] * v[k][j]).sum()).collect();
            let bv: Vec<f64> = (0..3).map(|i| (0..3).map(|j| b[i][j] * v[k][j]).sum()).collect();
            for i in 0..3 {
                assert!(close(av[i], l[k] * bv[i], 1e-11));
            }
            let norm: f64 = (0..3).map(|i| v[k][i] * bv[i]).sum();
            assert!(close(norm, 1.0, 1e-12));
        }
        // scipy.linalg.eigh(A, B, eigvals_only=True)[::-1]
        let expected = [1.687088468806_f64, 0.650633047937, 0.274954539595];
        for (x, e) in l.iter().zip(expected) {
            assert!(close(*x, e, 1e-10), "{x} vs {e}");
        }
        let singular = vec![vec![1.0, 1.0, 0.0], vec![1.0, 1.0, 0.0], vec![0.0, 0.0, 1.0]];
        assert!(generalized_symmetric_eigen(&a, &singular).is_err());
    }
}
