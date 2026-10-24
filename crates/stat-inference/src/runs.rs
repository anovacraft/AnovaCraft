//! Mood's median test and tests of randomness based on runs (Conover,
//! *Practical Nonparametric Statistics*, 3rd ed., 1999, §4.3 and §3.5;
//! Bradley, *Distribution-Free Statistical Tests*, 1968, ch. 11).
//!
//! * **Mood's median test** for `k` groups: each value is classed as above the
//!   grand median or not (values equal to it count as not above by default,
//!   as in Conover; they may instead count as above or be left out). The
//!   2 × k table of counts gives Pearson's
//!   `χ² = Σ (O − E)²/E` on `k − 1` df.
//! * **Runs above and below a reference** (Wald & Wolfowitz 1940): values
//!   equal to the reference are dropped; with `n₁` above, `n₂` below and `R`
//!   runs, the exact null distribution is
//!   `P(R = 2s) = 2·C(n₁−1, s−1)C(n₂−1, s−1)/C(n, n₁)` and
//!   `P(R = 2s+1) = [C(n₁−1, s)C(n₂−1, s−1) + C(n₁−1, s−1)C(n₂−1, s)]/C(n, n₁)`,
//!   with mean `1 + 2n₁n₂/n` and variance `2n₁n₂(2n₁n₂ − n)/(n²(n − 1))`.
//!   Too few runs means clustering; too many, mixing.
//! * **Runs up and down** (Levene & Wolfowitz 1944; Edgington 1961): the
//!   number `V` of runs of increases and decreases among successive values
//!   (repeats of the previous value dropped). For `n` distinct values in
//!   random order, the probabilities follow the recursion
//!   `P(n, v) = [v·P(n−1, v) + 2·P(n−1, v−1) + (n − v)·P(n−1, v−2)]/n`
//!   from `P(2, 1) = 1`; the mean is `(2n − 1)/3` and the variance
//!   `(16n − 29)/90`. Exact for up to [`UP_DOWN_EXACT_LIMIT`] values, normal
//!   (with a continuity correction) beyond. Too few runs means a trend; too
//!   many, oscillation.

use crate::common::{Alternative, ChiSquareResult, check_finite, invalid, median};
use stat_core::distributions::{ChiSquared, ContinuousDistribution, Normal};
use stat_core::special::ln_gamma;
use stat_core::{Result, StatError};

/// How values equal to the grand median are counted in [`median_test`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum MedianTies {
    /// With the values below the median (Conover).
    #[default]
    Below,
    /// With the values above the median.
    Above,
    /// Left out.
    Ignore,
}

/// Result of Mood's median test.
#[derive(Debug, Clone, PartialEq)]
pub struct MedianTest {
    /// Grand median of all values.
    pub grand_median: f64,
    /// Values above the median, per group.
    pub above: Vec<usize>,
    /// Values not above (below, or tied when counted below), per group.
    pub below: Vec<usize>,
    /// Pearson's chi-square test of the 2 × k table.
    pub test: ChiSquareResult,
    /// Smallest expected count (the chi-square approximation is doubtful
    /// below 1, or when many expected counts are below 5).
    pub min_expected: f64,
}

