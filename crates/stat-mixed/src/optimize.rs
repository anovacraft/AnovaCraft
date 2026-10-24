//! Derivative-free and quasi-Newton minimisation with lower bounds, and
//! finite-difference Hessians.
//!
//! * Nelder–Mead (Nelder & Mead, *Computer Journal* 7, 1965) with vertices
//!   projected onto the bounds and restarts from the best point (a common
//!   remedy for premature collapse, Kelley, *Iterative Methods for
//!   Optimization*, 1999, §8.1).
//! * BFGS on finite-difference gradients with projection onto the bounds and
//!   an Armijo backtracking line search; variables held at a bound by the
//!   gradient are frozen for the step (Nocedal & Wright, *Numerical
//!   Optimization*, 2nd ed., 2006, §6.1 and §16.7).

use crate::dense::Square;

/// Outcome of a minimisation.
#[derive(Debug, Clone)]
pub(crate) struct Minimum {
    pub x: Vec<f64>,
    pub f: f64,
    pub converged: bool,
}

fn project(x: &mut [f64], lower: &[f64]) {
    for (v, l) in x.iter_mut().zip(lower) {
        if *v < *l {
            *v = *l;
        }
    }
}

/// Minimise `f` from `x0` subject to `x ≥ lower` (use `−∞` for free
/// variables) by Nelder–Mead with initial steps `step`.
pub(crate) fn nelder_mead(
    f: &mut dyn FnMut(&[f64]) -> f64,
    x0: &[f64],
    lower: &[f64],
    step: &[f64],
    max_evaluations: usize,
) -> Minimum {
    let n = x0.len();
    let mut evals = 0usize;
    let mut eval = |x: &[f64], evals: &mut usize| -> f64 {
        *evals += 1;
        let v = f(x);
        if v.is_nan() { f64::INFINITY } else { v }
    };
    let mut best_x = x0.to_vec();
    project(&mut best_x, lower);
    let mut best_f = eval(&best_x, &mut evals);
    if n == 0 {
        return Minimum { x: best_x, f: best_f, converged: true };
    }
    let mut converged = false;
    let mut scale = 1.0;
    for _restart in 0..6 {
        // Initial simplex around the best point.
        let mut pts: Vec<Vec<f64>> = vec![best_x.clone()];
        let mut vals = vec![best_f];
        for i in 0..n {
            let mut p = best_x.clone();
            let s = step[i] * scale;
            p[i] += s;
            if p[i] < lower[i] {
                p[i] = best_x[i] - s;
            }
            project(&mut p, lower);
            if p == best_x {
                p[i] += s.abs().max(1e-4);
            }
            vals.push(eval(&p, &mut evals));
            pts.push(p);
        }
        let start_f = best_f;
        loop {
            // Order.
            let mut order: Vec<usize> = (0..=n).collect();
            order.sort_by(|&a, &b| vals[a].total_cmp(&vals[b]));
            pts = order.iter().map(|&i| pts[i].clone()).collect();
            vals = order.iter().map(|&i| vals[i]).collect();
            let spread = (vals[n] - vals[0]).abs();
            let size = (1..=n)
                .map(|i| pts[i].iter().zip(&pts[0]).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max))
                .fold(0.0, f64::max);
            if (spread <= 1e-11 * (vals[0].abs() + 1e-8)
                && size <= 1e-7 * (1.0 + pts[0].iter().map(|v| v.abs()).fold(0.0, f64::max)))
                || evals >= max_evaluations
            {
                break;
            }
            let centroid: Vec<f64> = (0..n).map(|j| pts[..n].iter().map(|p| p[j]).sum::<f64>() / n as f64).collect();
            let along = |t: f64| -> Vec<f64> {
                let mut p: Vec<f64> = (0..n).map(|j| centroid[j] + t * (pts[n][j] - centroid[j])).collect();
                project(&mut p, lower);
                p
            };
            let xr = along(-1.0);
            let fr = eval(&xr, &mut evals);
            if fr < vals[0] {
                let xe = along(-2.0);
                let fe = eval(&xe, &mut evals);
                if fe < fr {
                    pts[n] = xe;
                    vals[n] = fe;
                } else {
                    pts[n] = xr;
                    vals[n] = fr;
                }
            } else if fr < vals[n - 1] {
                pts[n] = xr;
                vals[n] = fr;
            } else {
                let (xc, fc) = if fr < vals[n] {
                    let xc = along(-0.5);
                    let fc = eval(&xc, &mut evals);
                    (xc, fc)
                } else {
                    let xc = along(0.5);
                    let fc = eval(&xc, &mut evals);
                    (xc, fc)
                };
                if fc < vals[n].min(fr) {
                    pts[n] = xc;
                    vals[n] = fc;
                } else {
                    // Shrink towards the best.
                    for i in 1..=n {
                        let mut p: Vec<f64> = (0..n).map(|j| pts[0][j] + 0.5 * (pts[i][j] - pts[0][j])).collect();
                        project(&mut p, lower);
                        vals[i] = eval(&p, &mut evals);
                        pts[i] = p;
                    }
                }
            }
        }
        let i = (0..=n).min_by(|&a, &b| vals[a].total_cmp(&vals[b])).unwrap_or(0);
        let improved = start_f - vals[i];
        if vals[i] <= best_f {
            best_f = vals[i];
            best_x = pts[i].clone();
        }
        if evals >= max_evaluations {
            break;
        }
        // Converged when a fresh, smaller simplex finds nothing better.
        if improved <= 1e-9 * (best_f.abs() + 1e-8) {
            converged = true;
            break;
        }
        scale *= 0.5;
    }
    Minimum { x: best_x, f: best_f, converged }
}

