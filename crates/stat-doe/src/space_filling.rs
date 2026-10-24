//! Space-filling designs for computer experiments.
//!
//! A deterministic computer model has no random error, so replicates and
//! centre points tell nothing and the classical designs, which put runs at
//! the corners of the region, waste most of them. Space-filling designs
//! spread the runs evenly over the whole region instead, so that a flexible
//! surrogate (a Gaussian process, a spline, a polynomial) can be fitted
//! anywhere (Santner, Williams & Notz, *The Design and Analysis of Computer
//! Experiments*, 2nd ed., 2018, ch. 5).
//!
//! Every design here is built in the unit cube `[0, 1]^k` and returned as a
//! [`Design`] in coded units `[−1, 1]^k` (coded = 2u − 1), so each factor's
//! low and high settings map to the ends of its range. All criteria are
//! measured in the unit cube.
//!
//! # Criteria
//! * **Minimum distance** `d_min = min_{i<j} ‖x_i − x_j‖` (Euclidean): a
//!   *maximin* design makes it as large as possible (Johnson, Moore &
//!   Ylvisaker 1990, "Minimax and maximin distance designs", *Journal of
//!   Statistical Planning and Inference* 26).
//! * **φ_p** `= (Σ_{i<j} d_ij^{−p})^{1/p}` (Morris & Mitchell 1995,
//!   "Exploratory designs for computational experiments", *J. Statist. Plann.
//!   Inference* 43): a smooth stand-in for `1/d_min` that also counts how many
//!   pairs are close; smaller is better, and as `p → ∞` minimizing φ_p
//!   maximizes `d_min`. [`phi_p`] evaluates it.
//! * **Fill distance** `h = sup_{x ∈ [0,1]^k} min_i ‖x − x_i‖`, the radius of
//!   the largest empty ball centred in the region: a *minimax* design makes it
//!   as small as possible, so no point of the region is far from a run.
//!   [`fill_distance`] measures it over a finite set of points; with
//!   [`evaluation_points`] (a regular grid that includes the corners when it
//!   fits, plus a Halton sequence) the result is an approximation from below.
//! * **Centered L2 discrepancy** (Hickernell 1998, "A generalized discrepancy
//!   and quadrature error bound", *Mathematics of Computation* 67), for `n`
//!   points with `z = x − ½`:
//!   `CD² = (13/12)^k − (2/n) Σ_i Π_j (1 + ½|z_ij| − ½z_ij²) +
//!   (1/n²) Σ_i Σ_l Π_j (1 + ½|z_ij| + ½|z_lj| − ½|x_ij − x_lj|)`.
//!   It measures how far the points' empirical distribution is from uniform
//!   over every corner-anchored box; [`centered_l2_discrepancy`] returns `CD`
//!   (the square root).
//!
//! # Designs
//! * [`maximin_latin_hypercube`]: a Latin hypercube (each column a permutation
//!   of the levels `(i − ½)/n`, so every factor is sampled once in each of `n`
//!   equal slices) minimizing φ_p (p = 50) by simulated annealing over swaps
//!   of two entries of a column (Morris & Mitchell 1995). Schedule: the
//!   starting temperature accepts the average worsening of `ln φ_p` among 100
//!   sampled swaps with probability ½; the temperature falls geometrically
//!   through 100 levels to 10⁻³ of its start, with an equal share of the swap
//!   budget at each level; the best design seen is kept.
//! * [`maximin_distance`]: maximin design not restricted to a lattice. Runs
//!   are chosen from a dense candidate set (regular grids with 2, 3, 4, …
//!   levels per factor while they fit, plus uniform random points) by greedy
//!   farthest-point selection, then improved by exchange: a point of the
//!   closest pair is replaced by the candidate farthest from the other points
//!   whenever that increases `d_min` or reduces the number of pairs at `d_min`.
//!   Finally the points move continuously by projected gradient descent on
//!   `ln φ_p` with `p` rising from 20 to 5000; the refined design is kept only
//!   if its `d_min` is larger.
//! * [`minimax_distance`]: the clustering method of Mak & Joseph (2018),
//!   "Minimax and minimax projection designs using clustering", *Journal of
//!   Computational and Graphical Statistics* 27(1): the region is represented
//!   by [`evaluation_points`]; k-means (k-means++ starts, Lloyd iterations)
//!   gives an initial partition; then each run is moved to the centre of the
//!   smallest ball enclosing its cluster (Frank–Wolfe iterations of Yildirim
//!   2008, *SIAM J. Optim.* 19) and the points are reassigned, repeating while
//!   the fill distance over the evaluation points falls. The design minimizes
//!   the fill distance *over those points*, an approximation to the region.
//! * [`uniform_design`]: a U-type design (columns permutations of the levels
//!   `(2i − 1)/(2n)`) minimizing the centered L2 discrepancy by threshold
//!   accepting with swaps of two entries of a column (Fang, Lin, Winker & Zhang
//!   2000, "Uniform design: theory and application", *Technometrics* 42).
//!   Thresholds fall linearly through 50 levels from the 10th percentile of
//!   `|ΔCD²|` among 200 sampled swaps to zero.
//!
//! Every search is seeded, so the same seed gives the same design, and its
//! work is budgeted by the size of the problem so that designs up to
//! [`MAX_RUNS`] runs and [`MAX_FACTORS`] factors finish quickly; they are
//! heuristics, and another seed may give a slightly better design.
//!
//! # Failure modes
//! [`StatError::InvalidArgument`] for fewer than 2 runs, no factors, or more
//! than [`MAX_RUNS`] runs or [`MAX_FACTORS`] factors.

use crate::design::{Design, Factor, PointType, Rng, default_factor_names};
use stat_core::{Result, StatError};

/// Most runs of a space-filling design.
pub const MAX_RUNS: usize = 1000;
/// Most factors of a space-filling design.
pub const MAX_FACTORS: usize = 20;
/// The exponent `p` of the φ_p criterion used by the maximin Latin hypercube.
pub const DEFAULT_P: f64 = 50.0;

