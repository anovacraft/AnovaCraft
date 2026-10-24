//! The Kaplan–Meier product-limit estimator and the log-rank test.
//!
//! # Product-limit estimate
//! Let `t₁ < t₂ < … < t_m` be the distinct observed times, `nⱼ` the number of
//! units **at risk** just before `tⱼ` (observed time `≥ tⱼ`) and `dⱼ` the number
//! of events at `tⱼ`. Kaplan & Meier (1958, *JASA* 53:457) estimate the survival
//! function `S(t) = P(T > t)` by
//!
//! ```text
//! Ŝ(t) = ∏_{tⱼ ≤ t} (1 − dⱼ / nⱼ).
//! ```
//!
//! A unit censored at `tⱼ` counts as at risk at `tⱼ` (censoring is taken to
//! happen just after any events at the same time), so ties between events and
//! censorings are handled in the conventional way.
//!
//! # Greenwood standard error
//! Greenwood's formula (1926) estimates the variance
//!
//! ```text
//! Var[Ŝ(t)] = Ŝ(t)² · Σ_{tⱼ ≤ t} dⱼ / (nⱼ (nⱼ − dⱼ)).
//! ```
//!
//! # Pointwise confidence bands
//! With `σ(t)² = Σ dⱼ/(nⱼ(nⱼ−dⱼ))` and `z` the standard normal quantile
//! ([`BandTransform`]; Kalbfleisch & Prentice, *The Statistical Analysis of
//! Failure Time Data*, 2nd ed., §1.4; Klein & Moeschberger §4.3):
//!
//! * plain: `Ŝ ± z Ŝ σ`, clipped to `[0, 1]`;
//! * log: `Ŝ · exp(± z σ)`, clipped at 1;
//! * log–log: `Ŝ^{exp(± z σ / |ln Ŝ|)}` — always inside `[0, 1]` and the most
//!   accurate in small samples (Borgan & Liestøl 1990).
//!
//! # Quantiles and median survival
//! The `p`-quantile (the time by which a fraction `p` has failed) is the
//! smallest time with `Ŝ(t) ≤ 1 − p`; when `Ŝ` equals `1 − p` exactly over an
//! interval the midpoint of that interval is used. It is undefined (`None`) if
//! `Ŝ` never falls that far. The confidence interval of Brookmeyer & Crowley
//! (1982, *Biometrics* 38:29) is the set of times whose pointwise band contains
//! `1 − p`: its lower limit is the first time the band's lower curve reaches
//! `1 − p`, its upper limit the first time the upper curve does.
//!
//! # Log-rank test
//! For `g` groups, at each distinct event time the observed events of group `k`
//! are compared with their expectation `eₖⱼ = dⱼ nₖⱼ / nⱼ` under equal hazards,
//! with hypergeometric covariance
//! `Vₖₗ = Σⱼ dⱼ (nⱼ − dⱼ)/(nⱼ − 1) · (nₖⱼ/nⱼ)(δₖₗ − nₗⱼ/nⱼ)`. The statistic
//! `(O − E)ᵀ V⁻¹ (O − E)` over the first `g − 1` groups is referred to χ² with
//! `g − 1` degrees of freedom (Mantel 1966; Peto & Peto 1972).

use stat_core::{ChiSquared, ContinuousDistribution, Normal, Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// How pointwise confidence limits for `S(t)` are constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BandTransform {
    /// `Ŝ ± z·SE`, clipped to `[0, 1]`.
    Plain,
    /// Symmetric on the log scale: `Ŝ·exp(±z·SE/Ŝ)`, clipped at 1.
    Log,
    /// Symmetric on the log(−log) scale; limits always lie inside `[0, 1]`.
    #[default]
    LogLog,
}

impl BandTransform {
    /// Short description for reports.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Log => "log",
            Self::LogLog => "log–log",
        }
    }
}

/// The estimate at one distinct observed time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProductLimitStep {
    /// Observed time `tⱼ`.
    pub time: f64,
    /// Units at risk just before `tⱼ`.
    pub at_risk: usize,
    /// Events at `tⱼ`.
    pub events: usize,
    /// Censorings at `tⱼ`.
    pub censored: usize,
    /// `Ŝ(tⱼ)`, the estimate just after `tⱼ`.
    pub survival: f64,
    /// Greenwood sum `Σ_{tᵢ ≤ tⱼ} dᵢ/(nᵢ(nᵢ − dᵢ))` (infinite once `Ŝ = 0`).
    pub greenwood_sum: f64,
}

