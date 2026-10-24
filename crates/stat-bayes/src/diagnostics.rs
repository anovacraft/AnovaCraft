//! Convergence diagnostics of Markov chain Monte Carlo draws (Vehtari,
//! Gelman, Simpson, Carpenter & Bürkner 2021, "Rank-normalization, folding,
//! and localization: an improved R̂ for assessing convergence of MCMC",
//! *Bayesian Analysis* 16(2)).
//!
//! * **R̂**: each chain is split in half (the middle draw of an odd-length
//!   chain is dropped); the pooled draws are rank-normalized,
//!   `z = Φ⁻¹((rank − ⅜)/(S + ¼))` with average ranks for ties; R̂ is the
//!   larger of the classic potential scale reduction
//!   `√(((n − 1)/n · W + B/n)/W)` of the rank-normalized draws and of the
//!   rank-normalized folded draws `|θ − median θ|`.
//! * **Effective sample size** of split chains from the autocorrelations,
//!   combined across chains as `ρ̂ₜ = 1 − (W − mean autocovariance at lag t)/var⁺`,
//!   summed over Geyer's (1992, *Stat. Sci.* 7) initial positive and
//!   monotone sequence: `ESS = mn/τ̂`, `τ̂ = −1 + 2Σρ̂ₜ` (at least
//!   `1/log₁₀(mn)`). **Bulk ESS** is that of the rank-normalized draws;
//!   **tail ESS** the smaller of the ESS of the indicators `θ ≤ q₀.₀₅` and
//!   `θ ≤ q₀.₉₅`.
//! * **MCSE** of the mean: `sd/√ESS` (ESS of the untransformed draws).

use stat_core::{ContinuousDistribution, Normal};

/// Diagnostics of one parameter's draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParameterDiagnostics {
    /// Rank-normalized split R̂.
    pub rhat: f64,
    /// Bulk effective sample size.
    pub ess_bulk: f64,
    /// Tail effective sample size.
    pub ess_tail: f64,
    /// Monte Carlo standard error of the posterior mean.
    pub mcse_mean: f64,
}

/// Split each chain into halves.
fn split(chains: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let n = chains.iter().map(Vec::len).min().unwrap_or(0);
    let half = n / 2;
    let mut out = Vec::with_capacity(2 * chains.len());
    for c in chains {
        out.push(c[..half].to_vec());
        out.push(c[n - half..n].to_vec());
    }
    out
}

/// Average ranks (1-based) of all values.
fn ranks(values: &[f64]) -> Vec<f64> {
    let mut idx: Vec<usize> = (0..values.len()).collect();
    idx.sort_by(|&a, &b| values[a].total_cmp(&values[b]));
    let mut r = vec![0.0; values.len()];
    let mut i = 0;
    while i < idx.len() {
        let mut j = i;
        while j + 1 < idx.len() && values[idx[j + 1]] == values[idx[i]] {
            j += 1;
        }
        let avg = (i + j) as f64 / 2.0 + 1.0;
        for k in i..=j {
            r[idx[k]] = avg;
        }
        i = j + 1;
    }
    r
}

/// Rank-normalized copy of the chains.
fn z_scale(chains: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let flat: Vec<f64> = chains.iter().flatten().copied().collect();
    let s = flat.len() as f64;
    let r = ranks(&flat);
    let n = Normal::standard();
    let mut out = Vec::with_capacity(chains.len());
    let mut k = 0;
    for c in chains {
        out.push(
            c.iter()
                .map(|_| {
                    let v = n.quantile((r[k] - 0.375) / (s + 0.25)).unwrap_or(0.0);
                    k += 1;
                    v
                })
                .collect(),
        );
    }
    out
}

fn mean(x: &[f64]) -> f64 {
    x.iter().sum::<f64>() / x.len() as f64
}

fn var(x: &[f64]) -> f64 {
    let m = mean(x);
    x.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / (x.len() as f64 - 1.0)
}

/// Classic potential scale reduction factor.
#[must_use]
pub fn rhat_basic(chains: &[Vec<f64>]) -> f64 {
    let m = chains.len() as f64;
    let n = chains.first().map_or(0, Vec::len) as f64;
    if m < 2.0 || n < 2.0 {
        return f64::NAN;
    }
    let means: Vec<f64> = chains.iter().map(|c| mean(c)).collect();
    let b = n * var(&means);
    let w = chains.iter().map(|c| var(c)).sum::<f64>() / m;
    if w.is_nan() || w <= 0.0 {
        return if b == 0.0 { 1.0 } else { f64::NAN };
    }
    (((n - 1.0) / n * w + b / n) / w).sqrt()
}