/// The space-filling designs offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpaceFillingKind {
    /// Latin hypercube minimizing φ₅₀ ([`maximin_latin_hypercube`]).
    MaximinLatinHypercube,
    /// Largest minimum distance ([`maximin_distance`]).
    MaximinDistance,
    /// Smallest fill distance ([`minimax_distance`]).
    MinimaxDistance,
    /// Smallest centered L2 discrepancy ([`uniform_design`]).
    Uniform,
}

impl SpaceFillingKind {
    /// Every kind, in menu order.
    pub const ALL: [Self; 4] =
        [Self::MaximinLatinHypercube, Self::MaximinDistance, Self::MinimaxDistance, Self::Uniform];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::MaximinLatinHypercube => "Latin hypercube (maximin)",
            Self::MaximinDistance => "Maximin distance",
            Self::MinimaxDistance => "Minimax distance",
            Self::Uniform => "Uniform (centered L2 discrepancy)",
        }
    }
}

/// The space-filling criteria of a set of points in `[0, 1]^k`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpaceFillingCriteria {
    /// Smallest distance between two points.
    pub min_distance: f64,
    /// φ_p with p = [`DEFAULT_P`].
    pub phi_p: f64,
    /// Centered L2 discrepancy (`CD`, not squared).
    pub centered_l2: f64,
    /// Fill distance over [`evaluation_points`] (an approximation from below).
    pub fill_distance: f64,
    /// Number of evaluation points used for the fill distance.
    pub evaluation_points: usize,
}

/// Build the space-filling design `kind` with `runs` runs and `factors`
/// factors (named `A, B, …`, coded −1 to +1), searching from `seed`.
///
/// # Errors
/// See the [module documentation](self).
pub fn space_filling_design(kind: SpaceFillingKind, runs: usize, factors: usize, seed: u64) -> Result<Design> {
    match kind {
        SpaceFillingKind::MaximinLatinHypercube => maximin_latin_hypercube(runs, factors, seed),
        SpaceFillingKind::MaximinDistance => maximin_distance(runs, factors, seed),
        SpaceFillingKind::MinimaxDistance => minimax_distance(runs, factors, seed),
        SpaceFillingKind::Uniform => uniform_design(runs, factors, seed),
    }
}

/// A Latin hypercube minimizing φ₅₀ by simulated annealing (Morris & Mitchell
/// 1995); see the [module documentation](self).
///
/// # Errors
/// [`StatError::InvalidArgument`] for runs outside `2..=1000` or factors
/// outside `1..=20`.
pub fn maximin_latin_hypercube(runs: usize, factors: usize, seed: u64) -> Result<Design> {
    validate(runs, factors)?;
    let mut rng = Rng::new(seed);
    let (points, proposals) = anneal_phi_p(runs, factors, DEFAULT_P, &mut rng);
    finish(
        format!("Maximin Latin hypercube ({runs} runs)"),
        &points,
        format!("Search: simulated annealing on φ₅₀ over column swaps, {proposals} proposals, seed {seed}"),
    )
}

/// A maximin distance design (Johnson, Moore & Ylvisaker 1990) by point
/// exchange over a dense candidate set and continuous refinement; see the
/// [module documentation](self).
///
/// # Errors
/// As for [`maximin_latin_hypercube`].
pub fn maximin_distance(runs: usize, factors: usize, seed: u64) -> Result<Design> {
    validate(runs, factors)?;
    let mut rng = Rng::new(seed);
    let (points, candidates) = maximin_points(runs, factors, &mut rng);
    finish(
        format!("Maximin distance design ({runs} runs)"),
        &points,
        format!(
            "Search: point exchange over {candidates} candidates, then gradient refinement of φ_p (p up to 5000), \
             seed {seed}"
        ),
    )
}

/// A minimax distance design by clustering (Mak & Joseph 2018); see the
/// [module documentation](self).
///
/// # Errors
/// As for [`maximin_latin_hypercube`].
pub fn minimax_distance(runs: usize, factors: usize, seed: u64) -> Result<Design> {
    validate(runs, factors)?;
    let mut rng = Rng::new(seed);
    let region = evaluation_points(factors, runs);
    let points = minimax_points(runs, &region, &mut rng);
    finish(
        format!("Minimax distance design ({runs} runs)"),
        &points,
        format!(
            "Search: k-means then minimum-enclosing-ball centres of the clusters of {} points representing the \
             region, seed {seed}",
            region.len()
        ),
    )
}

/// A uniform design minimizing the centered L2 discrepancy by threshold
/// accepting (Fang, Lin, Winker & Zhang 2000); see the
/// [module documentation](self).
///
/// # Errors
/// As for [`maximin_latin_hypercube`].
pub fn uniform_design(runs: usize, factors: usize, seed: u64) -> Result<Design> {
    validate(runs, factors)?;
    let mut rng = Rng::new(seed);
    let (points, proposals) = threshold_accept_cd(runs, factors, &mut rng);
    finish(
        format!("Uniform design ({runs} runs)"),
        &points,
        format!("Search: threshold accepting on the centered L2 discrepancy, {proposals} proposals, seed {seed}"),
    )
}

fn validate(runs: usize, factors: usize) -> Result<()> {
    let invalid = |message: String| Err(StatError::InvalidArgument { message });
    if !(2..=MAX_RUNS).contains(&runs) {
        return invalid(format!("a space-filling design needs 2 to {MAX_RUNS} runs (got {runs})"));
    }
    if !(1..=MAX_FACTORS).contains(&factors) {
        return invalid(format!("a space-filling design needs 1 to {MAX_FACTORS} factors (got {factors})"));
    }
    Ok(())
}

/// Turn unit-cube points into a coded design with criteria notes.
fn finish(description: String, points: &[Vec<f64>], search: String) -> Result<Design> {
    let k = points[0].len();
    let factors = default_factor_names(k).into_iter().map(Factor::coded).collect();
    let runs = points.iter().map(|p| p.iter().map(|u| 2.0 * u - 1.0).collect()).collect();
    let mut design = Design::new(description, factors, runs, vec![PointType::Other; points.len()])?;
    let c = criteria(points);
    design.notes = vec![
        "Criteria with each factor's range scaled to 0…1:".to_owned(),
        format!("Minimum distance between runs: {:.6}", c.min_distance),
        format!("φ₅₀ (Morris–Mitchell; smaller is better): {:.6}", c.phi_p),
        format!("Centered L2 discrepancy: {:.6}", c.centered_l2),
        format!(
            "Fill distance (largest gap, over {} points of the region): {:.6}",
            c.evaluation_points, c.fill_distance
        ),
        search,
    ];
    Ok(design)
}

