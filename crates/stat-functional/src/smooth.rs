//! Curves from discrete observations by penalized least squares.
//!
//! Each curve `i` observed as `(tᵢⱼ, yᵢⱼ)`, `j = 1 … nᵢ`, gets the
//! coefficients minimizing
//!
//! `PENSSE_λ(c) = Σⱼ (yᵢⱼ − φ(tᵢⱼ)ᵀc)² + λ cᵀRc`,
//!
//! where `R` is the roughness-penalty matrix of the basis
//! ([`Basis::penalty_matrix`]); the solution is `ĉ = (ΦᵀΦ + λR)⁻¹Φᵀy`
//! (Ramsay & Silverman 2005, §5.2). `λ = 0` is ordinary regression on the
//! basis (a regression spline or truncated Fourier series). The fit's
//! degrees of freedom are the trace of the hat matrix,
//! `df(λ) = tr[(ΦᵀΦ + λR)⁻¹ΦᵀΦ]`.
//!
//! # Choosing λ
//! One `λ` serves every curve, so the curves are smoothed alike. It
//! minimizes the summed generalized cross-validation criterion of Craven &
//! Wahba (1979),
//!
//! `GCV(λ) = Σᵢ nᵢ SSEᵢ(λ) / (nᵢ − dfᵢ(λ))²`,
//!
//! over `log₁₀ λ`: a grid in steps of 0.25 decades, then a golden-section
//! search in the best grid cell. The grid is placed relative to
//! `s = tr(ΦᵀΦ)/tr(R)`, from `10⁻¹⁰ s` to `10⁶ s`, so it does not depend on
//! the units of time. With a common set of times the summed criterion is
//! the pooled `N n SSE / (n − df)²` used for a sample of curves by
//! scikit-fda and the fda package.

use crate::basis::Basis;
use crate::fdata::FunctionalData;
use crate::linalg::{dot, spd_inverse};
use stat_core::{Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// One curve's observations.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveData {
    /// Observation times (inside the basis range).
    pub t: Vec<f64>,
    /// Observed values, one per time.
    pub y: Vec<f64>,
}

/// How the smoothing parameter is set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Smoothing {
    /// No penalty: least squares on the basis.
    None,
    /// This `λ`.
    Fixed(f64),
    /// The `λ` minimizing GCV.
    Gcv,
}

/// Options of [`smooth_curves`].
#[derive(Debug, Clone, PartialEq)]
pub struct SmoothOptions {
    /// Coefficients of the penalty operator `L = Σₘ aₘ Dᵐ`
    /// ([`crate::Roughness::operator`]).
    pub operator: Vec<f64>,
    /// How `λ` is set.
    pub smoothing: Smoothing,
}

/// Smoothed curves and the fit of each.
#[derive(Debug, Clone, PartialEq)]
pub struct SmoothResult {
    /// The curves.
    pub data: FunctionalData,
    /// The smoothing parameter used.
    pub lambda: f64,
    /// Degrees of freedom of each curve's fit.
    pub df: Vec<f64>,
    /// Residual sum of squares of each curve.
    pub sse: Vec<f64>,
    /// Each curve's GCV, `nᵢ SSEᵢ / (nᵢ − dfᵢ)²` (∞ when `dfᵢ ≥ nᵢ`).
    pub gcv: Vec<f64>,
    /// The fitted value at each observation, per curve.
    pub fitted: Vec<Vec<f64>>,
    /// `(log₁₀ λ, summed GCV)` over the search grid (empty unless chosen by
    /// GCV).
    pub gcv_path: Vec<(f64, f64)>,
    /// `s = tr(ΦᵀΦ)/tr(R)`, the scale the λ search is placed on.
    pub lambda_scale: f64,
}

impl SmoothResult {
    /// The summed GCV criterion.
    #[must_use]
    pub fn total_gcv(&self) -> f64 {
        self.gcv.iter().sum()
    }
}

/// Curves that share one set of times.
struct Group {
    phi: Vec<Vec<f64>>,
    gram: Vec<Vec<f64>>,
    members: Vec<usize>,
    /// `Φᵀy` per member.
    projections: Vec<Vec<f64>>,
}

struct Fit {
    coefs: Vec<Vec<f64>>,
    df: Vec<f64>,
    sse: Vec<f64>,
    fitted: Vec<Vec<f64>>,
}

