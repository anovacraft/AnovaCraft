//! Householder QR factorization with column pivoting.
//!
//! # Method
//! For an `n × p` matrix `A` with `n ≥ p`, the factorization is
//!
//! ```text
//! A P = Q R
//! ```
//!
//! where `P` is a column permutation, `Q` is orthogonal (`n × n`, stored implicitly
//! as `p` Householder reflectors `Hₖ = I − τₖ vₖ vₖᵀ`) and `R` is `p × p` upper
//! triangular with non-increasing `|Rₖₖ|`. At each step the remaining column with
//! the largest trailing 2-norm is moved into position (Businger & Golub, 1965,
//! "Linear least squares solutions by Householder transformations",
//! Numer. Math. 7). Trailing norms are recomputed rather than down-dated, which
//! costs the same asymptotic `O(n p²)` as the factorization and avoids the
//! cancellation that makes norm down-dating unreliable.
//!
//! Householder QR is backward stable: the computed factors are the exact factors
//! of `A + δA` with `‖δA‖ ≤ c·n·p·ε‖A‖` (Higham, *Accuracy and Stability of
//! Numerical Algorithms*, 2nd ed., §19). Least-squares solutions computed from it
//! avoid the squaring of the condition number incurred by the normal equations.
//!
//! # Numerical rank
//! The pivoting makes the diagonal of `R` rank-revealing in practice: the numerical
//! rank is the number of `|Rₖₖ|` exceeding `tol · |R₀₀|`. Callers should scale the
//! columns of `A` to comparable norms first, otherwise a column that is merely
//! measured in small units can be mistaken for a linearly dependent one.

use crate::error::{Result, StatError};
use crate::matrix::Matrix;

/// Euclidean norm computed with scaling to avoid spurious overflow/underflow.
#[must_use]
pub fn norm2(x: &[f64]) -> f64 {
    let scale = x.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
    if scale == 0.0 || !scale.is_finite() {
        return scale;
    }
    let ssq = crate::summation::sum_iter(x.iter().map(|v| {
        let t = v / scale;
        t * t
    }));
    scale * ssq.sqrt()
}

/// The result of factoring `A P = Q R`.
#[derive(Debug, Clone)]
pub struct QrDecomposition {
    /// `R` in the upper triangle; Householder vectors (with implicit unit leading
    /// element) strictly below the diagonal.
    factors: Matrix,
    /// Householder scalars `τₖ`.
    tau: Vec<f64>,
    /// `perm[k]` is the index in the original matrix of the column placed at
    /// position `k`.
    perm: Vec<usize>,
}

impl QrDecomposition {
    /// Factor `a` with column pivoting.
    ///
    /// # Errors
    /// * [`StatError::InsufficientData`] if `a` has fewer rows than columns.
    /// * [`StatError::NonFiniteInput`] if `a` contains NaN or ±∞.
    pub fn new(mut a: Matrix) -> Result<Self> {
        let (n, p) = (a.nrows(), a.ncols());
        if n < p {
            return Err(StatError::InsufficientData { required: p, found: n });
        }
        if !a.is_finite() {
            return Err(StatError::NonFiniteInput { context: "matrix passed to QR" });
        }
        let mut tau = vec![0.0; p];
        let mut perm: Vec<usize> = (0..p).collect();

        for k in 0..p {
            // Pivot: bring the column with the largest trailing norm to position k.
            let mut best = k;
            let mut best_norm = -1.0;
            for j in k..p {
                let nj = norm2(&a.col(j)[k..]);
                if nj > best_norm {
                    best_norm = nj;
                    best = j;
                }
            }
            if best != k {
                swap_columns(&mut a, k, best);
                perm.swap(k, best);
            }

            // Generate the reflector that annihilates a[k+1.., k].
            let (beta, t) = {
                let col = &mut a.col_mut(k)[k..];
                let alpha = col[0];
                let xnorm = norm2(&col[1..]);
                if xnorm == 0.0 {
                    (alpha, 0.0)
                } else {
                    let beta = -alpha.hypot(xnorm).copysign(alpha);
                    let t = (beta - alpha) / beta;
                    let inv = 1.0 / (alpha - beta);
                    for v in &mut col[1..] {
                        *v *= inv;
                    }
                    (beta, t)
                }
            };
            tau[k] = t;
            a[(k, k)] = beta;

            // Apply Hₖ to the trailing columns.
            if t != 0.0 {
                for j in k + 1..p {
                    let (vk, cj) = split_two_columns(&mut a, k, j);
                    reflect(&vk[k + 1..], t, &mut cj[k..]);
                }
            }
        }
        Ok(Self { factors: a, tau, perm })
    }

