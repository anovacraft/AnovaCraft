//! Tests and intervals for proportions (Agresti, *Categorical Data Analysis*,
//! 3rd ed., §§1.4, 3.1, 3.5, 11.1; Newcombe 1998, *Stat. Med.* 17:873).
//!
//! * **Exact binomial test** of `p = p₀`: one-sided tails of Binomial(`n`,
//!   `p₀`); the two-sided p-value sums the probabilities of every outcome no
//!   more likely than the one observed (relative tolerance 10⁻⁷). Interval:
//!   Clopper–Pearson, `B(α/2; x, n − x + 1)` to `B(1 − α/2; x + 1, n − x)`.
//! * **Score test:** `z = (p̂ − p₀)/√(p₀(1 − p₀)/n)` with the Wilson interval
//!   `[p̂ + z²/2n ± z√(p̂(1−p̂)/n + z²/4n²)] / (1 + z²/n)`.
//! * **Two proportions:** pooled z test
//!   `z = (p̂₁ − p̂₂)/√(p̄(1 − p̄)(1/n₁ + 1/n₂))`; interval for `p₁ − p₂` by
//!   Newcombe's hybrid score method (from the two Wilson intervals).
//! * **Fisher's exact test** conditions on the margins (hypergeometric); the
//!   two-sided p-value sums tables no more likely than the observed one.
//! * **Relative risk** `p̂₁/p̂₂` and **odds ratio** with log-scale (Katz,
//!   Woolf) two-sided intervals. For the odds ratio ½ is added to every cell
//!   when any cell is zero; for the relative risk only when a group has no
//!   successes (each group's total then grows by 1), since `p̂₁/p̂₂` and its
//!   log-scale variance stay finite when a group has no failures. The Fisher result's interval is the
//!   Woolf interval at the test's level and alternative (one-sided: `z_{1−α}`,
//!   with the unbounded side `[0, …` or `…, ∞)`).
//! * **McNemar's test** for paired binary outcomes uses only the discordant
//!   counts `b, c`: `χ² = (|b − c| − 1)²/(b + c)` on 1 df (continuity
//!   corrected), and the exact binomial test of `b` out of `b + c` at ½.

use crate::common::{Alternative, TestResult, check_level, invalid};
use stat_core::Result;
use stat_core::discrete::{beta_quantile, binomial_cdf, binomial_pmf, binomial_sf_inclusive, hypergeometric_pmf};
use stat_core::distributions::{ChiSquared, ContinuousDistribution, Normal};

const REL_TOLERANCE: f64 = 1.0 + 1e-7;

fn check_counts(x: u64, n: u64) -> Result<()> {
    if n == 0 || x > n {
        return Err(invalid(format!("need 0 ≤ events ≤ trials and trials ≥ 1 (got {x} of {n})")));
    }
    Ok(())
}

fn check_p(p: f64) -> Result<()> {
    if p > 0.0 && p < 1.0 {
        Ok(())
    } else {
        Err(invalid(format!("the hypothesized proportion must lie in (0, 1) (got {p})")))
    }
}

/// Two-sided exact binomial p-value: total probability of outcomes no more
/// likely than `x`.
pub(crate) fn binomial_two_sided(x: u64, n: u64, p: f64) -> Result<f64> {
    let d = binomial_pmf(x, n, p)? * REL_TOLERANCE;
    let mode = (((n as f64 + 1.0) * p).floor() as u64).min(n);
    two_sided_unimodal(&|k| binomial_pmf(k, n, p), mode, n, d, &|k| binomial_cdf(k, n, p), &|k| {
        binomial_sf_inclusive(k, n, p)
    })
}

