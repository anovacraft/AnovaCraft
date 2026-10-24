//! One-way analysis of variance.
//!
//! # Model
//! `y_ij = μ + τ_i + ε_ij`, `ε ~ N(0, σ²)`, for `k` groups of sizes `n_i`
//! (balanced or not). The decomposition is the textbook one (e.g. NIST/SEMATECH
//! e-Handbook §7.4.3; Montgomery, *Design and Analysis of Experiments*, ch. 3):
//!
//! ```text
//! SS_between = Σ_i n_i (ȳ_i − ȳ)²          df = k − 1
//! SS_within  = Σ_i Σ_j (y_ij − ȳ_i)²       df = N − k
//! F = MS_between / MS_within,   R² = SS_between / (SS_between + SS_within)
//! ```
//!
//! # Numerical method
//! Group and grand means use compensated summation with a correction pass, and
//! both sums of squares are accumulated from centered values (two-pass), never
//! as `Σy² − (Σy)²/n`, which loses every significant digit when the data have a
//! large common offset (the NIST SmLs datasets are built to expose exactly that).

use stat_core::distributions::{ContinuousDistribution, FisherSnedecor, StudentsT};
use stat_core::summation::{mean, sum_iter};
use stat_core::{Result, StatError};

/// Summary of one group.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroupSummary {
    /// Number of observations.
    pub n: usize,
    /// Group mean.
    pub mean: f64,
    /// Group standard deviation (divisor `n − 1`); `None` for a single value.
    pub std_dev: Option<f64>,
    /// Standard error of the mean from the pooled error variance, `s_p / √n`.
    pub pooled_std_error: f64,
    /// 95% confidence interval for the group mean using the pooled variance and
    /// `t` with `N − k` degrees of freedom.
    pub confidence_interval: (f64, f64),
}

/// Result of a one-way ANOVA.
#[derive(Debug, Clone, PartialEq)]
pub struct OneWayAnova {
    /// Per-group summaries, in input order.
    pub groups: Vec<GroupSummary>,
    /// Total number of observations `N`.
    pub n_total: usize,
    /// Grand mean.
    pub grand_mean: f64,
    /// Between-groups degrees of freedom, `k − 1`.
    pub df_between: usize,
    /// Within-groups (error) degrees of freedom, `N − k`.
    pub df_within: usize,
    /// Between-groups sum of squares.
    pub ss_between: f64,
    /// Within-groups sum of squares.
    pub ss_within: f64,
    /// `SS_between / df_between`.
    pub ms_between: f64,
    /// `SS_within / df_within`.
    pub ms_within: f64,
    /// `F = MS_between / MS_within`; `None` when `MS_within = 0`.
    pub f_statistic: Option<f64>,
    /// `P(F_{k−1, N−k} ≥ F)`; `None` when `F` is undefined.
    pub p_value: Option<f64>,
    /// `R² = SS_between / (SS_between + SS_within)`.
    pub r_squared: f64,
    /// Residual (pooled) standard deviation `√MS_within`.
    pub residual_std_dev: f64,
}

