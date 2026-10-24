//! Uniform manifold approximation and projection (UMAP; McInnes, Healy &
//! Melville 2018, "UMAP: Uniform Manifold Approximation and Projection for
//! Dimension Reduction", arXiv:1802.03426).
//!
//! # Fuzzy neighbour graph
//! Each object's `k` nearest neighbours (counting itself, as the reference
//! implementation does; exact search) define a local fuzzy set: with `ρᵢ`
//! the distance to the nearest other object and `σᵢ` chosen by bisection so
//! that `Σ_{j=1}^{k−1} exp(−max(0, dᵢⱼ − ρᵢ)/σᵢ) = log₂ k`, the membership
//! of the edge `i → j` is `exp(−max(0, dᵢⱼ − ρᵢ)/σᵢ)` (`σᵢ` is kept above
//! 10⁻³ times the mean neighbour distance). The local sets are combined by
//! the fuzzy union `W = A + Aᵀ − A ∘ Aᵀ`.
//!
//! # Layout
//! The low-dimensional similarity is `(1 + a‖yᵢ − yⱼ‖^{2b})⁻¹`, with `a`
//! and `b` fitted by least squares to the curve that is 1 up to `min_dist`
//! and `exp(−(x − min_dist)/spread)` beyond it (300 points on
//! `[0, 3·spread]`). The start is the spectral layout: eigenvectors of the
//! largest non-trivial eigenvalues of `D^{-1/2} W D^{-1/2}` (the smallest of
//! the normalized graph Laplacian), scaled to a maximum absolute coordinate
//! of 10 with N(0, 10⁻⁴²) noise, then rescaled to `[0, 10]` per axis; or
//! seeded uniform noise on `[−10, 10]`. The cross-entropy between the two
//! fuzzy sets is minimized by stochastic gradient descent: edge `(i, j)` is
//! sampled every `max W / wᵢⱼ` epochs (edges below `max W / epochs` are
//! dropped), pulling the pair together with gradient coefficient
//! `−2ab d^{2(b−1)} / (1 + a d^{2b})`; each sample also pushes `i` away
//! from `negative_sample_rate` objects drawn at random, with coefficient
//! `2b / ((0.001 + d²)(1 + a d^{2b}))`. Gradient components are clipped to
//! ±4 and the learning rate falls linearly from 1 to 0. Reproducible from
//! the seed (single-threaded SGD).

use stat_core::rng::Rng;
use stat_core::{Result, StatError};

use crate::embedding::Proximity;
use crate::sym_eigen::symmetric_eigen;

/// Settings of a UMAP embedding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UmapOptions {
    /// Embedding dimensions (1–3).
    pub dims: usize,
    /// Neighbourhood size `k` (counting the object itself), 2 to `n`.
    pub neighbours: usize,
    /// Smallest distance between embedded points (`≥ 0`, below `spread`).
    pub min_dist: f64,
    /// Scale of the embedded clusters.
    pub spread: f64,
    /// SGD epochs (`None`: 500 up to 10 000 objects, else 200).
    pub epochs: Option<usize>,
    /// Negative samples per positive sample.
    pub negative_sample_rate: usize,
    /// Start from random noise instead of the spectral layout.
    pub random_start: bool,
    /// Seed of the noise and of the negative sampling.
    pub seed: u64,
}

impl UmapOptions {
    /// Two dimensions, 15 neighbours, `min_dist` 0.1, spread 1, automatic
    /// epochs, 5 negative samples, spectral start, seed 1.
    #[must_use]
    pub fn new() -> Self {
        Self {
            dims: 2,
            neighbours: 15,
            min_dist: 0.1,
            spread: 1.0,
            epochs: None,
            negative_sample_rate: 5,
            random_start: false,
            seed: 1,
        }
    }
}

