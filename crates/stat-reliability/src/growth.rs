//! Reliability growth: the power-law nonhomogeneous Poisson process (the
//! Crow–AMSAA model) fitted to the failure times of one developing system.
//!
//! # Model
//! Failures of a system under development, at cumulative operating times
//! `t₁ < t₂ < … < tₙ`, follow a Poisson process with intensity
//!
//! ```text
//! u(t) = λ β t^(β−1),      E N(t) = λ t^β,
//! ```
//!
//! so `β < 1` means failures arrive ever more slowly (reliability is
//! growing) and `β = 1` is a constant failure rate. The instantaneous mean
//! time between failures is `M(t) = 1/u(t)` (Crow 1974, "Reliability
//! analysis for complex, repairable systems", in *Reliability and
//! Biometry*, SIAM; MIL-HDBK-189C, *Reliability Growth Management*, 2011,
//! §5.2).
//!
//! # Estimation
//! For a test **time-terminated** at `T` (`T ≥ tₙ`) or **failure-terminated**
//! at `T = tₙ` the maximum-likelihood estimates are
//!
//! ```text
//! β̂ = n / Σ ln(T/tᵢ),      λ̂ = n / T^β̂
//! ```
//!
//! (the term for `tₙ = T` is zero). Conditional on `n`, `2nβ/β̂` is χ² with
//! `2n` degrees of freedom (time-terminated) or `2(n − 1)` (failure-
//! terminated), which gives exact intervals for `β` and the unbiased
//! estimates `β̄ = (n − 1)β̂/n` and `β̄ = (n − 2)β̂/n` respectively (Crow 1974).
//! With the unbiased estimate, `λ̄ = n/T^β̄`.
//!
//! A **phase** that starts at `s > 0` (after design changes, with the
//! cumulative clock running on) is fitted to the failures in `(s, T]` with
//! the left-truncated likelihood
//! `ℓ = n ln λ + n ln β + (β − 1) Σ ln tᵢ − λ(T^β − s^β)`; `β̂` solves the
//! (monotone) profile score equation by bisection, and only the maximum-
//! likelihood estimate applies.
//!
//! # Intervals for intensity and MTBF
//! Wald intervals on the log scale from the observed information of
//! `(a, β)` with `a = ln λ + β ln T` (time measured in units of `T`, which
//! keeps the information well conditioned): with `r = s/T` and
//! `c = r^β/(1 − r^β)`,
//!
//! ```text
//! J = n [ 1          −c ln r          ]
//!       [ −c ln r    1/β² − c ln² r   ]
//! ```
//!
//! and `Var ln û(t) = gᵀ J⁻¹ g` with `g = (1, 1/β + ln(t/T))`. For `s = 0`
//! this gives `Var ln M̂(T) = 2/n`.
//!
//! # Goodness of fit
//! The Cramér–von Mises statistic of MIL-HDBK-189C (§5.2.5) for a phase
//! starting at zero, with `M = n` (time-terminated) or `M = n − 1`
//! (failure-terminated, the last failure left out) and the unbiased `β̄`:
//!
//! ```text
//! C²_M = 1/(12M) + Σᵢ₌₁ᴹ [ (tᵢ/T)^β̄ − (2i − 1)/(2M) ]².
//! ```
//!
//! Under the model the ratios `(tᵢ/T)^β` are ordered uniform values, and in
//! both cases `β̄ = (M − 1)/Σ ln(T/tᵢ)`, so the statistic's null distribution
//! depends on `M` only. Its p-value and critical value are simulated
//! (20 000 seeded replicates) rather than read from a table.

use stat_core::rng::Rng;
use stat_core::{ChiSquared, ContinuousDistribution, Normal, Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// How a growth test (or phase) ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Termination {
    /// Stopped at a chosen time `T`, at or after the last failure.
    Time,
    /// Stopped at the last failure.
    Failure,
}

impl Termination {
    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Time => "time-terminated",
            Self::Failure => "failure-terminated",
        }
    }
}

