//! Comparisons of groups beyond the classical one-way ANOVA: tests that do not
//! assume equal variances, tests of equal variances, and rank-based tests.
//!
//! # Definitions
//! * **Two-sample t (pooled and Welch).** Difference of means `ȳ₁ − ȳ₂` with
//!   standard error `s_p√(1/n₁ + 1/n₂)` on `n₁ + n₂ − 2` df (pooled), or
//!   `√(s₁²/n₁ + s₂²/n₂)` on the Welch–Satterthwaite df
//!   `(s₁²/n₁ + s₂²/n₂)² / [(s₁²/n₁)²/(n₁−1) + (s₂²/n₂)²/(n₂−1)]`.
//! * **Welch ANOVA** (Welch 1951, *Biometrika* 38): weights `wᵢ = nᵢ/sᵢ²`,
//!   `ȳ_w = Σwᵢȳᵢ/Σwᵢ`, `A = Σwᵢ(ȳᵢ − ȳ_w)²/(k−1)`,
//!   `λ = Σ(1 − wᵢ/W)²/(nᵢ − 1)`, `F = A / (1 + 2(k−2)λ/(k²−1))` on
//!   `k − 1` and `(k² − 1)/(3λ)` df. For two groups it equals the Welch t².
//! * **Levene** (1960): one-way ANOVA F on `|yᵢⱼ − ȳᵢ|`; **Brown–Forsythe**
//!   (1974): the same on `|yᵢⱼ − medianᵢ|`, robust to non-normality.
//! * **Bartlett** (1937): `χ² = [(N−k) ln s_p² − Σ(nᵢ−1) ln sᵢ²] / C`,
//!   `C = 1 + (Σ 1/(nᵢ−1) − 1/(N−k)) / (3(k−1))`, on `k − 1` df.
//! * **Wilcoxon rank-sum / Mann–Whitney** (two groups): `U₁ = R₁ − n₁(n₁+1)/2`
//!   from mid-ranks; normal approximation with tie correction
//!   `σ² = n₁n₂/12 · [(N+1) − Σ(t³−t)/(N(N−1))]` and a continuity correction of ½.
//! * **Kruskal–Wallis** (1952): `H = [12/(N(N+1)) Σ Rᵢ²/nᵢ − 3(N+1)] / (1 − Σ(t³−t)/(N³−N))`,
//!   chi-square on `k − 1` df.

use crate::anova::one_way_anova;
use stat_core::descriptive::median;
use stat_core::distributions::{ChiSquared, ContinuousDistribution, FisherSnedecor, Normal, StudentsT};
use stat_core::summation::{mean, sum_iter};
use stat_core::{Result, StatError};

/// A t-test of a difference of two means.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeanDifference {
    /// `ȳ₁ − ȳ₂`.
    pub difference: f64,
    /// Standard error of the difference.
    pub std_error: f64,
    /// Degrees of freedom.
    pub df: f64,
    /// `t = difference / std_error`.
    pub t_statistic: f64,
    /// Two-sided p-value.
    pub p_value: f64,
    /// Confidence interval for the difference.
    pub confidence_interval: (f64, f64),
}

/// Pooled-variance and Welch two-sample t-tests.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TwoSampleTests {
    /// Assuming equal variances.
    pub pooled: MeanDifference,
    /// Not assuming equal variances (Welch–Satterthwaite).
    pub welch: MeanDifference,
}

/// An F test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FTest {
    /// F statistic.
    pub f_statistic: f64,
    /// Numerator degrees of freedom.
    pub df1: f64,
    /// Denominator degrees of freedom.
    pub df2: f64,
    /// Upper-tail p-value.
    pub p_value: f64,
}

/// A chi-square test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChiSquareTest {
    /// Chi-square statistic.
    pub statistic: f64,
    /// Degrees of freedom.
    pub df: f64,
    /// Upper-tail p-value.
    pub p_value: f64,
}

