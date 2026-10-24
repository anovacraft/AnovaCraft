//! t-distributed stochastic neighbour embedding (t-SNE; van der Maaten &
//! Hinton 2008, "Visualizing data using t-SNE", *JMLR* 9), with exact
//! gradients or the Barnes–Hut approximation (van der Maaten 2014,
//! "Accelerating t-SNE using tree-based algorithms", *JMLR* 15).
//!
//! # Affinities
//! For each object `i`, conditional probabilities
//! `p_{j|i} = exp(−β_i δᵢⱼ²) / Σ_{k≠i} exp(−β_i δᵢₖ²)` with the precision
//! `β_i` found by bisection so that the perplexity `2^{H(P_i)}` (equivalently
//! `exp` of the entropy in nats) equals the requested value. The joint
//! affinities are `pᵢⱼ = (p_{j|i} + p_{i|j}) / (2n)`. The Barnes–Hut
//! variant uses only the `⌊3·perplexity⌋` nearest neighbours of each object
//! (exact search), giving a sparse `P`.
//!
//! # Embedding
//! Low-dimensional similarities use a Student t kernel with one degree of
//! freedom: `wᵢⱼ = (1 + ‖yᵢ − yⱼ‖²)⁻¹`, `qᵢⱼ = wᵢⱼ / Z`, `Z = Σ_{k≠l} w_{kl}`.
//! The cost is `KL(P‖Q) = Σ pᵢⱼ ln(pᵢⱼ/qᵢⱼ)` with gradient
//! `∂C/∂yᵢ = 4 Σⱼ (pᵢⱼ − qᵢⱼ) wᵢⱼ (yᵢ − yⱼ)`. Barnes–Hut splits it into an
//! attractive part over the sparse `P` and a repulsive part
//! `Σⱼ wᵢⱼ² (yᵢ − yⱼ) / Z` summarised by a quadtree: a cell of width `r` at
//! distance `d` whose ratio `r/d` is below `θ` acts as one point at its
//! centre of mass (two-dimensional embeddings only).
//!
//! Gradient descent with momentum and per-coordinate adaptive gains
//! (Jacobs 1988): gains grow by 0.2 when the gradient changes sign against
//! the previous update and shrink by ×0.8 otherwise (minimum 0.01). The
//! first iterations multiply `P` by the early exaggeration factor (12) with
//! momentum 0.5; momentum is 0.8 afterwards. The learning rate defaults to
//! `max(n / exaggeration / 4, 50)` (Belkina et al. 2019, *Nature
//! Communications* 10). The start is the principal component (or, for a
//! distance matrix, classical scaling) layout scaled to standard deviation
//! 10⁻⁴ on its first axis, or seeded N(0, 10⁻⁴) noise. Everything is
//! reproducible from the seed.

use rayon::prelude::*;
use stat_core::rng::Rng;
use stat_core::{Result, StatError};

use crate::embedding::Proximity;

/// How the gradient is computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TsneMethod {
    /// Exact `O(n²)` gradient from the full affinity matrix.
    Exact,
    /// Barnes–Hut: sparse nearest-neighbour affinities and a quadtree
    /// (`O(n log n)` per iteration; two dimensions).
    BarnesHut,
}

impl TsneMethod {
    /// Stable key: `"exact"` or `"barnes-hut"`.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::BarnesHut => "barnes-hut",
        }
    }

    /// The method with this key.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        [Self::Exact, Self::BarnesHut].into_iter().find(|m| m.key() == key)
    }

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Exact => "exact gradient",
            Self::BarnesHut => "Barnes–Hut",
        }
    }
}

/// Largest number of objects for which [`TsneOptions::method`] `None`
/// chooses the exact gradient.
pub const EXACT_LIMIT: usize = 600;

