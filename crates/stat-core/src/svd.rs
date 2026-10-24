//! Singular value decomposition by one-sided Jacobi rotations.
//!
//! For an `m × n` matrix `A` the thin SVD is `A = U Σ Vᵀ` with `U` (`m × r`)
//! and `V` (`n × r`) having orthonormal columns, `r = min(m, n)`, and singular
//! values `σ₁ ≥ σ₂ ≥ … ≥ σᵣ ≥ 0`.
//!
//! # Method
//! Hestenes' one-sided Jacobi method (Hestenes 1958; Demmel & Veselić 1992,
//! "Jacobi's method is more accurate than QR", *SIAM J. Matrix Anal. Appl.* 13):
//! plane rotations are applied to pairs of columns of `A` until every pair is
//! orthogonal to working precision (`|aᵢᵀaⱼ| ≤ ε √(aᵢᵀaᵢ · aⱼᵀaⱼ)`); the rotations
//! accumulate into `V`, the final column norms are the singular values, and the
//! normalized columns form `U`. The method computes small singular values to
//! high relative accuracy and is simple to verify. For `m < n` the transpose is
//! decomposed. Cost is `O(m n² · sweeps)`, with typically 6–10 sweeps.

use crate::error::{Result, StatError};
use crate::matrix::{Matrix, scale_by_power_of_two, unit_scale_exponent};

/// Thin singular value decomposition `A = U Σ Vᵀ`.
#[derive(Debug, Clone, PartialEq)]
pub struct Svd {
    /// Left singular vectors (`m × r`, orthonormal columns).
    pub u: Matrix,
    /// Singular values in decreasing order (length `r`).
    pub singular_values: Vec<f64>,
    /// Right singular vectors (`n × r`, orthonormal columns).
    pub v: Matrix,
}

const MAX_SWEEPS: usize = 60;

/// Compute the thin SVD of `a`.
///
/// # Errors
/// [`StatError::NonFiniteInput`] for NaN or ±∞; [`StatError::InvalidArgument`]
/// for an empty matrix; [`StatError::ConvergenceFailure`] if the rotations do
/// not converge (not expected for finite input).
pub fn svd(a: &Matrix) -> Result<Svd> {
    if a.nrows() == 0 || a.ncols() == 0 {
        return Err(StatError::InvalidArgument { message: "the SVD needs a non-empty matrix".into() });
    }
    if !a.is_finite() {
        return Err(StatError::NonFiniteInput { context: "SVD input" });
    }
    // Entries beyond 10^{±150} are brought to unit scale by an exact power of
    // two, since the rotations form sums of squares of columns.
    let max_abs = a.as_slice().iter().fold(0.0_f64, |m, x| m.max(x.abs()));
    if let Some(exponent) = unit_scale_exponent(max_abs) {
        let mut scaled = a.clone();
        for x in scaled.as_mut_slice() {
            *x = scale_by_power_of_two(*x, exponent);
        }
        let mut result = svd(&scaled)?;
        for s in &mut result.singular_values {
            *s = scale_by_power_of_two(*s, -exponent);
        }
        return Ok(result);
    }
    if a.nrows() < a.ncols() {
        let t = svd_tall(&a.transpose())?;
        return Ok(Svd { u: t.v, singular_values: t.singular_values, v: t.u });
    }
    svd_tall(a)
}

/// One-sided Jacobi for `m ≥ n`.
fn svd_tall(a: &Matrix) -> Result<Svd> {
    let (m, n) = (a.nrows(), a.ncols());
    let mut w = a.clone();
    let mut v = Matrix::identity(n);
    let eps = f64::EPSILON;
    let mut converged = false;
    for _ in 0..MAX_SWEEPS {
        let mut rotated = false;
        for p in 0..n {
            for q in p + 1..n {
                let (alpha, beta, gamma) = {
                    let (cp, cq) = (w.col(p), w.col(q));
                    let alpha: f64 = cp.iter().map(|x| x * x).sum();
                    let beta: f64 = cq.iter().map(|x| x * x).sum();
                    let gamma: f64 = cp.iter().zip(cq).map(|(x, y)| x * y).sum();
                    (alpha, beta, gamma)
                };
                if gamma == 0.0 || gamma.abs() <= eps * (alpha * beta).sqrt() {
                    continue;
                }
                rotated = true;
                // Rotation that zeroes the (p, q) entry of WᵀW.
                let zeta = (beta - alpha) / (2.0 * gamma);
                let t = zeta.signum() / (zeta.abs() + (1.0 + zeta * zeta).sqrt());
                let t = if zeta == 0.0 { 1.0 } else { t };
                let c = 1.0 / (1.0 + t * t).sqrt();
                let s = c * t;
                rotate(&mut w, p, q, c, s, m);
                rotate(&mut v, p, q, c, s, n);
            }
        }
        if !rotated {
            converged = true;
            break;
        }
    }
    if !converged {
        return Err(StatError::ConvergenceFailure { routine: "one-sided Jacobi SVD", iterations: MAX_SWEEPS });
    }
    // Singular values are the column norms; sort in decreasing order.
    let norms: Vec<f64> = (0..n).map(|j| w.col(j).iter().map(|x| x * x).sum::<f64>().sqrt()).collect();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&i, &j| norms[j].total_cmp(&norms[i]));
    let mut u = Matrix::zeros(m, n);
    let mut vs = Matrix::zeros(n, n);
    let mut singular_values = Vec::with_capacity(n);
    let tiny = norms.iter().copied().fold(0.0, f64::max) * eps * m.max(n) as f64;
    for (k, &j) in order.iter().enumerate() {
        let s = norms[j];
        singular_values.push(s);
        for i in 0..m {
            u[(i, k)] = if s > tiny { w[(i, j)] / s } else { 0.0 };
        }
        for i in 0..n {
            vs[(i, k)] = v[(i, j)];
        }
    }
    // Give each null direction of U a unit vector orthogonal to the rest, so
    // U always has orthonormal columns (Gram–Schmidt against the identity).
    for k in 0..n {
        if singular_values[k] > tiny {
            continue;
        }
        for e in 0..m {
            let mut cand = vec![0.0; m];
            cand[e] = 1.0;
            for j in 0..n {
                if j == k || (singular_values[j] <= tiny && j > k) {
                    continue;
                }
                let d: f64 = (0..m).map(|i| u[(i, j)] * cand[i]).sum();
                for (i, c) in cand.iter_mut().enumerate() {
                    *c -= d * u[(i, j)];
                }
            }
            let norm = cand.iter().map(|x| x * x).sum::<f64>().sqrt();
            if norm > 0.5 {
                for i in 0..m {
                    u[(i, k)] = cand[i] / norm;
                }
                break;
            }
        }
    }
    // Sign convention: the largest-magnitude entry of each right singular
    // vector is positive (makes results reproducible across implementations).
    for k in 0..n {
        let col = vs.col(k);
        let big = col.iter().copied().fold(0.0f64, |acc, x| if x.abs() > acc.abs() { x } else { acc });
        if big < 0.0 {
            for i in 0..n {
                vs[(i, k)] = -vs[(i, k)];
            }
            for i in 0..m {
                u[(i, k)] = -u[(i, k)];
            }
        }
    }
    Ok(Svd { u, singular_values, v: vs })
}