/// Tests that the group variances are equal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VarianceTests {
    /// Levene's test (deviations from group means).
    pub levene: FTest,
    /// Brown–Forsythe test (deviations from group medians).
    pub brown_forsythe: FTest,
    /// Bartlett's test.
    pub bartlett: ChiSquareTest,
}

/// Wilcoxon rank-sum (Mann–Whitney) test, normal approximation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RankSumTest {
    /// Sum of the mid-ranks of the first group.
    pub rank_sum: f64,
    /// Mann–Whitney `U` of the first group.
    pub u_statistic: f64,
    /// Standardized statistic (with continuity correction).
    pub z: f64,
    /// Two-sided p-value.
    pub p_value: f64,
}

fn check_groups<G: AsRef<[f64]>>(groups: &[G], min_size: usize) -> Result<()> {
    if groups.len() < 2 {
        return Err(StatError::InvalidArgument { message: "at least two groups are needed".into() });
    }
    if groups.iter().flat_map(|g| g.as_ref()).any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "group comparison data" });
    }
    if let Some(g) = groups.iter().find(|g| g.as_ref().len() < min_size) {
        return Err(StatError::InsufficientData { required: min_size, found: g.as_ref().len() });
    }
    Ok(())
}

/// Sample mean and variance (two-pass).
fn mean_var(x: &[f64]) -> (f64, f64) {
    let m = mean(x).unwrap_or(f64::NAN);
    let ss = sum_iter(x.iter().map(|v| (v - m) * (v - m)));
    (m, ss / (x.len() as f64 - 1.0))
}

fn mean_difference(difference: f64, std_error: f64, df: f64, level: f64) -> Result<MeanDifference> {
    let t_dist = StudentsT::new(df)?;
    let t_statistic = difference / std_error;
    let crit = t_dist.quantile(0.5 + level / 2.0)?;
    Ok(MeanDifference {
        difference,
        std_error,
        df,
        t_statistic,
        p_value: t_dist.two_sided_p_value(t_statistic)?,
        confidence_interval: (difference - crit * std_error, difference + crit * std_error),
    })
}

/// Pooled and Welch t-tests of `mean(a) − mean(b)`, with `level` confidence intervals.
///
/// # Errors
/// [`StatError::InsufficientData`] if a group has fewer than two values;
/// [`StatError::InvalidArgument`] if both groups have zero variance or `level ∉ (0, 1)`;
/// [`StatError::NonFiniteInput`] for NaN/±∞.
pub fn two_sample_t_tests(a: &[f64], b: &[f64], level: f64) -> Result<TwoSampleTests> {
    check_groups(&[a, b], 2)?;
    if !(level > 0.0 && level < 1.0) {
        return Err(StatError::InvalidArgument {
            message: format!("confidence level must lie in (0, 1) (got {level})"),
        });
    }
    let (n1, n2) = (a.len() as f64, b.len() as f64);
    let ((m1, v1), (m2, v2)) = (mean_var(a), mean_var(b));
    if v1 == 0.0 && v2 == 0.0 {
        return Err(StatError::InvalidArgument { message: "both groups have zero variance".into() });
    }
    let diff = m1 - m2;
    let df_pooled = n1 + n2 - 2.0;
    let sp2 = ((n1 - 1.0) * v1 + (n2 - 1.0) * v2) / df_pooled;
    let pooled = mean_difference(diff, (sp2 * (1.0 / n1 + 1.0 / n2)).sqrt(), df_pooled, level)?;
    let (q1, q2) = (v1 / n1, v2 / n2);
    let df_welch = (q1 + q2) * (q1 + q2) / (q1 * q1 / (n1 - 1.0) + q2 * q2 / (n2 - 1.0));
    let welch = mean_difference(diff, (q1 + q2).sqrt(), df_welch, level)?;
    Ok(TwoSampleTests { pooled, welch })
}