    /// Number of rows of the factored matrix.
    #[must_use]
    pub fn nrows(&self) -> usize {
        self.factors.nrows()
    }

    /// Number of columns of the factored matrix.
    #[must_use]
    pub fn ncols(&self) -> usize {
        self.factors.ncols()
    }

    /// Column permutation: position `k` of `R` corresponds to original column `perm[k]`.
    #[must_use]
    pub fn permutation(&self) -> &[usize] {
        &self.perm
    }

    /// Diagonal of `R` (non-increasing in magnitude).
    #[must_use]
    pub fn r_diagonal(&self) -> Vec<f64> {
        (0..self.ncols()).map(|k| self.factors[(k, k)]).collect()
    }

    /// The upper-triangular factor `R` (`p × p`, in pivoted column order).
    #[must_use]
    pub fn r(&self) -> Matrix {
        let p = self.ncols();
        let mut r = Matrix::zeros(p, p);
        for j in 0..p {
            for i in 0..=j {
                r[(i, j)] = self.factors[(i, j)];
            }
        }
        r
    }

    /// Default relative tolerance for [`rank`](Self::rank): `max(n, p) · ε`,
    /// the conventional choice (as used by LAPACK-based rank estimators).
    #[must_use]
    pub fn default_rank_tolerance(&self) -> f64 {
        self.nrows().max(self.ncols()) as f64 * f64::EPSILON
    }

    /// Numerical rank: the number of `|Rₖₖ| > rel_tol · |R₀₀|`.
    #[must_use]
    pub fn rank(&self, rel_tol: f64) -> usize {
        let d = self.r_diagonal();
        let Some(&r00) = d.first() else { return 0 };
        let threshold = rel_tol * r00.abs();
        if r00 == 0.0 {
            return 0;
        }
        d.iter().take_while(|v| v.abs() > threshold).count()
    }

    /// Overwrite `b` with `Qᵀ b`.
    ///
    /// # Errors
    /// [`StatError::DimensionMismatch`] if `b.len() != nrows`.
    pub fn apply_qt(&self, b: &mut [f64]) -> Result<()> {
        self.check_len(b.len())?;
        for k in 0..self.ncols() {
            reflect(&self.factors.col(k)[k + 1..], self.tau[k], &mut b[k..]);
        }
        Ok(())
    }

    /// Overwrite `b` with `Q b`.
    ///
    /// # Errors
    /// [`StatError::DimensionMismatch`] if `b.len() != nrows`.
    pub fn apply_q(&self, b: &mut [f64]) -> Result<()> {
        self.check_len(b.len())?;
        for k in (0..self.ncols()).rev() {
            reflect(&self.factors.col(k)[k + 1..], self.tau[k], &mut b[k..]);
        }
        Ok(())
    }

    /// The first `p` columns of `Q` (an orthonormal basis for the column space).
    #[must_use]
    pub fn thin_q(&self) -> Matrix {
        let (n, p) = (self.nrows(), self.ncols());
        let mut q = Matrix::zeros(n, p);
        for j in 0..p {
            let col = q.col_mut(j);
            col[j] = 1.0;
            for k in (0..p).rev() {
                reflect(&self.factors.col(k)[k + 1..], self.tau[k], &mut col[k..]);
            }
        }
        q
    }