/// One-way ANOVA of `groups` (each a slice of observations).
///
/// # Errors
/// * [`StatError::InvalidArgument`] for fewer than two groups or an empty group.
/// * [`StatError::InsufficientData`] unless `N > k` (an error variance needs at
///   least one within-group degree of freedom).
/// * [`StatError::NonFiniteInput`] for NaN or ±∞.
pub fn one_way_anova<G: AsRef<[f64]>>(groups: &[G]) -> Result<OneWayAnova> {
    let k = groups.len();
    if k < 2 {
        return Err(StatError::InvalidArgument { message: "one-way ANOVA needs at least two groups".into() });
    }
    if groups.iter().any(|g| g.as_ref().is_empty()) {
        return Err(StatError::InvalidArgument { message: "every group needs at least one observation".into() });
    }
    if groups.iter().flat_map(|g| g.as_ref()).any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "ANOVA observations" });
    }
    let n_total: usize = groups.iter().map(|g| g.as_ref().len()).sum();
    if n_total <= k {
        return Err(StatError::InsufficientData { required: k + 1, found: n_total });
    }
    let all: Vec<f64> = groups.iter().flat_map(|g| g.as_ref().iter().copied()).collect();
    let grand_mean = mean(&all).unwrap_or(f64::NAN);
    let means: Vec<f64> = groups.iter().map(|g| mean(g.as_ref()).unwrap_or(f64::NAN)).collect();

    let ss_between = sum_iter(groups.iter().zip(&means).map(|(g, &m)| {
        let d = m - grand_mean;
        g.as_ref().len() as f64 * d * d
    }));
    let ss_each: Vec<f64> =
        groups.iter().zip(&means).map(|(g, &m)| sum_iter(g.as_ref().iter().map(|&y| (y - m) * (y - m)))).collect();
    let ss_within = sum_iter(ss_each.iter().copied());

    let df_between = k - 1;
    let df_within = n_total - k;
    let ms_between = ss_between / df_between as f64;
    let ms_within = ss_within / df_within as f64;
    let (f_statistic, p_value) = if ms_within > 0.0 {
        let f = ms_between / ms_within;
        (Some(f), Some(FisherSnedecor::new(df_between as f64, df_within as f64)?.sf(f)?))
    } else {
        (None, None)
    };
    let pooled_sd = ms_within.sqrt();
    let t = StudentsT::new(df_within as f64)?.quantile(0.975)?;
    let summaries = groups
        .iter()
        .zip(&means)
        .zip(&ss_each)
        .map(|((g, &m), &ss)| {
            let n = g.as_ref().len();
            let se = pooled_sd / (n as f64).sqrt();
            GroupSummary {
                n,
                mean: m,
                std_dev: (n > 1).then(|| (ss / (n - 1) as f64).sqrt()),
                pooled_std_error: se,
                confidence_interval: (m - t * se, m + t * se),
            }
        })
        .collect();
    let total = ss_between + ss_within;
    Ok(OneWayAnova {
        groups: summaries,
        n_total,
        grand_mean,
        df_between,
        df_within,
        ss_between,
        ss_within,
        ms_between,
        ms_within,
        f_statistic,
        p_value,
        r_squared: if total > 0.0 { ss_between / total } else { f64::NAN },
        residual_std_dev: pooled_sd,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-worked example: groups {1, 2, 3}, {4, 5, 6}, {7, 8, 9}.
    /// Means 2, 5, 8; grand mean 5; SS_between = 3·(9 + 0 + 9) = 54;
    /// SS_within = 3 · 2 = 6; F = (54/2) / (6/6) = 27.
    #[test]
    fn hand_worked_example() {
        let a = one_way_anova(&[vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0], vec![7.0, 8.0, 9.0]]).unwrap();
        assert_eq!((a.df_between, a.df_within, a.n_total), (2, 6, 9));
        assert_eq!(a.ss_between, 54.0);
        assert_eq!(a.ss_within, 6.0);
        assert_eq!(a.f_statistic, Some(27.0));
        assert_eq!(a.grand_mean, 5.0);
        assert!((a.r_squared - 0.9).abs() < 1e-15);
        assert_eq!(a.residual_std_dev, 1.0);
        assert_eq!(a.groups[1].mean, 5.0);
        assert_eq!(a.groups[1].std_dev, Some(1.0));
        // F(2, 6) upper tail at 27: (1 + 2·27/6)^(−3) = 10^(−3).
        assert!((a.p_value.unwrap() - 1e-3).abs() < 1e-15);
        // Pooled SE = 1/√3, CI = mean ± t_{0.975,6}/√3.
        let half = 2.446_911_851_144_969 / 3f64.sqrt(); // t_{0.975, 6}
        assert!((a.groups[0].confidence_interval.1 - (2.0 + half)).abs() < 1e-12);
    }

    #[test]
    fn unbalanced_groups_and_large_offsets() {
        // Shifting every observation by a constant leaves the sums of squares unchanged.
        let base = [vec![0.1, 0.3], vec![0.2, 0.4, 0.6], vec![0.5]];
        let a = one_way_anova(&base).unwrap();
        let shifted: Vec<Vec<f64>> = base.iter().map(|g| g.iter().map(|v| v + 1e6).collect()).collect();
        let b = one_way_anova(&shifted).unwrap();
        assert!((a.ss_between - b.ss_between).abs() < 1e-9);
        assert!((a.ss_within - b.ss_within).abs() < 1e-9);
        assert_eq!(a.groups[2].std_dev, None, "single observation");
        assert_eq!((a.df_between, a.df_within), (2, 3));
    }

    #[test]
    fn rejects_bad_input() {
        assert!(one_way_anova(&[vec![1.0, 2.0]]).is_err(), "one group");
        assert!(one_way_anova(&[vec![1.0], vec![]]).is_err(), "empty group");
        assert!(one_way_anova(&[vec![1.0], vec![2.0]]).is_err(), "no error df");
        assert!(one_way_anova(&[vec![1.0, f64::NAN], vec![2.0]]).is_err());
        // Identical values within groups: F undefined.
        let a = one_way_anova(&[vec![1.0, 1.0], vec![2.0, 2.0]]).unwrap();
        assert_eq!(a.f_statistic, None);
    }
}
