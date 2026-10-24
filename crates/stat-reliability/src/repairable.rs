//! Repairable systems: recurrent failures (repairs) of several systems, each
//! observed from age 0 to its own end of observation.
//!
//! # Mean cumulative function
//! Nelson's nonparametric estimate of the mean number of repairs per system
//! by age `t` (Nelson 2003, *Recurrent Events Data Analysis for Product
//! Repairs, Disease Recurrences, and Other Applications*, SIAM, ch. 2–3):
//! at each distinct repair age `u_j`, with `d_j` repairs among the `r_j`
//! systems still observed (end of observation `≥ u_j`),
//!
//! ```text
//! M̂(t) = Σ_{u_j ≤ t} d_j / r_j.
//! ```
//!
//! Its variance is estimated either robustly, without assuming a Poisson
//! process (Lawless & Nadeau 1995, *Technometrics* 37, 158–168; Nelson 2003,
//! §3.3):
//!
//! ```text
//! V̂(t) = Σ_i [ Σ_{u_j ≤ t, i at risk} (d_ij − d_j/r_j) / r_j ]²,
//! ```
//!
//! or under a Poisson process, `V̂(t) = Σ_{u_j ≤ t} d_j / r_j²`. Pointwise
//! limits are `M̂ ± z·SE` (normal) or `M̂·exp(±z·SE/M̂)` (log, never
//! negative). Two populations are compared through the difference
//! `M̂₁(t) − M̂₂(t)` with variance `V̂₁ + V̂₂` (Nelson 2003, §3.5): where its
//! limits exclude zero, the populations differ at that age.
//!
//! # Parametric fits
//! A homogeneous Poisson process (constant rate `ρ = n/ΣT_j`, with the exact
//! interval `[χ²_{α/2}(2n), χ²_{1−α/2}(2n + 2)]/(2ΣT_j)`) and the power-law
//! process `u(t) = λβt^(β−1)` shared by every system, fitted by maximum
//! likelihood (Crow 1974): `λ = n/ΣT_j^β` and `β` solves
//! `n/β + Σ ln t_ij − n ΣT_j^β ln T_j / ΣT_j^β = 0`. The likelihood-ratio
//! test of `β = 1` compares them.
//!
//! # Trend tests
//! For each system observed to `T_j` with `m_j` repairs (a system whose
//! observation ended at a repair drops that repair and uses its age as
//! `T_j`):
//!
//! * **Laplace**: `U = [Σ t_ij − Σ m_j T_j/2] / √(Σ m_j T_j²/12)`, standard
//!   normal under a constant rate; `U > 0` suggests deterioration.
//! * **Military handbook** (MIL-HDBK-189): `χ² = 2 Σ ln(T_j/t_ij)` with
//!   `2Σ m_j` degrees of freedom; small values suggest deterioration, large
//!   values improvement.
//!
//! Both p-values are two-sided (Ascher & Feingold 1984, *Repairable Systems
//! Reliability*; Meeker & Escobar 1998, §16.5).

use stat_core::{ChiSquared, ContinuousDistribution, Normal, Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn z_value(confidence: f64) -> f64 {
    Normal::standard().quantile(0.5 + confidence / 2.0).unwrap_or(1.959_963_984_540_054)
}

fn check_confidence(confidence: f64) -> Result<()> {
    if confidence > 0.0 && confidence < 1.0 {
        Ok(())
    } else {
        Err(invalid("the confidence level must be between 0 and 1"))
    }
}

/// One system's repair history.
#[derive(Debug, Clone, PartialEq)]
pub struct SystemHistory {
    /// Ages at repairs (any order; ties allowed).
    pub repairs: Vec<f64>,
    /// Age at the end of observation (`≥` every repair age).
    pub end: f64,
    /// `true` when observation ended at the last repair (no separate end
    /// record): the trend tests then condition on that repair.
    pub ended_at_repair: bool,
}

impl SystemHistory {
    /// The repairs and end used by the trend tests: all repairs to `end`,
    /// or (ended at a repair) the earlier repairs to the last one.
    fn trend_data(&self) -> (Vec<f64>, f64) {
        let mut r = self.repairs.clone();
        r.sort_by(f64::total_cmp);
        if self.ended_at_repair && !r.is_empty() {
            let last = r.pop().unwrap_or(self.end);
            (r, last)
        } else {
            (r, self.end)
        }
    }
}

fn check_systems(systems: &[SystemHistory]) -> Result<()> {
    if systems.is_empty() {
        return Err(invalid("no systems"));
    }
    for s in systems {
        if !(s.end.is_finite() && s.end > 0.0) {
            return Err(invalid("every system needs a positive, finite end of observation"));
        }
        if s.repairs.iter().any(|&t| !(t.is_finite() && t >= 0.0 && t <= s.end)) {
            return Err(invalid("repair ages must be finite, non-negative and not after the end of observation"));
        }
    }
    Ok(())
}

/// How the variance of the mean cumulative function is estimated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum McfVariance {
    /// Lawless–Nadeau robust estimate (no Poisson assumption).
    #[default]
    Robust,
    /// Poisson-process estimate `Σ d/r²`.
    Poisson,
}