    /// Minimum-norm-residual solution of `min ‖A x − b‖₂`, returned in the
    /// original column order.
    ///
    /// Computed as `x = P R⁻¹ (Qᵀ b)[0..p]`.
    ///
    /// # Errors
    /// * [`StatError::DimensionMismatch`] if `b.len() != nrows`.
    /// * [`StatError::RankDeficient`] if any diagonal element of `R` is exactly zero.
    ///   (Numerically rank-deficient problems should be detected with
    ///   [`rank`](Self::rank) before calling this.)
    pub fn solve_least_squares(&self, b: &[f64]) -> Result<Vec<f64>> {
        let mut c = b.to_vec();
        self.apply_qt(&mut c)?;
        let p = self.ncols();
        c.truncate(p);
        self.solve_r_in_place(&mut c)?;
        let mut x = vec![0.0; p];
        for (k, &v) in c.iter().enumerate() {
            x[self.perm[k]] = v;
        }
        Ok(x)
    }

    /// Solve `R z = c` where `c` is in pivoted (QR) order, returning `P z` — the
    /// solution in the original column order.
    ///
    /// # Errors
    /// * [`StatError::DimensionMismatch`] if `c.len() != ncols`.
    /// * [`StatError::RankDeficient`] if a diagonal element of `R` is zero.
    pub fn solve_r(&self, c: &[f64]) -> Result<Vec<f64>> {
        let p = self.ncols();
        if c.len() != p {
            return Err(StatError::DimensionMismatch {
                context: "vector length vs. QR columns",
                expected: p,
                found: c.len(),
            });
        }
        let mut z = c.to_vec();
        self.solve_r_in_place(&mut z)?;
        let mut x = vec![0.0; p];
        for (k, &v) in z.iter().enumerate() {
            x[self.perm[k]] = v;
        }
        Ok(x)
    }

    /// Solve `R z = c` in place by back substitution (pivoted order).
    fn solve_r_in_place(&self, c: &mut [f64]) -> Result<()> {
        let p = self.ncols();
        for k in (0..p).rev() {
            let rkk = self.factors[(k, k)];
            if rkk == 0.0 {
                return Err(StatError::RankDeficient { rank: k, columns: p });
            }
            let s = c[k] - crate::summation::dot(&self.factors_row_tail(k), &c[k + 1..p]);
            c[k] = s / rkk;
        }
        Ok(())
    }

    fn factors_row_tail(&self, k: usize) -> Vec<f64> {
        (k + 1..self.ncols()).map(|j| self.factors[(k, j)]).collect()
    }

    /// Solve `Rᵀ w = Pᵀ u` by forward substitution, where `u` is given in the
    /// original column order.
    ///
    /// Useful because `uᵀ (AᵀA)⁻¹ u = ‖w‖²`: quadratic forms in the inverse Gram
    /// matrix become sums of squares, with no cancellation.
    ///
    /// # Errors
    /// * [`StatError::DimensionMismatch`] if `u.len() != ncols`.
    /// * [`StatError::RankDeficient`] if a diagonal element of `R` is zero.
    pub fn solve_r_transpose(&self, u: &[f64]) -> Result<Vec<f64>> {
        let p = self.ncols();
        if u.len() != p {
            return Err(StatError::DimensionMismatch {
                context: "vector length vs. QR columns",
                expected: p,
                found: u.len(),
            });
        }
        let mut w: Vec<f64> = self.perm.iter().map(|&j| u[j]).collect();
        for k in 0..p {
            let rkk = self.factors[(k, k)];
            if rkk == 0.0 {
                return Err(StatError::RankDeficient { rank: k, columns: p });
            }
            // Column k of R above the diagonal is contiguous in storage.
            let s = w[k] - crate::summation::dot(&self.factors.col(k)[..k], &w[..k]);
            w[k] = s / rkk;
        }
        Ok(w)
    }

    /// `R⁻¹` (upper triangular, pivoted order).
    ///
    /// # Errors
    /// [`StatError::RankDeficient`] if a diagonal element of `R` is zero.
    pub fn r_inverse(&self) -> Result<Matrix> {
        let p = self.ncols();
        let mut inv = Matrix::zeros(p, p);
        for j in 0..p {
            let mut e = vec![0.0; p];
            e[j] = 1.0;
            // Only the leading j+1 entries are non-zero for an upper-triangular inverse.
            self.solve_r_in_place(&mut e)?;
            inv.col_mut(j).copy_from_slice(&e);
        }
        Ok(inv)
    }

