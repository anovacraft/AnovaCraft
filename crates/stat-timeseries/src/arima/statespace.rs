//! State-space form of a stationary ARMA process and its Kalman filter.
//!
//! With `r = max(p, q + 1)`, `φⱼ = 0` for `j > p` and `θⱼ = 0` for `j > q`
//! (`θ₀ = 1`), the process `wₜ = Σ φⱼ w_{t−j} + εₜ + Σ θⱼ ε_{t−j}` has the
//! state equation (Harvey 1989, §3.4; Gardner, Harvey & Phillips 1980)
//!
//! ```text
//! αₜ₊₁ = T αₜ + R εₜ₊₁,   wₜ = αₜ[0],
//! T = [φ | I_{r−1} over 0],   R = (1, θ₁, …, θ_{r−1})ᵀ.
//! ```
//!
//! Solving the recursion gives `αₜ[i] = Σ_{k>i} φₖ w_{t+i−k} + Σ_{k≥i} θₖ ε_{t+i−k}`,
//! so the stationary state covariance `Q₀` (the solution of the Lyapunov
//! equation `Q₀ = T Q₀ Tᵀ + R Rᵀ`) follows from the autocovariances of `w` and
//! the cross-covariances `Cov(w_s, ε_u) = ψ_{s−u}`. All variances are in units
//! of σ², which the likelihood concentrates out.

/// ψ-weights `ψ₀ = 1, ψⱼ = θⱼ + Σ φᵢ ψ_{j−i}` for `j = 0..len`.
pub(crate) fn psi_weights(phi: &[f64], theta: &[f64], len: usize) -> Vec<f64> {
    let mut psi = vec![0.0; len];
    for j in 0..len {
        let mut v = if j == 0 { 1.0 } else { theta.get(j - 1).copied().unwrap_or(0.0) };
        for (i, &ph) in phi.iter().enumerate() {
            if i < j {
                v += ph * psi[j - 1 - i];
            }
        }
        psi[j] = v;
    }
    psi
}

