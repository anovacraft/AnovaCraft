//! Acceptance sampling: lot-by-lot sampling plans, their operating
//! characteristics and the plans that meet a producer's and a consumer's risk
//! point (Montgomery, *Introduction to Statistical Quality Control*, ch. 15;
//! Schilling & Neubauer, *Acceptance Sampling in Quality Control*).
//!
//! # Plans
//!
//! * **Single attribute plan** `(n, c)`: inspect `n` items, accept the lot when
//!   at most `c` are nonconforming.
//! * **Double attribute plan** `(n₁, c₁, n₂, c₂)`: inspect `n₁`; accept when
//!   `d₁ ≤ c₁`, reject when `d₁ > c₂`, otherwise inspect `n₂` more and accept
//!   when `d₁ + d₂ ≤ c₂`.
//! * **Variables plan by the k-method** `(n, k)` for one specification limit
//!   (shown for an upper limit `U`; a lower limit is symmetric): accept when
//!   `(U − x̄)/σ ≥ k` (σ known) or `(U − x̄)/s ≥ k` (σ unknown). The quality
//!   characteristic is assumed normal, so the fraction nonconforming `p`
//!   fixes the distance `z_p = Φ⁻¹(1 − p)` from the mean to the limit in σ units.
//!
//! # Probability of acceptance `Pa(p)` (the OC curve)
//!
//! For attribute plans the number nonconforming in a sample of `n` follows
//! one of three models ([`OcModel`]): binomial `Bin(n, p)` (type-B OC, a
//! process or a large lot), hypergeometric with `D = round(pN)` nonconforming
//! items in the lot of `N` (type-A OC, an isolated lot) or Poisson with mean
//! `np` (an approximation, or counts of nonconformities). Then
//!
//! ```text
//! single:  Pa = P(d ≤ c)
//! double:  Pa = P(d₁ ≤ c₁) + Σ_{j = c₁+1}^{c₂} P(d₁ = j) · P(d₂ ≤ c₂ − j)
//! ```
//!
//! (for the hypergeometric model the second sample is drawn from the `N − n₁`
//! items left, with `D − j` nonconforming). For variables plans
//!
//! ```text
//! σ known:    Pa = Φ(√n (z_p − k))
//! σ unknown:  Pa = P(T ≥ k√n),  T ~ noncentral t(n − 1, δ = √n z_p)
//! ```
//!
//! which is exact under normality (the noncentral-t form; Schilling &
//! Neubauer ch. 7).
//!
//! # Rectifying inspection
//!
//! Rejected lots are screened 100% and nonconforming items replaced. With lot
//! size `N`:
//!
//! ```text
//! AOQ(p) = p · Pa · (N − n) / N                              (single, variables)
//! AOQ(p) = p · [Pa_I (N − n₁) + Pa_II (N − n₁ − n₂)] / N       (double)
//! ATI(p) = n + (1 − Pa)(N − n)                               (single, variables)
//! ATI(p) = n₁ Pa_I + (n₁ + n₂) Pa_II + N (1 − Pa)              (double)
//! ASN(p) = n₁ + n₂ · P(c₁ < d₁ ≤ c₂)                          (double; n otherwise)
//! ```
//!
//! with `Pa_I` and `Pa_II` the probabilities of accepting on the first and on
//! the second sample. The average outgoing quality limit (AOQL) is the
//! maximum of `AOQ(p)` over `p`.
//!
//! # Plans from risk points
//!
//! Given the producer's point `(p₁, 1 − α)` (acceptable quality level) and
//! the consumer's point `(p₂, β)` (lot tolerance percent defective):
//!
//! * [`single_plan`]: for `c = 0, 1, 2, …` the smallest `n` with
//!   `Pa(p₂) ≤ β` is found; the first `c` whose plan also has
//!   `Pa(p₁) ≥ 1 − α` gives the plan. Because that smallest `n` grows with
//!   `c`, this is the plan with the smallest sample size meeting both points.
//! * [`double_plan`]: with `n₂ = r·n₁` for a chosen ratio `r`, every
//!   `c₁ < c₂` up to a bound is tried with its smallest `n₁` meeting the
//!   consumer's point; among those that meet the producer's point the plan
//!   with the smallest average sample number at `p₁` is chosen.
//! * [`variables_plan`]: with σ known, the classical solution
//!   `n = ⌈((z_α + z_β)/(z_{p₁} − z_{p₂}))²⌉`,
//!   `k = (z_{p₁} z_β + z_{p₂} z_α)/(z_α + z_β)`. With σ unknown, the smallest
//!   `n` for which some `k` meets both points under the exact noncentral-t OC
//!   curve, and `k` midway between the largest value meeting the producer's
//!   point and the smallest meeting the consumer's point.
//!
//! No tables from published sampling standards are used: every plan is
//! computed from the risk points.