    /// `(Aᵀ A)⁻¹` in the original column order, computed as `P R⁻¹ R⁻ᵀ Pᵀ`.
    ///
    /// For least squares this is the covariance of the estimates divided by σ².
    ///
    /// # Errors
    /// [`StatError::RankDeficient`] if a diagonal element of `R` is zero.
    pub fn inverse_gram(&self) -> Result<Matrix> {
        let p = self.ncols();
        let rinv = self.r_inverse()?;
        let mut out = Matrix::zeros(p, p);
        for i in 0..p {
            for j in 0..=i {
                // (R⁻¹ R⁻ᵀ)ᵢⱼ = Σₖ R⁻¹ᵢₖ R⁻¹ⱼₖ, with k ≥ max(i, j) = i.
                let ri: Vec<f64> = (i..p).map(|k| rinv[(i, k)]).collect();
                let rj: Vec<f64> = (i..p).map(|k| rinv[(j, k)]).collect();
                let v = crate::summation::dot(&ri, &rj);
                let (oi, oj) = (self.perm[i], self.perm[j]);
                out[(oi, oj)] = v;
                out[(oj, oi)] = v;
            }
        }
        Ok(out)
    }

    fn check_len(&self, len: usize) -> Result<()> {
        if len == self.nrows() {
            Ok(())
        } else {
            Err(StatError::DimensionMismatch {
                context: "vector length vs. QR rows",
                expected: self.nrows(),
                found: len,
            })
        }
    }
}

/// Apply `H = I − τ v vᵀ`, with `v = [1, tail…]`, to `x` (length `1 + tail.len()`).
#[inline]
fn reflect(tail: &[f64], tau: f64, x: &mut [f64]) {
    if tau == 0.0 {
        return;
    }
    let (head, rest) = x.split_at_mut(1);
    let w = head[0] + crate::summation::dot(tail, rest);
    let tw = tau * w;
    head[0] -= tw;
    for (xi, &vi) in rest.iter_mut().zip(tail) {
        *xi -= tw * vi;
    }
}

fn swap_columns(a: &mut Matrix, i: usize, j: usize) {
    let n = a.nrows();
    for r in 0..n {
        let t = a[(r, i)];
        a[(r, i)] = a[(r, j)];
        a[(r, j)] = t;
    }
}

