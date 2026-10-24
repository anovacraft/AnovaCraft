//! Effect sizes for analysis of variance and contingency tables, with
//! confidence intervals from noncentral distributions (Steiger 2004,
//! *Psychological Methods* 9(2); Smithson 2003, *Confidence Intervals*).
//!
//! **ANOVA effects.** For an effect with sum of squares `SSₑ` on `dfₑ`, error
//! `SS_err` on `df_err`, corrected total `SS_T` and `N` observations, with
//! `MS_err = SS_err/df_err` and `F = (SSₑ/dfₑ)/MS_err`:
//!
//! * `η² = SSₑ / SS_T`;
//! * partial `η²ₚ = SSₑ / (SSₑ + SS_err)`;
//! * partial `ω²ₚ = dfₑ(MSₑ − MS_err) / (dfₑ MSₑ + (N − dfₑ) MS_err)`
//!   (Olejnik & Algina 2003; in a one-way layout it is the usual
//!   `(SS_B − (k − 1)MS_W)/(SS_T + MS_W)`); it is negative when `F < 1`;
//! * Cohen's `f = √(η²ₚ / (1 − η²ₚ))`.
//!
//! The interval inverts the noncentral F distribution: `λ_L` and `λ_U` solve
//! `P(F′ ≤ F_obs; dfₑ, df_err, λ) = 1 − α/2` and `= α/2` (a bound is 0 when
//! even `λ = 0` gives a smaller probability), and with `N′ = dfₑ + df_err + 1`
//! the population partial η² lies in `[λ_L/(λ_L + N′), λ_U/(λ_U + N′)]` and
//! `f` in `[√(λ_L/N′), √(λ_U/N′)]` (Steiger 2004, eqs. 9–12). The population
//! quantity is the same for η²ₚ and ω²ₚ, so the interval serves both.
//!
//! **Contingency tables.** For Pearson's `X²` on `df` from `n` counts in an
//! `r × c` table, `φ = √(X²/n)` and Cramér's `V = √(X²/(n(min(r, c) − 1)))`.
//! Their intervals map the noncentral χ² interval for `λ` the same way:
//! `[√(λ_L/n), √(λ_U/n)]` for `φ`, divided by `√(min(r, c) − 1)` for `V`
//! (capped at 1).

use crate::common::{check_level, increasing_root, invalid};
use stat_core::{NoncentralChiSquared, NoncentralF, Result};

/// Noncentrality interval: `λ` with `cdf(observed; λ) = 1 − α/2` and `α/2`.
/// `cdf` decreases in `λ`.
fn invert(level: f64, mut cdf: impl FnMut(f64) -> Result<f64>, routine: &'static str) -> Result<(f64, f64)> {
    check_level(level)?;
    let alpha = 1.0 - level;
    let at_zero = cdf(0.0)?;
    let mut bound = |target: f64| -> Result<f64> {
        if at_zero <= target {
            return Ok(0.0);
        }
        increasing_root(|lambda| Ok(target - cdf(lambda)?), 0.0, 1.0, routine)
    };
    Ok((bound(1.0 - alpha / 2.0)?, bound(alpha / 2.0)?))
}

/// Interval for the noncentrality of an F statistic `f` on `(df1, df2)` df.
///
/// # Errors
/// Invalid degrees of freedom or level; a negative or non-finite `f`.
pub fn f_noncentrality_interval(f: f64, df1: f64, df2: f64, level: f64) -> Result<(f64, f64)> {
    if !(f.is_finite() && f >= 0.0) {
        return Err(invalid(format!("the F statistic must be non-negative and finite (got {f})")));
    }
    NoncentralF::new(df1, df2, 0.0)?;
    invert(level, |lambda| NoncentralF::new(df1, df2, lambda)?.cdf(f), "noncentral F interval")
}

