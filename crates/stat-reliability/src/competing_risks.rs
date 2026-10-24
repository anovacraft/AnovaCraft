//! Competing risks: units that can fail from one of several causes (failure
//! modes), the first of which ends their life, with right censoring.
//!
//! * [`aalen_johansen`] — the cumulative incidence function of each cause,
//!   `F_j(t) = P(T ≤ t, cause = j)`, by the Aalen–Johansen estimator with the
//!   delta-method (Greenwood-type) variance and log(−log) pointwise limits.
//! * [`gray_test`] — Gray's (1988) K-sample test that one cause's cumulative
//!   incidence is the same in every group.
//! * [`fine_gray`] — Fine & Gray's (1999) proportional subdistribution hazards
//!   regression with inverse-probability-of-censoring weights, robust
//!   (sandwich) covariance and predicted cumulative incidence.
//! * [`fit_failure_modes`] and [`SeriesSystem`] — for engineering data, a
//!   parametric life distribution per failure mode (the other modes treated as
//!   censoring) combined into the system's failure distribution
//!   `F_sys(t) = 1 − Π_j (1 − F_j(t))` with delta-method limits (Meeker &
//!   Escobar, *Statistical Methods for Reliability Data*, 1998, ch. 15).
//!
//! # Data
//! Each [`CompetingObservation`] is a time, the index of the cause that ended
//! the unit's life (`None` when the unit was still running at that time) and
//! a frequency weight. At a tied time, failures are taken to happen before
//! censoring.
//!
//! # Aalen–Johansen estimator
//! With distinct failure times `t_i`, `n_i` units at risk just before `t_i`,
//! `d_ji` failures from cause `j` and `d_i = Σ_j d_ji` from any cause, and
//! `Ŝ` the Kaplan–Meier estimate of surviving every cause,
//!
//! ```text
//! F̂_j(t) = Σ_{t_i ≤ t} Ŝ(t_{i−1}) d_ji / n_i
//! ```
//!
//! (Kalbfleisch & Prentice, *The Statistical Analysis of Failure Time Data*,
//! 2nd ed., 2002, §8.2; Klein & Moeschberger, *Survival Analysis*, 2nd ed.,
//! 2003, §2.7). The causes' incidences add up to `1 − Ŝ(t)`. The variance is
//! the delta-method (Greenwood-type) estimate (Marubini & Valsecchi 1995,
//! *Analysing Survival Data from Clinical Trials and Observational Studies*,
//! §10.2; Collett, *Modelling Survival Data in Medical Research*, 3rd ed.,
//! 2015, §12.2):
//!
//! ```text
//! Var F̂_j(t) = Σ_{t_i ≤ t} { [F̂_j(t) − F̂_j(t_i)]² d_i / (n_i (n_i − d_i))
//!                          + Ŝ(t_{i−1})² d_ji (n_i − d_ji) / n_i³
//!                          − 2 [F̂_j(t) − F̂_j(t_i)] Ŝ(t_{i−1}) d_ji / n_i² }
//! ```
//!
//! and the pointwise limits are formed on the log(−log) scale,
//! `exp(−exp(ln(−ln F̂) ± z·SE/(F̂ |ln F̂|)))`, which keeps them inside (0, 1).
//!
//! # Gray's test
//! For cause 1 (any cause can play that part) and groups `k = 1…K`, let
//! `R_k(t) = Y_k(t) [1 − F̂_1k(t−)] / Ŝ_k(t−)` be the estimated number in
//! group `k` still exposed to cause 1 in the subdistribution sense (units at
//! risk, plus those that failed from another cause, reweighted). The scores
//! compare the observed cause-1 failures with their share of the pooled ones,
//!
//! ```text
//! z_k = Σ_t [ d_1k(t) − R_k(t) d_1·(t) / R_·(t) ],
//! ```
//!
//! which is Gray's (1988, *Annals of Statistics* 16, 1141–1154) statistic
//! with weight `L(t) = 1` (the default `ρ = 0` of his class). The
//! covariance is the one of his §2, from the asymptotic representation of
//! `z` as a sum of integrals over each group's cause-specific counting-process
//! martingales, with the group-specific cause-specific hazards estimated by
//! `d/Y`; ties are handled by the exact linearization of the product-limit
//! estimator (denominators `Y − d`), and the multinomial covariance of the
//! failures at a time. The statistic `zᵀ V⁻ z` over `K − 1` groups is
//! referred to χ² with `K − 1` degrees of freedom.
//!
//! # Fine–Gray regression
//! The subdistribution hazard of cause 1 is modelled as
//! `λ₁(t | Z) = λ₁₀(t) exp(βᵀZ)` (Fine & Gray 1999, *JASA* 94, 496–509), so
//! `F₁(t | Z) = 1 − exp(−Λ₁₀(t) e^{βᵀZ})`. Units that failed from another
//! cause stay in the risk set after their failure with weight
//! `w_j(t) = Ĝ(t−)/Ĝ(X_j−)`, where `Ĝ` is the Kaplan–Meier estimate of the
//! censoring distribution (censored rows as its events); units at risk have
//! weight 1. β maximizes the weighted partial likelihood (Breslow's handling
//! of ties) by Newton–Raphson with step halving; the baseline cumulative
//! subdistribution hazard is the Breslow-type estimate
//! `Λ̂₁₀(t) = Σ_{t_i ≤ t} d_1i / Σ_j w_j(t_i) e^{β̂ᵀZ_j}`.
//!
//! The robust covariance is the sandwich `I⁻¹ (Σ_i (η_i + ψ_i)(η_i + ψ_i)ᵀ) I⁻¹`
//! of Fine & Gray (1999, §3): `η_i` is unit `i`'s weighted score residual and
//! `ψ_i` accounts for estimating `Ĝ` (each unit's censoring martingale
//! acting through the weights of units that failed from other causes). With
//! no censoring `ψ ≡ 0` and the fit is Cox regression on the subdistribution
//! risk set with the Lin–Wei robust covariance.
//!
//! # Failure modes of a series system
//! Each mode's life distribution is fitted by maximum likelihood with
//! [`fit_censored`](crate::censored_fit::fit_censored), failures from the
//! other modes being right-censored at their times (valid when the modes act
//! independently). The system fails at its first mode failure, so
//! `F_sys(t) = 1 − Π_j (1 − F̂_j(t))`. Because the modes' estimates come from
//! separate likelihoods they are independent, and the delta method gives
//! `Var F̂_sys(t) = Σ_j [Π_{k≠j} (1 − F̂_k(t))]² Var F̂_j(t)`; limits are formed
//! on the logit scale (Meeker & Escobar 1998, §15.3).

use stat_core::distribution_fit::FitFamily;
use stat_core::{ChiSquared, ContinuousDistribution, Normal, Result, StatError};