fn rotate(a: &mut Matrix, p: usize, q: usize, c: f64, s: f64, rows: usize) {
    for i in 0..rows {
        let (x, y) = (a[(i, p)], a[(i, q)]);
        a[(i, p)] = c * x - s * y;
        a[(i, q)] = s * x + c * y;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reconstruct(s: &Svd) -> Matrix {
        let (m, n) = (s.u.nrows(), s.v.nrows());
        let mut a = Matrix::zeros(m, n);
        for (k, &sv) in s.singular_values.iter().enumerate() {
            for i in 0..m {
                for j in 0..n {
                    a[(i, j)] += s.u[(i, k)] * sv * s.v[(j, k)];
                }
            }
        }
        a
    }

    fn assert_orthonormal(m: &Matrix) {
        for a in 0..m.ncols() {
            for b in 0..m.ncols() {
                let d: f64 = m.col(a).iter().zip(m.col(b)).map(|(x, y)| x * y).sum();
                let e = if a == b { 1.0 } else { 0.0 };
                assert!((d - e).abs() < 1e-12, "columns {a}, {b}: {d}");
            }
        }
    }

    #[test]
    fn known_singular_values() {
        // [[3, 2, 2], [2, 3, -2]]: singular values 5 and 3.
        let a = Matrix::from_rows(&[[3.0, 2.0, 2.0], [2.0, 3.0, -2.0]]).unwrap();
        let s = svd(&a).unwrap();
        assert!((s.singular_values[0] - 5.0).abs() < 1e-14);
        assert!((s.singular_values[1] - 3.0).abs() < 1e-14);
        assert_eq!((s.u.nrows(), s.u.ncols(), s.v.nrows(), s.v.ncols()), (2, 2, 3, 2));
        let r = reconstruct(&s);
        for i in 0..2 {
            for j in 0..3 {
                assert!((r[(i, j)] - a[(i, j)]).abs() < 1e-13);
            }
        }
    }

    #[test]
    fn tall_rank_deficient_and_reconstruction() {
        // Third column = first + second: rank 2.
        let rows: Vec<[f64; 3]> = (0..7)
            .map(|i| {
                let x = f64::from(i);
                [x, x * x - 3.0, x + x * x - 3.0]
            })
            .collect();
        let a = Matrix::from_rows(&rows).unwrap();
        let s = svd(&a).unwrap();
        assert!(s.singular_values[2] < 1e-12 * s.singular_values[0]);
        assert!(s.singular_values.windows(2).all(|w| w[0] >= w[1]));
        assert_orthonormal(&s.u);
        assert_orthonormal(&s.v);
        let r = reconstruct(&s);
        for i in 0..7 {
            for j in 0..3 {
                assert!((r[(i, j)] - a[(i, j)]).abs() < 1e-12 * 50.0);
            }
        }
    }

    #[test]
    fn hilbert_matrix_small_singular_values() {
        // Hilbert 6×6: smallest singular value ≈ 1.0827e-7 (its eigenvalue, since
        // the matrix is symmetric positive definite).
        let n = 6;
        let rows: Vec<Vec<f64>> = (0..n).map(|i| (0..n).map(|j| 1.0 / (i + j + 1) as f64).collect()).collect();
        let s = svd(&Matrix::from_rows(&rows).unwrap()).unwrap();
        assert!((s.singular_values[0] - 1.618_899_858_924_338_8).abs() < 1e-13);
        assert!((s.singular_values[5] / 1.082_799_484_565_14e-7 - 1.0).abs() < 1e-8);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(svd(&Matrix::zeros(0, 3)).is_err());
        let mut m = Matrix::zeros(2, 2);
        m[(0, 0)] = f64::NAN;
        assert!(svd(&m).is_err());
        let z = svd(&Matrix::zeros(3, 2)).unwrap();
        assert_eq!(z.singular_values, [0.0, 0.0]);
        assert_orthonormal(&z.u);
    }
}