/// Which estimate of the shape `β` the fitted curve uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BetaEstimator {
    /// The maximum-likelihood estimate `β̂`.
    MaximumLikelihood,
    /// The unbiased estimate `β̄` (`(n − 1)β̂/n` time-terminated,
    /// `(n − 2)β̂/n` failure-terminated); phases that start after zero use
    /// `β̂`.
    #[default]
    Unbiased,
}

impl BetaEstimator {
    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::MaximumLikelihood => "maximum likelihood",
            Self::Unbiased => "unbiased",
        }
    }
}

/// The Cramér–von Mises goodness-of-fit test of the power-law model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CramerVonMises {
    /// `C²_M`.
    pub statistic: f64,
    /// Number of failure times in the statistic, `M`.
    pub m: usize,
    /// Simulated critical value at the requested significance level.
    pub critical_value: f64,
    /// Significance level of `critical_value`.
    pub alpha: f64,
    /// Simulated p-value.
    pub p_value: f64,
}

/// A power-law (Crow–AMSAA) process fitted to one phase.
#[derive(Debug, Clone, PartialEq)]
pub struct PowerLawProcess {
    /// Start of the phase (0 for a whole test).
    pub start: f64,
    /// End of the phase, `T`.
    pub end: f64,
    /// Failures in the phase.
    pub n: usize,
    /// How the phase ended.
    pub termination: Termination,
    /// Maximum-likelihood `β̂`.
    pub beta_mle: f64,
    /// The `β` used by the fitted curve (see [`Self::estimator`]).
    pub beta: f64,
    /// Which estimate `beta` is.
    pub estimator: BetaEstimator,
    /// `ln λ` (on the original time scale) for the `β` used.
    pub ln_lambda: f64,
    /// Confidence interval for `β`.
    pub beta_interval: (f64, f64),
    /// `true` when `beta_interval` is the exact χ² interval (phases from
    /// zero); otherwise a Wald interval on `ln β`.
    pub beta_interval_exact: bool,
    /// Confidence level of the intervals.
    pub confidence: f64,
    /// Inverse observed information of `(a, β)` (see the module notes).
    covariance: [f64; 3],
    /// Cramér–von Mises test (phases from zero with `M ≥ 2`).
    pub cramer_von_mises: Option<CramerVonMises>,
}

impl PowerLawProcess {
    /// `λ` for the `β` used.
    #[must_use]
    pub fn lambda(&self) -> f64 {
        self.ln_lambda.exp()
    }

    /// Expected failures in the phase by time `t`: `λ(t^β − s^β)`.
    #[must_use]
    pub fn expected_failures(&self, t: f64) -> f64 {
        let at = |x: f64| if x > 0.0 { (self.ln_lambda + self.beta * x.ln()).exp() } else { 0.0 };
        at(t) - at(self.start)
    }

    /// Failure intensity `u(t) = λβt^(β−1)`.
    #[must_use]
    pub fn intensity(&self, t: f64) -> f64 {
        (self.ln_lambda + self.beta.ln() + (self.beta - 1.0) * t.ln()).exp()
    }

    /// Instantaneous mean time between failures `1/u(t)`.
    #[must_use]
    pub fn mtbf(&self, t: f64) -> f64 {
        1.0 / self.intensity(t)
    }

    /// Standard error of `ln u(t)` (equally of `ln M(t)`).
    #[must_use]
    pub fn log_intensity_std_error(&self, t: f64) -> f64 {
        let [v11, v12, v22] = self.covariance;
        let g2 = 1.0 / self.beta + (t / self.end).ln();
        (v11 + 2.0 * g2 * v12 + g2 * g2 * v22).max(0.0).sqrt()
    }

    /// Wald confidence interval for the intensity at `t` (log scale).
    #[must_use]
    pub fn intensity_interval(&self, t: f64) -> (f64, f64) {
        let z = z_value(self.confidence);
        let (u, se) = (self.intensity(t), self.log_intensity_std_error(t));
        (u * (-z * se).exp(), u * (z * se).exp())
    }