/// All space-filling criteria of `points` (in `[0, 1]^k`).
#[must_use]
pub fn criteria(points: &[Vec<f64>]) -> SpaceFillingCriteria {
    let k = points.first().map_or(1, Vec::len);
    let region = evaluation_points(k, points.len());
    SpaceFillingCriteria {
        min_distance: min_distance(points),
        phi_p: phi_p(points, DEFAULT_P),
        centered_l2: centered_l2_discrepancy(points),
        fill_distance: fill_distance(points, &region),
        evaluation_points: region.len(),
    }
}

fn sq_dist(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum()
}

/// The smallest Euclidean distance between two of `points` (`∞` for fewer
/// than two points).
#[must_use]
pub fn min_distance(points: &[Vec<f64>]) -> f64 {
    let mut best = f64::INFINITY;
    for (i, a) in points.iter().enumerate() {
        for b in &points[i + 1..] {
            best = best.min(sq_dist(a, b));
        }
    }
    best.sqrt()
}

/// The Morris–Mitchell criterion `φ_p = (Σ_{i<j} d_ij^{−p})^{1/p}`, evaluated
/// as `(1/d_min)(Σ (d_min/d_ij)^p)^{1/p}` so large `p` neither overflows nor
/// underflows. `∞` when two points coincide; 0 for fewer than two points.
#[must_use]
pub fn phi_p(points: &[Vec<f64>], p: f64) -> f64 {
    if points.len() < 2 {
        return 0.0;
    }
    let dmin = min_distance(points);
    if dmin == 0.0 {
        return f64::INFINITY;
    }
    let mut sum = 0.0;
    for (i, a) in points.iter().enumerate() {
        for b in &points[i + 1..] {
            sum += (dmin / sq_dist(a, b).sqrt()).powf(p);
        }
    }
    sum.powf(1.0 / p) / dmin
}

/// Hickernell's (1998) centered L2 discrepancy `CD` of `points` in
/// `[0, 1]^k` (the square root of the formula in the
/// [module documentation](self)); 0 for no points.
#[must_use]
pub fn centered_l2_discrepancy(points: &[Vec<f64>]) -> f64 {
    let n = points.len();
    if n == 0 {
        return 0.0;
    }
    let k = points[0].len();
    let single: f64 = points.iter().map(|x| x.iter().map(|&u| cd_single(u - 0.5)).product::<f64>()).sum();
    let mut pairs = 0.0;
    for a in points {
        for b in points {
            pairs += a.iter().zip(b).map(|(&u, &v)| cd_pair(u, v)).product::<f64>();
        }
    }
    let nf = n as f64;
    let cd2 = (13.0_f64 / 12.0).powi(k as i32) - 2.0 / nf * single + pairs / (nf * nf);
    cd2.max(0.0).sqrt()
}

fn cd_single(z: f64) -> f64 {
    1.0 + 0.5 * z.abs() - 0.5 * z * z
}

fn cd_pair(u: f64, v: f64) -> f64 {
    1.0 + 0.5 * (u - 0.5).abs() + 0.5 * (v - 0.5).abs() - 0.5 * (u - v).abs()
}

/// The fill distance of `design` over `region`: the largest distance from a
/// point of `region` to its nearest design point. Over a finite set this
/// bounds the true fill distance of the continuous region from below.
#[must_use]
pub fn fill_distance(design: &[Vec<f64>], region: &[Vec<f64>]) -> f64 {
    region
        .iter()
        .map(|r| design.iter().map(|d| sq_dist(r, d)).fold(f64::INFINITY, f64::min))
        .fold(0.0_f64, f64::max)
        .sqrt()
}

const PRIMES: [u64; MAX_FACTORS] = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59, 61, 67, 71];

fn radical_inverse(mut i: u64, base: u64) -> f64 {
    let (mut f, mut r) = (1.0, 0.0);
    while i > 0 {
        f /= base as f64;
        r += f * (i % base) as f64;
        i /= base;
    }
    r
}

/// Points representing the region `[0, 1]^k` for measuring fill distance
/// for a design of `runs` runs: a regular grid with as many levels per
/// factor as fit in half the budget (including the corners whenever
/// `2^k` fits), then a Halton sequence (bases 2, 3, 5, …; indices from 1)
/// up to `clamp(20·runs, 4096, 10000)` points in all. Deterministic.
#[must_use]
pub fn evaluation_points(k: usize, runs: usize) -> Vec<Vec<f64>> {
    let k = k.clamp(1, MAX_FACTORS);
    let total = (20 * runs).clamp(4096, 10_000);
    let mut points = Vec::with_capacity(total);
    let mut m = 2usize;
    while (m + 1).checked_pow(k as u32).is_some_and(|c| c <= total / 2) {
        m += 1;
    }
    if m.checked_pow(k as u32).is_some_and(|c| c <= total / 2) {
        points.extend(grid(k, m));
    }
    let mut i = 1u64;
    while points.len() < total {
        points.push((0..k).map(|j| radical_inverse(i, PRIMES[j])).collect());
        i += 1;
    }
    points
}

/// Every point of the regular grid with `m` levels `0, 1/(m−1), …, 1` per factor.
fn grid(k: usize, m: usize) -> Vec<Vec<f64>> {
    let count = m.pow(k as u32);
    (0..count)
        .map(|mut c| {
            (0..k)
                .map(|_| {
                    let level = c % m;
                    c /= m;
                    level as f64 / (m - 1) as f64
                })
                .collect()
        })
        .collect()
}

/// A random U-type design: each column a random permutation of `(2i+1)/(2n)`.
fn random_u_type(n: usize, k: usize, rng: &mut Rng) -> Vec<Vec<f64>> {
    let mut x = vec![vec![0.0; k]; n];
    let mut perm: Vec<usize> = (0..n).collect();
    for j in 0..k {
        rng.shuffle(&mut perm);
        for (row, &l) in x.iter_mut().zip(&perm) {
            row[j] = (2 * l + 1) as f64 / (2 * n) as f64;
        }
    }
    x
}

