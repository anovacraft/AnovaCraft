//! Control chart constants, derived from their mathematical definitions.
//!
//! For `n` independent `N(0, 1)` observations with range `W`:
//!
//! * `d₂(n) = E[W] = ∫ [1 − Φ(x)ⁿ − (1 − Φ(x))ⁿ] dx`
//! * `d₃(n) = sd(W)`, from
//!   `E[W²] = 2 ∫₀^∞ ∫ [1 − Φ(x+t)ⁿ − (1 − Φ(x))ⁿ + (Φ(x+t) − Φ(x))ⁿ] dx dt`
//!   (Tippett 1925, *Biometrika* 17; Harter 1960, *Ann. Math. Statist.* 31).
//! * `c₄(n) = E[s]/σ = √(2/(n−1)) · Γ(n/2) / Γ((n−1)/2)`.
//!
//! `d₂` and `d₃` for `n = 2..=25` were evaluated by composite Simpson quadrature
//! (step 0.005) with this crate's normal CDF and are tabulated below to 10
//! decimals; the tests recompute `d₂` independently and check the closed forms
//! `d₂(2) = 2/√π`, `d₃(2) = √(2 − 4/π)`. `c₄` is computed directly.
//!
//! The chart factors follow from 3-sigma limits:
//!
//! | factor | conventional symbol | definition |
//! |---|---|---|
//! | [`mean_from_range`](ChartConstants::mean_from_range) | A₂ | `3 / (d₂ √n)` |
//! | [`mean_from_sd`](ChartConstants::mean_from_sd) | A₃ | `3 / (c₄ √n)` |
//! | [`range_lower`](ChartConstants::range_lower) | D₃ | `max(0, 1 − 3 d₃/d₂)` |
//! | [`range_upper`](ChartConstants::range_upper) | D₄ | `1 + 3 d₃/d₂` |
//! | [`sd_lower`](ChartConstants::sd_lower) | B₃ | `max(0, 1 − 3 √(1−c₄²)/c₄)` |
//! | [`sd_upper`](ChartConstants::sd_upper) | B₄ | `1 + 3 √(1−c₄²)/c₄` |

use stat_core::special::ln_gamma_difference;
use stat_core::{Result, StatError};

/// Largest subgroup size for which range-based constants are tabulated.
pub const MAX_RANGE_SUBGROUP: usize = 25;

/// `d₂(n)` for `n = 2..=25`.
const D2: [f64; 24] = [
    std::f64::consts::FRAC_2_SQRT_PI, // exactly 2/√π
    1.692_568_750_6,
    2.058_750_746_0,
    2.325_928_947_3,
    2.534_412_721_2,
    2.704_356_751_2,
    2.847_200_612_1,
    2.970_026_324_4,
    3.077_505_461_7,
    3.172_872_703_8,
    3.258_455_279_7,
    3.335_980_354_1,
    3.406_763_108_2,
    3.471_826_889_9,
    3.531_982_786_1,
    3.587_883_961_8,
    3.640_063_757_9,
    3.688_963_023_2,
    3.734_950_119_6,
    3.778_335_829_8,
    3.819_384_643_4,
    3.858_323_423_3,
    3.895_348_148_5,
    3.930_629_219_5,
];

/// `d₃(n)` for `n = 2..=25`.
const D3: [f64; 24] = [
    0.852_502_466_4,
    0.888_368_004_0,
    0.879_808_202_8,
    0.864_081_941_1,
    0.848_039_686_1,
    0.833_205_335_6,
    0.819_831_489_8,
    0.807_834_274_6,
    0.797_050_673_5,
    0.787_314_620_6,
    0.778_478_341_2,
    0.770_416_202_1,
    0.763_023_095_6,
    0.756_211_429_7,
    0.749_908_089_4,
    0.744_051_784_0,
    0.738_590_853_4,
    0.733_481_495_5,
    0.728_686_345_7,
    0.724_173_340_7,
    0.719_914_808_4,
    0.715_886_735_5,
    0.712_068_175_1,
    0.708_440_765_9,
];