    /// Wald confidence interval for the instantaneous MTBF at `t`.
    #[must_use]
    pub fn mtbf_interval(&self, t: f64) -> (f64, f64) {
        let (lo, hi) = self.intensity_interval(t);
        (1.0 / hi, 1.0 / lo)
    }

    /// Standard error of `β` from the observed information.
    #[must_use]
    pub fn beta_std_error(&self) -> f64 {
        self.covariance[2].max(0.0).sqrt()
    }
}

fn z_value(confidence: f64) -> f64 {
    Normal::standard().quantile(0.5 + confidence / 2.0).unwrap_or(1.959_963_984_540_054)
}

/// Fit the power-law process to the failure times of one phase.
///
/// * `failures` — cumulative times of the phase's failures, each in
///   `(start, end]`, in any order.
/// * `start` — start of the phase (0 for a whole test).
/// * `end` — `Some(T)` for a time-terminated phase, `None` for one that
///   ended at its last failure.
/// * `estimator` — which `β` the fitted curve uses (phases starting after
///   zero always use the maximum-likelihood estimate).
/// * `confidence` — confidence level of the intervals, in (0, 1).
///
/// # Errors
/// [`StatError::InvalidArgument`] for too few failures (2 time-terminated,
/// 3 failure-terminated), non-finite or out-of-range times, a confidence
/// level outside (0, 1); an error when the phase's
/// shape estimate does not exist (failures piled up at its start).
pub fn fit_power_law(
    failures: &[f64],
    start: f64,
    end: Option<f64>,
    estimator: BetaEstimator,
    confidence: f64,
) -> Result<PowerLawProcess> {
    if !(confidence > 0.0 && confidence < 1.0) {
        return Err(invalid("the confidence level must be between 0 and 1"));
    }
    if !(start.is_finite() && start >= 0.0) {
        return Err(invalid("the phase start must be a finite, non-negative time"));
    }
    let mut t: Vec<f64> = failures.to_vec();
    if t.iter().any(|x| !x.is_finite()) {
        return Err(invalid("failure times must be finite"));
    }
    t.sort_by(f64::total_cmp);
    let n = t.len();
    let termination = if end.is_some() { Termination::Time } else { Termination::Failure };
    let needed = if termination == Termination::Time { 2 } else { 3 };
    if n < needed {
        return Err(invalid(format!("a {} fit needs at least {needed} failures (got {n})", termination.label())));
    }
    let end = end.unwrap_or(t[n - 1]);
    if !end.is_finite() {
        return Err(invalid("the end of the test must be a finite time"));
    }
    if t[0] <= start || t[0] <= 0.0 {
        return Err(invalid("every failure time must be after the start of its phase (and positive)"));
    }
    if t[n - 1] > end {
        return Err(invalid("a failure time is after the end of the test"));
    }
    let nf = n as f64;
    // Times in units of the end: τᵢ = tᵢ/T, r = s/T.
    let sum_log: f64 = t.iter().map(|x| (x / end).ln()).sum();
    let r = start / end;
    let truncated = start > 0.0;
    let beta_mle = if truncated {
        let ln_r = r.ln();
        let score = |b: f64| {
            let x = b * ln_r;
            nf / b + sum_log + nf * x.exp() / -x.exp_m1() * ln_r
        };
        bisect_decreasing(score, 1e-3, 1e3).ok_or_else(|| {
            invalid("the phase's shape estimate does not exist: its failures sit too close to its start")
        })?
    } else {
        if sum_log >= 0.0 {
            return Err(invalid("all failures at the end of the test: the shape cannot be estimated"));
        }
        -nf / sum_log
    };
    let (beta, estimator) = match (estimator, truncated, termination) {
        (BetaEstimator::Unbiased, false, Termination::Time) => ((nf - 1.0) / nf * beta_mle, BetaEstimator::Unbiased),
        (BetaEstimator::Unbiased, false, Termination::Failure) => ((nf - 2.0) / nf * beta_mle, BetaEstimator::Unbiased),
        _ => (beta_mle, BetaEstimator::MaximumLikelihood),
    };
    // λ = n / (T^β − s^β) = n / (T^β (1 − r^β)).
    let one_minus = if truncated { -(beta * r.ln()).exp_m1() } else { 1.0 };
    let ln_lambda = nf.ln() - beta * end.ln() - one_minus.ln();
    // Observed information of (a, β) on the scaled clock.
    let (c, ln_r) = if truncated {
        let rb = (beta * r.ln()).exp();
        (rb / -(beta * r.ln()).exp_m1(), r.ln())
    } else {
        (0.0, 0.0)
    };
    let (j11, j12, j22) = (nf, -nf * c * ln_r, nf / (beta * beta) - nf * c * ln_r * ln_r);
    let det = j11 * j22 - j12 * j12;
    if det.is_nan() || det <= 0.0 {
        return Err(invalid("the information matrix is singular"));
    }
    let covariance = [j22 / det, -j12 / det, j11 / det];
    let (beta_interval, beta_interval_exact) = if truncated {
        let se = covariance[2].sqrt();
        let z = z_value(confidence);
        ((beta_mle * (-z * se / beta_mle).exp(), beta_mle * (z * se / beta_mle).exp()), false)
    } else {
        let df = if termination == Termination::Time { 2.0 * nf } else { 2.0 * (nf - 1.0) };
        let chi = ChiSquared::new(df)?;
        let a = (1.0 - confidence) / 2.0;
        let lo = beta_mle * chi.quantile(a)? / (2.0 * nf);
        let hi = beta_mle * chi.quantile(1.0 - a)? / (2.0 * nf);
        ((lo, hi), true)
    };
    let cramer_von_mises = if truncated {
        None
    } else {
        let m = if termination == Termination::Time { n } else { n - 1 };
        let ratios: Vec<f64> = t[..m].iter().map(|x| x / end).collect();
        (m >= 2).then(|| cramer_von_mises_test(&ratios, 1.0 - confidence))
    };
    Ok(PowerLawProcess {
        start,
        end,
        n,
        termination,
        beta_mle,
        beta,
        estimator,
        ln_lambda,
        beta_interval,
        beta_interval_exact,
        confidence,
        covariance,
        cramer_von_mises,
    })
}

