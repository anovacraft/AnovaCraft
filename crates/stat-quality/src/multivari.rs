//! Multi-vari analysis (Seder, "Diagnosis with diagrams", *Industrial
//! Quality Control* 6(4–5), 1950): a response broken down by up to four
//! nested or crossed factors, to see which "family" of variation — within a
//! piece, from piece to piece, or over time — dominates.
//!
//! Factors are given outermost first. The observations are grouped into the
//! cells of every leading set of factors that occurs in the data (factor 1;
//! factors 1–2; …), so nested factors (levels unique within their parent)
//! and crossed factors are handled alike. For each cell the mean, extremes,
//! range and standard deviation are computed.
//!
//! Two summaries accompany the chart:
//!
//! * **Variation by factor**: for factor `d`, the range of the means of its
//!   levels within each cell of the factors above it (the spread a
//!   multi-vari chart shows as the length of the connecting lines), averaged
//!   over those cells; and, within the innermost cells, the average range and
//!   the pooled standard deviation `√(Σ (nᵢ − 1) sᵢ² / Σ (nᵢ − 1))`.
//! * **Range limits**: the ranges of the innermost cells charted with 3-sigma
//!   limits (Montgomery, *Introduction to Statistical Quality Control*,
//!   section 6.2): `σ̂ = mean(Rᵢ / d₂(nᵢ))`, centre `d₂(nᵢ) σ̂`, limits
//!   `max(0, d₂(nᵢ) − 3 d₃(nᵢ)) σ̂` and `(d₂(nᵢ) + 3 d₃(nᵢ)) σ̂`, which for
//!   equal cell sizes are `R̄`, `D₃ R̄` and `D₄ R̄`.

use std::collections::BTreeMap;

use stat_core::{Result, StatError};

use crate::constants::{MAX_RANGE_SUBGROUP, d2, d3};

/// The most factors a multi-vari chart takes.
pub const MAX_MULTI_VARI_FACTORS: usize = 4;

/// One cell: the observations sharing the levels of the leading factors.
#[derive(Debug, Clone, PartialEq)]
pub struct MultiVariCell {
    /// Level codes of factors `0..=depth`.
    pub path: Vec<usize>,
    /// Indices of the observations in the cell, in input order.
    pub members: Vec<usize>,
    /// Mean of the response.
    pub mean: f64,
    /// Smallest value.
    pub min: f64,
    /// Largest value.
    pub max: f64,
    /// Sample standard deviation (NaN for a single observation).
    pub sd: f64,
    /// Indices of the cells one level deeper that lie in this one.
    pub children: Vec<usize>,
}

impl MultiVariCell {
    /// Number of observations.
    #[must_use]
    pub fn n(&self) -> usize {
        self.members.len()
    }

    /// Range `max − min`.
    #[must_use]
    pub fn range(&self) -> f64 {
        self.max - self.min
    }
}

/// Spread of one factor's level means within the cells of the factors above
/// it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FactorVariation {
    /// The factor (0 = outermost).
    pub factor: usize,
    /// Cells of the factors above with two or more levels of this factor
    /// (the whole data for the outermost factor).
    pub groups: usize,
    /// Average range of the level means within those cells (NaN without any).
    pub mean_range: f64,
    /// Largest such range.
    pub max_range: f64,
}

/// Range-chart limits of one innermost cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellRangeLimits {
    /// Expected range `d₂(n) σ̂`.
    pub center: f64,
    /// Lower limit (0 for cells of six or fewer).
    pub lower: f64,
    /// Upper limit.
    pub upper: f64,
}

/// A computed multi-vari breakdown.
#[derive(Debug, Clone, PartialEq)]
pub struct MultiVari {
    /// `levels[d]` holds the cells of factors `0..=d`, ordered by their level
    /// codes (lexicographically).
    pub levels: Vec<Vec<MultiVariCell>>,
    /// Mean of every observation.
    pub grand_mean: f64,
}