/// Settings of a t-SNE embedding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TsneOptions {
    /// Embedding dimensions (1–3; Barnes–Hut needs 2).
    pub dims: usize,
    /// Perplexity (effective number of neighbours), below `n − 1`.
    pub perplexity: f64,
    /// Total gradient-descent iterations.
    pub iterations: usize,
    /// Early exaggeration factor.
    pub exaggeration: f64,
    /// Iterations with early exaggeration.
    pub exaggeration_iterations: usize,
    /// Learning rate (`None`: `max(n / exaggeration / 4, 50)`).
    pub learning_rate: Option<f64>,
    /// Barnes–Hut accuracy `θ` (0 = exact repulsion).
    pub theta: f64,
    /// Gradient method (`None`: exact up to [`EXACT_LIMIT`] objects, else
    /// Barnes–Hut when two-dimensional).
    pub method: Option<TsneMethod>,
    /// Start from random noise instead of the principal component layout.
    pub random_start: bool,
    /// Seed of the random start.
    pub seed: u64,
}

impl TsneOptions {
    /// Two dimensions, perplexity 30, 1000 iterations (250 exaggerated by
    /// 12), automatic learning rate and method, θ = 0.5, principal
    /// component start, seed 1.
    #[must_use]
    pub fn new() -> Self {
        Self {
            dims: 2,
            perplexity: 30.0,
            iterations: 1000,
            exaggeration: 12.0,
            exaggeration_iterations: 250,
            learning_rate: None,
            theta: 0.5,
            method: None,
            random_start: false,
            seed: 1,
        }
    }
}

impl Default for TsneOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// A t-SNE embedding.
#[derive(Debug, Clone, PartialEq)]
pub struct Tsne {
    /// Coordinates (`n` rows of `dims`).
    pub coordinates: Vec<Vec<f64>>,
    /// Final `KL(P‖Q)` (over the sparse affinities for Barnes–Hut).
    pub kl_divergence: f64,
    /// Gradient method used.
    pub method: TsneMethod,
    /// Learning rate used.
    pub learning_rate: f64,
}

/// Affinities as sparse symmetric rows `(j, pᵢⱼ)`, summing to 1 overall.
pub type SparseAffinities = Vec<Vec<(usize, f64)>>;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Conditional probabilities `p_{j|i}` over the given squared distances
/// (the other objects only) with perplexity `perplexity`: the precision β is
/// bisected (on the log scale) until the entropy is within 1e-10 of
/// `ln perplexity`. Returns the probabilities and β.
#[must_use]
pub fn calibrate(squared: &[f64], perplexity: f64) -> (Vec<f64>, f64) {
    let target = perplexity.ln();
    let min = squared.iter().copied().fold(f64::INFINITY, f64::min);
    let eval = |beta: f64| -> (Vec<f64>, f64) {
        // Shift by the smallest distance for stability.
        let w: Vec<f64> = squared.iter().map(|d| (-(d - min) * beta).exp()).collect();
        let s: f64 = w.iter().sum();
        let p: Vec<f64> = w.iter().map(|v| v / s).collect();
        let h = -p.iter().filter(|&&v| v > 0.0).map(|v| v * v.ln()).sum::<f64>();
        (p, h)
    };
    let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
    let mut log_beta = 0.0_f64;
    // Scale the starting guess to the distances.
    let mean = squared.iter().sum::<f64>() / squared.len().max(1) as f64;
    if mean > 0.0 {
        log_beta = -mean.ln();
    }
    let mut best = eval(log_beta.exp());
    for _ in 0..200 {
        let h = best.1;
        if (h - target).abs() <= 1e-10 {
            break;
        }
        // Entropy falls as β rises.
        if h > target {
            lo = log_beta;
            log_beta = if hi.is_finite() { 0.5 * (lo + hi) } else { log_beta + 1.0 };
        } else {
            hi = log_beta;
            log_beta = if lo.is_finite() { 0.5 * (lo + hi) } else { log_beta - 1.0 };
        }
        best = eval(log_beta.exp());
    }
    (best.0, log_beta.exp())
}

fn check(proximity: &Proximity<'_>, perplexity: f64) -> Result<usize> {
    proximity.validate(4)?;
    let n = proximity.n();
    if !(perplexity >= 1.0 && perplexity < (n - 1) as f64) {
        return Err(invalid(format!("the perplexity must be at least 1 and below n − 1 = {}", n - 1)));
    }
    Ok(n)
}

