//! Choosing the number of k-means clusters: the gap statistic and average
//! silhouette widths across `k`.
//!
//! **Silhouette** (Rousseeuw 1987, *J. Comput. Appl. Math.* 20, 53–65): for
//! point `i` in cluster `A`, `a(i)` is its mean Euclidean distance to the
//! other members of `A` and `b(i)` the smallest mean distance to the members
//! of another cluster; `s(i) = (b − a) / max(a, b)`, and `s(i) = 0` in a
//! cluster of one. Widths near 1 mean well-separated clusters; the `k` with
//! the largest average width is suggested.
//!
//! **Gap statistic** (Tibshirani, Walther & Hastie 2001, *J. R. Stat. Soc. B*
//! 63, 411–423): with `W_k` the pooled within-cluster sum of squares of the
//! k-means solution, `Gap(k) = E*[log W_k] − log W_k`, the expectation
//! estimated from `B` reference data sets drawn uniformly over the box
//! spanned by the data (or by its principal components, which follows the
//! data's shape), each clustered the same way. `s_k = sd(log W*_k) √(1 + 1/B)`
//! (the standard deviation with divisor `B − 1`), and the suggested `k` is
//! the smallest with `Gap(k) ≥ Gap(k+1) − s_{k+1}`.

use crate::kmeans::{KMeansOptions, k_means};
use rayon::prelude::*;
use stat_core::eigen::symmetric_eigen;
use stat_core::rng::Rng;
use stat_core::{Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Silhouette widths of a clustering.
#[derive(Debug, Clone, PartialEq)]
pub struct Silhouette {
    /// Width of each point.
    pub widths: Vec<f64>,
    /// Average width in each cluster.
    pub cluster_average: Vec<f64>,
    /// Average width over all points.
    pub average: f64,
}

/// Silhouette widths of `assignments` (cluster `0..k` of each point) with
/// Euclidean distances.
///
/// # Errors
/// Mismatched lengths, an assignment outside `0..k`, fewer than two
/// clusters, or points of different dimensions.
pub fn silhouette(points: &[Vec<f64>], assignments: &[usize], k: usize) -> Result<Silhouette> {
    let n = points.len();
    if assignments.len() != n {
        return Err(invalid("one cluster per point"));
    }
    if k < 2 || assignments.iter().any(|&a| a >= k) {
        return Err(invalid("silhouettes need at least two clusters and assignments below k"));
    }
    let d = points.first().map_or(0, Vec::len);
    if points.iter().any(|p| p.len() != d) {
        return Err(invalid("all points need the same number of coordinates"));
    }
    let size: Vec<usize> = (0..k).map(|c| assignments.iter().filter(|&&a| a == c).count()).collect();
    let widths: Vec<f64> = (0..n)
        .into_par_iter()
        .map(|i| {
            let own = assignments[i];
            if size[own] <= 1 {
                return 0.0;
            }
            let mut sums = vec![0.0; k];
            for (j, p) in points.iter().enumerate() {
                if j != i {
                    sums[assignments[j]] += points[i].iter().zip(p).map(|(a, b)| (a - b) * (a - b)).sum::<f64>().sqrt();
                }
            }
            let a = sums[own] / (size[own] - 1) as f64;
            let b = (0..k)
                .filter(|&c| c != own && size[c] > 0)
                .map(|c| sums[c] / size[c] as f64)
                .fold(f64::INFINITY, f64::min);
            let m = a.max(b);
            if m > 0.0 && b.is_finite() { (b - a) / m } else { 0.0 }
        })
        .collect();
    let cluster_average = (0..k)
        .map(|c| {
            let v: Vec<f64> = (0..n).filter(|&i| assignments[i] == c).map(|i| widths[i]).collect();
            if v.is_empty() { f64::NAN } else { v.iter().sum::<f64>() / v.len() as f64 }
        })
        .collect();
    let average = widths.iter().sum::<f64>() / n as f64;
    Ok(Silhouette { widths, cluster_average, average })
}

/// The box reference data are drawn from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GapReference {
    /// Uniform over the range of each variable.
    #[default]
    DataRange,
    /// Uniform over the range of each principal component, rotated back
    /// (Tibshirani et al.'s second method; follows the data's shape).
    PrincipalComponents,
}

