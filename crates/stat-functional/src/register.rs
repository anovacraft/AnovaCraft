//! Registration: aligning curves in time before they are averaged or
//! analysed, so that variation in timing (phase) is separated from
//! variation in size (amplitude).
//!
//! A **warping function** `hᵢ` is a strictly increasing map of `[a, b]` onto
//! itself; the registered curve is `xᵢ*(t) = xᵢ(hᵢ(t))`, so the feature of
//! `xᵢ` at time `hᵢ(t)` appears at `t` (Ramsay & Silverman 2005, ch. 7).
//!
//! # Landmark registration
//! Landmarks are times of recognisable features, here the time of each
//! curve's maximum or minimum inside a window. With `tᵢₗ` the landmarks of
//! curve `i` and `t₀ₗ` their mean over the curves, `hᵢ` interpolates
//! `hᵢ(a) = a`, `hᵢ(t₀ₗ) = tᵢₗ`, `hᵢ(b) = b` by the monotone piecewise cubic
//! Hermite interpolant of Fritsch & Carlson (1980), with the derivative
//! choices of Fritsch & Butland (1984) as implemented by SciPy's
//! `PchipInterpolator` ([`Pchip`]); a monotone interpolant of increasing
//! points is increasing, so `hᵢ` is a valid warp. Each registered curve has
//! its landmarks exactly at the mean landmark times. This is the warping of
//! scikit-fda's `landmark_elastic_registration_warping`.
//!
//! # Continuous registration
//! Ramsay & Li (1998, *JRSS B* 60, 351–363) align each curve to a target
//! `x₀` by minimizing the smallest eigenvalue of
//!
//! `T(h) = [ ∫x₀²      ∫x₀ xᵢ∘h ]`
//! `       [ ∫x₀ xᵢ∘h  ∫(xᵢ∘h)² ]`,
//!
//! which is zero exactly when `xᵢ∘h` is proportional to `x₀`, so curves are
//! matched in shape whatever their amplitude. The warp is
//! `h(t) = a + (b − a) ∫ₐᵗ exp W / ∫ₐᵇ exp W` with `W` a cubic B-spline
//! (`n_basis` functions, the constant direction removed since it does not
//! change `h`), which is strictly increasing for any `W`. The criterion is
//! scaled by `∫x₀²` and a roughness penalty `κ (b − a)³ ∫(W″)²` (scale free)
//! keeps warps smooth. Each curve's `W` is found by the Nelder–Mead method
//! from the previous solution. The target starts as the mean curve and is
//! replaced by the mean of the registered curves after each pass (a
//! Procrustes iteration, Ramsay & Silverman §7.6), until the warps stop
//! changing or the passes run out. Integrals use Simpson's rule on 401
//! equally spaced points; curves are evaluated at warped times by cubic
//! Hermite interpolation on 2001 points.

use crate::basis::{BSplineBasis, Basis, Roughness};
use crate::fdata::FunctionalData;
use crate::linalg::{dot, interp, linspace, quad_form, simpson_weights};
use rayon::prelude::*;
use stat_core::optimize::minimize_in_box;
use stat_core::{Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Points on which warping functions are stored.
pub const WARP_GRID: usize = 401;

/// Which feature marks a landmark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LandmarkKind {
    /// The time of the curve's largest value in the window.
    Maximum,
    /// The time of the curve's smallest value in the window.
    Minimum,
}

/// One landmark: a feature searched for in a window of time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LandmarkSpec {
    /// Maximum or minimum.
    pub kind: LandmarkKind,
    /// `[from, to]`, clamped to the curves' range.
    pub window: (f64, f64),
}

