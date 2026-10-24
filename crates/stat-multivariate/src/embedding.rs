//! Shared pieces of the dimension-reduction methods ([`crate::mds`],
//! [`crate::tsne`], [`crate::umap`]): the dissimilarities between objects
//! (Euclidean distances between data rows, or a given distance matrix),
//! exact nearest neighbours, and the trustworthiness of an embedding.

use rayon::prelude::*;
use stat_core::{Result, StatError};

/// The dissimilarities an embedding preserves.
#[derive(Debug, Clone, Copy)]
pub enum Proximity<'a> {
    /// Euclidean distances between these points (rows of equal length).
    Points(&'a [Vec<f64>]),
    /// A given symmetric distance matrix (`n × n`, zero diagonal).
    Distances(&'a [Vec<f64>]),
}

impl Proximity<'_> {
    /// Number of objects.
    #[must_use]
    pub fn n(&self) -> usize {
        match self {
            Self::Points(p) | Self::Distances(p) => p.len(),
        }
    }

    /// Dissimilarity of objects `i` and `j`.
    #[must_use]
    pub fn distance(&self, i: usize, j: usize) -> f64 {
        match self {
            Self::Points(p) => p[i].iter().zip(&p[j]).map(|(a, b)| (a - b) * (a - b)).sum::<f64>().sqrt(),
            Self::Distances(d) => d[i][j],
        }
    }

    /// Check the input: at least `min_n` objects; points of one finite
    /// dimension, or a square, symmetric (to 1e-8 relative), non-negative,
    /// finite matrix with a zero diagonal.
    ///
    /// # Errors
    /// [`StatError::InsufficientData`], [`StatError::NonFiniteInput`] or
    /// [`StatError::InvalidArgument`] naming the problem.
    pub fn validate(&self, min_n: usize) -> Result<()> {
        let n = self.n();
        if n < min_n {
            return Err(StatError::InsufficientData { required: min_n, found: n });
        }
        match self {
            Self::Points(p) => {
                let d = p[0].len();
                if d == 0 || p.iter().any(|r| r.len() != d) {
                    return Err(invalid("every row needs the same, non-zero number of values"));
                }
                if p.iter().flatten().any(|v| !v.is_finite()) {
                    return Err(StatError::NonFiniteInput { context: "embedding data" });
                }
            }
            Self::Distances(d) => {
                if d.iter().any(|r| r.len() != n) {
                    return Err(invalid("the distance matrix must be square: as many columns as rows"));
                }
                if d.iter().flatten().any(|v| !v.is_finite()) {
                    return Err(StatError::NonFiniteInput { context: "distance matrix" });
                }
                let scale = d.iter().flatten().fold(0.0_f64, |m, v| m.max(v.abs()));
                for i in 0..n {
                    if d[i][i].abs() > 1e-12 * scale.max(1.0) {
                        return Err(invalid("the distance matrix needs zeros on its diagonal"));
                    }
                    for j in 0..i {
                        if d[i][j] < 0.0 {
                            return Err(invalid("distances cannot be negative"));
                        }
                        if (d[i][j] - d[j][i]).abs() > 1e-8 * scale.max(f64::MIN_POSITIVE) {
                            return Err(invalid(format!(
                                "the distance matrix is not symmetric (rows {} and {} differ)",
                                i + 1,
                                j + 1
                            )));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// The full distance matrix (`n²` values).
    #[must_use]
    pub fn matrix(&self) -> Vec<Vec<f64>> {
        let n = self.n();
        (0..n)
            .into_par_iter()
            .map(|i| (0..n).map(|j| if i == j { 0.0 } else { self.distance(i, j) }).collect())
            .collect()
    }

    /// The `k` nearest other objects of each object, nearest first, as
    /// `(index, distance)`; ties broken by index. Exact (brute force,
    /// `O(n²)` distances, in parallel).
    #[must_use]
    pub fn nearest_neighbours(&self, k: usize) -> Vec<Vec<(usize, f64)>> {
        let n = self.n();
        let k = k.min(n.saturating_sub(1));
        (0..n)
            .into_par_iter()
            .map(|i| {
                let mut all: Vec<(usize, f64)> = (0..n).filter(|&j| j != i).map(|j| (j, self.distance(i, j))).collect();
                let by = |a: &(usize, f64), b: &(usize, f64)| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0));
                if k < all.len() {
                    all.select_nth_unstable_by(k, by);
                    all.truncate(k);
                }
                all.sort_by(by);
                all
            })
            .collect()
    }
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Trustworthiness of an embedding (Venna & Kaski 2001, "Neighborhood
/// preservation in nonlinear projection methods: an experimental study",
/// *ICANN*): with `r(i, j)` the rank of `j` among the neighbours of `i` in
/// the original space (1 = nearest) and `Uₖ(i)` the objects among the `k`
/// nearest of `i` in the embedding but not in the original space,
///
/// `T(k) = 1 − 2 / (n k (2n − 3k − 1)) · Σᵢ Σ_{j ∈ Uₖ(i)} (r(i, j) − k)`.
///
/// `T = 1` when every embedded neighbourhood is a true one; values near
/// 0.5 are typical of a random layout.
///
/// # Errors
/// [`StatError::InvalidArgument`] when `embedding` does not have one row per
/// object or `k` is not below `n / 2`.
pub fn trustworthiness(original: &Proximity<'_>, embedding: &[Vec<f64>], k: usize) -> Result<f64> {
    let n = original.n();
    if embedding.len() != n {
        return Err(invalid("the embedding needs one row per object"));
    }
    if k == 0 || 2 * k >= n {
        return Err(invalid(format!("the neighbourhood size must be at least 1 and below n / 2 = {}", n / 2)));
    }
    let embedded = Proximity::Points(embedding).nearest_neighbours(k);
    let penalty: f64 = (0..n)
        .into_par_iter()
        .map(|i| {
            // Rank of every object in the original space.
            let mut order: Vec<(usize, f64)> =
                (0..n).filter(|&j| j != i).map(|j| (j, original.distance(i, j))).collect();
            order.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
            let mut rank = vec![0usize; n];
            for (r, &(j, _)) in order.iter().enumerate() {
                rank[j] = r + 1;
            }
            embedded[i].iter().map(|&(j, _)| rank[j].saturating_sub(k) as f64).sum::<f64>()
        })
        .sum();
    let (nf, kf) = (n as f64, k as f64);
    Ok(1.0 - 2.0 / (nf * kf * (2.0 * nf - 3.0 * kf - 1.0)) * penalty)
}

/// Standardize columns of row vectors to mean 0 and SD 1 (divisor `n − 1`).
///
/// # Errors
/// [`StatError::InvalidArgument`] naming a constant column (by index).
pub fn standardize_columns(points: &[Vec<f64>]) -> Result<Vec<Vec<f64>>> {
    let n = points.len();
    let p = points.first().map_or(0, Vec::len);
    let mut out = points.to_vec();
    for j in 0..p {
        let m = points.iter().map(|r| r[j]).sum::<f64>() / n as f64;
        let sd = (points.iter().map(|r| (r[j] - m) * (r[j] - m)).sum::<f64>() / (n as f64 - 1.0)).sqrt();
        if !(sd > 0.0 && sd.is_finite()) {
            return Err(invalid(format!("column {} is constant, so it cannot be standardized", j + 1)));
        }
        for r in &mut out {
            r[j] = (r[j] - m) / sd;
        }
    }
    Ok(out)
}

/// Leading principal component scores of `points` (centred), `dims`
/// columns; columns beyond the data's rank are zero. Used to start
/// embeddings and for classical scaling of Euclidean distances.
pub(crate) fn pca_scores(points: &[Vec<f64>], dims: usize) -> Result<(Vec<Vec<f64>>, Vec<f64>)> {
    let n = points.len();
    let p = points[0].len();
    let mean: Vec<f64> = (0..p).map(|j| points.iter().map(|r| r[j]).sum::<f64>() / n as f64).collect();
    let centred: Vec<Vec<f64>> = points.iter().map(|r| r.iter().zip(&mean).map(|(v, m)| v - m).collect()).collect();
    let x = stat_core::Matrix::from_rows(&centred)?;
    let s = stat_core::svd(&x)?;
    let r = s.singular_values.len();
    let scores = (0..n)
        .map(|i| (0..dims).map(|k| if k < r { s.u[(i, k)] * s.singular_values[k] } else { 0.0 }).collect())
        .collect();
    let squares = s.singular_values.iter().map(|v| v * v).collect();
    Ok((scores, squares))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbours_and_validation() {
        let pts = vec![vec![0.0], vec![1.0], vec![3.0], vec![7.0]];
        let p = Proximity::Points(&pts);
        let nn = p.nearest_neighbours(2);
        assert_eq!(nn[0], [(1, 1.0), (2, 3.0)]);
        assert_eq!(nn[3], [(2, 4.0), (1, 6.0)]);
        let m = p.matrix();
        assert!((m[1][3] - 6.0).abs() < 1e-15);
        assert!(Proximity::Distances(&m).validate(2).is_ok());
        let mut bad = m.clone();
        bad[0][1] = 2.0;
        assert!(Proximity::Distances(&bad).validate(2).is_err());
        bad = m;
        bad[2][2] = 1.0;
        assert!(Proximity::Distances(&bad).validate(2).is_err());
        assert!(Proximity::Points(&[vec![1.0], vec![f64::NAN]]).validate(2).is_err());
        assert!(Proximity::Points(&pts).validate(5).is_err());
    }

    #[test]
    fn trustworthiness_of_a_perfect_and_a_reversed_layout() {
        let pts: Vec<Vec<f64>> = (0..20).map(|i| vec![f64::from(i) * f64::from(i) * 0.1]).collect();
        let p = Proximity::Points(&pts);
        assert!((trustworthiness(&p, &pts, 3).unwrap() - 1.0).abs() < 1e-15);
        assert!(trustworthiness(&p, &pts, 10).is_err());
    }
}