impl McfVariance {
    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Robust => "robust (Lawless–Nadeau)",
            Self::Poisson => "Poisson",
        }
    }
}

/// How pointwise limits are formed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum McfLimits {
    /// `M̂ ± z·SE`.
    #[default]
    Normal,
    /// `M̂·exp(±z·SE/M̂)`.
    Log,
}

impl McfLimits {
    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Log => "log",
        }
    }
}

/// The estimate at one distinct repair age.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct McfStep {
    /// Repair age `u_j`.
    pub time: f64,
    /// Repairs at that age, `d_j`.
    pub repairs: usize,
    /// Systems still observed, `r_j`.
    pub at_risk: usize,
    /// `M̂(u_j)`.
    pub mcf: f64,
    /// Standard error of `M̂(u_j)`.
    pub std_error: f64,
    /// Lower confidence limit.
    pub lower: f64,
    /// Upper confidence limit.
    pub upper: f64,
}

/// Nelson's mean cumulative function.
#[derive(Debug, Clone, PartialEq)]
pub struct MeanCumulativeFunction {
    /// One step per distinct repair age, in order.
    pub steps: Vec<McfStep>,
    /// Latest end of observation (the estimate's range).
    pub end: f64,
    /// Number of systems.
    pub systems: usize,
    /// Confidence level of the limits.
    pub confidence: f64,
}

impl MeanCumulativeFunction {
    /// `(M̂(t), SE)` at age `t` (0 before the first repair).
    #[must_use]
    pub fn at(&self, t: f64) -> (f64, f64) {
        match self.steps.iter().rposition(|s| s.time <= t) {
            Some(i) => (self.steps[i].mcf, self.steps[i].std_error),
            None => (0.0, 0.0),
        }
    }

    /// Vertices of the step function from age 0 to the end of observation.
    #[must_use]
    pub fn step_polyline(&self) -> Vec<[f64; 2]> {
        let mut out = vec![[0.0, 0.0]];
        let mut level = 0.0;
        for s in &self.steps {
            out.push([s.time, level]);
            out.push([s.time, s.mcf]);
            level = s.mcf;
        }
        out.push([self.end, level]);
        out
    }

    /// The confidence band as constant pieces `[from, to, lower, upper]`.
    #[must_use]
    pub fn band_pieces(&self) -> Vec<[f64; 4]> {
        self.steps
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let to = self.steps.get(i + 1).map_or(self.end, |n| n.time);
                [s.time, to, s.lower, s.upper]
            })
            .collect()
    }
}

fn limits(m: f64, se: f64, z: f64, kind: McfLimits) -> (f64, f64) {
    match kind {
        McfLimits::Normal => (m - z * se, m + z * se),
        McfLimits::Log if m > 0.0 => {
            let w = (z * se / m).exp();
            (m / w, m * w)
        }
        McfLimits::Log => (0.0, 0.0),
    }
}

