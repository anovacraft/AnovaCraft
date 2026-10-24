//! Functional principal component analysis.
//!
//! For curves `xᵢ = Σⱼ cᵢⱼ φⱼ` with mean `x̄`, the sample covariance operator
//! `v(s, t) = N⁻¹ Σᵢ (xᵢ(s) − x̄(s))(xᵢ(t) − x̄(t))` has eigenfunctions `ξₖ`
//! solving `∫ v(s, t) ξ(t) dt = ρ ξ(s)`. In a basis with centred
//! coefficient matrix `C` (rows `cᵢ − c̄`), Gram matrix `W = LLᵀ` and
//! `V = N⁻¹CᵀC`, an eigenfunction `ξ = φᵀb` satisfies `VWb = ρb`; with
//! `u = Lᵀb` this is the symmetric problem `LᵀVL u = ρu`, so
//! `b = L⁻ᵀu` and the eigenfunctions are orthonormal, `∫ ξₖ ξₗ = δₖₗ`
//! (Ramsay & Silverman 2005, §8.4.2; the computation of `pca.fd` in the
//! fda package). The divisor is `N`, as there.
//!
//! The **scores** are `sᵢₖ = ∫ (xᵢ − x̄) ξₖ dt = (cᵢ − c̄)ᵀWbₖ`; the
//! eigenvalue `ρₖ` is the variance (divisor `N`) of the k-th scores and
//! `ρₖ / Σρ` the proportion of total variation `Σᵢ ∫(xᵢ − x̄)²/N` it
//! explains. Truncating at `K` components gives the reconstruction
//! `x̂ᵢ = x̄ + Σₖ sᵢₖ ξₖ`, the best `K`-term approximation in squared
//! integrated error.
//!
//! Eigenfunction signs are arbitrary; each is oriented so that its
//! integral is positive (or, if the integral is nearly zero, so that its
//! largest-magnitude coefficient is positive).

use crate::fdata::FunctionalData;
use crate::linalg::{backward_transpose, dot, mat_vec};
use stat_core::eigen::{cholesky, symmetric_eigen};
use stat_core::{Result, StatError};

/// A functional principal component analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct Fpca {
    /// Coefficients of the mean curve.
    pub mean: Vec<f64>,
    /// Every eigenvalue, largest first (`n_basis` of them; those beyond
    /// `N − 1` are zero up to rounding).
    pub eigenvalues: Vec<f64>,
    /// Coefficients of the retained eigenfunctions.
    pub harmonics: Vec<Vec<f64>>,
    /// Scores: `scores[i][k]` for curve `i`, component `k`.
    pub scores: Vec<Vec<f64>>,
}

impl Fpca {
    /// Number of retained components.
    #[must_use]
    pub fn n_components(&self) -> usize {
        self.harmonics.len()
    }

    /// Total variation `Σ ρₖ`.
    #[must_use]
    pub fn total_variance(&self) -> f64 {
        self.eigenvalues.iter().map(|v| v.max(0.0)).sum()
    }

    /// Proportion of total variation explained by component `k`.
    #[must_use]
    pub fn proportion(&self, k: usize) -> f64 {
        let total = self.total_variance();
        if total > 0.0 { self.eigenvalues[k].max(0.0) / total } else { 0.0 }
    }

    /// Coefficients of `x̄ + Σₖ<K sₖ ξₖ` for the given scores (the first
    /// `scores.len()` components).
    #[must_use]
    pub fn curve_from_scores(&self, scores: &[f64]) -> Vec<f64> {
        let mut c = self.mean.clone();
        for (s, h) in scores.iter().zip(&self.harmonics) {
            for (cj, hj) in c.iter_mut().zip(h) {
                *cj += s * hj;
            }
        }
        c
    }

    /// Coefficients of curve `i` reconstructed from its first `k` scores.
    #[must_use]
    pub fn reconstruct(&self, i: usize, k: usize) -> Vec<f64> {
        let k = k.min(self.n_components());
        self.curve_from_scores(&self.scores[i][..k])
    }
}

