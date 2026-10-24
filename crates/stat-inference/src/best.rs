//! Multiple comparisons with the best (Hsu 1981, *Annals of Statistics* 9;
//! Hsu 1984, *Annals of Statistics* 12; Hsu, *Multiple Comparisons*, 1996,
//! ch. 4).
//!
//! For each group `i` the parameter of interest is how far its mean is from
//! the best of the others, `θᵢ = μᵢ − max_{j≠i} μⱼ` (larger is better; for
//! smaller is better the data are negated, so `θᵢ = μᵢ − min_{j≠i} μⱼ` and
//! the interval is reflected). `θᵢ > 0` means group `i` is the best;
//! `θᵢ < 0` means it is not.
//!
//! With the pooled mean square `MSE = s²` on `ν = N − k` df, let `dᵢ` be the
//! one-sided Dunnett critical value with group `i` as the control: the
//! `1 − α` quantile of `max_{j≠i} Tⱼ` for the multivariate t with
//! correlations `λⱼλₗ`, `λⱼ = √(nⱼ/(nⱼ + nᵢ))` (computed by
//! [`crate::control::DunnettDistribution`]). The constrained simultaneous
//! intervals `[Dᵢ⁻, Dᵢ⁺]` (Hsu 1996, §4.1; Stefansson, Kim & Hsu 1988) are
//!
//! ```text
//! Dᵢ⁺ = max(0, min_{j≠i} (ȳᵢ − ȳⱼ + dᵢ s √(1/nᵢ + 1/nⱼ)))
//! G   = { i : Dᵢ⁺ > 0 }
//! Dᵢ⁻ = 0                                                 if G = {i}
//!     = min(0, min_{j∈G, j≠i} (ȳᵢ − ȳⱼ − dⱼ s √(1/nᵢ + 1/nⱼ)))  otherwise,
//! ```
//!
//! and cover every `θᵢ` together with probability at least `1 − α`. With
//! the true best group `b`, the one-sided Dunnett event centred on `b` holds
//! with probability `1 − α` and implies each bound: it gives `Dᵦ⁺` its
//! upper bound, puts `b` in `G`, and bounds `μᵢ − μᵦ` from below for `i ≠ b`.
//! A group whose upper limit is 0 is shown not to be the best; a group whose
//! lower limit is 0 (the only member of `G`) is shown to be the best.

use crate::common::{check_finite, check_level, invalid, mean_var};
use crate::control::DunnettDistribution;
use stat_core::{Result, StatError};

/// Which direction is better.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum BestDirection {
    /// The largest mean is best.
    #[default]
    Largest,
    /// The smallest mean is best.
    Smallest,
}

impl BestDirection {
    /// Short description.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Largest => "largest is best",
            Self::Smallest => "smallest is best",
        }
    }
}

/// What the interval of one group shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BestVerdict {
    /// The interval is `[0, …]`: this group is the best.
    Best,
    /// The interval is `[…, 0]`: this group is not the best.
    NotBest,
    /// The interval contains 0 on both sides.
    Undecided,
}

impl BestVerdict {
    /// Short description.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Best => "the best",
            Self::NotBest => "not the best",
            Self::Undecided => "may be the best",
        }
    }
}

/// One group compared with the best of the others.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BestComparison {
    /// Group index.
    pub group: usize,
    /// Group size.
    pub n: usize,
    /// Group mean.
    pub mean: f64,
    /// `ȳᵢ − max_{j≠i} ȳⱼ` (or `− min` when smaller is better).
    pub difference: f64,
    /// Critical value `dᵢ` (one-sided Dunnett with this group as control).
    pub critical_value: f64,
    /// Lower limit `Dᵢ⁻` (`≤ 0`).
    pub lower: f64,
    /// Upper limit `Dᵢ⁺` (`≥ 0`).
    pub upper: f64,
    /// What the interval shows.
    pub verdict: BestVerdict,
}

/// Result of Hsu's comparisons with the best.
#[derive(Debug, Clone, PartialEq)]
pub struct BestTest {
    /// Pooled mean square error.
    pub mse: f64,
    /// Its degrees of freedom.
    pub df: f64,
    /// Simultaneous confidence level.
    pub level: f64,
    /// Direction.
    pub direction: BestDirection,
    /// One entry per group, in the order given.
    pub comparisons: Vec<BestComparison>,
}

