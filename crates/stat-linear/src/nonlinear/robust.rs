//! Robust nonlinear fitting and outlier identification (Motulsky & Brown,
//! "Detecting outliers when fitting data with nonlinear regression – a new
//! method based on robust nonlinear regression and the false discovery rate",
//! *BMC Bioinformatics* 7:123, 2006).
//!
//! # Robust scale
//! The robust standard deviation of the residuals is
//! `RSDR = P₆₈ · N/(N − K)`, where `P₆₈` is the 68.27th percentile of the
//! absolute (weighted) residuals (linear interpolation between order
//! statistics) — for normal errors about one standard deviation — and the
//! factor corrects for the `K` fitted parameters.
//!
//! # Robust fit (Cauchy / Lorentzian loss)
//! The estimates minimise `Σ ln(1 + (rᵢ/RSDR)²)`, the negative
//! log-likelihood of Cauchy-distributed errors, so a point far from the curve
//! has bounded influence. The minimum is found by iteratively reweighted
//! least squares: at a fixed scale the normal equations of the loss are
//! those of weighted least squares with `wᵢ = 1/(1 + (rᵢ/RSDR)²)`, so each
//! step refits with these weights (times any given weights) and recomputes
//! `RSDR` from the new residuals, as the paper does, until the estimates
//! stop changing (relative change below 10⁻⁷). Standard errors from the last
//! weighted fit treat the robust weights as known, so they are approximate.
//!
//! # Outliers (the paper's false-discovery-rate rule)
//! With the absolute residuals of the robust fit sorted from smallest to
//! largest (`i = 1..N`), each of the largest 30% (from `i = ⌊0.7N⌋`) is
//! given `tᵢ = |rᵢ|/RSDR` and a two-sided p-value `Pᵢ` from the t
//! distribution on `N − K` df. Moving up from the smallest of these, the
//! first `i` with `Pᵢ < αᵢ = Q(N − (i − 1))/N` marks that point and every
//! point further from the curve as outliers (a Benjamini–Hochberg threshold:
//! the largest residual is tested at `Q/N`). `Q` (1% recommended) bounds the
//! expected fraction of identified outliers that are false discoveries. The
//! paper then removes the outliers and refits by ordinary (or weighted)
//! least squares.

use super::{ModelFunction, NonlinearFit, NonlinearOptions, fit_nonlinear};
use stat_core::distributions::StudentsT;
use stat_core::{Result, StatError};

/// `RSDR = P₆₈ · N/(N − K)` of residuals `r` with `k` fitted parameters
/// (module documentation); `NaN` when `N ≤ K`.
#[must_use]
pub fn robust_sd(r: &[f64], k: usize) -> f64 {
    let n = r.len();
    if n <= k || n == 0 {
        return f64::NAN;
    }
    let mut a: Vec<f64> = r.iter().map(|v| v.abs()).collect();
    a.sort_by(f64::total_cmp);
    let h = 0.6827 * (n - 1) as f64;
    let lo = h.floor() as usize;
    let hi = (lo + 1).min(n - 1);
    let p68 = a[lo] + (h - lo as f64) * (a[hi] - a[lo]);
    p68 * n as f64 / (n - k) as f64
}

/// Indices of the outliers among residuals `r` of a fit with `k` parameters,
/// by the false-discovery-rate rule at level `q` (module documentation),
/// most extreme last.
///
/// # Errors
/// [`StatError::InvalidArgument`] for `q` outside (0, 1);
/// [`StatError::InsufficientData`] with no more residuals than parameters.
pub fn outliers(r: &[f64], k: usize, q: f64) -> Result<Vec<usize>> {
    if !(q > 0.0 && q < 1.0) {
        return Err(StatError::InvalidArgument { message: "Q must be between 0 and 1".into() });
    }
    let n = r.len();
    if n <= k {
        return Err(StatError::InsufficientData { required: k + 1, found: n });
    }
    let scale = robust_sd(r, k);
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| r[a].abs().total_cmp(&r[b].abs()));
    if scale.is_nan() || scale <= 0.0 {
        // Most points on the curve: anything off it is an outlier.
        return Ok(order.into_iter().filter(|&i| r[i] != 0.0).collect());
    }
    let t = StudentsT::new((n - k) as f64)?;
    let first = ((0.7 * n as f64) as usize).max(1);
    for i in first..=n {
        let idx = order[i - 1];
        let p = t.two_sided_p_value(r[idx].abs() / scale)?;
        let alpha = q * (n - (i - 1)) as f64 / n as f64;
        if p < alpha {
            return Ok(order[i - 1..].to_vec());
        }
    }
    Ok(Vec::new())
}

