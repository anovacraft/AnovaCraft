//! Baselines (slowly varying backgrounds) under spectra and chromatograms.
//!
//! # Penalized least squares
//! Both penalized methods solve, for weights `wᵢ`, the Whittaker smoother
//! (Eilers, "A perfect smoother", *Analytical Chemistry* 75, 2003)
//!
//! ```text
//! z = argmin Σ wᵢ (yᵢ − zᵢ)² + λ Σ (Δᵈ zᵢ)²,   i.e.  (W + λ DᵀD) z = W y,
//! ```
//!
//! a banded system (half-bandwidth `d`, the difference order, 2 by default)
//! solved by banded Cholesky factorization, and re-weight the points until
//! the weights settle: the iterations stop when
//! `‖w_new − w‖₂ / ‖w‖₂ < tol` (the baseline is then the last one computed)
//! or after `max_iter + 1` solves. Starting weights are all 1. The
//! positions `x` do not enter: the penalty is on differences between
//! neighbouring points.
//!
//! * **Asymmetric least squares** (AsLS; Eilers & Boelens, "Baseline
//!   correction with asymmetric least squares smoothing", Leiden University
//!   Medical Centre report, 2005): `wᵢ = p` where `yᵢ > zᵢ` (a peak) and
//!   `1 − p` elsewhere, `p` small (0.001–0.1).
//! * **Asymmetrically reweighted penalized least squares** (arPLS; Baek,
//!   Park, Ahn & Choo, *Analyst* 140, 2015): with `dᵢ = yᵢ − zᵢ` and `m⁻`,
//!   `s⁻` the mean and standard deviation (divisor `n⁻ − 1`) of the negative
//!   `dᵢ`, `wᵢ = 1 / (1 + exp(2(dᵢ − (2s⁻ − m⁻))/s⁻))`: points well above
//!   the noise band get no weight, with no `p` to choose. Iterations stop
//!   early (keeping the last baseline) when fewer than two residuals are
//!   negative.
//!
//! # SNIP
//! The statistics-sensitive nonlinear iterative peak-clipping filter (Ryan,
//! Clayton, Griffin, Sie & Cousens, *Nucl. Instrum. Methods B* 34, 1988; in
//! the form of Morháč, Kliman, Matoušek, Veselský & Turzo, *Nucl. Instrum.
//! Methods A* 401, 1997): for `p = 1, 2, …, m` (or `m … 1`, decreasing),
//! every point is replaced by `min(vᵢ, (vᵢ₋ₚ + vᵢ₊ₚ)/2)` (all points at once).
//! Peaks narrower than about `2m + 1` points are clipped away. The signal is
//! first extended by `m` points at each end along a straight line fitted by
//! least squares to its first (last) `m` points, so the ends are clipped
//! too. Optionally the values are first transformed by
//! `v = ln(ln(√(y + 1) + 1) + 1)` (log–log–square root, for counts spanning
//! several orders of magnitude) and the baseline transformed back.

use crate::banded::{difference_penalty, solve_with_diagonal};
use crate::{Result, invalid};

/// A baseline method and its settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BaselineMethod {
    /// Asymmetric least squares.
    Asls {
        /// Smoothness `λ` (larger: stiffer baseline).
        lambda: f64,
        /// Weight `p` of points above the baseline (0 < p < 1).
        p: f64,
    },
    /// Asymmetrically reweighted penalized least squares.
    Arpls {
        /// Smoothness `λ`.
        lambda: f64,
    },
    /// SNIP peak clipping.
    Snip {
        /// Largest half window `m` in points.
        half_window: usize,
        /// Run the windows from `m` down to 1 instead of up.
        decreasing: bool,
        /// Clip the log–log–square-root transformed values.
        lls: bool,
    },
}

impl BaselineMethod {
    /// A short description.
    #[must_use]
    pub fn label(&self) -> String {
        match *self {
            Self::Asls { lambda, p } => format!("Asymmetric least squares (λ = {lambda:e}, p = {p})"),
            Self::Arpls { lambda } => format!("arPLS (λ = {lambda:e})"),
            Self::Snip { half_window, decreasing, lls } => format!(
                "SNIP (half window {half_window} points{}{})",
                if decreasing { ", decreasing" } else { "" },
                if lls { ", log–log–square-root scale" } else { "" }
            ),
        }
    }
}

