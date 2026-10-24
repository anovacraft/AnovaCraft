//! Eigen-decomposition of a real symmetric matrix by Householder reduction
//! to tridiagonal form followed by the implicit QL algorithm (Wilkinson &
//! Reinsch 1971, *Handbook for Automatic Computation* vol. II, procedures
//! `tred2` and `tql2`; the formulation follows the public-domain JAMA
//! translation). It costs about `(4/3 + 3) n³` flops regardless of the
//! spectrum, so classical scaling of a few thousand objects stays quick,
//! where cyclic Jacobi sweeps would take many times longer.

use stat_core::{Result, StatError};

/// Eigenvalues (largest first) and eigenvectors of the symmetric `a`
/// (`n × n`, row vectors); `vectors[i][k]` is component `i` of the
/// eigenvector of `values[k]`, and the eigenvectors are orthonormal.
///
/// # Errors
/// [`StatError::NonFiniteInput`] for NaN or ±∞, [`StatError::InvalidArgument`]
/// for an empty or non-square matrix, [`StatError::ConvergenceFailure`] if
/// the QL iterations do not converge (not expected for finite input).
pub(crate) fn symmetric_eigen(a: &[Vec<f64>]) -> Result<(Vec<f64>, Vec<Vec<f64>>)> {
    let n = a.len();
    if n == 0 || a.iter().any(|r| r.len() != n) {
        return Err(StatError::InvalidArgument { message: "the eigen-decomposition needs a square matrix".into() });
    }
    if a.iter().flatten().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "eigen-decomposition input" });
    }
    // Use the symmetric part (guards against rounding asymmetry).
    let mut v: Vec<Vec<f64>> = (0..n).map(|i| (0..n).map(|j| 0.5 * (a[i][j] + a[j][i])).collect()).collect();
    let mut d = vec![0.0; n];
    let mut e = vec![0.0; n];
    tred2(&mut v, &mut d, &mut e);
    tql2(&mut v, &mut d, &mut e)?;
    // Sort largest first.
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&x, &y| d[y].total_cmp(&d[x]));
    let values = order.iter().map(|&k| d[k]).collect();
    let vectors = (0..n).map(|i| order.iter().map(|&k| v[i][k]).collect()).collect();
    Ok((values, vectors))
}

/// Householder reduction to tridiagonal form; `v` becomes the accumulated
/// orthogonal transformation, `d` the diagonal, `e` the subdiagonal.
#[allow(clippy::many_single_char_names)]
fn tred2(v: &mut [Vec<f64>], d: &mut [f64], e: &mut [f64]) {
    let n = d.len();
    d[..n].copy_from_slice(&v[n - 1][..n]);
    for i in (1..n).rev() {
        let mut scale = 0.0;
        let mut h = 0.0;
        for k in 0..i {
            scale += d[k].abs();
        }
        if scale == 0.0 {
            e[i] = d[i - 1];
            for j in 0..i {
                d[j] = v[i - 1][j];
                v[i][j] = 0.0;
                v[j][i] = 0.0;
            }
        } else {
            for k in 0..i {
                d[k] /= scale;
                h += d[k] * d[k];
            }
            let mut f = d[i - 1];
            let mut g = h.sqrt();
            if f > 0.0 {
                g = -g;
            }
            e[i] = scale * g;
            h -= f * g;
            d[i - 1] = f - g;
            for item in e.iter_mut().take(i) {
                *item = 0.0;
            }
            for j in 0..i {
                f = d[j];
                v[j][i] = f;
                g = e[j] + v[j][j] * f;
                for k in j + 1..i {
                    g += v[k][j] * d[k];
                    e[k] += v[k][j] * f;
                }
                e[j] = g;
            }
            f = 0.0;
            for j in 0..i {
                e[j] /= h;
                f += e[j] * d[j];
            }
            let hh = f / (h + h);
            for j in 0..i {
                e[j] -= hh * d[j];
            }
            for j in 0..i {
                f = d[j];
                g = e[j];
                for k in j..i {
                    v[k][j] -= f * e[k] + g * d[k];
                }
                d[j] = v[i - 1][j];
                v[i][j] = 0.0;
            }
        }
        d[i] = h;
    }
    for i in 0..n.saturating_sub(1) {
        v[n - 1][i] = v[i][i];
        v[i][i] = 1.0;
        let h = d[i + 1];
        if h != 0.0 {
            for k in 0..=i {
                d[k] = v[k][i + 1] / h;
            }
            for j in 0..=i {
                let mut g = 0.0;
                for k in 0..=i {
                    g += v[k][i + 1] * v[k][j];
                }
                for k in 0..=i {
                    v[k][j] -= g * d[k];
                }
            }
        }
        for k in 0..=i {
            v[k][i + 1] = 0.0;
        }
    }
    for j in 0..n {
        d[j] = v[n - 1][j];
        v[n - 1][j] = 0.0;
    }
    v[n - 1][n - 1] = 1.0;
    e[0] = 0.0;
}

