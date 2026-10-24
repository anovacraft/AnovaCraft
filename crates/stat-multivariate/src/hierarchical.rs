//! Agglomerative hierarchical clustering.
//!
//! Starting from single points, the two closest clusters are merged
//! repeatedly until one remains. Cluster distances follow the
//! Lance–Williams recurrence (Lance & Williams 1967, *Computer Journal* 9):
//!
//! * **single** linkage — the closest pair of members;
//! * **complete** — the farthest pair;
//! * **average** (UPGMA) — the mean of all member distances;
//! * **Ward** — the increase in within-cluster sum of squares. Distances are
//!   updated on squared Euclidean distances,
//!   `d²(k, i∪j) = [(nₖ+nᵢ)d²(k,i) + (nₖ+nⱼ)d²(k,j) − nₖ d²(i,j)] / (nₖ+nᵢ+nⱼ)`,
//!   and the reported merge height is `√d²` (Ward 1963, *JASA* 58).
//!
//! [`hierarchical_from_distances`] runs the same updates on a given
//! distance matrix (Ward on the squared given distances).
//!
//! The merge list uses the usual convention: points are clusters `0..n`, and
//! merge `m` creates cluster `n + m`. Ties are broken by the smaller pair of
//! indices. Each step finds the closest pair from cached nearest neighbours, so
//! typical cost is `O(n²)` time and memory.

use stat_core::{Result, StatError};

use crate::distance::DistanceMatrix;

/// Linkage criterion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Linkage {
    /// Minimum distance between members.
    Single,
    /// Maximum distance between members.
    Complete,
    /// Average distance between members (UPGMA).
    Average,
    /// Ward's minimum-variance criterion.
    Ward,
}

impl Linkage {
    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Complete => "complete",
            Self::Average => "average",
            Self::Ward => "Ward",
        }
    }
}

/// One merge of two clusters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Merge {
    /// First cluster merged (point index, or `n + m` for merge `m`).
    pub left: usize,
    /// Second cluster merged.
    pub right: usize,
    /// Distance at which they merged.
    pub height: f64,
    /// Number of points in the new cluster.
    pub size: usize,
}

/// The full merge history.
#[derive(Debug, Clone, PartialEq)]
pub struct Dendrogram {
    /// Number of points.
    pub n: usize,
    /// The `n − 1` merges, in order.
    pub merges: Vec<Merge>,
    /// The linkage used.
    pub linkage: Linkage,
}

/// Largest number of points accepted (the distance matrix needs `n²` memory).
pub const MAX_POINTS: usize = 5000;

/// Cluster `points` hierarchically.
///
/// # Errors
/// [`StatError::InvalidArgument`] for fewer than two points, more than
/// [`MAX_POINTS`], or points of different dimensions;
/// [`StatError::NonFiniteInput`] for NaN or ±∞.
pub fn hierarchical_clustering(points: &[Vec<f64>], linkage: Linkage) -> Result<Dendrogram> {
    let n = points.len();
    if !(2..=MAX_POINTS).contains(&n) {
        return Err(StatError::InvalidArgument {
            message: format!("hierarchical clustering needs 2 to {MAX_POINTS} points (got {n})"),
        });
    }
    let d = points[0].len();
    if points.iter().any(|p| p.len() != d) {
        return Err(StatError::InvalidArgument { message: "all points need the same number of coordinates".into() });
    }
    if points.iter().flatten().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "clustering data" });
    }
    // Ward works with squared distances, the others with distances.
    let squared = linkage == Linkage::Ward;
    let mut dist = vec![0.0; n * n];
    for i in 0..n {
        for j in i + 1..n {
            let d2: f64 = points[i].iter().zip(&points[j]).map(|(a, b)| (a - b) * (a - b)).sum();
            let v = if squared { d2 } else { d2.sqrt() };
            dist[i * n + j] = v;
            dist[j * n + i] = v;
        }
    }
    Ok(agglomerate(n, dist, linkage))
}