use stat_core::discrete::{binomial_cdf, binomial_pmf, hypergeometric_pmf, poisson_cdf, poisson_pmf};
use stat_core::distributions::{ContinuousDistribution, Normal};
use stat_core::noncentral::NoncentralT;
use stat_core::{Result, StatError};

/// Largest acceptance number tried by the plan searches.
pub const MAX_ACCEPTANCE_NUMBER: u64 = 400;
/// Largest sample size the plan searches consider (unless the lot is smaller).
pub const MAX_SAMPLE_SIZE: u64 = 1_000_000;

/// Distribution of the number nonconforming in a sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum OcModel {
    /// Binomial: sampling from a process or a large lot (type-B OC curve).
    #[default]
    Binomial,
    /// Hypergeometric: sampling without replacement from a lot of the given
    /// size holding `round(pN)` nonconforming items (type-A OC curve).
    Hypergeometric,
    /// Poisson with mean `np`.
    Poisson,
}

impl OcModel {
    /// Every model, in menu order.
    pub const ALL: [Self; 3] = [Self::Binomial, Self::Hypergeometric, Self::Poisson];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Binomial => "Binomial (process or large lot)",
            Self::Hypergeometric => "Hypergeometric (isolated lot)",
            Self::Poisson => "Poisson",
        }
    }

    /// Key used in project files (`"binomial"`, `"hypergeometric"`, `"poisson"`).
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Binomial => "binomial",
            Self::Hypergeometric => "hypergeometric",
            Self::Poisson => "poisson",
        }
    }

    /// The model with this [`key`](Self::key).
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }
}

/// Whether a variables plan uses a known process σ or the sample SD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SigmaKnowledge {
    /// σ known: the statistic is `(U − x̄)/σ`.
    #[default]
    Known,
    /// σ unknown: the statistic is `(U − x̄)/s`.
    Unknown,
}

/// A lot-by-lot acceptance sampling plan.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SamplingPlan {
    /// Inspect `n`, accept with at most `c` nonconforming.
    Single {
        /// Sample size.
        n: u64,
        /// Acceptance number.
        c: u64,
    },
    /// Two-stage attribute plan (reject on the first sample when `d₁ > c₂`).
    Double {
        /// First sample size.
        n1: u64,
        /// Acceptance number for the first sample.
        c1: u64,
        /// Second sample size.
        n2: u64,
        /// Acceptance number for both samples together.
        c2: u64,
    },
    /// k-method variables plan for one specification limit.
    Variables {
        /// Sample size.
        n: u64,
        /// Acceptability constant.
        k: f64,
        /// Known σ or sample SD.
        sigma: SigmaKnowledge,
    },
}

/// The lot and OC model a plan is evaluated with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlanContext {
    /// Distribution of the number nonconforming (attribute plans only).
    pub model: OcModel,
    /// Lot size `N` (for the hypergeometric model, AOQ and ATI).
    pub lot_size: u64,
}

/// The producer's and consumer's risk points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RiskPoints {
    /// Acceptable quality level `p₁` (fraction nonconforming).
    pub aql: f64,
    /// Producer's risk `α`: lots at `p₁` are accepted with probability at least `1 − α`.
    pub alpha: f64,
    /// Lot tolerance fraction nonconforming `p₂ > p₁`.
    pub ltpd: f64,
    /// Consumer's risk `β`: lots at `p₂` are accepted with probability at most `β`.
    pub beta: f64,
}

impl Default for RiskPoints {
    fn default() -> Self {
        Self { aql: 0.01, alpha: 0.05, ltpd: 0.06, beta: 0.10 }
    }
}

impl RiskPoints {
    /// Check `0 < p₁ < p₂ < 1` and `0 < α, β < 0.5`.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] otherwise.
    pub fn validate(&self) -> Result<()> {
        let open = |v: f64, hi: f64| v > 0.0 && v < hi;
        if !(open(self.aql, 1.0) && open(self.ltpd, 1.0)) {
            return Err(invalid("quality levels must be fractions between 0 and 1"));
        }
        if self.aql >= self.ltpd {
            return Err(invalid("the consumer's quality level must be worse (larger) than the producer's"));
        }
        if !(open(self.alpha, 0.5) && open(self.beta, 0.5)) {
            return Err(invalid("the producer's and consumer's risks must be between 0 and 0.5"));
        }
        Ok(())
    }
}