/// Implicit QL iterations on the tridiagonal (`d`, `e`), accumulating into `v`.
#[allow(clippy::many_single_char_names)]
fn tql2(v: &mut [Vec<f64>], d: &mut [f64], e: &mut [f64]) -> Result<()> {
    let n = d.len();
    for i in 1..n {
        e[i - 1] = e[i];
    }
    e[n - 1] = 0.0;
    let mut f = 0.0;
    let mut tst1 = 0.0_f64;
    let eps = f64::EPSILON;
    for l in 0..n {
        tst1 = tst1.max(d[l].abs() + e[l].abs());
        let mut m = l;
        while m < n {
            if e[m].abs() <= eps * tst1 {
                break;
            }
            m += 1;
        }
        let m = m.min(n - 1);
        if m > l {
            let mut iter = 0;
            loop {
                iter += 1;
                if iter > 60 {
                    return Err(StatError::ConvergenceFailure {
                        routine: "symmetric eigen-decomposition",
                        iterations: iter,
                    });
                }
                let mut g = d[l];
                let mut p = (d[l + 1] - g) / (2.0 * e[l]);
                let mut r = p.hypot(1.0);
                if p < 0.0 {
                    r = -r;
                }
                d[l] = e[l] / (p + r);
                d[l + 1] = e[l] * (p + r);
                let dl1 = d[l + 1];
                let mut h = g - d[l];
                for item in d.iter_mut().take(n).skip(l + 2) {
                    *item -= h;
                }
                f += h;
                p = d[m];
                let mut c = 1.0;
                let mut c2 = c;
                let mut c3 = c;
                let el1 = e[l + 1];
                let mut s = 0.0;
                let mut s2 = 0.0;
                for i in (l..m).rev() {
                    c3 = c2;
                    c2 = c;
                    s2 = s;
                    g = c * e[i];
                    h = c * p;
                    r = p.hypot(e[i]);
                    e[i + 1] = s * r;
                    s = e[i] / r;
                    c = p / r;
                    p = c * d[i] - s * g;
                    d[i + 1] = h + s * (c * g + s * d[i]);
                    for row in v.iter_mut() {
                        h = row[i + 1];
                        row[i + 1] = s * row[i] + c * h;
                        row[i] = c * row[i] - s * h;
                    }
                }
                p = -s * s2 * c3 * el1 * e[l] / dl1;
                e[l] = s * p;
                d[l] = c * p;
                if e[l].abs() <= eps * tst1 {
                    break;
                }
            }
        }
        d[l] += f;
        e[l] = 0.0;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconstructs_a_symmetric_matrix() {
        // A 5 × 5 symmetric matrix with a repeated and a negative eigenvalue.
        let a = vec![
            vec![4.0, 1.0, -2.0, 0.5, 0.0],
            vec![1.0, 3.0, 0.0, 1.0, -1.0],
            vec![-2.0, 0.0, -1.0, 0.0, 2.0],
            vec![0.5, 1.0, 0.0, 2.0, 0.3],
            vec![0.0, -1.0, 2.0, 0.3, 1.0],
        ];
        let (values, vectors) = symmetric_eigen(&a).unwrap();
        assert!(values.windows(2).all(|w| w[0] >= w[1]));
        // Trace is the sum of the eigenvalues.
        assert!((values.iter().sum::<f64>() - 9.0).abs() < 1e-12);
        for i in 0..5 {
            for j in 0..5 {
                let rebuilt: f64 = (0..5).map(|k| vectors[i][k] * values[k] * vectors[j][k]).sum();
                assert!((rebuilt - a[i][j]).abs() < 1e-12, "A[{i}][{j}]");
                let dot: f64 = (0..5).map(|k| vectors[k][i] * vectors[k][j]).sum();
                assert!((dot - f64::from(u8::from(i == j))).abs() < 1e-12);
            }
        }
        // Diagonal and 1 × 1 matrices.
        let (v, _) = symmetric_eigen(&[vec![2.0, 0.0], vec![0.0, 5.0]]).unwrap();
        assert!((v[0] - 5.0).abs() < 1e-15 && (v[1] - 2.0).abs() < 1e-15);
        assert_eq!(symmetric_eigen(&[vec![3.0]]).unwrap().0, [3.0]);
        assert!(symmetric_eigen(&[vec![1.0, 2.0]]).is_err());
    }
}