/// Finite-difference gradient (central inside, forward at a lower bound).
fn gradient(f: &mut dyn FnMut(&[f64]) -> f64, x: &[f64], fx: f64, lower: &[f64], evals: &mut usize) -> Vec<f64> {
    let mut g = vec![0.0; x.len()];
    let mut p = x.to_vec();
    for i in 0..x.len() {
        let h = 1e-6 * x[i].abs().max(1.0);
        if x[i] - h < lower[i] {
            p[i] = x[i] + h;
            let fp = f(&p);
            *evals += 1;
            g[i] = (fp - fx) / h;
        } else {
            p[i] = x[i] + h;
            let fp = f(&p);
            p[i] = x[i] - h;
            let fm = f(&p);
            *evals += 2;
            g[i] = (fp - fm) / (2.0 * h);
        }
        p[i] = x[i];
    }
    g
}

/// Minimise `f` from `x0` subject to `x ≥ lower` by projected BFGS on
/// finite-difference gradients.
pub(crate) fn bfgs(f: &mut dyn FnMut(&[f64]) -> f64, x0: &[f64], lower: &[f64], max_evaluations: usize) -> Minimum {
    let n = x0.len();
    let mut evals = 0usize;
    let mut x = x0.to_vec();
    project(&mut x, lower);
    let mut fx = f(&x);
    evals += 1;
    if n == 0 || !fx.is_finite() {
        return Minimum { x, f: fx, converged: n == 0 };
    }
    let mut h = Square::identity(n);
    let mut fresh = true;
    let mut g = gradient(f, &x, fx, lower, &mut evals);
    let mut converged = false;
    let mut small_steps = 0;
    while evals < max_evaluations {
        let free: Vec<bool> = (0..n).map(|i| !(x[i] <= lower[i] && g[i] > 0.0)).collect();
        let pg = (0..n).filter(|&i| free[i]).map(|i| g[i].abs()).fold(0.0, f64::max);
        if pg <= 1e-6 * (1.0 + fx.abs()) * 1e-2 {
            converged = true;
            break;
        }
        let mut d: Vec<f64> = (0..n)
            .map(|i| if free[i] { -(0..n).filter(|&j| free[j]).map(|j| h.get(i, j) * g[j]).sum::<f64>() } else { 0.0 })
            .collect();
        let mut slope: f64 = d.iter().zip(&g).map(|(a, b)| a * b).sum();
        if slope >= 0.0 {
            h = Square::identity(n);
            fresh = true;
            d = (0..n).map(|i| if free[i] { -g[i] } else { 0.0 }).collect();
            slope = d.iter().zip(&g).map(|(a, b)| a * b).sum();
        }
        // First step from an identity metric: keep it modest.
        let dn = d.iter().map(|v| v.abs()).fold(0.0, f64::max);
        let mut t = if fresh && dn > 1.0 { 1.0 / dn } else { 1.0 };
        let mut accepted = None;
        for _ in 0..40 {
            let mut xn: Vec<f64> = (0..n).map(|i| x[i] + t * d[i]).collect();
            project(&mut xn, lower);
            let fnew = f(&xn);
            evals += 1;
            let dec: f64 = (0..n).map(|i| g[i] * (xn[i] - x[i])).sum();
            if fnew.is_finite() && fnew <= fx + 1e-4 * dec.min(0.0) && fnew <= fx {
                accepted = Some((xn, fnew));
                break;
            }
            t *= 0.5;
        }
        let _ = slope;
        let Some((xn, fnew)) = accepted else {
            if fresh {
                converged = true;
                break;
            }
            h = Square::identity(n);
            fresh = true;
            continue;
        };
        let gn = gradient(f, &xn, fnew, lower, &mut evals);
        let s: Vec<f64> = (0..n).map(|i| xn[i] - x[i]).collect();
        let y: Vec<f64> = (0..n).map(|i| gn[i] - g[i]).collect();
        let sy: f64 = s.iter().zip(&y).map(|(a, b)| a * b).sum();
        if sy > 1e-14 {
            if fresh {
                let yy: f64 = y.iter().map(|v| v * v).sum();
                h = Square::identity(n);
                for i in 0..n {
                    *h.at(i, i) = sy / yy;
                }
                fresh = false;
            }
            // H ← (I − ρ s yᵀ) H (I − ρ y sᵀ) + ρ s sᵀ
            let rho = 1.0 / sy;
            let hy: Vec<f64> = (0..n).map(|i| (0..n).map(|j| h.get(i, j) * y[j]).sum()).collect();
            let yhy: f64 = y.iter().zip(&hy).map(|(a, b)| a * b).sum();
            for i in 0..n {
                for j in 0..n {
                    let v = h.get(i, j) - rho * (hy[i] * s[j] + s[i] * hy[j]) + (rho * rho * yhy + rho) * s[i] * s[j];
                    *h.at(i, j) = v;
                }
            }
        }
        let change = fx - fnew;
        x = xn;
        fx = fnew;
        g = gn;
        if change <= 1e-12 * (fx.abs() + 1e-8) {
            small_steps += 1;
            if small_steps >= 3 {
                converged = true;
                break;
            }
        } else {
            small_steps = 0;
        }
    }
    Minimum { x, f: fx, converged }
}