fn range_index(n: usize) -> Result<usize> {
    if (2..=MAX_RANGE_SUBGROUP).contains(&n) {
        Ok(n - 2)
    } else {
        Err(StatError::InvalidArgument {
            message: format!("range-based constants need a subgroup size of 2 to {MAX_RANGE_SUBGROUP} (got {n})"),
        })
    }
}

/// `d₂(n)`: expected range of `n` standard normal observations.
///
/// # Errors
/// [`StatError::InvalidArgument`] unless `2 ≤ n ≤ 25`.
pub fn d2(n: usize) -> Result<f64> {
    Ok(D2[range_index(n)?])
}

/// `d₃(n)`: standard deviation of the range of `n` standard normal observations.
///
/// # Errors
/// [`StatError::InvalidArgument`] unless `2 ≤ n ≤ 25`.
pub fn d3(n: usize) -> Result<f64> {
    Ok(D3[range_index(n)?])
}

/// `c₄(n) = √(2/(n−1)) Γ(n/2)/Γ((n−1)/2)`, so that `E[s] = c₄σ` for normal data.
///
/// # Errors
/// [`StatError::InvalidArgument`] if `n < 2`.
pub fn c4(n: usize) -> Result<f64> {
    if n < 2 {
        return Err(StatError::InvalidArgument {
            message: format!("c4 needs a subgroup size of at least 2 (got {n})"),
        });
    }
    let n = n as f64;
    Ok((2.0 / (n - 1.0)).sqrt() * ln_gamma_difference((n - 1.0) / 2.0, 0.5).exp())
}

/// All 3-sigma chart factors for subgroup size `n`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChartConstants {
    /// Subgroup size.
    pub n: usize,
    /// Expected relative range `d₂`.
    pub d2: f64,
    /// Standard deviation of the relative range `d₃`.
    pub d3: f64,
    /// Bias factor of the sample standard deviation `c₄`.
    pub c4: f64,
    /// A₂: half-width of the mean chart limits per unit of average range.
    pub mean_from_range: f64,
    /// A₃: half-width of the mean chart limits per unit of average standard deviation.
    pub mean_from_sd: f64,
    /// D₃: range chart lower limit per unit of average range.
    pub range_lower: f64,
    /// D₄: range chart upper limit per unit of average range.
    pub range_upper: f64,
    /// B₃: standard deviation chart lower limit per unit of average sd.
    pub sd_lower: f64,
    /// B₄: standard deviation chart upper limit per unit of average sd.
    pub sd_upper: f64,
}

