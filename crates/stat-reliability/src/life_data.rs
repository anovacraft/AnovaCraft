//! Life data with exact, right-, left- and interval-censored observations,
//! Turnbull's nonparametric estimate of the distribution function, and an
//! Anderson–Darling distance between that estimate and a fitted distribution.
//!
//! # Observations
//! Each observation says that a unit's failure time `T` lies in an interval
//! and carries a weight (a frequency):
//!
//! | Kind | Known | Interval |
//! |---|---|---|
//! | exact failure | `T = t` | `[t, t]` |
//! | right-censored | `T > l` (still running at `l`) | `(l, ∞)` |
//! | left-censored | `T ≤ u` (already failed at the first look) | `(−∞, u]` |
//! | interval-censored | `l < T ≤ u` (failed between inspections) | `(l, u]` |
//!
//! # Turnbull's estimate
//! The nonparametric maximum-likelihood estimate of `F` for such data puts
//! all its probability on the **innermost intervals** `[qⱼ, pⱼ]`: intervals
//! whose left end is an observation's left end and whose right end is an
//! observation's right end, with no other end point between them (Turnbull
//! 1976, "The empirical distribution function with arbitrarily grouped,
//! censored and truncated data", *JRSS B* 38, 290–295; Peto 1973). With
//! `αᵢⱼ = 1` when innermost interval `j` lies inside observation `i`'s
//! interval and weights `wᵢ`, the masses `sⱼ` maximize
//! `Σᵢ wᵢ ln Σⱼ αᵢⱼ sⱼ` and satisfy the self-consistency equations
//!
//! ```text
//! sⱼ = (1/W) Σᵢ wᵢ αᵢⱼ sⱼ / Σₖ αᵢₖ sₖ,      W = Σᵢ wᵢ,
//! ```
//!
//! which are iterated (an EM algorithm, Dempster, Laird & Rubin 1977) from
//! equal masses, accelerated by squared extrapolation (SQUAREM; Varadhan &
//! Roland 2008, *Scand. J. Statist.* 35) with a fall-back to the plain step
//! whenever the extrapolated step lowers the likelihood. Within an innermost
//! interval that is not a single point the estimate does not say how the
//! mass is spread; plots join its ends with a straight line. For exact and
//! right-censored data the innermost intervals are the failure times (plus
//! `(l_max, ∞)` when the largest time is censored) and the estimate is the
//! Kaplan–Meier product-limit estimate (Efron 1967).
//!
//! # Anderson–Darling distance
//! [`censored_anderson_darling`] evaluates the integral form of the
//! Anderson–Darling statistic (Anderson & Darling 1952, *Ann. Math. Statist.*
//! 23, 193–212)
//!
//! ```text
//! A² = W ∫ (F̂(t) − F(t))² / [F(t)(1 − F(t))] dF(t)
//! ```
//!
//! with the empirical distribution function replaced by the nonparametric
//! estimate, as Koziol & Green (1976, *Biometrika* 63, 465–474) do for the
//! Cramér–von Mises statistic with the product-limit estimate. The integral
//! runs over the range where the estimate is determined: up to `u = 1` when
//! it has placed all its mass at finite times, otherwise up to the largest
//! finite time in the data (as for Type I censored samples, Pettitt &
//! Stephens 1976, *Biometrika* 63). Inside an innermost interval the
//! estimate is taken to rise linearly in `u = F(t)`. On complete data the
//! statistic equals the usual computing formula
//! `A² = −n − (1/n) Σ (2i − 1)[ln F(x₍ᵢ₎) + ln(1 − F(x₍ₙ₊₁₋ᵢ₎))]`.
//! Each piece is integrated exactly: for `F̂ = c + (b + 1)u` on `[u₁, u₂]`,
//! `∫ (c + bu)²/(u(1 − u)) du = c² ln(u₂/u₁) − (c + b)² ln((1 − u₂)/(1 − u₁)) − b²(u₂ − u₁)`.
//! Smaller is better; with censoring there is no general null distribution,
//! so the statistic is used to compare fits on the same data.