/// The landmark times of every curve (`out[i][l]`), each located on a grid
/// of 1001 points in its window and refined to a zero of the derivative by
/// bisection when it is inside the window.
///
/// # Errors
/// A window outside the range or empty.
pub fn find_landmarks(data: &FunctionalData, specs: &[LandmarkSpec]) -> Result<Vec<Vec<f64>>> {
    let (a, b) = data.basis.range();
    let windows: Vec<(f64, f64)> = specs
        .iter()
        .map(|s| {
            let (lo, hi) = (s.window.0.max(a), s.window.1.min(b));
            if hi > lo { Ok((lo, hi)) } else { Err(invalid("a landmark window lies outside the curves' range")) }
        })
        .collect::<Result<_>>()?;
    let mut out = vec![Vec::with_capacity(specs.len()); data.n_curves()];
    for (spec, &(lo, hi)) in specs.iter().zip(&windows) {
        let grid = linspace(lo, hi, 1001);
        let phi: Vec<Vec<f64>> = grid.iter().map(|&t| data.basis.evaluate(t, 0)).collect();
        for (i, c) in data.coefs.iter().enumerate() {
            let sign = if spec.kind == LandmarkKind::Maximum { 1.0 } else { -1.0 };
            let values: Vec<f64> = phi.iter().map(|p| sign * dot(c, p)).collect();
            let g = (0..values.len()).max_by(|&x, &y| values[x].total_cmp(&values[y]).then(y.cmp(&x))).unwrap_or(0);
            let mut t = grid[g];
            if g > 0 && g + 1 < grid.len() {
                let d = |s: f64| data.eval(i, s, 1);
                let (mut l, mut r) = (grid[g - 1], grid[g + 1]);
                let (dl, dr) = (d(l), d(r));
                if dl * dr < 0.0 {
                    let left_sign = dl.signum();
                    for _ in 0..60 {
                        let m = 0.5 * (l + r);
                        if d(m).signum() == left_sign {
                            l = m;
                        } else {
                            r = m;
                        }
                    }
                    t = 0.5 * (l + r);
                }
            }
            out[i].push(t);
        }
    }
    Ok(out)
}

/// A monotone piecewise cubic Hermite interpolant (Fritsch–Carlson, with
/// SciPy's `PchipInterpolator` derivative rules).
#[derive(Debug, Clone, PartialEq)]
pub struct Pchip {
    x: Vec<f64>,
    y: Vec<f64>,
    d: Vec<f64>,
}

impl Pchip {
    /// Interpolant of the points `(x, y)`, `x` strictly increasing.
    ///
    /// # Errors
    /// Fewer than two points, mismatched lengths or `x` not increasing.
    pub fn new(x: &[f64], y: &[f64]) -> Result<Self> {
        let n = x.len();
        if n < 2 || y.len() != n || x.windows(2).any(|w| w[1] <= w[0]) {
            return Err(invalid("monotone interpolation needs two or more points with increasing abscissae"));
        }
        let h: Vec<f64> = x.windows(2).map(|w| w[1] - w[0]).collect();
        let m: Vec<f64> = (0..n - 1).map(|k| (y[k + 1] - y[k]) / h[k]).collect();
        let mut d = vec![0.0; n];
        if n == 2 {
            d = vec![m[0], m[0]];
        } else {
            for k in 1..n - 1 {
                let (m0, m1) = (m[k - 1], m[k]);
                if m0.signum() != m1.signum() || m0 == 0.0 || m1 == 0.0 {
                    d[k] = 0.0;
                } else {
                    let w1 = 2.0 * h[k] + h[k - 1];
                    let w2 = h[k] + 2.0 * h[k - 1];
                    d[k] = (w1 + w2) / (w1 / m0 + w2 / m1);
                }
            }
            let edge = |h0: f64, h1: f64, m0: f64, m1: f64| {
                let d = ((2.0 * h0 + h1) * m0 - h0 * m1) / (h0 + h1);
                if d.signum() != m0.signum() {
                    0.0
                } else if m0.signum() != m1.signum() && d.abs() > 3.0 * m0.abs() {
                    3.0 * m0
                } else {
                    d
                }
            };
            d[0] = edge(h[0], h[1], m[0], m[1]);
            d[n - 1] = edge(h[n - 2], h[n - 3], m[n - 2], m[n - 3]);
        }
        Ok(Self { x: x.to_vec(), y: y.to_vec(), d })
    }

    /// Value at `t` (the end segments extend beyond the points).
    #[must_use]
    pub fn eval(&self, t: f64) -> f64 {
        let n = self.x.len();
        let k = self.x.partition_point(|v| *v <= t).saturating_sub(1).min(n - 2);
        let h = self.x[k + 1] - self.x[k];
        let s = (t - self.x[k]) / h;
        let (s2, s3) = (s * s, s * s * s);
        let h00 = 2.0 * s3 - 3.0 * s2 + 1.0;
        let h10 = s3 - 2.0 * s2 + s;
        let h01 = -2.0 * s3 + 3.0 * s2;
        let h11 = s3 - s2;
        h00 * self.y[k] + h10 * h * self.d[k] + h01 * self.y[k + 1] + h11 * h * self.d[k + 1]
    }
}

