//! Latent semantic analysis: a truncated singular value decomposition of a
//! weighted document–term matrix.
//!
//! # Method
//! For the `D × T` weighted matrix `A` (usually TF-IDF with unit-length
//! rows), latent semantic analysis keeps the `k` largest singular triplets
//! of `A = U Σ Vᵀ` (Deerwester, Dumais, Furnas, Landauer & Harshman 1990,
//! "Indexing by latent semantic analysis", *JASIS* 41(6)). Document `d` is
//! placed at `(U Σ)_{d·}` and term `t` has loadings `V_{t·}`; documents with
//! similar mixes of terms lie close together even when they share few exact
//! words. The share of `‖A‖²_F` carried by dimension `i` is `σᵢ² / ‖A‖²_F`.
//!
//! When the smaller side of `A` has at most [`EXACT_LIMIT`] rows or columns
//! the full decomposition is computed (one-sided Jacobi,
//! [`stat_core::svd::svd`]). Larger matrices use the randomized subspace
//! iteration of Halko, Martinsson & Tropp (2011), "Finding structure with
//! randomness", *SIAM Review* 53(2), Algorithm 4.4: a Gaussian test matrix
//! with `max(2k, k + 10)` columns, power iterations with
//! re-orthonormalisation, and an exact decomposition of the small projected
//! matrix after each iteration. The iterations continue until the `k`
//! leading singular values change by less than 10⁻¹³ relative (at most 200),
//! so they agree with the exact ones to working precision even when the
//! spectrum decays slowly, as it does for text.
//!
//! Each dimension's sign is fixed so that its largest term loading (in
//! absolute value) is positive, so results do not depend on the method.

use stat_core::matrix::Matrix;
use stat_core::rng::Rng;
use stat_core::svd::svd;

use crate::document_term::SparseRow;
use crate::{Result, StatError};

/// Largest smaller side of the matrix decomposed exactly.
pub const EXACT_LIMIT: usize = 400;

const OVERSAMPLE: usize = 10;
const MAX_POWER_ITERATIONS: usize = 200;
/// Relative change of the leading singular values that ends the iteration.
const CONVERGENCE: f64 = 1e-13;

/// A fitted latent semantic space.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticSpace {
    /// The `k` largest singular values, decreasing.
    pub singular_values: Vec<f64>,
    /// Document coordinates `U Σ` (`D` rows of `k`).
    pub document_scores: Vec<Vec<f64>>,
    /// Term loadings `V` (`T` rows of `k`).
    pub term_loadings: Vec<Vec<f64>>,
    /// `‖A‖²_F`, the total the dimensions' shares are taken of.
    pub total_sum_squares: f64,
    /// Whether the randomized method was used.
    pub randomized: bool,
}

impl SemanticSpace {
    /// Share of `‖A‖²_F` carried by each dimension.
    #[must_use]
    pub fn shares(&self) -> Vec<f64> {
        self.singular_values.iter().map(|s| s * s / self.total_sum_squares).collect()
    }
}

