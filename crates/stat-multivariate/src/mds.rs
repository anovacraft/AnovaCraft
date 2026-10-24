//! Multidimensional scaling: classical (Torgerson) scaling and metric
//! scaling by SMACOF.
//!
//! # Classical scaling
//! Torgerson (1952, "Multidimensional scaling: I. Theory and method",
//! *Psychometrika* 17); Gower (1966). From dissimilarities `δᵢⱼ` form
//! `B = −½ J Δ⁽²⁾ J`, with `Δ⁽²⁾` the squared dissimilarities and
//! `J = I − 11ᵀ/n` the centring matrix. With eigenvalues `λ₁ ≥ λ₂ ≥ …` and
//! unit eigenvectors `vₖ` of `B`, the coordinates on dimension `k` are
//! `√λₖ vₖ`. For Euclidean distances between data rows `B` is the centred
//! cross-product matrix, so the solution equals the principal component
//! scores and is computed from the SVD of the centred data instead. The
//! share of `Σ λₖ⁺` carried by the kept dimensions measures the fit;
//! negative eigenvalues mean the dissimilarities are not Euclidean.
//!
//! # Metric scaling by SMACOF
//! De Leeuw (1977, "Applications of convex analysis to multidimensional
//! scaling", in *Recent Developments in Statistics*); Borg & Groenen,
//! *Modern Multidimensional Scaling* (2005), ch. 8. Minimizes raw stress
//! `σ(X) = Σ_{i<j} (δᵢⱼ − dᵢⱼ(X))²` over configurations `X` (`n × d`) by
//! the Guttman transform `X⁺ = n⁻¹ B(X) X`, where `bᵢⱼ = −δᵢⱼ / dᵢⱼ(X)`
//! for `i ≠ j` (0 when `dᵢⱼ = 0`) and `bᵢᵢ = −Σ_{j≠i} bᵢⱼ`. Each step
//! cannot raise the stress (majorization); iterations stop when the stress
//! falls by less than `tolerance` relative to its value. The report's
//! stress-1 is `√(σ / Σ_{i<j} δᵢⱼ²)` (Kruskal's normalization by the
//! dissimilarities). The first start is the classical solution; further
//! starts are seeded random configurations, and the lowest stress is kept.

use stat_core::rng::Rng;
use stat_core::{Result, StatError};

use crate::embedding::{Proximity, pca_scores};
use crate::sym_eigen::symmetric_eigen;

/// A classical (Torgerson) scaling solution.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassicalScaling {
    /// Coordinates (`n` rows of `d`).
    pub coordinates: Vec<Vec<f64>>,
    /// Eigenvalues of `B`, largest first (for data rows: the squared
    /// singular values of the centred data, followed by zeros).
    pub eigenvalues: Vec<f64>,
}

impl ClassicalScaling {
    /// Share of the positive eigenvalue total carried by the first `d`
    /// dimensions, `Σₖ≤d λₖ⁺ / Σ λₖ⁺`.
    #[must_use]
    pub fn proportion(&self) -> f64 {
        let d = self.coordinates.first().map_or(0, Vec::len);
        let pos: f64 = self.eigenvalues.iter().filter(|v| **v > 0.0).sum();
        let kept: f64 = self.eigenvalues.iter().take(d).map(|v| v.max(0.0)).sum();
        if pos > 0.0 { kept / pos } else { 0.0 }
    }
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Classical scaling of `proximity` into `dims` dimensions. Dimensions
/// with a non-positive eigenvalue get zero coordinates.
///
/// # Errors
/// Fewer than 3 objects or `dims` not below `n`; invalid input (see
/// [`Proximity::validate`]); failure of the eigen-decomposition or SVD.
pub fn classical_scaling(proximity: &Proximity<'_>, dims: usize) -> Result<ClassicalScaling> {
    proximity.validate(3)?;
    let n = proximity.n();
    if dims == 0 || dims >= n {
        return Err(invalid(format!("the number of dimensions must be between 1 and {}", n - 1)));
    }
    if let Proximity::Points(points) = proximity {
        let (coordinates, mut eigenvalues) = pca_scores(points, dims)?;
        eigenvalues.resize(n, 0.0);
        return Ok(ClassicalScaling { coordinates: orient(coordinates), eigenvalues });
    }
    let d2: Vec<Vec<f64>> = (0..n).map(|i| (0..n).map(|j| proximity.distance(i, j).powi(2)).collect()).collect();
    let row: Vec<f64> = d2.iter().map(|r| r.iter().sum::<f64>() / n as f64).collect();
    let grand = row.iter().sum::<f64>() / n as f64;
    let b: Vec<Vec<f64>> =
        (0..n).map(|i| (0..n).map(|j| -0.5 * (d2[i][j] - row[i] - row[j] + grand)).collect()).collect();
    let (eigenvalues, vectors) = symmetric_eigen(&b)?;
    let coordinates = (0..n)
        .map(|i| {
            (0..dims).map(|k| if eigenvalues[k] > 0.0 { vectors[i][k] * eigenvalues[k].sqrt() } else { 0.0 }).collect()
        })
        .collect();
    Ok(ClassicalScaling { coordinates: orient(coordinates), eigenvalues })
}

/// Flip each dimension so its largest absolute coordinate is positive
/// (eigenvector signs are arbitrary).
fn orient(mut x: Vec<Vec<f64>>) -> Vec<Vec<f64>> {
    let d = x.first().map_or(0, Vec::len);
    for k in 0..d {
        let big = x.iter().map(|r| r[k]).fold(0.0_f64, |m, v| if v.abs() > m.abs() { v } else { m });
        if big < 0.0 {
            x.iter_mut().for_each(|r| r[k] = -r[k]);
        }
    }
    x
}

/// Settings of metric SMACOF scaling.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SmacofOptions {
    /// Dimensions of the configuration.
    pub dims: usize,
    /// Most Guttman iterations per start.
    pub max_iterations: usize,
    /// Relative decrease of raw stress that ends the iterations.
    pub tolerance: f64,
    /// Seeded random starts in addition to the classical start.
    pub random_starts: usize,
    /// Seed of the random starts.
    pub seed: u64,
}

