//! Derivatives, integrals and interpolation of x–y data.
//!
//! All functions take positions `x` in strictly increasing order (spacing may
//! be uneven) and values `y` of the same length.
//!
//! * [`gradient`]: second-order central differences in the interior,
//!   `y′ᵢ ≈ (hₛ² yᵢ₊₁ + (h_d² − hₛ²) yᵢ − h_d² yᵢ₋₁) / (hₛ h_d (hₛ + h_d))` with
//!   `hₛ = xᵢ − xᵢ₋₁`, `h_d = xᵢ₊₁ − xᵢ`, and one-sided first differences at
//!   the ends (exact for quadratics in the interior; NumPy's `gradient`).
//! * [`cumulative_integral`]: the running integral from `x₀` (0 at the first
//!   point) by the trapezoid rule, or by Simpson's rule on each interval —
//!   the quadratic through the interval's two points and a neighbour, the
//!   next point for even-numbered intervals and the previous one for
//!   odd-numbered intervals and the last (Cartwright, "Simpson's rule
//!   cumulative integration with MS Excel and irregularly-spaced data",
//!   *J. Math. Sci. Math. Educ.* 12, 2017; SciPy's `cumulative_simpson`).
//! * [`Interpolator`]: piecewise linear; cubic spline (twice continuously
//!   differentiable) with *not-a-knot* end conditions (the third derivative
//!   also continuous at the second and second-last points) or *natural* ends
//!   (zero second derivative); or PCHIP, the shape-preserving piecewise cubic
//!   Hermite interpolant of Fritsch & Carlson ("Monotone piecewise cubic
//!   interpolation", *SIAM J. Numer. Anal.* 17, 1980) with the slopes of
//!   Fritsch & Butland (1984): zero where the data turn, otherwise a weighted
//!   harmonic mean of the neighbouring secant slopes, and three-point ends
//!   limited to keep monotone data monotone. Outside `[x₀, xₙ₋₁]` every
//!   interpolant gives `NaN` (no extrapolation).

use crate::{Result, invalid};

fn check_xy(x: &[f64], y: &[f64], min: usize) -> Result<()> {
    if x.len() != y.len() {
        return Err(invalid(format!("x has {} values and y has {}", x.len(), y.len())));
    }
    if x.len() < min {
        return Err(invalid(format!("at least {min} points are needed (got {})", x.len())));
    }
    if x.iter().chain(y).any(|v| !v.is_finite()) {
        return Err(invalid("x and y must not contain missing or infinite values"));
    }
    if x.windows(2).any(|w| w[1] <= w[0]) {
        return Err(invalid("x must be strictly increasing (sort the rows, and average repeated x values)"));
    }
    Ok(())
}

/// Numerical derivative `dy/dx` at every point (see the module documentation).
///
/// # Errors
/// [`crate::StatError::InvalidArgument`] for fewer than two points, lengths that
/// differ, missing values or `x` not strictly increasing.
pub fn gradient(x: &[f64], y: &[f64]) -> Result<Vec<f64>> {
    check_xy(x, y, 2)?;
    let n = x.len();
    let mut g = vec![0.0; n];
    g[0] = (y[1] - y[0]) / (x[1] - x[0]);
    g[n - 1] = (y[n - 1] - y[n - 2]) / (x[n - 1] - x[n - 2]);
    for i in 1..n - 1 {
        let hs = x[i] - x[i - 1];
        let hd = x[i + 1] - x[i];
        g[i] = (hs * hs * y[i + 1] + (hd * hd - hs * hs) * y[i] - hd * hd * y[i - 1]) / (hs * hd * (hd + hs));
    }
    Ok(g)
}

/// Rule for [`cumulative_integral`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Integration {
    /// Trapezoid rule.
    Trapezoid,
    /// Simpson's rule on each interval (trapezoid with only two points).
    Simpson,
}

impl Integration {
    /// `"trapezoid"` or `"Simpson"`.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Trapezoid => "trapezoid",
            Self::Simpson => "Simpson",
        }
    }
}

/// Integral over `[x₁, x₂]` of the quadratic through `(x₁, f₁)`, `(x₂, f₂)`,
/// `(x₃, f₃)`, with `h₂₁ = x₂ − x₁` and `h₃₂ = x₃ − x₂` (either sign of
/// direction, as long as both have the same sign).
fn simpson_interval(f1: f64, f2: f64, f3: f64, h21: f64, h32: f64) -> f64 {
    let h31 = h21 + h32;
    let a = h21 / h31;
    let b = a * (h21 / h32);
    h21 / 6.0 * ((3.0 - a) * f1 + (3.0 + b + a) * f2 - b * f3)
}