/// Fit `dimensions` dimensions to the sparse rows of a `D × n_terms` matrix.
///
/// # Errors
/// [`StatError::InvalidArgument`] for zero dimensions, more dimensions than
/// `min(D, T)`, a term index out of range, non-finite values, or an all-zero
/// matrix.
pub fn latent_semantic_analysis(
    rows: &[SparseRow],
    n_terms: usize,
    dimensions: usize,
    seed: u64,
) -> Result<SemanticSpace> {
    fit(rows, n_terms, dimensions, seed, EXACT_LIMIT)
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

pub(crate) fn fit(
    rows: &[SparseRow],
    n_terms: usize,
    dimensions: usize,
    seed: u64,
    exact_limit: usize,
) -> Result<SemanticSpace> {
    let n_docs = rows.len();
    if dimensions == 0 {
        return Err(invalid("choose at least one dimension"));
    }
    if dimensions > n_docs.min(n_terms) {
        return Err(invalid(format!(
            "at most {} dimensions are possible with {n_docs} documents and {n_terms} terms",
            n_docs.min(n_terms)
        )));
    }
    let mut total = 0.0;
    for row in rows {
        for &(j, v) in row {
            if j >= n_terms {
                return Err(invalid(format!("term index {j} is out of range")));
            }
            if !v.is_finite() {
                return Err(StatError::NonFiniteInput { context: "document-term matrix" });
            }
            total += v * v;
        }
    }
    if total <= 0.0 {
        return Err(invalid("the document-term matrix is all zero"));
    }
    let (u, s, v, randomized) = if n_docs.min(n_terms) <= exact_limit {
        let mut dense = Matrix::zeros(n_docs, n_terms);
        for (d, row) in rows.iter().enumerate() {
            for &(j, x) in row {
                dense.col_mut(j)[d] = x;
            }
        }
        let f = svd(&dense)?;
        let take = |m: &Matrix| -> Vec<Vec<f64>> { (0..m.nrows()).map(|i| m.row(i)[..dimensions].to_vec()).collect() };
        (take(&f.u), f.singular_values[..dimensions].to_vec(), take(&f.v), false)
    } else {
        let (u, s, v) = randomized_svd(rows, n_terms, dimensions, seed)?;
        (u, s, v, true)
    };
    let mut document_scores = u;
    let mut term_loadings = v;
    for i in 0..dimensions {
        let lead = term_loadings.iter().map(|r| r[i]).fold(0.0_f64, |a, x| if x.abs() > a.abs() { x } else { a });
        let sign = if lead < 0.0 { -1.0 } else { 1.0 };
        term_loadings.iter_mut().for_each(|r| r[i] *= sign);
        document_scores.iter_mut().for_each(|r| r[i] *= sign * s[i]);
    }
    Ok(SemanticSpace { singular_values: s, document_scores, term_loadings, total_sum_squares: total, randomized })
}

/// `A X` for sparse `A` (`D × T`) and dense `X` (`T` rows of `l`).
fn times(rows: &[SparseRow], x: &[Vec<f64>], l: usize) -> Vec<Vec<f64>> {
    rows.iter()
        .map(|row| {
            let mut out = vec![0.0; l];
            for &(j, a) in row {
                for (o, &b) in out.iter_mut().zip(&x[j]) {
                    *o += a * b;
                }
            }
            out
        })
        .collect()
}

/// `Aᵀ Y` for sparse `A` (`D × T`) and dense `Y` (`D` rows of `l`).
fn transpose_times(rows: &[SparseRow], y: &[Vec<f64>], n_terms: usize, l: usize) -> Vec<Vec<f64>> {
    let mut out = vec![vec![0.0; l]; n_terms];
    for (row, yd) in rows.iter().zip(y) {
        for &(j, a) in row {
            for (o, &b) in out[j].iter_mut().zip(yd) {
                *o += a * b;
            }
        }
    }
    out
}

/// Orthonormalise the columns of `m` (rows of `l`) by modified Gram–Schmidt,
/// applied twice for stability; a column that vanishes is replaced by zeros.
fn orthonormalize(m: &mut [Vec<f64>], l: usize) {
    for _ in 0..2 {
        for c in 0..l {
            for p in 0..c {
                let dot: f64 = m.iter().map(|r| r[c] * r[p]).sum();
                m.iter_mut().for_each(|r| r[c] -= dot * r[p]);
            }
            let norm = m.iter().map(|r| r[c] * r[c]).sum::<f64>().sqrt();
            let scale = if norm > 1e-300 { 1.0 / norm } else { 0.0 };
            m.iter_mut().for_each(|r| r[c] *= scale);
        }
    }
}

type Triplets = (Vec<Vec<f64>>, Vec<f64>, Vec<Vec<f64>>);

fn project(rows: &[SparseRow], q: &[Vec<f64>], n_terms: usize, l: usize) -> Result<stat_core::svd::Svd> {
    // B = Qᵀ A is l × T; its transpose Aᵀ Q is T × l.
    let bt = transpose_times(rows, q, n_terms, l);
    let mut b = Matrix::zeros(l, n_terms);
    for (j, r) in bt.iter().enumerate() {
        for (c, &x) in r.iter().enumerate() {
            b.col_mut(j)[c] = x;
        }
    }
    svd(&b)
}

fn randomized_svd(rows: &[SparseRow], n_terms: usize, k: usize, seed: u64) -> Result<Triplets> {
    let l = (2 * k).max(k + OVERSAMPLE).min(rows.len()).min(n_terms);
    let mut rng = Rng::new(seed);
    let omega: Vec<Vec<f64>> = (0..n_terms).map(|_| (0..l).map(|_| rng.normal()).collect()).collect();
    let mut q = times(rows, &omega, l);
    orthonormalize(&mut q, l);
    let mut previous: Option<Vec<f64>> = None;
    let mut f = project(rows, &q, n_terms, l)?;
    for _ in 0..MAX_POWER_ITERATIONS {
        let mut z = transpose_times(rows, &q, n_terms, l);
        orthonormalize(&mut z, l);
        q = times(rows, &z, l);
        orthonormalize(&mut q, l);
        f = project(rows, &q, n_terms, l)?;
        let current = f.singular_values[..k].to_vec();
        let settled = previous.as_ref().is_some_and(|p| {
            p.iter().zip(&current).all(|(a, b)| (a - b).abs() <= CONVERGENCE * b.abs().max(f64::MIN_POSITIVE))
        });
        if settled {
            break;
        }
        previous = Some(current);
    }
    // U = Q Ũ.
    let u: Vec<Vec<f64>> =
        q.iter().map(|qd| (0..k).map(|i| (0..l).map(|c| qd[c] * f.u.col(i)[c]).sum()).collect()).collect();
    let v: Vec<Vec<f64>> = (0..n_terms).map(|j| (0..k).map(|i| f.v.col(i)[j]).collect()).collect();
    Ok((u, f.singular_values[..k].to_vec(), v))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn random_sparse(d: usize, t: usize, density: f64, seed: u64) -> Vec<SparseRow> {
        let mut rng = Rng::new(seed);
        (0..d)
            .map(|_| {
                let mut row = Vec::new();
                for j in 0..t {
                    if rng.next_f64() < density {
                        row.push((j, 1.0 + 4.0 * rng.next_f64()));
                    }
                }
                row
            })
            .collect()
    }

    #[test]
    fn randomized_matches_exact_leading_triplets() {
        let rows = random_sparse(120, 80, 0.15, 3);
        let exact = fit(&rows, 80, 5, 1, EXACT_LIMIT).unwrap();
        let fast = fit(&rows, 80, 5, 1, 10).unwrap();
        assert!(!exact.randomized && fast.randomized);
        for i in 0..5 {
            let rel = (exact.singular_values[i] - fast.singular_values[i]).abs() / exact.singular_values[i];
            assert!(rel < 1e-10, "σ{i}: {rel}");
        }
        // The first dimensions are well separated, so their vectors agree.
        for i in 0..2 {
            for t in 0..80 {
                assert!((exact.term_loadings[t][i] - fast.term_loadings[t][i]).abs() < 1e-5);
            }
            for d in 0..120 {
                assert!((exact.document_scores[d][i] - fast.document_scores[d][i]).abs() < 1e-4);
            }
        }
    }

    #[test]
    fn reconstruction_and_orthogonality() {
        let rows = random_sparse(30, 12, 0.4, 9);
        let s = latent_semantic_analysis(&rows, 12, 12, 1).unwrap();
        let shares: f64 = s.shares().iter().sum();
        assert!((shares - 1.0).abs() < 1e-12, "all dimensions carry the whole matrix");
        // A = (UΣ) Vᵀ.
        for (d, row) in rows.iter().enumerate() {
            for t in 0..12 {
                let a = row.iter().find(|e| e.0 == t).map_or(0.0, |e| e.1);
                let r: f64 = (0..12).map(|i| s.document_scores[d][i] * s.term_loadings[t][i]).sum();
                assert!((a - r).abs() < 1e-10);
            }
        }
    }

    #[test]
    fn errors() {
        let rows = random_sparse(5, 4, 0.5, 1);
        assert!(latent_semantic_analysis(&rows, 4, 0, 1).is_err());
        assert!(latent_semantic_analysis(&rows, 4, 5, 1).is_err());
        assert!(latent_semantic_analysis(&[vec![], vec![]], 3, 1, 1).is_err());
        assert!(latent_semantic_analysis(&[vec![(7, 1.0)]], 3, 1, 1).is_err());
    }
}