/// Dense joint affinities `pᵢⱼ` (`n × n`, zero diagonal, summing to 1).
///
/// # Errors
/// Invalid input (see [`Proximity::validate`]), fewer than 4 objects, or a
/// perplexity outside `[1, n − 1)`.
pub fn joint_probabilities(proximity: &Proximity<'_>, perplexity: f64) -> Result<Vec<Vec<f64>>> {
    let n = check(proximity, perplexity)?;
    let conditional: Vec<Vec<f64>> = (0..n)
        .into_par_iter()
        .map(|i| {
            let sq: Vec<f64> = (0..n).filter(|&j| j != i).map(|j| proximity.distance(i, j).powi(2)).collect();
            let (p, _) = calibrate(&sq, perplexity);
            let mut row = vec![0.0; n];
            for (j, v) in (0..n).filter(|&j| j != i).zip(p) {
                row[j] = v;
            }
            row
        })
        .collect();
    let nn = 2.0 * n as f64;
    Ok((0..n).map(|i| (0..n).map(|j| (conditional[i][j] + conditional[j][i]) / nn).collect()).collect())
}

/// Sparse joint affinities from each object's `min(n − 1, ⌊3·perplexity⌋ + 1)`
/// nearest neighbours (as scikit-learn's Barnes–Hut t-SNE), symmetrized
/// and normalized to sum 1.
///
/// # Errors
/// As [`joint_probabilities`].
pub fn sparse_joint_probabilities(proximity: &Proximity<'_>, perplexity: f64) -> Result<SparseAffinities> {
    let n = check(proximity, perplexity)?;
    let k = (n - 1).min((3.0 * perplexity) as usize + 1);
    let knn = proximity.nearest_neighbours(k);
    let conditional: Vec<Vec<(usize, f64)>> = knn
        .par_iter()
        .map(|nb| {
            let sq: Vec<f64> = nb.iter().map(|&(_, d)| d * d).collect();
            let (p, _) = calibrate(&sq, perplexity);
            nb.iter().map(|&(j, _)| j).zip(p).collect()
        })
        .collect();
    Ok(symmetrize(&conditional))
}

/// `P + Pᵀ` normalized to sum 1, as sorted sparse rows.
fn symmetrize(conditional: &[Vec<(usize, f64)>]) -> SparseAffinities {
    let n = conditional.len();
    let mut rows: Vec<std::collections::BTreeMap<usize, f64>> = vec![std::collections::BTreeMap::new(); n];
    for (i, row) in conditional.iter().enumerate() {
        for &(j, p) in row {
            *rows[i].entry(j).or_insert(0.0) += p;
            *rows[j].entry(i).or_insert(0.0) += p;
        }
    }
    let total: f64 = rows.iter().flat_map(|r| r.values()).sum();
    rows.into_iter().map(|r| r.into_iter().map(|(j, v)| (j, v / total)).collect()).collect()
}

/// `KL(P‖Q)` and its exact gradient for affinities `p` (dense) at
/// configuration `y`.
#[must_use]
pub fn exact_gradient(p: &[Vec<f64>], y: &[Vec<f64>]) -> (f64, Vec<Vec<f64>>) {
    let n = y.len();
    let d = y[0].len();
    let w: Vec<Vec<f64>> = (0..n)
        .into_par_iter()
        .map(|i| (0..n).map(|j| if i == j { 0.0 } else { 1.0 / (1.0 + sq_dist(&y[i], &y[j])) }).collect())
        .collect();
    let z: f64 = w.iter().flatten().sum();
    let grad: Vec<Vec<f64>> = (0..n)
        .into_par_iter()
        .map(|i| {
            let mut g = vec![0.0; d];
            for j in 0..n {
                if j == i {
                    continue;
                }
                let m = (p[i][j] - w[i][j] / z) * w[i][j];
                for k in 0..d {
                    g[k] += 4.0 * m * (y[i][k] - y[j][k]);
                }
            }
            g
        })
        .collect();
    let mut kl = 0.0;
    for i in 0..n {
        for j in 0..n {
            if i != j && p[i][j] > 0.0 {
                kl += p[i][j] * (p[i][j] / (w[i][j] / z).max(f64::MIN_POSITIVE)).ln();
            }
        }
    }
    (kl, grad)
}