/// Mood's median test that `groups` share one median.
///
/// # Errors
/// [`StatError::InvalidArgument`] for fewer than two groups or a table with
/// an empty row or column; [`StatError::NonFiniteInput`] for NaN/±∞.
pub fn median_test<G: AsRef<[f64]>>(groups: &[G], ties: MedianTies) -> Result<MedianTest> {
    if groups.len() < 2 {
        return Err(invalid("the median test needs at least two groups"));
    }
    for g in groups {
        check_finite(g.as_ref(), "group values")?;
    }
    let all: Vec<f64> = groups.iter().flat_map(|g| g.as_ref().iter().copied()).collect();
    if all.is_empty() {
        return Err(StatError::InsufficientData { required: 1, found: 0 });
    }
    let grand_median = median(&all);
    let mut above = Vec::with_capacity(groups.len());
    let mut below = Vec::with_capacity(groups.len());
    for g in groups {
        let g = g.as_ref();
        let over = g.iter().filter(|&&v| v > grand_median).count();
        let under = g.iter().filter(|&&v| v < grand_median).count();
        let tied = g.len() - over - under;
        match ties {
            MedianTies::Below => {
                above.push(over);
                below.push(under + tied);
            }
            MedianTies::Above => {
                above.push(over + tied);
                below.push(under);
            }
            MedianTies::Ignore => {
                above.push(over);
                below.push(under);
            }
        }
    }
    let a: usize = above.iter().sum();
    let b: usize = below.iter().sum();
    let totals: Vec<usize> = above.iter().zip(&below).map(|(x, y)| x + y).collect();
    if a == 0 || b == 0 || totals.contains(&0) {
        return Err(invalid("the table of counts has an empty row or column"));
    }
    let n = (a + b) as f64;
    let mut chi2 = 0.0;
    let mut min_expected = f64::INFINITY;
    for (j, &t) in totals.iter().enumerate() {
        for (observed, row) in [(above[j], a), (below[j], b)] {
            let e = row as f64 * t as f64 / n;
            min_expected = min_expected.min(e);
            chi2 += (observed as f64 - e).powi(2) / e;
        }
    }
    let df = (groups.len() - 1) as f64;
    let p_value = ChiSquared::new(df)?.sf(chi2)?;
    Ok(MedianTest { grand_median, above, below, test: ChiSquareResult { statistic: chi2, df, p_value }, min_expected })
}

/// A runs test.
#[derive(Debug, Clone, PartialEq)]
pub struct RunsTest {
    /// Number of runs observed.
    pub runs: usize,
    /// Values used (after dropping ties with the reference or the previous value).
    pub n: usize,
    /// Values above the reference (runs above and below; 0 for up and down).
    pub above: usize,
    /// Values below the reference (runs above and below; 0 for up and down).
    pub below: usize,
    /// Reference value (runs above and below; NaN for up and down).
    pub reference: f64,
    /// Expected number of runs.
    pub expected: f64,
    /// Standard deviation of the number of runs.
    pub std_dev: f64,
    /// `P(runs ≤ observed)`: small for too few runs (clustering, trends).
    pub p_fewer: f64,
    /// `P(runs ≥ observed)`: small for too many runs (mixing, oscillation).
    pub p_more: f64,
    /// p-value for the alternative asked for (`Less`: too few runs,
    /// `Greater`: too many, `TwoSided`: twice the smaller tail).
    pub p_value: f64,
    /// Whether the p-values are exact (else the normal approximation).
    pub exact: bool,
}

fn ln_choose(n: usize, k: usize) -> f64 {
    ln_gamma(n as f64 + 1.0) - ln_gamma(k as f64 + 1.0) - ln_gamma((n - k) as f64 + 1.0)
}

/// Exact null probabilities `P(R = r)`, `r = 0, …, n`, of the number of runs
/// of `n1` and `n2` symbols in random order.
#[must_use]
pub fn two_symbol_runs_distribution(n1: usize, n2: usize) -> Vec<f64> {
    let n = n1 + n2;
    let mut p = vec![0.0; n + 1];
    if n1 == 0 || n2 == 0 {
        if n > 0 {
            p[1] = 1.0;
        }
        return p;
    }
    let denom = ln_choose(n, n1);
    for (r, slot) in p.iter_mut().enumerate().skip(2) {
        let s = r / 2;
        let ways = if r % 2 == 0 {
            if s > n1 || s > n2 {
                0.0
            } else {
                2.0 * (ln_choose(n1 - 1, s - 1) + ln_choose(n2 - 1, s - 1) - denom).exp()
            }
        } else {
            let a =
                if s < n1 && s <= n2 { (ln_choose(n1 - 1, s) + ln_choose(n2 - 1, s - 1) - denom).exp() } else { 0.0 };
            let b =
                if s <= n1 && s < n2 { (ln_choose(n1 - 1, s - 1) + ln_choose(n2 - 1, s) - denom).exp() } else { 0.0 };
            a + b
        };
        *slot = ways;
    }
    p
}

fn tails(dist: &[f64], observed: usize) -> (f64, f64) {
    let fewer: f64 = dist[..=observed].iter().sum();
    let more: f64 = dist[observed..].iter().sum();
    (fewer.min(1.0), more.min(1.0))
}