use crate::censored_fit::{CensoredFit, fit_censored};
use crate::life_data::{LifeData, LifeObservation};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn z_value(level: f64) -> Result<f64> {
    if !(level > 0.0 && level < 1.0) {
        return Err(invalid("the confidence level must be between 0 and 1"));
    }
    Normal::standard().quantile(0.5 + level / 2.0)
}

/// One unit (or a frequency of identical units) in a competing-risks study.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompetingObservation {
    /// Failure or censoring time.
    pub time: f64,
    /// Index of the cause that ended the unit's life, `None` when censored.
    pub cause: Option<usize>,
    /// Frequency weight (number of identical units).
    pub weight: f64,
}

impl CompetingObservation {
    /// A failure from `cause` at `time`.
    #[must_use]
    pub fn failure(time: f64, cause: usize, weight: f64) -> Self {
        Self { time, cause: Some(cause), weight }
    }

    /// A unit still running at `time`.
    #[must_use]
    pub fn censored(time: f64, weight: f64) -> Self {
        Self { time, cause: None, weight }
    }
}

fn check(obs: &[CompetingObservation], n_causes: usize) -> Result<()> {
    if obs.is_empty() {
        return Err(invalid("no observations"));
    }
    if n_causes == 0 {
        return Err(invalid("at least one cause is needed"));
    }
    for o in obs {
        if !o.time.is_finite() || o.time < 0.0 {
            return Err(invalid("times must be finite and non-negative"));
        }
        if !(o.weight.is_finite() && o.weight > 0.0) {
            return Err(invalid("frequencies must be positive"));
        }
        if o.cause.is_some_and(|c| c >= n_causes) {
            return Err(invalid("a cause index is out of range"));
        }
    }
    Ok(())
}

/// Per distinct time: `(time, at risk, failures per cause, censored)`.
fn risk_table(obs: &[CompetingObservation], n_causes: usize) -> Vec<(f64, f64, Vec<f64>, f64)> {
    let mut sorted: Vec<&CompetingObservation> = obs.iter().collect();
    sorted.sort_by(|a, b| a.time.total_cmp(&b.time));
    let mut at_risk: f64 = obs.iter().map(|o| o.weight).sum();
    let mut out = Vec::new();
    let mut i = 0;
    while i < sorted.len() {
        let t = sorted[i].time;
        let mut d = vec![0.0; n_causes];
        let mut c = 0.0;
        while i < sorted.len() && sorted[i].time == t {
            match sorted[i].cause {
                Some(j) => d[j] += sorted[i].weight,
                None => c += sorted[i].weight,
            }
            i += 1;
        }
        let removed = d.iter().sum::<f64>() + c;
        out.push((t, at_risk, d, c));
        at_risk -= removed;
    }
    out
}

// ----------------------------------------------------------------------------
// Aalen–Johansen
// ----------------------------------------------------------------------------

/// The estimates at one distinct failure time.
#[derive(Debug, Clone, PartialEq)]
pub struct IncidenceStep {
    /// Failure time `t_i`.
    pub time: f64,
    /// Units at risk just before `t_i` (weighted).
    pub at_risk: f64,
    /// Failures from each cause at `t_i`.
    pub failures: Vec<f64>,
    /// Kaplan–Meier probability of surviving every cause past `t_i`.
    pub survival: f64,
    /// Cumulative incidence of each cause at `t_i`.
    pub incidence: Vec<f64>,
    /// Delta-method variance of each cause's cumulative incidence at `t_i`.
    pub variance: Vec<f64>,
}

impl IncidenceStep {
    /// Standard error of cause `j`'s incidence.
    #[must_use]
    pub fn std_error(&self, j: usize) -> f64 {
        self.variance[j].max(0.0).sqrt()
    }
}

/// The Aalen–Johansen estimates of every cause's cumulative incidence.
#[derive(Debug, Clone, PartialEq)]
pub struct CumulativeIncidence {
    /// Number of causes.
    pub n_causes: usize,
    /// One step per distinct failure time (any cause), increasing.
    pub steps: Vec<IncidenceStep>,
    /// The largest observed time (failure or censoring).
    pub end: f64,
    /// Total units (weighted).
    pub n: f64,
}

/// log(−log) limits of an incidence `f` with standard error `se`.
fn loglog_limits(f: f64, se: f64, z: f64) -> (f64, f64) {
    if f.is_nan() || f <= 0.0 {
        return (0.0, 0.0);
    }
    if f >= 1.0 || se.is_nan() || se <= 0.0 {
        return (f, f);
    }
    let l = f.ln();
    let s = z * se / (f * l.abs());
    let center = (-l).ln();
    ((-(center + s).exp()).exp(), (-(center - s).exp()).exp())
}

impl CumulativeIncidence {
    fn index_at(&self, t: f64) -> Option<usize> {
        let k = self.steps.partition_point(|s| s.time <= t);
        k.checked_sub(1)
    }

    /// `F̂_j(t)` (a right-continuous step function, 0 before the first failure).
    #[must_use]
    pub fn incidence_at(&self, j: usize, t: f64) -> f64 {
        self.index_at(t).map_or(0.0, |k| self.steps[k].incidence[j])
    }

    /// Standard error of `F̂_j(t)`.
    #[must_use]
    pub fn std_error_at(&self, j: usize, t: f64) -> f64 {
        self.index_at(t).map_or(0.0, |k| self.steps[k].std_error(j))
    }

    /// Kaplan–Meier probability of surviving every cause past `t`.
    #[must_use]
    pub fn survival_at(&self, t: f64) -> f64 {
        self.index_at(t).map_or(1.0, |k| self.steps[k].survival)
    }

    /// Pointwise log(−log) limits of `F̂_j` at step `k` with confidence `level`.
    ///
    /// # Errors
    /// A confidence level outside (0, 1).
    pub fn limits(&self, j: usize, k: usize, level: f64) -> Result<(f64, f64)> {
        let z = z_value(level)?;
        let s = &self.steps[k];
        Ok(loglog_limits(s.incidence[j], s.std_error(j), z))
    }

    /// Vertices of `F̂_j` as a step function from `start`, extended to the
    /// last observed time. With `stack`, the sum of causes `0..=j` (the
    /// stacked display, whose top curve is `1 − Ŝ`).
    #[must_use]
    pub fn step_polyline(&self, j: usize, start: f64, stack: bool) -> Vec<[f64; 2]> {
        let value = |s: &IncidenceStep| -> f64 { if stack { s.incidence[..=j].iter().sum() } else { s.incidence[j] } };
        let mut out = vec![[start, 0.0]];
        let mut level = 0.0;
        for s in &self.steps {
            let v = value(s);
            if v != level {
                out.push([s.time, level]);
                out.push([s.time, v]);
                level = v;
            }
        }
        out.push([self.end.max(start), level]);
        out
    }

