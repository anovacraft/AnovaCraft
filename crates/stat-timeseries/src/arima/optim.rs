//! Quasi-Newton minimisation with numerical derivatives, and the numerical
//! Hessian used for standard errors.
//!
//! BFGS (Broyden 1970; Fletcher 1970; Goldfarb 1970; Shanno 1970) on the
//! inverse Hessian (Nocedal & Wright, *Numerical Optimization*, 2006,
//! Algorithm 6.1) in the variable-metric arrangement of Nash (*Compact
//! Numerical Methods for Computers*, 2nd ed., 1990, Algorithm 21): start from
//! the identity, try the full step and backtrack by a factor 0.2 until the
//! Armijo condition holds, skip the update when the curvature condition fails
//! and restart from the identity when a step makes no progress. Gradients are
//! central differences. The likelihoods of richer ARMA models can have
//! several local maxima, some on the boundary of invertibility; this
//! conservative arrangement (no step-length scaling) tends to stay with the
//! interior maximum nearest the conditional-sum-of-squares start.

/// Result of a minimisation.
#[derive(Debug, Clone)]
pub(crate) struct Minimum {
    pub x: Vec<f64>,
    pub value: f64,
    pub iterations: usize,
    pub converged: bool,
}

fn gradient(f: &mut dyn FnMut(&[f64]) -> f64, x: &[f64], fx: f64) -> Vec<f64> {
    let mut g = vec![0.0; x.len()];
    let mut xt = x.to_vec();
    for i in 0..x.len() {
        let h = 1e-6 * x[i].abs().max(1.0);
        xt[i] = x[i] + h;
        let fp = f(&xt);
        xt[i] = x[i] - h;
        let fm = f(&xt);
        xt[i] = x[i];
        g[i] = if fp.is_finite() && fm.is_finite() {
            (fp - fm) / (2.0 * h)
        } else if fp.is_finite() {
            (fp - fx) / h
        } else if fm.is_finite() {
            (fx - fm) / h
        } else {
            0.0
        };
    }
    g
}

/// Minimise `f` from `x0`. A non-finite objective counts as +∞ (the line
/// search backs off). Converged when the largest gradient component is below
/// `gtol` or no step decreases the objective even after resetting the
/// inverse-Hessian approximation.
pub(crate) fn bfgs(f: &mut dyn FnMut(&[f64]) -> f64, x0: &[f64], max_iter: usize, gtol: f64) -> Minimum {
    let n = x0.len();
    let mut x = x0.to_vec();
    let mut fx = f(&x);
    if n == 0 || !fx.is_finite() {
        return Minimum { x, value: fx, iterations: 0, converged: n == 0 && fx.is_finite() };
    }
    let mut g = gradient(f, &x, fx);
    let identity = |n: usize| -> Vec<f64> {
        let mut h = vec![0.0; n * n];
        for i in 0..n {
            h[i * n + i] = 1.0;
        }
        h
    };
    let mut h = identity(n);
    let mut fresh = true;
    let mut iterations = 0;
    let mut converged = false;
    while iterations < max_iter {
        iterations += 1;
        if g.iter().fold(0.0_f64, |m, v| m.max(v.abs())) < gtol {
            converged = true;
            break;
        }
        let mut d: Vec<f64> = (0..n).map(|i| -(0..n).map(|j| h[i * n + j] * g[j]).sum::<f64>()).collect();
        let mut slope: f64 = d.iter().zip(&g).map(|(a, b)| a * b).sum();
        if slope >= 0.0 {
            h = identity(n);
            fresh = true;
            d = g.iter().map(|v| -v).collect();
            slope = -g.iter().map(|v| v * v).sum::<f64>();
        }
        let mut t = 1.0;
        let mut accepted = None;
        for _ in 0..60 {
            let xn: Vec<f64> = x.iter().zip(&d).map(|(a, b)| a + t * b).collect();
            let fnew = f(&xn);
            if fnew.is_finite() && fnew <= fx + 1e-4 * t * slope {
                accepted = Some((xn, fnew));
                break;
            }
            t *= 0.2;
        }
        let Some((xn, fnew)) = accepted else {
            if fresh {
                // No descent along the gradient: as good as the derivatives allow.
                converged = true;
                break;
            }
            h = identity(n);
            fresh = true;
            continue;
        };
        let gn = gradient(f, &xn, fnew);
        let s: Vec<f64> = xn.iter().zip(&x).map(|(a, b)| a - b).collect();
        let y: Vec<f64> = gn.iter().zip(&g).map(|(a, b)| a - b).collect();
        let sy: f64 = s.iter().zip(&y).map(|(a, b)| a * b).sum();
        let small_change = (fx - fnew).abs() <= 1e-15 * (1.0 + fx.abs());
        x = xn;
        g = gn;
        let previous = fx;
        fx = fnew;
        if sy > 1e-14 * s.iter().map(|v| v * v).sum::<f64>().sqrt() * y.iter().map(|v| v * v).sum::<f64>().sqrt() {
            // H ← (I − ρ s yᵀ) H (I − ρ y sᵀ) + ρ s sᵀ.
            let rho = 1.0 / sy;
            let hy: Vec<f64> = (0..n).map(|i| (0..n).map(|j| h[i * n + j] * y[j]).sum()).collect();
            let yhy: f64 = y.iter().zip(&hy).map(|(a, b)| a * b).sum();
            for i in 0..n {
                for j in 0..n {
                    h[i * n + j] += -rho * (hy[i] * s[j] + s[i] * hy[j]) + (rho * rho * yhy + rho) * s[i] * s[j];
                }
            }
            fresh = false;
        }
        if small_change && previous - fx >= 0.0 && g.iter().fold(0.0_f64, |m, v| m.max(v.abs())) < gtol.sqrt() {
            converged = true;
            break;
        }
    }
    Minimum { x, value: fx, iterations, converged }
}