/// Iteration limits of the penalized methods.
const MAX_ITER: usize = 50;
const TOL: f64 = 1e-3;
const DIFF_ORDER: usize = 2;

fn check(y: &[f64], lambda: Option<f64>) -> Result<()> {
    if y.len() < DIFF_ORDER + 2 {
        return Err(invalid(format!("a baseline needs at least {} points", DIFF_ORDER + 2)));
    }
    if y.iter().any(|v| !v.is_finite()) {
        return Err(invalid("the signal must not contain missing or infinite values"));
    }
    if let Some(l) = lambda
        && !(l.is_finite() && l > 0.0)
    {
        return Err(invalid("λ must be positive"));
    }
    Ok(())
}

fn relative_difference(old: &[f64], new: &[f64]) -> f64 {
    let num: f64 = old.iter().zip(new).map(|(a, b)| (b - a) * (b - a)).sum::<f64>().sqrt();
    let den: f64 = old.iter().map(|a| a * a).sum::<f64>().sqrt();
    num / den.max(f64::MIN_POSITIVE)
}

fn reweighted(y: &[f64], lambda: f64, mut reweight: impl FnMut(&[f64]) -> Option<Vec<f64>>) -> Result<Vec<f64>> {
    let penalty = difference_penalty(y.len(), DIFF_ORDER, lambda);
    let mut w = vec![1.0; y.len()];
    let mut baseline = Vec::new();
    for _ in 0..=MAX_ITER {
        let rhs: Vec<f64> = w.iter().zip(y).map(|(a, b)| a * b).collect();
        baseline = solve_with_diagonal(&penalty, &w, &rhs)?;
        let Some(new) = reweight(&baseline) else { break };
        let done = relative_difference(&w, &new) < TOL;
        if done {
            break;
        }
        w = new;
    }
    Ok(baseline)
}

/// Asymmetric least squares baseline of `y` (see the module documentation).
///
/// # Errors
/// [`crate::StatError::InvalidArgument`] for fewer than four points, missing
/// values, `λ ≤ 0` or `p` outside (0, 1).
pub fn asls(y: &[f64], lambda: f64, p: f64) -> Result<Vec<f64>> {
    check(y, Some(lambda))?;
    if !(p > 0.0 && p < 1.0) {
        return Err(invalid("p must lie strictly between 0 and 1"));
    }
    reweighted(y, lambda, |z| Some(y.iter().zip(z).map(|(a, b)| if a > b { p } else { 1.0 - p }).collect()))
}

/// arPLS baseline of `y` (see the module documentation).
///
/// # Errors
/// As [`asls`].
pub fn arpls(y: &[f64], lambda: f64) -> Result<Vec<f64>> {
    check(y, Some(lambda))?;
    reweighted(y, lambda, |z| {
        let d: Vec<f64> = y.iter().zip(z).map(|(a, b)| a - b).collect();
        let neg: Vec<f64> = d.iter().copied().filter(|v| *v < 0.0).collect();
        if neg.len() < 2 {
            return None;
        }
        let m = neg.iter().sum::<f64>() / neg.len() as f64;
        let s = (neg.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / (neg.len() - 1) as f64).sqrt();
        let s = s.max(f64::MIN_POSITIVE);
        Some(d.iter().map(|&di| 1.0 / (1.0 + (2.0 * (di - (2.0 * s - m)) / s).exp())).collect())
    })
}

/// Least-squares line through `(i, v[i])` for `i` in `range`, evaluated at `t`.
fn line_at(v: &[f64], range: std::ops::Range<usize>, t: f64) -> f64 {
    let k = range.len() as f64;
    let mx = range.clone().map(|i| i as f64).sum::<f64>() / k;
    let my = range.clone().map(|i| v[i]).sum::<f64>() / k;
    let sxx: f64 = range.clone().map(|i| (i as f64 - mx).powi(2)).sum();
    let sxy: f64 = range.map(|i| (i as f64 - mx) * (v[i] - my)).sum();
    let slope = if sxx > 0.0 { sxy / sxx } else { 0.0 };
    my + slope * (t - mx)
}