/// Two distinct random rows and a random column.
fn random_swap(n: usize, k: usize, rng: &mut Rng) -> (usize, usize, usize) {
    let j = rng.below(k);
    let a = rng.below(n);
    let mut b = rng.below(n - 1);
    if b >= a {
        b += 1;
    }
    (a, b, j)
}

/// Swap budget for the lattice searches: about 3·10⁶ distance updates.
fn swap_budget(n: usize) -> usize {
    (3_000_000 / n).clamp(2_000, 100_000)
}

/// Simulated annealing of a Latin hypercube on φ_p. Returns the best design
/// and the number of proposals made.
fn anneal_phi_p(n: usize, k: usize, p: f64, rng: &mut Rng) -> (Vec<Vec<f64>>, usize) {
    let mut x = random_u_type(n, k, rng);
    if k == 1 || n == 2 {
        // Every Latin hypercube has the same points (k = 1) or distances (n = 2).
        return (x, 0);
    }
    // Squared distances scaled by s² = n²/k, so every one is at least 1 and
    // d^−p stays within floating-point range.
    let s2 = (n * n) as f64 / k as f64;
    let q = p / 2.0;
    let mut d2 = vec![0.0; n * n];
    let mut sum = 0.0;
    for a in 0..n {
        for b in a + 1..n {
            let v = s2 * sq_dist(&x[a], &x[b]);
            d2[a * n + b] = v;
            d2[b * n + a] = v;
            sum += v.powf(-q);
        }
    }
    // Change in Σ d^−p from swapping column j of rows a and b.
    let delta = |x: &[Vec<f64>], d2: &[f64], a: usize, b: usize, j: usize| -> f64 {
        let (xa, xb) = (x[a][j], x[b][j]);
        let mut change = 0.0;
        for (m, row) in x.iter().enumerate() {
            if m == a || m == b {
                continue;
            }
            let shift = s2 * ((xb - row[j]).powi(2) - (xa - row[j]).powi(2));
            let (old_a, old_b) = (d2[a * n + m], d2[b * n + m]);
            change += (old_a + shift).powf(-q) - old_a.powf(-q) + (old_b - shift).powf(-q) - old_b.powf(-q);
        }
        change
    };
    let apply = |x: &mut [Vec<f64>], d2: &mut [f64], a: usize, b: usize, j: usize| {
        let (xa, xb) = (x[a][j], x[b][j]);
        for m in 0..n {
            if m == a || m == b {
                continue;
            }
            let shift = s2 * ((xb - x[m][j]).powi(2) - (xa - x[m][j]).powi(2));
            d2[a * n + m] += shift;
            d2[m * n + a] += shift;
            d2[b * n + m] -= shift;
            d2[m * n + b] -= shift;
        }
        x[a][j] = xb;
        x[b][j] = xa;
    };
    // Starting temperature on the scale of ln φ_p = (1/p) ln Σ d^−p.
    let log_change = |sum: f64, change: f64| ((sum + change).max(f64::MIN_POSITIVE) / sum).ln() / p;
    let mut worse = Vec::new();
    for _ in 0..100 {
        let (a, b, j) = random_swap(n, k, rng);
        let l = log_change(sum, delta(&x, &d2, a, b, j));
        if l > 0.0 {
            worse.push(l);
        }
    }
    let mean_worse = if worse.is_empty() { 1e-3 } else { worse.iter().sum::<f64>() / worse.len() as f64 };
    let levels = 100;
    let budget = swap_budget(n);
    let per_level = (budget / levels).max(1);
    let cooling = 1e-3_f64.powf(1.0 / (levels - 1) as f64);
    let mut temperature = mean_worse / std::f64::consts::LN_2;
    let (mut best, mut best_sum) = (x.clone(), sum);
    for _ in 0..levels {
        for _ in 0..per_level {
            let (a, b, j) = random_swap(n, k, rng);
            let change = delta(&x, &d2, a, b, j);
            let l = log_change(sum, change);
            if l <= 0.0 || rng.next_f64() < (-l / temperature).exp() {
                apply(&mut x, &mut d2, a, b, j);
                sum += change;
                if sum < best_sum {
                    best_sum = sum;
                    best.clone_from(&x);
                }
            }
        }
        temperature *= cooling;
    }
    (best, levels * per_level)
}

