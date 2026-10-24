//! Exact tests for Poisson event rates (counts per unit of exposure such as
//! time, area or units inspected).
//!
//! * **One rate:** with `x` events in exposure `T`, `X ~ Poisson(λT)`; the test
//!   of `λ = λ₀` uses the Poisson tails at mean `λ₀T` (two-sided: total
//!   probability of counts no more likely than `x`). The Garwood (1936)
//!   interval is `(χ²_{α/2}(2x)/2T, χ²_{1−α/2}(2x + 2)/2T)`.
//! * **Rate ratio** `λ₁/λ₂`: conditional on `x₁ + x₂`, `x₁` is
//!   Binomial(`x₁ + x₂`, `π`), `π = ρT₁/(ρT₁ + T₂)`, so the exact binomial test
//!   of `π = π₀` tests `ρ = ρ₀`; the Clopper–Pearson limits `(π_L, π_U)` give
//!   `ρ = π/(1 − π)·T₂/T₁` (Przyborowski & Wilenski 1940; Lehmann & Romano,
//!   *Testing Statistical Hypotheses*, §4.6).

use crate::common::{Alternative, TestResult, check_level, invalid};
use crate::proportions::{binomial_two_sided, clopper_pearson};
use stat_core::Result;
use stat_core::discrete::{binomial_cdf, binomial_sf_inclusive, poisson_cdf, poisson_pmf, poisson_sf_inclusive};
use stat_core::distributions::{ChiSquared, ContinuousDistribution};

fn check_exposure(t: f64) -> Result<()> {
    if t > 0.0 && t.is_finite() { Ok(()) } else { Err(invalid(format!("exposure must be positive (got {t})"))) }
}

fn poisson_two_sided(x: u64, mean: f64) -> Result<f64> {
    let d = poisson_pmf(x, mean)? * (1.0 + 1e-7);
    // Beyond this point the Poisson probabilities are negligible (< 1e-300).
    let max = (mean + 50.0 * mean.sqrt() + 100.0).ceil().min(u64::MAX as f64) as u64;
    crate::proportions::two_sided_unimodal(
        &|k| poisson_pmf(k, mean),
        (mean.floor() as u64).min(max),
        max.max(x),
        d,
        &|k| poisson_cdf(k, mean),
        &|k| poisson_sf_inclusive(k, mean),
    )
}

/// Exact test that the event rate equals `rate0`, with the Garwood interval.
///
/// # Errors
/// Non-positive exposure or `rate0`, or a bad level.
pub fn one_rate_test(
    count: u64,
    exposure: f64,
    rate0: f64,
    level: f64,
    alternative: Alternative,
) -> Result<TestResult> {
    check_exposure(exposure)?;
    check_level(level)?;
    if !(rate0 > 0.0 && rate0.is_finite()) {
        return Err(invalid(format!("the hypothesized rate must be positive (got {rate0})")));
    }
    let mean = rate0 * exposure;
    let p_value = match alternative {
        Alternative::TwoSided => poisson_two_sided(count, mean)?,
        Alternative::Less => poisson_cdf(count, mean)?,
        Alternative::Greater => poisson_sf_inclusive(count, mean)?,
    };
    let tail = alternative.tail(level);
    let x = count as f64;
    let lo = if count == 0 { 0.0 } else { ChiSquared::new(2.0 * x)?.quantile(tail)? / (2.0 * exposure) };
    let hi = ChiSquared::new(2.0 * x + 2.0)?.quantile(1.0 - tail)? / (2.0 * exposure);
    let (lo, hi) = alternative.bound(lo, hi);
    Ok(TestResult {
        method: "Exact Poisson rate",
        estimate: x / exposure,
        null_value: rate0,
        std_error: x.sqrt() / exposure,
        statistic: x,
        statistic_name: "events",
        df: None,
        df2: None,
        p_value,
        confidence_interval: (lo.max(0.0), hi),
        level,
        alternative,
    })
}