/// Mean, extremes and standard deviation of `values[members]`.
fn describe(values: &[f64], members: &[usize]) -> (f64, f64, f64, f64) {
    let n = members.len() as f64;
    let mean = stat_core::summation::sum_iter(members.iter().map(|&i| values[i])) / n;
    let ss = stat_core::summation::sum_iter(members.iter().map(|&i| (values[i] - mean).powi(2)));
    let sd = if members.len() > 1 { (ss / (n - 1.0)).sqrt() } else { f64::NAN };
    let (min, max) =
        members.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &i| (lo.min(values[i]), hi.max(values[i])));
    (mean, min, max, sd)
}

/// Group `values` by the factors `factors` (outermost first; `factors[d][i]`
/// is observation `i`'s level code of factor `d`).
///
/// # Errors
/// [`StatError::InvalidArgument`] without observations, with no factors or
/// more than [`MAX_MULTI_VARI_FACTORS`], when a factor's length differs from
/// the values', or when a value is not finite.
pub fn multi_vari(values: &[f64], factors: &[Vec<usize>]) -> Result<MultiVari> {
    let invalid = |message: String| StatError::InvalidArgument { message };
    if factors.is_empty() || factors.len() > MAX_MULTI_VARI_FACTORS {
        return Err(invalid(format!("a multi-vari chart takes one to {MAX_MULTI_VARI_FACTORS} factors")));
    }
    if values.is_empty() {
        return Err(invalid("a multi-vari chart needs at least one observation".to_owned()));
    }
    if factors.iter().any(|f| f.len() != values.len()) {
        return Err(invalid("every factor needs one level per observation".to_owned()));
    }
    if values.iter().any(|v| !v.is_finite()) {
        return Err(invalid("response values must be finite".to_owned()));
    }
    let mut levels: Vec<Vec<MultiVariCell>> = Vec::with_capacity(factors.len());
    for depth in 0..factors.len() {
        let mut cells: BTreeMap<Vec<usize>, Vec<usize>> = BTreeMap::new();
        for i in 0..values.len() {
            let path: Vec<usize> = factors[..=depth].iter().map(|f| f[i]).collect();
            cells.entry(path).or_default().push(i);
        }
        let cells: Vec<MultiVariCell> = cells
            .into_iter()
            .map(|(path, members)| {
                let (mean, min, max, sd) = describe(values, &members);
                MultiVariCell { path, members, mean, min, max, sd, children: Vec::new() }
            })
            .collect();
        if let Some(parents) = levels.last_mut() {
            for (k, cell) in cells.iter().enumerate() {
                let parent = parents
                    .binary_search_by(|p| p.path.as_slice().cmp(&cell.path[..depth]))
                    .expect("every cell has a parent");
                parents[parent].children.push(k);
            }
        }
        levels.push(cells);
    }
    let all: Vec<usize> = (0..values.len()).collect();
    Ok(MultiVari { levels, grand_mean: describe(values, &all).0 })
}

impl MultiVari {
    /// The innermost cells (all factors).
    #[must_use]
    pub fn cells(&self) -> &[MultiVariCell] {
        self.levels.last().map_or(&[], Vec::as_slice)
    }