/// Principal components of the curves in `data`, keeping `n_components`.
///
/// # Errors
/// Fewer than two curves, `n_components` zero or more than
/// `min(N − 1, n_basis)`, or a singular Gram matrix.
pub fn functional_pca(data: &FunctionalData, n_components: usize) -> Result<Fpca> {
    let n = data.n_curves();
    let k = data.basis.n_basis();
    if n < 2 {
        return Err(StatError::InsufficientData { required: 2, found: n });
    }
    let most = (n - 1).min(k);
    if n_components == 0 || n_components > most {
        return Err(StatError::InvalidArgument {
            message: format!("choose between 1 and {most} components for {n} curves in {k} basis functions"),
        });
    }
    let mean = data.mean_coefs();
    let centred: Vec<Vec<f64>> = data.coefs.iter().map(|c| c.iter().zip(&mean).map(|(x, m)| x - m).collect()).collect();
    let mut v = vec![vec![0.0; k]; k];
    for c in &centred {
        for j in 0..k {
            for l in j..k {
                v[j][l] += c[j] * c[l] / n as f64;
            }
        }
    }
    for j in 0..k {
        for l in 0..j {
            v[j][l] = v[l][j];
        }
    }
    let w = data.basis.gram();
    let l = cholesky(&w)
        .ok_or_else(|| StatError::InvalidArgument { message: "the basis Gram matrix is singular".into() })?;
    // A = Lᵀ V L.
    let vl: Vec<Vec<f64>> = (0..k).map(|i| (0..k).map(|j| (0..k).map(|m| v[i][m] * l[m][j]).sum()).collect()).collect();
    let a: Vec<Vec<f64>> = (0..k).map(|i| (0..k).map(|j| (0..k).map(|m| l[m][i] * vl[m][j]).sum()).collect()).collect();
    let (eigenvalues, vectors) = symmetric_eigen(&a)?;
    let integrals = data.basis.integrals();
    let harmonics: Vec<Vec<f64>> = vectors
        .iter()
        .take(n_components)
        .map(|u| {
            let mut b = backward_transpose(&l, u);
            let area = dot(&b, &integrals);
            let size = mat_vec(&w, &b).iter().zip(&b).map(|(x, y)| x * y).sum::<f64>().sqrt();
            let range = data.basis.range();
            let flip = if area.abs() > 1e-8 * size * (range.1 - range.0).sqrt() {
                area < 0.0
            } else {
                let big = b.iter().copied().fold(0.0_f64, |m, x| if x.abs() > m.abs() { x } else { m });
                big < 0.0
            };
            if flip {
                b.iter_mut().for_each(|x| *x = -*x);
            }
            b
        })
        .collect();
    let wb: Vec<Vec<f64>> = harmonics.iter().map(|b| mat_vec(&w, b)).collect();
    let scores = centred.iter().map(|c| wb.iter().map(|x| dot(c, x)).collect()).collect();
    Ok(Fpca { mean, eigenvalues, harmonics, scores })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::basis::{Basis, FourierBasis};

    #[test]
    fn two_known_modes_are_recovered() {
        // xᵢ = aᵢ φ₁ + bᵢ φ₂ in an orthonormal Fourier basis: the
        // eigenfunctions are φ₁ and φ₂ and the scores the centred a, b.
        let basis = Basis::Fourier(FourierBasis::new((0.0, 1.0), 5, 1.0).unwrap());
        let a = [3.0, -1.0, 2.0, -4.0, 0.0];
        let b = [0.1, 0.1, 0.1, 0.1, -0.4];
        let coefs = a.iter().zip(&b).map(|(x, y)| vec![0.0, 0.0, *x, *y, 0.0]).collect();
        let fd = FunctionalData { basis, coefs };
        let p = functional_pca(&fd, 2).unwrap();
        let ma = a.iter().sum::<f64>() / 5.0;
        let va = a.iter().map(|x| (x - ma).powi(2)).sum::<f64>() / 5.0;
        assert!((p.eigenvalues[0] - va).abs() < 1e-10);
        assert!((p.harmonics[0][2].abs() - 1.0).abs() < 1e-10);
        for (i, ai) in a.iter().enumerate() {
            assert!((p.scores[i][0].abs() - (ai - ma).abs()).abs() < 1e-10);
            let rec = p.reconstruct(i, 2);
            assert!((rec[2] - ai).abs() < 1e-10 && (rec[3] - b[i]).abs() < 1e-10);
        }
        assert!((p.proportion(0) + p.proportion(1) - 1.0).abs() < 1e-10);
    }
}
