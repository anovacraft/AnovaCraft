//! Analysis of a fitted second-order response surface (Box & Draper,
//! *Response Surfaces, Mixtures, and Ridge Analyses*, 2nd ed., ch. 10–11;
//! Myers, Montgomery & Anderson-Cook, *Response Surface Methodology*, ch. 6).
//!
//! In coded factors `x` the fitted model is
//!
//! ```text
//! ŷ(x) = b₀ + xᵀb + xᵀBx,
//! ```
//!
//! with `b` the first-order coefficients and `B` symmetric, holding the pure
//! quadratic coefficients on its diagonal and half of each two-factor
//! interaction coefficient off it.
//!
//! * **Stationary point** — where the gradient `b + 2Bx` vanishes:
//!   `xₛ = −½ B⁻¹ b`, with `ŷₛ = b₀ + ½ xₛᵀb`.
//! * **Canonical analysis** — with `B = VΛVᵀ` (eigenvalues λᵢ, orthonormal
//!   eigenvectors in the columns of `V`) and `w = Vᵀ(x − xₛ)`, the surface is
//!   `ŷ = ŷₛ + Σ λᵢ wᵢ²`: all λᵢ < 0 is a maximum, all > 0 a minimum, mixed
//!   signs a saddle; a λᵢ near zero means the response hardly changes along
//!   that axis (a ridge).
//! * **Ridge analysis** (Hoerl 1959; Draper 1963) — the point of greatest (or
//!   least) predicted response on the sphere `‖x‖ = R`, by Lagrange
//!   multipliers `(B − μI)x = −½ b`, with `μ > λ_max` for the maximum and
//!   `μ < λ_min` for the minimum. `‖x(μ)‖` is monotone in `μ` on each branch,
//!   so `μ` is found by bisection. When `b` has no component along the extreme
//!   eigenvector, the path reaches a finite radius as `μ → λ`; beyond it the
//!   path continues along that eigenvector (the "hard case").

use crate::optimal::ModelFormula;
use stat_core::{Result, StatError};

/// Relative size below which an eigenvalue counts as zero.
const SINGULAR_TOLERANCE: f64 = 1e-10;

/// A second-order polynomial `b₀ + xᵀb + xᵀBx` in `k` coded factors.
#[derive(Debug, Clone, PartialEq)]
pub struct QuadraticSurface {
    intercept: f64,
    linear: Vec<f64>,
    quadratic: Vec<Vec<f64>>,
}

/// Shape of the surface at its stationary point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StationaryKind {
    /// Every eigenvalue negative.
    Maximum,
    /// Every eigenvalue positive.
    Minimum,
    /// Eigenvalues of both signs.
    Saddle,
    /// `B` is singular: there is no unique stationary point.
    Undetermined,
}

impl StationaryKind {
    /// Readable name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Maximum => "Maximum",
            Self::Minimum => "Minimum",
            Self::Saddle => "Saddle point",
            Self::Undetermined => "No unique stationary point",
        }
    }
}

/// Result of [`QuadraticSurface::canonical_analysis`].
#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalAnalysis {
    /// Stationary point in coded units (`None` when `B` is singular).
    pub stationary_point: Option<Vec<f64>>,
    /// Predicted response at the stationary point.
    pub stationary_value: Option<f64>,
    /// Eigenvalues of `B`, largest first.
    pub eigenvalues: Vec<f64>,
    /// `eigenvectors[i]` is the unit eigenvector of `eigenvalues[i]`, its
    /// largest component made positive.
    pub eigenvectors: Vec<Vec<f64>>,
    /// Shape at the stationary point.
    pub kind: StationaryKind,
}

/// Direction of a ridge path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RidgeDirection {
    /// Greatest predicted response at each radius.
    Maximum,
    /// Least predicted response at each radius.
    Minimum,
}

/// One point of a ridge path.
#[derive(Debug, Clone, PartialEq)]
pub struct RidgePoint {
    /// Distance from the design centre, in coded units.
    pub radius: f64,
    /// The point, in coded units.
    pub x: Vec<f64>,
    /// Predicted response there.
    pub value: f64,
}

