//! Leave-one-out cross-validation by Pareto-smoothed importance sampling
//! (PSIS-LOO; Vehtari, Gelman & Gabry 2017, *Stat. Comput.* 27; Vehtari,
//! Simpson, Gelman, Yao & Gabry 2024, "Pareto smoothed importance
//! sampling", *JMLR* 25) and the widely applicable information criterion
//! (WAIC; Watanabe 2010, *JMLR* 11).
//!
//! For each observation `i` the importance ratios of its leave-one-out
//! posterior are `1/p(yᵢ │ θˢ)`. The largest `M = min(⌈0.2S⌉, ⌈3√S⌉)` are
//! replaced by the expected order statistics of a generalized Pareto
//! distribution fitted to them (Zhang & Stephens 2009, *Technometrics* 51,
//! profile-likelihood estimator over a grid, with the shape shrunk toward ½
//! by a weak prior worth ten observations); smoothed values are truncated at
//! the largest raw ratio. The fitted shape `k̂` diagnoses reliability:
//! below 0.7 good, otherwise the estimate for that observation is
//! unreliable. Then `elpd_loo,i = ln Σₛ wᵢˢ p(yᵢ │ θˢ) − ln Σₛ wᵢˢ`,
//! `p_loo = lpd − elpd_loo` with `lpd = Σᵢ ln (1/S Σₛ p(yᵢ │ θˢ))`, the LOO
//! information criterion `−2 elpd_loo`, and standard errors
//! `√(n var(elpd_loo,i))`. WAIC uses `p_waic = Σᵢ varₛ ln p(yᵢ │ θˢ)`.

/// PSIS-LOO and WAIC of a model.
#[derive(Debug, Clone, PartialEq)]
pub struct LooResult {
    /// Expected log pointwise predictive density (LOO).
    pub elpd_loo: f64,
    /// Its standard error.
    pub se_elpd_loo: f64,
    /// Effective number of parameters (LOO).
    pub p_loo: f64,
    /// LOO information criterion, `−2 elpd_loo`.
    pub looic: f64,
    /// WAIC expected log predictive density.
    pub elpd_waic: f64,
    /// Its standard error.
    pub se_elpd_waic: f64,
    /// Effective number of parameters (WAIC).
    pub p_waic: f64,
    /// `−2 elpd_waic`.
    pub waic: f64,
    /// Pareto shape `k̂` of each observation.
    pub pareto_k: Vec<f64>,
    /// Pointwise `elpd_loo,i`.
    pub pointwise: Vec<f64>,
}

fn log_sum_exp(x: &[f64]) -> f64 {
    let m = x.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if !m.is_finite() {
        return m;
    }
    m + x.iter().map(|v| (v - m).exp()).sum::<f64>().ln()
}

/// Generalized Pareto fit `(k, σ)` of positive exceedances `x` (sorted
/// ascending), Zhang & Stephens (2009) with the weakly informative prior.
#[must_use]
pub fn gpd_fit(x: &[f64]) -> (f64, f64) {
    let n = x.len();
    let nf = n as f64;
    let prior = 3.0;
    let m = 30 + (nf.sqrt().floor() as usize);
    let xstar = x[((nf / 4.0 + 0.5).floor() as usize).max(1) - 1];
    let theta: Vec<f64> =
        (1..=m).map(|j| 1.0 / x[n - 1] + (1.0 - (m as f64 / (j as f64 - 0.5)).sqrt()) / prior / xstar).collect();
    let l_theta: Vec<f64> = theta
        .iter()
        .map(|&t| {
            let a = -t;
            let k = x.iter().map(|&v| (a * v).ln_1p()).sum::<f64>() / nf;
            nf * ((a / k).ln() - k - 1.0)
        })
        .collect();
    let lse = log_sum_exp(&l_theta);
    let theta_hat: f64 = theta.iter().zip(&l_theta).map(|(&t, &l)| t * (l - lse).exp()).sum();
    let mut k = x.iter().map(|&v| (-theta_hat * v).ln_1p()).sum::<f64>() / nf;
    let sigma = -k / theta_hat;
    let a = 10.0;
    k = k * nf / (nf + a) + a * 0.5 / (nf + a);
    if k.is_nan() {
        k = f64::INFINITY;
    }
    (k, sigma)
}

