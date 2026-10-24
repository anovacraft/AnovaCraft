//! Implicit models `F(x, y; θ) = 0` fitted by orthogonal distance.
//!
//! # Model
//! The observations `(xᵢ, yᵢ)` lie on the curve `F(x, y; θ) = 0` up to
//! independent errors of equal variance in both coordinates. The estimates
//! minimise the sum of squared orthogonal (geometric) distances
//! `Σ dᵢ(θ)²`, `dᵢ` the distance from `(xᵢ, yᵢ)` to the nearest point of the
//! curve (Boggs, Byrd & Schnabel, "A stable and efficient algorithm for
//! nonlinear orthogonal distance regression", *SIAM J. Sci. Stat. Comput.*
//! 8, 1987; Ahn, Rauh & Warnecke, "Least-squares orthogonal distances fitting
//! of circle, sphere, ellipse, hyperbola, and parabola", *Pattern
//! Recognition* 34, 2001). The model is passed to
//! [`fit_nonlinear`](super::fit_nonlinear) as a [`ModelFunction`] whose value
//! for observation `i` is the signed distance `dᵢ` (the sign of
//! `F(xᵢ, yᵢ)`), fitted to a response of zeros. The error of an observation
//! normal to the curve has the variance `σ²` of each coordinate, so
//! `s² = Σdᵢ²/(n − P)` estimates `σ²`, and the standard errors follow from
//! the Jacobian of the distances.
//!
//! # Foot points
//! The nearest point `q` of the curve to `p = (xᵢ, yᵢ)` starts as the Newton
//! projection of `p` onto the curve (`z ← z − F(z)∇F(z)/‖∇F(z)‖²` until
//! `|F|/‖∇F‖ ≤ 10⁻¹²·scale`). It then moves along the tangent `t` by the
//! tangential part of `p − q`, `(p − q)·t`, and is projected back onto the
//! curve; the move is halved until the distance falls, and the steps stop
//! when the tangential part is below `10⁻¹⁰·scale`. At the end `F(q) = 0`
//! and `p − q ∥ ∇F(q)`, the conditions for a stationary distance.
//! The gradient `∇F` is a central difference with step `ε^{1/3}·max(|q|,
//! scale)`, `scale` the spread of the data.
//!
//! # Failure modes
//! `F` undefined near an observation, a vanishing gradient (a singular point
//! of the curve), or a projection that does not converge make the model
//! uncomputable at that `θ` (the solver then shortens its step).

use super::ModelFunction;

/// `F(x, y; θ)` for observation `i` at the point `(x, y)`, or `None` where
/// undefined.
pub type ImplicitFunction<'a> = dyn FnMut(usize, f64, f64, &[f64]) -> Option<f64> + 'a;

/// Signed orthogonal distances to the curve `F = 0` as a [`ModelFunction`].
pub struct ImplicitModel<'a> {
    f: Box<ImplicitFunction<'a>>,
    x: &'a [f64],
    y: &'a [f64],
    scale: f64,
    /// Foot points at the last successful evaluation.
    feet: Vec<[f64; 2]>,
}

impl std::fmt::Debug for ImplicitModel<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImplicitModel").field("n", &self.x.len()).field("scale", &self.scale).finish_non_exhaustive()
    }
}