impl ProductLimitStep {
    /// Greenwood standard error `Ŝ · √(Σ d/(n(n−d)))`; zero once `Ŝ = 0`.
    #[must_use]
    pub fn std_error(&self) -> f64 {
        if self.survival == 0.0 { 0.0 } else { self.survival * self.greenwood_sum.sqrt() }
    }

    /// Pointwise `level` confidence limits for `S(tⱼ)`.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] unless `0 < level < 1`.
    pub fn confidence_limits(&self, level: f64, transform: BandTransform) -> Result<(f64, f64)> {
        let z = normal_quantile(level)?;
        Ok(limits(self.survival, self.greenwood_sum, z, transform))
    }
}

fn normal_quantile(level: f64) -> Result<f64> {
    if !(level > 0.0 && level < 1.0) {
        return Err(invalid(format!("confidence level must lie strictly between 0 and 1 (got {level})")));
    }
    Normal::standard().quantile(0.5 + level / 2.0)
}

fn limits(s: f64, sum: f64, z: f64, transform: BandTransform) -> (f64, f64) {
    if s <= 0.0 {
        return (0.0, 0.0);
    }
    if s >= 1.0 || sum == 0.0 {
        return (s, s);
    }
    let sigma = sum.sqrt();
    match transform {
        BandTransform::Plain => ((s - z * s * sigma).max(0.0), (s + z * s * sigma).min(1.0)),
        BandTransform::Log => (s * (-z * sigma).exp(), (s * (z * sigma).exp()).min(1.0)),
        BandTransform::LogLog => {
            let w = z * sigma / s.ln().abs();
            (s.powf(w.exp()), s.powf((-w).exp()))
        }
    }
}

/// A Kaplan–Meier survival estimate.
#[derive(Debug, Clone, PartialEq)]
pub struct KaplanMeier {
    /// One entry per distinct observed time (event or censoring), increasing.
    pub steps: Vec<ProductLimitStep>,
}

impl KaplanMeier {
    /// Number of observations.
    #[must_use]
    pub fn n(&self) -> usize {
        self.steps.first().map_or(0, |s| s.at_risk)
    }

    /// Number of events.
    #[must_use]
    pub fn n_events(&self) -> usize {
        self.steps.iter().map(|s| s.events).sum()
    }

    /// Number of censored observations.
    #[must_use]
    pub fn n_censored(&self) -> usize {
        self.steps.iter().map(|s| s.censored).sum()
    }

    /// Largest observed time.
    #[must_use]
    pub fn last_time(&self) -> f64 {
        self.steps.last().map_or(0.0, |s| s.time)
    }

    /// Number at risk at time `t`: observations whose time is at least `t`
    /// (as R's `summary(survfit, times = t)` reports it).
    #[must_use]
    pub fn at_risk_at(&self, t: f64) -> usize {
        let j = self.steps.partition_point(|s| s.time < t);
        self.steps.get(j).map_or(0, |s| s.at_risk)
    }

    /// Censored observations with a time in `(after, through]` (R's
    /// `n.censor` of `summary(survfit, times)` between consecutive times).
    #[must_use]
    pub fn censored_between(&self, after: f64, through: f64) -> usize {
        self.steps.iter().filter(|s| s.time > after && s.time <= through).map(|s| s.censored).sum()
    }

    /// Index of the last step at or before `t`, if any.
    fn step_at(&self, t: f64) -> Option<usize> {
        self.steps.partition_point(|s| s.time <= t).checked_sub(1)
    }

    /// `Ŝ(t)` (1 before the first observed time). Beyond the last observed
    /// time the estimate is held at its final value.
    #[must_use]
    pub fn survival_at(&self, t: f64) -> f64 {
        self.step_at(t).map_or(1.0, |j| self.steps[j].survival)
    }

    /// Greenwood standard error of `Ŝ(t)`.
    #[must_use]
    pub fn std_error_at(&self, t: f64) -> f64 {
        self.step_at(t).map_or(0.0, |j| self.steps[j].std_error())
    }

