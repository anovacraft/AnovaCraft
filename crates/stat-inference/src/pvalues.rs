//! Multiple-testing adjustment and combination of p-values.
//!
//! **Adjustment.** For `m` p-values with ordered values
//! `p₍₁₎ ≤ … ≤ p₍ₘ₎`, each method returns adjusted p-values `p̃ᵢ`: rejecting
//! every hypothesis with `p̃ᵢ ≤ α` controls the stated error rate at `α`.
//!
//! * **Bonferroni:** `p̃ᵢ = min(1, m pᵢ)` (familywise error rate, FWER).
//! * **Šidák** (1967): `p̃ᵢ = 1 − (1 − pᵢ)ᵐ` (FWER for independent tests).
//! * **Holm** (1979) step-down: `p̃₍ᵢ₎ = maxⱼ≤ᵢ min(1, (m − j + 1) p₍ⱼ₎)` (FWER).
//! * **Hochberg** (1988) step-up: `p̃₍ᵢ₎ = minⱼ≥ᵢ min(1, (m − j + 1) p₍ⱼ₎)`
//!   (FWER under independence or positive dependence).
//! * **Benjamini–Hochberg** (1995): `p̃₍ᵢ₎ = minⱼ≥ᵢ min(1, m p₍ⱼ₎ / j)`
//!   (false discovery rate, FDR, under independence or positive dependence).
//! * **Benjamini–Yekutieli** (2001): as Benjamini–Hochberg multiplied by
//!   `c(m) = Σₖ₌₁ᵐ 1/k` (FDR under any dependence).
//! * **Storey q-values** (Storey 2002, *JRSS B* 64; Storey & Tibshirani
//!   2003, *PNAS* 100): the proportion of true null hypotheses is estimated
//!   as `π̂₀(λ) = #{pᵢ > λ} / (m(1 − λ))` (capped at 1; `λ = ½` by default),
//!   and `q₍ᵢ₎ = minⱼ≥ᵢ min(1, π̂₀ m p₍ⱼ₎ / j)` — Benjamini–Hochberg scaled by
//!   `π̂₀`.
//!
//! Missing (NaN) p-values are left out: `m` counts the others and their
//! adjusted values stay NaN, as R's `p.adjust` does.
//!
//! **Combination** of `k` independent p-values for one overall hypothesis:
//!
//! * **Fisher** (1932): `X² = −2 Σ ln pᵢ`, χ² on `2k` df under the null.
//! * **Stouffer** (1949), optionally weighted (Liptak 1958):
//!   `Z = Σ wᵢ Φ⁻¹(1 − pᵢ) / √(Σ wᵢ²)`, standard normal under the null; the
//!   combined p-value is `1 − Φ(Z)`. The inputs are read as one-sided
//!   p-values (all in the same direction).

use crate::common::invalid;
use stat_core::distributions::{ChiSquared, ContinuousDistribution, Normal};
use stat_core::{Result, StatError};

/// How to adjust p-values for multiple testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AdjustMethod {
    /// Bonferroni (FWER).
    Bonferroni,
    /// Šidák (FWER, independent tests).
    Sidak,
    /// Holm's step-down (FWER).
    Holm,
    /// Hochberg's step-up (FWER, independent or positively dependent tests).
    Hochberg,
    /// Benjamini–Hochberg (FDR).
    BenjaminiHochberg,
    /// Benjamini–Yekutieli (FDR under any dependence).
    BenjaminiYekutieli,
}

impl AdjustMethod {
    /// Every method, in report order.
    pub const ALL: [Self; 6] =
        [Self::Bonferroni, Self::Sidak, Self::Holm, Self::Hochberg, Self::BenjaminiHochberg, Self::BenjaminiYekutieli];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Bonferroni => "Bonferroni",
            Self::Sidak => "Šidák",
            Self::Holm => "Holm",
            Self::Hochberg => "Hochberg",
            Self::BenjaminiHochberg => "Benjamini–Hochberg",
            Self::BenjaminiYekutieli => "Benjamini–Yekutieli",
        }
    }

    /// Identifier in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Bonferroni => "bonferroni",
            Self::Sidak => "sidak",
            Self::Holm => "holm",
            Self::Hochberg => "hochberg",
            Self::BenjaminiHochberg => "bh",
            Self::BenjaminiYekutieli => "by",
        }
    }

    /// The method named `key` (see [`key`](Self::key)).
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }

    /// The error rate the method controls.
    #[must_use]
    pub fn controls(self) -> &'static str {
        match self {
            Self::BenjaminiHochberg | Self::BenjaminiYekutieli => "false discovery rate",
            _ => "familywise error rate",
        }
    }
}