/// Options for [`choose_cluster_number`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GapOptions {
    /// Largest `k` examined (from 1).
    pub max_k: usize,
    /// Reference data sets `B`.
    pub references: usize,
    /// Where reference data are drawn.
    pub reference: GapReference,
    /// Seed of the reference draws and the k-means starts.
    pub seed: u64,
    /// k-means starts per fit.
    pub starts: usize,
    /// Compute silhouettes only up to this many points (they need all
    /// pairwise distances).
    pub max_silhouette_points: usize,
}

impl Default for GapOptions {
    fn default() -> Self {
        Self {
            max_k: 10,
            references: 50,
            reference: GapReference::DataRange,
            seed: 1,
            starts: 5,
            max_silhouette_points: 5000,
        }
    }
}

/// Gap and silhouette at one `k`.
#[derive(Debug, Clone, PartialEq)]
pub struct ClusterNumberRow {
    /// Number of clusters.
    pub k: usize,
    /// `log W_k` of the data.
    pub log_w: f64,
    /// Mean `log W*_k` of the reference data sets.
    pub expected_log_w: f64,
    /// `Gap(k)`.
    pub gap: f64,
    /// `s_k`.
    pub std_error: f64,
    /// Average silhouette width (`None` for `k = 1` or too many points).
    pub silhouette: Option<f64>,
}

/// Gap statistic and silhouettes for `k = 1, …, max_k`.
#[derive(Debug, Clone, PartialEq)]
pub struct ClusterNumber {
    /// One row per `k`.
    pub rows: Vec<ClusterNumberRow>,
    /// Smallest `k` with `Gap(k) ≥ Gap(k+1) − s_{k+1}` (Tibshirani et al.
    /// 2001); `max_k` when none qualifies.
    pub gap_choice: usize,
    /// `k` with the largest average silhouette width, when computed.
    pub silhouette_choice: Option<usize>,
}

/// Gap statistics from observed `log_w` (index `k − 1`) and reference
/// `reference_log_w[b][k − 1]`: `(E*[log W], Gap, s)` for each `k`.
#[must_use]
pub fn gap_from_reference(log_w: &[f64], reference_log_w: &[Vec<f64>]) -> Vec<(f64, f64, f64)> {
    let b = reference_log_w.len() as f64;
    (0..log_w.len())
        .map(|k| {
            let v: Vec<f64> = reference_log_w.iter().map(|r| r[k]).collect();
            let mean = v.iter().sum::<f64>() / b;
            let var = if b > 1.0 { v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (b - 1.0) } else { 0.0 };
            (mean, mean - log_w[k], (var * (1.0 + 1.0 / b)).sqrt())
        })
        .collect()
}

/// Tibshirani et al.'s rule: the smallest `k` (1-based) with
/// `Gap(k) ≥ Gap(k+1) − s_{k+1}`, or the largest `k` examined.
#[must_use]
pub fn gap_rule(gap: &[f64], se: &[f64]) -> usize {
    (0..gap.len().saturating_sub(1)).find(|&k| gap[k] >= gap[k + 1] - se[k + 1]).map_or(gap.len(), |k| k + 1)
}

fn log_within(points: &[Vec<f64>], k: usize, starts: usize, seed: u64) -> Result<(f64, Vec<usize>)> {
    let fit = k_means(points, &KMeansOptions { k, starts, max_iterations: 300, seed })?;
    Ok((fit.total_within_ss().max(f64::MIN_POSITIVE).ln(), fit.assignments))
}