/// Total probability of the outcomes whose probability is at most `d`, for a
/// unimodal distribution on `0..=max` with its mode at `mode`: those outcomes
/// form a lower tail `0..=a` and an upper tail `b..=max`, whose ends are found
/// by bisection on each monotone side, so the cost is logarithmic in `max`
/// rather than linear (counts in the millions stay instant).
pub(crate) fn two_sided_unimodal(
    pmf: &dyn Fn(u64) -> Result<f64>,
    mode: u64,
    max: u64,
    d: f64,
    cdf: &dyn Fn(u64) -> Result<f64>,
    sf_inclusive: &dyn Fn(u64) -> Result<f64>,
) -> Result<f64> {
    if pmf(mode)? <= d {
        return Ok(1.0);
    }
    // Lower tail: the largest a < mode with pmf(a) ≤ d (pmf rises towards the mode).
    let lower = if pmf(0)? <= d {
        let (mut ok, mut bad) = (0_u64, mode);
        while bad - ok > 1 {
            let mid = ok + (bad - ok) / 2;
            if pmf(mid)? <= d { ok = mid } else { bad = mid }
        }
        cdf(ok)?
    } else {
        0.0
    };
    // Upper tail: the smallest b > mode with pmf(b) ≤ d (pmf falls after the mode).
    let upper = if pmf(max)? <= d {
        let (mut bad, mut ok) = (mode, max);
        while ok - bad > 1 {
            let mid = bad + (ok - bad) / 2;
            if pmf(mid)? <= d { ok = mid } else { bad = mid }
        }
        sf_inclusive(ok)?
    } else {
        0.0
    };
    Ok((lower + upper).min(1.0))
}

/// Clopper–Pearson limits for `x` events in `n` trials with tail `tail` on
/// each bounded side.
pub(crate) fn clopper_pearson(x: u64, n: u64, tail: f64) -> Result<(f64, f64)> {
    let (xf, nf) = (x as f64, n as f64);
    let lo = if x == 0 { 0.0 } else { beta_quantile(xf, nf - xf + 1.0, tail)? };
    let hi = if x == n { 1.0 } else { beta_quantile(xf + 1.0, nf - xf, 1.0 - tail)? };
    Ok((lo, hi))
}

fn wilson(x: u64, n: u64, z: f64) -> (f64, f64) {
    let (xf, nf) = (x as f64, n as f64);
    let p = xf / nf;
    let den = 1.0 + z * z / nf;
    let centre = (p + z * z / (2.0 * nf)) / den;
    let half = z * (p * (1.0 - p) / nf + z * z / (4.0 * nf * nf)).sqrt() / den;
    ((centre - half).max(0.0), (centre + half).min(1.0))
}

/// Difference of two independent proportions `x₁/n₁ − x₂/n₂` with
/// Newcombe's hybrid score interval (Newcombe, *Stat. Med.* 17, 1998,
/// method 10): from the Wilson limits `(l₁, u₁)` and `(l₂, u₂)`,
/// `d − √((p₁ − l₁)² + (u₂ − p₂)²)` to `d + √((u₁ − p₁)² + (p₂ − l₂)²)`.
/// Unlike the Wald interval it stays inside [−1, 1] and needs no
/// continuity correction when a proportion is 0 or 1.
///
/// # Errors
/// Invalid counts or level.
pub fn newcombe_difference(x1: u64, n1: u64, x2: u64, n2: u64, level: f64) -> Result<(f64, f64, f64)> {
    check_counts(x1, n1)?;
    check_counts(x2, n2)?;
    check_level(level)?;
    let z = Normal::standard().quantile(0.5 + level / 2.0)?;
    let (p1, p2) = (x1 as f64 / n1 as f64, x2 as f64 / n2 as f64);
    let (l1, u1) = wilson(x1, n1, z);
    let (l2, u2) = wilson(x2, n2, z);
    let d = p1 - p2;
    Ok((d, d - ((p1 - l1).powi(2) + (u2 - p2).powi(2)).sqrt(), d + ((u1 - p1).powi(2) + (p2 - l2).powi(2)).sqrt()))
}

/// Exact binomial test of `p = p0` with the Clopper–Pearson interval.
///
/// # Errors
/// [`crate::common`] argument errors: `n = 0`, `x > n`, `p₀ ∉ (0, 1)`, bad level.
pub fn exact_binomial_test(x: u64, n: u64, p0: f64, level: f64, alternative: Alternative) -> Result<TestResult> {
    check_counts(x, n)?;
    check_p(p0)?;
    check_level(level)?;
    let p_value = match alternative {
        Alternative::TwoSided => binomial_two_sided(x, n, p0)?,
        Alternative::Less => binomial_cdf(x, n, p0)?,
        Alternative::Greater => binomial_sf_inclusive(x, n, p0)?,
    };
    let (lo, hi) = clopper_pearson(x, n, alternative.tail(level))?;
    let (lo, hi) = alternative.bound(lo, hi);
    Ok(TestResult {
        method: "Exact binomial",
        estimate: x as f64 / n as f64,
        null_value: p0,
        std_error: f64::NAN,
        statistic: x as f64,
        statistic_name: "events",
        df: None,
        df2: None,
        p_value,
        confidence_interval: (lo.max(0.0), hi.min(1.0)),
        level,
        alternative,
    })
}