    /// Pointwise limits of `F̂_j` as constant pieces `[t_start, t_end, lower,
    /// upper]` from its first failure to the last observed time.
    ///
    /// # Errors
    /// A confidence level outside (0, 1).
    pub fn band_pieces(&self, j: usize, level: f64) -> Result<Vec<[f64; 4]>> {
        let z = z_value(level)?;
        let mut out = Vec::new();
        let marks: Vec<usize> = (0..self.steps.len()).filter(|&k| self.steps[k].failures[j] > 0.0).collect();
        for (m, &k) in marks.iter().enumerate() {
            let s = &self.steps[k];
            let next = marks.get(m + 1).map_or(self.end, |&n| self.steps[n].time);
            let (lo, hi) = loglog_limits(s.incidence[j], s.std_error(j), z);
            if next > s.time {
                out.push([s.time, next, lo, hi]);
            }
        }
        Ok(out)
    }
}

/// The Aalen–Johansen cumulative incidence of each of `n_causes` causes,
/// with delta-method variances (see the module docs).
///
/// # Errors
/// No observations, a negative or non-finite time, a non-positive frequency,
/// or a cause index of `n_causes` or more.
pub fn aalen_johansen(obs: &[CompetingObservation], n_causes: usize) -> Result<CumulativeIncidence> {
    check(obs, n_causes)?;
    let table = risk_table(obs, n_causes);
    let n: f64 = obs.iter().map(|o| o.weight).sum();
    let end = table.last().map_or(0.0, |r| r.0);
    let mut steps: Vec<IncidenceStep> = Vec::new();
    let mut survival = 1.0;
    let mut incidence = vec![0.0; n_causes];
    // Accumulators for the variance (expanded squares, as sums over steps i ≤ k):
    // a = d/(n(n−d)) for Σ a (F_k − F_i)², b = S_{i−1} d_j/n² for Σ b (F_k − F_i),
    // c = S_{i−1}² d_j (n − d_j)/n³.
    let mut sum_a = 0.0;
    let mut sum_af = vec![0.0; n_causes];
    let mut sum_aff = vec![0.0; n_causes];
    let mut sum_b = vec![0.0; n_causes];
    let mut sum_bf = vec![0.0; n_causes];
    let mut sum_c = vec![0.0; n_causes];
    for (t, at_risk, d, _) in table {
        let total: f64 = d.iter().sum();
        if total <= 0.0 {
            continue;
        }
        let before = survival;
        for j in 0..n_causes {
            incidence[j] += before * d[j] / at_risk;
        }
        survival = before * (1.0 - total / at_risk);
        if at_risk - total > 0.0 {
            let a = total / (at_risk * (at_risk - total));
            sum_a += a;
            for j in 0..n_causes {
                sum_af[j] += a * incidence[j];
                sum_aff[j] += a * incidence[j] * incidence[j];
            }
        }
        let mut variance = vec![0.0; n_causes];
        for j in 0..n_causes {
            let b = before * d[j] / (at_risk * at_risk);
            sum_b[j] += b;
            sum_bf[j] += b * incidence[j];
            sum_c[j] += before * before * d[j] * (at_risk - d[j]) / at_risk.powi(3);
            let f = incidence[j];
            let first = f * f * sum_a - 2.0 * f * sum_af[j] + sum_aff[j];
            let third = f * sum_b[j] - sum_bf[j];
            variance[j] = (first + sum_c[j] - 2.0 * third).max(0.0);
        }
        steps.push(IncidenceStep { time: t, at_risk, failures: d, survival, incidence: incidence.clone(), variance });
    }
    Ok(CumulativeIncidence { n_causes, steps, end, n })
}

// ----------------------------------------------------------------------------
// Gray's test
// ----------------------------------------------------------------------------

/// Gray's K-sample test of one cause's cumulative incidence.
#[derive(Debug, Clone, PartialEq)]
pub struct GrayTest {
    /// The cause tested.
    pub cause: usize,
    /// Score of each group (they add up to zero).
    pub scores: Vec<f64>,
    /// Covariance of the scores of the first `K − 1` groups.
    pub covariance: Vec<Vec<f64>>,
    /// χ² statistic.
    pub statistic: f64,
    /// Degrees of freedom, `K − 1`.
    pub df: f64,
    /// Upper-tail χ² p-value.
    pub p_value: f64,
}

/// Solve `A x = b` for symmetric positive definite `A` (Cholesky); `None`
/// when `A` is not positive definite.
fn spd_solve(a: &[Vec<f64>], b: &[f64]) -> Option<Vec<f64>> {
    let l = stat_core::eigen::cholesky(a)?;
    let n = b.len();
    let mut y = vec![0.0; n];
    for i in 0..n {
        y[i] = (b[i] - (0..i).map(|k| l[i][k] * y[k]).sum::<f64>()) / l[i][i];
    }
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        x[i] = (y[i] - (i + 1..n).map(|k| l[k][i] * x[k]).sum::<f64>()) / l[i][i];
    }
    Some(x)
}

/// Inverse of a symmetric positive definite matrix.
fn spd_inverse(a: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let n = a.len();
    let mut cols = Vec::with_capacity(n);
    for i in 0..n {
        let mut e = vec![0.0; n];
        e[i] = 1.0;
        cols.push(spd_solve(a, &e)?);
    }
    Some((0..n).map(|r| (0..n).map(|c| 0.5 * (cols[c][r] + cols[r][c])).collect()).collect())
}

/// Per-group quantities at each pooled failure time, for Gray's test.
struct GroupPath {
    /// At risk `Y(t)`.
    y: Vec<f64>,
    /// Failures from the tested cause, `d₁(t)`.
    d1: Vec<f64>,
    /// Failures from other causes, `d₂(t)`.
    d2: Vec<f64>,
    /// `Ŝ(t−)`.
    s_before: Vec<f64>,
    /// `F̂₁(t−)`.
    f_before: Vec<f64>,
    /// `F̂₁(t)`.
    f_after: Vec<f64>,
}

