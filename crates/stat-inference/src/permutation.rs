//! Permutation tests (Fisher 1935, *The Design of Experiments* §21; Pitman
//! 1937; Good, *Permutation, Parametric and Bootstrap Tests of Hypotheses*,
//! 2005): the null distribution of a statistic is the set of its values over
//! every rearrangement of the data that the null hypothesis makes equally
//! likely.
//!
//! * [`two_groups`] — two independent samples: the group labels are
//!   exchangeable, so every way of choosing `n₁` of the `n₁ + n₂` pooled
//!   values as the first group is equally likely (`C(n, n₁)` arrangements).
//!   Statistic: difference of means or of medians (first − second).
//! * [`paired`] — paired differences `dᵢ`: under the null each difference is
//!   equally likely to have either sign (`2ⁿ` sign patterns). Statistic: the
//!   mean difference.
//! * [`correlation`] — pairs `(xᵢ, yᵢ)`: under independence every pairing of
//!   the `y` values with the `x` values is equally likely (`n!`
//!   arrangements). Statistic: Pearson's r, or Spearman's ρ (Pearson's r of
//!   the average ranks).
//!
//! # p-values
//! When the number of arrangements `N` is at most
//! [`PermutationOptions::exact_limit`] every arrangement is enumerated and
//! the p-value is exact: the lower tail is `#{T* ≤ T}/N`, the upper tail
//! `#{T* ≥ T}/N` (the observed arrangement is among the `N`). Otherwise `B`
//! random arrangements are drawn and the tails are `(b + 1)/(B + 1)`, where
//! `b` counts the random arrangements at least as extreme, the observed
//! arrangement counted as one more (Davison & Hinkley 1997, eq. 4.11; Phipson
//! & Smyth 2010) — never zero, and a valid test. The two-sided p-value is
//! twice the smaller tail, capped at 1. Values within `10⁻⁹` of the data's
//! scale of the observed statistic count as equal to it, so arrangements that
//! give the observed value in exact arithmetic are not lost to rounding.
//!
//! Random arrangement `b` is drawn by a generator seeded with
//! [`crate::bootstrap::replicate_seed`]`(seed, b)` and the arrangements are
//! evaluated in parallel, so results are reproducible from the seed whatever
//! the number of threads.

use rayon::prelude::*;
use stat_core::correlation::average_ranks;
use stat_core::rng::Rng;
use stat_core::{Result, StatError};

use crate::bootstrap::replicate_seed;
use crate::common::{Alternative, invalid};

/// Statistic compared between two independent groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum GroupStatistic {
    /// Mean of the first group minus mean of the second.
    #[default]
    MeanDifference,
    /// Median of the first group minus median of the second.
    MedianDifference,
}

impl GroupStatistic {
    /// Short description.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::MeanDifference => "Difference of means",
            Self::MedianDifference => "Difference of medians",
        }
    }
}

/// Correlation coefficient tested for independence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum CorrelationStatistic {
    /// Pearson's product-moment r.
    #[default]
    Pearson,
    /// Spearman's ρ (Pearson's r of the average ranks).
    Spearman,
}

impl CorrelationStatistic {
    /// Short description.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Pearson => "Pearson correlation r",
            Self::Spearman => "Spearman correlation ρ",
        }
    }
}

/// How a permutation test is carried out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PermutationOptions {
    /// Alternative hypothesis about the statistic (`Greater`: the statistic
    /// is larger than under the null).
    pub alternative: Alternative,
    /// Number of random arrangements `B` when the test is not exact.
    pub resamples: usize,
    /// Seed of the random arrangements.
    pub seed: u64,
    /// Enumerate every arrangement when there are at most this many.
    pub exact_limit: u64,
}

impl Default for PermutationOptions {
    fn default() -> Self {
        Self { alternative: Alternative::TwoSided, resamples: 9999, seed: 1, exact_limit: 100_000 }
    }
}

/// Result of a permutation test.
#[derive(Debug, Clone, PartialEq)]
pub struct PermutationTest {
    /// What the statistic is (e.g. "Difference of means").
    pub statistic_name: &'static str,
    /// The statistic on the data as observed.
    pub observed: f64,
    /// p-value for [`alternative`](Self::alternative).
    pub p_value: f64,
    /// Share of the null distribution at or below the observed value (with
    /// the `(b + 1)/(B + 1)` convention when not exact).
    pub lower_tail: f64,
    /// Share of the null distribution at or above the observed value.
    pub upper_tail: f64,
    /// Alternative hypothesis.
    pub alternative: Alternative,
    /// `true` when every arrangement was enumerated.
    pub exact: bool,
    /// Number of distinct arrangements of the data (may be astronomically
    /// large, hence a float).
    pub arrangements: f64,
    /// Values of the statistic over the arrangements evaluated: all of them
    /// when exact (the observed arrangement included), else the `B` random ones.
    pub null_values: Vec<f64>,
}