/// Welch's ANOVA for equality of means without assuming equal variances.
///
/// # Errors
/// [`StatError::InsufficientData`] if a group has fewer than two values;
/// [`StatError::InvalidArgument`] for fewer than two groups or a group with
/// zero variance; [`StatError::NonFiniteInput`] for NaN/±∞.
pub fn welch_anova<G: AsRef<[f64]>>(groups: &[G]) -> Result<FTest> {
    check_groups(groups, 2)?;
    let k = groups.len() as f64;
    let stats: Vec<(f64, f64, f64)> = groups
        .iter()
        .map(|g| {
            let (m, v) = mean_var(g.as_ref());
            (g.as_ref().len() as f64, m, v)
        })
        .collect();
    if stats.iter().any(|&(_, _, v)| v <= 0.0) {
        return Err(StatError::InvalidArgument {
            message: "Welch's test needs every group to have non-zero variance".into(),
        });
    }
    let w: Vec<f64> = stats.iter().map(|&(n, _, v)| n / v).collect();
    let w_total = sum_iter(w.iter().copied());
    let weighted_mean = sum_iter(w.iter().zip(&stats).map(|(wi, &(_, m, _))| wi * m)) / w_total;
    let a = sum_iter(w.iter().zip(&stats).map(|(wi, &(_, m, _))| wi * (m - weighted_mean) * (m - weighted_mean)))
        / (k - 1.0);
    let lambda = sum_iter(w.iter().zip(&stats).map(|(wi, &(n, _, _))| {
        let r = 1.0 - wi / w_total;
        r * r / (n - 1.0)
    }));
    let b = 1.0 + 2.0 * (k - 2.0) * lambda / (k * k - 1.0);
    let f = a / b;
    let (df1, df2) = (k - 1.0, (k * k - 1.0) / (3.0 * lambda));
    Ok(FTest { f_statistic: f, df1, df2, p_value: FisherSnedecor::new(df1, df2)?.sf(f)? })
}

/// Levene, Brown–Forsythe and Bartlett tests of equal variances.
///
/// # Errors
/// [`StatError::InsufficientData`] if a group has fewer than two values;
/// [`StatError::InvalidArgument`] for fewer than two groups or a group with
/// zero variance (Bartlett's statistic is undefined); [`StatError::NonFiniteInput`].
pub fn variance_tests<G: AsRef<[f64]>>(groups: &[G]) -> Result<VarianceTests> {
    check_groups(groups, 2)?;
    let anova_f = |centers: &[f64]| -> Result<FTest> {
        let dev: Vec<Vec<f64>> =
            groups.iter().zip(centers).map(|(g, c)| g.as_ref().iter().map(|v| (v - c).abs()).collect()).collect();
        let a = one_way_anova(&dev)?;
        // Sums of squares below 10⁻¹² of the deviations' own scale are
        // rounding noise (|100.1 − 100.15| is not exactly 0.05).
        let tiny = 1e-12 * dev.iter().flatten().map(|d| d * d).sum::<f64>();
        let (f, p) = match (a.f_statistic, a.p_value) {
            (Some(f), Some(p)) if a.ss_within > tiny => (f, p),
            // No spread of the deviations within groups (every group of two
            // values does this). Groups whose deviations still differ in
            // level differ in spread with certainty: F = ∞, p = 0. Identical
            // deviations everywhere give no evidence of unequal variance.
            _ => {
                if a.ss_between > tiny {
                    (f64::INFINITY, 0.0)
                } else {
                    (0.0, 1.0)
                }
            }
        };
        Ok(FTest { f_statistic: f, df1: a.df_between as f64, df2: a.df_within as f64, p_value: p })
    };
    let means: Vec<f64> = groups.iter().map(|g| mean(g.as_ref()).unwrap_or(f64::NAN)).collect();
    let medians: Vec<f64> = groups.iter().map(|g| median(g.as_ref())).collect::<Result<_>>()?;
    let levene = anova_f(&means)?;
    let brown_forsythe = anova_f(&medians)?;

    let k = groups.len() as f64;
    let ns: Vec<f64> = groups.iter().map(|g| g.as_ref().len() as f64).collect();
    let vars: Vec<f64> = groups.iter().map(|g| mean_var(g.as_ref()).1).collect();
    if vars.iter().any(|&v| v <= 0.0) {
        return Err(StatError::InvalidArgument {
            message: "Bartlett's test needs every group to have non-zero variance".into(),
        });
    }
    let n_total: f64 = ns.iter().sum();
    let sp2 = sum_iter(ns.iter().zip(&vars).map(|(n, v)| (n - 1.0) * v)) / (n_total - k);
    let numerator = (n_total - k) * sp2.ln() - sum_iter(ns.iter().zip(&vars).map(|(n, v)| (n - 1.0) * v.ln()));
    let c = 1.0 + (sum_iter(ns.iter().map(|n| 1.0 / (n - 1.0))) - 1.0 / (n_total - k)) / (3.0 * (k - 1.0));
    let statistic = numerator / c;
    let bartlett = ChiSquareTest { statistic, df: k - 1.0, p_value: ChiSquared::new(k - 1.0)?.sf(statistic)? };
    Ok(VarianceTests { levene, brown_forsythe, bartlett })
}