/// A robust fit.
#[derive(Debug, Clone)]
pub struct RobustFit {
    /// The last weighted least-squares fit; its estimates are the robust
    /// estimates (its standard errors are approximate).
    pub fit: NonlinearFit,
    /// Robust weights `1/(1 + (rᵢ/RSDR)²)` at the solution (without any
    /// given weights).
    pub robust_weights: Vec<f64>,
    /// `RSDR` at the solution.
    pub scale: f64,
    /// Reweighting iterations.
    pub iterations: usize,
    /// Whether the estimates settled within the iteration limit.
    pub converged: bool,
}

/// Fit `y = f(x; θ)` with the Cauchy loss (module documentation), from
/// `start`, with the settings of `options` (its weights, if any, multiply
/// the robust ones).
///
/// # Errors
/// As [`fit_nonlinear`].
pub fn fit_robust(
    model: &mut dyn ModelFunction,
    y: &[f64],
    start: &[f64],
    options: &NonlinearOptions,
) -> Result<RobustFit> {
    let n = y.len();
    let base: Vec<f64> = if options.weights.is_empty() { vec![1.0; n] } else { options.weights.clone() };
    let quick = NonlinearOptions { profile_intervals: false, skewness: false, ..options.clone() };
    let mut fit = fit_nonlinear(model, y, start, &quick)?;
    let mut theta = fit.estimates();
    let mut robust = vec![1.0; n];
    let mut scale = f64::NAN;
    let mut converged = false;
    let mut iterations = 0;
    for _ in 0..100 {
        iterations += 1;
        let wr: Vec<f64> = fit.residuals.iter().zip(&base).map(|(r, w)| r * w.sqrt()).collect();
        scale = robust_sd(&wr, fit.n_estimated);
        if scale.is_nan() || scale <= 0.0 {
            converged = true;
            break;
        }
        robust = wr.iter().map(|r| 1.0 / (1.0 + (r / scale).powi(2))).collect();
        let weights: Vec<f64> = robust.iter().zip(&base).map(|(a, b)| a * b).collect();
        let next = fit_nonlinear(model, y, &theta, &NonlinearOptions { weights, ..quick.clone() })?;
        let new_theta = next.estimates();
        let change =
            new_theta.iter().zip(&theta).map(|(a, b)| (a - b).abs() / (b.abs().max(1e-8))).fold(0.0_f64, f64::max);
        fit = next;
        theta = new_theta;
        if change < 1e-7 {
            converged = true;
            break;
        }
    }
    let weights: Vec<f64> = robust.iter().zip(&base).map(|(a, b)| a * b).collect();
    let mut fit = fit_nonlinear(model, y, &theta, &NonlinearOptions { weights, ..options.clone() })?;
    if !converged {
        fit.warnings.push(format!(
            "The robust weights had not settled after {iterations} reweighting steps; the estimates may not be the \
             minimum of the robust loss."
        ));
    }
    Ok(RobustFit { fit, robust_weights: robust, scale, iterations, converged })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn robust_sd_of_known_values() {
        // |r| sorted 1..10: position 0.6827·9 = 6.1443 → 7 + 0.1443 = 7.1443.
        let r: Vec<f64> = (1..=10).map(|v| if v % 2 == 0 { -f64::from(v) } else { f64::from(v) }).collect();
        let s = robust_sd(&r, 2);
        assert!((s - 7.1443 * 10.0 / 8.0).abs() < 1e-12, "{s}");
    }

    #[test]
    fn one_gross_outlier_is_found_and_ordinary_noise_is_not() {
        // Deterministic near-normal residuals (scaled normal quantiles).
        let normal = stat_core::distributions::Normal::new(0.0, 1.0).unwrap();
        use stat_core::distributions::ContinuousDistribution;
        let mut r: Vec<f64> = (0..30).map(|i| normal.quantile((f64::from(i) + 0.5) / 30.0).unwrap()).collect();
        // Interleave signs order-independently.
        r.swap(0, 29);
        assert!(outliers(&r, 3, 0.01).unwrap().is_empty());
        r[7] = 12.0;
        assert_eq!(outliers(&r, 3, 0.01).unwrap(), vec![7]);
    }
}