/// Measures of a plan at one quality level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlanPoint {
    /// Fraction nonconforming.
    pub p: f64,
    /// Probability of acceptance.
    pub pa: f64,
    /// Average outgoing quality.
    pub aoq: f64,
    /// Average total inspection.
    pub ati: f64,
    /// Average sample number.
    pub asn: f64,
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Probability that the number nonconforming in `n` items is at most `c`.
/// `lot` is `(N, D)` for the hypergeometric model.
fn count_cdf(c: u64, n: u64, p: f64, model: OcModel, lot: (u64, u64)) -> Result<f64> {
    match model {
        OcModel::Binomial => binomial_cdf(c, n, p),
        OcModel::Poisson => poisson_cdf(c, n as f64 * p),
        OcModel::Hypergeometric => {
            let (total, bad) = lot;
            let mut s = 0.0;
            for d in 0..=c.min(n).min(bad) {
                s += hypergeometric_pmf(d, total, bad, n)?;
            }
            Ok(s.min(1.0))
        }
    }
}

fn count_pmf(d: u64, n: u64, p: f64, model: OcModel, lot: (u64, u64)) -> Result<f64> {
    match model {
        OcModel::Binomial => binomial_pmf(d, n, p),
        OcModel::Poisson => poisson_pmf(d, n as f64 * p),
        OcModel::Hypergeometric => hypergeometric_pmf(d, lot.0, lot.1, n),
    }
}

/// `z_p = Φ⁻¹(1 − p)`.
fn z_upper(p: f64) -> Result<f64> {
    Normal::standard().quantile(1.0 - p)
}

impl SamplingPlan {
    /// Short description, e.g. `"n = 89, c = 2"`.
    #[must_use]
    pub fn describe(&self) -> String {
        match *self {
            Self::Single { n, c } => format!("n = {n}, c = {c}"),
            Self::Double { n1, c1, n2, c2 } => format!("n₁ = {n1}, c₁ = {c1}, n₂ = {n2}, c₂ = {c2}"),
            Self::Variables { n, k, sigma } => {
                let s = match sigma {
                    SigmaKnowledge::Known => "σ known",
                    SigmaKnowledge::Unknown => "σ unknown",
                };
                format!("n = {n}, k = {k:.4} ({s})")
            }
        }
    }