impl PermutationTest {
    /// Number of arrangements evaluated (`N` when exact, `B` otherwise).
    #[must_use]
    pub fn evaluated(&self) -> usize {
        self.null_values.len()
    }
}

/// Binomial coefficient `C(n, k)` as a float (exact for the sizes enumerated).
fn choose(n: usize, k: usize) -> f64 {
    let k = k.min(n - k);
    (0..k).fold(1.0, |c, i| c * (n - i) as f64 / (i + 1) as f64).round()
}

fn factorial(n: usize) -> f64 {
    (1..=n).fold(1.0, |f, i| f * i as f64)
}

fn check_finite(x: &[f64], context: &'static str) -> Result<()> {
    if x.iter().all(|v| v.is_finite()) { Ok(()) } else { Err(StatError::NonFiniteInput { context }) }
}

fn check_options(o: &PermutationOptions) -> Result<()> {
    if o.resamples == 0 {
        return Err(invalid("a permutation test needs at least one random arrangement"));
    }
    Ok(())
}

/// The tails and p-value from a null distribution.
fn finish(
    statistic_name: &'static str,
    observed: f64,
    null_values: Vec<f64>,
    exact: bool,
    arrangements: f64,
    scale: f64,
    alternative: Alternative,
) -> PermutationTest {
    let tol = 1e-9 * scale.max(observed.abs());
    let at_most = null_values.iter().filter(|&&v| v <= observed + tol).count() as f64;
    let at_least = null_values.iter().filter(|&&v| v >= observed - tol).count() as f64;
    let n = null_values.len() as f64;
    let (lower_tail, upper_tail) =
        if exact { (at_most / n, at_least / n) } else { ((at_most + 1.0) / (n + 1.0), (at_least + 1.0) / (n + 1.0)) };
    PermutationTest {
        statistic_name,
        observed,
        p_value: alternative.p_value(lower_tail, upper_tail),
        lower_tail,
        upper_tail,
        alternative,
        exact,
        arrangements,
        null_values,
    }
}

fn mean(x: &[f64]) -> f64 {
    x.iter().sum::<f64>() / x.len() as f64
}

fn median_in_place(x: &mut [f64]) -> f64 {
    x.sort_by(f64::total_cmp);
    let n = x.len();
    if n % 2 == 1 { x[n / 2] } else { 0.5 * (x[n / 2 - 1] + x[n / 2]) }
}

fn max_abs(x: &[f64]) -> f64 {
    x.iter().fold(0.0, |m, v| m.max(v.abs()))
}

/// Permutation test comparing two independent samples `first` and `second`
/// by `statistic` (first − second). See the module notes for the p-value.
///
/// # Errors
/// [`StatError::InsufficientData`] when a sample is empty;
/// [`StatError::NonFiniteInput`] for NaN or ±∞; [`StatError::InvalidArgument`]
/// for zero resamples.
pub fn two_groups(
    first: &[f64],
    second: &[f64],
    statistic: GroupStatistic,
    options: &PermutationOptions,
) -> Result<PermutationTest> {
    check_options(options)?;
    if first.is_empty() || second.is_empty() {
        return Err(StatError::InsufficientData { required: 1, found: 0 });
    }
    check_finite(first, "first sample")?;
    check_finite(second, "second sample")?;
    let pooled: Vec<f64> = first.iter().chain(second).copied().collect();
    let (n, n1) = (pooled.len(), first.len());
    let total: f64 = pooled.iter().sum();
    let stat = |chosen: &[usize]| -> f64 {
        match statistic {
            GroupStatistic::MeanDifference => {
                let s1: f64 = chosen.iter().map(|&i| pooled[i]).sum();
                s1 / n1 as f64 - (total - s1) / (n - n1) as f64
            }
            GroupStatistic::MedianDifference => {
                let mut in_first = vec![false; n];
                chosen.iter().for_each(|&i| in_first[i] = true);
                let mut a: Vec<f64> = chosen.iter().map(|&i| pooled[i]).collect();
                let mut b: Vec<f64> = (0..n).filter(|&i| !in_first[i]).map(|i| pooled[i]).collect();
                median_in_place(&mut a) - median_in_place(&mut b)
            }
        }
    };
    let observed = match statistic {
        GroupStatistic::MeanDifference => mean(first) - mean(second),
        GroupStatistic::MedianDifference => {
            median_in_place(&mut first.to_vec()) - median_in_place(&mut second.to_vec())
        }
    };
    let arrangements = choose(n, n1);
    let exact = arrangements <= options.exact_limit as f64;
    let null_values = if exact {
        // Every n1-subset in lexicographic order.
        let mut idx: Vec<usize> = (0..n1).collect();
        let mut values = Vec::with_capacity(arrangements as usize);
        loop {
            values.push(stat(&idx));
            let Some(i) = (0..n1).rev().find(|&i| idx[i] < n - n1 + i) else { break };
            idx[i] += 1;
            for j in i + 1..n1 {
                idx[j] = idx[j - 1] + 1;
            }
        }
        values
    } else {
        (0..options.resamples)
            .into_par_iter()
            .map(|b| {
                let mut rng = Rng::new(replicate_seed(options.seed, b));
                let mut order: Vec<usize> = (0..n).collect();
                // Partial Fisher–Yates: the first n1 positions are a uniform subset.
                for i in 0..n1 {
                    let j = i + rng.below(n - i);
                    order.swap(i, j);
                }
                stat(&order[..n1])
            })
            .collect()
    };
    Ok(finish(statistic.label(), observed, null_values, exact, arrangements, max_abs(&pooled), options.alternative))
}