/// Exact conditional test that the rate ratio `λ₁/λ₂` equals `ratio0`.
///
/// # Errors
/// Non-positive exposures or `ratio0`, no events at all, or a bad level.
pub fn rate_ratio_test(
    count1: u64,
    exposure1: f64,
    count2: u64,
    exposure2: f64,
    ratio0: f64,
    level: f64,
    alternative: Alternative,
) -> Result<TestResult> {
    check_exposure(exposure1)?;
    check_exposure(exposure2)?;
    check_level(level)?;
    if !(ratio0 > 0.0 && ratio0.is_finite()) {
        return Err(invalid(format!("the hypothesized rate ratio must be positive (got {ratio0})")));
    }
    let n = count1 + count2;
    if n == 0 {
        return Err(invalid("a rate comparison needs at least one event"));
    }
    let pi0 = ratio0 * exposure1 / (ratio0 * exposure1 + exposure2);
    let p_value = match alternative {
        Alternative::TwoSided => binomial_two_sided(count1, n, pi0)?,
        Alternative::Less => binomial_cdf(count1, n, pi0)?,
        Alternative::Greater => binomial_sf_inclusive(count1, n, pi0)?,
    };
    let (pl, pu) = clopper_pearson(count1, n, alternative.tail(level))?;
    let to_ratio = |p: f64| if p >= 1.0 { f64::INFINITY } else { p / (1.0 - p) * exposure2 / exposure1 };
    let (lo, hi) = alternative.bound(to_ratio(pl), to_ratio(pu));
    let (r1, r2) = (count1 as f64 / exposure1, count2 as f64 / exposure2);
    Ok(TestResult {
        method: "Exact rate ratio (conditional binomial)",
        estimate: if r2 > 0.0 { r1 / r2 } else { f64::INFINITY },
        null_value: ratio0,
        std_error: f64::NAN,
        statistic: count1 as f64,
        statistic_name: "events",
        df: None,
        df2: None,
        p_value,
        confidence_interval: (lo, hi),
        level,
        alternative,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::means::tests::close;

    /// Published (R `poisson.test` documentation): 137 events in 24.19893
    /// person-years, 95% CI (4.753125, 6.692709); comparison of 11 events in
    /// 800 with 21 in 3011: p = 0.07967, ratio 1.971488, CI (0.8584264,
    /// 4.2772659).
    #[test]
    fn rates() {
        let r = one_rate_test(137, 24.198_93, 1.0, 0.95, Alternative::TwoSided).unwrap();
        close(r.confidence_interval.0, 4.753_124_804_848_75, 1e-10);
        close(r.confidence_interval.1, 6.692_709_334_176_073, 1e-10);
        assert!(r.p_value < 1e-50);
        close(
            one_rate_test(12, 1.0, 8.0, 0.95, Alternative::TwoSided).unwrap().p_value,
            0.154_304_113_010_202_28,
            1e-9,
        );
        let c = rate_ratio_test(11, 800.0, 21, 3011.0, 1.0, 0.95, Alternative::TwoSided).unwrap();
        close(c.p_value, 0.079_668_633_033_329_5, 1e-9);
        close(c.estimate, 1.971_488_095_238_095_3, 1e-14);
        close(c.confidence_interval.0, 0.858_426_403_391_639_7, 1e-9);
        close(c.confidence_interval.1, 4.277_265_943_654_073, 1e-9);
        assert!(rate_ratio_test(0, 1.0, 0, 1.0, 1.0, 0.95, Alternative::TwoSided).is_err());
        assert_eq!(one_rate_test(0, 2.0, 1.0, 0.95, Alternative::TwoSided).unwrap().confidence_interval.0, 0.0);
    }

    #[test]
    fn poisson_two_sided_matches_brute_force_and_scales() {
        for &mean in &[0.3, 2.0, 9.5, 40.0] {
            for x in 0..80_u64 {
                let d = poisson_pmf(x, mean).unwrap() * (1.0 + 1e-7);
                let upper = (mean + 50.0 * mean.sqrt() + 100.0).ceil() as u64;
                let brute: f64 = (0..=upper.max(x)).map(|i| poisson_pmf(i, mean).unwrap()).filter(|&q| q <= d).sum();
                assert!((poisson_two_sided(x, mean).unwrap() - brute.min(1.0)).abs() < 1e-12, "x={x} mean={mean}");
            }
        }
        let start = std::time::Instant::now();
        assert!(poisson_two_sided(1_000_000_000, 1e9).unwrap() > 0.99);
        assert!(start.elapsed().as_secs_f64() < 1.0);
    }
}