/// Cluster objects hierarchically from a given distance matrix (for
/// example Gower distances of mixed data; see [`crate::distance`]).
///
/// The Lance–Williams updates are those of [`hierarchical_clustering`],
/// which this equals on Euclidean distances. Ward's criterion is applied
/// to the squared given distances and heights are reported on the
/// distance scale, as SciPy's `linkage(method = "ward")` on a condensed
/// matrix and R's `hclust(method = "ward.D2")` do; for non-Euclidean
/// distances the heights are no longer within-cluster sums of squares but
/// the updates are still well defined.
///
/// # Errors
/// [`StatError::InvalidArgument`] for fewer than two objects or more than
/// [`MAX_POINTS`].
pub fn hierarchical_from_distances(d: &DistanceMatrix, linkage: Linkage) -> Result<Dendrogram> {
    let n = d.n();
    if !(2..=MAX_POINTS).contains(&n) {
        return Err(StatError::InvalidArgument {
            message: format!("hierarchical clustering needs 2 to {MAX_POINTS} objects (got {n})"),
        });
    }
    let dist = if linkage == Linkage::Ward { d.values().iter().map(|v| v * v).collect() } else { d.values().to_vec() };
    Ok(agglomerate(n, dist, linkage))
}

/// The agglomeration on a full `n × n` matrix `dist` (squared distances
/// for Ward, distances otherwise), which it overwrites.
fn agglomerate(n: usize, mut dist: Vec<f64>, linkage: Linkage) -> Dendrogram {
    let squared = linkage == Linkage::Ward;
    let mut active = vec![true; n];
    let mut size = vec![1usize; n];
    let mut id: Vec<usize> = (0..n).collect();
    let nearest = |dist: &[f64], active: &[bool], i: usize| -> (usize, f64) {
        let mut best = (usize::MAX, f64::INFINITY);
        for j in 0..n {
            if j != i && active[j] && dist[i * n + j] < best.1 {
                best = (j, dist[i * n + j]);
            }
        }
        best
    };
    let mut nn: Vec<(usize, f64)> = (0..n).map(|i| nearest(&dist, &active, i)).collect();
    let mut merges = Vec::with_capacity(n - 1);
    for m in 0..n - 1 {
        // Closest pair; ties go to the smaller first index, then smaller second.
        let (mut a, mut best) = (usize::MAX, f64::INFINITY);
        for i in 0..n {
            if active[i] && (nn[i].1 < best || (nn[i].1 == best && a != usize::MAX && i.min(nn[i].0) < a.min(nn[a].0)))
            {
                a = i;
                best = nn[i].1;
            }
        }
        let b = nn[a].0;
        let (a, b) = (a.min(b), a.max(b));
        let dab = dist[a * n + b];
        let (na, nb) = (size[a] as f64, size[b] as f64);
        for k in 0..n {
            if !active[k] || k == a || k == b {
                continue;
            }
            let (dak, dbk) = (dist[a * n + k], dist[b * n + k]);
            let nk = size[k] as f64;
            let v = match linkage {
                Linkage::Single => dak.min(dbk),
                Linkage::Complete => dak.max(dbk),
                Linkage::Average => (na * dak + nb * dbk) / (na + nb),
                Linkage::Ward => ((nk + na) * dak + (nk + nb) * dbk - nk * dab) / (nk + na + nb),
            };
            dist[a * n + k] = v;
            dist[k * n + a] = v;
        }
        active[b] = false;
        let (left, right) = (id[a].min(id[b]), id[a].max(id[b]));
        size[a] += size[b];
        merges.push(Merge { left, right, height: if squared { dab.max(0.0).sqrt() } else { dab }, size: size[a] });
        id[a] = n + m;
        // Refresh nearest neighbours that may have changed.
        for k in 0..n {
            if !active[k] {
                continue;
            }
            if k == a || nn[k].0 == a || nn[k].0 == b {
                nn[k] = nearest(&dist, &active, k);
            } else if dist[k * n + a] < nn[k].1 {
                nn[k] = (a, dist[k * n + a]);
            }
        }
    }
    Dendrogram { n, merges, linkage }
}

impl Dendrogram {
    /// Cluster labels (`0..k`, in order of first appearance) from stopping
    /// before the last `k − 1` merges.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] unless `1 ≤ k ≤ n`.
    pub fn cut(&self, k: usize) -> Result<Vec<usize>> {
        if k == 0 || k > self.n {
            return Err(StatError::InvalidArgument { message: format!("k must be between 1 and {}", self.n) });
        }
        let total = 2 * self.n - 1;
        let mut parent: Vec<usize> = (0..total).collect();
        fn find(parent: &mut [usize], x: usize) -> usize {
            let mut r = x;
            while parent[r] != r {
                r = parent[r];
            }
            let mut c = x;
            while parent[c] != r {
                let next = parent[c];
                parent[c] = r;
                c = next;
            }
            r
        }
        for (m, merge) in self.merges.iter().take(self.n - k).enumerate() {
            let new = self.n + m;
            let (l, r) = (find(&mut parent, merge.left), find(&mut parent, merge.right));
            parent[l] = new;
            parent[r] = new;
        }
        let mut labels = vec![usize::MAX; total];
        let mut next = 0;
        Ok((0..self.n)
            .map(|i| {
                let root = find(&mut parent, i);
                if labels[root] == usize::MAX {
                    labels[root] = next;
                    next += 1;
                }
                labels[root]
            })
            .collect())
    }