/// SNIP baseline of `y` (see the module documentation). `half_window` is
/// capped at `(n − 1)/2`.
///
/// # Errors
/// [`crate::StatError::InvalidArgument`] for fewer than four points, missing
/// values, a zero half window, or values below −1 with the log–log–square-root
/// transform.
pub fn snip(y: &[f64], half_window: usize, decreasing: bool, lls: bool) -> Result<Vec<f64>> {
    check(y, None)?;
    if half_window == 0 {
        return Err(invalid("the SNIP half window must be at least one point"));
    }
    if lls && y.iter().any(|v| *v < -1.0) {
        return Err(invalid("the log–log–square-root transform needs values of at least −1"));
    }
    let n = y.len();
    let m = half_window.min((n - 1) / 2);
    let v: Vec<f64> = if lls { y.iter().map(|&a| ((a + 1.0).sqrt() + 1.0).ln().ln_1p()).collect() } else { y.to_vec() };
    // Extend by m points of a straight line at each end.
    let mut ext = Vec::with_capacity(n + 2 * m);
    for i in 0..m {
        let t = i as f64 - m as f64;
        ext.push(if m == 1 { v[0] } else { line_at(&v, 0..m, t) });
    }
    ext.extend_from_slice(&v);
    for i in 0..m {
        let t = (n + i) as f64;
        ext.push(if m == 1 { v[n - 1] } else { line_at(&v, n - m..n, t) });
    }
    let total = ext.len();
    let order: Vec<usize> = if decreasing { (1..=m).rev().collect() } else { (1..=m).collect() };
    let mut next = ext.clone();
    for p in order {
        for i in p..total - p {
            let mean = 0.5 * (ext[i - p] + ext[i + p]);
            next[i] = ext[i].min(mean);
        }
        ext.copy_from_slice(&next);
    }
    let out = &ext[m..m + n];
    Ok(if lls { out.iter().map(|&b| b.exp_m1().exp_m1().powi(2) - 1.0).collect() } else { out.to_vec() })
}

/// The baseline of `y` by `method`.
///
/// # Errors
/// As the method's function.
pub fn estimate_baseline(y: &[f64], method: &BaselineMethod) -> Result<Vec<f64>> {
    match *method {
        BaselineMethod::Asls { lambda, p } => asls(y, lambda, p),
        BaselineMethod::Arpls { lambda } => arpls(y, lambda),
        BaselineMethod::Snip { half_window, decreasing, lls } => snip(y, half_window, decreasing, lls),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spectrum() -> (Vec<f64>, Vec<f64>) {
        let x: Vec<f64> = (0..400).map(|i| i as f64).collect();
        let base: Vec<f64> = x.iter().map(|t| 5.0 + 0.01 * t + 2.0 * (t / 150.0).sin()).collect();
        let y = x
            .iter()
            .zip(&base)
            .map(|(t, b)| b + 20.0 * (-(t - 120.0).powi(2) / 50.0).exp() + 12.0 * (-(t - 280.0).powi(2) / 30.0).exp())
            .collect();
        (base, y)
    }

    #[test]
    fn penalized_baselines_follow_the_background() {
        let (base, y) = spectrum();
        for z in [asls(&y, 1e5, 0.001).unwrap(), arpls(&y, 1e5).unwrap()] {
            let worst = z.iter().zip(&base).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
            assert!(worst < 0.6, "{worst}");
        }
    }

    #[test]
    fn snip_clips_narrow_peaks() {
        let (base, y) = spectrum();
        let z = snip(&y, 30, false, false).unwrap();
        let worst = z[40..360].iter().zip(&base[40..360]).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
        assert!(worst < 0.8, "{worst}");
        let zl = snip(&y, 30, false, true).unwrap();
        assert!(zl.iter().zip(&y).all(|(b, v)| *b <= v + 1e-9));
    }
}