/// Threshold accepting of a U-type design on CD². Returns the best design and
/// the number of proposals made.
fn threshold_accept_cd(n: usize, k: usize, rng: &mut Rng) -> (Vec<Vec<f64>>, usize) {
    let mut x = random_u_type(n, k, rng);
    if k == 1 {
        // The only U-type design in one factor (up to order).
        return (x, 0);
    }
    // g_i = Π_j cd_single(z_ij); h_il = Π_j cd_pair(x_ij, x_lj).
    let mut g: Vec<f64> = x.iter().map(|r| r.iter().map(|&u| cd_single(u - 0.5)).product()).collect();
    let mut h = vec![0.0; n * n];
    for a in 0..n {
        for b in 0..n {
            h[a * n + b] = x[a].iter().zip(&x[b]).map(|(&u, &v)| cd_pair(u, v)).product();
        }
    }
    let nf = n as f64;
    let total = |g: &[f64], h: &[f64]| {
        (13.0_f64 / 12.0).powi(k as i32) - 2.0 / nf * g.iter().sum::<f64>() + h.iter().sum::<f64>() / (nf * nf)
    };
    let mut value = total(&g, &h);
    // Change in CD² from swapping column j of rows a and b (h_ab is unchanged).
    let delta = |x: &[Vec<f64>], g: &[f64], h: &[f64], a: usize, b: usize, j: usize| -> f64 {
        let (ua, ub) = (x[a][j], x[b][j]);
        let (sa, sb) = (cd_single(ua - 0.5), cd_single(ub - 0.5));
        let dg = g[a] * (sb / sa - 1.0) + g[b] * (sa / sb - 1.0);
        let (pa, pb) = (cd_pair(ua, ua), cd_pair(ub, ub));
        let mut dh = h[a * n + a] * (pb / pa - 1.0) + h[b * n + b] * (pa / pb - 1.0);
        for (m, row) in x.iter().enumerate() {
            if m == a || m == b {
                continue;
            }
            let (fa, fb) = (cd_pair(ua, row[j]), cd_pair(ub, row[j]));
            dh += 2.0 * (h[a * n + m] * (fb / fa - 1.0) + h[b * n + m] * (fa / fb - 1.0));
        }
        -2.0 / nf * dg + dh / (nf * nf)
    };
    let apply = |x: &mut [Vec<f64>], g: &mut [f64], h: &mut [f64], a: usize, b: usize, j: usize| {
        let (ua, ub) = (x[a][j], x[b][j]);
        let (sa, sb) = (cd_single(ua - 0.5), cd_single(ub - 0.5));
        g[a] *= sb / sa;
        g[b] *= sa / sb;
        let (pa, pb) = (cd_pair(ua, ua), cd_pair(ub, ub));
        h[a * n + a] *= pb / pa;
        h[b * n + b] *= pa / pb;
        for m in 0..n {
            if m == a || m == b {
                continue;
            }
            let (fa, fb) = (cd_pair(ua, x[m][j]), cd_pair(ub, x[m][j]));
            h[a * n + m] *= fb / fa;
            h[m * n + a] *= fb / fa;
            h[b * n + m] *= fa / fb;
            h[m * n + b] *= fa / fb;
        }
        x[a][j] = ub;
        x[b][j] = ua;
    };
    let mut sampled: Vec<f64> = (0..200)
        .map(|_| {
            let (a, b, j) = random_swap(n, k, rng);
            delta(&x, &g, &h, a, b, j).abs()
        })
        .filter(|d| *d > 0.0)
        .collect();
    sampled.sort_by(f64::total_cmp);
    let start = sampled.get(sampled.len() / 10).copied().unwrap_or(0.0);
    let levels = 50;
    let per_level = (swap_budget(n) / levels).max(1);
    let (mut best, mut best_value) = (x.clone(), value);
    for level in 0..levels {
        let threshold = start * (levels - 1 - level) as f64 / (levels - 1) as f64;
        for _ in 0..per_level {
            let (a, b, j) = random_swap(n, k, rng);
            let change = delta(&x, &g, &h, a, b, j);
            if change <= threshold {
                apply(&mut x, &mut g, &mut h, a, b, j);
                value += change;
                if value < best_value - 1e-15 {
                    best_value = value;
                    best.clone_from(&x);
                }
            }
        }
    }
    (best, levels * per_level)
}

/// Candidates for the maximin exchange: grids with 2, 3, … levels per
/// factor while they fit in half the budget, then uniform random points.
fn maximin_candidates(n: usize, k: usize, rng: &mut Rng) -> Vec<Vec<f64>> {
    // About 4·10⁶ distance terms per exchange step.
    let total = (4_000_000 / (n * k)).clamp(4 * n, 5000).max(4 * n);
    let mut c = Vec::with_capacity(total);
    let mut m = 2usize;
    while let Some(size) = m.checked_pow(k as u32) {
        if c.len() + size > total / 2 {
            break;
        }
        c.extend(grid(k, m));
        m += 1;
    }
    while c.len() < total {
        c.push((0..k).map(|_| rng.next_f64()).collect());
    }
    c
}

/// Lexicographic maximin value of a distance matrix: (smallest squared
/// distance, number of pairs within a relative 10⁻¹⁰ of it, a closest pair).
fn closest(d2: &[f64], n: usize) -> (f64, usize, (usize, usize)) {
    let mut best = (f64::INFINITY, 0, (0, 1));
    for a in 0..n {
        for b in a + 1..n {
            let v = d2[a * n + b];
            if v < best.0 * (1.0 - 1e-10) {
                best = (v, 1, (a, b));
            } else if v <= best.0 * (1.0 + 1e-10) {
                best.1 += 1;
            }
        }
    }
    best
}

fn better(new: (f64, usize), old: (f64, usize)) -> bool {
    new.0 > old.0 * (1.0 + 1e-10) || (new.0 >= old.0 * (1.0 - 1e-10) && new.1 < old.1)
}

/// Maximin points by greedy selection, exchange and refinement; also the
/// number of candidates.
fn maximin_points(n: usize, k: usize, rng: &mut Rng) -> (Vec<Vec<f64>>, usize) {
    let candidates = maximin_candidates(n, k, rng);
    let nc = candidates.len();
    // Greedy farthest-point selection from a random candidate.
    let mut chosen = vec![rng.below(nc)];
    let mut nearest: Vec<f64> = candidates.iter().map(|c| sq_dist(c, &candidates[chosen[0]])).collect();
    while chosen.len() < n {
        let next = (0..nc).max_by(|&a, &b| nearest[a].total_cmp(&nearest[b])).unwrap_or(0);
        chosen.push(next);
        for (m, c) in candidates.iter().enumerate() {
            nearest[m] = nearest[m].min(sq_dist(c, &candidates[next]));
        }
    }
    let mut x: Vec<Vec<f64>> = chosen.iter().map(|&i| candidates[i].clone()).collect();
    let mut d2 = vec![0.0; n * n];
    for a in 0..n {
        for b in a + 1..n {
            let v = sq_dist(&x[a], &x[b]);
            d2[a * n + b] = v;
            d2[b * n + a] = v;
        }
    }
    // Exchange a point of the closest pair for the candidate farthest from the rest.
    for _ in 0..20 * n {
        let (dmin, count, (a, b)) = closest(&d2, n);
        let mut improved = false;
        for i in [a, b] {
            let mut best = (f64::NEG_INFINITY, 0);
            for (ci, c) in candidates.iter().enumerate() {
                let mut m = f64::INFINITY;
                for (j, xj) in x.iter().enumerate() {
                    if j != i {
                        m = m.min(sq_dist(c, xj));
                        if m <= best.0 {
                            break;
                        }
                    }
                }
                if m > best.0 {
                    best = (m, ci);
                }
            }
            if best.0 < dmin * (1.0 - 1e-10) {
                continue;
            }
            let old = std::mem::replace(&mut x[i], candidates[best.1].clone());
            let saved: Vec<f64> = (0..n).map(|j| d2[i * n + j]).collect();
            for j in 0..n {
                if j != i {
                    let v = sq_dist(&x[i], &x[j]);
                    d2[i * n + j] = v;
                    d2[j * n + i] = v;
                }
            }
            let (new_min, new_count, _) = closest(&d2, n);
            if better((new_min, new_count), (dmin, count)) {
                improved = true;
                break;
            }
            x[i] = old;
            for j in 0..n {
                d2[i * n + j] = saved[j];
                d2[j * n + i] = saved[j];
            }
        }
        if !improved {
            break;
        }
    }
    let refined = refine_maximin(&x);
    if min_distance(&refined) > min_distance(&x) * (1.0 + 1e-12) {
        x = refined;
    }
    (x, nc)
}

