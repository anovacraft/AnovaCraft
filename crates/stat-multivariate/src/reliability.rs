//! Reliability of a multi-item scale (item analysis).
//!
//! For `k` items scored by `n` respondents, with item variances `sⱼ²`,
//! total-score variance `s_T²` (the total is the sum of the items) and mean
//! inter-item correlation `r̄`:
//!
//! * **Cronbach's alpha** (Cronbach, *Psychometrika* 16, 1951):
//!   `α = k/(k − 1) · (1 − Σ sⱼ² / s_T²)`;
//! * **standardized alpha** — alpha of the items rescaled to unit variance:
//!   `α_std = k r̄ / (1 + (k − 1) r̄)` (the Spearman–Brown form);
//! * **Feldt's interval** (Feldt, *Psychometrika* 30, 1965): under the
//!   usual normal model `(1 − α)/(1 − α̂)` follows `F(n − 1, (n − 1)(k − 1))`,
//!   so a `γ` interval is
//!   `[1 − (1 − α̂) F_{(1+γ)/2}, 1 − (1 − α̂) F_{(1−γ)/2}]`;
//! * per item: mean and standard deviation; the **item–total correlation**
//!   (with the total including the item); the **item–rest correlation**
//!   (with the total of the other items, the "corrected" item–total
//!   correlation, which does not count the item's correlation with itself);
//!   and alpha (raw and standardized) of the scale with that item deleted.
//!
//! Variances use the divisor `n − 1`; only respondents with every item
//! answered take part (listwise). Items to be reverse-scored are flipped by
//! the caller before analysis.

use stat_core::summation::sum_iter;
use stat_core::{ContinuousDistribution, FisherSnedecor, Result, StatError};

use crate::spd::{covariance, mean_vector};

/// Statistics of one item.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemStatistics {
    /// Item mean.
    pub mean: f64,
    /// Item standard deviation.
    pub std_dev: f64,
    /// Correlation of the item with the total score (including the item).
    pub item_total: f64,
    /// Correlation of the item with the total of the other items.
    pub item_rest: f64,
    /// Raw alpha of the other items (NaN with only two items).
    pub alpha_if_deleted: f64,
    /// Standardized alpha of the other items (NaN with only two items).
    pub standardized_alpha_if_deleted: f64,
}

/// Reliability of a scale.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemAnalysis {
    /// Respondents.
    pub n: usize,
    /// Items.
    pub k: usize,
    /// Cronbach's alpha.
    pub alpha: f64,
    /// Standardized alpha.
    pub standardized_alpha: f64,
    /// Mean inter-item correlation `r̄`.
    pub mean_inter_item_correlation: f64,
    /// Mean of the total score.
    pub total_mean: f64,
    /// Standard deviation of the total score.
    pub total_std_dev: f64,
    /// Inter-item correlation matrix.
    pub correlations: Vec<Vec<f64>>,
    /// One entry per item.
    pub items: Vec<ItemStatistics>,
}

impl ItemAnalysis {
    /// Feldt's confidence interval for alpha at confidence `level`.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] unless `level ∈ (0, 1)`.
    pub fn feldt_interval(&self, level: f64) -> Result<(f64, f64)> {
        feldt_interval(self.alpha, self.n, self.k, level)
    }
}

/// Feldt's (1965) interval for coefficient alpha `alpha` from `n`
/// respondents and `k` items, at confidence `level`.
///
/// # Errors
/// [`StatError::InvalidArgument`] unless `level ∈ (0, 1)`, `n ≥ 2` and `k ≥ 2`.
pub fn feldt_interval(alpha: f64, n: usize, k: usize, level: f64) -> Result<(f64, f64)> {
    if !(level > 0.0 && level < 1.0) {
        return Err(StatError::InvalidArgument {
            message: format!("confidence level must lie in (0, 1) (got {level})"),
        });
    }
    if n < 2 || k < 2 {
        return Err(StatError::InvalidArgument { message: "Feldt's interval needs n ≥ 2 and k ≥ 2".to_owned() });
    }
    let df1 = n as f64 - 1.0;
    let f = FisherSnedecor::new(df1, df1 * (k as f64 - 1.0))?;
    let upper_q = f.quantile(0.5 + level / 2.0)?;
    let lower_q = f.quantile(0.5 - level / 2.0)?;
    Ok((1.0 - (1.0 - alpha) * upper_q, 1.0 - (1.0 - alpha) * lower_q))
}

/// Raw alpha from the covariance matrix of the items in `items`.
fn raw_alpha(cov: &[Vec<f64>], items: &[usize]) -> f64 {
    let k = items.len() as f64;
    let sum_var = sum_iter(items.iter().map(|&j| cov[j][j]));
    let total_var = sum_iter(items.iter().flat_map(|&a| items.iter().map(move |&b| cov[a][b])));
    k / (k - 1.0) * (1.0 - sum_var / total_var)
}

