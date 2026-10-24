//! Derivative-free minimisation inside a box, and space-filling start points.
//!
//! * [`minimize_in_box`]: Nelder–Mead (Nelder & Mead, "A simplex method for
//!   function minimization", *Computer Journal* 7, 1965) with the standard
//!   coefficients (reflection 1, expansion 2, contraction ½, shrink ½),
//!   every trial point projected onto the box, and restarts from the best
//!   point found until a restart no longer improves it (a common remedy for a
//!   simplex that collapses early; Kelley, *Iterative Methods for
//!   Optimization*, 1999, §8.1).
//! * [`latin_hypercube`]: a random Latin hypercube sample of the unit cube
//!   (McKay, Beckman & Conover, *Technometrics* 21, 1979): each coordinate's
//!   `n` values fall one in each of `n` equal strata, so a few starts already
//!   spread across every factor's range.
//! * [`levenberg_marquardt`]: nonlinear least squares (Levenberg–Marquardt
//!   with Gauss–Newton steps, bounds, Bates & Watts' relative offset
//!   convergence criterion, analytic or central-difference derivatives); see
//!   [`least_squares`].

pub mod least_squares;

pub use least_squares::{
    BoundState, LeastSquaresOptions, LeastSquaresProblem, LeastSquaresResult, ResidualFn, Termination,
    levenberg_marquardt, numeric_jacobian,
};

use crate::rng::Rng;

/// The best point a search found.
#[derive(Debug, Clone, PartialEq)]
pub struct BoxMinimum {
    /// Location (inside the box).
    pub x: Vec<f64>,
    /// Objective value there.
    pub value: f64,
    /// Number of objective evaluations used.
    pub evaluations: usize,
}

fn project(x: &mut [f64], lower: &[f64], upper: &[f64]) {
    for ((v, &l), &u) in x.iter_mut().zip(lower).zip(upper) {
        *v = v.clamp(l, u);
    }
}

/// Minimise `f` over the box `lower ≤ x ≤ upper`, starting from `x0`, with
/// at most `max_evaluations` evaluations of `f`. The initial simplex steps a
/// tenth of each side's width (towards the inside where `x0` is on an edge).
/// A NaN objective counts as +∞. Coordinates with `lower == upper` stay put.
///
/// A bound that is NaN, or a lower bound above its upper bound, defines no
/// box: the search is not run and the result has `value = +∞` and no
/// evaluations, which callers already treat as a failed search.
///
/// # Panics
/// If `x0`, `lower` and `upper` differ in length.
pub fn minimize_in_box(
    f: &mut dyn FnMut(&[f64]) -> f64,
    x0: &[f64],
    lower: &[f64],
    upper: &[f64],
    max_evaluations: usize,
) -> BoxMinimum {
    let n = x0.len();
    assert!(lower.len() == n && upper.len() == n, "bounds must match the start point");
    // `l <= u` is false for NaN as well as for inverted bounds.
    if !lower.iter().zip(upper).all(|(l, u)| l <= u) {
        return BoxMinimum { x: x0.to_vec(), value: f64::INFINITY, evaluations: 0 };
    }
    let mut evaluations = 0usize;
    let mut eval = |x: &[f64], evaluations: &mut usize| -> f64 {
        *evaluations += 1;
        let v = f(x);
        if v.is_nan() { f64::INFINITY } else { v }
    };
    let mut best_x = x0.to_vec();
    project(&mut best_x, lower, upper);
    let mut best = eval(&best_x, &mut evaluations);
    let free: Vec<usize> = (0..n).filter(|&i| upper[i] > lower[i]).collect();
    if free.is_empty() {
        return BoxMinimum { x: best_x, value: best, evaluations };
    }
    for restart in 0..8 {
        // Simplex around the best point, over the free coordinates.
        let scale = if restart == 0 { 0.1 } else { 0.05 };
        let mut pts = vec![best_x.clone()];
        let mut vals = vec![best];
        for &i in &free {
            let mut p = best_x.clone();
            let step = scale * (upper[i] - lower[i]);
            p[i] = if p[i] + step <= upper[i] { p[i] + step } else { p[i] - step };
            vals.push(eval(&p, &mut evaluations));
            pts.push(p);
        }
        let before = best;
        while evaluations < max_evaluations {
            let mut order: Vec<usize> = (0..pts.len()).collect();
            order.sort_by(|&a, &b| vals[a].total_cmp(&vals[b]));
            pts = order.iter().map(|&i| pts[i].clone()).collect();
            vals = order.iter().map(|&i| vals[i]).collect();
            let worst = pts.len() - 1;
            // Converged when the simplex is tiny relative to the box and flat.
            let size = free
                .iter()
                .map(|&i| {
                    let (lo, hi) =
                        pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p[i]), b.max(p[i])));
                    (hi - lo) / (upper[i] - lower[i])
                })
                .fold(0.0_f64, f64::max);
            if size < 1e-9 || (vals[worst] - vals[0]).abs() <= 1e-12 * (1.0 + vals[0].abs()) && size < 1e-4 {
                break;
            }
            let centroid: Vec<f64> =
                (0..n).map(|i| pts[..worst].iter().map(|p| p[i]).sum::<f64>() / worst as f64).collect();
            let along = |t: f64| -> Vec<f64> {
                let mut p: Vec<f64> = (0..n).map(|i| centroid[i] + t * (pts[worst][i] - centroid[i])).collect();
                project(&mut p, lower, upper);
                p
            };
            let reflected = along(-1.0);
            let fr = eval(&reflected, &mut evaluations);
            if fr < vals[0] {
                let expanded = along(-2.0);
                let fe = eval(&expanded, &mut evaluations);
                if fe < fr {
                    (pts[worst], vals[worst]) = (expanded, fe);
                } else {
                    (pts[worst], vals[worst]) = (reflected, fr);
                }
            } else if fr < vals[worst - 1] {
                (pts[worst], vals[worst]) = (reflected, fr);
            } else {
                let (contracted, fc) = if fr < vals[worst] {
                    let c = along(-0.5);
                    let v = eval(&c, &mut evaluations);
                    (c, v)
                } else {
                    let c = along(0.5);
                    let v = eval(&c, &mut evaluations);
                    (c, v)
                };
                if fc < vals[worst].min(fr) {
                    (pts[worst], vals[worst]) = (contracted, fc);
                } else {
                    // Shrink towards the best vertex.
                    for k in 1..pts.len() {
                        let mut p: Vec<f64> = (0..n).map(|i| pts[0][i] + 0.5 * (pts[k][i] - pts[0][i])).collect();
                        project(&mut p, lower, upper);
                        vals[k] = eval(&p, &mut evaluations);
                        pts[k] = p;
                    }
                }
            }
        }
        let k = (0..vals.len()).min_by(|&a, &b| vals[a].total_cmp(&vals[b])).unwrap_or(0);
        if vals[k] < best {
            best = vals[k];
            best_x = pts[k].clone();
        }
        if evaluations >= max_evaluations || before - best <= 1e-12 * (1.0 + best.abs()) {
            break;
        }
    }
    BoxMinimum { x: best_x, value: best, evaluations }
}