    /// Spread of each factor's level means within the cells above it.
    #[must_use]
    pub fn factor_variation(&self) -> Vec<FactorVariation> {
        (0..self.levels.len())
            .map(|d| {
                // Groups of sibling means: children of each parent cell, or
                // every top-level cell for the outermost factor.
                let groups: Vec<Vec<f64>> = if d == 0 {
                    vec![self.levels[0].iter().map(|c| c.mean).collect()]
                } else {
                    self.levels[d - 1]
                        .iter()
                        .map(|p| p.children.iter().map(|&k| self.levels[d][k].mean).collect())
                        .collect()
                };
                let ranges: Vec<f64> = groups
                    .iter()
                    .filter(|g| g.len() >= 2)
                    .map(|g| {
                        let (lo, hi) =
                            g.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &m| (a.min(m), b.max(m)));
                        hi - lo
                    })
                    .collect();
                let mean_range =
                    if ranges.is_empty() { f64::NAN } else { stat_core::summation::sum(&ranges) / ranges.len() as f64 };
                FactorVariation {
                    factor: d,
                    groups: ranges.len(),
                    mean_range,
                    max_range: ranges.iter().copied().fold(f64::NAN, f64::max),
                }
            })
            .collect()
    }

    /// Average range of the innermost cells with two or more observations
    /// (NaN without any).
    #[must_use]
    pub fn mean_within_range(&self) -> f64 {
        let ranges: Vec<f64> = self.cells().iter().filter(|c| c.n() >= 2).map(MultiVariCell::range).collect();
        if ranges.is_empty() { f64::NAN } else { stat_core::summation::sum(&ranges) / ranges.len() as f64 }
    }

    /// Pooled within-cell standard deviation `√(Σ (nᵢ − 1) sᵢ² / Σ (nᵢ − 1))`
    /// over the innermost cells (NaN when no cell has two observations).
    #[must_use]
    pub fn pooled_within_sd(&self) -> f64 {
        let (ss, df) = self
            .cells()
            .iter()
            .filter(|c| c.n() >= 2)
            .fold((0.0, 0.0), |(ss, df), c| (ss + (c.n() as f64 - 1.0) * c.sd * c.sd, df + c.n() as f64 - 1.0));
        if df > 0.0 { (ss / df).sqrt() } else { f64::NAN }
    }

    /// Range-chart limits for each innermost cell, parallel to
    /// [`MultiVari::cells`]: `None` for a cell of one observation, and
    /// limits of NaN for cells larger than
    /// [`MAX_RANGE_SUBGROUP`](crate::constants::MAX_RANGE_SUBGROUP) (whose
    /// ranges are still charted). Returns all `None` when no cell has two to
    /// 25 observations.
    #[must_use]
    pub fn cell_range_limits(&self) -> Vec<Option<CellRangeLimits>> {
        let usable = |n: usize| (2..=MAX_RANGE_SUBGROUP).contains(&n);
        let scaled: Vec<f64> = self
            .cells()
            .iter()
            .filter(|c| usable(c.n()))
            .map(|c| c.range() / d2(c.n()).expect("n is in range"))
            .collect();
        if scaled.is_empty() {
            return vec![None; self.cells().len()];
        }
        let sigma = stat_core::summation::sum(&scaled) / scaled.len() as f64;
        self.cells()
            .iter()
            .map(|c| match c.n() {
                0 | 1 => None,
                n if usable(n) => {
                    let (d2, d3) = (d2(n).expect("n is in range"), d3(n).expect("n is in range"));
                    Some(CellRangeLimits {
                        center: d2 * sigma,
                        lower: ((d2 - 3.0 * d3) * sigma).max(0.0),
                        upper: (d2 + 3.0 * d3) * sigma,
                    })
                }
                _ => Some(CellRangeLimits { center: f64::NAN, lower: f64::NAN, upper: f64::NAN }),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChartConstants;

    /// Two batches × two parts (nested: part codes restart in each batch) ×
    /// three positions, one reading each; values chosen for hand computation.
    fn shafts() -> (Vec<f64>, Vec<Vec<usize>>) {
        let values = vec![
            10.0, 11.0, 12.0, // batch 0, part 0: positions 0, 1, 2
            13.0, 14.0, 18.0, // batch 0, part 1
            20.0, 21.0, 25.0, // batch 1, part 0
            24.0, 26.0, 28.0, // batch 1, part 1
        ];
        let batch = vec![0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1];
        let part = vec![0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1];
        (values, vec![batch, part])
    }

    #[test]
    fn cells_means_and_nesting() {
        let (values, factors) = shafts();
        let m = multi_vari(&values, &factors).unwrap();
        assert_eq!(m.levels.len(), 2);
        let batch_means: Vec<f64> = m.levels[0].iter().map(|c| c.mean).collect();
        // Batch 0: 78 / 6 = 13; batch 1: 144 / 6 = 24.
        assert!((batch_means[0] - 13.0).abs() < 1e-12 && (batch_means[1] - 24.0).abs() < 1e-12);
        let cells: Vec<(f64, f64)> = m.cells().iter().map(|c| (c.mean, c.range())).collect();
        let want = [(11.0, 2.0), (15.0, 5.0), (22.0, 5.0), (26.0, 4.0)];
        for (got, want) in cells.iter().zip(want) {
            assert!((got.0 - want.0).abs() < 1e-12 && (got.1 - want.1).abs() < 1e-12, "{got:?}");
        }
        assert_eq!(m.levels[0][1].children, [2, 3]);
        assert_eq!(m.cells()[1].members, [3, 4, 5]);
        assert!((m.grand_mean - 18.5).abs() < 1e-12);
        // Part 1 of batch 0: 13, 14, 18 → s² = (4 + 1 + 9) / 2 = 7.
        assert!((m.cells()[1].sd - 7.0_f64.sqrt()).abs() < 1e-12);
    }

    #[test]
    fn variation_by_factor_and_within() {
        let (values, factors) = shafts();
        let m = multi_vari(&values, &factors).unwrap();
        let v = m.factor_variation();
        // Batches: 24 − 13 = 11. Parts within batch: (15 − 11 + 26 − 22) / 2 = 4.
        assert!((v[0].mean_range - 11.0).abs() < 1e-12 && v[0].groups == 1);
        assert!((v[1].mean_range - 4.0).abs() < 1e-12 && (v[1].max_range - 4.0).abs() < 1e-12 && v[1].groups == 2);
        // Within parts: ranges 2, 5, 5, 4 → 4.
        assert!((m.mean_within_range() - 4.0).abs() < 1e-12);
        // Pooled s² = (1 + 7 + 7 + 4) / 4 (each cell has 2 df).
        assert!((m.pooled_within_sd() - (19.0_f64 / 4.0).sqrt()).abs() < 1e-12);
    }

    #[test]
    fn range_limits_reduce_to_d3_and_d4_for_equal_cells() {
        let (values, factors) = shafts();
        let m = multi_vari(&values, &factors).unwrap();
        let k = ChartConstants::for_subgroup_size(3).unwrap();
        let r_bar = 4.0;
        for limit in m.cell_range_limits() {
            let l = limit.unwrap();
            assert!((l.center - r_bar).abs() < 1e-12);
            assert!((l.upper - k.range_upper * r_bar).abs() < 1e-12);
            assert!((l.lower - k.range_lower * r_bar).abs() < 1e-12 && l.lower == 0.0);
        }
        // D₄ for n = 3 is 2.574 in published tables.
        assert!((k.range_upper - 2.574).abs() < 1e-3);
    }

    #[test]
    fn crossed_factors_single_factor_and_errors() {
        // Crossed: every operator measures both parts.
        let values = [1.0, 2.0, 3.0, 5.0];
        let m = multi_vari(&values, &[vec![0, 0, 1, 1], vec![0, 1, 0, 1]]).unwrap();
        assert_eq!(m.cells().len(), 4);
        assert!(m.cell_range_limits().iter().all(Option::is_none), "single readings have no range");
        assert!(m.mean_within_range().is_nan() && m.pooled_within_sd().is_nan());
        let one = multi_vari(&values, &[vec![0, 1, 0, 1]]).unwrap();
        assert!((one.levels[0][0].mean - 2.0).abs() < 1e-12 && (one.levels[0][1].mean - 3.5).abs() < 1e-12);
        assert!(multi_vari(&values, &[]).is_err());
        assert!(multi_vari(&values, &vec![vec![0; 4]; 5]).is_err());
        assert!(multi_vari(&values, &[vec![0; 3]]).is_err());
        assert!(multi_vari(&[f64::NAN], &[vec![0]]).is_err());
        assert!(multi_vari(&[], &[vec![]]).is_err());
    }
}