/// Pareto-smoothed log weights of one observation and the shape `k̂`.
#[must_use]
pub fn psis(log_ratios: &[f64], r_eff: f64) -> (Vec<f64>, f64) {
    let s = log_ratios.len();
    let max = log_ratios.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let mut lw: Vec<f64> = log_ratios.iter().map(|v| v - max).collect();
    let tail = ((0.2 * s as f64).ceil() as usize).min((3.0 * (s as f64 / r_eff).sqrt()).ceil() as usize);
    let mut khat = f64::INFINITY;
    if tail >= 5 && tail < s {
        let mut ord: Vec<usize> = (0..s).collect();
        ord.sort_by(|&a, &b| lw[a].total_cmp(&lw[b]));
        let ids = &ord[s - tail..];
        let lo = lw[ids[0]];
        let hi = lw[ids[tail - 1]];
        if (hi - lo).abs() >= f64::EPSILON / 100.0 {
            let cutoff = lw[ord[s - tail - 1]];
            let ec = cutoff.exp();
            let ex: Vec<f64> = ids.iter().map(|&i| lw[i].exp() - ec).collect();
            let (k, sigma) = gpd_fit(&ex);
            khat = k;
            if k.is_finite() && sigma > 0.0 {
                for (j, &i) in ids.iter().enumerate() {
                    let p = (j as f64 + 0.5) / tail as f64;
                    let q = sigma * (-k * (-p).ln_1p()).exp_m1() / k + ec;
                    lw[i] = q.ln();
                }
            }
        }
    }
    for v in &mut lw {
        if *v > 0.0 {
            *v = 0.0;
        }
    }
    (lw, khat)
}

/// PSIS-LOO and WAIC from a pointwise log-likelihood matrix
/// `log_lik[s][i]` (draws × observations), with relative efficiencies
/// `r_eff` per observation (`None`: all 1).
#[must_use]
pub fn loo(log_lik: &[Vec<f64>], r_eff: Option<&[f64]>) -> LooResult {
    let s = log_lik.len();
    let n = log_lik.first().map_or(0, Vec::len);
    let ln_s = (s as f64).ln();
    let mut elpd_i = vec![0.0; n];
    let mut lpd_i = vec![0.0; n];
    let mut waic_i = vec![0.0; n];
    let mut pw_i = vec![0.0; n];
    let mut ks = vec![0.0; n];
    for i in 0..n {
        let ll: Vec<f64> = (0..s).map(|d| log_lik[d][i]).collect();
        let neg: Vec<f64> = ll.iter().map(|v| -v).collect();
        let (lw, k) = psis(&neg, r_eff.map_or(1.0, |r| r[i]));
        let num: Vec<f64> = lw.iter().zip(&ll).map(|(a, b)| a + b).collect();
        elpd_i[i] = log_sum_exp(&num) - log_sum_exp(&lw);
        lpd_i[i] = log_sum_exp(&ll) - ln_s;
        let m = ll.iter().sum::<f64>() / s as f64;
        pw_i[i] = ll.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / (s as f64 - 1.0);
        waic_i[i] = lpd_i[i] - pw_i[i];
        ks[i] = k;
    }
    let se = |v: &[f64]| {
        let m = v.iter().sum::<f64>() / n as f64;
        (n as f64 * v.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (n as f64 - 1.0)).sqrt()
    };
    let elpd: f64 = elpd_i.iter().sum();
    let lpd: f64 = lpd_i.iter().sum();
    let ew: f64 = waic_i.iter().sum();
    LooResult {
        elpd_loo: elpd,
        se_elpd_loo: se(&elpd_i),
        p_loo: lpd - elpd,
        looic: -2.0 * elpd,
        elpd_waic: ew,
        se_elpd_waic: se(&waic_i),
        p_waic: pw_i.iter().sum(),
        waic: -2.0 * ew,
        pareto_k: ks,
        pointwise: elpd_i,
    }
}