/// Score (z) test of `p = p0` with the Wilson interval.
///
/// # Errors
/// As [`exact_binomial_test`].
pub fn score_test(x: u64, n: u64, p0: f64, level: f64, alternative: Alternative) -> Result<TestResult> {
    check_counts(x, n)?;
    check_p(p0)?;
    check_level(level)?;
    let nf = n as f64;
    let p = x as f64 / nf;
    let se0 = (p0 * (1.0 - p0) / nf).sqrt();
    let z = (p - p0) / se0;
    let normal = Normal::standard();
    let crit = normal.quantile(1.0 - alternative.tail(level))?;
    let (lo, hi) = wilson(x, n, crit);
    let (lo, hi) = alternative.bound(lo, hi);
    Ok(TestResult {
        method: "Score z (Wilson interval)",
        estimate: p,
        null_value: p0,
        std_error: (p * (1.0 - p) / nf).sqrt(),
        statistic: z,
        statistic_name: "z",
        df: None,
        df2: None,
        p_value: alternative.p_value(normal.cdf(z)?, normal.sf(z)?),
        confidence_interval: (lo, hi),
        level,
        alternative,
    })
}

/// A ratio estimate with confidence limits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RatioEstimate {
    /// Estimate.
    pub estimate: f64,
    /// Lower limit.
    pub lower: f64,
    /// Upper limit.
    pub upper: f64,
}

/// Comparison of two independent proportions.
#[derive(Debug, Clone, PartialEq)]
pub struct TwoProportions {
    /// Pooled z test of `p₁ − p₂ = 0` with Newcombe's interval.
    pub z_test: TestResult,
    /// Fisher's exact test (estimate: the sample odds ratio).
    pub fisher: TestResult,
    /// Relative risk `p₁/p₂`.
    pub relative_risk: RatioEstimate,
    /// Odds ratio.
    pub odds_ratio: RatioEstimate,
}

