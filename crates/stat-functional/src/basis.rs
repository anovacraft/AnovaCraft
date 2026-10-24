//! Basis systems for curves: B-splines and Fourier series.
//!
//! A curve is represented as `x(t) = Σⱼ cⱼ φⱼ(t)` on a closed interval
//! `[a, b]`. Smoothing, registration and principal components only need
//! the basis values and derivatives at given times and two matrices:
//!
//! * the **Gram matrix** `Wⱼₖ = ∫ φⱼ φₖ dt`, the inner products of the basis
//!   functions, so `∫ x y dt = cₓᵀ W c_y`;
//! * the **roughness penalty** `Rⱼₖ = ∫ (Lφⱼ)(Lφₖ) dt` of a linear
//!   differential operator `L = Σₘ aₘ Dᵐ`, so `∫ (Lx)² dt = cᵀ R c`. The usual
//!   choice is `L = D²` (curvature); for periodic data the harmonic
//!   acceleration operator `L = ω²D + D³`, `ω = 2π/T`, annihilates constants
//!   and the first harmonic, so a heavily penalized fit tends to a sinusoid
//!   of period `T` instead of a constant (Ramsay & Silverman 2005, §5.2.4).
//!
//! Both integrals are evaluated by Gauss–Legendre quadrature. For B-splines
//! the panels are the knot intervals, inside which the integrands are
//! polynomials of degree at most `2(k − 1)`, so an `(k + 1)`-point rule is
//! exact; for Fourier bases `8(M + 1)` equal panels of 12 points each give
//! full double precision.
//!
//! # B-splines
//! Order `k` (degree `k − 1`) B-splines on the knot vector
//! `a = … = a < τ₁ < … < τ_q < b = … = b` (each end repeated `k` times),
//! `n = q + k` functions, evaluated with their derivatives by the
//! Cox–de Boor recursion in the form of Piegl & Tiller, *The NURBS Book*,
//! 2nd ed., Algorithm A2.3. Order 4 (cubic) is the default. The basis is a
//! partition of unity: `Σⱼ φⱼ(t) = 1`.
//!
//! # Fourier series
//! For period `T` and `n = 2M + 1` functions:
//! `φ₀ = 1/√T`, `φ₂ₘ₋₁ = sin(mω(t − a))/√(T/2)`, `φ₂ₘ = cos(mω(t − a))/√(T/2)`,
//! orthonormal over one period.

use stat_core::quadrature::gauss_legendre;
use stat_core::{Result, StatError};
use std::f64::consts::PI;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// The roughness a smoothing penalty measures.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Roughness {
    /// `∫ (Dᵐx)² dt`; `m = 2` (curvature) is the usual choice.
    Derivative(usize),
    /// `∫ (ω²Dx + D³x)² dt` with `ω = 2π/T` (periodic data); `T` is the
    /// given period.
    HarmonicAcceleration {
        /// Period `T`.
        period: f64,
    },
}

impl Roughness {
    /// Operator coefficients `aₘ` of `L = Σₘ aₘ Dᵐ`, indexed by derivative order.
    #[must_use]
    pub fn operator(&self) -> Vec<f64> {
        match *self {
            Self::Derivative(m) => {
                let mut a = vec![0.0; m + 1];
                a[m] = 1.0;
                a
            }
            Self::HarmonicAcceleration { period } => {
                let w = 2.0 * PI / period;
                vec![0.0, w * w, 0.0, 1.0]
            }
        }
    }
}

/// B-spline basis of a given order on a knot sequence.
#[derive(Debug, Clone, PartialEq)]
pub struct BSplineBasis {
    /// Full knot vector: each end repeated `order` times.
    knots: Vec<f64>,
    order: usize,
}

