//! K-means clustering.
//!
//! Minimizes the total within-cluster sum of squared Euclidean distances
//! `W = Σₖ Σ_{i∈k} ‖xᵢ − cₖ‖²` by Lloyd's algorithm (Lloyd 1982, *IEEE Trans.
//! Inf. Theory* 28): assign every point to its nearest centre, move each centre
//! to the mean of its points, repeat until no assignment changes. Starting
//! centres come from k-means++ seeding (Arthur & Vassilvitskii 2007, "k-means++:
//! the advantages of careful seeding", *SODA*): the first uniformly at random,
//! each next with probability proportional to its squared distance from the
//! nearest centre chosen so far. Several seeded starts are run and the solution
//! with the smallest `W` is kept. An emptied cluster is restarted at the point
//! farthest from its centre among clusters with more than one point. A point
//! equally near several centres keeps its current cluster if that is one of
//! them, else joins the lowest-numbered, so duplicate points cannot flip
//! between equal centres and the iterations converge.
//!
//! Starting centres can also be given (Hartigan & Wong 1979, *Appl. Statist.*
//! 28, take them as an input; so does R's `kmeans(x, centers = )`): Lloyd's
//! iterations then start from exactly those centres, alone or as one more
//! start beside the seeded ones, the smallest `W` winning (the given start
//! on a tie).
//!
//! Clusters are numbered in order of first appearance in the data, so labels do
//! not depend on the random start.

use stat_core::{Result, StatError};

/// Options for [`k_means`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KMeansOptions {
    /// Number of clusters.
    pub k: usize,
    /// Number of k-means++ starts.
    pub starts: usize,
    /// Most Lloyd iterations per start.
    pub max_iterations: usize,
    /// Seed of the starts.
    pub seed: u64,
}

impl KMeansOptions {
    /// `k` clusters, 10 starts, 300 iterations, seed 1.
    #[must_use]
    pub fn new(k: usize) -> Self {
        Self { k, starts: 10, max_iterations: 300, seed: 1 }
    }
}

/// A k-means solution.
#[derive(Debug, Clone, PartialEq)]
pub struct KMeans {
    /// Cluster of each point (`0..k`, in order of first appearance).
    pub assignments: Vec<usize>,
    /// Cluster centres.
    pub centers: Vec<Vec<f64>>,
    /// Number of points per cluster.
    pub sizes: Vec<usize>,
    /// Within-cluster sum of squares per cluster.
    pub within_ss: Vec<f64>,
    /// Total sum of squares about the overall mean.
    pub total_ss: f64,
    /// `true` when the solution came from the given starting centres
    /// ([`k_means_from_centers`]), not a seeded start.
    pub from_given_centers: bool,
}

impl KMeans {
    /// Total within-cluster sum of squares `W`.
    #[must_use]
    pub fn total_within_ss(&self) -> f64 {
        self.within_ss.iter().sum()
    }

    /// Between-cluster sum of squares (total − within).
    #[must_use]
    pub fn between_ss(&self) -> f64 {
        self.total_ss - self.total_within_ss()
    }
}

fn dist2(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum()
}

/// SplitMix64, for reproducible starts.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Cluster `points` (each of the same dimension) into `options.k` groups.
///
/// # Errors
/// [`StatError::InvalidArgument`] for `k = 0`, `k > n`, points of different
/// dimensions, or no starts; [`StatError::NonFiniteInput`] for NaN or ±∞.
pub fn k_means(points: &[Vec<f64>], options: &KMeansOptions) -> Result<KMeans> {
    k_means_with(points, options, None, true)
}

/// Cluster `points` from the given starting `centers` (one per cluster, so
/// `options.k` is ignored): Lloyd's iterations from those centres, and when
/// `also_seeded` also `options.starts` k-means++ starts, keeping the solution
/// with the smallest `W` (the given start on a tie).
///
/// # Errors
/// As [`k_means`]; [`StatError::InvalidArgument`] for no centres, more
/// centres than points, or centres of the wrong dimension; non-finite centre
/// coordinates are [`StatError::NonFiniteInput`].
pub fn k_means_from_centers(
    points: &[Vec<f64>],
    centers: &[Vec<f64>],
    options: &KMeansOptions,
    also_seeded: bool,
) -> Result<KMeans> {
    let d = points.first().map_or(0, Vec::len);
    if centers.iter().any(|c| c.len() != d) {
        return Err(StatError::InvalidArgument {
            message: format!("each starting center needs {d} coordinates, one per column"),
        });
    }
    if centers.iter().flatten().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "k-means starting centers" });
    }
    k_means_with(points, &KMeansOptions { k: centers.len(), ..*options }, Some(centers), also_seeded)
}