impl QuadraticSurface {
    /// Build from `b₀`, `b` and a symmetric `B` (given as `k` rows).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] when the sizes disagree, `B` is not
    /// symmetric, or a value is not finite.
    pub fn new(intercept: f64, linear: Vec<f64>, quadratic: Vec<Vec<f64>>) -> Result<Self> {
        let k = linear.len();
        if quadratic.len() != k || quadratic.iter().any(|r| r.len() != k) {
            return Err(invalid("B must be k × k, with k the number of first-order coefficients"));
        }
        let finite = intercept.is_finite()
            && linear.iter().all(|v| v.is_finite())
            && quadratic.iter().flatten().all(|v| v.is_finite());
        if !finite {
            return Err(invalid("surface coefficients must be finite"));
        }
        let scale = quadratic.iter().flatten().fold(0.0_f64, |m, v| m.max(v.abs())).max(1.0);
        for i in 0..k {
            for j in 0..i {
                if (quadratic[i][j] - quadratic[j][i]).abs() > 1e-12 * scale {
                    return Err(invalid("B must be symmetric"));
                }
            }
        }
        Ok(Self { intercept, linear, quadratic })
    }

    /// The surface whose coefficients `coefficients` follow the terms of a
    /// second-order `formula` (intercept first; terms absent from the formula
    /// are zero).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] when the lengths differ or the formula has
    /// a term of degree three.
    pub fn from_formula(formula: &ModelFormula, coefficients: &[f64]) -> Result<Self> {
        if coefficients.len() != formula.n_terms() {
            return Err(invalid("one coefficient per model term is required"));
        }
        let k = formula.n_factors();
        let (mut b0, mut b, mut bb) = (0.0, vec![0.0; k], vec![vec![0.0; k]; k]);
        for (term, &c) in formula.terms().iter().zip(coefficients) {
            let used: Vec<(usize, u8)> =
                term.powers.iter().enumerate().filter(|(_, p)| **p > 0).map(|(j, &p)| (j, p)).collect();
            match used.as_slice() {
                [] => b0 += c,
                [(j, 1)] => b[*j] += c,
                [(j, 2)] => bb[*j][*j] += c,
                [(i, 1), (j, 1)] => {
                    bb[*i][*j] += 0.5 * c;
                    bb[*j][*i] += 0.5 * c;
                }
                _ => return Err(invalid("only terms up to second order are supported")),
            }
        }
        Self::new(b0, b, bb)
    }

    /// Number of factors.
    #[must_use]
    pub fn n_factors(&self) -> usize {
        self.linear.len()
    }

    /// `b₀`.
    #[must_use]
    pub fn intercept(&self) -> f64 {
        self.intercept
    }

    /// `b`.
    #[must_use]
    pub fn linear(&self) -> &[f64] {
        &self.linear
    }

    /// `B`, as rows.
    #[must_use]
    pub fn quadratic(&self) -> &[Vec<f64>] {
        &self.quadratic
    }

    /// `ŷ(x)` at coded point `x` (which must have `k` values).
    #[must_use]
    pub fn value(&self, x: &[f64]) -> f64 {
        let mut v = self.intercept;
        for (i, &xi) in x.iter().enumerate() {
            v += self.linear[i] * xi;
            for (j, &xj) in x.iter().enumerate() {
                v += xi * self.quadratic[i][j] * xj;
            }
        }
        v
    }

    /// Stationary point, eigen-decomposition of `B` and the shape there.
    #[must_use]
    pub fn canonical_analysis(&self) -> CanonicalAnalysis {
        let (eigenvalues, eigenvectors) = symmetric_eigen(&self.quadratic);
        let biggest = eigenvalues.iter().fold(0.0_f64, |m, l| m.max(l.abs()));
        let singular = biggest == 0.0 || eigenvalues.iter().any(|l| l.abs() <= SINGULAR_TOLERANCE * biggest);
        let (stationary_point, stationary_value, kind) = if singular {
            (None, None, StationaryKind::Undetermined)
        } else {
            // xₛ = −½ V Λ⁻¹ Vᵀ b.
            let c: Vec<f64> = eigenvectors.iter().map(|v| dot(v, &self.linear)).collect();
            let mut xs = vec![0.0; self.n_factors()];
            for ((v, &ci), &l) in eigenvectors.iter().zip(&c).zip(&eigenvalues) {
                for (x, &vj) in xs.iter_mut().zip(v) {
                    *x -= 0.5 * ci / l * vj;
                }
            }
            let ys = self.intercept + 0.5 * dot(&xs, &self.linear);
            let kind = if eigenvalues.iter().all(|&l| l < 0.0) {
                StationaryKind::Maximum
            } else if eigenvalues.iter().all(|&l| l > 0.0) {
                StationaryKind::Minimum
            } else {
                StationaryKind::Saddle
            };
            (Some(xs), Some(ys), kind)
        };
        CanonicalAnalysis { stationary_point, stationary_value, eigenvalues, eigenvectors, kind }
    }