/// Mid-ranks of all values pooled across groups, returned per group, plus the
/// tie term `Σ(t³ − t)`.
fn pooled_ranks<G: AsRef<[f64]>>(groups: &[G]) -> (Vec<Vec<f64>>, f64) {
    let mut all: Vec<(f64, usize, usize)> = groups
        .iter()
        .enumerate()
        .flat_map(|(g, x)| x.as_ref().iter().enumerate().map(move |(i, &v)| (v, g, i)))
        .collect();
    all.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut ranks: Vec<Vec<f64>> = groups.iter().map(|g| vec![0.0; g.as_ref().len()]).collect();
    let mut ties = 0.0;
    let mut start = 0;
    while start < all.len() {
        let mut end = start + 1;
        while end < all.len() && all[end].0 == all[start].0 {
            end += 1;
        }
        let t = (end - start) as f64;
        let rank = (start + end + 1) as f64 / 2.0; // mean of ranks start+1 ..= end
        for &(_, g, i) in &all[start..end] {
            ranks[g][i] = rank;
        }
        ties += t * t * t - t;
        start = end;
    }
    (ranks, ties)
}

/// Wilcoxon rank-sum (Mann–Whitney) test of `a` against `b`.
///
/// # Errors
/// [`StatError::InsufficientData`] for an empty group; [`StatError::InvalidArgument`]
/// if all values are tied; [`StatError::NonFiniteInput`] for NaN/±∞.
pub fn rank_sum_test(a: &[f64], b: &[f64]) -> Result<RankSumTest> {
    check_groups(&[a, b], 1)?;
    let (ranks, ties) = pooled_ranks(&[a, b]);
    let (n1, n2) = (a.len() as f64, b.len() as f64);
    let n = n1 + n2;
    let rank_sum = sum_iter(ranks[0].iter().copied());
    let u = rank_sum - n1 * (n1 + 1.0) / 2.0;
    let variance = n1 * n2 / 12.0 * ((n + 1.0) - ties / (n * (n - 1.0)));
    if variance <= 0.0 {
        return Err(StatError::InvalidArgument { message: "all values are tied".into() });
    }
    let centered = u - n1 * n2 / 2.0;
    let corrected = (centered.abs() - 0.5).max(0.0) * centered.signum();
    let z = corrected / variance.sqrt();
    let p_value = (2.0 * Normal::standard().sf(z.abs())?).min(1.0);
    Ok(RankSumTest { rank_sum, u_statistic: u, z, p_value })
}