/// Projected gradient descent on `ln φ_p` with rising `p`, returning the
/// iterate with the largest minimum distance.
fn refine_maximin(start: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let n = start.len();
    let k = start[0].len();
    // About 4·10⁷ pairwise terms in all.
    let steps_total = (40_000_000 / (n * n * k)).min(4000);
    let schedule = [20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0];
    let per_p = steps_total / schedule.len();
    if per_p == 0 {
        return start.to_vec();
    }
    let mut x = start.to_vec();
    let mut best = (min_distance(&x), x.clone());
    // ln φ_p and its gradient with respect to every coordinate.
    let objective = |x: &[Vec<f64>], p: f64, grad: Option<&mut Vec<Vec<f64>>>| -> f64 {
        let dmin = min_distance(x);
        if dmin == 0.0 {
            return f64::INFINITY;
        }
        let mut sum = 0.0;
        let mut terms = Vec::with_capacity(n * (n - 1) / 2);
        for a in 0..n {
            for b in a + 1..n {
                let d = sq_dist(&x[a], &x[b]).sqrt();
                let w = (dmin / d).powf(p);
                sum += w;
                terms.push((a, b, w, d));
            }
        }
        if let Some(grad) = grad {
            for row in grad.iter_mut() {
                row.iter_mut().for_each(|v| *v = 0.0);
            }
            // ∂ ln φ_p / ∂x_a = −Σ_b w_ab (x_a − x_b)/d_ab², w normalized.
            for (a, b, w, d) in terms {
                let c = w / sum / (d * d);
                for j in 0..k {
                    let g = c * (x[a][j] - x[b][j]);
                    grad[a][j] -= g;
                    grad[b][j] += g;
                }
            }
        }
        -dmin.ln() + sum.ln() / p
    };
    let mut grad = vec![vec![0.0; k]; n];
    for p in schedule {
        let mut f = objective(&x, p, Some(&mut grad));
        let mut step = 0.01 * best.0 * best.0;
        for _ in 0..per_p {
            let trial: Vec<Vec<f64>> = x
                .iter()
                .zip(&grad)
                .map(|(row, g)| row.iter().zip(g).map(|(v, gv)| (v - step * gv).clamp(0.0, 1.0)).collect())
                .collect();
            let ft = objective(&trial, p, None);
            if ft < f {
                x = trial;
                f = objective(&x, p, Some(&mut grad));
                step *= 1.5;
                let d = min_distance(&x);
                if d > best.0 {
                    best = (d, x.clone());
                }
            } else {
                step *= 0.5;
                if step < 1e-14 {
                    break;
                }
            }
        }
    }
    best.1
}

/// Minimax points for `region` by k-means followed by minimum-enclosing-ball
/// centres of the clusters.
fn minimax_points(n: usize, region: &[Vec<f64>], rng: &mut Rng) -> Vec<Vec<f64>> {
    let k = region[0].len();
    let work = region.len() * n * k;
    // About 2·10⁸ distance terms in all.
    let rounds = (200_000_000 / work).clamp(2, 120);
    let starts = (rounds / 30).clamp(1, 4);
    let per_start = rounds / starts;
    let lloyd = (per_start / 3).max(1);
    let mut best: Option<(f64, Vec<Vec<f64>>)> = None;
    let mut assign = vec![0usize; region.len()];
    for _ in 0..starts {
        let mut centres = kmeans_plus_plus(n, region, rng);
        // Lloyd iterations (cluster means).
        for _ in 0..lloyd {
            assign_points(region, &centres, &mut assign);
            let mut sums = vec![vec![0.0; k]; n];
            let mut counts = vec![0usize; n];
            for (p, &c) in region.iter().zip(&assign) {
                counts[c] += 1;
                for j in 0..k {
                    sums[c][j] += p[j];
                }
            }
            for c in 0..n {
                if counts[c] > 0 {
                    centres[c] = sums[c].iter().map(|s| s / counts[c] as f64).collect();
                }
            }
        }
        // Minimax iterations (enclosing-ball centres).
        let mut stale = 0;
        for _ in lloyd..per_start {
            let fill = assign_points(region, &centres, &mut assign);
            if best.as_ref().is_none_or(|(b, _)| fill < *b - 1e-12) {
                best = Some((fill, centres.clone()));
                stale = 0;
            } else {
                stale += 1;
                if stale >= 3 {
                    break;
                }
            }
            let mut members: Vec<Vec<usize>> = vec![Vec::new(); n];
            for (i, &c) in assign.iter().enumerate() {
                members[c].push(i);
            }
            for (c, m) in members.iter().enumerate() {
                if m.is_empty() {
                    // An empty cluster takes the region point farthest from its centre.
                    let far = (0..region.len())
                        .max_by(|&a, &b| {
                            sq_dist(&region[a], &centres[assign[a]])
                                .total_cmp(&sq_dist(&region[b], &centres[assign[b]]))
                        })
                        .unwrap_or(0);
                    centres[c] = region[far].clone();
                } else {
                    centres[c] = enclosing_ball_centre(region, m);
                }
            }
        }
        let fill = assign_points(region, &centres, &mut assign);
        if best.as_ref().is_none_or(|(b, _)| fill < *b - 1e-12) {
            best = Some((fill, centres));
        }
    }
    best.map(|(_, c)| c).unwrap_or_default()
}