use stat_core::distribution_fit::FittedDistribution;
use stat_core::{Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// How an observation's failure time is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CensorKind {
    /// The failure time is known.
    Exact,
    /// The unit had not failed by the recorded time.
    Right,
    /// The unit had failed by the recorded time.
    Left,
    /// The unit failed between two times.
    Interval,
}

impl CensorKind {
    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Exact => "Exact failures",
            Self::Right => "Right-censored",
            Self::Left => "Left-censored",
            Self::Interval => "Interval-censored",
        }
    }
}

/// One observation: the failure time lies in `(lower, upper]` (exactly at
/// `lower` when `lower == upper`), with a frequency weight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LifeObservation {
    /// Lower end (`−∞` for a left-censored observation).
    pub lower: f64,
    /// Upper end (`+∞` for a right-censored observation).
    pub upper: f64,
    /// Frequency (positive).
    pub weight: f64,
}

impl LifeObservation {
    /// A failure at `t`.
    #[must_use]
    pub fn exact(t: f64, weight: f64) -> Self {
        Self { lower: t, upper: t, weight }
    }

    /// Still running at `t`.
    #[must_use]
    pub fn right(t: f64, weight: f64) -> Self {
        Self { lower: t, upper: f64::INFINITY, weight }
    }

    /// Failed by `t`.
    #[must_use]
    pub fn left(t: f64, weight: f64) -> Self {
        Self { lower: f64::NEG_INFINITY, upper: t, weight }
    }

    /// Failed in `(lower, upper]` (an exact failure when the two are equal).
    #[must_use]
    pub fn interval(lower: f64, upper: f64, weight: f64) -> Self {
        Self { lower, upper, weight }
    }

    /// The kind of observation.
    #[must_use]
    pub fn kind(&self) -> CensorKind {
        if self.lower == self.upper {
            CensorKind::Exact
        } else if self.upper == f64::INFINITY {
            CensorKind::Right
        } else if self.lower == f64::NEG_INFINITY {
            CensorKind::Left
        } else {
            CensorKind::Interval
        }
    }
}

/// A validated set of life observations.
#[derive(Debug, Clone, PartialEq)]
pub struct LifeData {
    observations: Vec<LifeObservation>,
}

impl LifeData {
    /// Validate observations.
    ///
    /// # Errors
    /// [`StatError::InsufficientData`] when there are none;
    /// [`StatError::InvalidArgument`] for a NaN end, a weight that is not
    /// finite and positive, a lower end above the upper end, an interval
    /// that is unbounded at both ends, or an exact time that is infinite.
    pub fn new(observations: Vec<LifeObservation>) -> Result<Self> {
        if observations.is_empty() {
            return Err(StatError::InsufficientData { required: 1, found: 0 });
        }
        for o in &observations {
            if o.lower.is_nan() || o.upper.is_nan() {
                return Err(invalid("an observation has a missing end"));
            }
            if !(o.weight.is_finite() && o.weight > 0.0) {
                return Err(invalid("frequencies must be finite and positive"));
            }
            if o.lower > o.upper {
                return Err(invalid(format!("an interval starts at {} after it ends at {}", o.lower, o.upper)));
            }
            if o.lower == f64::NEG_INFINITY && o.upper == f64::INFINITY {
                return Err(invalid("an observation is unbounded at both ends"));
            }
            if o.lower == o.upper && !o.lower.is_finite() {
                return Err(invalid("an exact failure time must be finite"));
            }
            if o.lower == f64::INFINITY || o.upper == f64::NEG_INFINITY {
                return Err(invalid("an observation lies at infinity"));
            }
        }
        Ok(Self { observations })
    }

    /// The observations.
    #[must_use]
    pub fn observations(&self) -> &[LifeObservation] {
        &self.observations
    }

    /// Total weight `W = Σ wᵢ` (the number of units).
    #[must_use]
    pub fn total_weight(&self) -> f64 {
        self.observations.iter().map(|o| o.weight).sum()
    }