impl BSplineBasis {
    /// `n_basis` B-splines of `order` on `[a, b]` with equally spaced
    /// interior knots (`n_basis − order` of them).
    ///
    /// # Errors
    /// An empty or non-finite range, order 0, or `n_basis < order`.
    pub fn equally_spaced(range: (f64, f64), n_basis: usize, order: usize) -> Result<Self> {
        if order == 0 {
            return Err(invalid("a B-spline basis needs an order of at least 1"));
        }
        if n_basis < order {
            return Err(invalid(format!("a B-spline basis of order {order} needs at least {order} functions")));
        }
        let (a, b) = range;
        let q = n_basis - order;
        let breaks: Vec<f64> = (0..=q + 1).map(|i| a + (b - a) * i as f64 / (q + 1) as f64).collect();
        Self::with_breaks(&breaks, order)
    }

    /// B-splines of `order` on the strictly increasing `breaks` (the two
    /// ends and the interior knots).
    ///
    /// # Errors
    /// Fewer than two breaks, breaks not strictly increasing or not finite,
    /// or order 0.
    pub fn with_breaks(breaks: &[f64], order: usize) -> Result<Self> {
        if order == 0 {
            return Err(invalid("a B-spline basis needs an order of at least 1"));
        }
        if breaks.len() < 2 || breaks.iter().any(|b| !b.is_finite()) || breaks.windows(2).any(|w| w[1] <= w[0]) {
            return Err(invalid("B-spline breaks must be finite and strictly increasing, with at least two"));
        }
        let mut knots = vec![breaks[0]; order - 1];
        knots.extend_from_slice(breaks);
        knots.extend(std::iter::repeat_n(breaks[breaks.len() - 1], order - 1));
        Ok(Self { knots, order })
    }

    /// Order (degree + 1).
    #[must_use]
    pub fn order(&self) -> usize {
        self.order
    }

    /// Number of basis functions.
    #[must_use]
    pub fn n_basis(&self) -> usize {
        self.knots.len() - self.order
    }

    /// The distinct knots, ends included.
    #[must_use]
    pub fn breaks(&self) -> Vec<f64> {
        let mut b = self.knots[self.order - 1..=self.n_basis()].to_vec();
        b.dedup();
        b
    }

    /// `[a, b]`.
    #[must_use]
    pub fn range(&self) -> (f64, f64) {
        (self.knots[0], self.knots[self.knots.len() - 1])
    }

    /// Values of the derivatives `0..=deriv` of every basis function at `t`
    /// (`out[d][j]`); `t` is clamped to the range.
    #[must_use]
    pub fn derivatives(&self, t: f64, deriv: usize) -> Vec<Vec<f64>> {
        let n = self.n_basis();
        let p = self.order - 1;
        let (a, b) = self.range();
        let t = t.clamp(a, b);
        // Knot span: knots[i] ≤ t < knots[i + 1], the last span at t = b.
        let mut i = p;
        while i + 1 < n && self.knots[i + 1] <= t {
            i += 1;
        }
        let d = deriv.min(p);
        let local = ders_basis_funs(i, t, p, d, &self.knots);
        let mut out = vec![vec![0.0; n]; deriv + 1];
        for (k, row) in local.iter().enumerate() {
            for (j, v) in row.iter().enumerate() {
                out[k][i - p + j] = *v;
            }
        }
        out
    }
}