/// Estimate the mean cumulative function of `systems`.
///
/// # Errors
/// [`StatError::InvalidArgument`] for no systems, no repairs, a system
/// without a positive end of observation, a repair after its system's end,
/// or a confidence level outside (0, 1).
pub fn mean_cumulative_function(
    systems: &[SystemHistory],
    variance: McfVariance,
    limit_kind: McfLimits,
    confidence: f64,
) -> Result<MeanCumulativeFunction> {
    check_systems(systems)?;
    check_confidence(confidence)?;
    let mut times: Vec<f64> = systems.iter().flat_map(|s| s.repairs.iter().copied()).collect();
    if times.is_empty() {
        return Err(invalid("there are no repairs"));
    }
    times.sort_by(f64::total_cmp);
    times.dedup();
    let z = z_value(confidence);
    // Running per-system sums for the robust variance.
    let mut running = vec![0.0; systems.len()];
    let (mut mcf, mut poisson_var) = (0.0, 0.0);
    let mut steps = Vec::with_capacity(times.len());
    for &u in &times {
        let counts: Vec<usize> = systems.iter().map(|s| s.repairs.iter().filter(|&&t| t == u).count()).collect();
        let at_risk: Vec<bool> = systems.iter().map(|s| s.end >= u).collect();
        let r = at_risk.iter().filter(|&&a| a).count();
        let d: usize = counts.iter().sum();
        let (rf, df) = (r as f64, d as f64);
        mcf += df / rf;
        poisson_var += df / (rf * rf);
        for (i, sum) in running.iter_mut().enumerate() {
            if at_risk[i] {
                *sum += (counts[i] as f64 - df / rf) / rf;
            }
        }
        let var = match variance {
            McfVariance::Robust => running.iter().map(|s| s * s).sum(),
            McfVariance::Poisson => poisson_var,
        };
        let se = f64::sqrt(var);
        let (lower, upper) = limits(mcf, se, z, limit_kind);
        steps.push(McfStep { time: u, repairs: d, at_risk: r, mcf, std_error: se, lower, upper });
    }
    let end = systems.iter().map(|s| s.end).fold(0.0, f64::max);
    Ok(MeanCumulativeFunction { steps, end, systems: systems.len(), confidence })
}

/// The difference of two mean cumulative functions at one age.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct McfDifferenceStep {
    /// Age (a repair age of either population).
    pub time: f64,
    /// `M̂₁(t) − M̂₂(t)`.
    pub difference: f64,
    /// `√(V̂₁ + V̂₂)`.
    pub std_error: f64,
    /// Lower limit.
    pub lower: f64,
    /// Upper limit.
    pub upper: f64,
}

/// `M̂₁ − M̂₂` at every repair age of either population up to the earlier of
/// the two ends of observation, with normal pointwise limits at
/// `confidence`.
///
/// # Errors
/// [`StatError::InvalidArgument`] for a confidence level outside (0, 1).
pub fn mcf_difference(
    first: &MeanCumulativeFunction,
    second: &MeanCumulativeFunction,
    confidence: f64,
) -> Result<Vec<McfDifferenceStep>> {
    check_confidence(confidence)?;
    let z = z_value(confidence);
    let end = first.end.min(second.end);
    let mut times: Vec<f64> = first.steps.iter().chain(&second.steps).map(|s| s.time).filter(|&t| t <= end).collect();
    times.sort_by(f64::total_cmp);
    times.dedup();
    Ok(times
        .into_iter()
        .map(|t| {
            let ((m1, s1), (m2, s2)) = (first.at(t), second.at(t));
            let se = (s1 * s1 + s2 * s2).sqrt();
            let difference = m1 - m2;
            McfDifferenceStep {
                time: t,
                difference,
                std_error: se,
                lower: difference - z * se,
                upper: difference + z * se,
            }
        })
        .collect())
}

/// A homogeneous Poisson process fit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HppFit {
    /// Total repairs `n`.
    pub repairs: usize,
    /// Total observed age `ΣT_j`.
    pub exposure: f64,
    /// Rate `n/ΣT_j`.
    pub rate: f64,
    /// Exact confidence interval for the rate.
    pub interval: (f64, f64),
    /// Maximized log-likelihood.
    pub log_likelihood: f64,
}