/// Gap statistic and average silhouette width of k-means solutions for
/// `k = 1, …, max_k` (see the module documentation).
///
/// # Errors
/// Fewer than three points, `max_k < 2` or larger than the number of
/// points, no reference sets, points of different dimensions, or non-finite
/// values.
pub fn choose_cluster_number(points: &[Vec<f64>], options: &GapOptions) -> Result<ClusterNumber> {
    let n = points.len();
    if n < 3 {
        return Err(invalid("choosing the number of clusters needs at least three points"));
    }
    if options.max_k < 2 || options.max_k > n {
        return Err(invalid(format!("the largest k must be between 2 and the number of points ({n})")));
    }
    if options.references == 0 {
        return Err(invalid("use at least one reference data set"));
    }
    let d = points[0].len();
    if points.iter().any(|p| p.len() != d) {
        return Err(invalid("all points need the same number of coordinates"));
    }
    if points.iter().flatten().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "cluster data" });
    }
    let ks: Vec<usize> = (1..=options.max_k).collect();
    let observed: Vec<(f64, Vec<usize>)> =
        ks.iter().map(|&k| log_within(points, k, options.starts, options.seed)).collect::<Result<_>>()?;
    // The reference box, in data coordinates or principal-component ones.
    let mean: Vec<f64> = (0..d).map(|j| points.iter().map(|p| p[j]).sum::<f64>() / n as f64).collect();
    let rotation: Vec<Vec<f64>> = match options.reference {
        GapReference::DataRange => (0..d).map(|i| (0..d).map(|j| f64::from(u8::from(i == j))).collect()).collect(),
        GapReference::PrincipalComponents => {
            let cov: Vec<Vec<f64>> = (0..d)
                .map(|a| (0..d).map(|b| points.iter().map(|p| (p[a] - mean[a]) * (p[b] - mean[b])).sum()).collect())
                .collect();
            symmetric_eigen(&cov)?.1
        }
    };
    // Scores on the rotated axes: s_c = (p − mean) · v_c.
    let scores: Vec<Vec<f64>> = points
        .iter()
        .map(|p| rotation.iter().map(|v| (0..d).map(|j| (p[j] - mean[j]) * v[j]).sum()).collect())
        .collect();
    let lo: Vec<f64> = (0..d).map(|c| scores.iter().map(|s| s[c]).fold(f64::INFINITY, f64::min)).collect();
    let hi: Vec<f64> = (0..d).map(|c| scores.iter().map(|s| s[c]).fold(f64::NEG_INFINITY, f64::max)).collect();
    let reference: Vec<Vec<f64>> = (0..options.references)
        .into_par_iter()
        .map(|b| {
            // Each reference set has its own seed, so results do not depend
            // on the thread schedule.
            let seed = options.seed.wrapping_add(0x9E37_79B9_7F4A_7C15_u64.wrapping_mul(b as u64 + 1));
            let mut rng = Rng::new(seed);
            let draws: Vec<Vec<f64>> = (0..n)
                .map(|_| {
                    let s: Vec<f64> = (0..d).map(|c| lo[c] + (hi[c] - lo[c]) * rng.next_f64()).collect();
                    (0..d).map(|j| mean[j] + (0..d).map(|c| s[c] * rotation[c][j]).sum::<f64>()).collect()
                })
                .collect();
            ks.iter().map(|&k| log_within(&draws, k, options.starts, seed).map(|r| r.0)).collect::<Result<Vec<f64>>>()
        })
        .collect::<Result<_>>()?;
    let log_w: Vec<f64> = observed.iter().map(|o| o.0).collect();
    let stats = gap_from_reference(&log_w, &reference);
    let with_silhouette = n <= options.max_silhouette_points;
    let mut rows = Vec::new();
    for (i, &k) in ks.iter().enumerate() {
        let silhouette = (with_silhouette && k >= 2)
            .then(|| silhouette(points, &observed[i].1, k).map(|s| s.average))
            .transpose()?;
        rows.push(ClusterNumberRow {
            k,
            log_w: log_w[i],
            expected_log_w: stats[i].0,
            gap: stats[i].1,
            std_error: stats[i].2,
            silhouette,
        });
    }
    let gaps: Vec<f64> = rows.iter().map(|r| r.gap).collect();
    let ses: Vec<f64> = rows.iter().map(|r| r.std_error).collect();
    let silhouette_choice = rows
        .iter()
        .filter_map(|r| r.silhouette.map(|s| (r.k, s)))
        .fold(None::<(usize, f64)>, |best, (k, s)| match best {
            Some((_, b)) if b >= s => best,
            _ => Some((k, s)),
        })
        .map(|b| b.0);
    Ok(ClusterNumber { rows, gap_choice: gap_rule(&gaps, &ses), silhouette_choice })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol * b.abs().max(1.0), "{a} vs {b}");
    }

    /// Twelve points in three groups; silhouette widths from R
    /// `cluster::silhouette(cl, dist(x))` (`tests/data/cluster_number_reference.R`).
    const X: [[f64; 2]; 12] = [
        [1.0, 1.2],
        [1.5, 0.8],
        [0.7, 1.9],
        [1.9, 1.4],
        [5.0, 5.5],
        [5.6, 4.9],
        [4.4, 5.2],
        [6.1, 6.0],
        [9.0, 1.0],
        [8.2, 1.6],
        [9.5, 0.4],
        [3.5, 3.0],
    ];
    const CL: [usize; 12] = [0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 0];

    #[test]
    fn silhouette_matches_cluster_package() {
        let pts: Vec<Vec<f64>> = X.iter().map(|p| p.to_vec()).collect();
        let s = silhouette(&pts, &CL, 3).unwrap();
        for (w, r) in s.widths.iter().zip(SIL) {
            close(*w, r, 1e-12);
        }
        close(s.average, SIL_AVG, 1e-12);
        // A cluster of one has width 0.
        let mut cl = CL;
        cl[11] = 3;
        assert_eq!(silhouette(&pts, &cl, 4).unwrap().widths[11], 0.0);
        assert!(silhouette(&pts, &CL, 1).is_err());
    }

    /// The gap arithmetic, given log W and reference log W* (R's clusGap
    /// formulas evaluated on the same numbers in the reference script), and
    /// Tibshirani's selection rule.
    #[test]
    fn gap_arithmetic_and_rule() {
        let log_w = [3.0, 2.0, 1.2, 1.1];
        let reference = vec![vec![3.1, 2.6, 2.2, 1.9], vec![3.3, 2.8, 2.1, 2.0], vec![3.2, 2.7, 2.3, 1.8]];
        let g = gap_from_reference(&log_w, &reference);
        close(g[0].1, 0.2, 1e-14);
        close(g[2].1, 1.0, 1e-14);
        // sd(c(2.2, 2.1, 2.3)) * sqrt(1 + 1/3) = 0.1 * 1.1547005383792515.
        close(g[2].2, 0.115_470_053_837_925_15, 1e-14);
        let gaps: Vec<f64> = g.iter().map(|t| t.1).collect();
        let ses: Vec<f64> = g.iter().map(|t| t.2).collect();
        // Gaps 0.2, 0.7, 1.0, 0.8: k = 3 is the first with Gap(k) ≥ Gap(k+1) − s.
        assert_eq!(gap_rule(&gaps, &ses), 3);
        assert_eq!(gap_rule(&[0.1, 0.2, 0.3], &[0.01, 0.01, 0.01]), 3);
    }

    /// Simulated data with three well-separated groups: both criteria pick 3,
    /// and the result does not depend on the thread schedule.
    #[test]
    fn known_structure() {
        let mut rng = Rng::new(9);
        let centres = [[0.0, 0.0], [6.0, 0.0], [3.0, 5.0]];
        let pts: Vec<Vec<f64>> =
            (0..90).map(|i| centres[i % 3].iter().map(|c| c + 0.6 * rng.normal()).collect()).collect();
        for reference in [GapReference::DataRange, GapReference::PrincipalComponents] {
            let opts = GapOptions { max_k: 6, references: 20, reference, ..GapOptions::default() };
            let r = choose_cluster_number(&pts, &opts).unwrap();
            assert_eq!(r.gap_choice, 3, "{reference:?}");
            assert_eq!(r.silhouette_choice, Some(3));
            assert_eq!(r.rows.len(), 6);
            assert!(r.rows[0].silhouette.is_none());
            assert_eq!(r, choose_cluster_number(&pts, &opts).unwrap());
        }
        // One diffuse cloud: the gap rule picks a single cluster.
        let cloud: Vec<Vec<f64>> = (0..60).map(|_| vec![rng.normal(), rng.normal()]).collect();
        let r =
            choose_cluster_number(&cloud, &GapOptions { max_k: 5, references: 20, ..GapOptions::default() }).unwrap();
        assert_eq!(r.gap_choice, 1);
    }

    const SIL: [f64; 12] = [
        0.775_092_301_557_660_4,
        0.761_431_636_254_588_5,
        0.721_539_390_796_299_1,
        0.752_231_436_246_829_2,
        0.820_994_748_842_706,
        0.783_636_837_837_783_4,
        0.718_296_874_416_763_5,
        0.750_365_672_231_729_2,
        0.846_488_996_073_216_3,
        0.713_867_607_540_038_6,
        0.806_150_618_501_189,
        0.063_619_603_576_672_07,
    ];
    const SIL_AVG: f64 = 0.709_476_310_322_956_2;
}