/// Assign each region point to its nearest centre; returns the largest
/// nearest distance (the fill distance over the region).
fn assign_points(region: &[Vec<f64>], centres: &[Vec<f64>], assign: &mut [usize]) -> f64 {
    let mut fill = 0.0_f64;
    for (p, a) in region.iter().zip(assign.iter_mut()) {
        let (mut bd, mut bi) = (f64::INFINITY, 0);
        for (i, c) in centres.iter().enumerate() {
            let d = sq_dist(p, c);
            if d < bd {
                (bd, bi) = (d, i);
            }
        }
        *a = bi;
        fill = fill.max(bd);
    }
    fill.sqrt()
}

/// k-means++ seeding (Arthur & Vassilvitskii 2007): each next centre is a
/// region point drawn with probability proportional to its squared distance
/// from the nearest centre so far.
fn kmeans_plus_plus(n: usize, region: &[Vec<f64>], rng: &mut Rng) -> Vec<Vec<f64>> {
    let mut centres = vec![region[rng.below(region.len())].clone()];
    let mut nearest: Vec<f64> = region.iter().map(|p| sq_dist(p, &centres[0])).collect();
    while centres.len() < n {
        let total: f64 = nearest.iter().sum();
        let mut target = rng.next_f64() * total;
        let mut pick = region.len() - 1;
        for (i, &d) in nearest.iter().enumerate() {
            target -= d;
            if target < 0.0 {
                pick = i;
                break;
            }
        }
        let c = region[pick].clone();
        for (d, p) in nearest.iter_mut().zip(region) {
            *d = d.min(sq_dist(p, &c));
        }
        centres.push(c);
    }
    centres
}