/// Effective sample size of equal-length chains (no splitting).
#[must_use]
pub fn ess_basic(chains: &[Vec<f64>]) -> f64 {
    let m = chains.len();
    let n = chains.first().map_or(0, Vec::len);
    if n < 3 || chains.iter().any(|c| c.len() != n || c.iter().any(|v| !v.is_finite())) {
        return f64::NAN;
    }
    let centred: Vec<Vec<f64>> = chains
        .iter()
        .map(|c| {
            let mu = mean(c);
            c.iter().map(|v| v - mu).collect()
        })
        .collect();
    let nf = n as f64;
    // Biased autocovariance at lag t, averaged over chains.
    let acov = |t: usize| -> f64 {
        centred.iter().map(|c| (0..n - t).map(|i| c[i] * c[i + t]).sum::<f64>() / nf).sum::<f64>() / m as f64
    };
    let a0 = acov(0);
    if a0.is_nan() || a0 <= 0.0 {
        return f64::NAN;
    }
    let mean_var = a0 * nf / (nf - 1.0);
    let mut var_plus = mean_var * (nf - 1.0) / nf;
    if m > 1 {
        let means: Vec<f64> = chains.iter().map(|c| mean(c)).collect();
        var_plus += var(&means);
    }
    let rho = |t: usize| 1.0 - (mean_var - acov(t)) / var_plus;
    let mut rho_t = vec![0.0; n];
    let mut t = 0;
    let mut even = 1.0;
    let mut odd = rho(1);
    rho_t[0] = even;
    rho_t[1] = odd;
    while t + 5 < n && even + odd > 0.0 {
        t += 2;
        even = rho(t);
        odd = rho(t + 1);
        if even + odd >= 0.0 {
            rho_t[t] = even;
            rho_t[t + 1] = odd;
        }
    }
    let max_t = t;
    if even > 0.0 {
        rho_t[max_t] = even;
    }
    // Initial monotone sequence.
    let mut t = 0;
    while t + 4 <= max_t {
        t += 2;
        if rho_t[t] + rho_t[t + 1] > rho_t[t - 2] + rho_t[t - 1] {
            rho_t[t] = 0.5 * (rho_t[t - 2] + rho_t[t - 1]);
            rho_t[t + 1] = rho_t[t];
        }
    }
    let total = (m * n) as f64;
    let tau = (-1.0 + 2.0 * rho_t[..max_t].iter().sum::<f64>() + rho_t[max_t]).max(1.0 / total.log10());
    total / tau
}

/// Type-7 sample quantile (linear interpolation) of all draws.
fn quantile(chains: &[Vec<f64>], p: f64) -> f64 {
    let mut v: Vec<f64> = chains.iter().flatten().copied().collect();
    v.sort_by(f64::total_cmp);
    let h = (v.len() - 1) as f64 * p;
    let lo = h.floor() as usize;
    let hi = (lo + 1).min(v.len() - 1);
    v[lo] + (h - lo as f64) * (v[hi] - v[lo])
}

/// Rank-normalized split R̂.
#[must_use]
pub fn rhat(chains: &[Vec<f64>]) -> f64 {
    let s = split(chains);
    let med = quantile(&s, 0.5);
    let folded: Vec<Vec<f64>> = s.iter().map(|c| c.iter().map(|v| (v - med).abs()).collect()).collect();
    rhat_basic(&z_scale(&s)).max(rhat_basic(&z_scale(&folded)))
}

/// Bulk effective sample size.
#[must_use]
pub fn ess_bulk(chains: &[Vec<f64>]) -> f64 {
    ess_basic(&z_scale(&split(chains)))
}

/// Effective sample size of the indicator `θ ≤ q_p`.
fn ess_quantile(chains: &[Vec<f64>], p: f64) -> f64 {
    let q = quantile(chains, p);
    let ind: Vec<Vec<f64>> = chains.iter().map(|c| c.iter().map(|&v| f64::from(u8::from(v <= q))).collect()).collect();
    ess_basic(&split(&ind))
}

/// Tail effective sample size.
#[must_use]
pub fn ess_tail(chains: &[Vec<f64>]) -> f64 {
    ess_quantile(chains, 0.05).min(ess_quantile(chains, 0.95))
}

/// Effective sample size for the mean (split chains, untransformed).
#[must_use]
pub fn ess_mean(chains: &[Vec<f64>]) -> f64 {
    ess_basic(&split(chains))
}

/// All diagnostics of one parameter (chains of equal length).
#[must_use]
pub fn diagnose(chains: &[Vec<f64>]) -> ParameterDiagnostics {
    let flat: Vec<f64> = chains.iter().flatten().copied().collect();
    let sd = if flat.len() > 1 { var(&flat).sqrt() } else { f64::NAN };
    ParameterDiagnostics {
        rhat: rhat(chains),
        ess_bulk: ess_bulk(chains),
        ess_tail: ess_tail(chains),
        mcse_mean: sd / ess_mean(chains).sqrt(),
    }
}

/// Autocorrelation of one chain at lags `0..=max_lag`.
#[must_use]
pub fn autocorrelation(chain: &[f64], max_lag: usize) -> Vec<f64> {
    let n = chain.len();
    if n < 2 {
        return Vec::new();
    }
    let mu = mean(chain);
    let c: Vec<f64> = chain.iter().map(|v| v - mu).collect();
    let a0: f64 = c.iter().map(|v| v * v).sum();
    (0..=max_lag.min(n - 1)).map(|t| (0..n - t).map(|i| c[i] * c[i + t]).sum::<f64>() / a0).collect()
}

/// Fractional ranks of every draw (pooled over chains), per chain: for
/// rank histograms.
#[must_use]
pub fn chain_ranks(chains: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let flat: Vec<f64> = chains.iter().flatten().copied().collect();
    let r = ranks(&flat);
    let mut k = 0;
    chains
        .iter()
        .map(|c| {
            c.iter()
                .map(|_| {
                    let v = r[k];
                    k += 1;
                    v
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_draws_have_full_ess_and_unit_rhat() {
        let mut rng = stat_core::rng::Rng::new(5);
        let chains: Vec<Vec<f64>> = (0..4).map(|_| (0..1000).map(|_| rng.normal()).collect()).collect();
        let d = diagnose(&chains);
        assert!((d.rhat - 1.0).abs() < 0.01);
        assert!(d.ess_bulk > 3000.0 && d.ess_bulk < 5000.0);
        // Shifted chain: R̂ well above 1.
        let mut bad = chains.clone();
        bad[0].iter_mut().for_each(|v| *v += 3.0);
        assert!(rhat(&bad) > 1.2);
    }
}