/// Gray's test that the cumulative incidence of `cause` is equal in the
/// `n_groups` groups (`groups[i]` is the group of `obs[i]`); see the module
/// docs.
///
/// # Errors
/// Invalid observations, fewer than two groups, a group without units, no
/// failures from `cause`, or a singular covariance.
pub fn gray_test(obs: &[CompetingObservation], groups: &[usize], n_groups: usize, cause: usize) -> Result<GrayTest> {
    let n_causes = obs.iter().filter_map(|o| o.cause).max().map_or(1, |m| m + 1).max(cause + 1);
    check(obs, n_causes)?;
    if groups.len() != obs.len() {
        return Err(StatError::DimensionMismatch { context: "groups", expected: obs.len(), found: groups.len() });
    }
    if n_groups < 2 {
        return Err(invalid("Gray's test needs at least two groups"));
    }
    if groups.iter().any(|&g| g >= n_groups) {
        return Err(invalid("a group index is out of range"));
    }
    if !obs.iter().any(|o| o.cause == Some(cause)) {
        return Err(invalid("there are no failures from the tested cause"));
    }
    // Pooled distinct failure times (any cause).
    let mut times: Vec<f64> = obs.iter().filter(|o| o.cause.is_some()).map(|o| o.time).collect();
    times.sort_by(f64::total_cmp);
    times.dedup();
    let m = times.len();
    let mut paths = Vec::with_capacity(n_groups);
    for g in 0..n_groups {
        let mine: Vec<CompetingObservation> =
            obs.iter().zip(groups).filter(|(_, k)| **k == g).map(|(o, _)| *o).collect();
        if mine.is_empty() {
            return Err(invalid("every group needs at least one unit"));
        }
        let mut p = GroupPath {
            y: vec![0.0; m],
            d1: vec![0.0; m],
            d2: vec![0.0; m],
            s_before: vec![1.0; m],
            f_before: vec![0.0; m],
            f_after: vec![0.0; m],
        };
        let mut total: f64 = mine.iter().map(|o| o.weight).sum();
        let mut sorted = mine;
        sorted.sort_by(|a, b| a.time.total_cmp(&b.time));
        let mut idx = 0;
        let (mut s, mut f) = (1.0, 0.0);
        for (k, &t) in times.iter().enumerate() {
            // Remove units whose time is before t.
            while idx < sorted.len() && sorted[idx].time < t {
                total -= sorted[idx].weight;
                idx += 1;
            }
            p.y[k] = total.max(0.0);
            let mut j = idx;
            while j < sorted.len() && sorted[j].time == t {
                match sorted[j].cause {
                    Some(c) if c == cause => p.d1[k] += sorted[j].weight,
                    Some(_) => p.d2[k] += sorted[j].weight,
                    None => {}
                }
                j += 1;
            }
            p.s_before[k] = s;
            p.f_before[k] = f;
            if p.y[k] > 0.0 {
                f += s * p.d1[k] / p.y[k];
                s *= 1.0 - (p.d1[k] + p.d2[k]) / p.y[k];
            }
            p.f_after[k] = f;
        }
        paths.push(p);
    }
    // R_k(t) and the scores.
    let r: Vec<Vec<f64>> = paths
        .iter()
        .map(|p| {
            (0..m)
                .map(|k| {
                    if p.y[k] > 0.0 && p.s_before[k] > 0.0 {
                        p.y[k] * (1.0 - p.f_before[k]) / p.s_before[k]
                    } else {
                        0.0
                    }
                })
                .collect()
        })
        .collect();
    let r_total: Vec<f64> = (0..m).map(|k| r.iter().map(|rk| rk[k]).sum()).collect();
    let d1_total: Vec<f64> = (0..m).map(|k| paths.iter().map(|p| p.d1[k]).sum()).collect();
    let mut scores = vec![0.0; n_groups];
    for k in 0..m {
        if r_total[k] <= 0.0 {
            continue;
        }
        for g in 0..n_groups {
            scores[g] += paths[g].d1[k] - r[g][k] * d1_total[k] / r_total[k];
        }
    }
    // Covariance: for each tested group a (first K − 1) and each group l, the
    // coefficients b1, b2 of l's cause-specific martingale increments.
    let q = n_groups - 1;
    let mut cov = vec![vec![0.0; q]; q];
    for (l, p) in paths.iter().enumerate() {
        // coefficients[a] = (b1, b2) per time.
        let mut coef: Vec<Vec<(f64, f64)>> = Vec::with_capacity(q);
        for a in 0..q {
            let delta = if a == l { 1.0 } else { 0.0 };
            let share = |k: usize| if r_total[k] > 0.0 { r[a][k] / r_total[k] } else { 0.0 };
            let mut c = vec![(0.0, 0.0); m];
            // B(v) = Σ_{t > v} A(t)/(1 − F₁(t−)), A(t) = (δ − R_a/R)·d₁_l(t).
            let mut b_after = 0.0;
            for v in (0..m).rev() {
                let b = b_after;
                if p.y[v] > 0.0 {
                    let d = p.d1[v] + p.d2[v];
                    let spare = p.y[v] - d;
                    let lin = if spare > 0.0 { b * (1.0 - p.f_after[v]) / spare } else { 0.0 };
                    let w = if r_total[v] > 0.0 { delta - share(v) } else { 0.0 };
                    let b1 = w + b * p.s_before[v] / p.y[v] - lin;
                    let b2 = -lin;
                    c[v] = (b1, b2);
                }
                let one_minus = 1.0 - p.f_before[v];
                if one_minus > 0.0 && r_total[v] > 0.0 {
                    b_after += (delta - share(v)) * p.d1[v] / one_minus;
                }
            }
            coef.push(c);
        }
        for v in 0..m {
            let y = p.y[v];
            if y <= 0.0 {
                continue;
            }
            let (d1, d2) = (p.d1[v], p.d2[v]);
            let v11 = d1 * (1.0 - d1 / y);
            let v22 = d2 * (1.0 - d2 / y);
            let v12 = -d1 * d2 / y;
            for a in 0..q {
                let (a1, a2) = coef[a][v];
                for b in 0..q {
                    let (c1, c2) = coef[b][v];
                    cov[a][b] += a1 * c1 * v11 + a2 * c2 * v22 + (a1 * c2 + a2 * c1) * v12;
                }
            }
        }
    }
    let x = spd_solve(&cov, &scores[..q])
        .ok_or_else(|| invalid("the covariance of Gray's scores is singular (too few failures in a group)"))?;
    let statistic: f64 = x.iter().zip(&scores[..q]).map(|(a, b)| a * b).sum::<f64>().max(0.0);
    let df = q as f64;
    let p_value = ChiSquared::new(df)?.sf(statistic)?;
    Ok(GrayTest { cause, scores, covariance: cov, statistic, df, p_value })
}

// ----------------------------------------------------------------------------
// Fine–Gray regression
// ----------------------------------------------------------------------------