fn check_p(p: &[f64]) -> Result<()> {
    if p.iter().any(|&v| v.is_infinite() || (!v.is_nan() && !(0.0..=1.0).contains(&v))) {
        return Err(invalid("p-values must lie between 0 and 1"));
    }
    Ok(())
}

/// Positions of the non-missing p-values sorted by value (ties by position).
fn order(p: &[f64]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..p.len()).filter(|&i| !p[i].is_nan()).collect();
    idx.sort_by(|&a, &b| p[a].total_cmp(&p[b]).then(a.cmp(&b)));
    idx
}

/// Step-up: `p̃₍ᵢ₎ = minⱼ≥ᵢ min(1, factor(j) p₍ⱼ₎)` with `j` the 1-based rank.
fn step_up(p: &[f64], factor: impl Fn(usize) -> f64) -> Vec<f64> {
    let idx = order(p);
    let mut out = vec![f64::NAN; p.len()];
    let mut running = f64::INFINITY;
    for (rank0, &i) in idx.iter().enumerate().rev() {
        running = running.min((factor(rank0 + 1) * p[i]).min(1.0));
        out[i] = running;
    }
    out
}

/// Adjust `p` by `method`.
///
/// # Errors
/// [`StatError::InvalidArgument`] for a value outside `[0, 1]` (NaN is
/// allowed and stays NaN).
pub fn adjust_p_values(p: &[f64], method: AdjustMethod) -> Result<Vec<f64>> {
    check_p(p)?;
    let m = p.iter().filter(|v| !v.is_nan()).count() as f64;
    Ok(match method {
        // (`f64::min` would turn NaN into 1.)
        AdjustMethod::Bonferroni => p.iter().map(|&v| if v.is_nan() { v } else { (m * v).min(1.0) }).collect(),
        AdjustMethod::Sidak => p.iter().map(|&v| -(m * (-v).ln_1p()).exp_m1()).collect(),
        AdjustMethod::Holm => {
            let idx = order(p);
            let mut out = vec![f64::NAN; p.len()];
            let mut running: f64 = 0.0;
            for (rank0, &i) in idx.iter().enumerate() {
                running = running.max(((m - rank0 as f64) * p[i]).min(1.0));
                out[i] = running;
            }
            out
        }
        AdjustMethod::Hochberg => step_up(p, |j| m - j as f64 + 1.0),
        AdjustMethod::BenjaminiHochberg => step_up(p, |j| m / j as f64),
        AdjustMethod::BenjaminiYekutieli => {
            let c: f64 = (1..=m as usize).map(|k| 1.0 / k as f64).sum();
            step_up(p, |j| c * m / j as f64)
        }
    })
}

/// Storey's q-values with the estimated proportion of true nulls.
#[derive(Debug, Clone, PartialEq)]
pub struct QValues {
    /// `π̂₀(λ)`, capped at 1.
    pub pi0: f64,
    /// The tuning value `λ`.
    pub lambda: f64,
    /// q-value of each p-value (NaN where the p-value is NaN).
    pub q_values: Vec<f64>,
}

/// Storey's (2002) q-values with `π̂₀(λ) = #{p > λ}/(m(1 − λ))`.
///
/// # Errors
/// [`StatError::InvalidArgument`] for `λ` outside `[0, 1)` or a p-value
/// outside `[0, 1]`; [`StatError::InsufficientData`] with no p-values.
pub fn storey_q_values(p: &[f64], lambda: f64) -> Result<QValues> {
    check_p(p)?;
    if !(0.0..1.0).contains(&lambda) {
        return Err(invalid(format!("λ must lie in [0, 1) (got {lambda})")));
    }
    let m = p.iter().filter(|v| !v.is_nan()).count();
    if m == 0 {
        return Err(StatError::InsufficientData { required: 1, found: 0 });
    }
    let above = p.iter().filter(|&&v| v > lambda).count() as f64;
    let mf = m as f64;
    let pi0 = (above / (mf * (1.0 - lambda))).min(1.0);
    // With every p-value at or below λ the estimate is 0; keep a positive
    // floor of one null so the q-values stay meaningful.
    let pi0 = if pi0 > 0.0 { pi0 } else { 1.0 / (mf * (1.0 - lambda)).max(1.0) };
    let q_values = step_up(p, |j| pi0 * mf / j as f64);
    Ok(QValues { pi0, lambda, q_values })
}