fn sq_dist(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(u, v)| (u - v) * (u - v)).sum()
}

/// A quadtree over two-dimensional points for the Barnes–Hut repulsion.
struct QuadTree {
    nodes: Vec<Node>,
}

struct Node {
    /// Centre and half-width of the square cell.
    cx: f64,
    cy: f64,
    half: f64,
    /// Sum of the coordinates of the points inside, and their number.
    sx: f64,
    sy: f64,
    count: usize,
    /// First child index (four consecutive nodes) or none for a leaf.
    children: Option<usize>,
    /// Points stored in a leaf (several only when they coincide or the
    /// depth limit is reached).
    points: Vec<usize>,
    depth: u32,
}

impl QuadTree {
    fn build(y: &[Vec<f64>]) -> Self {
        let (mut x0, mut x1, mut y0, mut y1) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
        for p in y {
            x0 = x0.min(p[0]);
            x1 = x1.max(p[0]);
            y0 = y0.min(p[1]);
            y1 = y1.max(p[1]);
        }
        let half = 0.5 * (x1 - x0).max(y1 - y0).max(1e-12) * (1.0 + 1e-9);
        let root = Node {
            cx: 0.5 * (x0 + x1),
            cy: 0.5 * (y0 + y1),
            half,
            sx: 0.0,
            sy: 0.0,
            count: 0,
            children: None,
            points: Vec::new(),
            depth: 0,
        };
        let mut t = Self { nodes: vec![root] };
        for i in 0..y.len() {
            t.insert(0, i, y);
        }
        t
    }

    fn insert(&mut self, mut node: usize, i: usize, y: &[Vec<f64>]) {
        let (px, py) = (y[i][0], y[i][1]);
        loop {
            let n = &mut self.nodes[node];
            n.sx += px;
            n.sy += py;
            n.count += 1;
            if let Some(first) = n.children {
                let q = usize::from(px > n.cx) + 2 * usize::from(py > n.cy);
                node = first + q;
                continue;
            }
            let coincide = n.points.iter().all(|&j| y[j][0] == px && y[j][1] == py);
            if n.points.is_empty() || coincide || n.depth >= 48 {
                n.points.push(i);
                return;
            }
            // Split the leaf and push its points down.
            let (cx, cy, h, depth) = (n.cx, n.cy, n.half / 2.0, n.depth + 1);
            let old = std::mem::take(&mut n.points);
            let first = self.nodes.len();
            self.nodes[node].children = Some(first);
            for q in 0..4 {
                let ox = if q & 1 == 1 { h } else { -h };
                let oy = if q & 2 == 2 { h } else { -h };
                self.nodes.push(Node {
                    cx: cx + ox,
                    cy: cy + oy,
                    half: h,
                    sx: 0.0,
                    sy: 0.0,
                    count: 0,
                    children: None,
                    points: Vec::new(),
                    depth,
                });
            }
            for j in old {
                let q = usize::from(y[j][0] > cx) + 2 * usize::from(y[j][1] > cy);
                self.insert_below(first + q, j, y);
            }
            // Continue inserting `i` from this (now internal) node, whose
            // sums already include it.
            let q = usize::from(px > cx) + 2 * usize::from(py > cy);
            node = first + q;
        }
    }

    /// Insert `j` starting at `node` (used when splitting: the parent's sums
    /// already include `j`).
    fn insert_below(&mut self, node: usize, j: usize, y: &[Vec<f64>]) {
        self.insert(node, j, y);
    }