/// Compare `x1` of `n1` with `x2` of `n2`.
///
/// # Errors
/// Invalid counts or level, or both proportions 0 or both 1 (no variation).
pub fn two_proportions(
    x1: u64,
    n1: u64,
    x2: u64,
    n2: u64,
    level: f64,
    alternative: Alternative,
) -> Result<TwoProportions> {
    check_counts(x1, n1)?;
    check_counts(x2, n2)?;
    check_level(level)?;
    let (f1, f2, m1, m2) = (x1 as f64, x2 as f64, n1 as f64, n2 as f64);
    let (p1, p2) = (f1 / m1, f2 / m2);
    let pooled = (f1 + f2) / (m1 + m2);
    if pooled == 0.0 || pooled == 1.0 {
        return Err(invalid("both samples are all events or all non-events"));
    }
    let normal = Normal::standard();
    let se0 = (pooled * (1.0 - pooled) * (1.0 / m1 + 1.0 / m2)).sqrt();
    let z = (p1 - p2) / se0;
    let crit = normal.quantile(1.0 - alternative.tail(level))?;
    let (l1, u1) = wilson(x1, n1, crit);
    let (l2, u2) = wilson(x2, n2, crit);
    let d = p1 - p2;
    let lo = d - ((p1 - l1).powi(2) + (u2 - p2).powi(2)).sqrt();
    let hi = d + ((u1 - p1).powi(2) + (p2 - l2).powi(2)).sqrt();
    let (lo, hi) = alternative.bound(lo, hi);
    let z_test = TestResult {
        method: "Two-proportion z (Newcombe interval)",
        estimate: d,
        null_value: 0.0,
        std_error: (p1 * (1.0 - p1) / m1 + p2 * (1.0 - p2) / m2).sqrt(),
        statistic: z,
        statistic_name: "z",
        df: None,
        df2: None,
        p_value: alternative.p_value(normal.cdf(z)?, normal.sf(z)?),
        confidence_interval: (lo, hi),
        level,
        alternative,
    };
    // Cells: [x1, n1 − x1; x2, n2 − x2].
    let cells = [f1, m1 - f1, f2, m2 - f2];
    let adj: [f64; 4] = if cells.contains(&0.0) { cells.map(|c| c + 0.5) } else { cells };
    let zc = normal.quantile(1.0 - (1.0 - level) / 2.0)?;
    let or = adj[0] * adj[3] / (adj[1] * adj[2]);
    let se_or = adj.iter().map(|c| 1.0 / c).sum::<f64>().sqrt();
    let odds_ratio = RatioEstimate { estimate: or, lower: or * (-zc * se_or).exp(), upper: or * (zc * se_or).exp() };
    // Haldane–Anscombe: ½ in every cell, so each total gains 1.
    let (a1, t1, a2, t2) = if x1 == 0 || x2 == 0 { (f1 + 0.5, m1 + 1.0, f2 + 0.5, m2 + 1.0) } else { (f1, m1, f2, m2) };
    let rr = (a1 / t1) / (a2 / t2);
    let se_rr = (1.0 / a1 - 1.0 / t1 + 1.0 / a2 - 1.0 / t2).sqrt();
    let relative_risk = RatioEstimate { estimate: rr, lower: rr * (-zc * se_rr).exp(), upper: rr * (zc * se_rr).exp() };
    // Fisher: X = x1 ~ hypergeometric(total, column total, n1).
    let total = n1 + n2;
    let successes = x1 + x2;
    let lo_x = successes.saturating_sub(n2);
    let hi_x = successes.min(n1);
    let p_obs = hypergeometric_pmf(x1, total, successes, n1)? * REL_TOLERANCE;
    let (mut two, mut lower_tail, mut upper_tail) = (0.0, 0.0, 0.0);
    for k in lo_x..=hi_x {
        let q = hypergeometric_pmf(k, total, successes, n1)?;
        if q <= p_obs {
            two += q;
        }
        if k <= x1 {
            lower_tail += q;
        }
        if k >= x1 {
            upper_tail += q;
        }
    }
    let fisher_p = match alternative {
        Alternative::TwoSided => two,
        Alternative::Less => lower_tail,
        Alternative::Greater => upper_tail,
    };
    let fisher = TestResult {
        method: "Fisher exact",
        estimate: or,
        null_value: 1.0,
        std_error: f64::NAN,
        statistic: f1,
        statistic_name: "events",
        df: None,
        df2: None,
        p_value: fisher_p.min(1.0),
        confidence_interval: {
            let z_alt = normal.quantile(1.0 - alternative.tail(level))?;
            let (lo, hi) = alternative.bound(or * (-z_alt * se_or).exp(), or * (z_alt * se_or).exp());
            (lo.max(0.0), hi)
        },
        level,
        alternative,
    };
    Ok(TwoProportions { z_test, fisher, relative_risk, odds_ratio })
}

/// McNemar's test for a paired 2×2 table `[[both, first only], [second only,
/// neither]]` (`n12` = yes then no, `n21` = no then yes).
#[derive(Debug, Clone, PartialEq)]
pub struct McNemarTest {
    /// Continuity-corrected chi-square test.
    pub chi_square: TestResult,
    /// Exact binomial test on the discordant pairs.
    pub exact: TestResult,
}

