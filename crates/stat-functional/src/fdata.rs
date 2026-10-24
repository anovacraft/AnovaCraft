//! A sample of curves stored as coefficients in a common basis.

use crate::basis::Basis;
use crate::linalg::{dot, linspace, quad_form, spd_inverse};
use stat_core::{Result, StatError};

/// Curves `xᵢ(t) = Σⱼ cᵢⱼ φⱼ(t)` sharing one basis.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionalData {
    /// The basis system.
    pub basis: Basis,
    /// One coefficient vector per curve, each of `basis.n_basis()` values.
    pub coefs: Vec<Vec<f64>>,
}

impl FunctionalData {
    /// Number of curves.
    #[must_use]
    pub fn n_curves(&self) -> usize {
        self.coefs.len()
    }

    /// Derivative `deriv` of curve `i` at `t`.
    #[must_use]
    pub fn eval(&self, i: usize, t: f64, deriv: usize) -> f64 {
        dot(&self.coefs[i], &self.basis.evaluate(t, deriv))
    }

    /// Derivative `deriv` of the curve with coefficients `coefs` at each of
    /// `times`.
    #[must_use]
    pub fn eval_coefs(&self, coefs: &[f64], times: &[f64], deriv: usize) -> Vec<f64> {
        times.iter().map(|&t| dot(coefs, &self.basis.evaluate(t, deriv))).collect()
    }

    /// Every curve at each of `times` (`out[i][g]`).
    #[must_use]
    pub fn eval_all(&self, times: &[f64], deriv: usize) -> Vec<Vec<f64>> {
        let phi: Vec<Vec<f64>> = times.iter().map(|&t| self.basis.evaluate(t, deriv)).collect();
        self.coefs.iter().map(|c| phi.iter().map(|p| dot(c, p)).collect()).collect()
    }

    /// Coefficients of the mean curve `x̄(t) = N⁻¹ Σᵢ xᵢ(t)`.
    #[must_use]
    pub fn mean_coefs(&self) -> Vec<f64> {
        let n = self.n_curves().max(1) as f64;
        let mut m = vec![0.0; self.basis.n_basis()];
        for c in &self.coefs {
            for (mj, cj) in m.iter_mut().zip(c) {
                *mj += cj / n;
            }
        }
        m
    }

    /// Pointwise standard deviation across curves at each of `times`
    /// (divisor `N − 1`; zeros for a single curve).
    #[must_use]
    pub fn std_dev(&self, times: &[f64]) -> Vec<f64> {
        let values = self.eval_all(times, 0);
        let n = values.len();
        (0..times.len())
            .map(|g| {
                if n < 2 {
                    return 0.0;
                }
                let m = values.iter().map(|v| v[g]).sum::<f64>() / n as f64;
                (values.iter().map(|v| (v[g] - m).powi(2)).sum::<f64>() / (n - 1) as f64).sqrt()
            })
            .collect()
    }

    /// `∫ xᵢ² dt` of the curve with these coefficients.
    #[must_use]
    pub fn squared_norm(&self, coefs: &[f64], gram: &[Vec<f64>]) -> f64 {
        quad_form(gram, coefs, coefs)
    }

    /// `n` equally spaced times spanning the basis range.
    #[must_use]
    pub fn grid(&self, n: usize) -> Vec<f64> {
        let (a, b) = self.basis.range();
        linspace(a, b, n)
    }

    /// Curves given by their values on `times` re-expressed in `basis` by
    /// least squares (the times must determine the coefficients: at least as
    /// many distinct times as basis functions).
    ///
    /// # Errors
    /// Mismatched lengths or too few times for the basis.
    pub fn from_values(basis: Basis, times: &[f64], values: &[Vec<f64>]) -> Result<Self> {
        if values.iter().any(|v| v.len() != times.len()) {
            return Err(StatError::InvalidArgument { message: "every curve needs a value at each time".into() });
        }
        let phi: Vec<Vec<f64>> = times.iter().map(|&t| basis.evaluate(t, 0)).collect();
        let k = basis.n_basis();
        let mut g = vec![vec![0.0; k]; k];
        for p in &phi {
            for j in 0..k {
                for l in 0..k {
                    g[j][l] += p[j] * p[l];
                }
            }
        }
        let inv = spd_inverse(&g).ok_or_else(|| StatError::InvalidArgument {
            message: "too few times to determine the basis coefficients".into(),
        })?;
        let coefs = values
            .iter()
            .map(|v| {
                let b: Vec<f64> = (0..k).map(|j| phi.iter().zip(v).map(|(p, y)| p[j] * y).sum()).collect();
                inv.iter().map(|r| dot(r, &b)).collect()
            })
            .collect();
        Ok(Self { basis, coefs })
    }
}