    /// Kind of plan, e.g. `"Single"`.
    #[must_use]
    pub fn kind_label(&self) -> &'static str {
        match self {
            Self::Single { .. } => "Single attribute",
            Self::Double { .. } => "Double attribute",
            Self::Variables { .. } => "Variables (k-method)",
        }
    }

    /// Largest number of items the plan inspects before deciding.
    #[must_use]
    pub fn max_sample(&self) -> u64 {
        match *self {
            Self::Single { n, .. } | Self::Variables { n, .. } => n,
            Self::Double { n1, n2, .. } => n1 + n2,
        }
    }

    /// Check the plan against the lot.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for an empty sample, an acceptance
    /// number not below the sample size, `c₁ ≥ c₂`, a non-finite `k`, fewer
    /// than two items with σ unknown, or more items than the lot holds.
    pub fn validate(&self, ctx: &PlanContext) -> Result<()> {
        match *self {
            Self::Single { n, c } => {
                if n == 0 || c >= n {
                    return Err(invalid(format!("plan {}: need n ≥ 1 and c < n", self.describe())));
                }
            }
            Self::Double { n1, c1, n2, c2 } => {
                if n1 == 0 || n2 == 0 || c1 >= c2 || c2 >= n1 + n2 {
                    return Err(invalid(format!("plan {}: need n₁, n₂ ≥ 1 and c₁ < c₂ < n₁ + n₂", self.describe())));
                }
            }
            Self::Variables { n, k, sigma } => {
                let min = if sigma == SigmaKnowledge::Unknown { 2 } else { 1 };
                if n < min || !k.is_finite() {
                    return Err(invalid(format!("plan {}: need a finite k and n ≥ {min}", self.describe())));
                }
            }
        }
        if ctx.lot_size < self.max_sample() {
            return Err(invalid(format!(
                "plan {} inspects more items than the lot of {} holds",
                self.describe(),
                ctx.lot_size
            )));
        }
        Ok(())
    }

    /// Probabilities of accepting on the first and on the second sample
    /// (the second is zero for single and variables plans).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for `p` outside `[0, 1]` or an invalid
    /// plan; numerical failures.
    pub fn acceptance_parts(&self, p: f64, ctx: &PlanContext) -> Result<(f64, f64)> {
        if !(0.0..=1.0).contains(&p) {
            return Err(invalid("the fraction nonconforming must be between 0 and 1"));
        }
        self.validate(ctx)?;
        let total = ctx.lot_size;
        let bad = ((p * total as f64).round() as u64).min(total);
        let model = ctx.model;
        match *self {
            Self::Single { n, c } => Ok((count_cdf(c, n, p, model, (total, bad))?, 0.0)),
            Self::Double { n1, c1, n2, c2 } => {
                let first = count_cdf(c1, n1, p, model, (total, bad))?;
                let mut second = 0.0;
                for j in c1 + 1..=c2 {
                    let pj = count_pmf(j, n1, p, model, (total, bad))?;
                    if pj == 0.0 {
                        continue;
                    }
                    let rest = (total - n1, bad.saturating_sub(j));
                    second += pj * count_cdf(c2 - j, n2, p, model, rest)?;
                }
                Ok((first, second.min(1.0 - first).max(0.0)))
            }
            Self::Variables { n, k, sigma } => {
                if p == 0.0 {
                    return Ok((1.0, 0.0));
                }
                if p == 1.0 {
                    return Ok((0.0, 0.0));
                }
                let z = z_upper(p)?;
                let root = (n as f64).sqrt();
                let pa = match sigma {
                    SigmaKnowledge::Known => Normal::standard().cdf(root * (z - k))?,
                    SigmaKnowledge::Unknown => NoncentralT::new((n - 1) as f64, root * z)?.sf(k * root)?,
                };
                Ok((pa.clamp(0.0, 1.0), 0.0))
            }
        }
    }

    /// Probability of accepting a lot with fraction nonconforming `p`.
    ///
    /// # Errors
    /// As [`acceptance_parts`](Self::acceptance_parts).
    pub fn acceptance_probability(&self, p: f64, ctx: &PlanContext) -> Result<f64> {
        let (a, b) = self.acceptance_parts(p, ctx)?;
        Ok((a + b).min(1.0))
    }

    /// Acceptance probability, AOQ, ATI and ASN at `p`.
    ///
    /// # Errors
    /// As [`acceptance_parts`](Self::acceptance_parts).
    pub fn evaluate(&self, p: f64, ctx: &PlanContext) -> Result<PlanPoint> {
        let (first, second) = self.acceptance_parts(p, ctx)?;
        let pa = (first + second).min(1.0);
        let lot = ctx.lot_size as f64;
        let (aoq, ati, asn) = match *self {
            Self::Single { n, .. } | Self::Variables { n, .. } => {
                let n = n as f64;
                (p * pa * (lot - n) / lot, n + (1.0 - pa) * (lot - n), n)
            }
            Self::Double { n1, c1, n2, c2 } => {
                let (n1f, n2f) = (n1 as f64, n2 as f64);
                let aoq = p * (first * (lot - n1f) + second * (lot - n1f - n2f)) / lot;
                let ati = n1f * first + (n1f + n2f) * second + lot * (1.0 - pa);
                // Second sample needed when c₁ < d₁ ≤ c₂.
                let total = ctx.lot_size;
                let bad = ((p * lot).round() as u64).min(total);
                let upto = |c| count_cdf(c, n1, p, ctx.model, (total, bad));
                let needed = (upto(c2)? - upto(c1)?).max(0.0);
                (aoq, ati, n1f + n2f * needed)
            }
        };
        Ok(PlanPoint { p, pa, aoq, ati, asn })
    }

    /// The average outgoing quality limit: `(max AOQ, p at the maximum)`.
    ///
    /// For the hypergeometric model every whole number of nonconforming items
    /// in the lot is tried; otherwise a grid over `p` is refined by golden-
    /// section search.
    ///
    /// # Errors
    /// As [`evaluate`](Self::evaluate).
    pub fn aoql(&self, ctx: &PlanContext) -> Result<(f64, f64)> {
        let aoq = |p: f64| self.evaluate(p, ctx).map(|e| e.aoq);
        let lot = ctx.lot_size as f64;
        if ctx.model == OcModel::Hypergeometric && !matches!(self, Self::Variables { .. }) {
            let mut best = (0.0, 0.0);
            for d in 1..=ctx.lot_size {
                let p = d as f64 / lot;
                let e = self.evaluate(p, ctx)?;
                if e.aoq > best.0 {
                    best = (e.aoq, p);
                }
                if e.pa < 1e-9 {
                    break;
                }
            }
            return Ok(best);
        }
        // Upper end of the search: where acceptance becomes negligible.
        let mut hi = 1e-4;
        while hi < 1.0 && self.acceptance_probability(hi, ctx)? > 1e-6 {
            hi = (hi * 2.0).min(1.0);
        }
        const GRID: usize = 400;
        let (mut best_i, mut best) = (1, f64::NEG_INFINITY);
        for i in 1..=GRID {
            let v = aoq(hi * i as f64 / GRID as f64)?;
            if v > best {
                best = v;
                best_i = i;
            }
        }
        let step = hi / GRID as f64;
        let (mut a, mut b) = (step * (best_i as f64 - 1.0), (step * (best_i as f64 + 1.0)).min(hi));
        let g = 0.5 * (5f64.sqrt() - 1.0);
        let mut x1 = b - g * (b - a);
        let mut x2 = a + g * (b - a);
        let (mut f1, mut f2) = (aoq(x1)?, aoq(x2)?);
        for _ in 0..80 {
            if f1 < f2 {
                a = x1;
                x1 = x2;
                f1 = f2;
                x2 = a + g * (b - a);
                f2 = aoq(x2)?;
            } else {
                b = x2;
                x2 = x1;
                f2 = f1;
                x1 = b - g * (b - a);
                f1 = aoq(x1)?;
            }
        }
        let p = 0.5 * (a + b);
        let v = aoq(p)?;
        Ok(if v >= best { (v, p) } else { (best, step * best_i as f64) })
    }
}