/// Permutation (sign-flip) test of paired differences `differences`
/// (e.g. after − before) having mean zero. See the module notes.
///
/// # Errors
/// [`StatError::InsufficientData`] when empty; [`StatError::NonFiniteInput`]
/// for NaN or ±∞; [`StatError::InvalidArgument`] for zero resamples.
pub fn paired(differences: &[f64], options: &PermutationOptions) -> Result<PermutationTest> {
    check_options(options)?;
    if differences.is_empty() {
        return Err(StatError::InsufficientData { required: 1, found: 0 });
    }
    check_finite(differences, "paired differences")?;
    let d = differences;
    let n = d.len();
    let observed = mean(d);
    let arrangements = 2f64.powi(n as i32);
    let exact = n < 63 && arrangements <= options.exact_limit as f64;
    let signed_mean = |negative: &dyn Fn(usize) -> bool| -> f64 {
        d.iter().enumerate().map(|(i, &v)| if negative(i) { -v } else { v }).sum::<f64>() / n as f64
    };
    let null_values = if exact {
        (0..1u64 << n).map(|mask| signed_mean(&|i| mask >> i & 1 == 1)).collect()
    } else {
        (0..options.resamples)
            .into_par_iter()
            .map(|b| {
                let mut rng = Rng::new(replicate_seed(options.seed, b));
                let signs: Vec<bool> = (0..n).map(|_| rng.next_u64() >> 63 == 1).collect();
                signed_mean(&|i| signs[i])
            })
            .collect()
    };
    Ok(finish("Mean difference", observed, null_values, exact, arrangements, max_abs(d), options.alternative))
}

/// Centered values and their sum of squares.
fn centered(x: &[f64]) -> (Vec<f64>, f64) {
    let m = mean(x);
    let c: Vec<f64> = x.iter().map(|v| v - m).collect();
    let ss = c.iter().map(|v| v * v).sum();
    (c, ss)
}