/// Kruskal–Wallis rank test that all groups come from the same distribution.
///
/// # Errors
/// [`StatError::InvalidArgument`] for fewer than two groups or all values
/// tied; [`StatError::InsufficientData`] for an empty group;
/// [`StatError::NonFiniteInput`] for NaN/±∞.
pub fn kruskal_wallis<G: AsRef<[f64]>>(groups: &[G]) -> Result<ChiSquareTest> {
    check_groups(groups, 1)?;
    let (ranks, ties) = pooled_ranks(groups);
    let n: f64 = groups.iter().map(|g| g.as_ref().len() as f64).sum();
    let correction = 1.0 - ties / (n * n * n - n);
    if correction <= 0.0 {
        return Err(StatError::InvalidArgument { message: "all values are tied".into() });
    }
    let sum_term = sum_iter(ranks.iter().map(|r| {
        let s = sum_iter(r.iter().copied());
        s * s / r.len() as f64
    }));
    let h = (12.0 / (n * (n + 1.0)) * sum_term - 3.0 * (n + 1.0)) / correction;
    let df = groups.len() as f64 - 1.0;
    Ok(ChiSquareTest { statistic: h, df, p_value: ChiSquared::new(df)?.sf(h.max(0.0))? })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reference values from SciPy 1.18 (`ttest_ind`, `levene`, `bartlett`,
    // `mannwhitneyu(method="asymptotic")`, `kruskal`) and Welch's formula
    // evaluated independently.
    const G1: [f64; 7] = [4.2, 4.8, 5.1, 3.9, 4.4, 5.0, 4.6];
    const G2: [f64; 6] = [5.6, 6.1, 5.9, 6.8, 5.2, 6.4];
    const G3: [f64; 8] = [4.9, 5.0, 5.0, 5.3, 4.7, 5.2, 5.1, 4.8];

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1e-300)
    }

    #[test]
    fn two_sample_t() {
        let t = two_sample_t_tests(&G1, &G2, 0.95).unwrap();
        assert!(close(t.pooled.t_statistic, -5.131_528_872_885_009, 1e-12));
        assert!(close(t.pooled.p_value, 3.275_288_222_541_052_4e-4, 1e-9));
        assert_eq!(t.pooled.df, 11.0);
        assert!(close(t.pooled.confidence_interval.0, -2.041_305_877_174_245, 1e-10));
        assert!(close(t.welch.t_statistic, -5.019_277_583_865_016, 1e-12));
        assert!(close(t.welch.df, 9.311_099_684_902_388, 1e-12));
        assert!(close(t.welch.p_value, 6.491_581_385_528_021e-4, 1e-8));
        assert!(close(t.welch.confidence_interval.1, -0.787_987_509_818_761_2, 1e-9));
    }

    #[test]
    fn welch_anova_matches_and_reduces_to_welch_t() {
        let w = welch_anova(&[&G1[..], &G2, &G3]).unwrap();
        assert!(close(w.f_statistic, 11.761_886_254_730_188, 1e-12));
        assert!(close(w.df2, 8.868_778_353_201_026, 1e-12));
        assert!(close(w.p_value, 3.201_071_382_717_803_4e-3, 1e-8));
        let w2 = welch_anova(&[&G1[..], &G2]).unwrap();
        let t = two_sample_t_tests(&G1, &G2, 0.95).unwrap().welch;
        assert!(close(w2.f_statistic, t.t_statistic * t.t_statistic, 1e-12));
        assert!(close(w2.df2, t.df, 1e-12));
    }

    #[test]
    fn variance_tests_match() {
        let v = variance_tests(&[&G1[..], &G2, &G3]).unwrap();
        assert!(close(v.levene.f_statistic, 3.069_439_564_300_772_4, 1e-12));
        assert!(close(v.levene.p_value, 0.071_285_080_638_160_75, 1e-9));
        assert!(close(v.brown_forsythe.f_statistic, 2.956_066_945_606_695, 1e-12));
        assert!(close(v.brown_forsythe.p_value, 0.077_604_613_781_663_92, 1e-9));
        assert!(close(v.bartlett.statistic, 5.684_507_221_441_76, 1e-12));
        assert!(close(v.bartlett.p_value, 0.058_294_145_510_924_16, 1e-9));
    }

    #[test]
    fn rank_tests_match() {
        let r = rank_sum_test(&G1, &G2).unwrap();
        assert_eq!(r.u_statistic, 0.0);
        assert!(close(r.p_value, 3.405_235_821_294_760_4e-3, 1e-9));
        let k = kruskal_wallis(&[&G1[..], &G2, &G3]).unwrap();
        assert!(close(k.statistic, 13.710_162_147_050_601, 1e-12));
        assert!(close(k.p_value, 1.054_086_173_648_43e-3, 1e-9));
        // With ties.
        let c = [1.0, 2.0, 2.0, 3.0, 3.0, 3.0, 4.0, 7.0];
        let d = [2.0, 3.0, 4.0, 4.0, 5.0, 6.0, 6.0, 8.0, 9.0];
        let r = rank_sum_test(&c, &d).unwrap();
        assert_eq!(r.u_statistic, 15.5);
        assert!(close(r.p_value, 0.051_498_148_671_545_51, 1e-9));
        let k = kruskal_wallis(&[&c[..], &d]).unwrap();
        assert!(close(k.statistic, 3.983_967_656_489_612_7, 1e-12));
        assert!(close(k.p_value, 0.045_935_240_981_598_31, 1e-9));
    }

    #[test]
    fn rejects_bad_input() {
        assert!(two_sample_t_tests(&[1.0], &[2.0, 3.0], 0.95).is_err());
        assert!(two_sample_t_tests(&[1.0, 1.0], &[2.0, 2.0], 0.95).is_err());
        assert!(welch_anova(&[&[1.0, 2.0][..]]).is_err());
        assert!(welch_anova(&[&[1.0, 1.0][..], &[1.0, 2.0]]).is_err());
        assert!(rank_sum_test(&[1.0, 1.0], &[1.0]).is_err());
        assert!(kruskal_wallis(&[&[f64::NAN][..], &[1.0]]).is_err());
    }

    /// Groups of two values have equal absolute deviations within each
    /// group, so the ANOVA of deviations has no within-group spread. With
    /// variances 2 and 50 that is certain evidence of unequal spread:
    /// SciPy's `levene` (center "mean" and "median") gives F = inf, p = 0.
    #[test]
    fn pairs_with_unequal_spread_give_infinite_f() {
        let v = variance_tests(&[vec![1.0, 3.0], vec![0.0, 10.0]]).unwrap();
        for t in [&v.levene, &v.brown_forsythe] {
            assert!(t.f_statistic.is_infinite(), "{t:?}");
            assert_eq!(t.p_value, 0.0);
        }
        let same = variance_tests(&[vec![1.0, 3.0], vec![5.0, 7.0]]).unwrap();
        assert_eq!((same.levene.f_statistic, same.levene.p_value), (0.0, 1.0));
    }

    /// Groups of two values with equal spreads ([100.1, 100.2] and [0, 0.1]):
    /// the absolute deviations are 0.05 everywhere, so Levene's and the
    /// Brown–Forsythe test show no difference (F = 0, p = 1). Rounding noise
    /// in 100.1 − 100.15 used to give F = 0.36, p = 0.61.
    #[test]
    fn equal_two_value_groups_give_no_evidence() {
        let v = variance_tests(&[vec![100.1, 100.2], vec![0.0, 0.1]]).unwrap();
        for t in [&v.levene, &v.brown_forsythe] {
            assert!(t.f_statistic == 0.0 && t.p_value == 1.0, "{t:?}");
        }
    }
}