    /// Repulsive force `Σⱼ wᵢⱼ² (yᵢ − yⱼ)` and `Σⱼ wᵢⱼ` for point `i`.
    fn repulsion(&self, i: usize, y: &[Vec<f64>], theta: f64) -> ([f64; 2], f64) {
        let (px, py) = (y[i][0], y[i][1]);
        let mut force = [0.0; 2];
        let mut z = 0.0;
        let mut stack = vec![0usize];
        while let Some(k) = stack.pop() {
            let n = &self.nodes[k];
            if n.count == 0 {
                continue;
            }
            match n.children {
                None => {
                    for &j in &n.points {
                        if j == i {
                            continue;
                        }
                        let (dx, dy) = (px - y[j][0], py - y[j][1]);
                        let w = 1.0 / (1.0 + dx * dx + dy * dy);
                        z += w;
                        force[0] += w * w * dx;
                        force[1] += w * w * dy;
                    }
                }
                Some(first) => {
                    let c = n.count as f64;
                    let (mx, my) = (n.sx / c, n.sy / c);
                    let (dx, dy) = (px - mx, py - my);
                    let d2 = dx * dx + dy * dy;
                    let inside = (px - n.cx).abs() <= n.half && (py - n.cy).abs() <= n.half;
                    if !inside && d2 > 0.0 && 2.0 * n.half < theta * d2.sqrt() {
                        let w = 1.0 / (1.0 + d2);
                        z += c * w;
                        force[0] += c * w * w * dx;
                        force[1] += c * w * w * dy;
                    } else {
                        stack.extend(first..first + 4);
                    }
                }
            }
        }
        (force, z)
    }
}

/// Barnes–Hut gradient for sparse affinities `p` at the two-dimensional
/// configuration `y`, with the normalization `Z` estimated by the tree.
#[must_use]
pub fn barnes_hut_gradient(p: &SparseAffinities, y: &[Vec<f64>], theta: f64) -> (Vec<Vec<f64>>, f64) {
    let tree = QuadTree::build(y);
    let reps: Vec<([f64; 2], f64)> = (0..y.len()).into_par_iter().map(|i| tree.repulsion(i, y, theta)).collect();
    let z: f64 = reps.iter().map(|r| r.1).sum();
    let grad = (0..y.len())
        .into_par_iter()
        .map(|i| {
            let mut g = [0.0; 2];
            for &(j, pij) in &p[i] {
                let (dx, dy) = (y[i][0] - y[j][0], y[i][1] - y[j][1]);
                let w = 1.0 / (1.0 + dx * dx + dy * dy);
                g[0] += pij * w * dx;
                g[1] += pij * w * dy;
            }
            vec![4.0 * (g[0] - reps[i].0[0] / z), 4.0 * (g[1] - reps[i].0[1] / z)]
        })
        .collect();
    (grad, z)
}

/// Starting layout: principal components (classical scaling for a distance
/// matrix) scaled to SD 10⁻⁴ on the first axis, or seeded N(0, 10⁻⁴²) noise.
pub(crate) fn start_layout(proximity: &Proximity<'_>, dims: usize, random: bool, seed: u64) -> Result<Vec<Vec<f64>>> {
    let n = proximity.n();
    if !random {
        let x = crate::mds::classical_scaling(proximity, dims.min(n - 1))?.coordinates;
        let m = x.iter().map(|r| r[0]).sum::<f64>() / n as f64;
        let sd = (x.iter().map(|r| (r[0] - m).powi(2)).sum::<f64>() / n as f64).sqrt();
        if sd > 0.0 {
            return Ok(x.into_iter().map(|r| r.into_iter().map(|v| v / sd * 1e-4).collect()).collect());
        }
    }
    let mut rng = Rng::new(seed);
    Ok((0..n).map(|_| (0..dims).map(|_| 1e-4 * rng.normal()).collect()).collect())
}