    /// The ridge path: at each radius in `radii` (coded units, non-negative),
    /// the point on the sphere of that radius with the greatest
    /// ([`RidgeDirection::Maximum`]) or least predicted response.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for a negative or non-finite radius.
    pub fn ridge_path(&self, direction: RidgeDirection, radii: &[f64]) -> Result<Vec<RidgePoint>> {
        if radii.iter().any(|r| !(r.is_finite() && *r >= 0.0)) {
            return Err(invalid("ridge radii must be finite and non-negative"));
        }
        // The minimum ridge of ŷ is the maximum ridge of −ŷ.
        let sign = match direction {
            RidgeDirection::Maximum => 1.0,
            RidgeDirection::Minimum => -1.0,
        };
        let b: Vec<f64> = self.linear.iter().map(|v| sign * v).collect();
        let bb: Vec<Vec<f64>> = self.quadratic.iter().map(|r| r.iter().map(|v| sign * v).collect()).collect();
        let (lambda, vectors) = symmetric_eigen(&bb);
        let Some(&top) = lambda.first() else {
            // No factors: the surface is the constant b₀.
            return Ok(radii
                .iter()
                .map(|&radius| RidgePoint { radius, x: Vec::new(), value: self.intercept })
                .collect());
        };
        let c: Vec<f64> = vectors.iter().map(|v| dot(v, &b)).collect();
        let scale =
            lambda.iter().fold(0.0_f64, |m, l| m.max(l.abs())).max(c.iter().fold(0.0_f64, |m, v| m.max(v.abs())));
        // Components along the eigenvectors of λ_max (several if repeated).
        let tied: Vec<usize> = (0..lambda.len()).filter(|&i| top - lambda[i] <= 1e-9 * scale.max(1e-300)).collect();
        let free = tied.iter().all(|&i| c[i].abs() <= 1e-12 * scale.max(1e-300));
        // Coordinates w (in the eigenbasis) of x(μ) = −½ (B − μI)⁻¹ b.
        let w_at = |mu: f64| -> Vec<f64> {
            c.iter()
                .zip(&lambda)
                .map(|(&ci, &l)| if (l - mu).abs() > 0.0 { -0.5 * ci / (l - mu) } else { 0.0 })
                .collect()
        };
        let norm = |w: &[f64]| w.iter().map(|v| v * v).sum::<f64>().sqrt();
        let mut out = Vec::with_capacity(radii.len());
        for &radius in radii {
            let w = if radius == 0.0 {
                vec![0.0; c.len()]
            } else if free && {
                // The hard case: the path ends at a finite radius as μ → λ_max.
                let w_lim: Vec<f64> = c
                    .iter()
                    .zip(&lambda)
                    .enumerate()
                    .map(|(i, (&ci, &l))| if tied.contains(&i) { 0.0 } else { -0.5 * ci / (l - top) })
                    .collect();
                norm(&w_lim) <= radius
            } {
                let mut w: Vec<f64> = c
                    .iter()
                    .zip(&lambda)
                    .enumerate()
                    .map(|(i, (&ci, &l))| if tied.contains(&i) { 0.0 } else { -0.5 * ci / (l - top) })
                    .collect();
                let rest = (radius * radius - w.iter().map(|v| v * v).sum::<f64>()).max(0.0).sqrt();
                w[tied[0]] = rest;
                w
            } else {
                // ‖x(μ)‖ falls from ∞ (μ → λ_max⁺) towards 0; ‖x‖ ≤ ‖c‖ / (2(μ − λ_max)).
                let c_norm = norm(&c);
                let mut lo = top;
                let mut hi = top + (c_norm / (2.0 * radius)).max(f64::MIN_POSITIVE);
                for _ in 0..300 {
                    let mid = 0.5 * (lo + hi);
                    if mid <= lo || mid >= hi {
                        break;
                    }
                    if norm(&w_at(mid)) > radius {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                w_at(hi)
            };
            let mut x = vec![0.0; c.len()];
            for (v, &wi) in vectors.iter().zip(&w) {
                for (xj, &vj) in x.iter_mut().zip(v) {
                    *xj += wi * vj;
                }
            }
            out.push(RidgePoint { radius, value: self.value(&x), x });
        }
        Ok(out)
    }
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Eigenvalues (largest first) and unit eigenvectors of a symmetric matrix,
/// by cyclic Jacobi rotations (Golub & Van Loan, *Matrix Computations*, §8.5).
/// Each eigenvector's largest-magnitude component is made positive.
#[must_use]
pub fn symmetric_eigen(a: &[Vec<f64>]) -> (Vec<f64>, Vec<Vec<f64>>) {
    let n = a.len();
    let mut m: Vec<Vec<f64>> = a.to_vec();
    let mut v: Vec<Vec<f64>> = (0..n).map(|i| (0..n).map(|j| f64::from(u8::from(i == j))).collect()).collect();
    for _sweep in 0..100 {
        let off: f64 = (0..n)
            .flat_map(|i| (0..n).filter(move |&j| j != i).map(move |j| (i, j)))
            .map(|(i, j)| m[i][j] * m[i][j])
            .sum();
        let total: f64 = m.iter().flatten().map(|x| x * x).sum();
        if off <= 1e-30 * total.max(f64::MIN_POSITIVE) {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                if m[p][q] == 0.0 {
                    continue;
                }
                let theta = (m[q][q] - m[p][p]) / (2.0 * m[p][q]);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let t = if theta == 0.0 { 1.0 } else { t };
                let cos = 1.0 / (t * t + 1.0).sqrt();
                let sin = t * cos;
                for k in 0..n {
                    let (mkp, mkq) = (m[k][p], m[k][q]);
                    m[k][p] = cos * mkp - sin * mkq;
                    m[k][q] = sin * mkp + cos * mkq;
                }
                for k in 0..n {
                    let (mpk, mqk) = (m[p][k], m[q][k]);
                    m[p][k] = cos * mpk - sin * mqk;
                    m[q][k] = sin * mpk + cos * mqk;
                }
                for row in &mut v {
                    let (vp, vq) = (row[p], row[q]);
                    row[p] = cos * vp - sin * vq;
                    row[q] = sin * vp + cos * vq;
                }
            }
        }
    }
    let mut pairs: Vec<(f64, Vec<f64>)> = (0..n)
        .map(|i| {
            let mut vec: Vec<f64> = v.iter().map(|row| row[i]).collect();
            let big = vec.iter().copied().fold(0.0_f64, |best, x| if x.abs() > best.abs() { x } else { best });
            if big < 0.0 {
                vec.iter_mut().for_each(|x| *x = -*x);
            }
            (m[i][i], vec)
        })
        .collect();
    pairs.sort_by(|a, b| b.0.total_cmp(&a.0));
    pairs.into_iter().unzip()
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Montgomery, *Design and Analysis of Experiments*, Example 11.2 (a
    /// central composite design for yield vs. reaction time and temperature),
    /// with the factors coded so the data span −1 to +1 (the axial points):
    /// coefficients, stationary point and ridge values from an independent
    /// least-squares fit and root-finding in NumPy/SciPy. In the design's own
    /// coding (±1 at the factorial points) the fit is the published
    /// `79.94 + 0.995x₁ + 0.515x₂ + 0.25x₁x₂ − 1.376x₁² − 1.001x₂²`, with the
    /// stationary point at 86.95 min, 176.53 °F and predicted yield 80.21.
    fn yield_surface() -> QuadraticSurface {
        let r = std::f64::consts::SQRT_2;
        let names: Vec<String> = ["A", "B"].map(String::from).to_vec();
        let f = ModelFormula::parse("A + B + A*B + A^2 + B^2", &names).unwrap();
        // Coefficients in the design's coding, converted to ±α coding: x_d = r·x.
        let design = [79.94, 0.994_974_75, 0.515_165_04, 0.25, -1.376_25, -1.001_25];
        let coded = [design[0], design[1] * r, design[2] * r, design[3] * 2.0, design[4] * 2.0, design[5] * 2.0];
        QuadraticSurface::from_formula(&f, &coded).unwrap()
    }

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol * b.abs().max(1.0), "{a} vs {b}");
    }

    #[test]
    fn canonical_analysis_of_the_yield_example() {
        let s = yield_surface();
        let c = s.canonical_analysis();
        assert_eq!(c.kind, StationaryKind::Maximum);
        let xs = c.stationary_point.unwrap();
        close(xs[0], 0.275_248_65, 1e-7);
        close(xs[1], 0.216_274_09, 1e-7);
        close(c.stationary_value.unwrap(), 80.212_435_73, 1e-9);
        // Natural units: time = 85 + 5√2·x, temperature = 175 + 5√2·x.
        let r = 5.0 * std::f64::consts::SQRT_2;
        assert!((85.0 + r * xs[0] - 86.95).abs() < 0.01 && (175.0 + r * xs[1] - 176.53).abs() < 0.01);
        close(c.eigenvalues[0], -1.926_806_09, 1e-8);
        close(c.eigenvalues[1], -2.828_193_91, 1e-8);
        // Eigenvectors are orthonormal and satisfy Bv = λv.
        for (l, v) in c.eigenvalues.iter().zip(&c.eigenvectors) {
            for i in 0..2 {
                close(dot(&s.quadratic()[i], v), l * v[i], 1e-12);
            }
            close(dot(v, v), 1.0, 1e-14);
        }
        close(dot(&c.eigenvectors[0], &c.eigenvectors[1]), 0.0, 1e-14);
        close(c.eigenvectors[1][0].abs(), 0.957_092_03, 1e-8);
        // ŷ(xₛ) agrees with the direct evaluation.
        close(s.value(&xs), c.stationary_value.unwrap(), 1e-13);
    }

    #[test]
    fn ridge_paths_of_the_yield_example() {
        let s = yield_surface();
        let max = s.ridge_path(RidgeDirection::Maximum, &[0.0, 0.5, 1.0]).unwrap();
        close(max[0].value, 79.94, 1e-14);
        close(max[1].x[0], 0.371_750_59, 1e-7);
        close(max[1].x[1], 0.334_367_32, 1e-7);
        close(max[1].value, 80.164_573_96, 1e-9);
        close(max[2].x[0], 0.630_844_89, 1e-7);
        close(max[2].value, 79.336_722_38, 1e-9);
        let min = s.ridge_path(RidgeDirection::Minimum, &[0.5, 1.0]).unwrap();
        close(min[0].x[0], -0.486_942_81, 1e-7);
        close(min[0].value, 78.521_293_18, 1e-9);
        close(min[1].x[1], -0.080_155_62, 1e-6);
        close(min[1].value, 75.771_290_68, 1e-9);
        for p in max.iter().chain(&min) {
            close(dot(&p.x, &p.x).sqrt(), p.radius, 1e-9);
        }
        assert!(s.ridge_path(RidgeDirection::Maximum, &[-1.0]).is_err());
    }

    #[test]
    fn ridge_hard_case_and_brute_force_check() {
        // b has no component along the top eigenvector (x₂): y = 1 + x₁ − x₁² − 0.25x₂²... with λ₂ > λ₁.
        let s = QuadraticSurface::new(1.0, vec![1.0, 0.0], vec![vec![-2.0, 0.0], vec![0.0, -0.25]]).unwrap();
        let path = s.ridge_path(RidgeDirection::Maximum, &[0.1, 0.5, 1.0, 2.0]).unwrap();
        for p in &path {
            // Brute force over the circle.
            let best = (0..200_000)
                .map(|i| {
                    let t = std::f64::consts::TAU * f64::from(i) / 200_000.0;
                    s.value(&[p.radius * t.cos(), p.radius * t.sin()])
                })
                .fold(f64::NEG_INFINITY, f64::max);
            assert!((p.value - best).abs() < 1e-7, "radius {}: {} vs {}", p.radius, p.value, best);
            close(dot(&p.x, &p.x).sqrt(), p.radius, 1e-9);
        }
        // Three factors with a saddle, against brute force on the sphere.
        let s = QuadraticSurface::new(
            2.0,
            vec![0.3, -0.7, 0.2],
            vec![vec![-1.0, 0.4, 0.1], vec![0.4, 0.5, -0.3], vec![0.1, -0.3, -0.2]],
        )
        .unwrap();
        assert_eq!(s.canonical_analysis().kind, StationaryKind::Saddle);
        for dir in [RidgeDirection::Maximum, RidgeDirection::Minimum] {
            let p = &s.ridge_path(dir, &[1.0]).unwrap()[0];
            let n = 600;
            let mut best: f64 = if dir == RidgeDirection::Maximum { f64::NEG_INFINITY } else { f64::INFINITY };
            for i in 0..=n {
                let th = std::f64::consts::PI * f64::from(i) / f64::from(n);
                for j in 0..2 * n {
                    let ph = std::f64::consts::PI * f64::from(j) / f64::from(n);
                    let v = s.value(&[th.sin() * ph.cos(), th.sin() * ph.sin(), th.cos()]);
                    best = if dir == RidgeDirection::Maximum { best.max(v) } else { best.min(v) };
                }
            }
            assert!((p.value - best).abs() < 1e-4, "{dir:?}: {} vs {best}", p.value);
        }
    }

    #[test]
    fn singular_and_invalid_surfaces() {
        let s = QuadraticSurface::new(0.0, vec![1.0, 1.0], vec![vec![-1.0, 0.0], vec![0.0, 0.0]]).unwrap();
        let c = s.canonical_analysis();
        assert_eq!(c.kind, StationaryKind::Undetermined);
        assert!(c.stationary_point.is_none());
        assert!(QuadraticSurface::new(0.0, vec![1.0], vec![vec![1.0, 0.0]]).is_err());
        assert!(QuadraticSurface::new(0.0, vec![1.0, 0.0], vec![vec![1.0, 0.5], vec![0.0, 1.0]]).is_err());
        let names: Vec<String> = ["A"].map(String::from).to_vec();
        let cubic = ModelFormula::parse("A + A^3", &names).unwrap();
        assert!(QuadraticSurface::from_formula(&cubic, &[0.0, 1.0, 1.0]).is_err());
        // A surface without factors is a constant: no stationary point, and a
        // ridge path that stays at b₀ (this used to panic on an empty index).
        let constant = QuadraticSurface::new(4.5, vec![], vec![]).unwrap();
        assert_eq!(constant.canonical_analysis().kind, StationaryKind::Undetermined);
        let path = constant.ridge_path(RidgeDirection::Maximum, &[0.0, 1.0]).unwrap();
        assert_eq!(path.len(), 2);
        assert!(path.iter().all(|p| p.x.is_empty() && (p.value - 4.5).abs() < 1e-15));
    }

    #[test]
    fn jacobi_eigen_of_a_known_matrix() {
        // [[2,1],[1,2]] has eigenvalues 3 and 1.
        let (l, v) = symmetric_eigen(&[vec![2.0, 1.0], vec![1.0, 2.0]]);
        close(l[0], 3.0, 1e-15);
        close(l[1], 1.0, 1e-15);
        close(v[0][0], std::f64::consts::FRAC_1_SQRT_2, 1e-15);
        let (l, _) = symmetric_eigen(&[vec![4.0, -2.0, 0.0], vec![-2.0, 4.0, -2.0], vec![0.0, -2.0, 4.0]]);
        let r2 = std::f64::consts::SQRT_2;
        for (a, b) in l.iter().zip([4.0 + 2.0 * r2, 4.0, 4.0 - 2.0 * r2]) {
            close(*a, b, 1e-14);
        }
    }
}