/// A Fine–Gray proportional subdistribution hazards fit.
#[derive(Debug, Clone, PartialEq)]
pub struct FineGrayFit {
    /// The modelled cause.
    pub cause: usize,
    /// Coefficients `β̂` (log subdistribution hazard ratios).
    pub coefficients: Vec<f64>,
    /// Robust (sandwich) covariance of `β̂`, including the censoring-weight term.
    pub covariance: Vec<Vec<f64>>,
    /// Model-based covariance `I(β̂)⁻¹`.
    pub naive_covariance: Vec<Vec<f64>>,
    /// Maximized log partial likelihood.
    pub log_likelihood: f64,
    /// Log partial likelihood at `β = 0`.
    pub null_log_likelihood: f64,
    /// Newton–Raphson iterations used.
    pub iterations: usize,
    /// Breslow-type baseline cumulative subdistribution hazard: `(t_i, Λ̂₁₀(t_i))`
    /// at each failure time of the modelled cause.
    pub baseline: Vec<[f64; 2]>,
    /// Weighted failures from the modelled cause.
    pub n_events: f64,
    /// The last observed time.
    pub end: f64,
}

/// One coefficient with its robust standard error, Wald test and the
/// subdistribution hazard ratio's interval.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HazardRatioEstimate {
    /// `β̂`.
    pub coefficient: f64,
    /// Robust standard error.
    pub std_error: f64,
    /// Wald `z = β̂ / SE`.
    pub z: f64,
    /// Two-sided p-value.
    pub p_value: f64,
    /// `exp(β̂)`.
    pub hazard_ratio: f64,
    /// Lower limit of `exp(β)`.
    pub lower: f64,
    /// Upper limit of `exp(β)`.
    pub upper: f64,
}

impl FineGrayFit {
    /// Robust standard errors.
    #[must_use]
    pub fn std_errors(&self) -> Vec<f64> {
        (0..self.coefficients.len()).map(|i| self.covariance[i][i].max(0.0).sqrt()).collect()
    }

    /// Each coefficient's subdistribution hazard ratio with Wald limits at
    /// confidence `level` (robust standard errors).
    ///
    /// # Errors
    /// A confidence level outside (0, 1).
    pub fn hazard_ratios(&self, level: f64) -> Result<Vec<HazardRatioEstimate>> {
        let z = z_value(level)?;
        let normal = Normal::standard();
        Ok(self
            .coefficients
            .iter()
            .zip(self.std_errors())
            .map(|(&b, se)| {
                let zs = b / se;
                let p_value = 2.0 * normal.sf(zs.abs()).unwrap_or(f64::NAN);
                HazardRatioEstimate {
                    coefficient: b,
                    std_error: se,
                    z: zs,
                    p_value,
                    hazard_ratio: b.exp(),
                    lower: (b - z * se).exp(),
                    upper: (b + z * se).exp(),
                }
            })
            .collect())
    }

    /// Robust Wald test that every coefficient is zero: `(χ², df, p)`.
    ///
    /// # Errors
    /// A singular robust covariance.
    pub fn wald_test(&self) -> Result<(f64, f64, f64)> {
        let x = spd_solve(&self.covariance, &self.coefficients)
            .ok_or_else(|| invalid("the robust covariance is singular"))?;
        let chi: f64 = x.iter().zip(&self.coefficients).map(|(a, b)| a * b).sum();
        let df = self.coefficients.len() as f64;
        Ok((chi, df, ChiSquared::new(df)?.sf(chi.max(0.0))?))
    }

    /// Baseline cumulative subdistribution hazard at `t`.
    #[must_use]
    pub fn baseline_at(&self, t: f64) -> f64 {
        let k = self.baseline.partition_point(|b| b[0] <= t);
        if k == 0 { 0.0 } else { self.baseline[k - 1][1] }
    }

    /// Predicted cumulative incidence `1 − exp(−Λ̂₁₀(t) e^{β̂ᵀz})` at `t` for
    /// covariates `z` (coded as the design the fit was made with).
    #[must_use]
    pub fn incidence_at(&self, z: &[f64], t: f64) -> f64 {
        let eta: f64 = self.coefficients.iter().zip(z).map(|(b, x)| b * x).sum();
        -(-self.baseline_at(t) * eta.exp()).exp_m1()
    }

    /// The predicted cumulative incidence for covariates `z` as a step
    /// function from `start` to the last observed time.
    #[must_use]
    pub fn incidence_polyline(&self, z: &[f64], start: f64) -> Vec<[f64; 2]> {
        let eta: f64 = self.coefficients.iter().zip(z).map(|(b, x)| b * x).sum();
        let mut out = vec![[start, 0.0]];
        let mut level = 0.0;
        for b in &self.baseline {
            let v = -(-b[1] * eta.exp()).exp_m1();
            out.push([b[0], level]);
            out.push([b[0], v]);
            level = v;
        }
        out.push([self.end.max(start), level]);
        out
    }
}

/// Weighted sums at one failure time: `S0`, `S1` (p) and `S2` (p×p).
struct RiskSums {
    s0: f64,
    s1: Vec<f64>,
    s2: Vec<Vec<f64>>,
}

/// The subdistribution risk set's weights at each failure time of the
/// modelled cause, shared by the fit and its variance.
struct Subdistribution<'a> {
    obs: &'a [CompetingObservation],
    cause: usize,
    /// Distinct failure times of the modelled cause, with their weighted counts.
    event_times: Vec<(f64, f64)>,
    /// Censoring Kaplan–Meier: distinct censoring times `u`, at risk `Y(u)`,
    /// censored `c(u)`.
    censor_steps: Vec<(f64, f64, f64)>,
}