/// A combined test of several p-values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CombinedTest {
    /// The statistic: `X²` (Fisher) or `Z` (Stouffer).
    pub statistic: f64,
    /// Degrees of freedom (`2k` for Fisher; NaN for Stouffer).
    pub df: f64,
    /// Combined p-value.
    pub p_value: f64,
    /// Number of p-values combined.
    pub k: usize,
}

fn present(p: &[f64]) -> Result<Vec<usize>> {
    check_p(p)?;
    let idx: Vec<usize> = (0..p.len()).filter(|&i| !p[i].is_nan()).collect();
    if idx.is_empty() {
        return Err(StatError::InsufficientData { required: 1, found: 0 });
    }
    Ok(idx)
}

/// Fisher's combined test `X² = −2 Σ ln pᵢ` on `2k` df (NaN p-values left out).
///
/// # Errors
/// No p-values, or a value outside `[0, 1]`.
pub fn fisher_combined(p: &[f64]) -> Result<CombinedTest> {
    let idx = present(p)?;
    let x2: f64 = idx.iter().map(|&i| -2.0 * p[i].ln()).sum();
    let df = 2.0 * idx.len() as f64;
    let p_value = if x2.is_infinite() { 0.0 } else { ChiSquared::new(df)?.sf(x2)? };
    Ok(CombinedTest { statistic: x2, df, p_value, k: idx.len() })
}