/// Fit a constant repair rate.
///
/// # Errors
/// As [`mean_cumulative_function`].
pub fn hpp_fit(systems: &[SystemHistory], confidence: f64) -> Result<HppFit> {
    check_systems(systems)?;
    check_confidence(confidence)?;
    let n: usize = systems.iter().map(|s| s.repairs.len()).sum();
    if n == 0 {
        return Err(invalid("there are no repairs"));
    }
    let exposure: f64 = systems.iter().map(|s| s.end).sum();
    let nf = n as f64;
    let rate = nf / exposure;
    let a = (1.0 - confidence) / 2.0;
    let lower = ChiSquared::new(2.0 * nf)?.quantile(a)? / (2.0 * exposure);
    let upper = ChiSquared::new(2.0 * nf + 2.0)?.quantile(1.0 - a)? / (2.0 * exposure);
    Ok(HppFit { repairs: n, exposure, rate, interval: (lower, upper), log_likelihood: nf * rate.ln() - nf })
}

/// The power-law process fitted to several systems.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PowerLawFit {
    /// Total repairs `n`.
    pub repairs: usize,
    /// Shape `β̂`.
    pub beta: f64,
    /// Standard error of `β̂` (observed information).
    pub beta_std_error: f64,
    /// Wald interval for `β` on the log scale.
    pub beta_interval: (f64, f64),
    /// `ln λ̂`.
    pub ln_lambda: f64,
    /// Maximized log-likelihood.
    pub log_likelihood: f64,
}

impl PowerLawFit {
    /// `λ̂`.
    #[must_use]
    pub fn lambda(&self) -> f64 {
        self.ln_lambda.exp()
    }

    /// Expected repairs per system by age `t`, `λt^β`.
    #[must_use]
    pub fn mean_cumulative(&self, t: f64) -> f64 {
        if t > 0.0 { (self.ln_lambda + self.beta * t.ln()).exp() } else { 0.0 }
    }

    /// Repair intensity `λβt^(β−1)`.
    #[must_use]
    pub fn intensity(&self, t: f64) -> f64 {
        (self.ln_lambda + self.beta.ln() + (self.beta - 1.0) * t.ln()).exp()
    }
}

/// Fit the power-law process by maximum likelihood.
///
/// # Errors
/// As [`mean_cumulative_function`]; also fewer than two repairs, a repair at
/// age 0, or no finite estimate.
pub fn power_law_fit(systems: &[SystemHistory], confidence: f64) -> Result<PowerLawFit> {
    check_systems(systems)?;
    check_confidence(confidence)?;
    let n: usize = systems.iter().map(|s| s.repairs.len()).sum();
    if n < 2 {
        return Err(invalid("the power-law fit needs at least two repairs"));
    }
    if systems.iter().flat_map(|s| &s.repairs).any(|&t| t <= 0.0) {
        return Err(invalid("the power-law fit needs repair ages above zero"));
    }
    let nf = n as f64;
    // Ages in units of the longest observation, τ = t/T_max.
    let scale = systems.iter().map(|s| s.end).fold(0.0, f64::max);
    let sum_log: f64 = systems.iter().flat_map(|s| &s.repairs).map(|t| (t / scale).ln()).sum();
    let ln_tau: Vec<f64> = systems.iter().map(|s| (s.end / scale).ln()).collect();
    // D(β) = Σ τ_j^β and its derivatives.
    let d = |b: f64| -> (f64, f64, f64) {
        let mut out = (0.0, 0.0, 0.0);
        for &l in &ln_tau {
            let w = (b * l).exp();
            out.0 += w;
            out.1 += w * l;
            out.2 += w * l * l;
        }
        out
    };
    let score = |b: f64| {
        let (d0, d1, _) = d(b);
        nf / b + sum_log - nf * d1 / d0
    };
    let (mut lo, mut hi) = (1e-8f64.ln(), 1e3f64.ln());
    if score(lo.exp()) <= 0.0 || score(hi.exp()) >= 0.0 {
        return Err(invalid("the power-law shape estimate does not exist"));
    }
    for _ in 0..200 {
        let m = 0.5 * (lo + hi);
        if score(m.exp()) > 0.0 {
            lo = m;
        } else {
            hi = m;
        }
    }
    let beta = (0.5 * (lo + hi)).exp();
    let (d0, d1, d2) = d(beta);
    // ln λ on the original scale: λ = n / (scale^β D).
    let ln_lambda = nf.ln() - beta * scale.ln() - d0.ln();
    // Observed information of (a, β), a = ln λ + β ln scale.
    let (a1, a2) = (d1 / d0, d2 / d0);
    let (j11, j12, j22) = (nf, nf * a1, nf / (beta * beta) + nf * a2);
    let det = j11 * j22 - j12 * j12;
    let var_beta = if det > 0.0 { j11 / det } else { f64::NAN };
    let se = var_beta.sqrt();
    let z = z_value(confidence);
    let beta_interval = (beta * (-z * se / beta).exp(), beta * (z * se / beta).exp());
    let sum_log_t: f64 = systems.iter().flat_map(|s| &s.repairs).map(|t| t.ln()).sum();
    let log_likelihood = nf * ln_lambda + nf * beta.ln() + (beta - 1.0) * sum_log_t - nf;
    Ok(PowerLawFit { repairs: n, beta, beta_std_error: se, beta_interval, ln_lambda, log_likelihood })
}