    /// Leaf order for drawing, so that no branches cross.
    #[must_use]
    pub fn leaf_order(&self) -> Vec<usize> {
        let mut order = Vec::with_capacity(self.n);
        let mut stack = vec![self.n + self.merges.len() - 1];
        if self.merges.is_empty() {
            return (0..self.n).collect();
        }
        while let Some(c) = stack.pop() {
            if c < self.n {
                order.push(c);
            } else {
                let m = self.merges[c - self.n];
                stack.push(m.right);
                stack.push(m.left);
            }
        }
        order
    }

    /// Line segments of the dendrogram: one polyline per merge
    /// (`[x_left, h_left] → [x_left, h] → [x_right, h] → [x_right, h_right]`),
    /// with leaves at `x = 1, 2, …` in [`leaf_order`](Self::leaf_order) and height 0.
    #[must_use]
    pub fn segments(&self) -> Vec<Vec<[f64; 2]>> {
        let order = self.leaf_order();
        let mut pos = vec![0.0; 2 * self.n - 1];
        let mut height = vec![0.0; 2 * self.n - 1];
        for (x, &leaf) in order.iter().enumerate() {
            pos[leaf] = (x + 1) as f64;
        }
        let mut out = Vec::with_capacity(self.merges.len());
        for (m, merge) in self.merges.iter().enumerate() {
            let (l, r) = (merge.left, merge.right);
            out.push(vec![[pos[l], height[l]], [pos[l], merge.height], [pos[r], merge.height], [pos[r], height[r]]]);
            pos[self.n + m] = (pos[l] + pos[r]) / 2.0;
            height[self.n + m] = merge.height;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn points() -> Vec<Vec<f64>> {
        let m = crate::pca::tests::data();
        (0..m.nrows()).map(|i| m.row(i)).collect()
    }

    /// Merge heights and three-cluster partitions match SciPy 1.18 `linkage` /
    /// `fcluster(maxclust = 3)` on the same points.
    #[test]
    fn matches_reference_linkages() {
        let cases: [(Linkage, [f64; 9], [usize; 10]); 4] = [
            (
                Linkage::Single,
                [
                    0.244_948_97,
                    0.374_165_74,
                    0.412_310_56,
                    0.616_441_4,
                    0.640_312_42,
                    0.648_074_07,
                    0.670_820_39,
                    0.714_142_84,
                    0.938_083_15,
                ],
                [0, 1, 0, 0, 2, 0, 0, 1, 0, 1],
            ),
            (
                Linkage::Complete,
                [
                    0.244_948_97,
                    0.374_165_74,
                    0.591_607_98,
                    0.616_441_4,
                    0.748_331_48,
                    0.781_024_97,
                    1.044_030_65,
                    2.111_871_21,
                    3.706_750_6,
                ],
                [0, 1, 0, 2, 0, 0, 2, 1, 2, 1],
            ),
            (
                Linkage::Average,
                [
                    0.244_948_97,
                    0.374_165_74,
                    0.501_959_27,
                    0.616_441_4,
                    0.698_202_77,
                    0.710_668_7,
                    1.005_507_28,
                    1.346_752_21,
                    2.080_762_86,
                ],
                [0, 1, 0, 2, 0, 0, 2, 1, 2, 1],
            ),
            (
                Linkage::Ward,
                [
                    0.244_948_97,
                    0.374_165_74,
                    0.571_547_61,
                    0.616_441_4,
                    0.743_863_79,
                    0.778_888_1,
                    1.193_035_34,
                    2.292_742_17,
                    4.254_331_13,
                ],
                [0, 1, 0, 2, 0, 0, 2, 1, 2, 1],
            ),
        ];
        for (linkage, heights, labels) in cases {
            let d = hierarchical_clustering(&points(), linkage).unwrap();
            for (m, h) in d.merges.iter().zip(heights) {
                assert!((m.height - h).abs() < 1e-8, "{linkage:?}: {} vs {h}", m.height);
            }
            assert_eq!(d.cut(3).unwrap(), labels, "{linkage:?}");
            assert_eq!(d.merges.last().unwrap().size, 10);
        }
    }

    /// The nearest-neighbour cache gives the same heights as a full search at
    /// every step, on 80 pseudo-random points in 3 dimensions.
    #[test]
    fn cached_search_matches_brute_force() {
        let mut state = 0x1234_5678_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 11) as f64 / (1u64 << 53) as f64
        };
        let pts: Vec<Vec<f64>> = (0..80).map(|_| (0..3).map(|_| next()).collect()).collect();
        for linkage in [Linkage::Single, Linkage::Complete, Linkage::Average, Linkage::Ward] {
            let fast = hierarchical_clustering(&pts, linkage).unwrap();
            let slow = brute_force(&pts, linkage);
            for (a, b) in fast.merges.iter().zip(&slow) {
                assert!((a.height - b).abs() < 1e-12, "{linkage:?}");
            }
        }
    }