/// Running integral of `y` over `x`, 0 at the first point.
///
/// # Errors
/// As [`gradient`] (at least two points).
pub fn cumulative_integral(x: &[f64], y: &[f64], rule: Integration) -> Result<Vec<f64>> {
    check_xy(x, y, 2)?;
    let n = x.len();
    let intervals: Vec<f64> = if rule == Integration::Trapezoid || n < 3 {
        (0..n - 1).map(|i| 0.5 * (x[i + 1] - x[i]) * (y[i] + y[i + 1])).collect()
    } else {
        (0..n - 1)
            .map(|j| {
                let forward = j % 2 == 0 && j + 2 < n;
                if forward {
                    simpson_interval(y[j], y[j + 1], y[j + 2], x[j + 1] - x[j], x[j + 2] - x[j + 1])
                } else {
                    // The quadratic through points j − 1, j, j + 1, integrated
                    // over [x_j, x_{j+1}] (computed from the right).
                    simpson_interval(y[j + 1], y[j], y[j - 1], x[j + 1] - x[j], x[j] - x[j - 1])
                }
            })
            .collect()
    };
    let mut out = Vec::with_capacity(n);
    let mut total = 0.0;
    out.push(0.0);
    for v in intervals {
        total += v;
        out.push(total);
    }
    Ok(out)
}

/// Interpolation method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interpolation {
    /// Straight lines between points.
    Linear,
    /// Cubic spline with not-a-knot ends.
    CubicSpline,
    /// Cubic spline with natural ends (zero second derivative).
    NaturalSpline,
    /// Shape-preserving piecewise cubic Hermite (PCHIP).
    Pchip,
}

impl Interpolation {
    /// Every method.
    pub const ALL: [Self; 4] = [Self::Linear, Self::CubicSpline, Self::NaturalSpline, Self::Pchip];

    /// A short name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::CubicSpline => "Cubic spline",
            Self::NaturalSpline => "Natural cubic spline",
            Self::Pchip => "Shape-preserving cubic (PCHIP)",
        }
    }

    /// Identifier in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::CubicSpline => "spline",
            Self::NaturalSpline => "natural-spline",
            Self::Pchip => "pchip",
        }
    }

    /// The method with identifier `key` (underscores accepted).
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key.replace('_', "-"))
    }
}

/// A piecewise-polynomial interpolant through x–y points.
#[derive(Debug, Clone, PartialEq)]
pub struct Interpolator {
    x: Vec<f64>,
    y: Vec<f64>,
    /// Slope at each point (Hermite form), for every method but linear.
    slopes: Vec<f64>,
    method: Interpolation,
}

/// Second derivatives `M` of the cubic spline through `(x, y)`.
fn spline_second_derivatives(x: &[f64], y: &[f64], natural: bool) -> Vec<f64> {
    let n = x.len();
    let h: Vec<f64> = x.windows(2).map(|w| w[1] - w[0]).collect();
    let d: Vec<f64> = (0..n - 1).map(|i| (y[i + 1] - y[i]) / h[i]).collect();
    if n == 3 && !natural {
        // Not-a-knot with three points: the parabola through them.
        let m = 2.0 * (d[1] - d[0]) / (h[0] + h[1]);
        return vec![m; 3];
    }
    // Rows i = 1 … n−2: h_{i−1} M_{i−1} + 2(h_{i−1}+h_i) M_i + h_i M_{i+1} = 6(d_i − d_{i−1}).
    let k = n - 2;
    let mut sub = vec![0.0; k];
    let mut diag = vec![0.0; k];
    let mut sup = vec![0.0; k];
    let mut rhs = vec![0.0; k];
    for r in 0..k {
        let i = r + 1;
        sub[r] = h[i - 1];
        diag[r] = 2.0 * (h[i - 1] + h[i]);
        sup[r] = h[i];
        rhs[r] = 6.0 * (d[i] - d[i - 1]);
    }
    if !natural {
        // Not-a-knot: M₀ = ((h₀+h₁)M₁ − h₀M₂)/h₁, substituted into row 1;
        // likewise M_{n−1} from the last two.
        let (h0, h1) = (h[0], h[1]);
        diag[0] += sub[0] * (h0 + h1) / h1;
        sup[0] -= sub[0] * h0 / h1;
        let (ha, hb) = (h[n - 3], h[n - 2]);
        diag[k - 1] += sup[k - 1] * (ha + hb) / ha;
        sub[k - 1] -= sup[k - 1] * hb / ha;
    }
    // Thomas algorithm (the system is diagonally dominant for natural ends
    // and well conditioned for not-a-knot ones).
    for r in 1..k {
        let w = sub[r] / diag[r - 1];
        diag[r] -= w * sup[r - 1];
        rhs[r] -= w * rhs[r - 1];
    }
    let mut inner = vec![0.0; k];
    inner[k - 1] = rhs[k - 1] / diag[k - 1];
    for r in (0..k - 1).rev() {
        inner[r] = (rhs[r] - sup[r] * inner[r + 1]) / diag[r];
    }
    let mut m = vec![0.0; n];
    m[1..n - 1].copy_from_slice(&inner);
    if !natural {
        m[0] = ((h[0] + h[1]) * m[1] - h[0] * m[2]) / h[1];
        m[n - 1] = ((h[n - 3] + h[n - 2]) * m[n - 2] - h[n - 2] * m[n - 3]) / h[n - 3];
    }
    m
}