/// McNemar's test. The estimate is the difference of the marginal
/// proportions `(n12 − n21)/n` with a Wald interval.
///
/// # Errors
/// [`crate::common`] argument errors when there are no discordant pairs.
pub fn mcnemar_test(n11: u64, n12: u64, n21: u64, n22: u64, level: f64) -> Result<McNemarTest> {
    check_level(level)?;
    let discordant = n12 + n21;
    if discordant == 0 {
        return Err(invalid("McNemar's test needs at least one discordant pair"));
    }
    let n = (n11 + n12 + n21 + n22) as f64;
    let (b, c) = (n12 as f64, n21 as f64);
    let diff = (b - c) / n;
    let se = ((b + c) - (b - c).powi(2) / n).max(0.0).sqrt() / n;
    let zc = Normal::standard().quantile(0.5 + level / 2.0)?;
    let ci = (diff - zc * se, diff + zc * se);
    let chi = ((b - c).abs() - 1.0).max(0.0).powi(2) / (b + c);
    let chi_square = TestResult {
        method: "McNemar χ² (continuity corrected)",
        estimate: diff,
        null_value: 0.0,
        std_error: se,
        statistic: chi,
        statistic_name: "χ²",
        df: Some(1.0),
        df2: None,
        p_value: ChiSquared::new(1.0)?.sf(chi)?,
        confidence_interval: ci,
        level,
        alternative: Alternative::TwoSided,
    };
    let exact = TestResult {
        method: "McNemar exact (binomial)",
        p_value: binomial_two_sided(n12, discordant, 0.5)?,
        statistic: b,
        statistic_name: "events",
        df: None,
        ..chi_square.clone()
    };
    Ok(McNemarTest { chi_square, exact })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::means::tests::close;

    /// Published (R `binom.test` documentation): 682 of 925 against 0.75,
    /// p = 0.3825, 95% CI (0.7076683, 0.7654066).
    #[test]
    fn one_proportion() {
        let e = exact_binomial_test(682, 925, 0.75, 0.95, Alternative::TwoSided).unwrap();
        close(e.p_value, 0.382_491_559_574_851_7, 1e-9);
        close(e.confidence_interval.0, 0.707_668_264_079_038_9, 1e-10);
        close(e.confidence_interval.1, 0.765_406_558_241_525_3, 1e-10);
        let s = score_test(682, 925, 0.75, 0.95, Alternative::TwoSided).unwrap();
        close(s.statistic, -0.892_208_516_007_347_7, 1e-12);
        close(s.p_value, 0.372_281_178_458_713_9, 1e-10);
        close(s.confidence_interval.0, 0.707_995_987_619_620_5, 1e-10);
        close(s.confidence_interval.1, 0.764_635_800_971_488_9, 1e-10);
        let g = exact_binomial_test(7, 10, 0.4, 0.95, Alternative::Greater).unwrap();
        close(g.p_value, binomial_sf_inclusive(7, 10, 0.4).unwrap(), 0.0);
        assert_eq!(g.confidence_interval.1, 1.0f64.min(f64::INFINITY));
        assert!(exact_binomial_test(3, 2, 0.5, 0.95, Alternative::TwoSided).is_err());
        assert_eq!(exact_binomial_test(0, 5, 0.5, 0.95, Alternative::TwoSided).unwrap().confidence_interval.0, 0.0);
    }

    /// z, Newcombe limits, Fisher p, and log-scale ratios evaluated
    /// independently (SciPy 1.18 `fisher_exact`, formulas in NumPy); Fisher's
    /// tea-tasting table gives p = 0.4857 (published).
    #[test]
    fn two_proportion_tests() {
        let t = two_proportions(45, 80, 30, 85, 0.95, Alternative::TwoSided).unwrap();
        close(t.z_test.statistic, 2.701_783_180_223_974_7, 1e-12);
        close(t.z_test.p_value, 0.006_896_872_139_141_908, 1e-9);
        close(t.z_test.confidence_interval.0, 0.057_498_608_710_455_56, 1e-9);
        close(t.z_test.confidence_interval.1, 0.348_745_245_216_489_6, 1e-9);
        close(t.fisher.p_value, 0.008_043_675_896_918_88, 1e-9);
        close(t.odds_ratio.estimate, 2.357_142_857_142_857, 1e-14);
        close(t.odds_ratio.lower, 1.259_281_231_252_178, 1e-9);
        close(t.odds_ratio.upper, 4.412_137_901_439_864_5, 1e-9);
        close(t.relative_risk.estimate, 1.593_75, 1e-14);
        close(t.relative_risk.lower, 1.126_806_350_550_432_9, 1e-9);
        close(t.relative_risk.upper, 2.254_193_066_323_435_5, 1e-9);
        let tea = two_proportions(3, 4, 1, 4, 0.95, Alternative::TwoSided).unwrap();
        close(tea.fisher.p_value, 0.485_714_285_714_285_65, 1e-12);
    }

    /// Continuity-corrected χ² and exact binomial p on discordant pairs
    /// 86 / 150 (SciPy 1.18 `chi2.sf`, `binomtest`).
    #[test]
    fn mcnemar() {
        let m = mcnemar_test(794, 86, 150, 570, 0.95).unwrap();
        close(m.chi_square.statistic, 16.817_796_610_169_49, 1e-12);
        close(m.chi_square.p_value, 4.114_562_281_345_944e-5, 1e-8);
        close(m.exact.p_value, 3.715_936_139_571_386_6e-5, 1e-8);
        close(m.chi_square.estimate, -64.0 / 1600.0, 1e-14);
        assert!(mcnemar_test(5, 0, 0, 5, 0.95).is_err());
    }

    #[test]
    fn two_sided_p_values_match_brute_force_and_scale() {
        let brute = |x: u64, n: u64, p: f64| {
            let d = binomial_pmf(x, n, p).unwrap() * REL_TOLERANCE;
            (0..=n).map(|i| binomial_pmf(i, n, p).unwrap()).filter(|&q| q <= d).sum::<f64>().min(1.0)
        };
        for n in [1, 2, 5, 17, 60] {
            for &p in &[0.01, 0.2, 0.5, 0.73, 0.99] {
                for x in 0..=n {
                    let fast = binomial_two_sided(x, n, p).unwrap();
                    assert!((fast - brute(x, n, p)).abs() < 1e-12, "x={x} n={n} p={p}");
                }
            }
        }
        // A billion trials: logarithmic cost, and an extreme count gives p ≈ 0.
        let start = std::time::Instant::now();
        assert!(binomial_two_sided(500_000_000, 1_000_000_000, 0.5).unwrap() > 0.99);
        assert!(binomial_two_sided(400_000_000, 1_000_000_000, 0.5).unwrap() < 1e-300);
        assert!(start.elapsed().as_secs_f64() < 1.0);
    }

    /// One-sided Woolf limits for the Fisher result use `z_{1−α}` and keep the
    /// odds ratio non-negative: OR = 45·55/(35·30), SE = √(Σ 1/cell),
    /// limits OR·exp(∓1.6449·SE) (SciPy 1.18.1 `norm.ppf`).
    #[test]
    fn one_sided_odds_ratio_interval() {
        let g = two_proportions(45, 80, 30, 85, 0.95, Alternative::Greater).unwrap();
        close(g.fisher.confidence_interval.0, 1.392_821_448_828_740_7, 1e-10);
        assert_eq!(g.fisher.confidence_interval.1, f64::INFINITY);
        let l = two_proportions(45, 80, 30, 85, 0.95, Alternative::Less).unwrap();
        assert_eq!(l.fisher.confidence_interval.0, 0.0);
        close(l.fisher.confidence_interval.1, 3.989_113_215_948_733_7, 1e-10);
        // Two-sided: unchanged, equal to the odds-ratio interval.
        let t = two_proportions(45, 80, 30, 85, 0.95, Alternative::TwoSided).unwrap();
        assert_eq!(t.fisher.confidence_interval, (t.odds_ratio.lower, t.odds_ratio.upper));
    }

    /// With a zero cell, ½ is added to every cell (each total gains 1):
    /// 0/10 vs 5/20 gives RR = (0.5/11)/(5.5/21) and
    /// SE² = 1/0.5 − 1/11 + 1/5.5 − 1/21, limits RR·exp(∓1.96·SE)
    /// (evaluated with NumPy/SciPy 1.18.1).
    #[test]
    fn relative_risk_zero_cell_correction() {
        let t = two_proportions(0, 10, 5, 20, 0.95, Alternative::TwoSided).unwrap();
        close(t.relative_risk.estimate, 0.173_553_719_008_264_47, 1e-12);
        close(t.relative_risk.lower, 0.010_536_466_046_271_73, 1e-9);
        close(t.relative_risk.upper, 2.858_728_272_773_937, 1e-9);
    }

    /// Newcombe (1998), Table II, example (c): 56/70 against 48/80 gives
    /// 0.2000 with method-10 limits 0.0524 to 0.3339.
    #[test]
    fn newcombe_difference_matches_the_published_example() {
        let (d, lo, hi) = newcombe_difference(56, 70, 48, 80, 0.95).unwrap();
        assert!((d - 0.2).abs() < 1e-12);
        assert!((lo - 0.0524).abs() < 5e-5 && (hi - 0.3339).abs() < 5e-5, "{lo} {hi}");
        // Zero events in one group still gives limits inside [−1, 1].
        let (_, lo, hi) = newcombe_difference(0, 10, 3, 10, 0.95).unwrap();
        assert!(lo > -1.0 && hi < 1.0 && lo < 0.0);
    }
}