    /// Total weight of the observations of one kind.
    #[must_use]
    pub fn weight_of(&self, kind: CensorKind) -> f64 {
        self.observations.iter().filter(|o| o.kind() == kind).map(|o| o.weight).sum()
    }

    /// `true` when every observation is an exact failure or right-censored.
    #[must_use]
    pub fn is_right_censored_only(&self) -> bool {
        self.observations.iter().all(|o| matches!(o.kind(), CensorKind::Exact | CensorKind::Right))
    }

    /// The smallest time by which some unit is known to have failed (the
    /// least upper end of the observations that are not right-censored);
    /// a threshold must lie below it. `None` when every unit is right-censored.
    #[must_use]
    pub fn earliest_failure_bound(&self) -> Option<f64> {
        self.observations.iter().filter(|o| o.upper.is_finite()).map(|o| o.upper).reduce(f64::min)
    }

    /// The smallest and largest finite end of any observation.
    #[must_use]
    pub fn finite_range(&self) -> (f64, f64) {
        self.observations
            .iter()
            .flat_map(|o| [o.lower, o.upper])
            .filter(|v| v.is_finite())
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)))
    }
}

/// One innermost interval of Turnbull's estimate with its probability.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TurnbullInterval {
    /// Left end `qⱼ` (`−∞` possible).
    pub lower: f64,
    /// Right end `pⱼ` (`+∞` possible).
    pub upper: f64,
    /// Whether the left end belongs to the interval (a single point
    /// `[t, t]`, or an interval whose left end is an exact failure time).
    pub lower_closed: bool,
    /// Estimated probability `sⱼ`.
    pub probability: f64,
}

/// Turnbull's nonparametric estimate of the failure-time distribution.
#[derive(Debug, Clone, PartialEq)]
pub struct NonparametricEstimate {
    /// Innermost intervals in increasing order (probabilities sum to one).
    pub intervals: Vec<TurnbullInterval>,
    /// Self-consistency iterations used.
    pub iterations: usize,
    /// Log-likelihood `Σ wᵢ ln Σⱼ αᵢⱼ sⱼ` at the estimate.
    pub log_likelihood: f64,
    /// Total weight `W`.
    pub total_weight: f64,
    /// Largest finite end of any observation.
    pub max_time: f64,
}

impl NonparametricEstimate {
    /// `F̂` after interval `j`: `Σ_{k ≤ j} sₖ`.
    #[must_use]
    pub fn cumulative(&self) -> Vec<f64> {
        let mut acc = 0.0;
        self.intervals
            .iter()
            .map(|iv| {
                acc += iv.probability;
                acc.min(1.0)
            })
            .collect()
    }

    /// `F̂(t)` where it is determined (outside the innermost intervals that
    /// are not single points); inside one, the straight line between its ends
    /// in `t`. `None` beyond the largest finite time when mass remains there.
    #[must_use]
    pub fn cdf_at(&self, t: f64) -> Option<f64> {
        let cum = self.cumulative();
        let mut before = 0.0;
        for (j, iv) in self.intervals.iter().enumerate() {
            let inside_left = t > iv.lower || (iv.lower_closed && t == iv.lower);
            if !inside_left {
                return Some(before);
            }
            if t >= iv.upper {
                before = cum[j];
                continue;
            }
            // Inside a proper innermost interval.
            if !iv.upper.is_finite() || !iv.lower.is_finite() {
                return None;
            }
            let f = (t - iv.lower) / (iv.upper - iv.lower);
            return Some(before + f * (cum[j] - before));
        }
        Some(before)
    }

    /// Vertices of the estimated survival curve `1 − F̂` from `start`: flat
    /// between innermost intervals, dropping at single points, and a straight
    /// line across an innermost interval that is not a point. Ends at the
    /// largest finite time.
    #[must_use]
    pub fn survival_polyline(&self, start: f64) -> Vec<[f64; 2]> {
        let cum = self.cumulative();
        let mut out = vec![[start, 1.0]];
        let mut level = 1.0;
        for (j, iv) in self.intervals.iter().enumerate() {
            if !iv.upper.is_finite() {
                break;
            }
            let left = if iv.lower.is_finite() { iv.lower.max(start) } else { start };
            out.push([left, level]);
            level = 1.0 - cum[j];
            out.push([iv.upper, level]);
        }
        out.push([self.max_time.max(start), level]);
        out
    }