impl SmacofOptions {
    /// `dims` dimensions, 300 iterations, tolerance 1e-6, 4 random starts, seed 1.
    #[must_use]
    pub fn new(dims: usize) -> Self {
        Self { dims, max_iterations: 300, tolerance: 1e-6, random_starts: 4, seed: 1 }
    }
}

/// A metric scaling solution.
#[derive(Debug, Clone, PartialEq)]
pub struct Smacof {
    /// Coordinates (`n` rows of `d`).
    pub coordinates: Vec<Vec<f64>>,
    /// Raw stress `Σ_{i<j} (δᵢⱼ − dᵢⱼ)²`.
    pub raw_stress: f64,
    /// Stress-1, `√(raw stress / Σ_{i<j} δᵢⱼ²)`.
    pub stress1: f64,
    /// Iterations of the kept start.
    pub iterations: usize,
    /// Whether the kept start met the tolerance.
    pub converged: bool,
    /// Which start was kept (0 = classical).
    pub start: usize,
}

/// Raw stress of configuration `x` against dissimilarities `delta`.
#[must_use]
pub fn raw_stress(delta: &[Vec<f64>], x: &[Vec<f64>]) -> f64 {
    let n = delta.len();
    let mut s = 0.0;
    for i in 0..n {
        for j in 0..i {
            let d = dist(&x[i], &x[j]);
            s += (delta[i][j] - d).powi(2);
        }
    }
    s
}

fn dist(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(u, v)| (u - v) * (u - v)).sum::<f64>().sqrt()
}

/// SMACOF iterations from configuration `init` (`n` rows of `d`).
///
/// # Errors
/// `init` of the wrong shape or with non-finite values.
pub fn smacof_from(delta: &[Vec<f64>], init: Vec<Vec<f64>>, max_iterations: usize, tolerance: f64) -> Result<Smacof> {
    let n = delta.len();
    if init.len() != n || init.iter().any(|r| r.len() != init[0].len()) {
        return Err(invalid("the starting configuration needs one row per object"));
    }
    if init.iter().flatten().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "starting configuration" });
    }
    let d = init[0].len();
    let mut x = init;
    let mut stress = raw_stress(delta, &x);
    let mut iterations = 0;
    let mut converged = false;
    let total: f64 = (0..n).map(|i| (0..i).map(|j| delta[i][j] * delta[i][j]).sum::<f64>()).sum();
    while iterations < max_iterations {
        iterations += 1;
        let mut next = vec![vec![0.0; d]; n];
        for i in 0..n {
            let mut bii = 0.0;
            for j in 0..n {
                if j == i {
                    continue;
                }
                let dij = dist(&x[i], &x[j]);
                let bij = if dij > 0.0 { -delta[i][j] / dij } else { 0.0 };
                bii -= bij;
                for k in 0..d {
                    next[i][k] += bij * x[j][k];
                }
            }
            for k in 0..d {
                next[i][k] = (next[i][k] + bii * x[i][k]) / n as f64;
            }
        }
        let s = raw_stress(delta, &next);
        x = next;
        let drop = stress - s;
        stress = s;
        if drop <= tolerance * stress.max(f64::MIN_POSITIVE) || stress <= 1e-30 * total {
            converged = true;
            break;
        }
    }
    let stress1 = if total > 0.0 { (stress / total).sqrt() } else { 0.0 };
    Ok(Smacof { coordinates: x, raw_stress: stress, stress1, iterations, converged, start: 0 })
}

/// Metric scaling by SMACOF from the classical solution and from
/// `options.random_starts` seeded random configurations, keeping the lowest
/// stress.
///
/// # Errors
/// As [`classical_scaling`]; no iterations.
pub fn metric_scaling(proximity: &Proximity<'_>, options: &SmacofOptions) -> Result<Smacof> {
    if options.max_iterations == 0 {
        return Err(invalid("use at least one iteration"));
    }
    let classical = classical_scaling(proximity, options.dims)?;
    let delta = proximity.matrix();
    let n = delta.len();
    let mut best = smacof_from(&delta, classical.coordinates, options.max_iterations, options.tolerance)?;
    // Random starts on the scale of the dissimilarities.
    let mean: f64 = delta.iter().flatten().sum::<f64>() / (n * n) as f64;
    let mut rng = Rng::new(options.seed);
    for s in 1..=options.random_starts {
        let init: Vec<Vec<f64>> = (0..n).map(|_| (0..options.dims).map(|_| mean * rng.normal()).collect()).collect();
        let mut fit = smacof_from(&delta, init, options.max_iterations, options.tolerance)?;
        fit.start = s;
        if fit.raw_stress < best.raw_stress * (1.0 - 1e-9) {
            best = fit;
        }
    }
    best.coordinates = orient(best.coordinates);
    Ok(best)
}

#[cfg(test)]
mod tests;
