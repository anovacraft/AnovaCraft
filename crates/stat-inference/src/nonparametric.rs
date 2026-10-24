//! Rank- and sign-based tests (Hollander, Wolfe & Chicken, *Nonparametric
//! Statistical Methods*, 3rd ed., chs. 3, 4, 7).
//!
//! * **Sign test** for a median `m₀`: `S = #{xᵢ > m₀}` among the `n'` values
//!   not equal to `m₀` is Binomial(`n'`, ½) under `H₀`. The confidence
//!   interval for the median is `(x₍ₖ₎, x₍ₙ₋ₖ₊₁₎)` with the largest `k` such
//!   that `P(B ≤ k − 1) ≤ α/2` (its actual coverage is at least the level).
//! * **Wilcoxon signed-rank:** with `dᵢ = xᵢ − m₀`, zeros dropped, `V` is the
//!   sum of the mid-ranks of `|dᵢ|` over positive `dᵢ`. Exact null
//!   distribution (all `2ⁿ` sign patterns) when `n' ≤ 50` without ties;
//!   otherwise normal with mean `n'(n'+1)/4`, variance
//!   `n'(n'+1)(2n'+1)/24 − Σ(t³ − t)/48` and a continuity correction of ½.
//!   The Hodges–Lehmann estimate is `m₀` plus the median of the Walsh
//!   averages `(dᵢ + dⱼ)/2, i ≤ j`.
//! * **Wilcoxon rank-sum / Mann–Whitney:** `U = R₁ − n₁(n₁+1)/2` from the
//!   mid-ranks of the pooled sample (after subtracting `Δ₀` from the first
//!   sample). Exact null distribution when both samples have at most 50 values
//!   and there are no ties, else normal with tie correction and continuity
//!   correction. The Hodges–Lehmann shift is the median of all `aᵢ − bⱼ`.
//! * **Friedman** test for `k` treatments in `n` blocks: within-block
//!   mid-ranks with column sums `Rⱼ`,
//!   `Q = [12/(nk(k+1)) ΣRⱼ² − 3n(k+1)] / [1 − Σ(t³ − t)/(nk(k² − 1))]`,
//!   χ² on `k − 1` df.

use crate::common::{
    Alternative, ChiSquareResult, TestResult, check_finite, check_level, invalid, median, ranks_with_ties,
};
use stat_core::discrete::{binomial_cdf, binomial_sf_inclusive};
use stat_core::distributions::{ChiSquared, ContinuousDistribution, Normal};
use stat_core::{Result, StatError};

/// Largest sample size for exact rank distributions.
pub const EXACT_LIMIT: usize = 50;

/// Sign test that the median equals `m0`, with an order-statistic confidence
/// interval for the median.
///
/// # Errors
/// [`StatError::InsufficientData`] if every value equals `m0`;
/// [`StatError::NonFiniteInput`] for NaN/±∞.
pub fn sign_test(x: &[f64], m0: f64, level: f64, alternative: Alternative) -> Result<TestResult> {
    check_finite(x, "sample values")?;
    check_level(level)?;
    let above = x.iter().filter(|&&v| v > m0).count() as u64;
    let below = x.iter().filter(|&&v| v < m0).count() as u64;
    let n = above + below;
    if n == 0 {
        return Err(StatError::InsufficientData { required: 1, found: 0 });
    }
    let lower_tail = binomial_cdf(above, n, 0.5)?;
    let upper_tail = binomial_sf_inclusive(above, n, 0.5)?;
    let mut sorted = x.to_vec();
    sorted.sort_by(f64::total_cmp);
    let len = sorted.len() as u64;
    let tail = alternative.tail(level);
    // Largest k (1-based) with P(B ≤ k − 1) ≤ tail, B ~ Binomial(len, ½).
    let mut k = 0u64;
    while k < len / 2 + 1 && binomial_cdf(k, len, 0.5)? <= tail {
        k += 1;
    }
    let (lo, hi) = if k == 0 {
        (f64::NEG_INFINITY, f64::INFINITY)
    } else {
        (sorted[(k - 1) as usize], sorted[(len - k) as usize])
    };
    let (lo, hi) = alternative.bound(lo, hi);
    Ok(TestResult {
        method: "Sign test",
        estimate: median(x),
        null_value: m0,
        std_error: f64::NAN,
        statistic: above as f64,
        statistic_name: "S",
        df: None,
        df2: None,
        p_value: alternative.p_value(lower_tail, upper_tail),
        confidence_interval: (lo, hi),
        level,
        alternative,
    })
}