/// Embed `proximity` by t-SNE.
///
/// # Errors
/// Invalid input or settings: fewer than 4 objects, a perplexity outside
/// `[1, n − 1)`, dimensions outside 1–3, Barnes–Hut requested for other than
/// two dimensions, no iterations, a non-positive learning rate or a
/// negative θ.
pub fn tsne(proximity: &Proximity<'_>, options: &TsneOptions) -> Result<Tsne> {
    let n = check(proximity, options.perplexity)?;
    if !(1..=3).contains(&options.dims) {
        return Err(invalid("t-SNE embeds in 1, 2 or 3 dimensions"));
    }
    if options.iterations == 0 {
        return Err(invalid("use at least one iteration"));
    }
    if options.theta.is_nan() || options.theta < 0.0 {
        return Err(invalid("θ must be zero or positive"));
    }
    let method = match options.method {
        Some(TsneMethod::BarnesHut) if options.dims != 2 => {
            return Err(invalid("the Barnes–Hut approximation needs a two-dimensional embedding"));
        }
        Some(m) => m,
        None if n <= EXACT_LIMIT || options.dims != 2 => TsneMethod::Exact,
        None => TsneMethod::BarnesHut,
    };
    let exaggeration = options.exaggeration.max(1.0);
    let rate = options.learning_rate.unwrap_or((n as f64 / exaggeration / 4.0).max(50.0));
    if !(rate > 0.0 && rate.is_finite()) {
        return Err(invalid("the learning rate must be positive"));
    }
    let mut y = start_layout(proximity, options.dims, options.random_start, options.seed)?;
    let dims = options.dims;
    let dense =
        (method == TsneMethod::Exact).then(|| joint_probabilities(proximity, options.perplexity)).transpose()?;
    let sparse = (method == TsneMethod::BarnesHut)
        .then(|| sparse_joint_probabilities(proximity, options.perplexity))
        .transpose()?;
    let mut update = vec![vec![0.0; dims]; n];
    let mut gains = vec![vec![1.0_f64; dims]; n];
    let scale = |f: f64, dense: &Option<Vec<Vec<f64>>>, sparse: &Option<SparseAffinities>| {
        (
            dense.as_ref().map(|p| p.iter().map(|r| r.iter().map(|v| v * f).collect()).collect::<Vec<Vec<f64>>>()),
            sparse
                .as_ref()
                .map(|p| p.iter().map(|r| r.iter().map(|&(j, v)| (j, v * f)).collect()).collect::<SparseAffinities>()),
        )
    };
    let (ex_dense, ex_sparse) = scale(exaggeration, &dense, &sparse);
    for it in 0..options.iterations {
        let early = it < options.exaggeration_iterations;
        let momentum = if early { 0.5 } else { 0.8 };
        let grad = match method {
            TsneMethod::Exact => {
                let p = if early { ex_dense.as_ref() } else { dense.as_ref() };
                exact_gradient(p.map_or(&[][..], Vec::as_slice), &y).1
            }
            TsneMethod::BarnesHut => {
                let p = if early { ex_sparse.as_ref() } else { sparse.as_ref() };
                barnes_hut_gradient(p.unwrap_or(&Vec::new()), &y, options.theta).0
            }
        };
        for i in 0..n {
            for k in 0..dims {
                let g = grad[i][k];
                if update[i][k] * g < 0.0 {
                    gains[i][k] += 0.2;
                } else {
                    gains[i][k] = (gains[i][k] * 0.8).max(0.01);
                }
                update[i][k] = momentum * update[i][k] - rate * gains[i][k] * g;
                y[i][k] += update[i][k];
            }
        }
    }
    let kl_divergence = match (&dense, &sparse) {
        (Some(p), _) => exact_gradient(p, &y).0,
        (_, Some(p)) => sparse_kl(p, &y),
        _ => f64::NAN,
    };
    // Centre the result.
    for k in 0..dims {
        let m = y.iter().map(|r| r[k]).sum::<f64>() / n as f64;
        y.iter_mut().for_each(|r| r[k] -= m);
    }
    Ok(Tsne { coordinates: y, kl_divergence, method, learning_rate: rate })
}

/// `KL(P‖Q)` over the sparse affinities, with `Z` summed exactly.
fn sparse_kl(p: &SparseAffinities, y: &[Vec<f64>]) -> f64 {
    let n = y.len();
    let z: f64 = (0..n)
        .into_par_iter()
        .map(|i| (0..n).filter(|&j| j != i).map(|j| 1.0 / (1.0 + sq_dist(&y[i], &y[j]))).sum::<f64>())
        .sum();
    let mut kl = 0.0;
    for (i, row) in p.iter().enumerate() {
        for &(j, pij) in row {
            if pij > 0.0 {
                let q = 1.0 / (1.0 + sq_dist(&y[i], &y[j])) / z;
                kl += pij * (pij / q).ln();
            }
        }
    }
    kl
}

#[cfg(test)]
mod tests;