/// Smallest `n ≥ lo` (up to `hi`) with `accept(n) ≤ beta`, assuming
/// `accept` decreases with `n`; `None` if even `hi` does not reach it.
fn smallest_n(lo: u64, hi: u64, beta: f64, accept: &dyn Fn(u64) -> Result<f64>) -> Result<Option<u64>> {
    if lo > hi {
        return Ok(None);
    }
    if accept(lo)? <= beta {
        return Ok(Some(lo));
    }
    // Grow an upper bracket, then bisect.
    let mut bad = lo;
    let mut step = 1u64;
    let good = loop {
        let candidate = bad.saturating_add(step).min(hi);
        if accept(candidate)? <= beta {
            break candidate;
        }
        if candidate == hi {
            return Ok(None);
        }
        bad = candidate;
        step = step.saturating_mul(2);
    };
    let (mut bad, mut good) = (bad, good);
    while good - bad > 1 {
        let mid = bad + (good - bad) / 2;
        if accept(mid)? <= beta {
            good = mid;
        } else {
            bad = mid;
        }
    }
    Ok(Some(good))
}

fn check_lot(risks: &RiskPoints, ctx: &PlanContext) -> Result<()> {
    risks.validate()?;
    if ctx.lot_size < 2 {
        return Err(invalid("the lot must hold at least two items"));
    }
    if ctx.model == OcModel::Hypergeometric {
        let d1 = (risks.aql * ctx.lot_size as f64).round();
        let d2 = (risks.ltpd * ctx.lot_size as f64).round();
        if d2 < 1.0 || d1 >= d2 {
            return Err(invalid(
                "with the hypergeometric model the lot is too small to tell the two quality levels apart \
                 (round(pN) must differ and the consumer's level must give at least one nonconforming item)",
            ));
        }
    }
    Ok(())
}

fn search_cap(ctx: &PlanContext) -> u64 {
    if ctx.model == OcModel::Hypergeometric { ctx.lot_size } else { MAX_SAMPLE_SIZE.min(ctx.lot_size) }
}