/// Central-difference Hessian of `f` at `x` with steps `hᵢ`; `None` when an
/// evaluation is not finite.
pub(crate) fn hessian(f: &mut dyn FnMut(&[f64]) -> f64, x: &[f64], steps: &[f64]) -> Option<Vec<f64>> {
    let n = x.len();
    let f0 = f(x);
    let mut hm = vec![0.0; n * n];
    let mut xt = x.to_vec();
    let mut eval = |xt: &[f64]| -> Option<f64> {
        let v = f(xt);
        v.is_finite().then_some(v)
    };
    for i in 0..n {
        let hi = steps[i];
        xt[i] = x[i] + hi;
        let fp = eval(&xt)?;
        xt[i] = x[i] - hi;
        let fm = eval(&xt)?;
        xt[i] = x[i];
        hm[i * n + i] = (fp - 2.0 * f0 + fm) / (hi * hi);
        for j in 0..i {
            let hj = steps[j];
            let mut corner = |si: f64, sj: f64, xt: &mut Vec<f64>| -> Option<f64> {
                xt[i] = x[i] + si * hi;
                xt[j] = x[j] + sj * hj;
                let v = eval(xt);
                xt[i] = x[i];
                xt[j] = x[j];
                v
            };
            let pp = corner(1.0, 1.0, &mut xt)?;
            let pm = corner(1.0, -1.0, &mut xt)?;
            let mp = corner(-1.0, 1.0, &mut xt)?;
            let mm = corner(-1.0, -1.0, &mut xt)?;
            let v = (pp - pm - mp + mm) / (4.0 * hi * hj);
            hm[i * n + j] = v;
            hm[j * n + i] = v;
        }
    }
    Some(hm)
}

/// Inverse of a symmetric positive definite matrix by Cholesky; `None` when it
/// is not positive definite.
pub(crate) fn invert_spd(a: &[f64], n: usize) -> Option<Vec<f64>> {
    let mut l = vec![0.0; n * n];
    for j in 0..n {
        let mut d = a[j * n + j];
        for k in 0..j {
            d -= l[j * n + k] * l[j * n + k];
        }
        if !d.is_finite() || d <= 0.0 {
            return None;
        }
        let dj = d.sqrt();
        l[j * n + j] = dj;
        for i in j + 1..n {
            let mut s = a[i * n + j];
            for k in 0..j {
                s -= l[i * n + k] * l[j * n + k];
            }
            l[i * n + j] = s / dj;
        }
    }
    // Columns of L⁻¹, then A⁻¹ = L⁻ᵀ L⁻¹.
    let mut linv = vec![0.0; n * n];
    for c in 0..n {
        for i in c..n {
            let mut s = if i == c { 1.0 } else { 0.0 };
            for k in c..i {
                s -= l[i * n + k] * linv[k * n + c];
            }
            linv[i * n + c] = s / l[i * n + i];
        }
    }
    let mut inv = vec![0.0; n * n];
    for i in 0..n {
        for j in 0..=i {
            let s: f64 = (i..n).map(|k| linv[k * n + i] * linv[k * n + j]).sum();
            inv[i * n + j] = s;
            inv[j * n + i] = s;
        }
    }
    Some(inv)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bfgs_finds_rosenbrock_minimum() {
        let mut f = |x: &[f64]| (1.0 - x[0]).powi(2) + 100.0 * (x[1] - x[0] * x[0]).powi(2);
        let m = bfgs(&mut f, &[-1.2, 1.0], 500, 1e-8);
        assert!((m.x[0] - 1.0).abs() < 1e-5 && (m.x[1] - 1.0).abs() < 1e-5, "{m:?}");
    }

    #[test]
    fn hessian_and_inverse_of_quadratic() {
        let mut f = |x: &[f64]| 2.0 * x[0] * x[0] + x[0] * x[1] + 3.0 * x[1] * x[1];
        let h = hessian(&mut f, &[0.3, -0.2], &[1e-3, 1e-3]).unwrap();
        let want = [4.0, 1.0, 1.0, 6.0];
        for (a, b) in h.iter().zip(want) {
            assert!((a - b).abs() < 1e-6);
        }
        let inv = invert_spd(&h, 2).unwrap();
        let det = 23.0;
        let want = [6.0 / det, -1.0 / det, -1.0 / det, 4.0 / det];
        for (a, b) in inv.iter().zip(want) {
            assert!((a - b).abs() < 1e-6);
        }
        assert!(invert_spd(&[1.0, 2.0, 2.0, 1.0], 2).is_none());
    }
}
