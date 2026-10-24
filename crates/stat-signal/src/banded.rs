//! Symmetric positive-definite banded systems, for the Whittaker smoother
//! `(W + λ DᵀD) z = W y` behind the penalized baselines.

use crate::{Result, invalid};

/// The lower band of `λ DᵀD`, `D` the `(n − d) × n` matrix of `d`-th
/// differences: `band[i][k] = (λ DᵀD)[i][i − k]`, `k = 0 … d`.
pub(crate) fn difference_penalty(n: usize, order: usize, lambda: f64) -> Vec<Vec<f64>> {
    // Coefficients of the d-th difference: (−1)^(d−j) C(d, j).
    let mut coef = vec![1.0];
    for _ in 0..order {
        let mut next = vec![0.0; coef.len() + 1];
        for (j, c) in coef.iter().enumerate() {
            next[j] -= c;
            next[j + 1] += c;
        }
        coef = next;
    }
    let mut band = vec![vec![0.0; order + 1]; n];
    if n <= order {
        return band;
    }
    for r in 0..n - order {
        for a in 0..=order {
            for b in 0..=a {
                band[r + a][a - b] += lambda * coef[a] * coef[b];
            }
        }
    }
    band
}

/// Solve `(diag(w) + P) z = rhs` for the banded penalty `P` (lower band as
/// from [`difference_penalty`]) by banded Cholesky factorization.
pub(crate) fn solve_with_diagonal(penalty: &[Vec<f64>], w: &[f64], rhs: &[f64]) -> Result<Vec<f64>> {
    let n = rhs.len();
    let b = penalty.first().map_or(0, |r| r.len() - 1);
    // l[i][k] = L[i][i − k].
    let mut l: Vec<Vec<f64>> = penalty.to_vec();
    for i in 0..n {
        l[i][0] += w[i];
    }
    for i in 0..n {
        for k in (0..=b.min(i)).rev() {
            let j = i - k;
            let mut s = l[i][k];
            // Σ_{m < j, |i−m| ≤ b, |j−m| ≤ b} L[i][m] L[j][m]
            let lo = i.saturating_sub(b);
            for m in lo..j {
                s -= l[i][i - m] * l[j][j - m];
            }
            if k == 0 {
                if s <= 0.0 || !s.is_finite() {
                    return Err(invalid("the baseline system is not positive definite (check λ and the weights)"));
                }
                l[i][0] = s.sqrt();
            } else {
                l[i][k] = s / l[j][0];
            }
        }
    }
    // Forward then backward substitution.
    let mut z = rhs.to_vec();
    for i in 0..n {
        let mut s = z[i];
        for k in 1..=b.min(i) {
            s -= l[i][k] * z[i - k];
        }
        z[i] = s / l[i][0];
    }
    for i in (0..n).rev() {
        let mut s = z[i];
        for k in 1..=b.min(n - 1 - i) {
            s -= l[i + k][k] * z[i + k];
        }
        z[i] = s / l[i][0];
    }
    Ok(z)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banded_solve_matches_dense() {
        let n = 7;
        let p = difference_penalty(n, 2, 3.0);
        let w = [1.0, 0.5, 2.0, 1.0, 0.1, 1.0, 3.0];
        let rhs = [1.0, -2.0, 0.5, 4.0, 1.0, 0.0, 2.0];
        let z = solve_with_diagonal(&p, &w, &rhs).unwrap();
        // Rebuild the dense matrix and check A z = rhs.
        let mut a = vec![vec![0.0; n]; n];
        for i in 0..n {
            a[i][i] += w[i];
            for k in 0..3 {
                if k <= i {
                    a[i][i - k] += p[i][k];
                    if k > 0 {
                        a[i - k][i] += p[i][k];
                    }
                }
            }
        }
        for i in 0..n {
            let s: f64 = (0..n).map(|j| a[i][j] * z[j]).sum();
            assert!((s - rhs[i]).abs() < 1e-12);
        }
        // Second differences: corner entries 1, −2, 1 pattern.
        assert!((p[0][0] - 3.0).abs() < 1e-15 && (p[1][1] + 6.0).abs() < 1e-15 && (p[2][0] - 18.0).abs() < 1e-15);
    }
}