/// The single attribute plan with the smallest sample size meeting both risk
/// points (see the module docs).
///
/// # Errors
/// [`StatError::InvalidArgument`] for invalid risk points or lot, or when no
/// plan with `c ≤` [`MAX_ACCEPTANCE_NUMBER`] fits in the lot.
pub fn single_plan(risks: &RiskPoints, ctx: &PlanContext) -> Result<SamplingPlan> {
    check_lot(risks, ctx)?;
    let cap = search_cap(ctx);
    let mut lo = 1;
    for c in 0..=MAX_ACCEPTANCE_NUMBER {
        let accept = |n: u64| SamplingPlan::Single { n, c }.acceptance_probability(risks.ltpd, ctx);
        let Some(n) = smallest_n(lo.max(c + 1), cap, risks.beta, &accept)? else {
            break;
        };
        lo = n;
        let plan = SamplingPlan::Single { n, c };
        if plan.acceptance_probability(risks.aql, ctx)? >= 1.0 - risks.alpha {
            return Ok(plan);
        }
    }
    Err(invalid(format!(
        "no single plan with c ≤ {MAX_ACCEPTANCE_NUMBER} and n ≤ {cap} meets both risk points; \
         move the quality levels further apart or allow larger risks"
    )))
}

/// The double attribute plan with `n₂ = ratio · n₁` meeting both risk points
/// with the smallest average sample number at the producer's quality level
/// (see the module docs).
///
/// # Errors
/// [`StatError::InvalidArgument`] for invalid risk points, lot or ratio
/// (1 to 5), or when no plan is found.
pub fn double_plan(risks: &RiskPoints, ctx: &PlanContext, ratio: u64) -> Result<SamplingPlan> {
    check_lot(risks, ctx)?;
    if !(1..=5).contains(&ratio) {
        return Err(invalid("the second sample must be 1 to 5 times the first"));
    }
    // Bound the acceptance numbers by the single plan's.
    let single_c = match single_plan(risks, ctx)? {
        SamplingPlan::Single { c, .. } => c,
        _ => unreachable!("single_plan returns a single plan"),
    };
    let max_c2 = (3 * single_c + 6).min(60);
    let cap = search_cap(ctx) / (1 + ratio);
    let mut best: Option<(f64, SamplingPlan)> = None;
    for c2 in 1..=max_c2 {
        for c1 in 0..c2 {
            let plan = |n1: u64| SamplingPlan::Double { n1, c1, n2: ratio * n1, c2 };
            let accept = |n1: u64| plan(n1).acceptance_probability(risks.ltpd, ctx);
            let lo = (c2 + 1).div_ceil(1 + ratio).max(1);
            let Some(n1) = smallest_n(lo, cap, risks.beta, &accept)? else {
                continue;
            };
            let candidate = plan(n1);
            let at_aql = candidate.evaluate(risks.aql, ctx)?;
            if at_aql.pa < 1.0 - risks.alpha {
                continue;
            }
            if best.as_ref().is_none_or(|(asn, _)| at_aql.asn < *asn - 1e-9) {
                best = Some((at_aql.asn, candidate));
            }
        }
    }
    best.map(|b| b.1).ok_or_else(|| {
        invalid(format!("no double plan with n₂ = {ratio}·n₁ and c₂ ≤ {max_c2} meets both risk points within the lot"))
    })
}