impl ChartConstants {
    /// Factors for subgroup size `n`.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] unless `2 ≤ n ≤ 25`.
    pub fn for_subgroup_size(n: usize) -> Result<Self> {
        let (d2, d3, c4) = (d2(n)?, d3(n)?, c4(n)?);
        let root_n = (n as f64).sqrt();
        let c4_spread = 3.0 * (1.0 - c4 * c4).sqrt() / c4;
        Ok(Self {
            n,
            d2,
            d3,
            c4,
            mean_from_range: 3.0 / (d2 * root_n),
            mean_from_sd: 3.0 / (c4 * root_n),
            range_lower: (1.0 - 3.0 * d3 / d2).max(0.0),
            range_upper: 1.0 + 3.0 * d3 / d2,
            sd_lower: (1.0 - c4_spread).max(0.0),
            sd_upper: 1.0 + c4_spread,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_core::{ContinuousDistribution, Normal};

    /// Round to `places` decimals, as printed tables do.
    fn round(v: f64, places: i32) -> f64 {
        let s = 10f64.powi(places);
        (v * s).round() / s
    }

    #[test]
    fn closed_forms_for_pairs() {
        let pi = std::f64::consts::PI;
        assert!((d2(2).unwrap() - 2.0 / pi.sqrt()).abs() < 1e-10);
        assert!((d3(2).unwrap() - (2.0 - 4.0 / pi).sqrt()).abs() < 1e-10);
        // c₄(2) = √(2/π).
        assert!((c4(2).unwrap() - (2.0 / pi).sqrt()).abs() < 1e-14);
    }

    #[test]
    fn d2_matches_independent_quadrature() {
        // Trapezoid rule on a different grid (step 0.002 over [-10, 10]).
        let nd = Normal::standard();
        let h = 0.002;
        let grid: Vec<f64> = (0..=10_000).map(|i| nd.cdf(-10.0 + f64::from(i) * h).unwrap()).collect();
        for n in 2..=MAX_RANGE_SUBGROUP {
            let k = n as i32;
            let integral: f64 = grid.iter().map(|&p| 1.0 - p.powi(k) - (1.0 - p).powi(k)).sum::<f64>() * h;
            assert!((integral - d2(n).unwrap()).abs() < 1e-9, "n = {n}: {integral} vs {}", d2(n).unwrap());
        }
    }

    /// Factors printed in the NIST/SEMATECH e-Handbook of Statistical Methods,
    /// §6.3.2.1, for n = 2..10 (three decimals).
    #[test]
    fn match_nist_handbook_table() {
        let printed = [
            (2, 1.880, 0.0, 3.267),
            (3, 1.023, 0.0, 2.575),
            (4, 0.729, 0.0, 2.282),
            (5, 0.577, 0.0, 2.115),
            (6, 0.483, 0.0, 2.004),
            (7, 0.419, 0.076, 1.924),
            (8, 0.373, 0.136, 1.864),
            (9, 0.337, 0.184, 1.816),
            (10, 0.308, 0.223, 1.777),
        ];
        // Printed tables round independently (and sometimes from rounded d₂, d₃), so
        // a value sitting on a rounding boundary can differ by one unit in the last
        // digit between published sources; allow exactly that.
        let within_last_digit = |got: f64, printed: f64| (round(got, 3) - printed).abs() <= 0.001 + 1e-12;
        for (n, a2, d3_factor, d4) in printed {
            let c = ChartConstants::for_subgroup_size(n).unwrap();
            assert!(within_last_digit(c.mean_from_range, a2), "A2 n={n}: {}", c.mean_from_range);
            assert!(within_last_digit(c.range_lower, d3_factor), "D3 n={n}: {}", c.range_lower);
            assert!(within_last_digit(c.range_upper, d4), "D4 n={n}: {}", c.range_upper);
        }
        // D₄(5) = 2.11450 sits on the rounding boundary (NIST prints 2.115, other
        // standard tables 2.114); pin the computed value itself.
        let d4_5 = ChartConstants::for_subgroup_size(5).unwrap().range_upper;
        assert!((d4_5 - 2.114_499_5).abs() < 1e-6, "{d4_5}");
    }

    /// Widely tabulated three/four-decimal values of d₂, d₃, c₄, A₃, B₃, B₄.
    #[test]
    fn match_standard_tabulated_values() {
        let d = |n| ChartConstants::for_subgroup_size(n).unwrap();
        assert_eq!(round(d(5).d2, 3), 2.326);
        assert_eq!(round(d(5).d3, 3), 0.864);
        assert_eq!(round(d(10).d2, 3), 3.078);
        assert_eq!(round(d(25).d2, 3), 3.931);
        assert_eq!(round(d(25).d3, 3), 0.708);
        assert_eq!(round(d(5).c4, 4), 0.9400);
        assert_eq!(round(d(10).c4, 4), 0.9727);
        assert_eq!(round(d(25).c4, 4), 0.9896);
        assert_eq!(round(d(5).mean_from_sd, 3), 1.427);
        assert_eq!(round(d(6).sd_lower, 3), 0.030);
        assert_eq!(round(d(6).sd_upper, 3), 1.970);
        assert_eq!(d(5).sd_lower, 0.0);
    }

    #[test]
    fn out_of_range_sizes_are_rejected() {
        assert!(d2(1).is_err());
        assert!(d3(26).is_err());
        assert!(c4(1).is_err());
        assert!(c4(100).is_ok(), "c4 has a closed form for any n");
        assert!(ChartConstants::for_subgroup_size(30).is_err());
    }
}