/// A warping function stored on [`WARP_GRID`] equally spaced times.
#[derive(Debug, Clone, PartialEq)]
pub struct Warp {
    /// Registered (clock) times.
    pub grid: Vec<f64>,
    /// `h(t)` at each grid time: the curve's own time.
    pub values: Vec<f64>,
}

impl Warp {
    /// The identity on `[a, b]`.
    #[must_use]
    pub fn identity(range: (f64, f64)) -> Self {
        let grid = linspace(range.0, range.1, WARP_GRID);
        Self { values: grid.clone(), grid }
    }

    /// `h(t)`, linear between grid points.
    #[must_use]
    pub fn eval(&self, t: f64) -> f64 {
        interp(&self.grid, &self.values, t)
    }

    /// `h⁻¹(s)`: the registered time of the curve's own time `s`.
    #[must_use]
    pub fn inverse(&self, s: f64) -> f64 {
        interp(&self.values, &self.grid, s)
    }

    /// `self ∘ inner`: `t ↦ self(inner(t))`.
    #[must_use]
    pub fn compose(&self, inner: &Self) -> Self {
        Self { grid: inner.grid.clone(), values: inner.values.iter().map(|&s| self.eval(s)).collect() }
    }

    /// Largest `|h(t) − t|`.
    #[must_use]
    pub fn max_shift(&self) -> f64 {
        self.grid.iter().zip(&self.values).map(|(t, h)| (h - t).abs()).fold(0.0, f64::max)
    }
}

/// Landmark warps on `range`: `hᵢ(location_l) = landmarks[i][l]`, with the
/// locations the landmarks' means unless given. Returns the warps and the
/// locations.
///
/// # Errors
/// Landmarks of a curve (or the locations) not strictly increasing and
/// strictly inside the range, or ragged landmark lists.
pub fn landmark_warps(
    range: (f64, f64),
    landmarks: &[Vec<f64>],
    location: Option<&[f64]>,
) -> Result<(Vec<Warp>, Vec<f64>)> {
    let (a, b) = range;
    let l = landmarks.first().map_or(0, Vec::len);
    if landmarks.iter().any(|v| v.len() != l) {
        return Err(invalid("every curve needs the same number of landmarks"));
    }
    let location: Vec<f64> = match location {
        Some(loc) => loc.to_vec(),
        None => (0..l).map(|j| landmarks.iter().map(|v| v[j]).sum::<f64>() / landmarks.len().max(1) as f64).collect(),
    };
    let inside = |v: &[f64]| {
        let mut pts = vec![a];
        pts.extend_from_slice(v);
        pts.push(b);
        pts.windows(2).all(|w| w[1] > w[0])
    };
    if !inside(&location) {
        return Err(invalid("the landmark locations must be increasing and inside the range"));
    }
    let mut xs = vec![a];
    xs.extend_from_slice(&location);
    xs.push(b);
    let grid = linspace(a, b, WARP_GRID);
    let mut warps = Vec::with_capacity(landmarks.len());
    for (i, lm) in landmarks.iter().enumerate() {
        if !inside(lm) {
            return Err(invalid(format!("curve {}: landmarks must be increasing and inside the range", i + 1)));
        }
        let mut ys = vec![a];
        ys.extend_from_slice(lm);
        ys.push(b);
        let p = Pchip::new(&xs, &ys)?;
        let values = grid.iter().map(|&t| p.eval(t).clamp(a, b)).collect();
        warps.push(Warp { grid: grid.clone(), values });
    }
    Ok((warps, location))
}