    fn brute_force(pts: &[Vec<f64>], linkage: Linkage) -> Vec<f64> {
        let n = pts.len();
        let sq = linkage == Linkage::Ward;
        let mut d = vec![vec![0.0; n]; n];
        for i in 0..n {
            for j in 0..n {
                let d2: f64 = pts[i].iter().zip(&pts[j]).map(|(a, b)| (a - b) * (a - b)).sum();
                d[i][j] = if sq { d2 } else { d2.sqrt() };
            }
        }
        let mut alive = vec![true; n];
        let mut size = vec![1.0; n];
        let mut heights = Vec::new();
        for _ in 0..n - 1 {
            let mut best = (0, 0, f64::INFINITY);
            for i in 0..n {
                for j in i + 1..n {
                    if alive[i] && alive[j] && d[i][j] < best.2 {
                        best = (i, j, d[i][j]);
                    }
                }
            }
            let (a, b, dab) = best;
            for k in 0..n {
                if alive[k] && k != a && k != b {
                    let v = match linkage {
                        Linkage::Single => d[a][k].min(d[b][k]),
                        Linkage::Complete => d[a][k].max(d[b][k]),
                        Linkage::Average => (size[a] * d[a][k] + size[b] * d[b][k]) / (size[a] + size[b]),
                        Linkage::Ward => {
                            ((size[k] + size[a]) * d[a][k] + (size[k] + size[b]) * d[b][k] - size[k] * dab)
                                / (size[k] + size[a] + size[b])
                        }
                    };
                    d[a][k] = v;
                    d[k][a] = v;
                }
            }
            alive[b] = false;
            size[a] += size[b];
            heights.push(if sq { dab.sqrt() } else { dab });
        }
        heights
    }

    /// Clustering a Euclidean distance matrix reproduces clustering the
    /// points themselves, merge for merge.
    #[test]
    fn distance_matrix_input_matches_points() {
        let pts = points();
        let columns: Vec<crate::distance::DistanceColumn> = (0..3)
            .map(|k| crate::distance::DistanceColumn {
                values: pts.iter().map(|r| r[k]).collect(),
                kind: crate::distance::VariableKind::Numeric,
            })
            .collect();
        let d = crate::distance::distance_matrix(&columns, crate::distance::DistanceMetric::Euclidean).unwrap();
        for linkage in [Linkage::Single, Linkage::Complete, Linkage::Average, Linkage::Ward] {
            let a = hierarchical_clustering(&pts, linkage).unwrap();
            let b = hierarchical_from_distances(&d, linkage).unwrap();
            for (x, y) in a.merges.iter().zip(&b.merges) {
                assert_eq!((x.left, x.right, x.size), (y.left, y.right, y.size), "{linkage:?}");
                assert!((x.height - y.height).abs() < 1e-12, "{linkage:?}");
            }
        }
        let one = DistanceMatrix::new(1, vec![0.0]).unwrap();
        assert!(hierarchical_from_distances(&one, Linkage::Average).is_err());
    }

    #[test]
    fn cuts_order_and_segments() {
        let d = hierarchical_clustering(&points(), Linkage::Average).unwrap();
        assert_eq!(d.cut(1).unwrap(), vec![0; 10]);
        assert_eq!(d.cut(10).unwrap(), (0..10).collect::<Vec<_>>());
        let mut order = d.leaf_order();
        order.sort_unstable();
        assert_eq!(order, (0..10).collect::<Vec<_>>());
        let segs = d.segments();
        assert_eq!(segs.len(), 9);
        // Heights never decrease along a merge's vertical lines.
        for s in &segs {
            assert!(s[1][1] >= s[0][1] && s[2][1] >= s[3][1]);
        }
        assert!(d.cut(0).is_err());
        assert!(hierarchical_clustering(&[vec![1.0]], Linkage::Single).is_err());
    }
}
