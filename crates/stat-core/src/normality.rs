//! Tests of normality.
//!
//! * **Shapiro–Wilk W** for `3 ≤ n ≤ 5000`, with the coefficient
//!   approximations and p-value transformation of Royston (1995), "Remark AS
//!   R94: A remark on algorithm AS 181: The W-test for normality", *Applied
//!   Statistics* 44(4), 547–551:
//!   * `mᵢ = Φ⁻¹((i − 3/8)/(n + 1/4))`, `u = 1/√n`;
//!   * `a_n = m_n/‖m‖ + 0.221157u − 0.147981u² − 2.071190u³ + 4.434685u⁴ − 2.706056u⁵`,
//!     and for `n > 5` also
//!     `a_{n−1} = m_{n−1}/‖m‖ + 0.042981u − 0.293762u² − 1.752461u³ + 5.682633u⁴ − 3.582633u⁵`;
//!     the remaining `aᵢ = mᵢ/√φ` with `φ` chosen so that `‖a‖ = 1`;
//!     antisymmetry `a_{n+1−i} = −aᵢ`;
//!   * `W = (Σ aᵢ x₍ᵢ₎)² / Σ(xᵢ − x̄)²`;
//!   * p-value: for `n = 3` exactly `p = (6/π)(asin √W − asin √¾)`; for
//!     `4 ≤ n ≤ 11`, `z = (−ln(γ − ln(1 − W)) − μ)/σ` with `γ = 0.459n − 2.273`,
//!     `μ = 0.5440 − 0.39978n + 0.025054n² − 0.0006714n³`,
//!     `ln σ = 1.3822 − 0.77857n + 0.062767n² − 0.0020322n³`; for `n ≥ 12`,
//!     `z = (ln(1 − W) − μ)/σ` with `L = ln n`,
//!     `μ = −1.5861 − 0.31082L − 0.083751L² + 0.0038915L³`,
//!     `ln σ = −0.4803 − 0.082676L + 0.0030302L²`; `p = 1 − Φ(z)`.
//! * **Anderson–Darling A²** with mean and variance estimated from the sample
//!   (Stephens' "case 3"): `A² = −n − (1/n) Σ (2i − 1)[ln Φ(z₍ᵢ₎) + ln(1 − Φ(z₍ₙ₊₁₋ᵢ₎))]`,
//!   `zᵢ = (xᵢ − x̄)/s`. The p-value uses the modified statistic
//!   `A* = A²(1 + 0.75/n + 2.25/n²)` and the piecewise approximation of
//!   D'Agostino & Stephens (1986), *Goodness-of-Fit Techniques*, Table 4.9.
//!
//! Small p-values are evidence against normality.

use crate::descriptive::moments;
use crate::distribution_fit::ln_normal_sf;
use crate::distributions::{ContinuousDistribution, Normal};
use crate::error::{Result, StatError, invalid};
use crate::summation::{dot, sum_iter};

/// Largest sample size accepted by [`shapiro_wilk`] (the range over which
/// Royston's approximations were fitted).
pub const SHAPIRO_WILK_MAX_N: usize = 5000;

/// A normality test result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NormalityTest {
    /// The test statistic (W or A²).
    pub statistic: f64,
    /// Probability of a statistic at least this extreme for normal data.
    pub p_value: f64,
}

fn sorted_finite(x: &[f64], min_n: usize) -> Result<Vec<f64>> {
    if x.iter().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "normality test input" });
    }
    if x.len() < min_n {
        return Err(StatError::InsufficientData { required: min_n, found: x.len() });
    }
    let mut s = x.to_vec();
    s.sort_by(f64::total_cmp);
    if s[0] == s[s.len() - 1] {
        return Err(invalid("all values are equal, so normality cannot be tested"));
    }
    Ok(s)
}

/// Evaluate `c[0] + c[1]x + c[2]x² + …`.
fn poly(c: &[f64], x: f64) -> f64 {
    c.iter().rev().fold(0.0, |acc, &ci| acc * x + ci)
}