    /// Pointwise `level` confidence limits for `S(t)`.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] unless `0 < level < 1`.
    pub fn confidence_limits_at(&self, t: f64, level: f64, transform: BandTransform) -> Result<(f64, f64)> {
        match self.step_at(t) {
            Some(j) => self.steps[j].confidence_limits(level, transform),
            None => normal_quantile(level).map(|_| (1.0, 1.0)),
        }
    }

    /// The time by which a fraction `p` of units has failed: the smallest
    /// observed time with `Ŝ ≤ 1 − p` (midpoint convention for exact ties, see
    /// the [module documentation](self)), or `None` if `Ŝ` stays above `1 − p`.
    ///
    /// # Panics
    /// Never; `p` outside `(0, 1)` returns `None`.
    #[must_use]
    pub fn quantile(&self, p: f64) -> Option<f64> {
        if !(p > 0.0 && p < 1.0) {
            return None;
        }
        let target = 1.0 - p;
        let tol = 1e-12;
        let j = self.steps.iter().position(|s| s.events > 0 && s.survival <= target + tol)?;
        let step = &self.steps[j];
        if (step.survival - target).abs() <= tol {
            // Flat at exactly 1 − p: midpoint up to the next event time, if any.
            if let Some(next) = self.steps[j + 1..].iter().find(|s| s.events > 0) {
                return Some(0.5 * (step.time + next.time));
            }
        }
        Some(step.time)
    }

    /// Median survival time, [`quantile`](Self::quantile)`(0.5)`.
    #[must_use]
    pub fn median(&self) -> Option<f64> {
        self.quantile(0.5)
    }

    /// Brookmeyer–Crowley `level` confidence limits for the `p`-quantile.
    /// Either limit is `None` when the band never reaches `1 − p` (the upper
    /// limit is then unbounded within the follow-up).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] unless `0 < p < 1` and `0 < level < 1`.
    pub fn quantile_confidence_interval(
        &self,
        p: f64,
        level: f64,
        transform: BandTransform,
    ) -> Result<(Option<f64>, Option<f64>)> {
        if !(p > 0.0 && p < 1.0) {
            return Err(invalid(format!("quantile probability must lie strictly between 0 and 1 (got {p})")));
        }
        let z = normal_quantile(level)?;
        let target = 1.0 - p + 1e-12;
        let (mut lower, mut upper) = (None, None);
        for s in self.steps.iter().filter(|s| s.events > 0) {
            let (lo, hi) = limits(s.survival, s.greenwood_sum, z, transform);
            if lower.is_none() && lo <= target {
                lower = Some(s.time);
            }
            if upper.is_none() && hi <= target {
                upper = Some(s.time);
                break;
            }
        }
        Ok((lower, upper))
    }

    /// Vertices of the step function `Ŝ(t)` from `(start, 1)` to the last
    /// observed time, for plotting: each event time contributes a vertical drop.
    #[must_use]
    pub fn step_polyline(&self, start: f64) -> Vec<[f64; 2]> {
        let mut out = vec![[start, 1.0]];
        let mut s = 1.0;
        for step in &self.steps {
            if step.events > 0 {
                out.push([step.time, s]);
                s = step.survival;
                out.push([step.time, s]);
            }
        }
        out.push([self.last_time().max(start), s]);
        out
    }

    /// Pointwise confidence band as constant pieces
    /// `[t_start, t_end, lower, upper]`, one per interval between consecutive
    /// event times (the last piece ends at the last observed time). The piece
    /// before the first event, where the band is the single value 1, is omitted.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] unless `0 < level < 1`.
    pub fn band_pieces(&self, level: f64, transform: BandTransform) -> Result<Vec<[f64; 4]>> {
        let z = normal_quantile(level)?;
        let events: Vec<&ProductLimitStep> = self.steps.iter().filter(|s| s.events > 0).collect();
        let last = self.last_time();
        Ok(events
            .iter()
            .enumerate()
            .map(|(k, s)| {
                let end = events.get(k + 1).map_or(last, |n| n.time);
                let (lo, hi) = limits(s.survival, s.greenwood_sum, z, transform);
                [s.time, end, lo, hi]
            })
            .collect())
    }
}

