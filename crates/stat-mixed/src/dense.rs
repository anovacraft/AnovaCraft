//! Small dense linear algebra on square matrices stored row-major: Cholesky
//! factorisation, triangular solves, inverses and a symmetric eigensolver.

/// A square matrix, row-major.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Square {
    pub n: usize,
    pub a: Vec<f64>,
}

impl Square {
    pub fn zeros(n: usize) -> Self {
        Self { n, a: vec![0.0; n * n] }
    }

    pub fn identity(n: usize) -> Self {
        let mut s = Self::zeros(n);
        for i in 0..n {
            s.a[i * n + i] = 1.0;
        }
        s
    }

    #[inline]
    pub fn get(&self, i: usize, j: usize) -> f64 {
        self.a[i * self.n + j]
    }

    #[inline]
    pub fn at(&mut self, i: usize, j: usize) -> &mut f64 {
        &mut self.a[i * self.n + j]
    }

    /// The rows and columns in `idx`.
    pub fn sub(&self, idx: &[usize]) -> Self {
        let m = idx.len();
        let mut s = Self::zeros(m);
        for (a, &i) in idx.iter().enumerate() {
            for (b, &j) in idx.iter().enumerate() {
                s.a[a * m + b] = self.get(i, j);
            }
        }
        s
    }
}

/// Lower Cholesky factor `L` (`A = L Lᵀ`) of a symmetric matrix (only the
/// lower triangle is read). `None` unless positive definite.
pub(crate) fn cholesky(m: &Square) -> Option<Square> {
    let n = m.n;
    let mut l = Square::zeros(n);
    for j in 0..n {
        let mut d = m.get(j, j);
        for k in 0..j {
            d -= l.get(j, k) * l.get(j, k);
        }
        if d.is_nan() || d <= 0.0 || !d.is_finite() {
            return None;
        }
        let d = d.sqrt();
        *l.at(j, j) = d;
        for i in j + 1..n {
            let mut s = m.get(i, j);
            for k in 0..j {
                s -= l.get(i, k) * l.get(j, k);
            }
            *l.at(i, j) = s / d;
        }
    }
    Some(l)
}

/// Solve `L x = b` in place (`L` lower triangular).
pub(crate) fn forward(l: &Square, b: &mut [f64]) {
    for i in 0..l.n {
        let mut s = b[i];
        for k in 0..i {
            s -= l.get(i, k) * b[k];
        }
        b[i] = s / l.get(i, i);
    }
}

/// Solve `Lᵀ x = b` in place.
pub(crate) fn backward(l: &Square, b: &mut [f64]) {
    for i in (0..l.n).rev() {
        let mut s = b[i];
        for k in i + 1..l.n {
            s -= l.get(k, i) * b[k];
        }
        b[i] = s / l.get(i, i);
    }
}

/// `log det A = 2 Σ log Lᵢᵢ` over the diagonal entries `range`.
pub(crate) fn log_det(l: &Square, range: std::ops::Range<usize>) -> f64 {
    range.map(|i| 2.0 * l.get(i, i).ln()).sum()
}

/// `A⁻¹` from the Cholesky factor of `A`.
pub(crate) fn inverse_from_cholesky(l: &Square) -> Square {
    let n = l.n;
    let mut inv = Square::zeros(n);
    let mut e = vec![0.0; n];
    for j in 0..n {
        e.iter_mut().for_each(|v| *v = 0.0);
        e[j] = 1.0;
        forward(l, &mut e);
        backward(l, &mut e);
        for i in 0..n {
            inv.a[i * n + j] = e[i];
        }
    }
    inv
}

/// Inverse of a symmetric positive definite matrix.
pub(crate) fn spd_inverse(m: &Square) -> Option<Square> {
    cholesky(m).map(|l| inverse_from_cholesky(&l))
}

/// Eigenvalues and eigenvectors (columns of the returned matrix) of a
/// symmetric matrix by cyclic Jacobi rotations.
pub(crate) fn symmetric_eigen(m: &Square) -> (Vec<f64>, Square) {
    let n = m.n;
    let mut a = m.clone();
    let mut v = Square::identity(n);
    for _ in 0..100 {
        let off: f64 = (0..n)
            .flat_map(|i| (0..n).filter(move |&j| j != i).map(move |j| (i, j)))
            .map(|(i, j)| a.get(i, j).powi(2))
            .sum();
        let scale: f64 = a.a.iter().map(|x| x * x).sum::<f64>().max(f64::MIN_POSITIVE);
        if off <= 1e-30 * scale {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                let apq = a.get(p, q);
                if apq == 0.0 {
                    continue;
                }
                let theta = (a.get(q, q) - a.get(p, p)) / (2.0 * apq);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let t = if theta == 0.0 { 1.0 } else { t };
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for k in 0..n {
                    let (akp, akq) = (a.get(k, p), a.get(k, q));
                    *a.at(k, p) = c * akp - s * akq;
                    *a.at(k, q) = s * akp + c * akq;
                }
                for k in 0..n {
                    let (apk, aqk) = (a.get(p, k), a.get(q, k));
                    *a.at(p, k) = c * apk - s * aqk;
                    *a.at(q, k) = s * apk + c * aqk;
                }
                for k in 0..n {
                    let (vkp, vkq) = (v.get(k, p), v.get(k, q));
                    *v.at(k, p) = c * vkp - s * vkq;
                    *v.at(k, q) = s * vkp + c * vkq;
                }
            }
        }
    }
    ((0..n).map(|i| a.get(i, i)).collect(), v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spd() -> Square {
        Square { n: 3, a: vec![4.0, 2.0, 0.6, 2.0, 5.0, 1.0, 0.6, 1.0, 3.0] }
    }

    #[test]
    fn cholesky_solves_and_inverts() {
        let m = spd();
        let l = cholesky(&m).unwrap();
        let mut b = vec![1.0, -2.0, 0.5];
        forward(&l, &mut b);
        backward(&l, &mut b);
        for i in 0..3 {
            let r: f64 = (0..3).map(|j| m.get(i, j) * b[j]).sum();
            assert!((r - [1.0, -2.0, 0.5][i]).abs() < 1e-12);
        }
        let inv = spd_inverse(&m).unwrap();
        for i in 0..3 {
            for j in 0..3 {
                let r: f64 = (0..3).map(|k| m.get(i, k) * inv.get(k, j)).sum();
                assert!((r - f64::from(u8::from(i == j))).abs() < 1e-12);
            }
        }
        // det = 4(15 − 1) − 2(6 − 0.6) + 0.6(2 − 3) = 56 − 10.8 − 0.6
        assert!((log_det(&l, 0..3) - 44.6f64.ln()).abs() < 1e-12);
        assert!(cholesky(&Square { n: 2, a: vec![1.0, 2.0, 2.0, 1.0] }).is_none());
        assert_eq!(spd().sub(&[2, 0]).a, vec![3.0, 0.6, 0.6, 4.0]);
    }

    #[test]
    fn eigen_decomposition_reconstructs() {
        let m = spd();
        let (vals, vecs) = symmetric_eigen(&m);
        for i in 0..3 {
            for j in 0..3 {
                let r: f64 = (0..3).map(|k| vecs.get(i, k) * vals[k] * vecs.get(j, k)).sum();
                assert!((r - m.get(i, j)).abs() < 1e-10);
            }
        }
        assert!((vals.iter().sum::<f64>() - 12.0).abs() < 1e-10);
    }
}