/// Root of a decreasing function on `[lo, hi]` by bisection on the log
/// scale; `None` when it does not change sign there.
fn bisect_decreasing(f: impl Fn(f64) -> f64, lo: f64, hi: f64) -> Option<f64> {
    let (mut a, mut b) = (lo.ln(), hi.ln());
    if f(lo) <= 0.0 || f(hi) >= 0.0 {
        return None;
    }
    for _ in 0..200 {
        let m = 0.5 * (a + b);
        if f(m.exp()) > 0.0 {
            a = m;
        } else {
            b = m;
        }
    }
    Some((0.5 * (a + b)).exp())
}

/// `C²_M` for ordered ratios `tᵢ/T` (`M ≥ 2`), with the unbiased shape
/// `β̄ = (M − 1)/Σ ln(T/tᵢ)`.
#[must_use]
pub fn cramer_von_mises_statistic(ratios: &[f64]) -> f64 {
    let m = ratios.len();
    let mf = m as f64;
    let s: f64 = ratios.iter().map(|x| -x.ln()).sum();
    let beta = (mf - 1.0) / s;
    let mut c = 1.0 / (12.0 * mf);
    for (i, x) in ratios.iter().enumerate() {
        let d = x.powf(beta) - (2.0 * i as f64 + 1.0) / (2.0 * mf);
        c += d * d;
    }
    c
}

/// Replicates in the simulated null distribution of `C²_M`.
const CVM_REPLICATES: usize = 20_000;

/// Simulated null distribution of `C²_M`, sorted (seeded, so repeatable).
fn cramer_von_mises_null(m: usize) -> Vec<f64> {
    let mut rng = Rng::new(0xC2A3_5A11_0000_0000 ^ m as u64);
    let mut u = vec![0.0; m];
    let mut out: Vec<f64> = (0..CVM_REPLICATES)
        .map(|_| {
            for v in &mut u {
                *v = 1.0 - rng.next_f64(); // in (0, 1]
            }
            u.sort_by(f64::total_cmp);
            cramer_von_mises_statistic(&u)
        })
        .collect();
    out.sort_by(f64::total_cmp);
    out
}