fn pchip_slopes(x: &[f64], y: &[f64]) -> Vec<f64> {
    let n = x.len();
    let h: Vec<f64> = x.windows(2).map(|w| w[1] - w[0]).collect();
    let d: Vec<f64> = (0..n - 1).map(|i| (y[i + 1] - y[i]) / h[i]).collect();
    if n == 2 {
        return vec![d[0]; 2];
    }
    let mut s = vec![0.0; n];
    for k in 1..n - 1 {
        if d[k - 1] == 0.0 || d[k] == 0.0 || d[k - 1].signum() != d[k].signum() {
            s[k] = 0.0;
        } else {
            let w1 = 2.0 * h[k] + h[k - 1];
            let w2 = h[k] + 2.0 * h[k - 1];
            s[k] = (w1 + w2) / (w1 / d[k - 1] + w2 / d[k]);
        }
    }
    let sign = |v: f64| (v > 0.0) as i8 - (v < 0.0) as i8;
    let edge = |h0: f64, h1: f64, d0: f64, d1: f64| {
        let e = ((2.0 * h0 + h1) * d0 - h0 * d1) / (h0 + h1);
        if sign(e) != sign(d0) {
            0.0
        } else if sign(d0) != sign(d1) && e.abs() > 3.0 * d0.abs() {
            3.0 * d0
        } else {
            e
        }
    };
    s[0] = edge(h[0], h[1], d[0], d[1]);
    s[n - 1] = edge(h[n - 2], h[n - 3], d[n - 2], d[n - 3]);
    s
}

impl Interpolator {
    /// The interpolant of `method` through `(x, y)`. Splines need at least
    /// three points; with two, every method is linear.
    ///
    /// # Errors
    /// As [`gradient`] (at least two points).
    pub fn new(x: &[f64], y: &[f64], method: Interpolation) -> Result<Self> {
        check_xy(x, y, 2)?;
        let n = x.len();
        let method = if n == 2 { Interpolation::Linear } else { method };
        let slopes = match method {
            Interpolation::Linear => Vec::new(),
            Interpolation::Pchip => pchip_slopes(x, y),
            Interpolation::CubicSpline | Interpolation::NaturalSpline => {
                let m = spline_second_derivatives(x, y, method == Interpolation::NaturalSpline);
                // Slope at each knot from the interval to its right (the last
                // from its left).
                (0..n)
                    .map(|i| {
                        if i + 1 < n {
                            let h = x[i + 1] - x[i];
                            (y[i + 1] - y[i]) / h - h * (2.0 * m[i] + m[i + 1]) / 6.0
                        } else {
                            let h = x[i] - x[i - 1];
                            (y[i] - y[i - 1]) / h + h * (m[i - 1] + 2.0 * m[i]) / 6.0
                        }
                    })
                    .collect()
            }
        };
        Ok(Self { x: x.to_vec(), y: y.to_vec(), slopes, method })
    }