/// Solve the dense system `A x = b` by Gaussian elimination with partial
/// pivoting; `None` when singular.
pub(crate) fn solve_dense(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for k in 0..n {
        let piv = (k..n).max_by(|&i, &j| a[i][k].abs().total_cmp(&a[j][k].abs()))?;
        if a[piv][k].abs() < 1e-300 || !a[piv][k].is_finite() {
            return None;
        }
        a.swap(k, piv);
        b.swap(k, piv);
        for i in k + 1..n {
            let f = a[i][k] / a[k][k];
            if f != 0.0 {
                for j in k..n {
                    a[i][j] -= f * a[k][j];
                }
                b[i] -= f * b[k];
            }
        }
    }
    let mut x = vec![0.0; n];
    for k in (0..n).rev() {
        let s: f64 = (k + 1..n).map(|j| a[k][j] * x[j]).sum();
        x[k] = (b[k] - s) / a[k][k];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// Autocovariances `γ(0..=max_lag)` of a stationary ARMA process with unit
/// innovation variance, from the linear system for `γ(0..p)` (Brockwell &
/// Davis 1991, §3.3, method 2) and the difference equation beyond.
/// `None` when the system is singular (a unit root).
pub(crate) fn arma_autocovariance(phi: &[f64], theta: &[f64], max_lag: usize) -> Option<Vec<f64>> {
    let p = phi.len();
    let q = theta.len();
    let psi = psi_weights(phi, theta, q + 1);
    // Right-hand side Σ_{j=k}^{q} θⱼ ψ_{j−k}, θ₀ = 1.
    let th = |j: usize| if j == 0 { 1.0 } else { theta.get(j - 1).copied().unwrap_or(0.0) };
    let rhs = |k: usize| (k..=q).map(|j| th(j) * psi[j - k]).sum::<f64>();
    let mut a = vec![vec![0.0; p + 1]; p + 1];
    let mut b = vec![0.0; p + 1];
    for k in 0..=p {
        a[k][k] += 1.0;
        for i in 1..=p {
            let lag = k.abs_diff(i);
            a[k][lag] -= phi[i - 1];
        }
        b[k] = rhs(k);
    }
    let g = solve_dense(a, b)?;
    let mut gamma = vec![0.0; max_lag.max(p) + 1];
    gamma[..=p].copy_from_slice(&g);
    for k in p + 1..gamma.len() {
        let mut v = rhs(k);
        for i in 1..=p {
            v += phi[i - 1] * gamma[k - i];
        }
        gamma[k] = v;
    }
    gamma.truncate(max_lag + 1);
    Some(gamma)
}

/// The ARMA model in state-space form.
#[derive(Debug, Clone)]
pub(crate) struct ArmaStateSpace {
    /// State dimension `r = max(p, q + 1)`.
    pub r: usize,
    /// `φ₁..φ_r` padded with zeros.
    pub phi: Vec<f64>,
    /// `R = (1, θ₁, …, θ_{r−1})`.
    pub rvec: Vec<f64>,
}

impl ArmaStateSpace {
    pub fn new(phi: &[f64], theta: &[f64]) -> Self {
        let r = phi.len().max(theta.len() + 1);
        let mut ph = phi.to_vec();
        ph.resize(r, 0.0);
        let mut rvec = vec![1.0];
        rvec.extend_from_slice(theta);
        rvec.resize(r, 0.0);
        Self { r, phi: ph, rvec }
    }

    /// Stationary state covariance `Q₀` (row-major `r × r`), or `None` for a
    /// non-stationary autoregression.
    pub fn stationary_covariance(&self, phi: &[f64], theta: &[f64]) -> Option<Vec<f64>> {
        let r = self.r;
        let gamma = arma_autocovariance(phi, theta, 2 * r + 1)?;
        let psi = psi_weights(phi, theta, 2 * r + 1);
        let g = |lag: isize| gamma[lag.unsigned_abs()];
        // c(a, b) = Cov(w_{t+a}, ε_{t+b}) = ψ_{a−b}.
        let c = |a: isize, b: isize| if a >= b { psi[(a - b) as usize] } else { 0.0 };
        let nz_phi: Vec<(usize, f64)> =
            self.phi.iter().enumerate().filter(|(_, v)| **v != 0.0).map(|(k, &v)| (k + 1, v)).collect();
        let nz_th: Vec<(usize, f64)> =
            self.rvec.iter().enumerate().filter(|(_, v)| **v != 0.0).map(|(k, &v)| (k, v)).collect();
        let mut q0 = vec![0.0; r * r];
        for i in 0..r {
            for j in i..r {
                let (ii, jj) = (i as isize, j as isize);
                let mut s = 0.0;
                for &(k, fk) in nz_phi.iter().filter(|(k, _)| *k > i) {
                    let a = ii - k as isize;
                    for &(l, fl) in nz_phi.iter().filter(|(l, _)| *l > j) {
                        s += fk * fl * g(a - (jj - l as isize));
                    }
                    for &(l, tl) in nz_th.iter().filter(|(l, _)| *l >= j) {
                        s += fk * tl * c(a, jj - l as isize);
                    }
                }
                for &(k, tk) in nz_th.iter().filter(|(k, _)| *k >= i) {
                    let a = ii - k as isize;
                    for &(l, fl) in nz_phi.iter().filter(|(l, _)| *l > j) {
                        s += tk * fl * c(jj - l as isize, a);
                    }
                    for &(l, tl) in nz_th.iter().filter(|(l, _)| *l >= j) {
                        if a == jj - l as isize {
                            s += tk * tl;
                        }
                    }
                }
                q0[i * r + j] = s;
                q0[j * r + i] = s;
            }
        }
        q0.iter().all(|v| v.is_finite()).then_some(q0)
    }
}

/// Output of the Kalman filter over a stationary series.
#[derive(Debug, Clone)]
pub(crate) struct FilterOutput {
    /// Σ vₜ² / Fₜ.
    pub ssq: f64,
    /// Σ ln Fₜ.
    pub sumlog: f64,
    /// Number of innovations.
    pub nu: usize,
    /// Innovations `vₜ` (empty unless requested).
    pub innovations: Vec<f64>,
    /// Innovation variances `Fₜ` in units of σ² (empty unless requested).
    pub gains: Vec<f64>,
    /// Predicted state `a_{n+1|n}` after the last observation.
    pub state: Vec<f64>,
    /// Its covariance (row-major), in units of σ².
    pub state_cov: Vec<f64>,
}

/// Run the Kalman filter for the zero-mean stationary series `w` under the
/// ARMA model, starting from `a₁ = 0`, `P₁ = Q₀`. Returns `None` when the
/// autoregression is not stationary or an innovation variance is not positive.
pub(crate) fn kalman_filter(w: &[f64], phi: &[f64], theta: &[f64], keep: bool) -> Option<FilterOutput> {
    let ss = ArmaStateSpace::new(phi, theta);
    let r = ss.r;
    let mut p = ss.stationary_covariance(phi, theta)?;
    let mut a = vec![0.0; r];
    let mut out = FilterOutput {
        ssq: 0.0,
        sumlog: 0.0,
        nu: 0,
        innovations: Vec::with_capacity(if keep { w.len() } else { 0 }),
        gains: Vec::with_capacity(if keep { w.len() } else { 0 }),
        state: Vec::new(),
        state_cov: Vec::new(),
    };
    let mut pu = vec![0.0; r * r];
    let mut m = vec![0.0; r * r];
    let mut steady = false;
    for &wt in w {
        let f = p[0];
        if !f.is_finite() || f <= 0.0 {
            return None;
        }
        let v = wt - a[0];
        out.ssq += v * v / f;
        out.sumlog += f.ln();
        out.nu += 1;
        if keep {
            out.innovations.push(v);
            out.gains.push(f);
        }
        // Update: a ← a + P e₁ v / F, P ← P − P e₁ e₁ᵀ P / F.
        let col: Vec<f64> = (0..r).map(|i| p[i * r]).collect();
        let au: Vec<f64> = (0..r).map(|i| a[i] + col[i] * v / f).collect();
        // Predict: a ← T a.
        for i in 0..r {
            a[i] = ss.phi[i] * au[0] + if i + 1 < r { au[i + 1] } else { 0.0 };
        }
        if steady {
            continue;
        }
        for i in 0..r {
            for j in 0..r {
                pu[i * r + j] = p[i * r + j] - col[i] * col[j] / f;
            }
        }
        // M = T Pu,  P = M Tᵀ + R Rᵀ.
        for i in 0..r {
            for j in 0..r {
                m[i * r + j] = ss.phi[i] * pu[j] + if i + 1 < r { pu[(i + 1) * r + j] } else { 0.0 };
            }
        }
        let mut change = 0.0_f64;
        for i in 0..r {
            for j in 0..r {
                let v = ss.phi[j] * m[i * r] + if j + 1 < r { m[i * r + j + 1] } else { 0.0 };
                let new = v + ss.rvec[i] * ss.rvec[j];
                change = change.max((new - p[i * r + j]).abs());
                p[i * r + j] = new;
            }
        }
        // Once the covariance has converged to the steady state (to rounding),
        // further updates leave it unchanged.
        if change < 1e-15 * p[0].abs().max(1.0) {
            steady = true;
        }
    }
    out.state = a;
    out.state_cov = p;
    Some(out)
}

/// Conditional sum of squares: residuals `eₜ = wₜ − Σ φⱼ w_{t−j} − Σ θⱼ e_{t−j}`
/// for `t ≥ p`, with earlier residuals set to zero (Box, Jenkins, Reinsel &
/// Ljung 2015, §7.1). Returns `(Σ eₜ², count)`.
pub(crate) fn conditional_sum_of_squares(w: &[f64], phi: &[f64], theta: &[f64]) -> (f64, usize) {
    let p = phi.len();
    let n = w.len();
    let mut e = vec![0.0; n];
    let mut ssq = 0.0;
    for t in p..n {
        let mut v = w[t];
        for (j, &ph) in phi.iter().enumerate() {
            v -= ph * w[t - 1 - j];
        }
        for (j, &th) in theta.iter().enumerate() {
            if t > j {
                v -= th * e[t - 1 - j];
            }
        }
        e[t] = v;
        ssq += v * v;
    }
    (ssq, n.saturating_sub(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lyapunov_residual(phi: &[f64], theta: &[f64]) -> f64 {
        let ss = ArmaStateSpace::new(phi, theta);
        let r = ss.r;
        let q = ss.stationary_covariance(phi, theta).unwrap();
        let t = |i: usize, j: usize| -> f64 {
            if j == 0 {
                ss.phi[i]
            } else if j == i + 1 {
                1.0
            } else {
                0.0
            }
        };
        let mut worst = 0.0_f64;
        for i in 0..r {
            for j in 0..r {
                let mut s = ss.rvec[i] * ss.rvec[j];
                for k in 0..r {
                    for l in 0..r {
                        s += t(i, k) * q[k * r + l] * t(j, l);
                    }
                }
                worst = worst.max((s - q[i * r + j]).abs());
            }
        }
        worst
    }

    #[test]
    fn stationary_covariance_solves_lyapunov_equation() {
        assert!(lyapunov_residual(&[0.5, -0.3], &[0.4]) < 1e-12);
        assert!(lyapunov_residual(&[0.9], &[]) < 1e-12);
        assert!(lyapunov_residual(&[], &[0.4, 0.0, 0.2]) < 1e-12);
        assert!(lyapunov_residual(&[0.2, 0.1, 0.0, 0.3], &[-0.5, 0.25, 0.1, 0.0, 0.0, 0.3]) < 1e-12);
    }

    #[test]
    fn ar1_autocovariance() {
        let g = arma_autocovariance(&[0.6], &[], 3).unwrap();
        let g0 = 1.0 / (1.0 - 0.36);
        for (k, v) in g.iter().enumerate() {
            assert!((v - g0 * 0.6_f64.powi(k as i32)).abs() < 1e-12);
        }
        // MA(1): γ0 = 1 + θ², γ1 = θ.
        let g = arma_autocovariance(&[], &[0.5], 2).unwrap();
        assert!((g[0] - 1.25).abs() < 1e-15 && (g[1] - 0.5).abs() < 1e-15 && g[2] == 0.0);
    }

    #[test]
    fn white_noise_likelihood_is_sum_of_squares() {
        let w = [0.3, -1.0, 2.0, 0.5];
        let f = kalman_filter(&w, &[], &[], true).unwrap();
        assert!((f.ssq - w.iter().map(|v| v * v).sum::<f64>()).abs() < 1e-14);
        assert!(f.sumlog.abs() < 1e-14);
    }
}