/// Interval for the noncentrality of a χ² statistic `x` on `df` df.
///
/// # Errors
/// Invalid degrees of freedom or level; a negative or non-finite `x`.
pub fn chi_square_noncentrality_interval(x: f64, df: f64, level: f64) -> Result<(f64, f64)> {
    if !(x.is_finite() && x >= 0.0) {
        return Err(invalid(format!("the χ² statistic must be non-negative and finite (got {x})")));
    }
    NoncentralChiSquared::new(df, 0.0)?;
    invert(level, |lambda| NoncentralChiSquared::new(df, lambda)?.cdf(x), "noncentral chi-square interval")
}

/// Effect sizes of one ANOVA effect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnovaEffectSize {
    /// `η² = SSₑ/SS_T`.
    pub eta_squared: f64,
    /// Partial `η²ₚ = SSₑ/(SSₑ + SS_err)`.
    pub partial_eta_squared: f64,
    /// Partial `ω²ₚ` (negative when `F < 1`).
    pub omega_squared: f64,
    /// Cohen's `f`.
    pub cohens_f: f64,
    /// The F statistic.
    pub f_statistic: f64,
    /// Interval for the noncentrality `λ`.
    pub noncentrality: (f64, f64),
    /// Interval for the population partial η² (and ω²).
    pub partial_eta_interval: (f64, f64),
    /// Interval for Cohen's `f`.
    pub cohens_f_interval: (f64, f64),
    /// Confidence level of the intervals.
    pub level: f64,
}

/// Effect sizes of an effect with `ss_effect` on `df_effect`, given the error
/// `ss_error` on `df_error`, the corrected total `ss_total` and `n`
/// observations.
///
/// # Errors
/// Non-positive degrees of freedom, a zero error sum of squares, negative
/// sums of squares, or an invalid level.
pub fn anova_effect_size(
    ss_effect: f64,
    df_effect: f64,
    ss_error: f64,
    df_error: f64,
    ss_total: f64,
    n: f64,
    level: f64,
) -> Result<AnovaEffectSize> {
    if !(df_effect > 0.0 && df_error > 0.0) {
        return Err(invalid("effect sizes need positive effect and error degrees of freedom"));
    }
    if !(ss_effect >= 0.0 && ss_error > 0.0 && ss_total > 0.0) {
        return Err(invalid("effect sizes need non-negative sums of squares and some error variation"));
    }
    let ms_error = ss_error / df_error;
    let ms_effect = ss_effect / df_effect;
    let f = ms_effect / ms_error;
    let partial = ss_effect / (ss_effect + ss_error);
    let omega = df_effect * (ms_effect - ms_error) / (df_effect * ms_effect + (n - df_effect) * ms_error);
    let (lo, hi) = f_noncentrality_interval(f, df_effect, df_error, level)?;
    let big_n = df_effect + df_error + 1.0;
    Ok(AnovaEffectSize {
        eta_squared: ss_effect / ss_total,
        partial_eta_squared: partial,
        omega_squared: omega,
        cohens_f: (partial / (1.0 - partial)).sqrt(),
        f_statistic: f,
        noncentrality: (lo, hi),
        partial_eta_interval: (lo / (lo + big_n), hi / (hi + big_n)),
        cohens_f_interval: ((lo / big_n).sqrt(), (hi / big_n).sqrt()),
        level,
    })
}

/// Effect sizes of a test of independence in a contingency table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AssociationEffectSize {
    /// `φ = √(X²/n)`.
    pub phi: f64,
    /// Cramér's `V`.
    pub cramers_v: f64,
    /// Interval for the noncentrality `λ`.
    pub noncentrality: (f64, f64),
    /// Interval for `φ`.
    pub phi_interval: (f64, f64),
    /// Interval for `V`.
    pub cramers_v_interval: (f64, f64),
    /// Confidence level.
    pub level: f64,
}