/// The registered curves `xᵢ(hᵢ(t))`, re-expressed in the same basis by
/// least squares on 501 equally spaced times.
///
/// # Errors
/// A different number of warps and curves.
pub fn apply_warps(data: &FunctionalData, warps: &[Warp]) -> Result<FunctionalData> {
    if warps.len() != data.n_curves() {
        return Err(invalid("there must be one warping function per curve"));
    }
    let (a, b) = data.basis.range();
    let grid = linspace(a, b, 501);
    let values: Vec<Vec<f64>> =
        (0..data.n_curves()).map(|i| grid.iter().map(|&t| data.eval(i, warps[i].eval(t), 0)).collect()).collect();
    FunctionalData::from_values(data.basis.clone(), &grid, &values)
}

/// Options of [`register_continuous`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContinuousOptions {
    /// B-spline functions for `W` (at least 2; the constant is removed, so
    /// each warp has `n_basis − 1` free parameters).
    pub n_basis: usize,
    /// Roughness penalty `κ` on `W`.
    pub penalty: f64,
    /// Greatest number of passes (target updates).
    pub iterations: usize,
    /// Objective evaluations allowed per curve and pass.
    pub max_evaluations: usize,
}

impl Default for ContinuousOptions {
    fn default() -> Self {
        Self { n_basis: 6, penalty: 1e-4, iterations: 3, max_evaluations: 3000 }
    }
}

/// The result of [`register_continuous`].
#[derive(Debug, Clone, PartialEq)]
pub struct ContinuousRegistration {
    /// One warp per curve.
    pub warps: Vec<Warp>,
    /// Each curve's scaled minimum-eigenvalue criterion against the final
    /// target before registration.
    pub criterion_before: Vec<f64>,
    /// The same after registration (without the penalty).
    pub criterion_after: Vec<f64>,
    /// Passes made.
    pub passes: usize,
}

/// Values and derivatives of a curve on a fine grid, for cubic Hermite
/// evaluation at arbitrary times.
struct Fine {
    a: f64,
    h: f64,
    v: Vec<f64>,
    d: Vec<f64>,
}

impl Fine {
    fn eval(&self, t: f64) -> f64 {
        let n = self.v.len();
        let u = ((t - self.a) / self.h).clamp(0.0, (n - 1) as f64);
        let k = (u.floor() as usize).min(n - 2);
        let s = u - k as f64;
        let (s2, s3) = (s * s, s * s * s);
        (2.0 * s3 - 3.0 * s2 + 1.0) * self.v[k]
            + (s3 - 2.0 * s2 + s) * self.h * self.d[k]
            + (-2.0 * s3 + 3.0 * s2) * self.v[k + 1]
            + (s3 - s2) * self.h * self.d[k + 1]
    }
}

/// Scaled smallest eigenvalue of the 2 × 2 matrix of inner products.
fn min_eigen(x0: &[f64], x: &[f64], w: &[f64]) -> f64 {
    let (mut aa, mut ab, mut bb) = (0.0, 0.0, 0.0);
    for ((p, q), wi) in x0.iter().zip(x).zip(w) {
        aa += wi * p * p;
        ab += wi * p * q;
        bb += wi * q * q;
    }
    let m = 0.5 * ((aa + bb) - ((aa - bb).powi(2) + 4.0 * ab * ab).sqrt());
    m / aa
}