/// Exact null distribution counts of the signed-rank statistic for `n` untied ranks.
fn signed_rank_counts(n: usize) -> Vec<f64> {
    let max = n * (n + 1) / 2;
    let mut c = vec![0.0; max + 1];
    c[0] = 1.0;
    for r in 1..=n {
        for s in (r..=max).rev() {
            c[s] += c[s - r];
        }
    }
    c
}

/// `(lower, upper)` tail probabilities of `stat` under a normal approximation
/// with continuity correction appropriate to `alternative`.
fn normal_tails(stat: f64, mean: f64, sd: f64, alternative: Alternative) -> Result<(f64, f64, f64)> {
    let centered = stat - mean;
    let correction = match alternative {
        // No correction at the null mean itself (`signum` would give ±1 there).
        Alternative::TwoSided if centered > 0.0 => 0.5,
        Alternative::TwoSided if centered < 0.0 => -0.5,
        Alternative::TwoSided => 0.0,
        Alternative::Greater => 0.5,
        Alternative::Less => -0.5,
    };
    let z = (centered - correction) / sd;
    let n = Normal::standard();
    Ok((n.cdf(z)?, n.sf(z)?, z))
}

fn walsh_median(d: &[f64]) -> f64 {
    let mut w = Vec::with_capacity(d.len() * (d.len() + 1) / 2);
    for i in 0..d.len() {
        for j in i..d.len() {
            w.push(0.5 * (d[i] + d[j]));
        }
    }
    median(&w)
}

/// Wilcoxon signed-rank test that the distribution of `x` is symmetric about
/// `m0`, with the Hodges–Lehmann estimate of the centre.
///
/// The confidence interval is not computed (NaN).
///
/// # Errors
/// [`StatError::InsufficientData`] if every value equals `m0`;
/// [`StatError::NonFiniteInput`] for NaN/±∞.
pub fn signed_rank_test(x: &[f64], m0: f64, level: f64, alternative: Alternative) -> Result<TestResult> {
    check_finite(x, "sample values")?;
    check_level(level)?;
    let all: Vec<f64> = x.iter().map(|v| v - m0).collect();
    let d: Vec<f64> = all.iter().copied().filter(|&v| v != 0.0).collect();
    let n = d.len();
    if n == 0 {
        return Err(StatError::InsufficientData { required: 1, found: 0 });
    }
    let abs: Vec<f64> = d.iter().map(|v| v.abs()).collect();
    let (ranks, ties) = ranks_with_ties(&abs);
    let v: f64 = d.iter().zip(&ranks).filter(|(x, _)| **x > 0.0).map(|(_, r)| r).sum();
    let nf = n as f64;
    let (lower_tail, upper_tail, method) = if n <= EXACT_LIMIT && ties == 0.0 {
        let c = signed_rank_counts(n);
        let total = 2f64.powi(n as i32);
        let vi = v.round() as usize;
        let lower: f64 = c[..=vi].iter().sum::<f64>() / total;
        let upper: f64 = c[vi..].iter().sum::<f64>() / total;
        (lower, upper, "Wilcoxon signed-rank (exact)")
    } else {
        let var = nf * (nf + 1.0) * (2.0 * nf + 1.0) / 24.0 - ties / 48.0;
        let (lo, hi, _) = normal_tails(v, nf * (nf + 1.0) / 4.0, var.sqrt(), alternative)?;
        (lo, hi, "Wilcoxon signed-rank (normal approximation)")
    };
    Ok(TestResult {
        method,
        estimate: m0 + walsh_median(&all),
        null_value: m0,
        std_error: f64::NAN,
        statistic: v,
        statistic_name: "V",
        df: None,
        df2: None,
        p_value: alternative.p_value(lower_tail, upper_tail),
        confidence_interval: (f64::NAN, f64::NAN),
        level,
        alternative,
    })
}