/// The Cramér–von Mises test for ordered ratios `tᵢ/T` (`M ≥ 2`), with the
/// critical value at significance level `alpha` and the p-value simulated
/// from the statistic's null distribution for this `M`.
#[must_use]
pub fn cramer_von_mises_test(ratios: &[f64], alpha: f64) -> CramerVonMises {
    let statistic = cramer_von_mises_statistic(ratios);
    let null = cramer_von_mises_null(ratios.len());
    let above = null.iter().filter(|&&c| c >= statistic).count();
    let k = ((1.0 - alpha) * null.len() as f64).ceil() as usize;
    CramerVonMises {
        statistic,
        m: ratios.len(),
        critical_value: null[k.clamp(1, null.len()) - 1],
        alpha,
        p_value: (above as f64 + 1.0) / (null.len() as f64 + 1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference values below were computed independently from the closed
    /// forms in Crow (1974) with Python's standard library
    /// (`python3 -I`, `math` only; chi-square quantiles by bisection on the
    /// regularized gamma series).
    const TIMES: [f64; 10] = [12.0, 35.0, 61.0, 102.0, 160.0, 230.0, 310.0, 420.0, 555.0, 700.0];

    #[test]
    fn time_terminated_closed_forms() {
        let f = fit_power_law(&TIMES, 0.0, Some(800.0), BetaEstimator::MaximumLikelihood, 0.95).unwrap();
        let s: f64 = TIMES.iter().map(|t| (800.0f64 / t).ln()).sum();
        assert!((f.beta_mle - 10.0 / s).abs() < 1e-12);
        assert!((f.lambda() - 10.0 / 800.0f64.powf(f.beta_mle)).abs() < 1e-12 * f.lambda());
        assert_eq!(f.termination, Termination::Time);
        // Expected failures by T equal n; MTBF(T) = T/(nβ).
        assert!((f.expected_failures(800.0) - 10.0).abs() < 1e-9);
        assert!((f.mtbf(800.0) - 800.0 / (10.0 * f.beta_mle)).abs() < 1e-9);
        // Var ln M̂(T) = 2/n for a phase from zero.
        assert!((f.log_intensity_std_error(800.0) - (0.2f64).sqrt()).abs() < 1e-12);
        // Python: β̂ = 0.5913700, χ²(20) quantiles 9.590777, 34.169607.
        assert!((f.beta_mle - 0.591_370_0).abs() < 1e-6, "{}", f.beta_mle);
        assert!((f.beta_interval.0 - f.beta_mle * 9.590_777 / 20.0).abs() < 1e-5);
        assert!((f.beta_interval.1 - f.beta_mle * 34.169_607 / 20.0).abs() < 1e-5);
        let u = fit_power_law(&TIMES, 0.0, Some(800.0), BetaEstimator::Unbiased, 0.95).unwrap();
        assert!((u.beta - 0.9 * u.beta_mle).abs() < 1e-12);
        assert!((u.expected_failures(800.0) - 10.0).abs() < 1e-9);
    }

    #[test]
    fn failure_terminated_uses_last_failure() {
        let f = fit_power_law(&TIMES, 0.0, None, BetaEstimator::Unbiased, 0.9).unwrap();
        let s: f64 = TIMES[..9].iter().map(|t| (700.0f64 / t).ln()).sum();
        assert!((f.beta_mle - 10.0 / s).abs() < 1e-12);
        assert!((f.beta - 0.8 * f.beta_mle).abs() < 1e-12);
        assert_eq!(f.end, 700.0);
        // χ²(18) interval.
        let chi = ChiSquared::new(18.0).unwrap();
        assert!((f.beta_interval.1 - f.beta_mle * chi.quantile(0.95).unwrap() / 20.0).abs() < 1e-12);
        let cvm = f.cramer_von_mises.unwrap();
        assert_eq!(cvm.m, 9);
        let ratios: Vec<f64> = TIMES[..9].iter().map(|t| t / 700.0).collect();
        assert!((cvm.statistic - cramer_von_mises_statistic(&ratios)).abs() < 1e-15);
        assert!(fit_power_law(&TIMES[..2], 0.0, None, BetaEstimator::Unbiased, 0.9).is_err());
    }

    #[test]
    fn cramer_von_mises_matches_hand_computation() {
        // M = 3, ratios 0.2, 0.5, 0.9: Σ ln(1/x) = 2.3025851 → β̄ = 2/that.
        let r = [0.2, 0.5, 0.9];
        let beta: f64 = 2.0 / (5.0f64.ln() + 2.0f64.ln() + (1.0f64 / 0.9).ln());
        let expected = 1.0 / 36.0
            + (0.2f64.powf(beta) - 1.0 / 6.0).powi(2)
            + (0.5f64.powf(beta) - 0.5).powi(2)
            + (0.9f64.powf(beta) - 5.0 / 6.0).powi(2);
        assert!((cramer_von_mises_statistic(&r) - expected).abs() < 1e-15);
        // Simulated critical values. Independent check: a Python simulation
        // (`random`, 200 000 replicates, a different generator) gives the
        // 95th percentile 0.2128 and the 90th 0.1681 for M = 10.
        let t = cramer_von_mises_test(&[0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 0.95], 0.05);
        assert!((t.critical_value - 0.2128).abs() < 0.006, "{}", t.critical_value);
        let t10 = cramer_von_mises_test(&[0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 0.95], 0.10);
        assert!((t10.critical_value - 0.1681).abs() < 0.004, "{}", t10.critical_value);
        assert!(t.p_value > 0.0 && t.p_value <= 1.0);
    }

    #[test]
    fn left_truncated_phase_solves_score_equation() {
        let failures = [480.0, 600.0, 750.0, 900.0, 1050.0, 1180.0];
        let f = fit_power_law(&failures, 400.0, Some(1200.0), BetaEstimator::Unbiased, 0.95).unwrap();
        assert_eq!(f.estimator, BetaEstimator::MaximumLikelihood);
        assert!(!f.beta_interval_exact);
        // Score at the estimate is zero.
        let (n, b) = (6.0, f.beta_mle);
        let r: f64 = 400.0 / 1200.0;
        let score = n / b
            + failures.iter().map(|t| (t / 1200.0f64).ln()).sum::<f64>()
            + n * r.powf(b) / (1.0 - r.powf(b)) * r.ln();
        assert!(score.abs() < 1e-9, "{score}");
        // Expected failures over the phase equal n.
        assert!((f.expected_failures(1200.0) - 6.0).abs() < 1e-9);
        assert!(f.cramer_von_mises.is_none());
        // Python (bisection on the same score): β̂ = 1.3353961.
        assert!((f.beta_mle - 1.335_396_1).abs() < 1e-6);
        assert!(f.beta_interval.0 < f.beta_mle && f.beta_mle < f.beta_interval.1);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(fit_power_law(&[1.0, 2.0], 0.0, Some(1.5), BetaEstimator::Unbiased, 0.95).is_err());
        assert!(fit_power_law(&[1.0, 2.0], 1.0, Some(3.0), BetaEstimator::Unbiased, 0.95).is_err());
        assert!(fit_power_law(&[1.0, 2.0], 0.0, Some(3.0), BetaEstimator::Unbiased, 1.5).is_err());
        // An infinite end used to give β̂ = 0 with a (0, 0) interval.
        assert!(fit_power_law(&[1.0, 2.0, 3.0], 0.0, Some(f64::INFINITY), BetaEstimator::Unbiased, 0.95).is_err());
        assert!(fit_power_law(&[1.0, 2.0, 3.0], 0.0, Some(f64::NAN), BetaEstimator::Unbiased, 0.95).is_err());
    }
}