/// `n` points of a random Latin hypercube in `[0, 1)^dims`, from `rng`:
/// for each coordinate the points take one value in each interval
/// `[k/n, (k+1)/n)`, in random order and at a random place inside it.
#[must_use]
pub fn latin_hypercube(n: usize, dims: usize, rng: &mut Rng) -> Vec<Vec<f64>> {
    let mut points = vec![vec![0.0; dims]; n];
    for d in 0..dims {
        let mut strata: Vec<usize> = (0..n).collect();
        rng.shuffle(&mut strata);
        for (p, &k) in points.iter_mut().zip(&strata) {
            p[d] = (k as f64 + rng.next_f64()) / n as f64;
        }
    }
    points
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_interior_and_boundary_minima() {
        // Rosenbrock's valley, minimum at (1, 1).
        let mut rosen = |x: &[f64]| (1.0 - x[0]).powi(2) + 100.0 * (x[1] - x[0] * x[0]).powi(2);
        let m = minimize_in_box(&mut rosen, &[-1.2, 1.0], &[-2.0, -2.0], &[2.0, 2.0], 5000);
        assert!((m.x[0] - 1.0).abs() < 1e-3 && (m.x[1] - 1.0).abs() < 1e-3, "{m:?}");
        // A bowl centred outside the box: the minimum is on the nearest edge.
        let mut bowl = |x: &[f64]| (x[0] - 3.0).powi(2) + (x[1] + 0.5).powi(2);
        let m = minimize_in_box(&mut bowl, &[0.0, 0.0], &[-1.0, -1.0], &[1.0, 1.0], 2000);
        assert!((m.x[0] - 1.0).abs() < 1e-6 && (m.x[1] + 0.5).abs() < 1e-4, "{m:?}");
        assert!(m.evaluations <= 2000);
        // A fixed coordinate stays where it is.
        let m = minimize_in_box(&mut bowl, &[0.0, 0.3], &[-1.0, 0.3], &[1.0, 0.3], 500);
        assert_eq!(m.x[1], 0.3);
        assert!((m.x[0] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn latin_hypercube_fills_every_stratum() {
        let mut rng = Rng::new(7);
        let pts = latin_hypercube(10, 3, &mut rng);
        for d in 0..3 {
            let mut strata: Vec<usize> = pts.iter().map(|p| (p[d] * 10.0).floor() as usize).collect();
            strata.sort_unstable();
            assert_eq!(strata, (0..10).collect::<Vec<_>>());
        }
    }
}