/// Shapiro–Wilk test (Royston 1995).
///
/// # Errors
/// [`StatError::InsufficientData`] for `n < 3`; [`StatError::InvalidArgument`]
/// for `n > 5000` or constant data; [`StatError::NonFiniteInput`] for NaN/±∞.
pub fn shapiro_wilk(x: &[f64]) -> Result<NormalityTest> {
    let s = sorted_finite(x, 3)?;
    let n = s.len();
    if n > SHAPIRO_WILK_MAX_N {
        return Err(invalid(format!("Shapiro–Wilk accepts at most {SHAPIRO_WILK_MAX_N} values (got {n})")));
    }
    let nf = n as f64;
    let half = n / 2;
    let std_normal = Normal::standard();

    // Coefficients for the lower half (negative values); a[i] pairs with s[i]
    // and −a[i] with s[n−1−i].
    let mut a = vec![0.0; half];
    if n == 3 {
        a[0] = -std::f64::consts::FRAC_1_SQRT_2;
    } else {
        let m: Vec<f64> =
            (1..=half).map(|i| std_normal.quantile((i as f64 - 0.375) / (nf + 0.25))).collect::<Result<_>>()?;
        let summ2 = 2.0 * dot(&m, &m);
        let ssumm2 = summ2.sqrt();
        let u = 1.0 / nf.sqrt();
        let a1 = m[0] / ssumm2 - poly(&[0.0, 0.221_157, -0.147_981, -2.071_19, 4.434_685, -2.706_056], u);
        let (first, fac) = if n > 5 {
            let a2 = m[1] / ssumm2 - poly(&[0.0, 0.042_981, -0.293_762, -1.752_461, 5.682_633, -3.582_633], u);
            let fac = ((summ2 - 2.0 * m[0] * m[0] - 2.0 * m[1] * m[1]) / (1.0 - 2.0 * a1 * a1 - 2.0 * a2 * a2)).sqrt();
            a[1] = a2;
            (2, fac)
        } else {
            let fac = ((summ2 - 2.0 * m[0] * m[0]) / (1.0 - 2.0 * a1 * a1)).sqrt();
            (1, fac)
        };
        a[0] = a1;
        for i in first..half {
            a[i] = m[i] / fac;
        }
    }

    // W from centered data; the numerator pairs extremes, so it is a sum of
    // non-negative differences.
    let mean = moments(&s)?.mean;
    let ss = sum_iter(s.iter().map(|v| (v - mean) * (v - mean)));
    let b = sum_iter((0..half).map(|i| -a[i] * (s[n - 1 - i] - s[i])));
    let w = ((b * b) / ss).min(1.0);

    let p_value = if n == 3 {
        let p = 6.0 / std::f64::consts::PI * (w.sqrt().asin() - 0.75_f64.sqrt().asin());
        p.clamp(0.0, 1.0)
    } else if n <= 11 {
        let gamma = poly(&[-2.273, 0.459], nf);
        let mu = poly(&[0.544, -0.399_78, 0.025_054, -6.714e-4], nf);
        let sigma = poly(&[1.3822, -0.778_57, 0.062_767, -0.002_032_2], nf).exp();
        let y = (-w).ln_1p();
        if y >= gamma { 0.0 } else { Normal::new(mu, sigma)?.sf(-(gamma - y).ln())? }
    } else {
        let l = nf.ln();
        let mu = poly(&[-1.5861, -0.310_82, -0.083_751, 0.003_891_5], l);
        let sigma = poly(&[-0.4803, -0.082_676, 0.003_030_2], l).exp();
        Normal::new(mu, sigma)?.sf((-w).ln_1p())?
    };
    Ok(NormalityTest { statistic: w, p_value })
}