/// Align every curve to the (iterated) mean curve by the minimum-eigenvalue
/// criterion.
///
/// # Errors
/// Fewer than two curves, `n_basis < 2`, a negative penalty, or a target
/// curve that is zero everywhere.
pub fn register_continuous(data: &FunctionalData, options: &ContinuousOptions) -> Result<ContinuousRegistration> {
    let n = data.n_curves();
    if n < 2 {
        return Err(StatError::InsufficientData { required: 2, found: n });
    }
    if options.n_basis < 2 {
        return Err(invalid("the warping functions need at least two basis functions"));
    }
    if !(options.penalty.is_finite() && options.penalty >= 0.0) {
        return Err(invalid("the warping penalty must be zero or positive"));
    }
    let (a, b) = data.basis.range();
    let len = b - a;
    let grid = linspace(a, b, WARP_GRID);
    let weights = simpson_weights(WARP_GRID, a, b);
    let fine_n = 2001;
    let fine_t = linspace(a, b, fine_n);
    let fine: Vec<Fine> = {
        let phi: Vec<Vec<Vec<f64>>> = fine_t.iter().map(|&t| data.basis.derivatives(t, 1)).collect();
        data.coefs
            .iter()
            .map(|c| Fine {
                a,
                h: len / (fine_n - 1) as f64,
                v: phi.iter().map(|p| dot(c, &p[0])).collect(),
                d: phi.iter().map(|p| dot(c, &p[1])).collect(),
            })
            .collect()
    };
    let order = options.n_basis.min(4);
    let wbasis = Basis::BSpline(BSplineBasis::equally_spaced((a, b), options.n_basis, order)?);
    let bmat: Vec<Vec<f64>> = grid.iter().map(|&t| wbasis.evaluate(t, 0)).collect();
    let pmat = wbasis.penalty_matrix(&Roughness::Derivative(if order >= 3 { 2 } else { 1 }).operator());
    let nw = options.n_basis;
    // Helmert basis of the coefficient vectors orthogonal to (1, …, 1).
    let q: Vec<Vec<f64>> = (1..nw)
        .map(|k| {
            let s = ((k * (k + 1)) as f64).sqrt();
            (0..nw)
                .map(|j| {
                    if j < k {
                        1.0 / s
                    } else if j == k {
                        -(k as f64) / s
                    } else {
                        0.0
                    }
                })
                .collect()
        })
        .collect();
    let coef_of =
        |z: &[f64]| -> Vec<f64> { (0..nw).map(|j| z.iter().zip(&q).map(|(zk, qk)| zk * qk[j]).sum()).collect() };
    let warp_of = |c: &[f64]| -> Vec<f64> {
        let e: Vec<f64> = bmat.iter().map(|r| dot(r, c).exp()).collect();
        let mut cum = vec![0.0; WARP_GRID];
        for g in 1..WARP_GRID {
            cum[g] = cum[g - 1] + 0.5 * (e[g] + e[g - 1]) * (grid[g] - grid[g - 1]);
        }
        let total = cum[WARP_GRID - 1];
        cum.iter().enumerate().map(|(g, s)| if g == WARP_GRID - 1 { b } else { a + len * s / total }).collect()
    };
    let mut target: Vec<f64> = grid.iter().map(|&t| (0..n).map(|i| fine[i].eval(t)).sum::<f64>() / n as f64).collect();
    let mut z: Vec<Vec<f64>> = vec![vec![0.0; nw - 1]; n];
    let mut warps: Vec<Vec<f64>> = vec![grid.clone(); n];
    let kappa = options.penalty * len.powi(3);
    let mut passes = 0;
    for _ in 0..options.iterations.max(1) {
        if dot(&target.iter().map(|v| v * v).collect::<Vec<_>>(), &weights) <= 0.0 {
            return Err(invalid("the mean curve is zero everywhere, so curves cannot be aligned to it"));
        }
        passes += 1;
        let results: Vec<(Vec<f64>, Vec<f64>)> = (0..n)
            .into_par_iter()
            .map(|i| {
                let mut objective = |zz: &[f64]| -> f64 {
                    let c = coef_of(zz);
                    let h = warp_of(&c);
                    let xw: Vec<f64> = h.iter().map(|&s| fine[i].eval(s)).collect();
                    min_eigen(&target, &xw, &weights) + kappa * quad_form(&pmat, &c, &c)
                };
                let lower = vec![-6.0; nw - 1];
                let upper = vec![6.0; nw - 1];
                let first = minimize_in_box(&mut objective, &z[i], &lower, &upper, options.max_evaluations);
                let second = minimize_in_box(&mut objective, &first.x, &lower, &upper, options.max_evaluations / 2);
                let best = if second.value <= first.value { second.x } else { first.x };
                let h = warp_of(&coef_of(&best));
                (best, h)
            })
            .collect();
        let mut change: f64 = 0.0;
        for (i, (zi, h)) in results.into_iter().enumerate() {
            change = change.max(h.iter().zip(&warps[i]).map(|(x, y)| (x - y).abs()).fold(0.0, f64::max));
            z[i] = zi;
            warps[i] = h;
        }
        target = (0..WARP_GRID).map(|g| (0..n).map(|i| fine[i].eval(warps[i][g])).sum::<f64>() / n as f64).collect();
        if change < 1e-4 * len {
            break;
        }
    }
    let criterion_before = (0..n)
        .map(|i| min_eigen(&target, &grid.iter().map(|&t| fine[i].eval(t)).collect::<Vec<_>>(), &weights))
        .collect();
    let criterion_after = (0..n)
        .map(|i| min_eigen(&target, &warps[i].iter().map(|&s| fine[i].eval(s)).collect::<Vec<_>>(), &weights))
        .collect();
    Ok(ContinuousRegistration {
        warps: warps.into_iter().map(|values| Warp { grid: grid.clone(), values }).collect(),
        criterion_before,
        criterion_after,
        passes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smooth::{CurveData, SmoothOptions, Smoothing, smooth_curves};

    fn bump(t: f64, centre: f64, height: f64) -> f64 {
        height * (-(t - centre).powi(2) / (2.0 * 0.08f64.powi(2))).exp()
    }

    fn shifted_bumps(centres: &[f64], heights: &[f64]) -> FunctionalData {
        let basis = Basis::BSpline(BSplineBasis::equally_spaced((0.0, 1.0), 40, 4).unwrap());
        let t: Vec<f64> = (0..201).map(|i| i as f64 / 200.0).collect();
        let curves: Vec<CurveData> = centres
            .iter()
            .zip(heights)
            .map(|(&c, &h)| CurveData { t: t.clone(), y: t.iter().map(|&s| bump(s, c, h)).collect() })
            .collect();
        smooth_curves(&basis, &curves, &SmoothOptions { operator: vec![0.0, 0.0, 1.0], smoothing: Smoothing::None })
            .unwrap()
            .data
    }

    #[test]
    fn pchip_matches_scipy() {
        // scipy.interpolate.PchipInterpolator([0, 1, 3, 4, 7], [0, 2, 3, 5, 6]) at 0.5, 2, 3.5, 5.5.
        let p = Pchip::new(&[0.0, 1.0, 3.0, 4.0, 7.0], &[0.0, 2.0, 3.0, 5.0, 6.0]).unwrap();
        let want = [1.2053571428571428, 2.5, 4.026061776061776, 5.743243243243243];
        for (t, w) in [0.5, 2.0, 3.5, 5.5].iter().zip(want) {
            assert!((p.eval(*t) - w).abs() < 1e-12, "{t}: {} vs {w}", p.eval(*t));
        }
    }

    #[test]
    fn landmark_registration_moves_peaks_to_their_mean() {
        let centres = [0.35, 0.45, 0.5, 0.62];
        let fd = shifted_bumps(&centres, &[1.0, 1.5, 0.8, 1.2]);
        let lm = find_landmarks(&fd, &[LandmarkSpec { kind: LandmarkKind::Maximum, window: (0.1, 0.9) }]).unwrap();
        for (l, c) in lm.iter().zip(centres) {
            assert!((l[0] - c).abs() < 1e-4, "{} vs {c}", l[0]);
        }
        let (warps, location) = landmark_warps((0.0, 1.0), &lm, None).unwrap();
        let mean = centres.iter().sum::<f64>() / 4.0;
        assert!((location[0] - mean).abs() < 1e-4);
        let reg = apply_warps(&fd, &warps).unwrap();
        let again = find_landmarks(&reg, &[LandmarkSpec { kind: LandmarkKind::Maximum, window: (0.1, 0.9) }]).unwrap();
        for l in again {
            assert!((l[0] - location[0]).abs() < 3e-3, "{} vs {}", l[0], location[0]);
        }
    }

    #[test]
    fn continuous_registration_aligns_shifted_bumps() {
        let centres = [0.4, 0.45, 0.5, 0.55, 0.6];
        let fd = shifted_bumps(&centres, &[1.0, 1.4, 0.7, 1.1, 0.9]);
        let r = register_continuous(&fd, &ContinuousOptions::default()).unwrap();
        let reg = apply_warps(&fd, &r.warps).unwrap();
        let peaks = find_landmarks(&reg, &[LandmarkSpec { kind: LandmarkKind::Maximum, window: (0.1, 0.9) }]).unwrap();
        let spread = peaks.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max)
            - peaks.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
        assert!(spread < 0.02, "peaks after registration span {spread}");
        for (b, a) in r.criterion_before.iter().zip(&r.criterion_after) {
            assert!(a <= b, "criterion {a} after vs {b} before");
        }
    }
}