/// The k-method variables plan meeting both risk points (see the module docs).
///
/// # Errors
/// [`StatError::InvalidArgument`] for invalid risk points; numerical failures.
pub fn variables_plan(risks: &RiskPoints, sigma: SigmaKnowledge) -> Result<SamplingPlan> {
    risks.validate()?;
    let normal = Normal::standard();
    let (z1, z2) = (z_upper(risks.aql)?, z_upper(risks.ltpd)?);
    let (za, zb) = (normal.quantile(1.0 - risks.alpha)?, normal.quantile(1.0 - risks.beta)?);
    let n_known = ((za + zb) / (z1 - z2)).powi(2);
    let k_known = (z1 * zb + z2 * za) / (za + zb);
    match sigma {
        SigmaKnowledge::Known => {
            Ok(SamplingPlan::Variables { n: (n_known - 1e-9).ceil().max(1.0) as u64, k: k_known, sigma })
        }
        SigmaKnowledge::Unknown => {
            // Feasible k at sample size n: [k_consumer, k_producer].
            let bounds = |n: u64| -> Result<(f64, f64)> {
                let pa = |k: f64, p: f64| {
                    SamplingPlan::Variables { n, k, sigma }
                        .acceptance_probability(p, &PlanContext { model: OcModel::Binomial, lot_size: u64::MAX })
                };
                // Pa decreases in k; bisect each bound on a bracket around the known-σ k.
                let solve = |p: f64, target: f64| -> Result<f64> {
                    let (mut lo, mut hi) = (k_known - 10.0, k_known + 10.0);
                    for _ in 0..100 {
                        let mid = 0.5 * (lo + hi);
                        if pa(mid, p)? > target {
                            lo = mid;
                        } else {
                            hi = mid;
                        }
                    }
                    Ok(0.5 * (lo + hi))
                };
                Ok((solve(risks.ltpd, risks.beta)?, solve(risks.aql, 1.0 - risks.alpha)?))
            };
            let feasible = |n: u64| bounds(n).map(|(kc, kp)| kc <= kp);
            // The Wallis approximation n(1 + k²/2) is a good starting bracket.
            let wallis = (n_known * (1.0 + k_known * k_known / 2.0)).ceil() as u64;
            let mut hi = wallis.max(3);
            while !feasible(hi)? {
                hi = hi.saturating_mul(2);
                if hi > MAX_SAMPLE_SIZE {
                    return Err(invalid("no variables plan with σ unknown was found"));
                }
            }
            let mut lo = 2;
            if feasible(lo)? {
                hi = lo;
            }
            while hi - lo > 1 {
                let mid = lo + (hi - lo) / 2;
                if feasible(mid)? {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            let (kc, kp) = bounds(hi)?;
            Ok(SamplingPlan::Variables { n: hi, k: 0.5 * (kc + kp), sigma })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BIG: PlanContext = PlanContext { model: OcModel::Binomial, lot_size: 10_000 };

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol, "{a} vs {b}");
    }

    /// Reference values in these tests come from an independent Python
    /// implementation (exact sums of `math.comb` terms; normal quantiles by
    /// bisection of `math.erf`; the noncentral t by numerical integration
    /// over the chi distribution), see `docs/validation.md`.
    #[test]
    fn single_plan_oc_aoq_ati() {
        let plan = SamplingPlan::Single { n: 89, c: 2 };
        close(plan.acceptance_probability(0.01, &BIG).unwrap(), 0.939_689_918_313_558_7, 1e-12);
        close(plan.acceptance_probability(0.06, &BIG).unwrap(), 0.091_869_347_172_184_88, 1e-12);
        let e = plan.evaluate(0.02, &BIG).unwrap();
        close(e.pa, 0.736_577_575_696_670_8, 1e-12);
        close(e.aoq, 0.02 * e.pa * (10_000.0 - 89.0) / 10_000.0, 1e-15);
        close(e.ati, 89.0 + (1.0 - e.pa) * (10_000.0 - 89.0), 1e-9);
        let (aoql, at) = plan.aoql(&BIG).unwrap();
        close(aoql, 0.015_246_342_929_140_917, 1e-9);
        close(at, 0.025_277, 2e-4);
        // Poisson and hypergeometric OC.
        let poisson = PlanContext { model: OcModel::Poisson, ..BIG };
        close(plan.acceptance_probability(0.02, &poisson).unwrap(), 0.735_970_602_309_604_6, 1e-12);
        let lot = PlanContext { model: OcModel::Hypergeometric, lot_size: 500 };
        close(plan.acceptance_probability(0.02, &lot).unwrap(), 0.744_173_683_212_768_2, 1e-12);
    }

    #[test]
    fn single_plan_from_risk_points() {
        let risks = RiskPoints::default();
        assert_eq!(single_plan(&risks, &BIG).unwrap(), SamplingPlan::Single { n: 110, c: 3 });
        let tight = RiskPoints { aql: 0.005, alpha: 0.05, ltpd: 0.02, beta: 0.10 };
        assert_eq!(single_plan(&tight, &BIG).unwrap(), SamplingPlan::Single { n: 462, c: 5 });
        let poisson = PlanContext { model: OcModel::Poisson, ..BIG };
        assert_eq!(single_plan(&risks, &poisson).unwrap(), SamplingPlan::Single { n: 112, c: 3 });
        let lot = PlanContext { model: OcModel::Hypergeometric, lot_size: 500 };
        assert_eq!(single_plan(&risks, &lot).unwrap(), SamplingPlan::Single { n: 83, c: 2 });
        // The plan meets both points and the next smaller sample does not.
        for ctx in [BIG, poisson, lot] {
            let SamplingPlan::Single { n, c } = single_plan(&risks, &ctx).unwrap() else { panic!() };
            let pa = |n, p| SamplingPlan::Single { n, c }.acceptance_probability(p, &ctx).unwrap();
            assert!(pa(n, 0.01) >= 0.95 && pa(n, 0.06) <= 0.10);
            assert!(pa(n - 1, 0.06) > 0.10);
        }
        assert!(single_plan(&RiskPoints { aql: 0.06, ..risks }, &BIG).is_err());
        let tiny = PlanContext { model: OcModel::Hypergeometric, lot_size: 5 };
        assert!(single_plan(&risks, &tiny).is_err());
    }

    #[test]
    fn double_plan_oc_and_search() {
        let plan = SamplingPlan::Double { n1: 50, c1: 1, n2: 100, c2: 3 };
        let (first, second) = plan.acceptance_parts(0.05, &BIG).unwrap();
        close(first, 0.279_431_752_320_695_17, 1e-12);
        close(second, 0.010_983_729_975_978_25, 1e-12);
        let e = plan.evaluate(0.05, &BIG).unwrap();
        close(e.asn, 50.0 + 100.0 * 0.480_976_208_629_403_27, 1e-9);
        let lot = 10_000.0;
        close(e.ati, 50.0 * first + 150.0 * second + lot * (1.0 - first - second), 1e-9);
        close(e.aoq, 0.05 * (first * (lot - 50.0) + second * (lot - 150.0)) / lot, 1e-15);
        let found = double_plan(&RiskPoints::default(), &BIG, 2).unwrap();
        assert_eq!(found, SamplingPlan::Double { n1: 43, c1: 0, n2: 86, c2: 3 });
        let at = found.evaluate(0.01, &BIG).unwrap();
        assert!(at.pa >= 0.95 && found.acceptance_probability(0.06, &BIG).unwrap() <= 0.10);
        assert!(
            (at.asn - 73.099_424_971_340_97).abs() < 1e-6 && at.asn < 110.0,
            "a double plan inspects fewer items on average at the AQL: {}",
            at.asn
        );
        assert!(double_plan(&RiskPoints::default(), &BIG, 9).is_err());
    }

    #[test]
    fn variables_plans() {
        let risks = RiskPoints::default();
        let SamplingPlan::Variables { n, k, .. } = variables_plan(&risks, SigmaKnowledge::Known).unwrap() else {
            panic!()
        };
        assert_eq!(n, 15);
        close(k, 1.892_666_729_932_626_5, 1e-9);
        let known = SamplingPlan::Variables { n: 15, k, sigma: SigmaKnowledge::Known };
        close(known.acceptance_probability(0.01, &BIG).unwrap(), 0.953_486_295_199_233, 1e-9);
        let unknown = SamplingPlan::Variables { n: 40, k: 1.9, sigma: SigmaKnowledge::Unknown };
        close(unknown.acceptance_probability(0.01, &BIG).unwrap(), 0.948_374_625_169_234, 1e-6);
        close(unknown.acceptance_probability(0.06, &BIG).unwrap(), 0.104_594_969_250_763_66, 1e-6);
        let SamplingPlan::Variables { n, k, .. } = variables_plan(&risks, SigmaKnowledge::Unknown).unwrap() else {
            panic!()
        };
        assert_eq!(n, 42);
        close(k, 0.5 * (1.897_562_271_015_561_3 + 1.905_285_051_640_107), 1e-6);
        let pa = |n, k, p| {
            SamplingPlan::Variables { n, k, sigma: SigmaKnowledge::Unknown }.acceptance_probability(p, &BIG).unwrap()
        };
        assert!(pa(n, k, 0.01) >= 0.95 && pa(n, k, 0.06) <= 0.10);
        // At n − 1 no k meets both: the producer's bound is below the consumer's.
        assert!((0..=400).all(|i| {
            let k = 1.5 + f64::from(i) / 1000.0;
            pa(n - 1, k, 0.01) < 0.95 || pa(n - 1, k, 0.06) > 0.10
        }));
        let (aoql, _) = known.aoql(&BIG).unwrap();
        assert!(aoql > 0.0 && aoql < 0.06);
    }

    #[test]
    fn invalid_plans() {
        assert!(SamplingPlan::Single { n: 5, c: 5 }.validate(&BIG).is_err());
        assert!(SamplingPlan::Double { n1: 5, c1: 2, n2: 5, c2: 2 }.validate(&BIG).is_err());
        let small = PlanContext { lot_size: 10, ..BIG };
        assert!(SamplingPlan::Single { n: 20, c: 1 }.validate(&small).is_err());
        assert!(SamplingPlan::Single { n: 5, c: 1 }.acceptance_probability(1.5, &BIG).is_err());
        assert!(RiskPoints { alpha: 0.6, ..RiskPoints::default() }.validate().is_err());
        assert_eq!(OcModel::from_key("poisson"), Some(OcModel::Poisson));
    }
}