/// Standardized alpha and mean inter-item correlation of the items in `items`.
fn standardized(corr: &[Vec<f64>], items: &[usize]) -> (f64, f64) {
    let k = items.len() as f64;
    let pairs = sum_iter(items.iter().enumerate().flat_map(|(i, &a)| items[i + 1..].iter().map(move |&b| corr[a][b])));
    let r = pairs / (k * (k - 1.0) / 2.0);
    (k * r / (1.0 + (k - 1.0) * r), r)
}

/// Item analysis of `rows` (one row per respondent, one value per item).
///
/// # Errors
/// [`StatError::InvalidArgument`] for fewer than two items, rows of unequal
/// length, or an item (or the total) with zero variance;
/// [`StatError::InsufficientData`] for fewer than three respondents;
/// [`StatError::NonFiniteInput`] for NaN/∞.
pub fn item_analysis(rows: &[Vec<f64>]) -> Result<ItemAnalysis> {
    let k = rows.first().map_or(0, Vec::len);
    if k < 2 {
        return Err(StatError::InvalidArgument { message: "choose at least two items".to_owned() });
    }
    if let Some(bad) = rows.iter().find(|r| r.len() != k) {
        return Err(StatError::DimensionMismatch { context: "item responses", expected: k, found: bad.len() });
    }
    if rows.iter().flatten().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "item responses" });
    }
    let n = rows.len();
    if n < 3 {
        return Err(StatError::InsufficientData { required: 3, found: n });
    }
    let mean = mean_vector(rows, k);
    let cov = covariance(rows, &mean);
    if let Some(j) = (0..k).find(|&j| cov[j][j] <= 0.0) {
        return Err(StatError::InvalidArgument {
            message: format!("item {} has the same value for every respondent", j + 1),
        });
    }
    let corr: Vec<Vec<f64>> =
        (0..k).map(|a| (0..k).map(|b| cov[a][b] / (cov[a][a] * cov[b][b]).sqrt()).collect()).collect();
    let all: Vec<usize> = (0..k).collect();
    let total_var = sum_iter(cov.iter().flatten().copied());
    if total_var <= 1e-12 * sum_iter((0..k).map(|j| cov[j][j])) {
        return Err(StatError::InvalidArgument {
            message: "the total score is the same for every respondent (are reversed items marked?)".to_owned(),
        });
    }
    let alpha = raw_alpha(&cov, &all);
    let (standardized_alpha, mean_r) = standardized(&corr, &all);
    let items = (0..k)
        .map(|j| {
            // cov(item, total) = Σ_b cov[j][b]; the rest total drops item j.
            let cov_total = sum_iter(cov[j].iter().copied());
            let item_total = cov_total / (cov[j][j] * total_var).sqrt();
            let rest_var = total_var - 2.0 * cov_total + cov[j][j];
            let item_rest = (cov_total - cov[j][j]) / (cov[j][j] * rest_var).sqrt();
            let others: Vec<usize> = all.iter().copied().filter(|&b| b != j).collect();
            let (alpha_if_deleted, standardized_alpha_if_deleted) = if others.len() >= 2 {
                (raw_alpha(&cov, &others), standardized(&corr, &others).0)
            } else {
                (f64::NAN, f64::NAN)
            };
            ItemStatistics {
                mean: mean[j],
                std_dev: cov[j][j].sqrt(),
                item_total,
                item_rest,
                alpha_if_deleted,
                standardized_alpha_if_deleted,
            }
        })
        .collect();
    Ok(ItemAnalysis {
        n,
        k,
        alpha,
        standardized_alpha,
        mean_inter_item_correlation: mean_r,
        total_mean: sum_iter(mean.iter().copied()),
        total_std_dev: total_var.sqrt(),
        correlations: corr,
        items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Six respondents, four items on a 1–5 scale.
    fn data() -> Vec<Vec<f64>> {
        vec![
            vec![4.0, 5.0, 4.0, 2.0],
            vec![3.0, 3.0, 4.0, 3.0],
            vec![5.0, 4.0, 5.0, 1.0],
            vec![2.0, 2.0, 3.0, 4.0],
            vec![4.0, 4.0, 3.0, 2.0],
            vec![1.0, 2.0, 2.0, 5.0],
        ]
    }

    #[test]
    fn alpha_and_item_statistics_match_hand_computation() {
        // Reference: Python (standard library, exact fractions) from the
        // definitions: sample covariances, α = k/(k−1)(1 − Σsⱼ²/s_T²), the
        // total and rest scores formed row by row and correlated directly.
        let a = item_analysis(&data()).unwrap();
        assert_eq!((a.n, a.k), (6, 4));
        assert!((a.alpha - ALPHA).abs() < 1e-13, "{}", a.alpha);
        assert!((a.standardized_alpha - STD_ALPHA).abs() < 1e-13, "{}", a.standardized_alpha);
        assert!((a.mean_inter_item_correlation - MEAN_R).abs() < 1e-13);
        for (j, item) in a.items.iter().enumerate() {
            assert!((item.item_total - ITEM_TOTAL[j]).abs() < 1e-13, "item–total {j}");
            assert!((item.item_rest - ITEM_REST[j]).abs() < 1e-13, "item–rest {j}");
            assert!((item.alpha_if_deleted - ALPHA_DELETED[j]).abs() < 1e-12, "α if deleted {j}");
            assert!((item.standardized_alpha_if_deleted - STD_ALPHA_DELETED[j]).abs() < 1e-12, "std α if deleted {j}");
        }
        assert!((a.total_mean - 77.0 / 6.0).abs() < 1e-13);
    }

    // Item 4 runs against the other three, so raw alpha is negative.
    const ALPHA: f64 = -0.874_666_666_666_666_7;
    const STD_ALPHA: f64 = -0.302_822_446_281_987_1;
    const MEAN_R: f64 = -0.061_693_890_450_371_07;
    const ITEM_TOTAL: [f64; 4] =
        [0.942_990_333_582_889_5, 0.916_911_812_950_009_3, 0.887_488_796_344_136, -0.942_990_333_582_889_5];
    const ITEM_REST: [f64; 4] =
        [0.554_700_196_225_229_1, 0.629_836_657_297_773_6, 0.629_836_657_297_773_6, -0.980_580_675_690_920_2];
    const ALPHA_DELETED: [f64; 4] = [-9.15, -5.909_090_909_090_909, -4.431_818_181_818_182, 0.908_333_333_333_333_3];
    const STD_ALPHA_DELETED: [f64; 4] = [-3.761_803_141_429_149_3, -3.0, -3.0, 0.912_848_939_250_066_4];

    #[test]
    fn reverse_scoring_raises_alpha() {
        // Item 4 runs against the others; flipping it (6 − x on a 1–5 scale)
        // gives the scale its intended direction.
        let flipped: Vec<Vec<f64>> = data()
            .into_iter()
            .map(|mut r| {
                r[3] = 6.0 - r[3];
                r
            })
            .collect();
        let raw = item_analysis(&data()).unwrap();
        let fixed = item_analysis(&flipped).unwrap();
        assert!(fixed.alpha > 0.8 && raw.alpha < fixed.alpha);
        assert!(raw.items[3].item_rest < 0.0 && fixed.items[3].item_rest > 0.0);
    }

    #[test]
    fn parallel_items_and_two_items() {
        // Identical items: every correlation is 1, so both alphas are 1.
        let rows: Vec<Vec<f64>> = (0..5).map(|i| vec![f64::from(i); 3]).collect();
        let a = item_analysis(&rows).unwrap();
        assert!((a.alpha - 1.0).abs() < 1e-14 && (a.standardized_alpha - 1.0).abs() < 1e-14);
        // Two items: alpha = 2 cov / (s₁² + s₂² + 2 cov), nothing to delete.
        let two: Vec<Vec<f64>> = data().into_iter().map(|r| r[..2].to_vec()).collect();
        let b = item_analysis(&two).unwrap();
        let x: Vec<f64> = two.iter().map(|r| r[0]).collect();
        let y: Vec<f64> = two.iter().map(|r| r[1]).collect();
        let (mx, my) = (x.iter().sum::<f64>() / 6.0, y.iter().sum::<f64>() / 6.0);
        let sxy: f64 = x.iter().zip(&y).map(|(a, b)| (a - mx) * (b - my)).sum::<f64>() / 5.0;
        let sxx: f64 = x.iter().map(|a| (a - mx).powi(2)).sum::<f64>() / 5.0;
        let syy: f64 = y.iter().map(|b| (b - my).powi(2)).sum::<f64>() / 5.0;
        assert!((b.alpha - 4.0 * sxy / (sxx + syy + 2.0 * sxy)).abs() < 1e-14);
        assert!(b.items[0].alpha_if_deleted.is_nan());
    }

    #[test]
    fn feldt_interval_inverts_the_f_distribution() {
        let (lo, hi) = feldt_interval(0.8, 50, 10, 0.95).unwrap();
        // (1 − α)/(1 − α̂) at each end sits at the 2.5% and 97.5% points of
        // F(49, 441).
        let f = FisherSnedecor::new(49.0, 441.0).unwrap();
        assert!((f.cdf((1.0 - lo) / 0.2).unwrap() - 0.975).abs() < 1e-10);
        assert!((f.cdf((1.0 - hi) / 0.2).unwrap() - 0.025).abs() < 1e-10);
        assert!(lo < 0.8 && 0.8 < hi);
        assert!(feldt_interval(0.8, 50, 10, 1.5).is_err());
    }

    #[test]
    fn errors() {
        assert!(item_analysis(&[vec![1.0], vec![2.0], vec![3.0]]).is_err());
        assert!(item_analysis(&[vec![1.0, 2.0], vec![2.0, 3.0]]).is_err());
        assert!(item_analysis(&[vec![1.0, 2.0], vec![1.0, 3.0], vec![1.0, 1.0]]).is_err(), "constant item");
        assert!(item_analysis(&[vec![1.0, 2.0], vec![f64::NAN, 3.0], vec![2.0, 1.0]]).is_err());
        // An item left un-reversed against its mirror: the total never varies.
        assert!(item_analysis(&[vec![1.0, 5.0], vec![2.0, 4.0], vec![4.0, 2.0]]).is_err(), "constant total");
    }
}