/// Smooth every curve in `basis`.
///
/// # Errors
/// No curves, a curve with mismatched or non-finite values or times outside
/// the basis range, a negative fixed λ, or (without a penalty) a curve with
/// too few distinct times to determine the coefficients.
pub fn smooth_curves(basis: &Basis, curves: &[CurveData], options: &SmoothOptions) -> Result<SmoothResult> {
    if curves.is_empty() {
        return Err(invalid("there are no curves to smooth"));
    }
    let (a, b) = basis.range();
    let tol = 1e-9 * (b - a).abs().max(1.0);
    for (i, c) in curves.iter().enumerate() {
        if c.t.len() != c.y.len() || c.t.is_empty() {
            return Err(invalid(format!("curve {} needs one value per time", i + 1)));
        }
        if c.t.iter().chain(&c.y).any(|v| !v.is_finite()) {
            return Err(invalid(format!("curve {} has a missing or infinite value", i + 1)));
        }
        if c.t.iter().any(|&t| t < a - tol || t > b + tol) {
            return Err(invalid(format!("curve {} has times outside the basis range", i + 1)));
        }
    }
    let k = basis.n_basis();
    let penalty = basis.penalty_matrix(&options.operator);
    let mut groups: Vec<Group> = Vec::new();
    for (i, c) in curves.iter().enumerate() {
        let g = match groups.iter().position(|g| curves[g.members[0]].t == c.t) {
            Some(g) => g,
            None => {
                let phi: Vec<Vec<f64>> = c.t.iter().map(|&t| basis.evaluate(t, 0)).collect();
                let mut gram = vec![vec![0.0; k]; k];
                for p in &phi {
                    for j in 0..k {
                        if p[j] == 0.0 {
                            continue;
                        }
                        for l in 0..k {
                            gram[j][l] += p[j] * p[l];
                        }
                    }
                }
                groups.push(Group { phi, gram, members: Vec::new(), projections: Vec::new() });
                groups.len() - 1
            }
        };
        let group = &mut groups[g];
        let proj = (0..k).map(|j| group.phi.iter().zip(&c.y).map(|(p, y)| p[j] * y).sum()).collect();
        group.members.push(i);
        group.projections.push(proj);
    }
    let trace_gram: f64 =
        groups.iter().map(|g| g.members.len() as f64 * (0..k).map(|j| g.gram[j][j]).sum::<f64>()).sum::<f64>()
            / curves.len() as f64;
    let trace_r: f64 = (0..k).map(|j| penalty[j][j]).sum();
    let scale = if trace_r > 0.0 { trace_gram / trace_r } else { 1.0 };

    let fit = |lambda: f64| -> Option<Fit> {
        let mut out = Fit {
            coefs: vec![Vec::new(); curves.len()],
            df: vec![0.0; curves.len()],
            sse: vec![0.0; curves.len()],
            fitted: vec![Vec::new(); curves.len()],
        };
        for g in &groups {
            let m: Vec<Vec<f64>> =
                (0..k).map(|j| (0..k).map(|l| g.gram[j][l] + lambda * penalty[j][l]).collect()).collect();
            let inv = spd_inverse(&m)?;
            let df: f64 = (0..k).map(|j| (0..k).map(|l| inv[j][l] * g.gram[l][j]).sum::<f64>()).sum();
            for (&i, proj) in g.members.iter().zip(&g.projections) {
                let c: Vec<f64> = inv.iter().map(|r| dot(r, proj)).collect();
                let fitted: Vec<f64> = g.phi.iter().map(|p| dot(p, &c)).collect();
                out.sse[i] = fitted.iter().zip(&curves[i].y).map(|(f, y)| (y - f).powi(2)).sum();
                out.df[i] = df;
                out.coefs[i] = c;
                out.fitted[i] = fitted;
            }
        }
        Some(out)
    };
    let gcv_of = |f: &Fit| -> Vec<f64> {
        curves
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let n = c.t.len() as f64;
                let room = n - f.df[i];
                if room > 1e-8 * n { n * f.sse[i] / (room * room) } else { f64::INFINITY }
            })
            .collect()
    };
    let total = |lambda: f64| -> f64 { fit(lambda).map_or(f64::INFINITY, |f| gcv_of(&f).iter().sum()) };

    let mut gcv_path = Vec::new();
    let lambda = match options.smoothing {
        Smoothing::None => 0.0,
        Smoothing::Fixed(l) => {
            if !(l.is_finite() && l >= 0.0) {
                return Err(invalid("the smoothing parameter must be zero or positive"));
            }
            l
        }
        Smoothing::Gcv => {
            if trace_r <= 0.0 {
                return Err(invalid("this basis has no roughness to penalize"));
            }
            let (lo, hi, step) = (-10.0, 6.0, 0.25);
            let n_grid = ((hi - lo) / step) as usize + 1;
            for g in 0..n_grid {
                let rho = lo + step * g as f64;
                gcv_path.push((rho + scale.log10(), total(scale * 10f64.powf(rho))));
            }
            let best = (0..n_grid)
                .filter(|&g| gcv_path[g].1.is_finite())
                .min_by(|&x, &y| gcv_path[x].1.total_cmp(&gcv_path[y].1))
                .ok_or_else(|| {
                    invalid("no smoothing parameter gives a finite GCV; add observations or use fewer basis functions")
                })?;
            let f = |rho: f64| total(scale * 10f64.powf(rho));
            let (mut x0, mut x1) =
                (lo + step * best.saturating_sub(1) as f64, lo + step * (best + 1).min(n_grid - 1) as f64);
            let r = 0.5 * (5f64.sqrt() - 1.0);
            let (mut c, mut d) = (x1 - r * (x1 - x0), x0 + r * (x1 - x0));
            let (mut fc, mut fd) = (f(c), f(d));
            while x1 - x0 > 1e-5 {
                if fc < fd {
                    x1 = d;
                    d = c;
                    fd = fc;
                    c = x1 - r * (x1 - x0);
                    fc = f(c);
                } else {
                    x0 = c;
                    c = d;
                    fc = fd;
                    d = x0 + r * (x1 - x0);
                    fd = f(d);
                }
            }
            let rho = 0.5 * (x0 + x1);
            let grid_best = lo + step * best as f64;
            let rho = if f(rho) <= gcv_path[best].1 { rho } else { grid_best };
            scale * 10f64.powf(rho)
        }
    };
    let f = fit(lambda).ok_or_else(|| {
        invalid(
            "the curves have too few distinct times for this basis; add a roughness penalty or use fewer basis \
             functions",
        )
    })?;
    let gcv = gcv_of(&f);
    Ok(SmoothResult {
        data: FunctionalData { basis: basis.clone(), coefs: f.coefs },
        lambda,
        df: f.df,
        sse: f.sse,
        gcv,
        fitted: f.fitted,
        gcv_path,
        lambda_scale: scale,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::basis::{BSplineBasis, Roughness};

    #[test]
    fn unpenalized_fit_reproduces_a_spline_exactly() {
        let basis = Basis::BSpline(BSplineBasis::equally_spaced((0.0, 1.0), 6, 4).unwrap());
        let truth = [1.0, -0.5, 2.0, 0.3, 1.1, -1.0];
        let t: Vec<f64> = (0..21).map(|i| i as f64 / 20.0).collect();
        let y: Vec<f64> = t.iter().map(|&s| dot(&truth, &basis.evaluate(s, 0))).collect();
        let r = smooth_curves(
            &basis,
            &[CurveData { t, y }],
            &SmoothOptions { operator: Roughness::Derivative(2).operator(), smoothing: Smoothing::None },
        )
        .unwrap();
        for (c, w) in r.data.coefs[0].iter().zip(truth) {
            assert!((c - w).abs() < 1e-10);
        }
        assert!((r.df[0] - 6.0).abs() < 1e-10);
    }

    #[test]
    fn heavy_penalty_gives_a_straight_line_and_gcv_picks_an_interior_lambda() {
        let basis = Basis::BSpline(BSplineBasis::equally_spaced((0.0, 1.0), 15, 4).unwrap());
        let t: Vec<f64> = (0..41).map(|i| i as f64 / 40.0).collect();
        let noise = |i: usize| ((i * 7919 % 101) as f64 / 101.0 - 0.5) * 0.2;
        let y: Vec<f64> =
            t.iter().enumerate().map(|(i, &s)| (2.0 * std::f64::consts::PI * s).sin() + noise(i)).collect();
        let curves = [CurveData { t: t.clone(), y: y.clone() }];
        let op = Roughness::Derivative(2).operator();
        let stiff =
            smooth_curves(&basis, &curves, &SmoothOptions { operator: op.clone(), smoothing: Smoothing::Fixed(1e8) })
                .unwrap();
        assert!((stiff.df[0] - 2.0).abs() < 1e-3, "df → 2 as λ → ∞: {}", stiff.df[0]);
        let gcv = smooth_curves(&basis, &curves, &SmoothOptions { operator: op, smoothing: Smoothing::Gcv }).unwrap();
        assert!(gcv.df[0] > 3.0 && gcv.df[0] < 12.0, "df {}", gcv.df[0]);
        let best = gcv.gcv_path.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
        assert!(gcv.total_gcv() <= best + 1e-12);
    }
}