impl Default for UmapOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// A UMAP embedding.
#[derive(Debug, Clone, PartialEq)]
pub struct Umap {
    /// Coordinates (`n` rows of `dims`).
    pub coordinates: Vec<Vec<f64>>,
    /// Curve parameters `a`, `b`.
    pub a: f64,
    /// See `a`.
    pub b: f64,
    /// Edges of the fuzzy graph used (each pair counted once).
    pub edges: usize,
    /// Epochs run.
    pub epochs: usize,
    /// Whether the spectral start was used (`false`: random start, chosen
    /// or because the eigenvectors could not be found).
    pub spectral: bool,
}

/// The fuzzy neighbour graph: symmetric sparse rows `(j, wᵢⱼ)` and each
/// object's `ρᵢ` and `σᵢ`.
#[derive(Debug, Clone, PartialEq)]
pub struct FuzzyGraph {
    /// Symmetric weights, rows sorted by column.
    pub rows: Vec<Vec<(usize, f64)>>,
    /// Distance to the nearest other object.
    pub rho: Vec<f64>,
    /// Bandwidth of each local fuzzy set.
    pub sigma: Vec<f64>,
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// `ρ` and `σ` of one object from its distances to its `k − 1` nearest
/// other objects (nearest first), so that the memberships sum to `log₂ k`.
#[must_use]
pub fn smooth_distances(distances: &[f64], k: usize, mean_all: f64) -> (f64, f64) {
    let target = (k as f64).log2();
    let rho = distances.iter().copied().find(|&d| d > 0.0).unwrap_or(0.0);
    let total = |sigma: f64| -> f64 {
        distances.iter().map(|&d| if d - rho > 0.0 { (-(d - rho) / sigma).exp() } else { 1.0 }).sum()
    };
    let (mut lo, mut hi, mut mid) = (0.0_f64, f64::INFINITY, 1.0_f64);
    for _ in 0..200 {
        let s = total(mid);
        if (s - target).abs() < 1e-12 * target {
            break;
        }
        if s > target {
            hi = mid;
            mid = 0.5 * (lo + hi);
        } else {
            lo = mid;
            mid = if hi.is_finite() { 0.5 * (lo + hi) } else { 2.0 * mid };
        }
    }
    // The mean over the neighbour distances includes the object itself (0).
    let mean_own = distances.iter().sum::<f64>() / (distances.len() + 1) as f64;
    let floor = 1e-3 * if rho > 0.0 { mean_own } else { mean_all };
    (rho, mid.max(floor))
}

/// The fuzzy neighbour graph of `proximity` with neighbourhood size `k`
/// (counting each object itself).
///
/// # Errors
/// Invalid input (see [`Proximity::validate`]) or `k` outside `2..=n`.
pub fn fuzzy_graph(proximity: &Proximity<'_>, k: usize) -> Result<FuzzyGraph> {
    proximity.validate(3)?;
    let n = proximity.n();
    if k < 2 || k > n {
        return Err(invalid(format!("the number of neighbours must be between 2 and the number of rows ({n})")));
    }
    let knn = proximity.nearest_neighbours(k - 1);
    let mean_all = knn.iter().flatten().map(|e| e.1).sum::<f64>() / (n * k) as f64;
    let mut rho = Vec::with_capacity(n);
    let mut sigma = Vec::with_capacity(n);
    let mut a: Vec<std::collections::BTreeMap<usize, f64>> = vec![std::collections::BTreeMap::new(); n];
    for (i, nb) in knn.iter().enumerate() {
        let d: Vec<f64> = nb.iter().map(|e| e.1).collect();
        let (r, s) = smooth_distances(&d, k, mean_all);
        for &(j, dij) in nb {
            let w = if dij - r <= 0.0 || s == 0.0 { 1.0 } else { (-(dij - r) / s).exp() };
            a[i].insert(j, w);
        }
        rho.push(r);
        sigma.push(s);
    }
    let mut rows: Vec<std::collections::BTreeMap<usize, f64>> = vec![std::collections::BTreeMap::new(); n];
    for i in 0..n {
        for (&j, &w) in &a[i] {
            let back = a[j].get(&i).copied().unwrap_or(0.0);
            let v = w + back - w * back;
            if v > 0.0 {
                rows[i].insert(j, v);
                rows[j].insert(i, v);
            }
        }
    }
    Ok(FuzzyGraph { rows: rows.into_iter().map(|r| r.into_iter().collect()).collect(), rho, sigma })
}

/// Fit `a`, `b` of `(1 + a x^{2b})⁻¹` to the target curve (Gauss–Newton
/// with step halving from `a = b = 1`).
///
/// # Errors
/// `spread ≤ 0`, `min_dist < 0` or `min_dist ≥ spread`.
pub fn curve_parameters(spread: f64, min_dist: f64) -> Result<(f64, f64)> {
    if !(spread > 0.0 && spread.is_finite()) || min_dist.is_nan() || min_dist < 0.0 || min_dist >= spread {
        return Err(invalid("UMAP needs spread > 0 and 0 ≤ minimum distance < spread"));
    }
    let xs: Vec<f64> = (0..300).map(|i| 3.0 * spread * f64::from(i) / 299.0).collect();
    let ys: Vec<f64> = xs.iter().map(|&x| if x < min_dist { 1.0 } else { (-(x - min_dist) / spread).exp() }).collect();
    let sse = |a: f64, b: f64| -> f64 {
        xs.iter().zip(&ys).map(|(&x, &y)| (1.0 / (1.0 + a * x.powf(2.0 * b)) - y).powi(2)).sum()
    };
    let (mut a, mut b) = (1.0_f64, 1.0_f64);
    let mut s = sse(a, b);
    for _ in 0..500 {
        // Normal equations of the 2-parameter Gauss–Newton step.
        let (mut jtj, mut jtr) = ([[0.0; 2]; 2], [0.0; 2]);
        for (&x, &y) in xs.iter().zip(&ys) {
            if x == 0.0 {
                continue;
            }
            let u = x.powf(2.0 * b);
            let f = 1.0 / (1.0 + a * u);
            let r = f - y;
            let da = -u * f * f;
            let db = -a * u * 2.0 * x.ln() * f * f;
            let g = [da, db];
            for p in 0..2 {
                jtr[p] += g[p] * r;
                for q in 0..2 {
                    jtj[p][q] += g[p] * g[q];
                }
            }
        }
        let det = jtj[0][0] * jtj[1][1] - jtj[0][1] * jtj[1][0];
        if det.abs() < 1e-300 {
            break;
        }
        let step = [(jtj[1][1] * jtr[0] - jtj[0][1] * jtr[1]) / det, (jtj[0][0] * jtr[1] - jtj[1][0] * jtr[0]) / det];
        let mut t = 1.0;
        let mut improved = false;
        while t > 1e-10 {
            let (na, nb) = (a - t * step[0], b - t * step[1]);
            if na > 0.0 && nb > 0.0 {
                let ns = sse(na, nb);
                if ns <= s {
                    let done = (s - ns) <= 1e-15 * s.max(1e-300) && t * (step[0].abs() + step[1].abs()) < 1e-12;
                    (a, b, s) = (na, nb, ns);
                    improved = !done;
                    break;
                }
            }
            t *= 0.5;
        }
        if !improved {
            break;
        }
    }
    Ok((a, b))
}

/// Leading non-trivial eigenvectors of `N = D^{-1/2} W D^{-1/2}`: dense
/// eigen-decomposition up to 1500 objects, else subspace iteration on
/// `(I + N)/2` with the trivial eigenvector `D^{1/2} 1` projected out.
fn spectral_start(graph: &FuzzyGraph, dims: usize) -> Option<Vec<Vec<f64>>> {
    spectral_start_with(graph, dims, 1500)
}

/// [`spectral_start`] with the dense route up to `dense_limit` objects.
fn spectral_start_with(graph: &FuzzyGraph, dims: usize, dense_limit: usize) -> Option<Vec<Vec<f64>>> {
    let n = graph.rows.len();
    let deg: Vec<f64> = graph.rows.iter().map(|r| r.iter().map(|e| e.1).sum()).collect();
    if deg.iter().any(|&d| d <= 0.0) || dims + 1 >= n {
        return None;
    }
    let inv_sqrt: Vec<f64> = deg.iter().map(|d| 1.0 / d.sqrt()).collect();
    if n <= dense_limit {
        let mut m = vec![vec![0.0; n]; n];
        for (i, row) in graph.rows.iter().enumerate() {
            for &(j, w) in row {
                m[i][j] = w * inv_sqrt[i] * inv_sqrt[j];
            }
        }
        let (_, v) = symmetric_eigen(&m).ok()?;
        return Some((0..n).map(|i| (1..=dims).map(|k| v[i][k]).collect()).collect());
    }
    // Subspace iteration.
    let total: f64 = deg.iter().sum();
    let trivial: Vec<f64> = deg.iter().map(|d| (d / total).sqrt()).collect();
    let m = dims + 4;
    let mut rng = Rng::new(0x05EC_77A1);
    let mut block: Vec<Vec<f64>> = (0..m).map(|_| (0..n).map(|_| rng.normal()).collect()).collect();
    let apply = |x: &[f64]| -> Vec<f64> {
        graph
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| 0.5 * (x[i] + row.iter().map(|&(j, w)| w * inv_sqrt[i] * inv_sqrt[j] * x[j]).sum::<f64>()))
            .collect()
    };
    let mut previous = vec![0.0; m];
    for _ in 0..2000 {
        block = block.iter().map(|x| apply(x)).collect();
        orthonormalize(&mut block, &trivial);
        // Rayleigh–Ritz.
        let ab: Vec<Vec<f64>> = block.iter().map(|x| apply(x)).collect();
        let h: Vec<Vec<f64>> = (0..m).map(|p| (0..m).map(|q| dot(&block[p], &ab[q])).collect()).collect();
        let (values, vecs) = symmetric_eigen(&h).ok()?;
        block = (0..m).map(|k| (0..n).map(|i| (0..m).map(|p| vecs[p][k] * block[p][i]).sum()).collect()).collect();
        let change = values.iter().zip(&previous).take(dims).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
        previous = values;
        if change < 1e-10 {
            break;
        }
    }
    Some((0..n).map(|i| (0..dims).map(|k| block[k][i]).collect()).collect())
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Gram–Schmidt (twice) of the rows of `block`, each first made orthogonal
/// to the unit vector `fixed`.
fn orthonormalize(block: &mut [Vec<f64>], fixed: &[f64]) {
    for _ in 0..2 {
        for k in 0..block.len() {
            let (done, rest) = block.split_at_mut(k);
            let x = &mut rest[0];
            let c = dot(x, fixed);
            x.iter_mut().zip(fixed).for_each(|(v, f)| *v -= c * f);
            for q in done.iter() {
                let c = dot(x, q);
                x.iter_mut().zip(q).for_each(|(v, f)| *v -= c * f);
            }
            let norm = dot(x, x).sqrt();
            if norm > 0.0 {
                x.iter_mut().for_each(|v| *v /= norm);
            }
        }
    }
}

/// Embed `proximity` by UMAP.
///
/// # Errors
/// Invalid input or settings: fewer than 3 objects, a neighbourhood size
/// outside `2..=n`, dimensions outside 1–3, invalid `min_dist`/`spread`, no
/// epochs.
pub fn umap(proximity: &Proximity<'_>, options: &UmapOptions) -> Result<Umap> {
    if !(1..=3).contains(&options.dims) {
        return Err(invalid("UMAP embeds in 1, 2 or 3 dimensions"));
    }
    if options.epochs == Some(0) {
        return Err(invalid("use at least one epoch"));
    }
    let (a, b) = curve_parameters(options.spread, options.min_dist)?;
    let graph = fuzzy_graph(proximity, options.neighbours)?;
    let n = graph.rows.len();
    let dims = options.dims;
    let epochs = options.epochs.unwrap_or(if n <= 10_000 { 500 } else { 200 });
    let mut rng = Rng::new(options.seed);
    // Start.
    let spectral = if options.random_start { None } else { spectral_start(&graph, dims) };
    let used_spectral = spectral.is_some();
    let mut y: Vec<Vec<f64>> = match spectral {
        Some(s) => {
            let big = s.iter().flatten().fold(0.0_f64, |m, v| m.max(v.abs()));
            let e = if big > 0.0 { 10.0 / big } else { 1.0 };
            s.into_iter().map(|r| r.into_iter().map(|v| v * e + 1e-4 * rng.normal()).collect()).collect()
        }
        None => (0..n).map(|_| (0..dims).map(|_| -10.0 + 20.0 * rng.next_f64()).collect()).collect(),
    };
    for k in 0..dims {
        let lo = y.iter().map(|r| r[k]).fold(f64::INFINITY, f64::min);
        let hi = y.iter().map(|r| r[k]).fold(f64::NEG_INFINITY, f64::max);
        let w = if hi > lo { hi - lo } else { 1.0 };
        y.iter_mut().for_each(|r| r[k] = 10.0 * (r[k] - lo) / w);
    }
    // Edges (both directions), dropping those too weak to be sampled.
    let max_w = graph.rows.iter().flatten().map(|e| e.1).fold(0.0, f64::max);
    let mut head = Vec::new();
    let mut tail = Vec::new();
    let mut per_sample = Vec::new();
    for (i, row) in graph.rows.iter().enumerate() {
        for &(j, w) in row {
            if w >= max_w / epochs as f64 {
                head.push(i);
                tail.push(j);
                per_sample.push(max_w / w);
            }
        }
    }
    let rate = options.negative_sample_rate as f64;
    let per_negative: Vec<f64> = per_sample.iter().map(|e| e / rate.max(f64::MIN_POSITIVE)).collect();
    let mut next_sample = per_sample.clone();
    let mut next_negative = per_negative.clone();
    let clip = |v: f64| v.clamp(-4.0, 4.0);
    for epoch in 0..epochs {
        let alpha = 1.0 - epoch as f64 / epochs as f64;
        let e = epoch as f64;
        for s in 0..head.len() {
            if next_sample[s] > e {
                continue;
            }
            let (j, k) = (head[s], tail[s]);
            let d2: f64 = (0..dims).map(|q| (y[j][q] - y[k][q]).powi(2)).sum();
            let coeff = if d2 > 0.0 { -2.0 * a * b * d2.powf(b - 1.0) / (a * d2.powf(b) + 1.0) } else { 0.0 };
            for q in 0..dims {
                let g = clip(coeff * (y[j][q] - y[k][q])) * alpha;
                y[j][q] += g;
                y[k][q] -= g;
            }
            next_sample[s] += per_sample[s];
            if options.negative_sample_rate > 0 {
                let draws = ((e - next_negative[s]) / per_negative[s]).max(0.0) as usize;
                for _ in 0..draws {
                    let other = rng.below(n);
                    let d2: f64 = (0..dims).map(|q| (y[j][q] - y[other][q]).powi(2)).sum();
                    if d2 > 0.0 {
                        let coeff = 2.0 * b / ((0.001 + d2) * (a * d2.powf(b) + 1.0));
                        for q in 0..dims {
                            let g = clip(coeff * (y[j][q] - y[other][q])) * alpha;
                            y[j][q] += g;
                        }
                    }
                }
                next_negative[s] += draws as f64 * per_negative[s];
            }
        }
    }
    Ok(Umap { coordinates: y, a, b, edges: head.len() / 2, epochs, spectral: used_spectral })
}

#[cfg(test)]
mod tests;