/// Piegl & Tiller Algorithm A2.3: the nonzero basis functions of degree `p`
/// at `u` in span `i` and their derivatives up to `n ≤ p` (`ders[k][j]` for
/// function `i − p + j`).
fn ders_basis_funs(i: usize, u: f64, p: usize, n: usize, knots: &[f64]) -> Vec<Vec<f64>> {
    let mut ndu = vec![vec![0.0; p + 1]; p + 1];
    let mut left = vec![0.0; p + 1];
    let mut right = vec![0.0; p + 1];
    ndu[0][0] = 1.0;
    for j in 1..=p {
        left[j] = u - knots[i + 1 - j];
        right[j] = knots[i + j] - u;
        let mut saved = 0.0;
        for r in 0..j {
            ndu[j][r] = right[r + 1] + left[j - r];
            let temp = ndu[r][j - 1] / ndu[j][r];
            ndu[r][j] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        ndu[j][j] = saved;
    }
    let mut ders = vec![vec![0.0; p + 1]; n + 1];
    for j in 0..=p {
        ders[0][j] = ndu[j][p];
    }
    let pi = p as isize;
    for r in 0..=pi {
        let mut a = [vec![0.0; p + 1], vec![0.0; p + 1]];
        let (mut s1, mut s2) = (0usize, 1usize);
        a[0][0] = 1.0;
        for k in 1..=n as isize {
            let mut d = 0.0;
            let rk = r - k;
            let pk = pi - k;
            if r >= k {
                a[s2][0] = a[s1][0] / ndu[(pk + 1) as usize][rk as usize];
                d = a[s2][0] * ndu[rk as usize][pk as usize];
            }
            let j1 = if rk >= -1 { 1 } else { -rk };
            let j2 = if r - 1 <= pk { k - 1 } else { pi - r };
            for j in j1..=j2 {
                let (ju, rkj) = (j as usize, (rk + j) as usize);
                a[s2][ju] = (a[s1][ju] - a[s1][ju - 1]) / ndu[(pk + 1) as usize][rkj];
                d += a[s2][ju] * ndu[rkj][pk as usize];
            }
            if r <= pk {
                let ku = k as usize;
                a[s2][ku] = -a[s1][ku - 1] / ndu[(pk + 1) as usize][r as usize];
                d += a[s2][ku] * ndu[r as usize][pk as usize];
            }
            ders[k as usize][r as usize] = d;
            std::mem::swap(&mut s1, &mut s2);
        }
    }
    let mut factor = p as f64;
    for k in 1..=n {
        for v in &mut ders[k] {
            *v *= factor;
        }
        factor *= (p - k) as f64;
    }
    ders
}

/// Fourier basis of odd size on `[a, b]` with a given period.
#[derive(Debug, Clone, PartialEq)]
pub struct FourierBasis {
    range: (f64, f64),
    n_basis: usize,
    period: f64,
}

impl FourierBasis {
    /// `n_basis` Fourier functions (rounded up to an odd number) of
    /// `period` on `range`.
    ///
    /// # Errors
    /// An empty or non-finite range, `n_basis` 0 or a non-positive period.
    pub fn new(range: (f64, f64), n_basis: usize, period: f64) -> Result<Self> {
        let (a, b) = range;
        if !(a.is_finite() && b.is_finite() && b > a) {
            return Err(invalid("a Fourier basis needs a finite range with a < b"));
        }
        if n_basis == 0 {
            return Err(invalid("a Fourier basis needs at least one function"));
        }
        if !(period.is_finite() && period > 0.0) {
            return Err(invalid("the period of a Fourier basis must be positive"));
        }
        Ok(Self { range, n_basis: n_basis | 1, period })
    }

    /// Number of basis functions (odd).
    #[must_use]
    pub fn n_basis(&self) -> usize {
        self.n_basis
    }

    /// The period.
    #[must_use]
    pub fn period(&self) -> f64 {
        self.period
    }

    /// `[a, b]`.
    #[must_use]
    pub fn range(&self) -> (f64, f64) {
        self.range
    }

    /// Values of the derivatives `0..=deriv` of every basis function at `t`.
    #[must_use]
    pub fn derivatives(&self, t: f64, deriv: usize) -> Vec<Vec<f64>> {
        let n = self.n_basis;
        let omega = 2.0 * PI / self.period;
        let s = t - self.range.0;
        let c0 = 1.0 / self.period.sqrt();
        let c = 1.0 / (self.period / 2.0).sqrt();
        let mut out = vec![vec![0.0; n]; deriv + 1];
        out[0][0] = c0;
        for m in 1..=n / 2 {
            let f = m as f64 * omega;
            let x = f * s;
            for (d, row) in out.iter_mut().enumerate() {
                let shift = d as f64 * PI / 2.0;
                let scale = c * f.powi(d as i32);
                row[2 * m - 1] = scale * (x + shift).sin();
                row[2 * m] = scale * (x + shift).cos();
            }
        }
        out
    }
}

/// A basis system for curves.
#[derive(Debug, Clone, PartialEq)]
pub enum Basis {
    /// B-splines.
    BSpline(BSplineBasis),
    /// Fourier series.
    Fourier(FourierBasis),
}

impl Basis {
    /// Number of basis functions.
    #[must_use]
    pub fn n_basis(&self) -> usize {
        match self {
            Self::BSpline(b) => b.n_basis(),
            Self::Fourier(f) => f.n_basis(),
        }
    }

    /// `[a, b]`.
    #[must_use]
    pub fn range(&self) -> (f64, f64) {
        match self {
            Self::BSpline(b) => b.range(),
            Self::Fourier(f) => f.range(),
        }
    }

    /// Derivatives `0..=deriv` of every basis function at `t` (`out[d][j]`).
    #[must_use]
    pub fn derivatives(&self, t: f64, deriv: usize) -> Vec<Vec<f64>> {
        match self {
            Self::BSpline(b) => b.derivatives(t, deriv),
            Self::Fourier(f) => f.derivatives(t, deriv),
        }
    }

    /// Values of derivative `deriv` of every basis function at `t`.
    #[must_use]
    pub fn evaluate(&self, t: f64, deriv: usize) -> Vec<f64> {
        self.derivatives(t, deriv).swap_remove(deriv)
    }

    /// Quadrature nodes and weights over `[a, b]` that integrate products of
    /// basis functions (and their derivatives) exactly or to full precision.
    #[must_use]
    pub fn quadrature(&self) -> (Vec<f64>, Vec<f64>) {
        let (panels, points) = match self {
            Self::BSpline(b) => (b.breaks(), b.order() + 1),
            Self::Fourier(f) => {
                let (a, bb) = f.range();
                let m = 8 * (f.n_basis() / 2 + 1);
                ((0..=m).map(|i| a + (bb - a) * i as f64 / m as f64).collect(), 12)
            }
        };
        let (x, w) = gauss_legendre(points);
        let mut nodes = Vec::with_capacity((panels.len() - 1) * points);
        let mut weights = Vec::with_capacity(nodes.capacity());
        for p in panels.windows(2) {
            let (mid, half) = (0.5 * (p[0] + p[1]), 0.5 * (p[1] - p[0]));
            for (xi, wi) in x.iter().zip(&w) {
                nodes.push(mid + half * xi);
                weights.push(half * wi);
            }
        }
        (nodes, weights)
    }

    /// `∫ (Lφⱼ)(Lφₖ) dt` for the operator `L = Σₘ aₘ Dᵐ` with coefficients
    /// `operator` (`[1.0]` gives the Gram matrix).
    #[must_use]
    pub fn penalty_matrix(&self, operator: &[f64]) -> Vec<Vec<f64>> {
        let n = self.n_basis();
        let top = operator.len().saturating_sub(1);
        let (nodes, weights) = self.quadrature();
        let mut r = vec![vec![0.0; n]; n];
        for (t, w) in nodes.iter().zip(&weights) {
            let d = self.derivatives(*t, top);
            let l: Vec<f64> = (0..n)
                .map(|j| operator.iter().enumerate().map(|(m, a)| if *a == 0.0 { 0.0 } else { a * d[m][j] }).sum())
                .collect();
            for j in 0..n {
                if l[j] == 0.0 {
                    continue;
                }
                for k in j..n {
                    r[j][k] += w * l[j] * l[k];
                }
            }
        }
        for j in 0..n {
            for k in 0..j {
                r[j][k] = r[k][j];
            }
        }
        r
    }

    /// The Gram matrix `∫ φⱼ φₖ dt`.
    #[must_use]
    pub fn gram(&self) -> Vec<Vec<f64>> {
        self.penalty_matrix(&[1.0])
    }

    /// `∫ φⱼ dt` for every basis function.
    #[must_use]
    pub fn integrals(&self) -> Vec<f64> {
        let (nodes, weights) = self.quadrature();
        let mut out = vec![0.0; self.n_basis()];
        for (t, w) in nodes.iter().zip(&weights) {
            for (o, v) in out.iter_mut().zip(self.evaluate(*t, 0)) {
                *o += w * v;
            }
        }
        out
    }

    /// Short description, e.g. "cubic B-splines (12 functions)".
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::BSpline(b) => {
                let kind = match b.order() {
                    1 => "constant".to_owned(),
                    2 => "linear".to_owned(),
                    3 => "quadratic".to_owned(),
                    4 => "cubic".to_owned(),
                    k => format!("order {k}"),
                };
                format!("{kind} B-splines ({} functions, {} interior knots)", b.n_basis(), b.breaks().len() - 2)
            }
            Self::Fourier(f) => format!("Fourier series ({} functions, period {})", f.n_basis(), short(f.period())),
        }
    }
}