/// Anderson–Darling test for normality with estimated mean and variance.
///
/// # Errors
/// [`StatError::InsufficientData`] for `n < 3`; [`StatError::InvalidArgument`]
/// for constant data; [`StatError::NonFiniteInput`] for NaN/±∞.
pub fn anderson_darling(x: &[f64]) -> Result<NormalityTest> {
    let s = sorted_finite(x, 3)?;
    let n = s.len();
    let nf = n as f64;
    let m = moments(&s)?;
    let sd = m.std_dev.unwrap_or(f64::NAN);
    let mut terms = Vec::with_capacity(n);
    for i in 0..n {
        let lo = (s[i] - m.mean) / sd;
        let hi = (s[n - 1 - i] - m.mean) / sd;
        // ln Φ(z) = ln Q(−z) and ln(1 − Φ(z)) = ln Q(z), each in log form so
        // that a point far in the tail (where Q underflows) stays finite.
        let ln_cdf = ln_normal_sf(-lo);
        let ln_sf = ln_normal_sf(hi);
        terms.push((2.0 * (i + 1) as f64 - 1.0) * (ln_cdf + ln_sf));
    }
    let a2 = -nf - sum_iter(terms) / nf;
    let a_star = a2 * (1.0 + 0.75 / nf + 2.25 / (nf * nf));
    let p_value = if a_star >= 0.6 {
        // The quadratic in the exponent has its minimum at A* = 5.709/0.0372
        // ≈ 153.5 and rises beyond it, so larger statistics keep the p-value
        // at that minimum (≈ 2e-190) instead of climbing back towards 1.
        let a = a_star.min(5.709 / (2.0 * 0.0186));
        (1.2937 - 5.709 * a + 0.0186 * a * a).exp()
    } else if a_star >= 0.34 {
        (0.9177 - 4.279 * a_star - 1.38 * a_star * a_star).exp()
    } else if a_star >= 0.2 {
        1.0 - (-8.318 + 42.796 * a_star - 59.938 * a_star * a_star).exp()
    } else {
        1.0 - (-13.436 + 101.14 * a_star - 223.73 * a_star * a_star).exp()
    };
    Ok(NormalityTest { statistic: a2, p_value: p_value.clamp(0.0, 1.0) })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference values from an independent implementation of the same
    /// published algorithms (SciPy 1.18 `shapiro`, and `anderson` for A²; the
    /// A-D p-values from the Table 4.9 formulas evaluated separately).
    #[allow(clippy::type_complexity)]
    const CASES: [(&str, &[f64], f64, f64, f64, f64); 5] = [
        (
            "n3",
            &[2.1, 3.7, 3.9],
            0.832_191_780_821_917_9,
            0.193_917_521_481_452_7,
            0.384_927_233_508_984_4,
            0.133_584_623_782_265_5,
        ),
        (
            "n4",
            &[1.0, 2.0, 2.5, 7.0],
            0.835_246_185_133_669_5,
            0.181_897_895_652_327_4,
            0.437_960_439_542_157_7,
            0.130_269_121_167_361_08,
        ),
        (
            "n8",
            &[148.0, 154.0, 158.0, 160.0, 161.0, 162.0, 166.0, 170.0],
            0.982_281_703_443_924_4,
            0.973_302_914_638_333_5,
            0.167_692_399_874_191_27,
            0.900_470_437_802_483_3,
        ),
        (
            "n20",
            &[6.1, 5.8, 7.0, 6.6, 5.4, 6.9, 7.4, 6.3, 5.9, 6.0, 6.8, 7.1, 5.5, 6.2, 6.4, 6.6, 7.9, 5.2, 6.7, 6.5],
            0.990_252_850_131_443_2,
            0.998_439_608_523_493_9,
            0.092_843_680_185_236_37,
            0.996_782_986_639_845_4,
        ),
        (
            "skew30",
            &[
                0.2, 0.3, 0.3, 0.4, 0.5, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0, 1.1, 1.3, 1.4, 1.6, 1.8, 2.0, 2.3, 2.6, 3.0,
                3.4, 3.9, 4.5, 5.2, 6.0, 7.1, 8.4, 10.0, 12.5, 16.0,
            ],
            0.764_219_663_050_932_1,
            1.559_105_979_010_789_3e-5,
            2.496_515_269_200_138,
            1.798_670_863_898_686_2e-6,
        ),
    ];

    fn rel(a: f64, b: f64) -> f64 {
        ((a - b) / b).abs()
    }

    #[test]
    fn shapiro_wilk_matches_reference() {
        for (name, x, w, p, _, _) in CASES {
            let t = shapiro_wilk(x).unwrap();
            assert!(rel(t.statistic, w) < 1e-9, "{name}: W {} vs {w}", t.statistic);
            assert!(rel(t.p_value, p) < 1e-6, "{name}: p {} vs {p}", t.p_value);
        }
    }

    #[test]
    fn anderson_darling_matches_reference() {
        for (name, x, _, _, a2, p) in CASES {
            let t = anderson_darling(x).unwrap();
            assert!(rel(t.statistic, a2) < 1e-10, "{name}: A² {} vs {a2}", t.statistic);
            assert!(rel(t.p_value, p) < 1e-9, "{name}: p {} vs {p}", t.p_value);
        }
    }

    #[test]
    fn invariant_to_location_and_scale() {
        let x = CASES[3].1;
        let y: Vec<f64> = x.iter().map(|v| 1e3 + 7.0 * v).collect();
        let (a, b) = (shapiro_wilk(x).unwrap(), shapiro_wilk(&y).unwrap());
        assert!((a.statistic - b.statistic).abs() < 1e-12);
        let (a, b) = (anderson_darling(x).unwrap(), anderson_darling(&y).unwrap());
        assert!((a.statistic - b.statistic).abs() < 1e-10);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(shapiro_wilk(&[1.0, 2.0]).is_err());
        assert!(shapiro_wilk(&[1.0, 1.0, 1.0]).is_err());
        assert!(shapiro_wilk(&[1.0, f64::NAN, 2.0]).is_err());
        assert!(shapiro_wilk(&vec![1.0; 5001]).is_err());
        assert!(anderson_darling(&[2.0, 2.0, 2.0, 2.0]).is_err());
    }

    /// Far from normal samples: A² beyond the vertex of the p-value
    /// polynomial must not climb back to p = 1, and a point whose upper tail
    /// underflows must not make A² NaN. A² references from mpmath at 50
    /// digits (the defining sum with exact ln Φ).
    #[test]
    fn anderson_darling_far_from_normal() {
        let two_values: Vec<f64> = [0.0; 1000].into_iter().chain([1.0; 1000]).collect();
        let t = anderson_darling(&two_values).unwrap();
        assert!((t.statistic - 359.117_437_297_677_2).abs() < 1e-9 * 359.0, "{}", t.statistic);
        assert!(t.p_value < 1e-180, "{}", t.p_value);
        let one_outlier: Vec<f64> = [0.0; 1499].into_iter().chain([1.0]).collect();
        let t = anderson_darling(&one_outlier).unwrap();
        assert!((t.statistic - 579.152_643_864_035_3).abs() < 1e-9 * 579.0, "{}", t.statistic);
        assert!(t.p_value < 1e-180, "{}", t.p_value);
        // The p-value never increases with the statistic.
        let skewed: Vec<f64> = (1..=5000).map(|i| (-(1.0 - f64::from(i) / 5001.0).ln()).powi(3)).collect();
        assert!(anderson_darling(&skewed).unwrap().p_value < 1e-180);
    }
}