/// Stouffer's (weighted) Z: `Σ wᵢ Φ⁻¹(1 − pᵢ) / √(Σ wᵢ²)`, p-value `1 − Φ(Z)`.
/// `weights`: one per p-value (`None`: all 1). Rows with a NaN p-value or a
/// NaN weight are left out.
///
/// # Errors
/// No usable p-values, a value outside `[0, 1]`, negative or non-finite
/// weights, all weights zero, or a weight list of the wrong length.
pub fn stouffer_combined(p: &[f64], weights: Option<&[f64]>) -> Result<CombinedTest> {
    check_p(p)?;
    if let Some(w) = weights {
        if w.len() != p.len() {
            return Err(invalid("one weight per p-value is needed"));
        }
        if w.iter().any(|&v| v.is_infinite() || v < 0.0) {
            return Err(invalid("weights must be non-negative and finite"));
        }
    }
    let weight = |i: usize| weights.map_or(1.0, |w| w[i]);
    let idx: Vec<usize> = (0..p.len()).filter(|&i| !p[i].is_nan() && !weight(i).is_nan()).collect();
    if idx.is_empty() {
        return Err(StatError::InsufficientData { required: 1, found: 0 });
    }
    let normal = Normal::standard();
    let mut num = 0.0;
    let mut den = 0.0;
    for &i in &idx {
        let w = weight(i);
        // Φ⁻¹(1 − p) = −Φ⁻¹(p), which keeps precision for small p.
        num += w * -normal.quantile(p[i])?;
        den += w * w;
    }
    if den == 0.0 {
        return Err(invalid("every weight is zero"));
    }
    let z = num / den.sqrt();
    let p_value = if z.is_nan() { f64::NAN } else { normal.sf(z)? };
    Ok(CombinedTest { statistic: z, df: f64::NAN, p_value, k: idx.len() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::means::tests::close;

    /// Ten p-values (`tests/data/pvalues_reference.R`).
    const P: [f64; 10] = [0.01, 0.04, 0.03, 0.005, 0.5, 0.2, 0.04, 0.8, 0.001, 0.07];

    fn check(method: AdjustMethod, expected: &[f64]) {
        let got = adjust_p_values(&P, method).unwrap();
        for (g, e) in got.iter().zip(expected) {
            close(*g, *e, 1e-12);
        }
    }

    /// R 4.3 `p.adjust(p, method)` for Bonferroni, Holm, Hochberg, BH and BY;
    /// Šidák from its formula `1 - (1 - p)^m` in R.
    #[test]
    fn adjustments_match_r() {
        check(AdjustMethod::Bonferroni, &[0.1, 0.4, 0.3, 0.05, 1.0, 1.0, 0.4, 1.0, 0.01, 0.7]);
        check(AdjustMethod::Holm, &[0.08, 0.24, 0.21, 0.045, 1.0, 0.6, 0.24, 1.0, 0.01, 0.28]);
        check(AdjustMethod::Hochberg, &[0.08, 0.2, 0.2, 0.045, 0.8, 0.6, 0.2, 0.8, 0.01, 0.28]);
        check(
            AdjustMethod::BenjaminiHochberg,
            &[1.0 / 30.0, 1.0 / 15.0, 1.0 / 15.0, 0.025, 5.0 / 9.0, 0.25, 1.0 / 15.0, 0.8, 0.01, 0.1],
        );
        check(
            AdjustMethod::BenjaminiYekutieli,
            &[
                0.097_632_275_132_275_13,
                0.195_264_550_264_550_25,
                0.195_264_550_264_550_25,
                0.073_224_206_349_206_34,
                1.0,
                0.732_242_063_492_063_4,
                0.195_264_550_264_550_25,
                1.0,
                0.029_289_682_539_682_54,
                0.292_896_825_396_825_43,
            ],
        );
        check(
            AdjustMethod::Sidak,
            &[
                0.095_617_924_991_195_59,
                0.335_167_364_008_499_2,
                0.262_575_873_105_071_9,
                0.048_889_869_534_228_136,
                0.999_023_437_5,
                0.892_625_817_6,
                0.335_167_364_008_499_2,
                0.999_999_897_6,
                0.009_955_119_790_251_765,
                0.516_017_692_820_707_2,
            ],
        );
    }

    #[test]
    fn missing_values_and_bad_input() {
        let p = [0.01, f64::NAN, 0.04];
        let a = adjust_p_values(&p, AdjustMethod::Bonferroni).unwrap();
        assert_eq!(a[0], 0.02);
        assert!(a[1].is_nan());
        assert_eq!(a[2], 0.08);
        assert!(adjust_p_values(&[1.5], AdjustMethod::Holm).is_err());
        assert!(storey_q_values(&[0.1], 1.0).is_err());
        assert!(fisher_combined(&[f64::NAN]).is_err());
        assert!(stouffer_combined(&[0.1, 0.2], Some(&[1.0])).is_err());
    }

    /// Hand calculation (R, `tests/data/pvalues_reference.R`): one of the
    /// 10 p-values exceeds λ = ½, so π̂₀ = 1/(10 · ½) = 0.2 and q = 0.2 × BH.
    #[test]
    fn storey_q_values_by_hand() {
        let q = storey_q_values(&P, 0.5).unwrap();
        close(q.pi0, 0.2, 1e-15);
        for (a, b) in q.q_values.iter().zip([
            0.006_666_666_666_666_667,
            0.013_333_333_333_333_334,
            0.013_333_333_333_333_334,
            0.005,
            0.111_111_111_111_111_12,
            0.05,
            0.013_333_333_333_333_334,
            0.16,
            0.002,
            0.02,
        ]) {
            close(*a, b, 1e-12);
        }
        // λ = 0: π̂₀ = 1 and the q-values are the Benjamini–Hochberg values.
        let q0 = storey_q_values(&P, 0.0).unwrap();
        assert_eq!(q0.pi0, 1.0);
        let bh = adjust_p_values(&P, AdjustMethod::BenjaminiHochberg).unwrap();
        for (a, b) in q0.q_values.iter().zip(&bh) {
            close(*a, *b, 1e-15);
        }
    }

    /// R: `pchisq(-2*sum(log(p)), 2*length(p), lower.tail = FALSE)` and
    /// `pnorm(sum(w*qnorm(1-p))/sqrt(sum(w^2)), lower.tail = FALSE)`.
    #[test]
    fn combined_tests_match_r() {
        let p = [0.01, 0.2, 0.3, 0.04];
        let f = fisher_combined(&p).unwrap();
        close(f.statistic, 21.274_913_455_232_657, 1e-12);
        assert_eq!(f.df, 8.0);
        close(f.p_value, 0.006_451_911_116_653_122, 1e-9);
        let s = stouffer_combined(&p, None).unwrap();
        close(s.statistic, 2.721_527_845_786_982_7, 1e-10);
        close(s.p_value, 0.003_249_045_515_247_154, 1e-9);
        let w = stouffer_combined(&p, Some(&[10.0, 20.0, 30.0, 40.0])).unwrap();
        close(w.statistic, 2.297_794_018_498_272_4, 1e-10);
        close(w.p_value, 0.010_786_757_687_321_402, 1e-9);
    }
}