fn k_means_with(
    points: &[Vec<f64>],
    options: &KMeansOptions,
    given: Option<&[Vec<f64>]>,
    seeded: bool,
) -> Result<KMeans> {
    let (n, k) = (points.len(), options.k);
    if k == 0 || k > n {
        return Err(StatError::InvalidArgument {
            message: format!("k must be between 1 and the number of points ({n})"),
        });
    }
    if seeded && options.starts == 0 {
        return Err(StatError::InvalidArgument { message: "use at least one start".into() });
    }
    let d = points[0].len();
    if points.iter().any(|p| p.len() != d) {
        return Err(StatError::InvalidArgument { message: "all points need the same number of coordinates".into() });
    }
    if points.iter().flatten().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "k-means data" });
    }
    let mut best: Option<(f64, Vec<usize>, bool)> = None;
    let mut consider = |start: Vec<Vec<f64>>, from_given: bool| {
        let (assign, centers, _) = lloyd(points, start, options.max_iterations);
        let w: f64 = points.iter().zip(&assign).map(|(p, &a)| dist2(p, &centers[a])).sum();
        if best.as_ref().is_none_or(|(bw, _, _)| w < *bw - 1e-12 * bw.abs()) {
            best = Some((w, assign, from_given));
        }
    };
    if let Some(c) = given {
        consider(c.to_vec(), true);
    }
    if seeded {
        let mut rng = Rng(options.seed);
        for _ in 0..options.starts {
            consider(seed_centers(points, k, &mut rng), false);
        }
    }
    let (_, raw, from_given_centers) = best.unwrap_or((0.0, vec![0; n], false));
    // Relabel by first appearance.
    let mut map = vec![usize::MAX; k];
    let mut next = 0;
    let assignments: Vec<usize> = raw
        .iter()
        .map(|&a| {
            if map[a] == usize::MAX {
                map[a] = next;
                next += 1;
            }
            map[a]
        })
        .collect();
    let centers = means(points, &assignments, k, d);
    let sizes = (0..k).map(|c| assignments.iter().filter(|&&a| a == c).count()).collect();
    let mut within_ss = vec![0.0; k];
    for (p, &a) in points.iter().zip(&assignments) {
        within_ss[a] += dist2(p, &centers[a]);
    }
    let grand: Vec<f64> = (0..d).map(|j| points.iter().map(|p| p[j]).sum::<f64>() / n as f64).collect();
    let total_ss = points.iter().map(|p| dist2(p, &grand)).sum();
    Ok(KMeans { assignments, centers, sizes, within_ss, total_ss, from_given_centers })
}