fn short(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bsplines_partition_unity_and_derivatives_match_differences() {
        let b = Basis::BSpline(BSplineBasis::equally_spaced((0.0, 10.0), 9, 4).unwrap());
        for &t in &[0.0, 0.3, 2.5, 5.0, 7.77, 10.0] {
            let v = b.evaluate(t, 0);
            assert!((v.iter().sum::<f64>() - 1.0).abs() < 1e-14, "partition of unity at {t}");
            assert!(v.iter().all(|x| *x >= -1e-15));
        }
        let h = 1e-5;
        for &t in &[0.7, 3.3, 6.1, 9.2] {
            let d1 = b.evaluate(t, 1);
            let d2 = b.evaluate(t, 2);
            let (lo, hi) = (b.evaluate(t - h, 0), b.evaluate(t + h, 0));
            let (dlo, dhi) = (b.evaluate(t - h, 1), b.evaluate(t + h, 1));
            for j in 0..9 {
                assert!((d1[j] - (hi[j] - lo[j]) / (2.0 * h)).abs() < 1e-7);
                assert!((d2[j] - (dhi[j] - dlo[j]) / (2.0 * h)).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn gram_of_linear_splines_is_exact() {
        // Hat functions on breaks 0, 1, 2: ∫φ₀² = 1/3, ∫φ₀φ₁ = 1/6, ∫φ₁² = 2/3.
        let b = Basis::BSpline(BSplineBasis::with_breaks(&[0.0, 1.0, 2.0], 2).unwrap());
        let g = b.gram();
        let want = [[1.0 / 3.0, 1.0 / 6.0, 0.0], [1.0 / 6.0, 2.0 / 3.0, 1.0 / 6.0], [0.0, 1.0 / 6.0, 1.0 / 3.0]];
        for j in 0..3 {
            for k in 0..3 {
                assert!((g[j][k] - want[j][k]).abs() < 1e-15);
            }
        }
        assert!((b.integrals().iter().sum::<f64>() - 2.0).abs() < 1e-15);
    }

    #[test]
    fn fourier_is_orthonormal_and_harmonic_penalty_is_diagonal() {
        let b = Basis::Fourier(FourierBasis::new((0.0, 365.0), 7, 365.0).unwrap());
        let g = b.gram();
        for j in 0..7 {
            for k in 0..7 {
                let want = if j == k { 1.0 } else { 0.0 };
                assert!((g[j][k] - want).abs() < 1e-12, "{j} {k} {}", g[j][k]);
            }
        }
        let r = b.penalty_matrix(&Roughness::HarmonicAcceleration { period: 365.0 }.operator());
        let w = 2.0 * PI / 365.0;
        for m in 1..=3usize {
            let f = m as f64;
            let want = f * f * w.powi(6) * (1.0 - f * f).powi(2);
            for j in [2 * m - 1, 2 * m] {
                assert!((r[j][j] - want).abs() <= 1e-10 * want.max(1e-30) + 1e-25, "{m}: {} vs {want}", r[j][j]);
            }
        }
        assert!(r[0][0].abs() < 1e-25 && r[1][1].abs() < 1e-25, "constant and first harmonic are not penalized");
    }
}