    /// The method used.
    #[must_use]
    pub fn method(&self) -> Interpolation {
        self.method
    }

    /// Value at `t` (`NaN` outside the data's range).
    #[must_use]
    pub fn eval(&self, t: f64) -> f64 {
        let n = self.x.len();
        if !(t >= self.x[0] && t <= self.x[n - 1]) {
            return f64::NAN;
        }
        let i = match self.x.partition_point(|&v| v <= t) {
            0 => 0,
            p => (p - 1).min(n - 2),
        };
        let h = self.x[i + 1] - self.x[i];
        let u = (t - self.x[i]) / h;
        let (y0, y1) = (self.y[i], self.y[i + 1]);
        if self.method == Interpolation::Linear {
            return y0 + u * (y1 - y0);
        }
        let (s0, s1) = (self.slopes[i], self.slopes[i + 1]);
        let u2 = u * u;
        let u3 = u2 * u;
        (2.0 * u3 - 3.0 * u2 + 1.0) * y0
            + (u3 - 2.0 * u2 + u) * h * s0
            + (-2.0 * u3 + 3.0 * u2) * y1
            + (u3 - u2) * h * s1
    }
}

/// `n` evenly spaced points from `from` to `to` inclusive.
#[must_use]
pub fn linear_grid(from: f64, to: f64, n: usize) -> Vec<f64> {
    match n {
        0 => Vec::new(),
        1 => vec![from],
        _ => (0..n).map(|i| from + (to - from) * i as f64 / (n - 1) as f64).collect(),
    }
}

/// Values of the `method` interpolant through `(x, y)` at `grid`.
///
/// # Errors
/// As [`Interpolator::new`].
pub fn resample(x: &[f64], y: &[f64], grid: &[f64], method: Interpolation) -> Result<Vec<f64>> {
    let f = Interpolator::new(x, y, method)?;
    Ok(grid.iter().map(|&t| f.eval(t)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradient_is_exact_for_quadratics() {
        let x = [0.0, 0.5, 1.7, 2.0, 3.1];
        let y: Vec<f64> = x.iter().map(|t| 1.0 - 2.0 * t + 3.0 * t * t).collect();
        let g = gradient(&x, &y).unwrap();
        for i in 1..4 {
            assert!((g[i] - (-2.0 + 6.0 * x[i])).abs() < 1e-12);
        }
    }

    #[test]
    fn simpson_is_exact_for_quadratics() {
        let x = [0.0, 0.4, 1.0, 1.3, 2.2, 3.0];
        let y: Vec<f64> = x.iter().map(|t| 2.0 + t - 0.5 * t * t).collect();
        let c = cumulative_integral(&x, &y, Integration::Simpson).unwrap();
        for i in 0..x.len() {
            let t = x[i];
            assert!((c[i] - (2.0 * t + t * t / 2.0 - t * t * t / 6.0)).abs() < 1e-12, "{i}");
        }
    }

    #[test]
    fn splines_reproduce_cubics_and_interpolate() {
        let x = [0.0, 1.0, 1.5, 3.0, 4.0, 6.0];
        let f = |t: f64| 1.0 + t - 0.4 * t * t + 0.05 * t * t * t;
        let y: Vec<f64> = x.iter().map(|&t| f(t)).collect();
        let s = Interpolator::new(&x, &y, Interpolation::CubicSpline).unwrap();
        for t in [0.3, 1.2, 2.5, 5.9] {
            assert!((s.eval(t) - f(t)).abs() < 1e-12, "{t}");
        }
        for method in Interpolation::ALL {
            let s = Interpolator::new(&x, &y, method).unwrap();
            for (xi, yi) in x.iter().zip(&y) {
                assert!((s.eval(*xi) - yi).abs() < 1e-12);
            }
            assert!(s.eval(-0.1).is_nan() && s.eval(6.1).is_nan());
        }
    }

    #[test]
    fn pchip_keeps_monotone_data_monotone() {
        let x = [0.0, 1.0, 2.0, 3.0, 4.0];
        let y = [0.0, 0.1, 5.0, 5.1, 5.2];
        let s = Interpolator::new(&x, &y, Interpolation::Pchip).unwrap();
        let mut last = f64::NEG_INFINITY;
        for i in 0..=400 {
            let v = s.eval(i as f64 / 100.0);
            assert!(v >= last - 1e-12);
            last = v;
        }
    }
}