    /// `true` when the estimate puts all its probability at finite times.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.intervals.iter().filter(|iv| !iv.upper.is_finite()).map(|iv| iv.probability).sum::<f64>() < 1e-12
    }
}

/// An end point in the Turnbull ordering: at equal values, a closed left end
/// (exact failure) comes first, then right ends, then open left ends.
#[derive(Debug, Clone, Copy, PartialEq)]
struct EndKey {
    value: f64,
    rank: u8,
}

impl EndKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.value.total_cmp(&other.value).then(self.rank.cmp(&other.rank))
    }
}

fn left_key(o: &LifeObservation) -> EndKey {
    EndKey { value: o.lower, rank: if o.lower == o.upper { 0 } else { 2 } }
}

fn right_key(o: &LifeObservation) -> EndKey {
    EndKey { value: o.upper, rank: 1 }
}

/// Turnbull's (1976) self-consistent nonparametric estimate (see the module
/// docs).
///
/// # Errors
/// [`StatError::ConvergenceFailure`] if the iterations do not settle
/// (maximum change of a mass below `1e-13`) within 100 000 steps.
pub fn turnbull(data: &LifeData) -> Result<NonparametricEstimate> {
    let obs = data.observations();
    // Sorted end points: (key, is_left).
    let mut ends: Vec<(EndKey, bool)> = Vec::with_capacity(2 * obs.len());
    for o in obs {
        ends.push((left_key(o), true));
        ends.push((right_key(o), false));
    }
    ends.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    let mut inner: Vec<(EndKey, EndKey)> = Vec::new();
    for w in ends.windows(2) {
        if w[0].1 && !w[1].1 {
            inner.push((w[0].0, w[1].0));
        }
    }
    let m = inner.len();
    // Membership αᵢⱼ as index lists.
    let members: Vec<Vec<usize>> = obs
        .iter()
        .map(|o| {
            let (l, r) = (left_key(o), right_key(o));
            (0..m).filter(|&j| inner[j].0.cmp(&l).is_ge() && inner[j].1.value <= r.value).collect()
        })
        .collect();
    let total = data.total_weight();
    let weights: Vec<f64> = obs.iter().map(|o| o.weight / total).collect();
    let em = |s: &[f64]| -> Vec<f64> {
        let mut next = vec![0.0; m];
        for (i, js) in members.iter().enumerate() {
            let denom: f64 = js.iter().map(|&j| s[j]).sum();
            if denom > 0.0 {
                for &j in js {
                    next[j] += weights[i] * s[j] / denom;
                }
            }
        }
        next
    };
    let loglik = |s: &[f64]| -> f64 {
        members
            .iter()
            .zip(&weights)
            .map(|(js, w)| {
                let p: f64 = js.iter().map(|&j| s[j]).sum();
                w * p.ln()
            })
            .sum()
    };
    let mut s = vec![1.0 / m as f64; m];
    let mut ll = loglik(&s);
    let mut iterations = 0;
    let mut converged = false;
    while iterations < 100_000 {
        let s1 = em(&s);
        let s2 = em(&s1);
        iterations += 2;
        let r: Vec<f64> = s1.iter().zip(&s).map(|(a, b)| a - b).collect();
        let v: Vec<f64> = (0..m).map(|j| s2[j] - 2.0 * s1[j] + s[j]).collect();
        let (nr, nv) = (r.iter().map(|x| x * x).sum::<f64>().sqrt(), v.iter().map(|x| x * x).sum::<f64>().sqrt());
        let mut candidate = s2.clone();
        if nv > 0.0 && nr > 0.0 {
            let alpha = -(nr / nv).max(1.0);
            let mut x: Vec<f64> = (0..m).map(|j| (s[j] - 2.0 * alpha * r[j] + alpha * alpha * v[j]).max(0.0)).collect();
            let sum: f64 = x.iter().sum();
            if sum > 0.0 {
                x.iter_mut().for_each(|p| *p /= sum);
                let stabilized = em(&x);
                iterations += 1;
                let l = loglik(&stabilized);
                if l.is_finite() && l >= loglik(&s2) {
                    candidate = stabilized;
                }
            }
        }
        let change = candidate.iter().zip(&s).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
        s = candidate;
        let new_ll = loglik(&s);
        let settled = change < 1e-13 || (change < 1e-10 && (new_ll - ll).abs() < 1e-15);
        ll = new_ll;
        if settled {
            converged = true;
            break;
        }
    }
    if !converged {
        return Err(StatError::ConvergenceFailure { routine: "Turnbull self-consistency", iterations });
    }
    // Plain steps polish the extrapolated solution.
    for _ in 0..20 {
        s = em(&s);
    }
    let sum: f64 = s.iter().sum();
    s.iter_mut().for_each(|p| *p /= sum);
    let intervals = inner
        .iter()
        .zip(&s)
        .map(|((l, r), &p)| TurnbullInterval {
            lower: l.value,
            upper: r.value,
            lower_closed: l.rank == 0,
            probability: if p < 1e-14 { 0.0 } else { p },
        })
        .collect();
    let max_time = data.finite_range().1;
    Ok(NonparametricEstimate {
        intervals,
        iterations,
        log_likelihood: total * loglik(&s),
        total_weight: total,
        max_time,
    })
}