/// Borrow column `k` immutably and column `j > k` mutably at the same time.
fn split_two_columns(a: &mut Matrix, k: usize, j: usize) -> (&[f64], &mut [f64]) {
    debug_assert!(k < j);
    let n = a.nrows();
    let (left, right) = a.as_mut_slice().split_at_mut(j * n);
    (&left[k * n..(k + 1) * n], &mut right[..n])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn max_abs_diff(a: &Matrix, b: &Matrix) -> f64 {
        a.as_slice().iter().zip(b.as_slice()).map(|(x, y)| (x - y).abs()).fold(0.0, f64::max)
    }

    fn sample() -> Matrix {
        Matrix::from_rows(&[[1.0, 2.0, 3.0], [4.0, 5.0, 6.5], [7.0, 8.0, 10.0], [1.0, -1.0, 2.0], [0.5, 3.0, -4.0]])
            .unwrap()
    }

    #[test]
    fn reconstructs_a_times_p() {
        let a = sample();
        let qr = QrDecomposition::new(a.clone()).unwrap();
        let qr_prod = qr.thin_q().matmul(&qr.r()).unwrap();
        let mut ap = Matrix::zeros(a.nrows(), a.ncols());
        for (k, &orig) in qr.permutation().iter().enumerate() {
            ap.col_mut(k).copy_from_slice(a.col(orig));
        }
        assert!(max_abs_diff(&qr_prod, &ap) < 1e-13);
    }

    #[test]
    fn q_is_orthonormal_and_diagonal_is_ordered() {
        let qr = QrDecomposition::new(sample()).unwrap();
        let q = qr.thin_q();
        let qtq = q.transpose().matmul(&q).unwrap();
        assert!(max_abs_diff(&qtq, &Matrix::identity(3)) < 1e-14);
        let d = qr.r_diagonal();
        assert!(d.windows(2).all(|w| w[0].abs() >= w[1].abs()));
    }

    #[test]
    fn apply_q_inverts_apply_qt() {
        let qr = QrDecomposition::new(sample()).unwrap();
        let b = vec![1.0, -2.0, 3.0, 0.25, 9.0];
        let mut c = b.clone();
        qr.apply_qt(&mut c).unwrap();
        qr.apply_q(&mut c).unwrap();
        for (x, y) in b.iter().zip(&c) {
            assert!((x - y).abs() < 1e-14);
        }
    }

    #[test]
    fn solves_consistent_system_exactly() {
        let a = sample();
        let x_true = [2.0, -1.0, 0.5];
        let b = a.mul_vec(&x_true).unwrap();
        let x = QrDecomposition::new(a).unwrap().solve_least_squares(&b).unwrap();
        for (u, v) in x.iter().zip(x_true) {
            assert!((u - v).abs() < 1e-13, "{u} vs {v}");
        }
    }

    #[test]
    fn least_squares_residual_is_orthogonal_to_columns() {
        let a = sample();
        let b = [1.0, 0.0, 2.0, -1.0, 3.0];
        let x = QrDecomposition::new(a.clone()).unwrap().solve_least_squares(&b).unwrap();
        let fitted = a.mul_vec(&x).unwrap();
        let r: Vec<f64> = b.iter().zip(&fitted).map(|(y, f)| y - f).collect();
        for j in 0..a.ncols() {
            assert!(crate::summation::dot(a.col(j), &r).abs() < 1e-13);
        }
    }

    #[test]
    fn inverse_gram_matches_definition() {
        let a = sample();
        let qr = QrDecomposition::new(a.clone()).unwrap();
        let inv = qr.inverse_gram().unwrap();
        let gram = a.transpose().matmul(&a).unwrap();
        let prod = gram.matmul(&inv).unwrap();
        assert!(max_abs_diff(&prod, &Matrix::identity(3)) < 1e-12);
    }

    #[test]
    fn quadratic_form_via_r_transpose() {
        let a = sample();
        let qr = QrDecomposition::new(a).unwrap();
        let u = [0.3, -1.2, 2.0];
        let w = qr.solve_r_transpose(&u).unwrap();
        let via_w = crate::summation::dot(&w, &w);
        let inv = qr.inverse_gram().unwrap();
        let direct = crate::summation::dot(&u, &inv.mul_vec(&u).unwrap());
        assert!(((via_w - direct) / direct).abs() < 1e-12);
    }

    #[test]
    fn detects_rank_deficiency() {
        // Third column = first + second.
        let a = Matrix::from_rows(&[[1.0, 2.0, 3.0], [2.0, 0.0, 2.0], [0.0, 1.0, 1.0], [3.0, 1.0, 4.0]]).unwrap();
        let qr = QrDecomposition::new(a).unwrap();
        assert_eq!(qr.rank(qr.default_rank_tolerance() * 10.0), 2);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(matches!(QrDecomposition::new(Matrix::zeros(2, 3)), Err(StatError::InsufficientData { .. })));
        let mut a = Matrix::zeros(3, 1);
        a[(1, 0)] = f64::NAN;
        assert!(matches!(QrDecomposition::new(a), Err(StatError::NonFiniteInput { .. })));
        let qr = QrDecomposition::new(sample()).unwrap();
        assert!(qr.solve_least_squares(&[1.0]).is_err());
    }

    #[test]
    fn norm2_is_scaled() {
        assert_eq!(norm2(&[3.0, 4.0]), 5.0);
        assert_eq!(norm2(&[3e300, 4e300]), 5e300);
        assert_eq!(norm2(&[]), 0.0);
    }
}