/// Lloyd's iterations from `centers` (at most `max_iterations`, at least
/// one): the assignment, the final centres and the iterations run.
fn lloyd(points: &[Vec<f64>], mut centers: Vec<Vec<f64>>, max_iterations: usize) -> (Vec<usize>, Vec<Vec<f64>>, usize) {
    let (n, k, d) = (points.len(), centers.len(), points[0].len());
    let mut assign = vec![usize::MAX; n];
    let mut iterations = 0;
    for _ in 0..max_iterations.max(1) {
        iterations += 1;
        let mut changed = false;
        for (i, p) in points.iter().enumerate() {
            let nearest = nearest_center(p, &centers, assign[i]);
            if assign[i] != nearest {
                assign[i] = nearest;
                changed = true;
            }
        }
        centers = means(points, &assign, k, d);
        // Restart each empty cluster at the point farthest from its centre
        // among clusters with more than one point (so no other cluster is
        // emptied in turn); ties go to the lowest row.
        let mut sizes = vec![0usize; k];
        assign.iter().for_each(|&a| sizes[a] += 1);
        for c in 0..k {
            if sizes[c] == 0 {
                let mut far: Option<(usize, f64)> = None;
                for (i, p) in points.iter().enumerate() {
                    let d2 = dist2(p, &centers[assign[i]]);
                    if sizes[assign[i]] > 1 && far.is_none_or(|(_, best)| d2 > best) {
                        far = Some((i, d2));
                    }
                }
                // k ≤ n, so an empty cluster means another has two or more.
                let Some((i, _)) = far else { break };
                sizes[assign[i]] -= 1;
                sizes[c] = 1;
                assign[i] = c;
                centers = means(points, &assign, k, d);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    (assign, centers, iterations)
}

/// The centre nearest `p`. A tie keeps the point's `current` centre when it
/// is among the nearest, else goes to the lowest-numbered one, so duplicate
/// points between equal centres do not flip from one iteration to the next.
fn nearest_center(p: &[f64], centers: &[Vec<f64>], current: usize) -> usize {
    let dists: Vec<f64> = centers.iter().map(|c| dist2(p, c)).collect();
    let min = dists.iter().copied().fold(f64::INFINITY, f64::min);
    if dists.get(current).is_some_and(|&d| d == min) {
        return current;
    }
    dists.iter().position(|&d| d == min).unwrap_or(0)
}

fn means(points: &[Vec<f64>], assign: &[usize], k: usize, d: usize) -> Vec<Vec<f64>> {
    let mut sums = vec![vec![0.0; d]; k];
    let mut counts = vec![0usize; k];
    for (p, &a) in points.iter().zip(assign) {
        if a < k {
            counts[a] += 1;
            for (s, v) in sums[a].iter_mut().zip(p) {
                *s += v;
            }
        }
    }
    sums.into_iter()
        .zip(counts)
        .map(|(s, c)| if c == 0 { s } else { s.into_iter().map(|v| v / c as f64).collect() })
        .collect()
}

fn seed_centers(points: &[Vec<f64>], k: usize, rng: &mut Rng) -> Vec<Vec<f64>> {
    let n = points.len();
    let mut centers = vec![points[((rng.next() * n as f64) as usize).min(n - 1)].clone()];
    let mut d2: Vec<f64> = points.iter().map(|p| dist2(p, &centers[0])).collect();
    while centers.len() < k {
        let total: f64 = d2.iter().sum();
        let idx = if total > 0.0 {
            let mut target = rng.next() * total;
            let mut chosen = n - 1;
            for (i, &w) in d2.iter().enumerate() {
                if target < w {
                    chosen = i;
                    break;
                }
                target -= w;
            }
            chosen
        } else {
            ((rng.next() * n as f64) as usize).min(n - 1)
        };
        centers.push(points[idx].clone());
        for (i, p) in points.iter().enumerate() {
            d2[i] = d2[i].min(dist2(p, &centers[centers.len() - 1]));
        }
    }
    centers
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three well-separated groups of five points.
    fn blobs() -> Vec<Vec<f64>> {
        let centres = [(0.0, 0.0), (10.0, 0.0), (5.0, 9.0)];
        let offsets = [(0.3, 0.1), (-0.2, 0.4), (0.1, -0.3), (-0.4, -0.2), (0.2, 0.0)];
        let mut out = Vec::new();
        for o in &offsets {
            for c in &centres {
                out.push(vec![c.0 + o.0, c.1 + o.1]);
            }
        }
        out
    }

    #[test]
    fn recovers_separated_groups() {
        let pts = blobs();
        let r = k_means(&pts, &KMeansOptions::new(3)).unwrap();
        assert_eq!(r.sizes, [5, 5, 5]);
        // Points are interleaved by group: labels cycle 0, 1, 2.
        for (i, &a) in r.assignments.iter().enumerate() {
            assert_eq!(a, i % 3);
        }
        assert!((r.centers[1][0] - 10.0).abs() < 1e-12 && r.centers[1][1].abs() < 1e-12);
        // W is the sum of the squared offsets, three times over; W + B = T.
        let expected_w: f64 = 3.0
            * [(0.3, 0.1), (-0.2, 0.4), (0.1, -0.3), (-0.4, -0.2), (0.2, 0.0)]
                .iter()
                .map(|(x, y)| x * x + y * y)
                .sum::<f64>();
        assert!((r.total_within_ss() - expected_w).abs() < 1e-12);
        assert!((r.between_ss() + r.total_within_ss() - r.total_ss).abs() < 1e-9);
    }

    /// R 4.x `kmeans(x, centers = c0, algorithm = "Lloyd")` (and
    /// "Hartigan-Wong", which agrees here): clusters 1 1 3 3 1 3 2 2 2 3 1 3,
    /// centers (1.375, 1.725), (9, 2.3333), (6.4, 8.7), withinss 2.635,
    /// 2.666667, 27. A poor start (three centers in one group) stops at a
    /// local optimum, W = 119.10375, which the seeded starts improve on.
    #[test]
    fn given_centers_match_r() {
        let x: Vec<Vec<f64>> = [
            [1.0, 2.0],
            [1.5, 1.8],
            [5.0, 8.0],
            [8.0, 8.0],
            [1.0, 0.6],
            [9.0, 11.0],
            [8.0, 2.0],
            [10.0, 2.0],
            [9.0, 3.0],
            [4.0, 7.0],
            [2.0, 2.5],
            [6.0, 9.5],
        ]
        .iter()
        .map(|r| r.to_vec())
        .collect();
        let start = vec![vec![1.0, 1.0], vec![5.0, 5.0], vec![9.0, 9.0]];
        let r = k_means_from_centers(&x, &start, &KMeansOptions::new(99), false).unwrap();
        // R numbers 1, 3, 2 in first-appearance order 0, 1, 2.
        assert_eq!(r.assignments, [0, 0, 1, 1, 0, 1, 2, 2, 2, 1, 0, 1]);
        let want = [[1.375, 1.725], [6.4, 8.7], [9.0, 7.0 / 3.0]];
        for (c, w) in r.centers.iter().zip(want) {
            assert!((c[0] - w[0]).abs() < 1e-12 && (c[1] - w[1]).abs() < 1e-12, "{c:?}");
        }
        for (w, want) in r.within_ss.iter().zip([2.635, 27.0, 2.666_666_666_666_667]) {
            assert!((w - want).abs() < 1e-10, "{w}");
        }
        assert!((r.total_ss - 273.099_166_666_666_7).abs() < 1e-9);
        assert!(r.from_given_centers);
        let poor = vec![vec![1.0, 2.0], vec![1.5, 1.8], vec![1.0, 0.6]];
        let local = k_means_from_centers(&x, &poor, &KMeansOptions::new(3), false).unwrap();
        assert!((local.total_within_ss() - 119.103_75).abs() < 1e-9);
        assert_eq!(local.sizes, [3, 8, 1]);
        let both = k_means_from_centers(&x, &poor, &KMeansOptions::new(3), true).unwrap();
        assert!(!both.from_given_centers && both.total_within_ss() < 40.0, "{}", both.total_within_ss());
        // With a good start beside the seeded ones the given start wins ties.
        let again = k_means_from_centers(&x, &start, &KMeansOptions::new(3), true).unwrap();
        assert!(again.from_given_centers && (again.total_within_ss() - r.total_within_ss()).abs() < 1e-12);
        assert!(k_means_from_centers(&x, &[vec![1.0]], &KMeansOptions::new(1), false).is_err());
        assert!(k_means_from_centers(&x, &[vec![f64::NAN, 1.0]], &KMeansOptions::new(1), false).is_err());
        assert!(k_means_from_centers(&x, &[], &KMeansOptions::new(1), false).is_err());
    }

    #[test]
    fn deterministic_and_validated() {
        let pts = blobs();
        let a = k_means(&pts, &KMeansOptions::new(4)).unwrap();
        let b = k_means(&pts, &KMeansOptions::new(4)).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.sizes.iter().sum::<usize>(), 15);
        assert!(a.sizes.iter().all(|&s| s > 0), "no empty clusters");
        let one = k_means(&pts, &KMeansOptions::new(1)).unwrap();
        assert!((one.total_within_ss() - one.total_ss).abs() < 1e-9);
        let all = k_means(&pts, &KMeansOptions::new(15)).unwrap();
        assert!(all.total_within_ss() < 1e-20);
        assert!(k_means(&pts, &KMeansOptions::new(0)).is_err());
        assert!(k_means(&pts, &KMeansOptions::new(16)).is_err());
        assert!(k_means(&[vec![1.0], vec![1.0, 2.0]], &KMeansOptions::new(1)).is_err());
    }

    /// Duplicate points tie between equal centres. Points keep their cluster
    /// on a tie, and an emptied cluster takes a point from a cluster that
    /// can spare one, so Lloyd's iterations settle at once instead of
    /// flipping a duplicate back and forth (and leaving a cluster empty).
    #[test]
    fn duplicate_points_converge_without_cycling() {
        let pts = vec![vec![0.0], vec![0.0], vec![5.0]];
        // Start with two identical centres, as a seeding over duplicates can.
        let (assign, _, iterations) = lloyd(&pts, vec![vec![0.0], vec![0.0], vec![5.0]], 300);
        assert!(iterations <= 2, "{iterations} iterations");
        let mut sizes = [0; 3];
        assign.iter().for_each(|&a| sizes[a] += 1);
        assert_eq!(sizes, [1, 1, 1]);
        let r = k_means(&pts, &KMeansOptions::new(3)).unwrap();
        assert!(r.sizes.iter().all(|&s| s == 1), "{:?}", r.sizes);
        assert_eq!(r.total_within_ss(), 0.0);
        // Many duplicates: the partition does not depend on the row order.
        let base: Vec<Vec<f64>> = (0..24).map(|i| vec![f64::from(i % 3) * 4.0, f64::from(i % 2)]).collect();
        let mut reversed = base.clone();
        reversed.reverse();
        let a = k_means(&base, &KMeansOptions::new(6)).unwrap();
        let b = k_means(&reversed, &KMeansOptions::new(6)).unwrap();
        assert_eq!(a.total_within_ss(), 0.0);
        assert_eq!(b.total_within_ss(), 0.0);
        assert!(a.sizes.iter().chain(&b.sizes).all(|&s| s == 4));
    }
}