/// `∫_{u₁}^{u₂} (F̂(u) − u)² / (u(1 − u)) du` for `F̂` rising linearly from
/// `f1` at `u₁` to `f2` at `u₂`, with `0·ln 0 = 0`. Writing `F̂ − u = c + b·u`,
/// the coefficients `c = (f1 − u₁) − b·u₁` and `c + b = (f2 − u₂) + b(1 − u₂)`
/// are formed from the end values so that they are exactly `f1` at `u₁ = 0`
/// and exactly `f2 − 1` at `u₂ = 1`: an estimate that reaches one at `u = 1`
/// gives a finite integral rather than a rounding residue times `ln 0`.
fn ad_piece(f1: f64, f2: f64, u1: f64, u2: f64) -> f64 {
    if u2 <= u1 {
        return 0.0;
    }
    let b = (f2 - f1) / (u2 - u1) - 1.0;
    let c = (f1 - u1) - b * u1;
    let cb = (f2 - u2) + b * (1.0 - u2);
    let term = |coef: f64, ratio: f64| if coef == 0.0 { 0.0 } else { coef * coef * ratio.ln() };
    term(c, u2 / u1) - term(cb, (1.0 - u2) / (1.0 - u1)) - b * b * (u2 - u1)
}

/// The Anderson–Darling distance between the nonparametric estimate and a
/// fitted distribution (see the module docs). `+∞` when the estimate puts
/// probability where the fitted distribution has none.
#[must_use]
pub fn censored_anderson_darling(estimate: &NonparametricEstimate, fit: &FittedDistribution) -> f64 {
    let u = |t: f64| -> f64 {
        if t == f64::NEG_INFINITY {
            0.0
        } else if t == f64::INFINITY {
            1.0
        } else {
            fit.cdf(t)
        }
    };
    let end_u = if estimate.is_complete() { 1.0 } else { u(estimate.max_time) };
    let mut total = 0.0;
    let mut level = 0.0;
    let mut u_prev = 0.0;
    for iv in &estimate.intervals {
        if !iv.upper.is_finite() {
            break;
        }
        let (ua, ub) = (u(iv.lower).min(end_u), u(iv.upper).min(end_u));
        // Flat piece up to the interval.
        total += ad_piece(level, level, u_prev, ua.max(u_prev));
        let mut next = level + iv.probability;
        if 1.0 - next < 1e-12 {
            // All the mass is placed (the sum of the masses can fall an ulp
            // or two short of one).
            next = 1.0;
        }
        let start = ua.max(u_prev);
        if ub > start {
            // F̂ rises linearly in u from `level` to `next`.
            total += ad_piece(level, next, start, ub);
        } else if iv.probability > 0.0 && ub <= 0.0 {
            return f64::INFINITY;
        }
        level = next.min(1.0);
        u_prev = ub.max(u_prev);
    }
    total += ad_piece(level, level, u_prev, end_u);
    let a2 = estimate.total_weight * total;
    if a2.is_nan() { f64::INFINITY } else { a2 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kaplan_meier::kaplan_meier;
    use stat_core::distribution_fit::{FitFamily, anderson_darling_statistic, fit_distribution};

    /// For exact and right-censored data Turnbull's estimate is Kaplan–Meier.
    #[test]
    fn turnbull_reduces_to_kaplan_meier() {
        let times = [3.0, 5.0, 5.0, 8.0, 10.0, 12.0, 12.0, 15.0, 20.0, 22.0, 25.0];
        let events = [true, true, false, true, false, true, true, false, true, false, false];
        let obs = times
            .iter()
            .zip(events)
            .map(|(&t, e)| if e { LifeObservation::exact(t, 1.0) } else { LifeObservation::right(t, 1.0) })
            .collect();
        let est = turnbull(&LifeData::new(obs).unwrap()).unwrap();
        let km = kaplan_meier(&times, &events).unwrap();
        for s in &km.steps {
            let f = est.cdf_at(s.time).unwrap();
            assert!((1.0 - f - s.survival).abs() < 1e-10, "t = {}: {} vs {}", s.time, 1.0 - f, s.survival);
        }
        assert!(!est.is_complete());
    }

    /// On complete data the integral form equals the computing formula.
    #[test]
    fn anderson_darling_matches_complete_formula() {
        let x = [2.1, 3.4, 3.9, 4.4, 5.0, 5.8, 6.1, 7.3, 8.8, 10.2];
        let data = LifeData::new(x.iter().map(|&t| LifeObservation::exact(t, 1.0)).collect()).unwrap();
        let est = turnbull(&data).unwrap();
        assert!(est.is_complete());
        let fit = fit_distribution(&x, FitFamily::Weibull).unwrap();
        let a = censored_anderson_darling(&est, &fit);
        let b = anderson_darling_statistic(&x, &fit);
        assert!((a - b).abs() < 1e-10 * b, "{a} vs {b}");
    }

    /// When the masses of a complete sample sum to an ulp or two below one,
    /// the last piece used to multiply that residue by `ln 0` and return an
    /// infinite distance (it did for every sample size below).
    #[test]
    fn anderson_darling_is_finite_when_masses_sum_short_of_one() {
        for n in [5usize, 9, 15, 21, 22, 23, 28, 30, 31, 38] {
            let mut x: Vec<f64> =
                (0..n).map(|i| 1.0 + (i as f64 * 0.731).sin().abs() * 10.0 + i as f64 * 0.01).collect();
            x.sort_by(f64::total_cmp);
            let data = LifeData::new(x.iter().map(|&t| LifeObservation::exact(t, 1.0)).collect()).unwrap();
            let est = turnbull(&data).unwrap();
            let fit = fit_distribution(&x, FitFamily::Weibull).unwrap();
            let a = censored_anderson_darling(&est, &fit);
            let b = anderson_darling_statistic(&x, &fit);
            assert!((a - b).abs() < 1e-8 * b, "n = {n}: {a} vs {b}");
        }
    }

    #[test]
    fn validation() {
        assert!(LifeData::new(Vec::new()).is_err());
        assert!(LifeData::new(vec![LifeObservation::interval(3.0, 2.0, 1.0)]).is_err());
        assert!(LifeData::new(vec![LifeObservation::exact(1.0, 0.0)]).is_err());
        let d = LifeData::new(vec![LifeObservation::left(4.0, 2.0), LifeObservation::interval(1.0, 3.0, 1.0)]).unwrap();
        assert_eq!(d.weight_of(CensorKind::Left), 2.0);
        assert_eq!(d.earliest_failure_bound(), Some(3.0));
    }
}
