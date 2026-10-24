//! Ordinal responses: the cumulative logit (proportional odds) mixed model
//!
//! ```text
//! logit P(Yᵢ ≤ k | b) = αₖ − ηᵢ,   ηᵢ = offsetᵢ + xᵢᵀβ + zᵢᵀb,   k = 1 … K−1,
//! ```
//!
//! with thresholds `α₁ < … < α_{K−1}` in place of an intercept (Hedeker &
//! Gibbons, "A random-effects ordinal regression model for multilevel
//! analysis", *Biometrics* 50, 1994; Agresti, *Analysis of Ordinal
//! Categorical Data*, 2nd ed., 2010, ch. 10). A positive coefficient moves
//! the response towards higher categories; `exp(β)` is the odds ratio of a
//! higher rather than a lower category.
//!
//! The likelihood integrates over the random effects as for the other
//! families: the Laplace approximation at the conditional mode of the
//! spherical effects u (found by Newton's method, which is penalized
//! iteratively reweighted least squares with the observed information
//! `−∂²ℓᵢ/∂ηᵢ²` as weights; the cumulative logit log-likelihood is concave in
//! η), or adaptive Gauss–Hermite quadrature for a single random intercept.
//! θ, the thresholds (as `α₁` and the logs of the gaps, so that they stay
//! ordered) and β are optimised together by quasi-Newton steps polished by
//! Nelder–Mead; standard errors come from the Hessian of the approximate
//! deviance in (θ, α, β).

use stat_core::quadrature::gauss_hermite;
use stat_core::{Result, StatError};

use crate::dense::{Square, spd_inverse};
use crate::family::MixedFamily;
use crate::fit::{
    FixedEffect, MixedFit, MixedOptions, Mode, Problem, coefficient, components, delta_se, interior_theta, modes_of,
    n_levels, satterthwaite_limits, singular_warning, to_matrix,
};
use crate::optimize::{bfgs, hessian, nelder_mead};

fn logistic(x: f64) -> f64 {
    if x >= 0.0 {
        1.0 / (1.0 + (-x).exp())
    } else {
        let e = x.exp();
        e / (1.0 + e)
    }
}

/// Log-likelihood of a row in category `y` at linear predictor `eta`, with
/// its first and second derivatives in η.
pub(crate) fn ordinal_row(alpha: &[f64], y: usize, eta: f64) -> (f64, f64, f64) {
    let k = alpha.len() + 1;
    // Upper and lower cut points on the logit scale.
    let upper = (y < k - 1).then(|| alpha[y] - eta);
    let lower = (y > 0).then(|| alpha[y - 1] - eta);
    let cdf = |x: Option<f64>, inf: f64| x.map_or(inf, logistic);
    // The logistic density e^{−|x|}/(1 + e^{−|x|})² (accurate in the tails)
    // and its derivative f(x)(1 − 2F(x)) = −f(x) tanh(x/2).
    let pdf = |v: f64| {
        let e = (-v.abs()).exp();
        e / ((1.0 + e) * (1.0 + e))
    };
    let dens = |x: Option<f64>| x.map_or(0.0, pdf);
    let dens1 = |x: Option<f64>| x.map_or(0.0, |v| -pdf(v) * (0.5 * v).tanh());
    // P = F(a) − F(b), from the upper tails when both points are large.
    let p = match (upper, lower) {
        (_, Some(b)) if b > 0.0 => logistic(-b) - upper.map_or(0.0, |a| logistic(-a)),
        _ => cdf(upper, 1.0) - cdf(lower, 0.0),
    }
    .max(1e-300);
    let d1 = (dens(lower) - dens(upper)) / p;
    let d2 = (dens1(upper) - dens1(lower)) / p - d1 * d1;
    (p.ln(), d1, d2)
}

/// Category probabilities at linear predictor `eta`.
#[must_use]
pub fn category_probabilities(alpha: &[f64], eta: f64) -> Vec<f64> {
    let k = alpha.len() + 1;
    let mut out = Vec::with_capacity(k);
    let mut prev = 0.0;
    for j in 0..k {
        let c = if j + 1 < k { logistic(alpha[j] - eta) } else { 1.0 };
        out.push((c - prev).max(0.0));
        prev = c;
    }
    out
}

/// Thresholds from `α₁` and log gaps.
fn thresholds_of(raw: &[f64]) -> Vec<f64> {
    let mut out = Vec::with_capacity(raw.len());
    for (j, &r) in raw.iter().enumerate() {
        out.push(if j == 0 { r } else { out[j - 1] + r.exp() });
    }
    out
}