/// `φ` and Cramér's `V` with intervals from Pearson's `chi_square` on `df`
/// for `n` counts in a `rows × columns` table.
///
/// # Errors
/// A table smaller than 2 × 2, no counts, or an invalid statistic or level.
pub fn association_effect_size(
    chi_square: f64,
    df: f64,
    n: f64,
    rows: usize,
    columns: usize,
    level: f64,
) -> Result<AssociationEffectSize> {
    if rows < 2 || columns < 2 {
        return Err(invalid("effect sizes need at least a 2 × 2 table"));
    }
    if n.is_nan() || n <= 0.0 {
        return Err(invalid("the table has no counts"));
    }
    let (lo, hi) = chi_square_noncentrality_interval(chi_square, df, level)?;
    let k = (rows.min(columns) - 1) as f64;
    let phi = (chi_square / n).sqrt();
    let phi_interval = ((lo / n).sqrt(), (hi / n).sqrt());
    Ok(AssociationEffectSize {
        phi,
        cramers_v: (phi / k.sqrt()).min(1.0),
        noncentrality: (lo, hi),
        phi_interval,
        cramers_v_interval: ((phi_interval.0 / k.sqrt()).min(1.0), (phi_interval.1 / k.sqrt()).min(1.0)),
        level,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::means::tests::close;

    /// R (`tests/data/effect_sizes_reference.R`): `uniroot` on
    /// `pf(F, df1, df2, ncp)` for the noncentrality bounds.
    #[test]
    fn noncentral_f_interval_matches_r() {
        let (lo, hi) = f_noncentrality_interval(5.0, 3.0, 36.0, 0.95).unwrap();
        close(lo, 1.477_986_099_902_9, 1e-6);
        close(hi, 33.646_678_929_582_5, 1e-6);
        // F below the lower critical value: the lower bound is 0.
        let (lo, hi) = f_noncentrality_interval(0.5, 2.0, 20.0, 0.95).unwrap();
        assert_eq!(lo, 0.0);
        close(hi, 7.037_532_573_642_1, 1e-6);
        assert_eq!(f_noncentrality_interval(0.01, 2.0, 20.0, 0.95).unwrap(), (0.0, 0.0));
    }

    /// One-way ANOVA effect sizes (R, as above, with `aov` sums of squares).
    #[test]
    fn anova_effect_sizes_match_r() {
        // SS_B = 30 on 3, SS_W = 72 on 36 (F = 5), N = 40.
        let e = anova_effect_size(30.0, 3.0, 72.0, 36.0, 102.0, 40.0, 0.95).unwrap();
        close(e.eta_squared, 30.0 / 102.0, 1e-14);
        close(e.partial_eta_squared, 30.0 / 102.0, 1e-14);
        close(e.omega_squared, (30.0 - 3.0 * 2.0) / (102.0 + 2.0), 1e-14);
        close(e.cohens_f, (30.0_f64 / 72.0).sqrt(), 1e-14);
        close(e.partial_eta_interval.0, 1.477_986_099_902_9 / (1.477_986_099_902_9 + 40.0), 1e-6);
        close(e.partial_eta_interval.1, 33.646_678_929_582_5 / (33.646_678_929_582_5 + 40.0), 1e-6);
        close(e.cohens_f_interval.1, (33.646_678_929_582_5_f64 / 40.0).sqrt(), 1e-6);
        assert!(anova_effect_size(1.0, 0.0, 1.0, 1.0, 2.0, 3.0, 0.95).is_err());
    }

    /// R: `uniroot` on `pchisq(x, df, ncp)`.
    #[test]
    fn association_matches_r() {
        // X² = 12 on 2 df, n = 100, 2 × 3 table.
        let a = association_effect_size(12.0, 2.0, 100.0, 2, 3, 0.95).unwrap();
        close(a.phi, 0.12_f64.sqrt(), 1e-14);
        close(a.cramers_v, 0.12_f64.sqrt(), 1e-14);
        close(a.noncentrality.0, 1.604_441_130_998_4, 1e-6);
        close(a.noncentrality.1, 28.166_379_836_838_3, 1e-6);
        close(a.cramers_v_interval.1, (28.166_379_836_838_3_f64 / 100.0).sqrt(), 1e-6);
        let b = association_effect_size(12.0, 4.0, 100.0, 3, 3, 0.95).unwrap();
        close(b.cramers_v, (0.12_f64 / 2.0).sqrt(), 1e-14);
        assert!(association_effect_size(1.0, 1.0, 10.0, 1, 3, 0.95).is_err());
    }
}