impl Subdistribution<'_> {
    /// `Ĝ(t−)`: product over censoring times `u < t` of `1 − c(u)/Y(u)`.
    fn g_before(&self, t: f64) -> f64 {
        self.censor_steps.iter().take_while(|s| s.0 < t).map(|s| 1.0 - s.2 / s.1).product()
    }

    /// Risk weight `r_j(t)` of unit `j` at failure time `t` (with `Ĝ(t−)`
    /// supplied), as in the module docs.
    fn weight(&self, j: usize, t: f64, g_t: f64, g_unit: &[f64]) -> f64 {
        let o = &self.obs[j];
        if o.time >= t {
            1.0
        } else {
            match o.cause {
                Some(c) if c != self.cause && g_unit[j] > 0.0 => g_t / g_unit[j],
                _ => 0.0,
            }
        }
    }
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Fit the Fine–Gray model for `cause` with covariates `x` (one row of `p`
/// values per observation, already coded: indicators for categorical
/// factors). See the module docs.
///
/// # Errors
/// Invalid observations, rows of different lengths, no covariates, no
/// failures from `cause`, a singular information matrix (a covariate constant
/// over the risk sets), or no convergence (e.g. a covariate that perfectly
/// separates failures from the cause, so the likelihood has no maximum).
pub fn fine_gray(obs: &[CompetingObservation], x: &[Vec<f64>], cause: usize) -> Result<FineGrayFit> {
    let n_causes = obs.iter().filter_map(|o| o.cause).max().map_or(1, |m| m + 1).max(cause + 1);
    check(obs, n_causes)?;
    if x.len() != obs.len() {
        return Err(StatError::DimensionMismatch { context: "covariate rows", expected: obs.len(), found: x.len() });
    }
    let p = x.first().map_or(0, Vec::len);
    if p == 0 {
        return Err(invalid("choose at least one covariate"));
    }
    if x.iter().any(|r| r.len() != p || r.iter().any(|v| !v.is_finite())) {
        return Err(invalid("every row needs a finite value of every covariate"));
    }
    let n = obs.len();
    // Failure times of the cause.
    let mut event_times: Vec<(f64, f64)> = Vec::new();
    {
        let mut ev: Vec<(f64, f64)> =
            obs.iter().filter(|o| o.cause == Some(cause)).map(|o| (o.time, o.weight)).collect();
        ev.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (t, w) in ev {
            match event_times.last_mut() {
                Some(last) if last.0 == t => last.1 += w,
                _ => event_times.push((t, w)),
            }
        }
    }
    if event_times.is_empty() {
        return Err(invalid("there are no failures from the modelled cause"));
    }
    // Censoring Kaplan–Meier (censored rows are its events; at risk: time ≥ u).
    let table = risk_table(obs, n_causes);
    let censor_steps: Vec<(f64, f64, f64)> = table.iter().filter(|r| r.3 > 0.0).map(|r| (r.0, r.1, r.3)).collect();
    let sd = Subdistribution { obs, cause, event_times, censor_steps };
    let g_unit: Vec<f64> = obs.iter().map(|o| sd.g_before(o.time)).collect();
    let g_event: Vec<f64> = sd.event_times.iter().map(|e| sd.g_before(e.0)).collect();
    let end = table.last().map_or(0.0, |r| r.0);

    let sums = |beta: &[f64], k: usize, second: bool| -> RiskSums {
        let t = sd.event_times[k].0;
        let mut s = RiskSums { s0: 0.0, s1: vec![0.0; p], s2: vec![vec![0.0; if second { p } else { 0 }]; p] };
        for j in 0..n {
            let r = sd.weight(j, t, g_event[k], &g_unit);
            if r == 0.0 {
                continue;
            }
            let e = obs[j].weight * r * dot(beta, &x[j]).exp();
            s.s0 += e;
            for a in 0..p {
                s.s1[a] += e * x[j][a];
                if second {
                    for b in 0..p {
                        s.s2[a][b] += e * x[j][a] * x[j][b];
                    }
                }
            }
        }
        s
    };
    // Sum of covariates over the failures at each event time.
    let mut zsum: Vec<Vec<f64>> = vec![vec![0.0; p]; sd.event_times.len()];
    for (j, o) in obs.iter().enumerate() {
        if o.cause == Some(cause) {
            let k = sd.event_times.partition_point(|e| e.0 < o.time);
            for a in 0..p {
                zsum[k][a] += o.weight * x[j][a];
            }
        }
    }
    let loglik = |beta: &[f64]| -> f64 {
        let mut l = 0.0;
        for (k, &(_, d)) in sd.event_times.iter().enumerate() {
            let s = sums(beta, k, false);
            if s.s0 <= 0.0 {
                return f64::NEG_INFINITY;
            }
            l += dot(beta, &zsum[k]) - d * s.s0.ln();
        }
        l
    };
    let derivatives = |beta: &[f64]| -> (Vec<f64>, Vec<Vec<f64>>) {
        let mut u = vec![0.0; p];
        let mut info = vec![vec![0.0; p]; p];
        for (k, &(_, d)) in sd.event_times.iter().enumerate() {
            let s = sums(beta, k, true);
            for a in 0..p {
                let ea = s.s1[a] / s.s0;
                u[a] += zsum[k][a] - d * ea;
                for b in 0..p {
                    info[a][b] += d * (s.s2[a][b] / s.s0 - ea * s.s1[b] / s.s0);
                }
            }
        }
        (u, info)
    };
    let null_log_likelihood = loglik(&vec![0.0; p]);
    let mut beta = vec![0.0; p];
    let mut ll = null_log_likelihood;
    let mut iterations = 0;
    let mut converged = false;
    for it in 1..=100 {
        iterations = it;
        let (u, info) = derivatives(&beta);
        let step = spd_solve(&info, &u).ok_or_else(|| {
            invalid("the information matrix is singular: a covariate does not vary within the risk sets")
        })?;
        let mut scale = 1.0;
        let mut accepted = false;
        for _ in 0..30 {
            let trial: Vec<f64> = beta.iter().zip(&step).map(|(b, s)| b + scale * s).collect();
            let lt = loglik(&trial);
            if lt.is_finite() && lt >= ll - 1e-12 * (1.0 + ll.abs()) {
                let change = step.iter().map(|s| (scale * s).abs()).fold(0.0, f64::max);
                beta = trial;
                let gain = lt - ll;
                ll = lt;
                accepted = true;
                if change < 1e-10 || gain.abs() < 1e-13 * (1.0 + ll.abs()) && change < 1e-6 {
                    converged = true;
                }
                break;
            }
            scale *= 0.5;
        }
        if !accepted {
            // No ascent left at this resolution: accept when the score is negligible.
            converged = u.iter().zip(&step).all(|(g, s)| (g * s).abs() < 1e-9 * (1.0 + ll.abs()));
            break;
        }
        if converged {
            break;
        }
        if beta.iter().any(|b| b.abs() > 50.0) {
            break;
        }
    }
    if !converged || beta.iter().any(|b| !b.is_finite() || b.abs() > 50.0) {
        return Err(invalid(
            "the subdistribution hazards fit did not converge (a covariate may separate the failures from the \
             modelled cause completely)",
        ));
    }
    let (_, info) = derivatives(&beta);
    let naive_covariance =
        spd_inverse(&info).ok_or_else(|| invalid("the information matrix is singular at the estimate"))?;

    // Baseline and per-time means.
    let m = sd.event_times.len();
    let mut s0 = vec![0.0; m];
    let mut ebar = vec![vec![0.0; p]; m];
    let mut dlambda = vec![0.0; m];
    let mut baseline = Vec::with_capacity(m);
    let mut cum = 0.0;
    for k in 0..m {
        let s = sums(&beta, k, false);
        s0[k] = s.s0;
        for a in 0..p {
            ebar[k][a] = s.s1[a] / s.s0;
        }
        dlambda[k] = sd.event_times[k].1 / s.s0;
        cum += dlambda[k];
        baseline.push([sd.event_times[k].0, cum]);
    }
    // η_i: weighted score residuals.
    let mut infl: Vec<Vec<f64>> = vec![vec![0.0; p]; n];
    for i in 0..n {
        let e = dot(&beta, &x[i]).exp();
        if obs[i].cause == Some(cause) {
            let k = sd.event_times.partition_point(|ev| ev.0 < obs[i].time);
            for a in 0..p {
                infl[i][a] += x[i][a] - ebar[k][a];
            }
        }
        for k in 0..m {
            let r = sd.weight(i, sd.event_times[k].0, g_event[k], &g_unit);
            if r == 0.0 {
                continue;
            }
            for a in 0..p {
                infl[i][a] -= r * e * (x[i][a] - ebar[k][a]) * dlambda[k];
            }
        }
    }
    // ψ_i: the censoring martingales acting through the competing failures' weights.
    // Q(u) = Σ_{t > u} ΔΛ(t) Σ_{j competing, X_j ≤ u} f_j r_j(t) e^{βZ_j} (Z_j − Ē(t)).
    let competing: Vec<usize> = (0..n).filter(|&j| matches!(obs[j].cause, Some(c) if c != cause)).collect();
    for &(u, y_u, c_u) in &sd.censor_steps {
        let spare = y_u - c_u;
        if spare <= 0.0 {
            continue;
        }
        let mut q = vec![0.0; p];
        for k in 0..m {
            let t = sd.event_times[k].0;
            if t <= u {
                continue;
            }
            for &j in &competing {
                if obs[j].time > u {
                    continue;
                }
                let r = sd.weight(j, t, g_event[k], &g_unit);
                if r == 0.0 {
                    continue;
                }
                let e = obs[j].weight * r * dot(&beta, &x[j]).exp() * dlambda[k];
                for a in 0..p {
                    q[a] += e * (x[j][a] - ebar[k][a]);
                }
            }
        }
        if q.iter().all(|v| *v == 0.0) {
            continue;
        }
        let hazard = c_u / y_u;
        for i in 0..n {
            if obs[i].time < u {
                continue;
            }
            let dn = if obs[i].time == u && obs[i].cause.is_none() { 1.0 } else { 0.0 };
            let dm = (dn - hazard) / spare;
            for a in 0..p {
                infl[i][a] += q[a] * dm;
            }
        }
    }
    let mut meat = vec![vec![0.0; p]; p];
    for (i, f) in infl.iter().enumerate() {
        for a in 0..p {
            for b in 0..p {
                meat[a][b] += obs[i].weight * f[a] * f[b];
            }
        }
    }
    let covariance: Vec<Vec<f64>> = (0..p)
        .map(|a| {
            (0..p)
                .map(|b| {
                    let mut v = 0.0;
                    for c in 0..p {
                        for d in 0..p {
                            v += naive_covariance[a][c] * meat[c][d] * naive_covariance[d][b];
                        }
                    }
                    v
                })
                .collect()
        })
        .collect();
    let n_events = sd.event_times.iter().map(|e| e.1).sum();
    Ok(FineGrayFit {
        cause,
        coefficients: beta,
        covariance,
        naive_covariance,
        log_likelihood: ll,
        null_log_likelihood,
        iterations,
        baseline,
        n_events,
        end,
    })
}

// ----------------------------------------------------------------------------
// Failure modes of a series system
// ----------------------------------------------------------------------------

/// The life data of failure mode `mode`: its failures exact, every other
/// unit right-censored at its time.
///
/// # Errors
/// Invalid observations.
pub fn mode_life_data(obs: &[CompetingObservation], mode: usize) -> Result<LifeData> {
    LifeData::new(
        obs.iter()
            .map(|o| {
                if o.cause == Some(mode) {
                    LifeObservation::exact(o.time, o.weight)
                } else {
                    LifeObservation::right(o.time, o.weight)
                }
            })
            .collect(),
    )
}

/// Fit `family` to each of `n_modes` failure modes separately, the other
/// modes' failures treated as censoring (see the module docs). A mode whose
/// fit fails (for example one with no failures) gives its error.
#[must_use]
pub fn fit_failure_modes(obs: &[CompetingObservation], n_modes: usize, family: FitFamily) -> Vec<Result<CensoredFit>> {
    (0..n_modes).map(|j| mode_life_data(obs, j).and_then(|d| fit_censored(&d, family))).collect()
}

/// The system's failure probability at a time, with its delta-method
/// standard error and logit-scale limits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SystemProbability {
    /// `F̂_sys(t)`.
    pub estimate: f64,
    /// Delta-method standard error.
    pub std_error: f64,
    /// Lower limit.
    pub lower: f64,
    /// Upper limit.
    pub upper: f64,
}