impl Problem<'_> {
    fn category(&self, i: usize) -> usize {
        self.model.y[i] as usize
    }

    /// Conditional mode of u at (θ, α, β) by Newton's method.
    fn ordinal_mode(&self, theta: &[f64], alpha: &[f64], beta: &[f64], warm: Option<&Mode>) -> Option<Mode> {
        let n = self.n;
        let a = self.st.a_rows(theta);
        let xb = self.st.fixed_part(beta);
        let base: Vec<f64> = (0..n).map(|i| self.offset[i] + xb[i]).collect();
        let eta_of = |u: &[f64]| -> Vec<f64> {
            let zu = self.st.random_part(&a, u);
            (0..n).map(|i| base[i] + zu[i]).collect()
        };
        let objective = |eta: &[f64], u: &[f64]| -> f64 {
            let lf: f64 = (0..n).map(|i| self.w[i] * ordinal_row(alpha, self.category(i), eta[i]).0).sum();
            -2.0 * lf + u.iter().map(|x| x * x).sum::<f64>()
        };
        let mut u = warm.map_or_else(|| vec![0.0; self.st.q_total], |m| m.u.clone());
        let mut eta = eta_of(&u);
        let mut pdev = objective(&eta, &u);
        let weights_at = |eta: &[f64]| -> (Vec<f64>, Vec<f64>) {
            let mut w = Vec::with_capacity(n);
            let mut g = Vec::with_capacity(n);
            for i in 0..n {
                let (_, d1, d2) = ordinal_row(alpha, self.category(i), eta[i]);
                w.push((-d2 * self.w[i]).max(1e-12));
                g.push(d1 * self.w[i]);
            }
            (w, g)
        };
        for _ in 0..100 {
            let (w, g) = weights_at(&eta);
            let z: Vec<f64> = (0..n).map(|i| eta[i] - base[i] + g[i] / w[i]).collect();
            let sol = self.st.solve(&a, &w, &z, false)?;
            let mut nu = sol.u;
            let mut neta = eta_of(&nu);
            let mut npdev = objective(&neta, &nu);
            let mut halvings = 0;
            while (npdev.is_nan() || npdev > pdev + 1e-10 * pdev.abs().max(1.0)) && halvings < 30 {
                halvings += 1;
                for (x, o) in nu.iter_mut().zip(&u) {
                    *x = 0.5 * (*x + o);
                }
                neta = eta_of(&nu);
                npdev = objective(&neta, &nu);
            }
            if !npdev.is_finite() {
                return None;
            }
            let change = (pdev - npdev).abs();
            let step: f64 = nu.iter().zip(&u).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
            u = nu;
            eta = neta;
            pdev = npdev;
            if change <= 1e-12 * (pdev.abs() + 1.0)
                && step <= 1e-9 * (1.0 + u.iter().map(|v| v.abs()).fold(0.0, f64::max))
            {
                break;
            }
        }
        let (weights, _) = weights_at(&eta);
        let zeros = vec![0.0; n];
        let sol = self.st.solve(&a, &weights, &zeros, false)?;
        let log_f: f64 = (0..n).map(|i| self.w[i] * ordinal_row(alpha, self.category(i), eta[i]).0).sum();
        Some(Mode { u, beta: beta.to_vec(), mu: eta.clone(), eta, log_f, logdet_u: sol.logdet_u, weights })
    }
}