/// Permutation test of independence of paired values `x` and `y` by their
/// correlation (`Greater`: positive association). See the module notes.
///
/// # Errors
/// [`StatError::DimensionMismatch`] for unequal lengths;
/// [`StatError::InsufficientData`] for fewer than three pairs;
/// [`StatError::NonFiniteInput`] for NaN or ±∞; [`StatError::InvalidArgument`]
/// when either variable is constant or for zero resamples.
pub fn correlation(
    x: &[f64],
    y: &[f64],
    statistic: CorrelationStatistic,
    options: &PermutationOptions,
) -> Result<PermutationTest> {
    check_options(options)?;
    if x.len() != y.len() {
        return Err(StatError::DimensionMismatch { context: "x vs. y", expected: x.len(), found: y.len() });
    }
    if x.len() < 3 {
        return Err(StatError::InsufficientData { required: 3, found: x.len() });
    }
    check_finite(x, "x")?;
    check_finite(y, "y")?;
    let (x, y) = match statistic {
        CorrelationStatistic::Pearson => (x.to_vec(), y.to_vec()),
        CorrelationStatistic::Spearman => (average_ranks(x), average_ranks(y)),
    };
    let n = x.len();
    let (xc, sxx) = centered(&x);
    let (yc, syy) = centered(&y);
    if sxx <= 0.0 || syy <= 0.0 {
        return Err(invalid("a correlation needs both variables to vary"));
    }
    let norm = (sxx * syy).sqrt();
    let r = |order: &[usize]| xc.iter().zip(order).map(|(a, &j)| a * yc[j]).sum::<f64>() / norm;
    let identity: Vec<usize> = (0..n).collect();
    let observed = r(&identity);
    let arrangements = factorial(n);
    let exact = arrangements <= options.exact_limit as f64;
    let null_values = if exact {
        // Heap's algorithm (Heap 1963), iteratively: every ordering of y.
        let mut order = identity;
        let mut c = vec![0usize; n];
        let mut values = Vec::with_capacity(arrangements as usize);
        values.push(r(&order));
        let mut i = 0;
        while i < n {
            if c[i] < i {
                if i % 2 == 0 {
                    order.swap(0, i);
                } else {
                    order.swap(c[i], i);
                }
                values.push(r(&order));
                c[i] += 1;
                i = 0;
            } else {
                c[i] = 0;
                i += 1;
            }
        }
        values
    } else {
        (0..options.resamples)
            .into_par_iter()
            .map(|b| {
                let mut rng = Rng::new(replicate_seed(options.seed, b));
                let mut order: Vec<usize> = (0..n).collect();
                rng.shuffle(&mut order);
                r(&order)
            })
            .collect()
    };
    Ok(finish(statistic.label(), observed, null_values, exact, arrangements, 1.0, options.alternative))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_enumeration_counts_every_arrangement() {
        let o = PermutationOptions::default();
        let t = two_groups(&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0, 7.0], GroupStatistic::MeanDifference, &o).unwrap();
        assert!(t.exact && t.arrangements == 35.0 && t.evaluated() == 35);
        // The observed split is the most extreme low one: p = 2 × 1/35.
        assert!((t.lower_tail - 1.0 / 35.0).abs() < 1e-15);
        assert!((t.p_value - 2.0 / 35.0).abs() < 1e-15);
        let p = paired(&[1.0, 2.0, 3.0, 4.0], &o).unwrap();
        assert_eq!(p.evaluated(), 16);
        assert!((p.upper_tail - 1.0 / 16.0).abs() < 1e-15);
        let c = correlation(&[1.0, 2.0, 3.0, 4.0], &[1.0, 3.0, 2.0, 4.0], CorrelationStatistic::Pearson, &o).unwrap();
        assert_eq!(c.evaluated(), 24);
        // Every ordering appears once: the null distribution is symmetric about 0.
        assert!(c.null_values.iter().sum::<f64>().abs() < 1e-12);
    }

    #[test]
    fn monte_carlo_is_reproducible_and_never_zero() {
        let a: Vec<f64> = (0..30).map(f64::from).collect();
        let b: Vec<f64> = (100..130).map(f64::from).collect();
        let o = PermutationOptions { resamples: 999, seed: 4, ..PermutationOptions::default() };
        let t = two_groups(&a, &b, GroupStatistic::MeanDifference, &o).unwrap();
        assert!(!t.exact);
        // No random split is as extreme: lower tail (0 + 1)/(999 + 1).
        assert!((t.lower_tail - 0.001).abs() < 1e-15);
        assert!((t.p_value - 0.002).abs() < 1e-15);
        let pool = rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap();
        let u = pool.install(|| two_groups(&a, &b, GroupStatistic::MeanDifference, &o).unwrap());
        assert_eq!(t, u);
        let m = two_groups(&a, &b, GroupStatistic::MedianDifference, &o).unwrap();
        assert!((m.observed + 100.0).abs() < 1e-12);
    }

    #[test]
    fn errors() {
        let o = PermutationOptions::default();
        assert!(two_groups(&[], &[1.0], GroupStatistic::MeanDifference, &o).is_err());
        assert!(two_groups(&[f64::NAN], &[1.0], GroupStatistic::MeanDifference, &o).is_err());
        assert!(paired(&[], &o).is_err());
        assert!(correlation(&[1.0, 2.0], &[1.0, 2.0], CorrelationStatistic::Pearson, &o).is_err());
        assert!(correlation(&[1.0, 1.0, 1.0], &[1.0, 2.0, 3.0], CorrelationStatistic::Pearson, &o).is_err());
        assert!(correlation(&[1.0, 2.0, 3.0], &[1.0, 2.0], CorrelationStatistic::Pearson, &o).is_err());
        let zero = PermutationOptions { resamples: 0, ..o };
        assert!(paired(&[1.0], &zero).is_err());
    }
}