/// Wilcoxon signed-rank test on paired differences `a − b` against `Δ₀`.
///
/// # Errors
/// As [`signed_rank_test`], plus unequal lengths.
pub fn paired_signed_rank(
    a: &[f64],
    b: &[f64],
    delta0: f64,
    level: f64,
    alternative: Alternative,
) -> Result<TestResult> {
    let d = crate::means::paired_differences(a, b)?;
    signed_rank_test(&d, delta0, level, alternative)
}

/// Exact null distribution counts of the rank sum `U` for sample sizes `(m, n)`.
fn rank_sum_counts(m: usize, n: usize) -> Vec<f64> {
    // dp[k][s]: subsets of size k of {1..N} with rank sum s.
    let big_n = m + n;
    let max = big_n * (big_n + 1) / 2;
    let mut dp = vec![vec![0.0; max + 1]; m + 1];
    dp[0][0] = 1.0;
    for r in 1..=big_n {
        for k in (1..=m.min(r)).rev() {
            for s in (r..=max).rev() {
                let add = dp[k - 1][s - r];
                if add != 0.0 {
                    dp[k][s] += add;
                }
            }
        }
    }
    let offset = m * (m + 1) / 2;
    dp[m][offset..=offset + m * n].to_vec()
}

/// Wilcoxon rank-sum (Mann–Whitney) test of a location shift `Δ₀` between `a`
/// and `b`, with the Hodges–Lehmann shift estimate. The statistic is `U` of
/// the first sample. The confidence interval is not computed (NaN).
///
/// # Errors
/// [`StatError::InsufficientData`] for an empty sample;
/// [`StatError::InvalidArgument`] if all values are tied.
pub fn rank_sum_test(a: &[f64], b: &[f64], delta0: f64, level: f64, alternative: Alternative) -> Result<TestResult> {
    check_finite(a, "sample values")?;
    check_finite(b, "sample values")?;
    check_level(level)?;
    if a.is_empty() || b.is_empty() {
        return Err(StatError::InsufficientData { required: 1, found: 0 });
    }
    let shifted: Vec<f64> = a.iter().map(|v| v - delta0).collect();
    let pooled: Vec<f64> = shifted.iter().chain(b).copied().collect();
    let (ranks, ties) = ranks_with_ties(&pooled);
    let (m, n) = (a.len(), b.len());
    let (mf, nf) = (m as f64, n as f64);
    let r1: f64 = ranks[..m].iter().sum();
    let u = r1 - mf * (mf + 1.0) / 2.0;
    let (lower_tail, upper_tail, method) = if m <= EXACT_LIMIT && n <= EXACT_LIMIT && ties == 0.0 {
        let c = rank_sum_counts(m, n);
        let total: f64 = c.iter().sum();
        let ui = u.round() as usize;
        (
            c[..=ui].iter().sum::<f64>() / total,
            c[ui..].iter().sum::<f64>() / total,
            "Wilcoxon rank-sum / Mann–Whitney (exact)",
        )
    } else {
        let big = mf + nf;
        let var = mf * nf / 12.0 * ((big + 1.0) - ties / (big * (big - 1.0)));
        if var <= 0.0 {
            return Err(invalid("all values are tied"));
        }
        let (lo, hi, _) = normal_tails(u, mf * nf / 2.0, var.sqrt(), alternative)?;
        (lo, hi, "Wilcoxon rank-sum / Mann–Whitney (normal approximation)")
    };
    let mut diffs = Vec::with_capacity(m * n);
    for x in a {
        for y in b {
            diffs.push(x - y);
        }
    }
    Ok(TestResult {
        method,
        estimate: median(&diffs),
        null_value: delta0,
        std_error: f64::NAN,
        statistic: u,
        statistic_name: "U",
        df: None,
        df2: None,
        p_value: alternative.p_value(lower_tail, upper_tail),
        confidence_interval: (f64::NAN, f64::NAN),
        level,
        alternative,
    })
}