/// A series system of independently acting failure modes, each with its own
/// fitted life distribution.
#[derive(Debug, Clone)]
pub struct SeriesSystem {
    /// The modes' fits.
    pub modes: Vec<CensoredFit>,
}

impl SeriesSystem {
    /// `F̂_sys(t) = 1 − Π_j (1 − F̂_j(t))`.
    #[must_use]
    pub fn failure_probability(&self, t: f64) -> f64 {
        1.0 - self.modes.iter().map(|m| 1.0 - m.distribution.cdf(t)).product::<f64>()
    }

    /// `F̂_sys(t)` with delta-method standard error and logit-scale limits at
    /// confidence `level` (see the module docs).
    ///
    /// # Errors
    /// A confidence level outside (0, 1).
    pub fn probability_interval(&self, t: f64, level: f64) -> Result<SystemProbability> {
        let z = z_value(level)?;
        let f: Vec<f64> = self.modes.iter().map(|m| m.distribution.cdf(t)).collect();
        let estimate = 1.0 - f.iter().map(|v| 1.0 - v).product::<f64>();
        let mut var = 0.0;
        for (j, m) in self.modes.iter().enumerate() {
            let fj = m.failure_probability(t, level, false);
            let se = if fj.std_error.is_finite() { fj.std_error } else { 0.0 };
            let others: f64 = f.iter().enumerate().filter(|(k, _)| *k != j).map(|(_, v)| 1.0 - v).product();
            var += (others * se).powi(2);
        }
        let std_error = var.sqrt();
        let (lower, upper) = if estimate > 0.0 && estimate < 1.0 && std_error > 0.0 {
            let logit = estimate.ln() - (-estimate).ln_1p();
            let s = z * std_error / (estimate * (1.0 - estimate));
            let expit = |v: f64| 1.0 / (1.0 + (-v).exp());
            (expit(logit - s), expit(logit + s))
        } else {
            (estimate, estimate)
        };
        Ok(SystemProbability { estimate, std_error, lower, upper })
    }