impl<'a> ImplicitModel<'a> {
    /// The curve `f = 0` and data `(x, y)` (same length).
    #[must_use]
    pub fn new(f: Box<ImplicitFunction<'a>>, x: &'a [f64], y: &'a [f64]) -> Self {
        let spread = |v: &[f64]| {
            let (lo, hi) = v.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &t| (a.min(t), b.max(t)));
            if hi > lo { hi - lo } else { 0.0 }
        };
        let scale = spread(x).max(spread(y)).max(1e-8);
        Self { f, x, y, scale, feet: Vec::new() }
    }

    /// `F` and its central-difference gradient at `q`.
    fn value_and_gradient(&mut self, i: usize, q: [f64; 2], theta: &[f64]) -> Option<(f64, [f64; 2])> {
        let eps = f64::EPSILON.cbrt();
        let f = &mut self.f;
        let fq = f(i, q[0], q[1], theta)?;
        let hx = eps * q[0].abs().max(self.scale);
        let hy = eps * q[1].abs().max(self.scale);
        let gx = (f(i, q[0] + hx, q[1], theta)? - f(i, q[0] - hx, q[1], theta)?) / (2.0 * hx);
        let gy = (f(i, q[0], q[1] + hy, theta)? - f(i, q[0], q[1] - hy, theta)?) / (2.0 * hy);
        let g2 = gx * gx + gy * gy;
        (fq.is_finite() && g2 > 0.0 && g2.is_finite()).then_some((fq, [gx, gy]))
    }

    /// Newton projection of `z` onto the curve: `z ← z − F∇F/‖∇F‖²` until
    /// `|F|/‖∇F‖ ≤ tol`.
    fn project(&mut self, i: usize, mut z: [f64; 2], theta: &[f64], tol: f64) -> Option<([f64; 2], [f64; 2])> {
        for _ in 0..60 {
            let (fz, g) = self.value_and_gradient(i, z, theta)?;
            let g2 = g[0] * g[0] + g[1] * g[1];
            if fz.abs() <= tol * g2.sqrt() {
                return Some((z, g));
            }
            z = [z[0] - fz * g[0] / g2, z[1] - fz * g[1] / g2];
            if !(z[0].is_finite() && z[1].is_finite()) {
                return None;
            }
        }
        None
    }

    /// The foot point of observation `i` (nearest point of the curve) and
    /// the signed distance.
    pub fn foot_point(&mut self, i: usize, theta: &[f64]) -> Option<([f64; 2], f64)> {
        let p = [self.x[i], self.y[i]];
        let fp = (self.f)(i, p[0], p[1], theta)?;
        if !fp.is_finite() {
            return None;
        }
        let tol = 1e-12 * self.scale;
        let dist = |q: [f64; 2]| (p[0] - q[0]).hypot(p[1] - q[1]);
        let (mut q, mut g) = self.project(i, p, theta, tol)?;
        for _ in 0..200 {
            let norm = g[0].hypot(g[1]);
            let t = [-g[1] / norm, g[0] / norm];
            let along = (p[0] - q[0]) * t[0] + (p[1] - q[1]) * t[1];
            if along.abs() <= 1e-10 * self.scale {
                break;
            }
            // Move along the tangent and back onto the curve, halving the
            // move until the distance falls.
            let d0 = dist(q);
            let mut alpha = 1.0;
            let mut accepted = None;
            for _ in 0..40 {
                let z = [q[0] + alpha * along * t[0], q[1] + alpha * along * t[1]];
                if let Some((c, gc)) = self.project(i, z, theta, tol) {
                    if dist(c) < d0 {
                        accepted = Some((c, gc));
                        break;
                    }
                }
                alpha /= 2.0;
            }
            let Some((c, gc)) = accepted else { break };
            q = c;
            g = gc;
        }
        let d = dist(q);
        Some((q, if fp < 0.0 { -d } else { d }))
    }
}

impl ImplicitModel<'_> {
    /// Foot points of the observations at the last computable evaluation.
    #[must_use]
    pub fn feet(&self) -> &[[f64; 2]] {
        &self.feet
    }
}

impl ModelFunction for ImplicitModel<'_> {
    fn n_observations(&self) -> usize {
        self.x.len()
    }

    fn values(&mut self, theta: &[f64], out: &mut [f64]) -> bool {
        let mut feet = Vec::with_capacity(self.x.len());
        for (i, o) in out.iter_mut().enumerate() {
            let Some((q, d)) = self.foot_point(i, theta) else { return false };
            feet.push(q);
            *o = d;
        }
        self.feet = feet;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_to_a_circle_is_exact() {
        let x = [3.0, 0.0, -1.0, 0.5];
        let y = [4.0, 0.5, 0.0, 2.5];
        let circle = |_: usize, x: f64, y: f64, t: &[f64]| Some((x - t[0]).powi(2) + (y - t[1]).powi(2) - t[2] * t[2]);
        let mut m = ImplicitModel::new(Box::new(circle), &x, &y);
        let theta = [0.0, 0.0, 2.0];
        let mut out = [0.0; 4];
        assert!(m.values(&theta, &mut out));
        for i in 0..4 {
            let exact = (x[i].hypot(y[i])) - 2.0;
            assert!((out[i] - exact).abs() < 1e-10, "{i}: {} vs {exact}", out[i]);
        }
    }
}