/// Hsu's constrained multiple comparisons with the best of `groups`.
///
/// # Errors
/// [`StatError::InsufficientData`] for an empty group;
/// [`StatError::NonFiniteInput`] for NaN/±∞; [`StatError::InvalidArgument`]
/// for fewer than two groups, no error degrees of freedom, zero variance or
/// a level outside (0, 1).
pub fn comparisons_with_best<G: AsRef<[f64]>>(groups: &[G], level: f64, direction: BestDirection) -> Result<BestTest> {
    check_level(level)?;
    if groups.len() < 2 {
        return Err(invalid("comparisons with the best need at least two groups"));
    }
    for g in groups {
        if g.as_ref().is_empty() {
            return Err(StatError::InsufficientData { required: 1, found: 0 });
        }
        check_finite(g.as_ref(), "group values")?;
    }
    let k = groups.len();
    let total: usize = groups.iter().map(|g| g.as_ref().len()).sum();
    let df = (total - k) as f64;
    if df < 1.0 {
        return Err(invalid("no error degrees of freedom: every group has a single value"));
    }
    let n: Vec<f64> = groups.iter().map(|g| g.as_ref().len() as f64).collect();
    let raw_means: Vec<f64> = groups.iter().map(|g| mean_var(g.as_ref()).0).collect();
    let ss: f64 =
        groups.iter().zip(&raw_means).map(|(g, m)| g.as_ref().iter().map(|v| (v - m).powi(2)).sum::<f64>()).sum();
    let mse = ss / df;
    if mse <= 0.0 {
        return Err(invalid("the groups have no within-group variation"));
    }
    let s = mse.sqrt();
    // Work with "larger is better" means.
    let sign = if direction == BestDirection::Largest { 1.0 } else { -1.0 };
    let means: Vec<f64> = raw_means.iter().map(|m| sign * m).collect();
    let mut d = Vec::with_capacity(k);
    for i in 0..k {
        let lambdas: Vec<f64> = (0..k).filter(|&j| j != i).map(|j| (n[j] / (n[j] + n[i])).sqrt()).collect();
        d.push(DunnettDistribution::new(lambdas, df)?.quantile(level, false)?);
    }
    let se = |i: usize, j: usize| s * (1.0 / n[i] + 1.0 / n[j]).sqrt();
    let upper: Vec<f64> = (0..k)
        .map(|i| {
            (0..k)
                .filter(|&j| j != i)
                .map(|j| means[i] - means[j] + d[i] * se(i, j))
                .fold(f64::INFINITY, f64::min)
                .max(0.0)
        })
        .collect();
    let candidates: Vec<usize> = (0..k).filter(|&i| upper[i] > 0.0).collect();
    let lower: Vec<f64> = (0..k)
        .map(|i| {
            if candidates == [i] {
                0.0
            } else {
                candidates
                    .iter()
                    .filter(|&&j| j != i)
                    .map(|&j| means[i] - means[j] - d[j] * se(i, j))
                    .fold(f64::INFINITY, f64::min)
                    .min(0.0)
            }
        })
        .collect();
    let comparisons = (0..k)
        .map(|i| {
            let best_other = (0..k).filter(|&j| j != i).map(|j| means[j]).fold(f64::NEG_INFINITY, f64::max);
            let difference = means[i] - best_other;
            // Back to the data's direction: θ = sign·θ', so the interval reflects.
            let (lo, hi) = if sign > 0.0 { (lower[i], upper[i]) } else { (-upper[i], -lower[i]) };
            let verdict = if upper[i] == 0.0 {
                BestVerdict::NotBest
            } else if lower[i] == 0.0 {
                BestVerdict::Best
            } else {
                BestVerdict::Undecided
            };
            BestComparison {
                group: i,
                n: n[i] as usize,
                mean: raw_means[i],
                difference: sign * difference,
                critical_value: d[i],
                lower: lo,
                upper: hi,
                verdict,
            }
        })
        .collect();
    Ok(BestTest { mse, df, level, direction, comparisons })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol, "{a} vs {b}");
    }

    /// Balanced: every critical value is the one-sided Dunnett value for
    /// `k − 1` comparisons, 2.11 for three groups of five (12 df) in
    /// Dunnett's (1955) table. Intervals by hand with d = 2.108 057 (nested
    /// adaptive quadrature of Dunnett's integral in SciPy 1.18,
    /// `scipy.integrate.quad`): ȳ = 10, 12, 15, s² = 2.5, se = s√(2/5) = 1.
    #[test]
    fn balanced_example_by_hand() {
        let a = [8.0, 9.0, 10.0, 11.0, 12.0];
        let b = [10.0, 11.0, 12.0, 13.0, 14.0];
        let c = [13.0, 14.0, 15.0, 16.0, 17.0];
        // Within-group SS = 3 × 10 = 30 on 12 df: s² = 2.5.
        let r = comparisons_with_best(&[&a[..], &b[..], &c[..]], 0.95, BestDirection::Largest).unwrap();
        close(r.mse, 2.5, 1e-12);
        let d = r.comparisons[0].critical_value;
        close(d, 2.108_057_3, 2e-6);
        assert!(r.comparisons.iter().all(|c| (c.critical_value - d).abs() < 1e-10));
        let half = d * (2.5f64 * 0.4).sqrt();
        // D⁺: a: min(10−12, 10−15) + h; b: min(12−10, 12−15) + h; c: min(5, 3) + h.
        close(r.comparisons[0].upper, (-5.0 + half).max(0.0), 1e-9);
        close(r.comparisons[1].upper, (-3.0 + half).max(0.0), 1e-9);
        close(r.comparisons[2].upper, 3.0 + half, 1e-9);
        // h ≈ 2.11 < 3, so only c is in G: c is the best, a and b are not.
        assert_eq!(r.comparisons[2].lower, 0.0);
        assert_eq!(r.comparisons[2].verdict, BestVerdict::Best);
        assert_eq!(r.comparisons[0].verdict, BestVerdict::NotBest);
        close(r.comparisons[0].lower, -5.0 - half, 1e-9);
        close(r.comparisons[1].lower, -3.0 - half, 1e-9);
        close(r.comparisons[2].difference, 3.0, 1e-12);
        // Smallest is best: on the negated means −10, −12, −15 the gap from a
        // to b is only 2 < h, so a and b both stay candidates and c is out.
        let s = comparisons_with_best(&[&a[..], &b[..], &c[..]], 0.95, BestDirection::Smallest).unwrap();
        assert_eq!(s.comparisons[0].verdict, BestVerdict::Undecided);
        assert_eq!(s.comparisons[1].verdict, BestVerdict::Undecided);
        assert_eq!(s.comparisons[2].verdict, BestVerdict::NotBest);
        close(s.comparisons[0].difference, -2.0, 1e-12);
        close(s.comparisons[0].lower, -2.0 - half, 1e-9);
        close(s.comparisons[0].upper, half - 2.0, 1e-9);
        close(s.comparisons[1].lower, 2.0 - half, 1e-9);
        close(s.comparisons[1].upper, 2.0 + half, 1e-9);
        close(s.comparisons[2].lower, 0.0, 0.0);
        close(s.comparisons[2].upper, 5.0 + half, 1e-9);
    }

    /// Dunnett's (1955) blood counts, unequal sizes 6, 4 and 5 (12 df,
    /// s² = 1.380 523). The critical values with each group as the control
    /// and the intervals were computed independently in SciPy 1.18 (nested
    /// `scipy.integrate.quad` of Dunnett's integral, root-found by `brentq`).
    #[test]
    fn unbalanced_matches_independent_computation() {
        let control = [7.40, 8.50, 7.20, 8.24, 9.84, 8.32];
        let drug_a = [9.76, 8.80, 7.68, 9.36];
        let drug_b = [12.80, 9.68, 12.16, 9.20, 10.55];
        let r = comparisons_with_best(&[&control[..], &drug_a[..], &drug_b[..]], 0.95, BestDirection::Largest).unwrap();
        let d = [2.121_078_0, 2.091_627_8, 2.109_516_4];
        let lower = [-4.128_861_1, -3.640_689_1, 0.0];
        let upper = [0.0, 0.0, 3.640_689_1];
        for (i, c) in r.comparisons.iter().enumerate() {
            close(c.critical_value, d[i], 2e-6);
            close(c.lower, lower[i], 1e-5);
            close(c.upper, upper[i], 1e-5);
        }
        close(r.mse, 1.380_523_333_333_333_5, 1e-12);
        assert_eq!(r.comparisons[2].verdict, BestVerdict::Best);
        assert_eq!(r.comparisons[0].verdict, BestVerdict::NotBest);
        assert!(comparisons_with_best(&[&control[..]], 0.95, BestDirection::Largest).is_err());
        assert!(comparisons_with_best(&[&[1.0][..], &[2.0][..]], 0.95, BestDirection::Largest).is_err());
    }
}