/// Runs of values above and below `reference` (the sample median when `None`).
///
/// # Errors
/// [`StatError::NonFiniteInput`] for NaN/±∞; [`StatError::InsufficientData`]
/// unless at least one value lies on each side of the reference.
pub fn runs_test(x: &[f64], reference: Option<f64>, alternative: Alternative) -> Result<RunsTest> {
    check_finite(x, "sample values")?;
    if x.is_empty() {
        return Err(StatError::InsufficientData { required: 2, found: 0 });
    }
    let reference = reference.unwrap_or_else(|| median(x));
    if !reference.is_finite() {
        return Err(StatError::NonFiniteInput { context: "reference value" });
    }
    let signs: Vec<bool> = x.iter().filter(|&&v| v != reference).map(|&v| v > reference).collect();
    let above = signs.iter().filter(|&&s| s).count();
    let below = signs.len() - above;
    if above == 0 || below == 0 {
        return Err(StatError::InsufficientData { required: 2, found: signs.len().min(1) });
    }
    let runs = 1 + signs.windows(2).filter(|w| w[0] != w[1]).count();
    let n = signs.len();
    let (n1, n2, nf) = (above as f64, below as f64, n as f64);
    let expected = 1.0 + 2.0 * n1 * n2 / nf;
    let var = 2.0 * n1 * n2 * (2.0 * n1 * n2 - nf) / (nf * nf * (nf - 1.0));
    let dist = two_symbol_runs_distribution(above, below);
    let (p_fewer, p_more) = tails(&dist, runs);
    Ok(RunsTest {
        runs,
        n,
        above,
        below,
        reference,
        expected,
        std_dev: var.max(0.0).sqrt(),
        p_fewer,
        p_more,
        p_value: alternative.p_value(p_fewer, p_more),
        exact: true,
    })
}

/// Largest number of values for the exact distribution of runs up and down.
pub const UP_DOWN_EXACT_LIMIT: usize = 400;

/// Exact null probabilities `P(V = v)`, `v = 0, …, n − 1`, of the number of
/// runs up and down in a random ordering of `n ≥ 2` distinct values.
#[must_use]
pub fn up_down_runs_distribution(n: usize) -> Vec<f64> {
    if n < 2 {
        return vec![1.0];
    }
    // p[v] for the current length, starting at n = 2.
    let mut p = vec![0.0, 1.0];
    for m in 3..=n {
        let mf = m as f64;
        let mut next = vec![0.0; m];
        for (v, slot) in next.iter_mut().enumerate().skip(1) {
            let same = if v < p.len() { v as f64 * p[v] } else { 0.0 };
            let one = if v >= 1 && v - 1 < p.len() { 2.0 * p[v - 1] } else { 0.0 };
            let two = if v >= 2 && v - 2 < p.len() { (mf - v as f64) * p[v - 2] } else { 0.0 };
            *slot = (same + one + two) / mf;
        }
        p = next;
    }
    p
}

/// Runs up and down among successive values of `x` (a value equal to the one
/// before it is dropped).
///
/// # Errors
/// [`StatError::NonFiniteInput`] for NaN/±∞; [`StatError::InsufficientData`]
/// for fewer than three distinct successive values.
pub fn up_down_runs_test(x: &[f64], alternative: Alternative) -> Result<RunsTest> {
    check_finite(x, "sample values")?;
    let mut values: Vec<f64> = Vec::with_capacity(x.len());
    for &v in x {
        if values.last() != Some(&v) {
            values.push(v);
        }
    }
    let n = values.len();
    if n < 3 {
        return Err(StatError::InsufficientData { required: 3, found: n });
    }
    let ups: Vec<bool> = values.windows(2).map(|w| w[1] > w[0]).collect();
    let runs = 1 + ups.windows(2).filter(|w| w[0] != w[1]).count();
    let nf = n as f64;
    let expected = (2.0 * nf - 1.0) / 3.0;
    let var = (16.0 * nf - 29.0) / 90.0;
    let (p_fewer, p_more, exact) = if n <= UP_DOWN_EXACT_LIMIT {
        let dist = up_down_runs_distribution(n);
        let (a, b) = tails(&dist, runs);
        (a, b, true)
    } else {
        let sd = var.sqrt();
        let z = Normal::standard();
        (z.cdf((runs as f64 + 0.5 - expected) / sd)?, z.sf((runs as f64 - 0.5 - expected) / sd)?, false)
    };
    Ok(RunsTest {
        runs,
        n,
        above: 0,
        below: 0,
        reference: f64::NAN,
        expected,
        std_dev: var.sqrt(),
        p_fewer,
        p_more,
        p_value: alternative.p_value(p_fewer, p_more),
        exact,
    })
}