/// Centre of the smallest ball enclosing `region[members]` by Frank–Wolfe on
/// the dual problem with exact line search (Yildirim 2008, Algorithm 1):
/// `c` is a convex combination of the points; each step moves it towards the
/// farthest point by `α = δ/(2(1 + δ))`, `δ = r²/γ − 1`, where `r` is that
/// point's distance and `γ` the dual value, until `δ < 10⁻⁶`.
fn enclosing_ball_centre(region: &[Vec<f64>], members: &[usize]) -> Vec<f64> {
    let k = region[0].len();
    let first = &region[members[0]];
    let far = |c: &[f64]| {
        members
            .iter()
            .map(|&i| (sq_dist(&region[i], c), i))
            .fold((0.0, members[0]), |a, b| if b.0 > a.0 { b } else { a })
    };
    // Start halfway between a point far from the first and the point far from that.
    let (_, a) = far(first);
    let (r0, b) = far(&region[a]);
    let mut c: Vec<f64> = (0..k).map(|j| 0.5 * (region[a][j] + region[b][j])).collect();
    let mut gamma = r0 / 4.0;
    if gamma == 0.0 {
        return c;
    }
    for _ in 0..500 {
        let (r2, i) = far(&c);
        let delta = r2 / gamma - 1.0;
        if delta < 1e-6 {
            break;
        }
        let alpha = delta / (2.0 * (1.0 + delta));
        for (cj, xj) in c.iter_mut().zip(&region[i]) {
            *cj += alpha * (xj - *cj);
        }
        gamma = (1.0 - alpha) * gamma + alpha * (1.0 - alpha) * r2;
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(d: &Design) -> Vec<Vec<f64>> {
        d.runs.iter().map(|r| r.iter().map(|c| (c + 1.0) / 2.0).collect()).collect()
    }

    /// Fixed designs and their centered L2 discrepancies (squared) from
    /// `scipy.stats.qmc.discrepancy(x, method='CD')`, SciPy 1.x; see
    /// `tests/data/space_filling_reference.py`.
    #[test]
    fn centered_discrepancy_matches_scipy() {
        for (points, scipy_cd2) in reference_cd_cases() {
            let cd = centered_l2_discrepancy(&points);
            assert!((cd * cd - scipy_cd2).abs() <= 1e-12, "{cd} vs {scipy_cd2}");
        }
    }

    /// `x_ij = frac((i + 1)·√p_j)` for the j-th prime, as in the script.
    fn frac_design(n: usize, k: usize) -> Vec<Vec<f64>> {
        let primes = [2.0_f64, 3.0, 5.0, 7.0, 11.0, 13.0, 17.0, 19.0];
        (0..n).map(|i| (0..k).map(|j| ((i + 1) as f64 * primes[j].sqrt()).rem_euclid(1.0)).collect()).collect()
    }

    fn reference_cd_cases() -> Vec<(Vec<Vec<f64>>, f64)> {
        let u_type = [[1, 4, 6], [2, 1, 3], [3, 6, 2], [4, 3, 5], [5, 5, 1], [6, 2, 4]]
            .iter()
            .map(|r| r.iter().map(|&l| (f64::from(l) - 0.5) / 6.0).collect())
            .collect();
        vec![
            (vec![vec![0.1, 0.2], vec![0.4, 0.8], vec![0.75, 0.35], vec![0.95, 0.6]], 2.620_720_486_111_061e-2),
            (u_type, 2.256_539_776_055_977e-2),
            (vec![vec![0.05], vec![0.5], vec![0.9]], 1.527_777_777_777_75e-2),
            (frac_design(15, 5), 3.098_875_073_860_33e-2),
            (frac_design(40, 8), 4.535_002_347_269_29e-2),
        ]
    }

    fn is_permutation_of_levels(points: &[Vec<f64>]) -> bool {
        let n = points.len();
        (0..points[0].len()).all(|j| {
            let mut levels: Vec<usize> = points.iter().map(|p| (p[j] * n as f64 - 0.5).round() as usize).collect();
            let exact = points.iter().zip(&levels).all(|(p, &l)| (p[j] - (l as f64 + 0.5) / n as f64).abs() < 1e-12);
            levels.sort_unstable();
            exact && levels == (0..n).collect::<Vec<_>>()
        })
    }

    /// The uniform design is at least as uniform as SciPy's
    /// `LatinHypercube(optimization="random-cd")` (best of seeds 0–4, from the
    /// reference script).
    #[test]
    fn uniform_design_beats_scipy_random_cd() {
        for (n, k, scipy_best) in [(10, 2, 0.054_525), (20, 4, 0.073_316), (30, 3, 0.034_118), (40, 8, 0.147_759)] {
            let d = uniform_design(n, k, 1).unwrap();
            let x = unit(&d);
            assert!(is_permutation_of_levels(&x), "U-type");
            let cd = centered_l2_discrepancy(&x);
            assert!(cd <= scipy_best + 5e-7, "n={n} k={k}: {cd} vs SciPy {scipy_best}");
            assert!(d.notes.iter().any(|s| s.contains(&format!("{cd:.6}"))), "{:?}", d.notes);
        }
    }

    #[test]
    fn latin_hypercube_is_a_lattice_and_beats_random() {
        for (n, k) in [(20, 4), (12, 2), (50, 6)] {
            let d = maximin_latin_hypercube(n, k, 3).unwrap();
            let x = unit(&d);
            assert!(is_permutation_of_levels(&x));
            let phi = phi_p(&x, DEFAULT_P);
            // Better than every one of 50 random Latin hypercubes.
            let mut rng = Rng::new(99);
            let best_random =
                (0..50).map(|_| phi_p(&random_u_type(n, k, &mut rng), DEFAULT_P)).fold(f64::INFINITY, f64::min);
            assert!(phi < best_random, "n={n} k={k}: {phi} vs random {best_random}");
            assert_eq!(d.runs, maximin_latin_hypercube(n, k, 3).unwrap().runs, "reproducible");
        }
        // 1 factor: the levels themselves.
        let x = unit(&maximin_latin_hypercube(5, 1, 1).unwrap());
        assert!(is_permutation_of_levels(&x));
    }

    /// Maximin packings of the unit square: n = 4, 5 and 9 are exact (Croft,
    /// Falconer & Guy, *Unsolved Problems in Geometry*, D1; Specht's tables of
    /// circle packings in a square give the separation m_n = 1, √2/2, 0.5);
    /// n = 6 has m₆ = √13/6 ≈ 0.600925 (Graham et al. 1998), reached here to
    /// within 0.2%.
    #[test]
    fn maximin_reaches_known_square_packings() {
        for (n, best, tol) in
            [(4, 1.0, 1e-9), (5, 0.5_f64.sqrt(), 1e-9), (9, 0.5, 1e-9), (6, 13.0_f64.sqrt() / 6.0, 1.2e-3)]
        {
            let x = unit(&maximin_distance(n, 2, 1).unwrap());
            let d = min_distance(&x);
            assert!(d <= best + 1e-9, "n={n}: {d} exceeds the optimum {best}");
            assert!(d >= best - tol, "n={n}: {d} vs {best}");
        }
        // Two points in a cube: opposite corners.
        let x = unit(&maximin_distance(2, 3, 5).unwrap());
        assert!((min_distance(&x) - 3.0_f64.sqrt()).abs() < 1e-9);
    }

    /// Fill distance of minimax designs with known optima: one point (the
    /// centre, h = √2/2) and four points (the quarter points, h = √2/4).
    #[test]
    fn minimax_reaches_known_optima() {
        let one = |n: usize| {
            let d = minimax_distance(n, 2, 1).unwrap();
            let x = unit(&d);
            (fill_distance(&x, &evaluation_points(2, n)), x)
        };
        let (h, x) = one(2);
        // Two points: the optimum is √(1/4 + 1/16) = √5/4 (points at (½, ¼) and (½, ¾)).
        assert!((h - 5.0_f64.sqrt() / 4.0).abs() < 5e-3, "{h} {x:?}");
        let (h, x) = one(4);
        assert!((h - 2.0_f64.sqrt() / 4.0).abs() < 5e-3, "{h} {x:?}");
        for p in &x {
            assert!(p.iter().all(|&v| (v - 0.25).abs() < 0.02 || (v - 0.75).abs() < 0.02), "{x:?}");
        }
        // One point: the centre (a design needs 2 runs, so check the routine directly).
        let region = evaluation_points(2, 2);
        let mut rng = Rng::new(1);
        let c = minimax_points(1, &region, &mut rng);
        assert!((fill_distance(&c, &region) - 0.5_f64.sqrt()).abs() < 1e-6, "{c:?}");
    }

    #[test]
    fn every_kind_builds_coded_designs_with_notes() {
        for kind in SpaceFillingKind::ALL {
            let d = space_filling_design(kind, 20, 4, 7).unwrap();
            assert_eq!((d.n_runs(), d.n_factors()), (20, 4), "{kind:?}");
            assert!(d.runs.iter().flatten().all(|v| (-1.0..=1.0).contains(v)));
            assert!(d.point_types.iter().all(|t| *t == PointType::Other));
            assert!(d.notes.iter().any(|n| n.starts_with("Fill distance")));
            assert!(min_distance(&unit(&d)) > 0.0, "no repeated runs");
            assert_eq!(d, space_filling_design(kind, 20, 4, 7).unwrap(), "seeded");
        }
        assert!(space_filling_design(SpaceFillingKind::Uniform, 1, 2, 1).is_err());
    }

    #[test]
    fn evaluation_points_include_corners() {
        let e = evaluation_points(2, 10);
        assert_eq!(e.len(), 4096);
        assert!(e.contains(&vec![1.0, 1.0]) && e.contains(&vec![0.0, 0.0]) && e.contains(&vec![0.5, 0.5]));
        assert_eq!(evaluation_points(20, 1000).len(), 10_000);
    }

    #[test]
    fn helpers_on_hand_examples() {
        let corners = vec![vec![0.0, 0.0], vec![1.0, 0.0], vec![0.0, 1.0], vec![1.0, 1.0]];
        assert!((min_distance(&corners) - 1.0).abs() < 1e-15);
        // Four pairs at distance 1 and two at √2.
        let phi = (4.0 + 2.0 * 2.0_f64.powf(-25.0)).powf(1.0 / 50.0);
        assert!((phi_p(&corners, 50.0) - phi).abs() < 1e-14);
        // The centre of the square is √2/2 from each corner.
        let centre = vec![vec![0.5, 0.5]];
        assert!((fill_distance(&centre, &corners) - 0.5_f64.sqrt()).abs() < 1e-15);
        assert_eq!(phi_p(&[vec![0.2], vec![0.2]], 50.0), f64::INFINITY);
        assert!(validate(1, 2).is_err() && validate(2, 0).is_err());
        assert!(validate(1001, 2).is_err() && validate(10, 21).is_err());
    }
}