fn check_input(times: &[f64], events: &[bool]) -> Result<()> {
    if times.len() != events.len() {
        return Err(StatError::DimensionMismatch {
            context: "survival times and event indicators",
            expected: times.len(),
            found: events.len(),
        });
    }
    if times.is_empty() {
        return Err(StatError::InsufficientData { required: 1, found: 0 });
    }
    if times.iter().any(|t| !t.is_finite() || *t < 0.0) {
        return Err(invalid("survival times must be finite and non-negative"));
    }
    Ok(())
}

/// Kaplan–Meier estimate from observed `times` and event indicators
/// (`events[i]` is `true` for an event, `false` for a right-censored time).
///
/// # Errors
/// [`StatError::DimensionMismatch`] if the slices differ in length,
/// [`StatError::InsufficientData`] if they are empty, and
/// [`StatError::InvalidArgument`] for negative or non-finite times.
pub fn kaplan_meier(times: &[f64], events: &[bool]) -> Result<KaplanMeier> {
    check_input(times, events)?;
    let mut order: Vec<usize> = (0..times.len()).collect();
    order.sort_by(|&a, &b| times[a].total_cmp(&times[b]));
    let mut steps = Vec::new();
    let (mut s, mut sum) = (1.0, 0.0);
    let mut at_risk = times.len();
    let mut i = 0;
    while i < order.len() {
        let t = times[order[i]];
        let (mut d, mut c) = (0, 0);
        while i < order.len() && times[order[i]] == t {
            if events[order[i]] {
                d += 1
            } else {
                c += 1
            }
            i += 1;
        }
        if d > 0 {
            s *= 1.0 - d as f64 / at_risk as f64;
            sum += if d == at_risk { f64::INFINITY } else { d as f64 / (at_risk as f64 * (at_risk - d) as f64) };
        }
        steps.push(ProductLimitStep { time: t, at_risk, events: d, censored: c, survival: s, greenwood_sum: sum });
        at_risk -= d + c;
    }
    Ok(KaplanMeier { steps })
}

/// Result of a log-rank test.
#[derive(Debug, Clone, PartialEq)]
pub struct LogRankTest {
    /// Observed events per group.
    pub observed: Vec<f64>,
    /// Expected events per group under equal hazards.
    pub expected: Vec<f64>,
    /// χ² statistic.
    pub chi_square: f64,
    /// Degrees of freedom (`groups − 1`).
    pub df: usize,
    /// Upper-tail p-value.
    pub p_value: f64,
    /// Covariance matrix of the observed-minus-expected counts under the
    /// null hypothesis (`groups × groups`, rank `groups − 1`).
    pub covariance: Vec<Vec<f64>>,
}

/// Log-rank test for trend across ordered groups.
#[derive(Debug, Clone, PartialEq)]
pub struct LogRankTrend {
    /// Group scores used (one per group).
    pub scores: Vec<f64>,
    /// `U = Σ w_g (O_g − E_g)`: positive when groups with higher scores have
    /// more events than expected.
    pub statistic: f64,
    /// `Var U = wᵀ V w`.
    pub variance: f64,
    /// `U² / Var U`, χ² on one degree of freedom.
    pub chi_square: f64,
    /// Upper-tail p-value.
    pub p_value: f64,
}

impl LogRankTest {
    /// Log-rank test for trend (Peto et al. 1977, *Br. J. Cancer* 35, §
    /// "test for trend"; Collett, *Modelling Survival Data in Medical
    /// Research*, 3rd ed., §2.6.4): with group scores `w` (e.g. 1, 2, 3 for
    /// ordered doses), `U = Σ w_g (O_g − E_g)` and `χ² = U² / (wᵀ V w)` on one
    /// degree of freedom, where `V` is the log-rank covariance matrix. It
    /// has more power than the overall test when hazards change steadily
    /// with the score.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for a score count different from the
    /// number of groups, non-finite scores, or scores that are all equal
    /// (zero variance).
    pub fn trend(&self, scores: &[f64]) -> Result<LogRankTrend> {
        let k = self.observed.len();
        if scores.len() != k || scores.iter().any(|s| !s.is_finite()) {
            return Err(invalid("one finite score per group"));
        }
        let u: f64 = (0..k).map(|g| scores[g] * (self.observed[g] - self.expected[g])).sum();
        let var: f64 = (0..k).map(|a| (0..k).map(|b| scores[a] * self.covariance[a][b] * scores[b]).sum::<f64>()).sum();
        // Equal scores give zero variance up to rounding.
        let scale = scores.iter().map(|s| s * s).fold(0.0, f64::max)
            * (0..k).map(|a| self.covariance[a][a]).fold(0.0, f64::max);
        if var.is_nan() || var <= 1e-10 * scale {
            return Err(invalid("the trend test needs scores that differ between groups with events"));
        }
        let chi = u * u / var;
        Ok(LogRankTrend {
            scores: scores.to_vec(),
            statistic: u,
            variance: var,
            chi_square: chi,
            p_value: ChiSquared::new(1.0)?.sf(chi)?,
        })
    }
}