/// The four run-chart tests of a sequence: runs about the median (too few:
/// clustering; too many: mixing) and runs up and down (too few: a trend; too
/// many: oscillation), each as a one-sided p-value.
#[derive(Debug, Clone, PartialEq)]
pub struct RunChartTests {
    /// Runs about the median.
    pub about_median: RunsTest,
    /// Runs up and down.
    pub up_down: RunsTest,
}

impl RunChartTests {
    /// p-value for clustering (too few runs about the median).
    #[must_use]
    pub fn clustering(&self) -> f64 {
        self.about_median.p_fewer
    }

    /// p-value for mixing (too many runs about the median).
    #[must_use]
    pub fn mixing(&self) -> f64 {
        self.about_median.p_more
    }

    /// p-value for a trend (too few runs up and down).
    #[must_use]
    pub fn trend(&self) -> f64 {
        self.up_down.p_fewer
    }

    /// p-value for oscillation (too many runs up and down).
    #[must_use]
    pub fn oscillation(&self) -> f64 {
        self.up_down.p_more
    }
}

/// Both runs tests of a run chart's sequence `x`.
///
/// # Errors
/// As [`runs_test`] and [`up_down_runs_test`].
pub fn run_chart_tests(x: &[f64]) -> Result<RunChartTests> {
    Ok(RunChartTests {
        about_median: runs_test(x, None, Alternative::TwoSided)?,
        up_down: up_down_runs_test(x, Alternative::TwoSided)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Grouped data checked against SciPy 1.18's `scipy.stats.median_test`.
    #[test]
    fn median_test_matches_scipy() {
        let g1 = [10.0, 14.0, 14.0, 18.0, 20.0, 22.0, 24.0, 25.0, 31.0, 31.0, 32.0, 39.0, 43.0, 43.0, 48.0, 49.0];
        let g2 = [28.0, 30.0, 31.0, 33.0, 34.0, 35.0, 36.0, 40.0, 44.0, 55.0, 57.0, 61.0, 91.0, 92.0, 99.0];
        let g3 = [0.0, 3.0, 9.0, 22.0, 23.0, 25.0, 25.0, 33.0, 34.0, 34.0, 40.0, 45.0, 46.0, 48.0, 62.0, 67.0, 84.0];
        let groups = [&g1[..], &g2[..], &g3[..]];
        let t = median_test(&groups, MedianTies::Below).unwrap();
        assert_eq!(t.grand_median, 34.0);
        assert_eq!(t.above, [5, 10, 7]);
        assert_eq!(t.below, [11, 5, 10]);
        assert!((t.test.statistic - 4.141505553270259).abs() < 1e-10);
        assert!((t.test.p_value - 0.12609082774093244).abs() < 1e-10);
        let a = median_test(&groups, MedianTies::Above).unwrap();
        assert_eq!(a.above, [5, 11, 9]);
        assert!((a.test.statistic - 5.5017084398976985).abs() < 1e-10);
        assert!((a.test.p_value - 0.06387327606955327).abs() < 1e-10);
        let i = median_test(&groups, MedianTies::Ignore).unwrap();
        assert_eq!(i.above.iter().sum::<usize>() + i.below.iter().sum::<usize>(), 45);
        assert!(median_test(&[&g1[..]], MedianTies::Below).is_err());
    }

    /// Every arrangement of `n1` ones and `n2` zeros, counted directly.
    #[test]
    fn two_symbol_distribution_matches_enumeration() {
        for (n1, n2) in [(1, 1), (2, 3), (4, 4), (5, 7), (3, 9)] {
            let n = n1 + n2;
            let mut counts = vec![0.0; n + 1];
            let mut total = 0.0;
            for mask in 0u32..(1 << n) {
                if mask.count_ones() as usize != n1 {
                    continue;
                }
                let runs = 1 + (0..n - 1).filter(|&i| ((mask >> i) & 1) != ((mask >> (i + 1)) & 1)).count();
                counts[runs] += 1.0;
                total += 1.0;
            }
            let d = two_symbol_runs_distribution(n1, n2);
            for r in 0..=n {
                assert!((d[r] - counts[r] / total).abs() < 1e-12, "n1={n1} n2={n2} r={r}");
            }
        }
    }

    fn permutations(n: usize) -> Vec<Vec<usize>> {
        if n == 1 {
            return vec![vec![0]];
        }
        let mut out = Vec::new();
        for p in permutations(n - 1) {
            for pos in 0..=p.len() {
                let mut q = p.clone();
                q.insert(pos, n - 1);
                out.push(q);
            }
        }
        out
    }

    /// Every permutation of up to 8 values, counted directly; and the
    /// published mean and variance for a larger n.
    #[test]
    fn up_down_distribution_matches_enumeration() {
        for n in 2..=8 {
            let perms = permutations(n);
            let mut counts = vec![0.0; n];
            for p in &perms {
                let ups: Vec<bool> = p.windows(2).map(|w| w[1] > w[0]).collect();
                counts[1 + ups.windows(2).filter(|w| w[0] != w[1]).count()] += 1.0;
            }
            let d = up_down_runs_distribution(n);
            for v in 0..n {
                assert!((d[v] - counts[v] / perms.len() as f64).abs() < 1e-12, "n={n} v={v}");
            }
        }
        let n = 60;
        let d = up_down_runs_distribution(n);
        let mean: f64 = d.iter().enumerate().map(|(v, p)| v as f64 * p).sum();
        let var: f64 = d.iter().enumerate().map(|(v, p)| (v as f64 - mean).powi(2) * p).sum();
        assert!((d.iter().sum::<f64>() - 1.0).abs() < 1e-12);
        assert!((mean - (2.0 * n as f64 - 1.0) / 3.0).abs() < 1e-10);
        assert!((var - (16.0 * n as f64 - 29.0) / 90.0).abs() < 1e-10);
    }

    #[test]
    fn runs_tests_on_sequences() {
        // Alternating about the median: the most runs possible.
        let x = [1.0, 9.0, 2.0, 8.0, 3.0, 7.0, 4.0, 6.0];
        let t = runs_test(&x, None, Alternative::Greater).unwrap();
        assert_eq!((t.runs, t.above, t.below), (8, 4, 4));
        assert!((t.p_value - 2.0 / 70.0).abs() < 1e-12);
        assert!((t.expected - 5.0).abs() < 1e-12);
        // Sorted values: two runs (clustering).
        let s: Vec<f64> = (0..10).map(f64::from).collect();
        let c = runs_test(&s, None, Alternative::Less).unwrap();
        assert_eq!(c.runs, 2);
        assert!((c.p_value - 2.0 / 252.0).abs() < 1e-12);
        // A reference value: ties with it are dropped.
        let r = runs_test(&[1.0, 5.0, 5.0, 9.0], Some(5.0), Alternative::TwoSided).unwrap();
        assert_eq!((r.n, r.runs), (2, 2));
        assert!(runs_test(&[1.0, 1.0], None, Alternative::TwoSided).is_err());
        // Up and down: a steady trend is one run.
        let u = up_down_runs_test(&s, Alternative::Less).unwrap();
        assert_eq!(u.runs, 1);
        assert!((u.p_value - 2.0 / 3_628_800.0).abs() < 1e-15);
        // Repeats of the previous value are dropped.
        let o = up_down_runs_test(&[1.0, 3.0, 3.0, 2.0, 4.0, 1.0, 5.0], Alternative::Greater).unwrap();
        assert_eq!((o.n, o.runs), (6, 5));
        assert!(up_down_runs_test(&[1.0, 1.0, 2.0], Alternative::TwoSided).is_err());
        // Beyond the exact limit: the normal approximation.
        let long: Vec<f64> = (0..500).map(|i| ((i * 7919) % 503) as f64).collect();
        let l = up_down_runs_test(&long, Alternative::TwoSided).unwrap();
        assert!(!l.exact && l.p_value > 0.0 && l.p_value <= 1.0);
        let chart = run_chart_tests(&s).unwrap();
        assert_eq!(chart.clustering(), c.p_fewer);
        assert_eq!(chart.trend(), u.p_fewer);
        assert!(chart.mixing() > 0.99 && chart.oscillation() > 1.0 - 1e-12);
    }
}