/// Finite-difference Hessian of `f` at `x` over the coordinates `vary`
/// (central differences; coordinates closer than one step to their lower
/// bound use forward differences).
pub(crate) fn hessian(f: &mut dyn FnMut(&[f64]) -> f64, x: &[f64], lower: &[f64], vary: &[usize]) -> Square {
    let m = vary.len();
    let mut out = Square::zeros(m);
    let steps: Vec<f64> = vary.iter().map(|&i| 1e-4 * x[i].abs().max(0.1)).collect();
    let f0 = f(x);
    let shifted = |f: &mut dyn FnMut(&[f64]) -> f64, moves: &[(usize, f64)]| -> f64 {
        let mut p = x.to_vec();
        for &(i, d) in moves {
            p[i] += d;
        }
        f(&p)
    };
    // Offsets: centred at x, or at x + 2h where a bound is near (forward).
    let forward: Vec<bool> = vary.iter().zip(&steps).map(|(&i, &h)| x[i] - 2.0 * h < lower[i]).collect();
    for a in 0..m {
        let (i, hi) = (vary[a], steps[a]);
        let diag = if forward[a] {
            let (f1, f2) = (shifted(f, &[(i, hi)]), shifted(f, &[(i, 2.0 * hi)]));
            (f2 - 2.0 * f1 + f0) / (hi * hi)
        } else {
            (shifted(f, &[(i, hi)]) - 2.0 * f0 + shifted(f, &[(i, -hi)])) / (hi * hi)
        };
        *out.at(a, a) = diag;
        for b in 0..a {
            let (j, hj) = (vary[b], steps[b]);
            let (si, sj) =
                (if forward[a] { [hi, 0.0] } else { [hi, -hi] }, if forward[b] { [hj, 0.0] } else { [hj, -hj] });
            let v = (shifted(f, &[(i, si[0]), (j, sj[0])])
                - shifted(f, &[(i, si[0]), (j, sj[1])])
                - shifted(f, &[(i, si[1]), (j, sj[0])])
                + shifted(f, &[(i, si[1]), (j, sj[1])]))
                / ((si[0] - si[1]) * (sj[0] - sj[1]));
            *out.at(a, b) = v;
            *out.at(b, a) = v;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rosenbrock(x: &[f64]) -> f64 {
        (1.0 - x[0]).powi(2) + 100.0 * (x[1] - x[0] * x[0]).powi(2)
    }

    #[test]
    fn nelder_mead_finds_minima_and_respects_bounds() {
        let m = nelder_mead(&mut |x| rosenbrock(x), &[-1.2, 1.0], &[f64::NEG_INFINITY; 2], &[0.5, 0.5], 5000);
        assert!(m.converged && (m.x[0] - 1.0).abs() < 1e-4 && (m.x[1] - 1.0).abs() < 1e-4, "{m:?}");
        // Constrained minimum on the bound: (x + 1)² + (y − 2)², x ≥ 0.
        let m = nelder_mead(
            &mut |x| (x[0] + 1.0).powi(2) + (x[1] - 2.0).powi(2),
            &[1.0, 0.0],
            &[0.0, f64::NEG_INFINITY],
            &[0.5, 0.5],
            2000,
        );
        assert!(m.x[0] == 0.0 && (m.x[1] - 2.0).abs() < 1e-5, "{m:?}");
    }

    #[test]
    fn bfgs_finds_minima_and_respects_bounds() {
        let m = bfgs(&mut |x| rosenbrock(x), &[-1.2, 1.0], &[f64::NEG_INFINITY; 2], 20_000);
        assert!((m.x[0] - 1.0).abs() < 1e-4 && (m.x[1] - 1.0).abs() < 1e-4, "{m:?}");
        let m = bfgs(
            &mut |x| (x[0] + 1.0).powi(2) + (x[1] - 2.0).powi(2) + x[0] * x[1],
            &[1.0, 0.0],
            &[0.0, f64::NEG_INFINITY],
            2000,
        );
        assert!(m.x[0] == 0.0 && (m.x[1] - 2.0).abs() < 1e-5, "{m:?}");
    }

    #[test]
    fn hessians_of_quadratics_are_exact() {
        let mut f = |x: &[f64]| 3.0 * x[0] * x[0] + 2.0 * x[0] * x[1] + 5.0 * x[1] * x[1] + x[2];
        let h = hessian(&mut f, &[0.3, -0.2, 1.0], &[f64::NEG_INFINITY; 3], &[0, 1]);
        assert!(
            (h.get(0, 0) - 6.0).abs() < 1e-5 && (h.get(0, 1) - 2.0).abs() < 1e-5 && (h.get(1, 1) - 10.0).abs() < 1e-5
        );
        // Forward differences at a bound.
        let h = hessian(&mut f, &[0.0, 0.0, 0.0], &[0.0, f64::NEG_INFINITY, f64::NEG_INFINITY], &[0, 1]);
        assert!((h.get(0, 0) - 6.0).abs() < 1e-5 && (h.get(1, 0) - 2.0).abs() < 1e-5);
    }
}