/// Friedman rank test for `k` treatments (columns) in `n` blocks (rows of
/// `data`).
///
/// # Errors
/// [`StatError::InvalidArgument`] for fewer than two blocks or treatments,
/// ragged rows, or every block entirely tied; [`StatError::NonFiniteInput`].
pub fn friedman_test<R: AsRef<[f64]>>(data: &[R]) -> Result<ChiSquareResult> {
    let n = data.len();
    let k = data.first().map_or(0, |r| r.as_ref().len());
    if n < 2 || k < 2 {
        return Err(invalid("the Friedman test needs at least two blocks and two treatments"));
    }
    if data.iter().any(|r| r.as_ref().len() != k) {
        return Err(invalid("every block needs one value per treatment"));
    }
    let mut sums = vec![0.0; k];
    let mut ties = 0.0;
    for row in data {
        check_finite(row.as_ref(), "Friedman data")?;
        let (r, t) = ranks_with_ties(row.as_ref());
        for j in 0..k {
            sums[j] += r[j];
        }
        ties += t;
    }
    let (nf, kf) = (n as f64, k as f64);
    let correction = 1.0 - ties / (nf * kf * (kf * kf - 1.0));
    if correction <= 0.0 {
        return Err(invalid("every block is entirely tied"));
    }
    let q =
        (12.0 / (nf * kf * (kf + 1.0)) * sums.iter().map(|s| s * s).sum::<f64>() - 3.0 * nf * (kf + 1.0)) / correction;
    let df = kf - 1.0;
    Ok(ChiSquareResult { statistic: q, df, p_value: ChiSquared::new(df)?.sf(q.max(0.0))? })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::means::tests::{SLEEP_1, SLEEP_2, X, close};

    /// Published (R `wilcox.test` documentation, sleep data, paired): V = 0,
    /// p = 0.009091 with zeros dropped, ties and continuity correction.
    #[test]
    fn signed_rank() {
        let r = paired_signed_rank(&SLEEP_1, &SLEEP_2, 0.0, 0.95, Alternative::TwoSided).unwrap();
        assert_eq!(r.statistic, 0.0);
        close(r.p_value, 0.009_090_698_015_925_044, 1e-9);
        assert!(r.method.contains("normal"));
        // Exact distribution (no ties): SciPy 1.18 `wilcoxon(method="exact")`.
        let e = signed_rank_test(&X, 5.02, 0.95, Alternative::TwoSided).unwrap();
        assert_eq!(e.statistic, 48.0);
        close(e.p_value, 0.037_109_375, 1e-14);
        close(signed_rank_test(&X, 5.02, 0.95, Alternative::Greater).unwrap().p_value, 0.018_554_687_5, 1e-14);
        close(signed_rank_test(&X, 5.02, 0.95, Alternative::Less).unwrap().p_value, 0.986_328_125, 1e-14);
        close(e.estimate, 5.4, 1e-14);
        assert_eq!(signed_rank_counts(3), [1.0, 1.0, 1.0, 2.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn sign_test_and_median_interval() {
        // 6 of 9 non-tied values above 5.2: p = 2·P(B ≥ 6) = 0.5078125.
        let s = sign_test(&X, 5.2, 0.95, Alternative::TwoSided).unwrap();
        assert_eq!(s.statistic, 6.0);
        close(s.p_value, 0.507_812_5, 1e-14);
        // k = 2: (x₍₂₎, x₍₉₎) with coverage 1 − 2·P(B ≤ 1) = 0.9785.
        assert_eq!(s.confidence_interval, (4.9, 5.9));
        close(s.estimate, 5.4, 1e-15);
        let g = sign_test(&X, 5.2, 0.95, Alternative::Greater).unwrap();
        close(g.p_value, 0.253_906_25, 1e-14);
        assert_eq!(g.confidence_interval.1, f64::INFINITY);
    }

    /// SciPy 1.18 `mannwhitneyu`: exact (no ties) and asymptotic with
    /// continuity correction (sleep data, ties: U = 25.5, p = 0.06933).
    #[test]
    #[allow(clippy::approx_constant)] // 3.14 is a data value
    fn rank_sum() {
        let a = [1.83, 0.50, 1.62, 2.48, 1.68, 1.88, 1.55, 3.06, 1.30];
        let b = [0.878, 0.647, 0.598, 2.05, 1.06, 1.29, 1.07, 3.14, 1.28];
        let r = rank_sum_test(&a, &b, 0.0, 0.95, Alternative::TwoSided).unwrap();
        assert_eq!(r.statistic, 58.0);
        close(r.p_value, 0.135_911_147_675_853_55, 1e-12);
        close(rank_sum_test(&a, &b, 0.0, 0.95, Alternative::Greater).unwrap().p_value, 0.067_955_573_837_926_77, 1e-12);
        close(rank_sum_test(&a, &b, 0.0, 0.95, Alternative::Less).unwrap().p_value, 0.943_253_805_018_511, 1e-12);
        close(r.estimate, 0.56, 1e-12);
        let s = rank_sum_test(&SLEEP_1, &SLEEP_2, 0.0, 0.95, Alternative::TwoSided).unwrap();
        assert_eq!(s.statistic, 25.5);
        close(s.p_value, 0.069_327_575_433_626_58, 1e-10);
        let total: f64 = rank_sum_counts(4, 3).iter().sum();
        assert_eq!(total, 35.0, "C(7, 3) arrangements");
    }

    /// SciPy 1.18 `friedmanchisquare`.
    #[test]
    fn friedman() {
        let y = [[8.0, 7.2, 6.1], [9.1, 8.5, 7.0], [7.5, 7.9, 6.8], [8.8, 8.0, 7.9], [9.5, 8.1, 7.2], [7.9, 7.0, 7.1]];
        let f = friedman_test(&y).unwrap();
        close(f.statistic, 8.333_333_333_333_329, 1e-12);
        close(f.p_value, 0.015_503_853_599_009_356, 1e-10);
        assert_eq!(f.df, 2.0);
        assert!(friedman_test(&[[1.0, 2.0]]).is_err());
    }

    /// A statistic exactly at its null mean gets no continuity correction, so
    /// the two-sided p-value is 1 (SciPy 1.18.1 `mannwhitneyu(method=
    /// "asymptotic")` and `wilcoxon(correction=True, method="approx")`; R's
    /// `wilcox.test` likewise).
    #[test]
    fn no_correction_at_the_null_mean() {
        let r = rank_sum_test(&[1.0, 2.0], &[1.0, 2.0], 0.0, 0.95, Alternative::TwoSided).unwrap();
        assert!(r.method.contains("normal"));
        assert_eq!(r.statistic, 2.0);
        close(r.p_value, 1.0, 1e-12);
        let s = signed_rank_test(&[1.0, -1.0, 2.0, -2.0], 0.0, 0.95, Alternative::TwoSided).unwrap();
        assert!(s.method.contains("normal"));
        assert_eq!(s.statistic, 5.0);
        close(s.p_value, 1.0, 1e-12);
    }
}