/// A trend test's result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrendTest {
    /// Test statistic.
    pub statistic: f64,
    /// Degrees of freedom (χ² tests).
    pub df: Option<f64>,
    /// Two-sided p-value.
    pub p_value: f64,
    /// `true` when the statistic points to a rising repair rate
    /// (deterioration), `false` to a falling one (improvement).
    pub increasing: bool,
}

/// The Laplace test of a constant repair rate, pooled over systems.
///
/// # Errors
/// As [`mean_cumulative_function`]; also no repairs before the systems' ends.
pub fn laplace_test(systems: &[SystemHistory]) -> Result<TrendTest> {
    check_systems(systems)?;
    let (mut num, mut var) = (0.0, 0.0);
    for s in systems {
        let (r, end) = s.trend_data();
        let m = r.len() as f64;
        num += r.iter().sum::<f64>() - m * end / 2.0;
        var += m * end * end / 12.0;
    }
    if var.is_nan() || var <= 0.0 {
        return Err(invalid("the Laplace test needs repairs before the end of observation"));
    }
    let u = num / var.sqrt();
    let p_value = (2.0 * Normal::standard().sf(u.abs())?).min(1.0);
    Ok(TrendTest { statistic: u, df: None, p_value, increasing: u > 0.0 })
}

/// The military handbook (MIL-HDBK-189) test of a constant repair rate.
///
/// # Errors
/// As [`laplace_test`]; also a repair at age 0.
pub fn military_handbook_test(systems: &[SystemHistory]) -> Result<TrendTest> {
    check_systems(systems)?;
    let (mut chi, mut m) = (0.0, 0usize);
    for s in systems {
        let (r, end) = s.trend_data();
        if r.iter().any(|&t| t <= 0.0) {
            return Err(invalid("the military handbook test needs repair ages above zero"));
        }
        chi += 2.0 * r.iter().map(|t| (end / t).ln()).sum::<f64>();
        m += r.len();
    }
    if m == 0 {
        return Err(invalid("the military handbook test needs repairs before the end of observation"));
    }
    let df = 2.0 * m as f64;
    let dist = ChiSquared::new(df)?;
    let p_value = (2.0 * dist.cdf(chi)?.min(dist.sf(chi)?)).min(1.0);
    Ok(TrendTest { statistic: chi, df: Some(df), p_value, increasing: chi < df })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sys(repairs: &[f64], end: f64) -> SystemHistory {
        SystemHistory { repairs: repairs.to_vec(), end, ended_at_repair: false }
    }

    /// Three systems; hand computation of Nelson's estimate:
    /// ages 2 (A), 3 (B), 5 (A, C), 7 (B), 9 (C). Ends 8, 10, 12.
    /// M̂: 1/3, 2/3, 4/3, 5/3 (r = 3 throughout to 8), then at 9 r = 2 → 13/6.
    fn three() -> Vec<SystemHistory> {
        vec![sys(&[2.0, 5.0], 8.0), sys(&[3.0, 7.0], 10.0), sys(&[5.0, 9.0], 12.0)]
    }

    #[test]
    fn mcf_by_hand() {
        let m = mean_cumulative_function(&three(), McfVariance::Poisson, McfLimits::Normal, 0.95).unwrap();
        let v: Vec<f64> = m.steps.iter().map(|s| s.mcf).collect();
        let expected = [1.0 / 3.0, 2.0 / 3.0, 4.0 / 3.0, 5.0 / 3.0, 13.0 / 6.0];
        for (a, b) in v.iter().zip(expected) {
            assert!((a - b).abs() < 1e-14);
        }
        // Poisson variance Σ d/r²: 1/9 + 1/9 + 2/9 + 1/9 + 1/4.
        let var = m.steps[4].std_error.powi(2);
        assert!((var - (5.0 / 9.0 + 0.25)).abs() < 1e-14);
        assert_eq!(m.steps[4].at_risk, 2);
        assert_eq!(m.at(6.0).0, 4.0 / 3.0);
        assert_eq!(m.at(1.0).0, 0.0);
    }

    #[test]
    fn robust_variance_by_hand() {
        let m = mean_cumulative_function(&three(), McfVariance::Robust, McfLimits::Normal, 0.95).unwrap();
        // Per-system sums S_i = Σ (d_ij − d_j/r_j)/r_j over ages to 7
        // (all three at risk, r = 3):
        // age 2: A +2/9, B −1/9, C −1/9; age 3: A −1/9, B +2/9, C −1/9;
        // age 5 (d = 2): A +1/9, B −2/9, C +1/9; age 7: A −1/9, B +2/9, C −1/9.
        // S = (1/9, 1/9, −2/9) → V = 6/81.
        assert!((m.steps[3].std_error.powi(2) - 6.0 / 81.0).abs() < 1e-14);
        // Age 9: A has ended (end 8); r = 2, d = 1 (C):
        // B: (0 − 1/2)/2 = −1/4, C: (1 − 1/2)/2 = +1/4.
        // S = (1/9, 1/9 − 1/4, −2/9 + 1/4).
        let s = [1.0 / 9.0, 1.0 / 9.0 - 0.25, -2.0 / 9.0 + 0.25];
        let v: f64 = s.iter().map(|x| x * x).sum();
        assert!((m.steps[4].std_error.powi(2) - v).abs() < 1e-14);
        let z = 1.959_963_984_540_054;
        assert!((m.steps[4].lower - (13.0 / 6.0 - z * v.sqrt())).abs() < 1e-9);
        let log = mean_cumulative_function(&three(), McfVariance::Robust, McfLimits::Log, 0.95).unwrap();
        assert!(log.steps.iter().all(|s| s.lower > 0.0 && s.lower < s.mcf && s.upper > s.mcf));
    }

    #[test]
    fn difference_of_identical_populations_is_zero() {
        let a = mean_cumulative_function(&three(), McfVariance::Robust, McfLimits::Normal, 0.95).unwrap();
        let d = mcf_difference(&a, &a, 0.95).unwrap();
        assert_eq!(d.len(), 5);
        assert!(d.iter().all(|s| s.difference == 0.0 && s.lower <= 0.0 && s.upper >= 0.0));
        let b = mean_cumulative_function(&[sys(&[1.0], 6.0)], McfVariance::Robust, McfLimits::Normal, 0.95).unwrap();
        let d = mcf_difference(&a, &b, 0.95).unwrap();
        // Only ages up to the shorter observation (6): 1, 2, 3, 5.
        assert_eq!(d.iter().map(|s| s.time).collect::<Vec<_>>(), [1.0, 2.0, 3.0, 5.0]);
        assert!((d[3].difference - (4.0 / 3.0 - 1.0)).abs() < 1e-14);
    }

    #[test]
    fn difference_rejects_confidence_outside_unit_interval() {
        let a = mean_cumulative_function(&three(), McfVariance::Robust, McfLimits::Normal, 0.95).unwrap();
        for level in [0.0, 1.0, -0.5, 1.5, f64::NAN] {
            assert!(
                matches!(mcf_difference(&a, &a, level), Err(StatError::InvalidArgument { .. })),
                "level {level} accepted"
            );
        }
    }

    #[test]
    fn hpp_and_power_law() {
        let systems = three();
        let h = hpp_fit(&systems, 0.95).unwrap();
        assert!((h.rate - 6.0 / 30.0).abs() < 1e-15);
        // χ²(12) 2.5% = 4.403789, χ²(14) 97.5% = 26.118948 (tables).
        assert!((h.interval.0 - 4.403_789 / 60.0).abs() < 1e-6);
        assert!((h.interval.1 - 26.118_948 / 60.0).abs() < 1e-6);
        // Equal ends reduce to Crow's closed form β̂ = n/Σ ln(T/t).
        let eq = vec![sys(&[2.0, 5.0], 10.0), sys(&[3.0, 7.0, 9.0], 10.0)];
        let p = power_law_fit(&eq, 0.95).unwrap();
        let s: f64 = [2.0f64, 5.0, 3.0, 7.0, 9.0].iter().map(|t| (10.0 / t).ln()).sum();
        assert!((p.beta - 5.0 / s).abs() < 1e-10);
        assert!((p.lambda() - 5.0 / (2.0 * 10.0f64.powf(p.beta))).abs() < 1e-10);
        // Unequal ends: the score is zero at the estimate.
        let q = power_law_fit(&systems, 0.95).unwrap();
        let ends = [8.0f64, 10.0, 12.0];
        let (d0, d1): (f64, f64) =
            ends.iter().fold((0.0, 0.0), |(a, b), t| (a + t.powf(q.beta), b + t.powf(q.beta) * t.ln()));
        let sl: f64 = systems.iter().flat_map(|s| &s.repairs).map(|t| t.ln()).sum();
        assert!((6.0 / q.beta + sl - 6.0 * d1 / d0).abs() < 1e-9);
        assert!((q.lambda() - 6.0 / d0).abs() < 1e-9 * q.lambda());
        assert!(q.log_likelihood >= h.log_likelihood - 1e-12);
    }

    #[test]
    fn trend_tests_by_hand() {
        let systems = three();
        let l = laplace_test(&systems).unwrap();
        // Σt − Σ m T/2 = (7 − 8) + (10 − 10) + (14 − 12) = 1; var = (2·64 + 2·100 + 2·144)/12.
        let u = 1.0 / (616.0f64 / 12.0).sqrt();
        assert!((l.statistic - u).abs() < 1e-14);
        assert!(l.increasing);
        let m = military_handbook_test(&systems).unwrap();
        let chi = 2.0
            * ((8.0f64 / 2.0).ln()
                + (8.0f64 / 5.0).ln()
                + (10.0f64 / 3.0).ln()
                + (10.0f64 / 7.0).ln()
                + (12.0f64 / 5.0).ln()
                + (12.0f64 / 9.0).ln());
        assert!((m.statistic - chi).abs() < 1e-12);
        assert_eq!(m.df, Some(12.0));
        // Ended at a repair: that repair is dropped and its age is the end.
        let f = vec![SystemHistory { repairs: vec![4.0, 1.0, 9.0], end: 9.0, ended_at_repair: true }];
        let m = military_handbook_test(&f).unwrap();
        assert!((m.statistic - 2.0 * (9.0f64.ln() + (9.0f64 / 4.0).ln())).abs() < 1e-12);
        assert_eq!(m.df, Some(4.0));
        let l = laplace_test(&f).unwrap();
        assert!((l.statistic - (5.0 - 9.0) / (2.0 * 81.0f64 / 12.0).sqrt()).abs() < 1e-12);
    }

    #[test]
    fn rejects_bad_systems() {
        assert!(mean_cumulative_function(&[], McfVariance::Robust, McfLimits::Normal, 0.95).is_err());
        assert!(mean_cumulative_function(&[sys(&[5.0], 4.0)], McfVariance::Robust, McfLimits::Normal, 0.95).is_err());
        assert!(mean_cumulative_function(&[sys(&[], 4.0)], McfVariance::Robust, McfLimits::Normal, 0.95).is_err());
    }
}