/// Fit the cumulative logit mixed model (see the module notes).
pub(crate) fn fit_ordinal(problem: &Problem<'_>, options: &MixedOptions) -> Result<MixedFit> {
    let st = &problem.st;
    let (n, p, nt) = (problem.n, problem.p, st.n_theta);
    let k = problem.model.categories.len();
    let cuts = k - 1;
    // Start: thresholds at the logits of the weighted cumulative proportions.
    let total: f64 = problem.w.iter().sum();
    let mut share = vec![0.0; k];
    for i in 0..n {
        share[problem.category(i)] += problem.w[i];
    }
    if share.iter().any(|&s| s <= 0.0) {
        return Err(StatError::InvalidArgument {
            message: "every category of an ordinal response needs observations".to_owned(),
        });
    }
    let mut alpha0 = Vec::with_capacity(cuts);
    let mut cum = 0.0;
    for s in &share[..cuts] {
        cum += s / total;
        let c = cum.clamp(1e-6, 1.0 - 1e-6);
        alpha0.push((c / (1.0 - c)).ln());
    }
    let mut x0 = st.theta_start();
    x0.extend(alpha0.iter().enumerate().map(|(j, &a)| if j == 0 { a } else { (a - alpha0[j - 1]).max(1e-3).ln() }));
    x0.extend(std::iter::repeat_n(0.0, p));
    let mut lower = st.theta_lower();
    lower.extend(std::iter::repeat_n(f64::NEG_INFINITY, cuts + p));
    let nodes = gauss_hermite(options.quadrature_points);
    let agq = options.quadrature_points > 1;
    let evals = std::cell::Cell::new(0usize);
    let warm = std::cell::RefCell::new(None::<Mode>);
    // The approximate deviance at (θ, α, β) with α given directly.
    let deviance = |x: &[f64], alpha: &[f64]| -> f64 {
        evals.set(evals.get() + 1);
        if alpha.windows(2).any(|w| w[1] <= w[0]) {
            return f64::INFINITY;
        }
        let th = &x[..nt];
        let beta = &x[nt + cuts..nt + cuts + p];
        let found = problem.ordinal_mode(th, alpha, beta, warm.borrow().as_ref());
        match found.or_else(|| problem.ordinal_mode(th, alpha, beta, None)) {
            Some(m) => {
                let d = if agq {
                    let log_f = |i: usize, eta: f64| problem.w[i] * ordinal_row(alpha, problem.category(i), eta).0;
                    problem.agq_with(th, &m, &nodes, &log_f)
                } else {
                    Problem::laplace(&m)
                };
                if d.is_finite() {
                    *warm.borrow_mut() = Some(m);
                }
                d
            }
            None => f64::INFINITY,
        }
    };
    let mut raw = |x: &[f64]| deviance(x, &thresholds_of(&x[nt..nt + cuts]));
    let m1 = bfgs(&mut raw, &x0, &lower, options.max_evaluations);
    let steps: Vec<f64> = m1.x.iter().map(|v| 0.05 * v.abs().max(0.1)).collect();
    let m2 = nelder_mead(&mut raw, &m1.x, &lower, &steps, options.max_evaluations / 4);
    let best = if m2.f < m1.f { m2.clone() } else { m1.clone() };
    let converged = m1.converged || m2.converged;
    // Direct parametrisation (θ, α, β) for inference.
    let alpha = thresholds_of(&best.x[nt..nt + cuts]);
    let mut xhat: Vec<f64> = best.x[..nt].to_vec();
    xhat.extend(&alpha);
    xhat.extend(&best.x[nt + cuts..]);
    let theta = xhat[..nt].to_vec();
    let beta = xhat[nt + cuts..].to_vec();
    let mut direct = |x: &[f64]| deviance(x, &x[nt..nt + cuts]);
    let crit = direct(&xhat);
    let mode = warm
        .borrow()
        .clone()
        .ok_or(StatError::ConvergenceFailure { routine: "ordinal mixed model", iterations: evals.get() })?;
    let mut vary = interior_theta(problem, &theta);
    vary.extend(nt..nt + cuts + p);
    let h = hessian(&mut direct, &xhat, &lower, &vary);
    // Restore the mode at the estimates (the Hessian moved it).
    let mode = problem.ordinal_mode(&theta, &alpha, &beta, Some(&mode)).unwrap_or(mode);
    let mut warnings = Vec::new();
    let inv = spd_inverse(&h).map(|mut m| {
        m.a.iter_mut().for_each(|v| *v *= 2.0);
        m
    });
    let ab_pos: Vec<usize> =
        (0..cuts + p).map(|j| vary.iter().position(|&v| v == nt + j).expect("thresholds and β vary")).collect();
    let cov_ab = match &inv {
        Some(c) => c.sub(&ab_pos),
        None => {
            warnings.push(
                "The curvature of the likelihood is not positive definite: standard errors are not available."
                    .to_owned(),
            );
            Square { n: cuts + p, a: vec![f64::NAN; (cuts + p) * (cuts + p)] }
        }
    };
    let level = options.confidence_level;
    let names = &problem.model.categories;
    let thresholds: Vec<FixedEffect> = (0..cuts)
        .map(|j| {
            let name = format!("{}|{}", names[j], names[j + 1]);
            coefficient(&name, alpha[j], cov_ab.get(j, j).max(0.0).sqrt(), None, level)
        })
        .collect();
    let fixed: Vec<FixedEffect> = (0..p)
        .map(|j| {
            let c = cuts + j;
            coefficient(&problem.model.x_names[j], beta[j], cov_ab.get(c, c).max(0.0).sqrt(), None, level)
        })
        .collect();
    let beta_idx: Vec<usize> = (cuts..cuts + p).collect();
    let cov_beta = cov_ab.sub(&beta_idx);
    let (mut comps, cors, _) = components(problem, &theta, 1.0);
    if let Some(c) = &inv {
        let theta_pos: Vec<usize> = (0..vary.len()).filter(|&i| vary[i] < nt).collect();
        let a = c.sub(&theta_pos);
        let theta_vary: Vec<usize> = theta_pos.iter().map(|&i| vary[i]).collect();
        let g = |x: &[f64]| components(problem, x, 1.0).2;
        let ses = delta_se(&g, &theta, &theta_vary, &a);
        for (comp, se) in comps.iter_mut().zip(&ses) {
            comp.std_error = Some(*se);
            comp.interval = satterthwaite_limits(comp.variance, *se, level);
        }
    }
    let blocks = {
        let a_rows = st.a_rows(&theta);
        let z = vec![0.0; n];
        st.solve(&a_rows, &mode.weights, &z, false).map(|s| st.conditional_blocks(&s))
    }
    .ok_or(StatError::ConvergenceFailure { routine: "ordinal mixed model", iterations: evals.get() })?;
    let modes = modes_of(problem, &theta, &mode.u, &blocks, 1.0);
    let xb = st.fixed_part(&beta);
    let marginal: Vec<f64> = (0..n).map(|i| problem.offset[i] + xb[i]).collect();
    if let Some(wn) = singular_warning(problem, &theta) {
        warnings.push(wn);
    }
    if !converged {
        warnings.push("The optimizer reached its evaluation limit before converging.".to_owned());
    }
    if options.robust {
        warnings.push("Robust standard errors are available for normal responses only.".to_owned());
    }
    let n_par = cuts + p + nt;
    let probs: Vec<Vec<f64>> = mode.eta.iter().map(|&e| category_probabilities(&alpha, e)).collect();
    let probs_marginal: Vec<Vec<f64>> = marginal.iter().map(|&e| category_probabilities(&alpha, e)).collect();
    Ok(MixedFit {
        family: MixedFamily::Ordinal,
        method: if agq {
            format!("Adaptive Gauss–Hermite quadrature ({} points)", options.quadrature_points)
        } else {
            "Laplace approximation".to_owned()
        },
        n_obs: n,
        fixed,
        fixed_covariance: to_matrix(&cov_beta),
        variance_components: comps,
        correlations: cors,
        residual: None,
        scale: None,
        minus_two_log_likelihood: crit,
        aic: crit + 2.0 * n_par as f64,
        bic: crit + (n as f64).ln() * n_par as f64,
        n_parameters: n_par,
        n_levels: n_levels(problem),
        modes,
        linear_predictor: mode.eta.clone(),
        fitted: mode.eta.clone(),
        fitted_marginal: marginal,
        theta,
        converged,
        warnings,
        evaluations: evals.get(),
        variance_covariance: None,
        residual_by_group: Vec::new(),
        equal_variance_test: None,
        robust: None,
        df_label: String::new(),
        thresholds,
        ordinal_covariance: Some(to_matrix(&cov_ab)),
        category_probabilities: probs,
        category_probabilities_marginal: probs_marginal,
        confidence_level: options.confidence_level,
        satterthwaite: None,
        between_within: None,
        robust_parts: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Derivatives of the row log-likelihood against central differences,
    /// and probabilities that sum to 1.
    #[test]
    fn row_derivatives_and_probabilities() {
        let alpha = [-1.2, 0.3, 2.0];
        for y in 0..4 {
            for &eta in &[-3.0, -0.4, 0.0, 1.1, 5.0, 40.0, -40.0] {
                let (l, d1, d2) = ordinal_row(&alpha, y, eta);
                let h = 1e-5;
                let (lp, d1p, _) = ordinal_row(&alpha, y, eta + h);
                let (lm, d1m, _) = ordinal_row(&alpha, y, eta - h);
                if l > -600.0 {
                    assert!((d1 - (lp - lm) / (2.0 * h)).abs() < 1e-6, "y={y} eta={eta}");
                    assert!((d2 - (d1p - d1m) / (2.0 * h)).abs() < 1e-5, "y={y} eta={eta}");
                    assert!(d2 <= 1e-15, "y={y} eta={eta} d2={d2}");
                }
                let probs = category_probabilities(&alpha, eta);
                assert!((probs.iter().sum::<f64>() - 1.0).abs() < 1e-12);
                if l > -600.0 && probs[y] > 1e-12 {
                    assert!((probs[y].ln() - l).abs() < 1e-9 * l.abs().max(1.0), "y={y} eta={eta}");
                }
            }
        }
        assert_eq!(thresholds_of(&[1.0, 0.0, 2f64.ln()]), vec![1.0, 2.0, 4.0]);
    }
}