/// Log-rank test that `n_groups` groups share one survival function;
/// `groups[i] < n_groups` is observation `i`'s group.
///
/// # Errors
/// As [`kaplan_meier`], plus [`StatError::InvalidArgument`] for fewer than two
/// groups, an out-of-range group number, a group without observations, no
/// events at all, or a singular covariance matrix.
pub fn log_rank_test(times: &[f64], events: &[bool], groups: &[usize], n_groups: usize) -> Result<LogRankTest> {
    check_input(times, events)?;
    if groups.len() != times.len() {
        return Err(StatError::DimensionMismatch {
            context: "survival times and group numbers",
            expected: times.len(),
            found: groups.len(),
        });
    }
    if n_groups < 2 {
        return Err(invalid("the log-rank test needs at least two groups"));
    }
    if groups.iter().any(|&g| g >= n_groups) {
        return Err(invalid("group number out of range"));
    }
    let mut at_risk = vec![0usize; n_groups];
    for &g in groups {
        at_risk[g] += 1;
    }
    if at_risk.contains(&0) {
        return Err(invalid("every group needs at least one observation"));
    }
    let mut order: Vec<usize> = (0..times.len()).collect();
    order.sort_by(|&a, &b| times[a].total_cmp(&times[b]));
    let k = n_groups;
    let mut observed = vec![0.0; k];
    let mut expected = vec![0.0; k];
    let mut v = vec![vec![0.0; k]; k];
    let mut i = 0;
    while i < order.len() {
        let t = times[order[i]];
        let start = i;
        let mut d_g = vec![0usize; k];
        while i < order.len() && times[order[i]] == t {
            if events[order[i]] {
                d_g[groups[order[i]]] += 1;
            }
            i += 1;
        }
        let d: usize = d_g.iter().sum();
        let n: usize = at_risk.iter().sum();
        if d > 0 {
            let (nf, df) = (n as f64, d as f64);
            let factor = if n > 1 { df * (nf - df) / (nf - 1.0) } else { 0.0 };
            for a in 0..k {
                let pa = at_risk[a] as f64 / nf;
                observed[a] += d_g[a] as f64;
                expected[a] += df * pa;
                for b in 0..k {
                    let pb = at_risk[b] as f64 / nf;
                    v[a][b] += factor * pa * (if a == b { 1.0 } else { 0.0 } - pb);
                }
            }
        }
        for &r in &order[start..i] {
            at_risk[groups[r]] -= 1;
        }
    }
    if observed.iter().sum::<f64>() == 0.0 {
        return Err(invalid("the log-rank test needs at least one event"));
    }
    // Solve the (k−1)-dimensional system by Cholesky factorization.
    let m = k - 1;
    let diff: Vec<f64> = (0..m).map(|a| observed[a] - expected[a]).collect();
    let mut l = vec![vec![0.0; m]; m];
    let scale = (0..m).map(|a| v[a][a]).fold(0.0f64, f64::max);
    for a in 0..m {
        for b in 0..=a {
            let s: f64 = v[a][b] - (0..b).map(|c| l[a][c] * l[b][c]).sum::<f64>();
            if a == b {
                if s.is_nan() || s <= 1e-12 * scale.max(f64::MIN_POSITIVE) {
                    return Err(invalid("log-rank covariance is singular (a group has no units at risk at any event)"));
                }
                l[a][a] = s.sqrt();
            } else {
                l[a][b] = s / l[b][b];
            }
        }
    }
    let mut y = vec![0.0; m];
    for a in 0..m {
        y[a] = (diff[a] - (0..a).map(|c| l[a][c] * y[c]).sum::<f64>()) / l[a][a];
    }
    let chi_square: f64 = y.iter().map(|x| x * x).sum();
    let p_value = ChiSquared::new(m as f64)?.sf(chi_square)?;
    Ok(LogRankTest { observed, expected, chi_square, df: m, p_value, covariance: v })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Freireich et al. (1963): weeks in remission, 6-MP arm (`false` = censored).
    pub(crate) fn six_mp() -> (Vec<f64>, Vec<bool>) {
        let data = [
            (6, 1),
            (6, 1),
            (6, 1),
            (6, 0),
            (7, 1),
            (9, 0),
            (10, 1),
            (10, 0),
            (11, 0),
            (13, 1),
            (16, 1),
            (17, 0),
            (19, 0),
            (20, 0),
            (22, 1),
            (23, 1),
            (25, 0),
            (32, 0),
            (32, 0),
            (34, 0),
            (35, 0),
        ];
        data.iter().map(|&(t, e)| (f64::from(t), e == 1)).unzip()
    }

    /// Placebo arm: every patient relapsed.
    pub(crate) fn placebo() -> Vec<f64> {
        [1, 1, 2, 2, 3, 4, 4, 5, 5, 8, 8, 8, 8, 11, 11, 12, 12, 15, 17, 22, 23].map(f64::from).to_vec()
    }

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol * b.abs().max(1.0), "{a} vs {b}");
    }

    /// Published product-limit table (Klein & Moeschberger §4.2, Table 4.1A):
    /// Ŝ = 0.857, 0.807, 0.753, 0.690, 0.627, 0.538, 0.448 with Greenwood SEs
    /// 0.076, 0.087, 0.096, 0.107, 0.114, 0.128, 0.135 — here checked to full
    /// precision against the exact rational products.
    #[test]
    fn six_mp_product_limit_table() {
        let (t, e) = six_mp();
        let km = kaplan_meier(&t, &e).unwrap();
        assert_eq!((km.n(), km.n_events(), km.n_censored()), (21, 9, 12));
        let expected = [
            (6.0, 21, 3, 1, 18.0 / 21.0, 0.076_360_354_832_121_25),
            (7.0, 17, 1, 0, 0.806_722_689_075_630_3, 0.086_935_285_180_057_2),
            (10.0, 15, 1, 1, 0.752_941_176_470_588_2, 0.096_349_652_994_320_51),
            (13.0, 12, 1, 0, 0.690_196_078_431_372_5, 0.106_814_707_775_009_83),
            (16.0, 11, 1, 0, 0.627_450_980_392_156_9, 0.114_053_865_256_752_54),
            (22.0, 7, 1, 0, 0.537_815_126_050_420_1, 0.128_233_751_693_034),
            (23.0, 6, 1, 0, 0.448_179_271_708_683_5, 0.134_591_456_755_760_45),
        ];
        for (time, n, d, c, s, se) in expected {
            let step = km.steps.iter().find(|x| x.time == time).unwrap();
            assert_eq!((step.at_risk, step.events, step.censored), (n, d, c), "t = {time}");
            close(step.survival, s, 1e-14);
            close(step.std_error(), se, 1e-13);
        }
        // Between and beyond event times the estimate is flat.
        close(km.survival_at(5.9), 1.0, 0.0);
        close(km.survival_at(9.0), 0.806_722_689_075_630_3, 1e-14);
        close(km.survival_at(100.0), 0.448_179_271_708_683_5, 1e-14);
        close(km.std_error_at(20.0), 0.114_053_865_256_752_54, 1e-13);
    }

    #[test]
    fn median_survival_times() {
        let (t, e) = six_mp();
        let km = kaplan_meier(&t, &e).unwrap();
        assert_eq!(km.median(), Some(23.0), "published median remission, 6-MP arm: 23 weeks");
        assert_eq!(km.quantile(0.25), Some(13.0));
        assert_eq!(km.quantile(0.75), None, "Ŝ never falls to 0.25");
        let p = placebo();
        let km = kaplan_meier(&p, &vec![true; p.len()]).unwrap();
        assert_eq!(km.median(), Some(8.0), "published median, placebo arm: 8 weeks");
        // Without censoring Ŝ is the empirical survival function.
        close(km.survival_at(8.0), 8.0 / 21.0, 1e-15);
        // Exact tie at one half: midpoint of the flat stretch.
        let km = kaplan_meier(&[1.0, 2.0, 3.0, 4.0], &[true; 4]).unwrap();
        assert_eq!(km.median(), Some(2.5));
    }

    /// Pointwise 95% limits at t = 6 under the three transforms, against the
    /// formulas evaluated independently (log–log: 0.6197, 0.9516; log: 0.7198, 1).
    #[test]
    fn confidence_bands_and_median_interval() {
        let (t, e) = six_mp();
        let km = kaplan_meier(&t, &e).unwrap();
        let ll = km.confidence_limits_at(6.0, 0.95, BandTransform::LogLog).unwrap();
        close(ll.0, 0.619_717_955_265_983_8, 1e-9);
        close(ll.1, 0.951_551_747_646_997, 1e-9);
        let lg = km.confidence_limits_at(6.0, 0.95, BandTransform::Log).unwrap();
        close(lg.0, 0.719_817_083_916_270_4, 1e-9);
        assert_eq!(lg.1, 1.0);
        let pl = km.confidence_limits_at(23.0, 0.95, BandTransform::Plain).unwrap();
        close(pl.0, 0.184_384_863_840_612_87, 1e-9);
        close(pl.1, 0.711_973_679_576_754_2, 1e-9);
        let ll23 = km.confidence_limits_at(23.0, 0.95, BandTransform::LogLog).unwrap();
        close(ll23.0, 0.188_052_005_959_232_44, 1e-9);
        close(ll23.1, 0.680_142_628_495_241_5, 1e-9);
        assert_eq!(km.confidence_limits_at(1.0, 0.95, BandTransform::LogLog).unwrap(), (1.0, 1.0));
        // Brookmeyer–Crowley interval for the median: [13, unbounded).
        let ci = km.quantile_confidence_interval(0.5, 0.95, BandTransform::LogLog).unwrap();
        assert_eq!(ci, (Some(13.0), None));
        let band = km.band_pieces(0.95, BandTransform::LogLog).unwrap();
        assert_eq!(band.len(), 7);
        assert_eq!(band[0][..2], [6.0, 7.0]);
        assert_eq!(band[6][..2], [23.0, 35.0]);
        assert!(km.confidence_limits_at(6.0, 1.0, BandTransform::Plain).is_err());
    }

    #[test]
    fn step_polyline_drops_at_event_times() {
        let km = kaplan_meier(&[2.0, 3.0, 3.0, 5.0], &[true, true, false, false]).unwrap();
        let line = km.step_polyline(0.0);
        assert_eq!(line, vec![[0.0, 1.0], [2.0, 1.0], [2.0, 0.75], [3.0, 0.75], [3.0, 0.5], [5.0, 0.5]]);
    }

    #[test]
    fn everyone_fails_gives_zero_survival_and_finite_band() {
        let km = kaplan_meier(&[1.0, 2.0], &[true, true]).unwrap();
        assert_eq!(km.survival_at(2.0), 0.0);
        assert_eq!(km.std_error_at(2.0), 0.0);
        assert_eq!(km.confidence_limits_at(2.0, 0.95, BandTransform::LogLog).unwrap(), (0.0, 0.0));
    }

    /// Log-rank comparison of the two arms: χ² = 16.79 on 1 df (published,
    /// e.g. Kalbfleisch & Prentice; Cox 1972), with 9 observed against 19.25
    /// expected relapses in the 6-MP arm.
    #[test]
    fn freireich_log_rank() {
        let (mut t, mut e) = six_mp();
        let mut g = vec![0; t.len()];
        let p = placebo();
        g.extend(std::iter::repeat_n(1, p.len()));
        e.extend(std::iter::repeat_n(true, p.len()));
        t.extend(p);
        let test = log_rank_test(&t, &e, &g, 2).unwrap();
        close(test.chi_square, 16.792_940_989_216_54, 1e-12);
        close(test.observed[0], 9.0, 0.0);
        close(test.expected[0], 19.250_500_948_031_13, 1e-12);
        close(test.observed[0] + test.observed[1], test.expected[0] + test.expected[1], 1e-12);
        assert_eq!(test.df, 1);
        close(test.p_value, 4.168_809_109_334_532e-5, 1e-8);
        // Three groups: the statistic is invariant to which group is dropped.
        let g3: Vec<usize> = (0..t.len())
            .map(|i| {
                if i < 21 {
                    0
                } else if i % 2 == 0 {
                    1
                } else {
                    2
                }
            })
            .collect();
        let a = log_rank_test(&t, &e, &g3, 3).unwrap();
        let swapped: Vec<usize> = g3.iter().map(|&x| 2 - x).collect();
        let b = log_rank_test(&t, &e, &swapped, 3).unwrap();
        close(a.chi_square, b.chi_square, 1e-10);
        assert_eq!(a.df, 2);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(kaplan_meier(&[], &[]).is_err());
        assert!(kaplan_meier(&[1.0], &[true, false]).is_err());
        assert!(kaplan_meier(&[-1.0], &[true]).is_err());
        assert!(kaplan_meier(&[f64::NAN], &[true]).is_err());
        assert!(log_rank_test(&[1.0, 2.0], &[true, true], &[0, 0], 2).is_err(), "empty group");
        assert!(log_rank_test(&[1.0, 2.0], &[false, false], &[0, 1], 2).is_err(), "no events");
        assert!(log_rank_test(&[1.0, 2.0], &[true, true], &[0, 1], 1).is_err());
    }

    /// Three ordered groups simulated in R (`tests/data/log_rank_trend_reference.R`):
    /// `survdiff` observed, expected and variance, the trend statistic from
    /// them, and `summary(survfit, times)` numbers at risk and censored.
    #[test]
    fn log_rank_trend_and_number_at_risk() {
        let time = [
            4., 33., 14., 28., 15., 5., 15., 31., 2., 5., 9., 17., 21., 9., 16., 9., 6., 2., 4., 11., 12., 15., 14.,
            9., 11., 2., 6., 19., 7., 3., 5., 5., 19., 8., 30., 6.,
        ];
        let status = [
            1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0, 1, 0, 1, 0, 0, 1,
        ];
        let dose: Vec<usize> = (0..36).map(|i| usize::from(i >= 12) + usize::from(i >= 25)).collect();
        let events: Vec<bool> = status.iter().map(|&s| s == 1).collect();
        let lr = log_rank_test(&time, &events, &dose, 3).unwrap();
        let close = |a: f64, b: f64| assert!((a - b).abs() <= 1e-11 * b.abs().max(1.0), "{a} vs {b}");
        close(lr.chi_square, 0.493_729_429_409_808);
        close(lr.expected[0], 11.187_321_787_689_43);
        close(lr.covariance[0][0], 5.126_159_999_831_98);
        close(lr.covariance[1][2], -1.680_020_824_529_16);
        let t = lr.trend(&[1.0, 2.0, 3.0]).unwrap();
        close(t.statistic, 0.945_321_590_174_533);
        close(t.variance, 14.089_866_235_748_3);
        close(t.chi_square, 0.063_423_803_597_426_3);
        close(t.p_value, 0.801_164_096_508_439);
        let t = lr.trend(&[0.0, 1.0, 4.0]).unwrap();
        close(t.chi_square, 0.003_755_723_852_287_61);
        assert!(lr.trend(&[2.0, 2.0, 2.0]).is_err());
        assert!(lr.trend(&[1.0, 2.0]).is_err());
        // Number at risk at 0, 10, 20, 30, 40 and censored in each interval.
        let expected = [
            ([12, 7, 3, 2, 0], [0, 1, 0, 0, 1]),
            ([13, 7, 1, 0, 0], [0, 2, 2, 0, 0]),
            ([11, 3, 1, 1, 0], [0, 4, 0, 1, 0]),
        ];
        for (g, (risk, cens)) in expected.iter().enumerate() {
            let (t, e): (Vec<f64>, Vec<bool>) = (0..36).filter(|&i| dose[i] == g).map(|i| (time[i], events[i])).unzip();
            let km = kaplan_meier(&t, &e).unwrap();
            let ticks = [0.0, 10.0, 20.0, 30.0, 40.0];
            for (k, &tick) in ticks.iter().enumerate() {
                assert_eq!(km.at_risk_at(tick), risk[k], "group {g} at {tick}");
                let after = if k == 0 { f64::NEG_INFINITY } else { ticks[k - 1] };
                assert_eq!(km.censored_between(after, tick), cens[k], "group {g} at {tick}");
            }
        }
    }
}