    /// The time by which a fraction `p` of systems has failed (bisection on
    /// `F̂_sys`), NaN when it cannot be bracketed.
    #[must_use]
    pub fn quantile(&self, p: f64) -> f64 {
        if !(p > 0.0 && p < 1.0) || self.modes.is_empty() {
            return f64::NAN;
        }
        // Bracket: the system fails no later than its earliest mode quantile.
        let mut hi = self.modes.iter().map(|m| m.distribution.quantile(p)).fold(f64::INFINITY, f64::min);
        if !hi.is_finite() {
            return f64::NAN;
        }
        let mut lo = self.modes.iter().map(|m| m.distribution.lower_bound()).fold(f64::NEG_INFINITY, f64::max);
        if !lo.is_finite() {
            lo = hi - 1.0;
            while self.failure_probability(lo) > p {
                lo -= 2.0 * (hi - lo);
            }
        }
        hi = hi.max(lo);
        for _ in 0..200 {
            let mid = 0.5 * (lo + hi);
            if self.failure_probability(mid) < p {
                lo = mid;
            } else {
                hi = mid;
            }
            if hi - lo <= 1e-12 * hi.abs().max(1e-300) {
                break;
            }
        }
        0.5 * (lo + hi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simple() -> Vec<CompetingObservation> {
        // Times with two causes and censoring; no ties across causes.
        let raw = [
            (1.0, Some(0)),
            (2.0, Some(1)),
            (3.0, None),
            (4.0, Some(0)),
            (5.0, Some(0)),
            (6.0, Some(1)),
            (7.0, None),
            (8.0, Some(1)),
            (9.0, Some(0)),
            (10.0, None),
        ];
        raw.iter().map(|&(t, c)| CompetingObservation { time: t, cause: c, weight: 1.0 }).collect()
    }

    #[test]
    fn incidences_add_up_to_one_minus_survival() {
        let aj = aalen_johansen(&simple(), 2).unwrap();
        for s in &aj.steps {
            let total: f64 = s.incidence.iter().sum();
            assert!((total - (1.0 - s.survival)).abs() < 1e-14);
        }
        // Hand values: F_0(1) = 1/10; S(1) = 0.9; F_1(2) = 0.9/9 = 0.1.
        assert!((aj.incidence_at(0, 1.0) - 0.1).abs() < 1e-15);
        assert!((aj.incidence_at(1, 2.0) - 0.1).abs() < 1e-15);
        assert_eq!(aj.incidence_at(0, 0.5), 0.0);
        // First step variance: S0² d (n − d)/n³ = 9/1000.
        assert!((aj.steps[0].variance[0] - 0.009).abs() < 1e-15);
        let (lo, hi) = aj.limits(0, 0, 0.95).unwrap();
        assert!(lo < 0.1 && hi > 0.1 && lo > 0.0 && hi < 1.0);
    }

    #[test]
    fn frequencies_match_repeated_rows() {
        let one = simple();
        let doubled: Vec<CompetingObservation> =
            one.iter().map(|o| CompetingObservation { weight: 2.0, ..*o }).collect();
        let repeated: Vec<CompetingObservation> = one.iter().chain(one.iter()).copied().collect();
        let a = aalen_johansen(&doubled, 2).unwrap();
        let b = aalen_johansen(&repeated, 2).unwrap();
        for (x, y) in a.steps.iter().zip(&b.steps) {
            assert!((x.incidence[0] - y.incidence[0]).abs() < 1e-14);
            assert!((x.variance[1] - y.variance[1]).abs() < 1e-14);
        }
    }

    #[test]
    fn gray_test_is_zero_for_identical_groups() {
        let one = simple();
        let both: Vec<CompetingObservation> = one.iter().chain(one.iter()).copied().collect();
        let groups: Vec<usize> = (0..both.len()).map(|i| usize::from(i >= one.len())).collect();
        let g = gray_test(&both, &groups, 2, 0).unwrap();
        assert!(g.statistic.abs() < 1e-20 && (g.p_value - 1.0).abs() < 1e-9, "{g:?}");
        assert!(g.scores.iter().sum::<f64>().abs() < 1e-12);
    }

    #[test]
    fn gray_test_has_its_nominal_size() {
        // Two groups with the same cause-specific hazards: rejections at 5%
        // should be near 5% (a loose check over 300 simulated studies).
        let mut rng = stat_core::rng::Rng::new(20_240_517);
        let mut rejections = 0;
        let sims = 300;
        for _ in 0..sims {
            let mut obs = Vec::new();
            let mut groups = Vec::new();
            for g in 0..2 {
                for _ in 0..60 {
                    let t1 = -(1.0 - rng.next_f64()).ln() / 0.5;
                    let t2 = -(1.0 - rng.next_f64()).ln() / 0.3;
                    let c = -(1.0 - rng.next_f64()).ln() / 0.2;
                    let t = t1.min(t2).min(c);
                    let cause = if t == c {
                        None
                    } else if t == t1 {
                        Some(0)
                    } else {
                        Some(1)
                    };
                    obs.push(CompetingObservation { time: t, cause, weight: 1.0 });
                    groups.push(g);
                }
            }
            if gray_test(&obs, &groups, 2, 0).unwrap().p_value < 0.05 {
                rejections += 1;
            }
        }
        let rate = f64::from(rejections) / f64::from(sims);
        assert!(rate > 0.015 && rate < 0.10, "rejection rate {rate}");
    }

    #[test]
    fn fine_gray_without_covariate_effect_and_errors() {
        let obs = simple();
        let x: Vec<Vec<f64>> = (0..obs.len()).map(|i| vec![(i % 2) as f64]).collect();
        let fit = fine_gray(&obs, &x, 0).unwrap();
        assert_eq!(fit.coefficients.len(), 1);
        assert!(fit.std_errors()[0] > 0.0);
        let hr = fit.hazard_ratios(0.95).unwrap();
        assert!(hr[0].lower < hr[0].hazard_ratio && hr[0].hazard_ratio < hr[0].upper);
        // The predicted incidence is a distribution function.
        let curve = fit.incidence_polyline(&[0.0], 0.0);
        assert!(curve.windows(2).all(|w| w[1][1] >= w[0][1] - 1e-15));
        assert!(fine_gray(&obs, &[], 0).is_err());
        let constant: Vec<Vec<f64>> = vec![vec![1.0]; obs.len()];
        assert!(fine_gray(&obs, &constant, 0).is_err());
    }

    #[test]
    fn series_system_combines_modes() {
        let obs = simple();
        let fits: Vec<CensoredFit> =
            fit_failure_modes(&obs, 2, FitFamily::Exponential).into_iter().map(|f| f.unwrap()).collect();
        // Exponential modes: rates add.
        let rate: f64 = fits.iter().map(|f| 1.0 / f.distribution.scale).sum();
        let sys = SeriesSystem { modes: fits };
        let t = 3.0;
        assert!((sys.failure_probability(t) - (1.0 - (-rate * t).exp())).abs() < 1e-12);
        let pi = sys.probability_interval(t, 0.95).unwrap();
        assert!(pi.lower < pi.estimate && pi.estimate < pi.upper && pi.std_error > 0.0);
        let q = sys.quantile(0.5);
        assert!((sys.failure_probability(q) - 0.5).abs() < 1e-9);
    }
}
