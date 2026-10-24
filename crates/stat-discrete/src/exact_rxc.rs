//! Fisher's exact test of independence for `r × c` tables (the
//! Freeman–Halton extension of Fisher's test), computed with the network
//! algorithm of Mehta & Patel (1983), with a seeded Monte Carlo estimate when
//! the exact computation would be too large.
//!
//! # Method
//! Conditional on the row totals `Rᵢ` and column totals `Cⱼ`, a table with
//! cells `nᵢⱼ` has the multiple hypergeometric probability
//!
//! ```text
//! P(n) = Πᵢ Rᵢ! Πⱼ Cⱼ! / (N! Πᵢⱼ nᵢⱼ!)
//! ```
//!
//! and the two-sided p-value is the total probability of every table with
//! the same margins whose probability does not exceed the observed table's,
//! `p = Σ { P(t) : P(t) ≤ P(n_obs) }` (with a relative tolerance of 10⁻⁷ on
//! the comparison, so that tables equal to the observed one in exact
//! arithmetic count).
//!
//! **Network algorithm** (Mehta & Patel 1983, *JASA* 78, 427–434). Tables are
//! built one column at a time. After `k` columns the only thing that matters
//! for the remaining columns is the multiset of remaining row totals, so the
//! partial tables collapse onto *nodes* (the remaining row totals, sorted).
//! Each node keeps the distinct "past" values `Σ ln nᵢⱼ!` of the paths that
//! reach it, with their multiplicities. Before a past value is stored, the
//! node's bounds on the remaining part of `Σ ln nᵢⱼ!` are checked:
//!
//! * if even the most probable completion is no more probable than the
//!   observed table, every completion counts and the node's whole remaining
//!   mass — known in closed form, `N′! / (Π R′ᵢ! Π C′ⱼ!)` — is added at once;
//! * if even the least probable completion is more probable than the
//!   observed table, no completion counts and the path is dropped;
//! * otherwise the path is kept and expanded at the next column.
//!
//! The bounds come from two relaxations, each solved exactly by majorization
//! (`ln x!` is convex): dropping the row-total constraints (each remaining
//! column split as evenly / as unevenly as the row totals allow) and dropping
//! the column-total constraints (each row split across the remaining
//! columns); the tighter of the two is used. Mehta & Patel's own bounds are
//! sharper but these are valid, which is all exactness needs.
//!
//! **Monte Carlo** (used when the network exceeds the work budget, or when
//! asked): tables with the observed margins are drawn from their null
//! distribution (the multiple hypergeometric) by Patefield's (1981)
//! algorithm (*Applied Statistics* 30, 91–97, AS 159): row by row, each
//! row's counts are a draw without replacement from the column totals still
//! unallocated, so each cell given the cells before it is hypergeometric —
//! `nᵢⱼ ~ H(population Σₖ≥ⱼ C′ₖ, successes C′ⱼ, draws R′ᵢ)` with `C′` and `R′`
//! what remains — and is drawn by inversion searching outward from the mode
//! when its variance is small, otherwise by Stadlober's (1989) ratio-of-uniforms
//! rejection method. The work per table grows with the number of cells, not
//! with `N`; `ln n!` is evaluated on demand, and a random table is compared
//! with the observed one through cell-by-cell differences `ln x! − ln o!`
//! (from Stirling's series for large counts), which stay accurate when
//! `Σ ln n!` itself is beyond `f64` precision. The estimate is
//! `(b + 1)/(B + 1)` where `b` of the `B` tables are no more probable than
//! the observed one. Totals up to 2⁵³ are accepted.
//!
//! The 2 × 2 special case is the two-sided test in
//! `stat_inference::proportions` (the same definition of "as or more
//! extreme"); the two agree, which the tests check.

use std::collections::HashMap;
use std::sync::OnceLock;

use stat_core::rng::Rng;
use stat_core::special::ln_gamma;
use stat_core::{Result, StatError};

/// Relative tolerance when comparing a table's probability with the observed
/// table's (on the log scale, an absolute tolerance of about 10⁻⁷).
const TOLERANCE: f64 = 1e-7;

/// Default bound on the network algorithm's work (arcs examined plus past
/// values stored) before it gives way to Monte Carlo.
pub const DEFAULT_WORK_LIMIT: usize = 2_000_000;

/// Largest total count accepted, 2⁵³ (counts above it are not exact in
/// `f64`).
pub const MAX_TOTAL: u64 = 1 << 53;

/// Settings of [`fisher_exact_rxc`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExactOptions {
    /// Largest amount of network work (arcs examined plus past values kept)
    /// before the test switches to Monte Carlo. 0 forces Monte Carlo.
    pub work_limit: usize,
    /// Number of random tables of the Monte Carlo estimate.
    pub monte_carlo_tables: usize,
    /// Seed of the Monte Carlo estimate.
    pub seed: u64,
}

impl Default for ExactOptions {
    fn default() -> Self {
        Self { work_limit: DEFAULT_WORK_LIMIT, monte_carlo_tables: 10_000, seed: 1 }
    }
}

/// How an exact p-value was obtained.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExactMethod {
    /// Exactly, by the network algorithm.
    Network,
    /// Estimated from random tables.
    MonteCarlo {
        /// Number of random tables drawn.
        tables: usize,
        /// Seed of the generator.
        seed: u64,
        /// Binomial standard error of the estimate, `√(p(1 − p)/B)`.
        standard_error: f64,
    },
}

/// Result of Fisher's exact test of an `r × c` table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExactTest {
    /// Two-sided p-value: the probability, given the margins, of a table no
    /// more probable than the observed one.
    pub p_value: f64,
    /// Probability of the observed table given its margins.
    pub table_probability: f64,
    /// Number of non-empty rows and columns used.
    pub dimensions: (usize, usize),
    /// Exact or Monte Carlo.
    pub method: ExactMethod,
}

impl ExactTest {
    /// `true` when the p-value is a Monte Carlo estimate.
    #[must_use]
    pub fn is_estimate(&self) -> bool {
        matches!(self.method, ExactMethod::MonteCarlo { .. })
    }
}

/// Size of the cached table of `ln n!`; larger arguments are computed on
/// demand.
const CACHED_FACTORIALS: usize = 4096;

/// `ln n!` (from a small cached table, otherwise from `ln Γ(n + 1)`).
fn ln_fact(n: u64) -> f64 {
    static TABLE: OnceLock<Vec<f64>> = OnceLock::new();
    let table = TABLE.get_or_init(|| (0..CACHED_FACTORIALS as u64).map(ln_fact_direct).collect());
    usize::try_from(n).ok().and_then(|i| table.get(i)).copied().unwrap_or_else(|| ln_fact_direct(n))
}

fn ln_fact_direct(n: u64) -> f64 {
    if n < 2 { 0.0 } else { ln_gamma(n as f64 + 1.0) }
}

/// Fisher's exact test of independence for a table of counts (rows of equal
/// length). Empty rows and columns are dropped; a table left with fewer than
/// two rows or columns has only one arrangement, so its p-value is 1.
///
/// # Errors
/// [`StatError::InvalidArgument`] for a ragged or empty table, or a total
/// count too large to represent.
pub fn fisher_exact_rxc(counts: &[Vec<u64>], options: &ExactOptions) -> Result<ExactTest> {
    let cols = counts.first().map_or(0, Vec::len);
    if counts.is_empty() || cols == 0 || counts.iter().any(|r| r.len() != cols) {
        return Err(StatError::InvalidArgument { message: "the table must be a non-empty grid of counts".into() });
    }
    // Saturating sums: a total that reaches u64::MAX fails the 2⁵³ check below
    // instead of wrapping round to a small count.
    let row_tot: Vec<u64> = counts.iter().map(|r| r.iter().fold(0, |a: u64, &c| a.saturating_add(c))).collect();
    let col_tot: Vec<u64> = (0..cols).map(|j| counts.iter().fold(0, |a: u64, r| a.saturating_add(r[j]))).collect();
    let keep_r: Vec<usize> = (0..counts.len()).filter(|&i| row_tot[i] > 0).collect();
    let keep_c: Vec<usize> = (0..cols).filter(|&j| col_tot[j] > 0).collect();
    let mut table: Vec<Vec<u64>> = keep_r.iter().map(|&i| keep_c.iter().map(|&j| counts[i][j]).collect()).collect();
    let dimensions = (keep_r.len(), keep_c.len());
    if keep_r.len() < 2 || keep_c.len() < 2 {
        return Ok(ExactTest { p_value: 1.0, table_probability: 1.0, dimensions, method: ExactMethod::Network });
    }
    // Counts are handled as f64 in the probabilities: beyond 2⁵³ they are no
    // longer exact.
    let n = row_tot.iter().try_fold(0u64, |a, &r| a.checked_add(r)).filter(|&n| n <= MAX_TOTAL).ok_or_else(|| {
        StatError::InvalidArgument {
            message: format!("the total count exceeds {MAX_TOTAL} (2⁵³), too large for an exact test"),
        }
    })?;
    // Fewer rows make smaller nodes: put the smaller dimension on the rows.
    if table.len() > table[0].len() {
        table = (0..table[0].len()).map(|j| table.iter().map(|r| r[j]).collect()).collect();
    }
    let rows: Vec<u64> = table.iter().map(|r| r.iter().sum()).collect();
    let mut columns: Vec<u64> = (0..table[0].len()).map(|j| table.iter().map(|r| r[j]).sum()).collect();
    let observed: f64 = table.iter().flatten().map(|&x| ln_fact(x)).sum();
    // K = ln(Π R! Π C! / N!): P(table) = exp(K − Σ ln n!).
    let k =
        rows.iter().map(|&r| ln_fact(r)).sum::<f64>() + columns.iter().map(|&c| ln_fact(c)).sum::<f64>() - ln_fact(n);
    let table_probability = (k - observed).exp().min(1.0);
    // The Monte Carlo comparison works cell by cell against the observed
    // table, in its own column order.
    let observed_cells: Vec<u64> = table.iter().flatten().copied().collect();
    let observed_columns = columns.clone();
    // Process the largest columns first: the network is narrower.
    columns.sort_unstable_by(|a, b| b.cmp(a));
    // With huge counts the rounding of Σ ln n! itself can exceed TOLERANCE.
    let threshold = observed - TOLERANCE.max(1e-14 * observed.abs());
    if options.work_limit > 0 {
        if let Some(p) = Network::new(&rows, &columns, threshold, options.work_limit).run(k) {
            return Ok(ExactTest {
                p_value: p.clamp(0.0, 1.0),
                table_probability,
                dimensions,
                method: ExactMethod::Network,
            });
        }
    }
    let (p, se) =
        monte_carlo(&rows, &observed_columns, &observed_cells, options.monte_carlo_tables.max(1), options.seed);
    Ok(ExactTest {
        p_value: p,
        table_probability,
        dimensions,
        method: ExactMethod::MonteCarlo {
            tables: options.monte_carlo_tables.max(1),
            seed: options.seed,
            standard_error: se,
        },
    })
}

/// Monte Carlo estimate of the p-value and its standard error, from tables
/// drawn by Patefield's algorithm. A random table is no more probable than
/// the observed one (row-major `observed`) when
/// `Σ (ln xᵢⱼ! − ln oᵢⱼ!) ≥ −TOLERANCE`; each difference is computed
/// directly ([`ln_fact_ratio`]), so the comparison keeps its precision even
/// when `Σ ln n!` itself is far larger than 1/ε.
fn monte_carlo(rows: &[u64], columns: &[u64], observed: &[u64], tables: usize, seed: u64) -> (f64, f64) {
    let mut rng = Rng::new(seed);
    let mut table = vec![0u64; rows.len() * columns.len()];
    let mut hits = 0usize;
    for _ in 0..tables {
        random_table(rows, columns, &mut rng, &mut table);
        let gap: f64 = table.iter().zip(observed).map(|(&x, &o)| ln_fact_ratio(x, o)).sum();
        hits += usize::from(gap >= -TOLERANCE);
    }
    let p = (hits as f64 + 1.0) / (tables as f64 + 1.0);
    (p, (p * (1.0 - p) / tables as f64).sqrt())
}

/// A random table with row totals `rows` and column totals `columns` (equal
/// sums) from the multiple hypergeometric distribution, written row-major
/// into `table` (Patefield 1981, AS 159). Row `i` takes its `Rᵢ` counts
/// without replacement from the column totals left by the rows before it;
/// the last row takes what is left.
fn random_table(rows: &[u64], columns: &[u64], rng: &mut Rng, table: &mut [u64]) {
    let c = columns.len();
    let mut left: Vec<u64> = columns.to_vec();
    let mut pool: u64 = columns.iter().sum();
    for (i, &r) in rows.iter().enumerate() {
        let out = &mut table[i * c..(i + 1) * c];
        if i + 1 == rows.len() {
            out.copy_from_slice(&left);
            break;
        }
        // `draws` of row i still to place among columns j.., whose remaining
        // totals add to `population`.
        let (mut draws, mut population) = (r, pool);
        for j in 0..c {
            let x = if j + 1 == c { draws } else { hypergeometric(population, left[j], draws, rng) };
            out[j] = x;
            draws -= x;
            population -= left[j];
            left[j] -= x;
        }
        pool -= r;
    }
}

/// `ln a! − ln b!`, accurate in absolute terms even when both are huge: for
/// `a, b ≥ 10⁴` from the difference of Stirling's series,
/// `d ln b + a ln(1 + d/b) − d + ½ ln(1 + d/b) + (1/12)(1/a − 1/b)
/// − (1/360)(1/a³ − 1/b³)` with `d = a − b` (the next term is below 10⁻²³).
fn ln_fact_ratio(a: u64, b: u64) -> f64 {
    if a == b {
        return 0.0;
    }
    if a.min(b) < 10_000 {
        return ln_fact(a) - ln_fact(b);
    }
    let (af, bf) = (a as f64, b as f64);
    let d = if a > b { (a - b) as f64 } else { -((b - a) as f64) };
    let r = (d / bf).ln_1p();
    d * bf.ln() + af * r - d + 0.5 * r + (1.0 / af - 1.0 / bf) / 12.0 - (af.powi(-3) - bf.powi(-3)) / 360.0
}

/// `ln C(n, j)`, through the smaller of `j` and `n − j`.
fn ln_choose(n: u64, j: u64) -> f64 {
    let j = j.min(n - j);
    ln_fact_ratio(n, n - j) - ln_fact(j)
}

/// Variance above which [`hypergeometric`] samples by rejection instead of
/// by searching outward from the mode.
const SEARCH_VARIANCE: f64 = 100.0;

/// A draw from the hypergeometric distribution: the number of successes in
/// `draws` taken without replacement from `population` items of which
/// `successes` are successes.
///
/// With a variance up to 100: inversion, the uniform's mass used up by
/// visiting the support outward from the mode, alternately above and below
/// (the probabilities follow by their ratio recurrences; a handful of steps).
/// Above: the ratio-of-uniforms rejection method of Stadlober (1989),
/// "Sampling from Poisson, binomial and hypergeometric distributions: ratio
/// of uniforms as a simple and fast alternative" (HRUA), whose expected
/// number of trials is bounded whatever the counts; the support is cut 16
/// standard deviations above the mean, where the probability left is far
/// below 10⁻⁵⁰. So the cost of a draw does not grow with the counts.
fn hypergeometric(population: u64, successes: u64, draws: u64, rng: &mut Rng) -> u64 {
    let failures = population - successes;
    let lo = draws.saturating_sub(failures);
    let hi = successes.min(draws);
    if lo == hi {
        return lo;
    }
    let (n, k, m) = (population as f64, successes as f64, draws as f64);
    let variance = m * (k / n) * (1.0 - k / n) * (n - m) / (n - 1.0);
    if variance > SEARCH_VARIANCE {
        return hypergeometric_rejection(population, successes, draws, rng);
    }
    let mode = ((((m + 1.0) * (k + 1.0)) / (n + 2.0)).floor() as u64).clamp(lo, hi);
    let p_mode = (ln_choose(successes, mode) + ln_choose(failures, draws - mode) - ln_choose(population, draws)).exp();
    let mut u = rng.next_f64() - p_mode;
    if u <= 0.0 {
        return mode;
    }
    // p(x + 1)/p(x) = (K − x)(m − x) / ((x + 1)(N − K − m + x + 1)).
    let (mut up, mut p_up) = (mode, p_mode);
    let (mut down, mut p_down) = (mode, p_mode);
    let base = n - k - m;
    loop {
        let mut moved = false;
        if up < hi {
            let x = up as f64;
            p_up *= (k - x) * (m - x) / ((x + 1.0) * (base + x + 1.0));
            up += 1;
            u -= p_up;
            if u <= 0.0 {
                return up;
            }
            moved = true;
        }
        if down > lo {
            let x = down as f64;
            p_down *= x * (base + x) / ((k - x + 1.0) * (m - x + 1.0));
            down -= 1;
            u -= p_down;
            if u <= 0.0 {
                return down;
            }
            moved = true;
        }
        if !moved {
            // Rounding left a sliver of mass unassigned: the mode is as good
            // a place for it as any.
            return mode;
        }
    }
}

/// Stadlober's HRUA for a hypergeometric variance above
/// [`SEARCH_VARIANCE`]. By symmetry it samples with the smaller of the
/// successes and failures and the smaller of the draws and the items left,
/// then maps back. With `a = μ + ½`, `h = 2√(2/e)·√(σ² + ½) + 3 − 2√(3/e)`
/// and `U, V` uniform, `X = a + h(V − ½)/U` is accepted as `K = ⌊X⌋` when
/// `U² ≤ p(K)/p(mode)`, tested on `T = ln p(K) − ln p(mode)` with the
/// squeezes `U(4 − U) − 3 ≤ T` (accept) and `U(U − T) ≥ 1` (reject).
fn hypergeometric_rejection(population: u64, successes: u64, draws: u64, rng: &mut Rng) -> u64 {
    const D1: f64 = 1.715_527_769_921_413_5; // 2√(2/e)
    const D2: f64 = 0.898_916_162_058_898_8; // 3 − 2√(3/e)
    let failures = population - successes;
    let sample = draws.min(population - draws);
    let (small, large) = (successes.min(failures), successes.max(failures));
    let (n, s, g) = (population as f64, sample as f64, small as f64);
    let p = g / n;
    let mean = s * p;
    let a = mean + 0.5;
    let variance = (n - s) * s * p * (1.0 - p) / (n - 1.0);
    let c = (variance + 0.5).sqrt();
    let h = D1 * c + D2;
    let mode = (((s + 1.0) * (g + 1.0)) / (n + 2.0)).floor() as u64;
    let bound = ((sample.min(small) + 1) as f64).min((a + 16.0 * c).floor());
    // T = ln p(K) − ln p(mode), in differences that stay accurate.
    let gap = |kk: u64| {
        ln_fact_ratio(mode, kk)
            + ln_fact_ratio(small - mode, small - kk)
            + ln_fact_ratio(sample - mode, sample - kk)
            + ln_fact_ratio(large - sample + mode, large - sample + kk)
    };
    let x = loop {
        let u = rng.next_f64();
        let v = rng.next_f64();
        if u <= 0.0 {
            continue;
        }
        let x = a + h * (v - 0.5) / u;
        if x < 0.0 || x >= bound {
            continue;
        }
        let kk = x.floor() as u64;
        let t = gap(kk);
        if u * (4.0 - u) - 3.0 <= t {
            break kk;
        }
        if u * (u - t) >= 1.0 {
            continue;
        }
        if 2.0 * u.ln() <= t {
            break kk;
        }
    };
    // Undo the symmetries: successes ↔ failures, draws ↔ items left.
    let x = if successes > failures { sample - x } else { x };
    if sample < draws { successes - x } else { x }
}

/// Bounds `(least, most)` of `Σ ln x!` over the completions of a node.
type Bounds = (f64, f64);

/// Distinct past values of a node: quantized value → (value, multiplicity).
type Pasts = HashMap<i64, (f64, f64)>;

/// The network of partial tables.
struct Network<'a> {
    rows: &'a [u64],
    columns: &'a [u64],
    /// `Σ ln Cⱼ!` over the columns from each stage on.
    columns_from: Vec<f64>,
    threshold: f64,
    work_limit: usize,
    work: usize,
    /// Bounds of each node, per stage.
    bounds: Vec<HashMap<Vec<u64>, Bounds>>,
}

impl<'a> Network<'a> {
    fn new(rows: &'a [u64], columns: &'a [u64], threshold: f64, work_limit: usize) -> Self {
        let mut columns_from = vec![0.0; columns.len() + 1];
        for s in (0..columns.len()).rev() {
            columns_from[s] = columns_from[s + 1] + ln_fact(columns[s]);
        }
        Self {
            rows,
            columns,
            columns_from,
            threshold,
            work_limit,
            work: 0,
            bounds: vec![HashMap::new(); columns.len() + 1],
        }
    }

    /// The p-value given `k = ln(Π R! Π C! / N!)`, or `None` when the work
    /// limit is reached.
    fn run(&mut self, k: f64) -> Option<f64> {
        let mut start = self.rows.to_vec();
        start.sort_unstable();
        let mut current: HashMap<Vec<u64>, Pasts> = HashMap::new();
        current.entry(start).or_default().insert(0, (0.0, 1.0));
        let mut total = 0.0;
        for stage in 0..self.columns.len() {
            let c = self.columns[stage];
            let mut next: HashMap<Vec<u64>, Pasts> = HashMap::new();
            for (key, pasts) in current {
                let mut splits: Vec<(f64, Vec<u64>)> = Vec::new();
                // Each split costs at least one unit of work: stop listing
                // them once the budget is gone (a column of a huge table has
                // too many splits to list).
                let budget = self.work_limit.saturating_sub(self.work);
                let complete = for_each_split(&key, c, &mut |x| {
                    let arc: f64 = x.iter().map(|&v| ln_fact(v)).sum();
                    let mut child: Vec<u64> = key.iter().zip(x).map(|(r, v)| r - v).collect();
                    child.sort_unstable();
                    splits.push((arc, child));
                    splits.len() <= budget
                });
                if !complete {
                    return None;
                }
                for (arc, child) in splits {
                    self.work += 1 + pasts.len();
                    if self.work > self.work_limit {
                        return None;
                    }
                    let (least, most) = self.node_bounds(stage + 1, &child);
                    let remaining: u64 = child.iter().sum();
                    // ln of the sum over completions of Π 1/n!: N′!/(Π R′! Π C′!).
                    let mass = ln_fact(remaining)
                        - child.iter().map(|&r| ln_fact(r)).sum::<f64>()
                        - self.columns_from[stage + 1];
                    for &(value, multiplicity) in pasts.values() {
                        let w = value + arc;
                        if w + least >= self.threshold {
                            total += (multiplicity.ln() + k - w + mass).exp();
                        } else if w + most >= self.threshold {
                            let slot = next.entry(child.clone()).or_default();
                            let q = (w * 1e9).round() as i64;
                            slot.entry(q).or_insert((w, 0.0)).1 += multiplicity;
                        }
                    }
                }
            }
            current = next;
        }
        Some(total)
    }

    /// Bounds on the rest of `Σ ln nᵢⱼ!` from `stage` on with remaining row
    /// totals `rows`: the tighter of the column-wise and row-wise relaxations.
    fn node_bounds(&mut self, stage: usize, rows: &[u64]) -> Bounds {
        if let Some(&b) = self.bounds[stage].get(rows) {
            return b;
        }
        let cols = &self.columns[stage..];
        let lf = ln_fact;
        let (mut lo_c, mut hi_c, mut lo_r, mut hi_r) = (0.0, 0.0, 0.0, 0.0);
        for &c in cols {
            lo_c += even_split(c, rows, lf);
            hi_c += uneven_split(c, rows, lf);
        }
        for &r in rows {
            lo_r += even_split(r, cols, lf);
            hi_r += uneven_split(r, cols, lf);
        }
        let b = (f64::max(lo_c, lo_r), f64::min(hi_c, hi_r));
        self.bounds[stage].insert(rows.to_vec(), b);
        b
    }
}

/// Smallest `Σ ln xᵢ!` with `Σ xᵢ = total`, `0 ≤ xᵢ ≤ capsᵢ` (as even as the
/// caps allow: water-filling, optimal because `ln x!` is convex).
fn even_split(total: u64, caps: &[u64], lf: impl Fn(u64) -> f64) -> f64 {
    let mut sorted = caps.to_vec();
    sorted.sort_unstable();
    let (mut remaining, mut left, mut value) = (total, sorted.len() as u64, 0.0);
    for &cap in &sorted {
        if left == 0 {
            break;
        }
        if cap <= remaining / left {
            value += lf(cap);
            remaining -= cap;
            left -= 1;
        } else {
            let (q, extra) = (remaining / left, remaining % left);
            value += extra as f64 * lf(q + 1) + (left - extra) as f64 * lf(q);
            break;
        }
    }
    value
}

/// Largest `Σ ln xᵢ!` with `Σ xᵢ = total`, `0 ≤ xᵢ ≤ capsᵢ` (the largest caps
/// filled first: this vector majorizes every other feasible one).
fn uneven_split(total: u64, caps: &[u64], lf: impl Fn(u64) -> f64) -> f64 {
    let mut sorted = caps.to_vec();
    sorted.sort_unstable_by(|a, b| b.cmp(a));
    let (mut remaining, mut value) = (total, 0.0);
    for cap in sorted {
        let x = cap.min(remaining);
        value += lf(x);
        remaining -= x;
    }
    value
}

/// Call `f` with every vector `x` with `Σ x = total` and `0 ≤ xᵢ ≤ capsᵢ`,
/// until `f` returns `false`. Returns `false` when stopped early.
fn for_each_split(caps: &[u64], total: u64, f: &mut impl FnMut(&[u64]) -> bool) -> bool {
    fn go(
        i: usize,
        left: u64,
        caps: &[u64],
        suffix: &[u64],
        x: &mut [u64],
        f: &mut impl FnMut(&[u64]) -> bool,
    ) -> bool {
        if i == caps.len() {
            return left != 0 || f(x);
        }
        // Whatever the later cells cannot hold must go here.
        let low = left.saturating_sub(suffix[i + 1]);
        for v in low..=caps[i].min(left) {
            x[i] = v;
            if !go(i + 1, left - v, caps, suffix, x, f) {
                return false;
            }
        }
        true
    }
    let mut suffix = vec![0u64; caps.len() + 1];
    for i in (0..caps.len()).rev() {
        suffix[i] = suffix[i + 1] + caps[i];
    }
    let mut x = vec![0u64; caps.len()];
    total > suffix[0] || go(0, total, caps, &suffix, &mut x, f)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exact(t: &[Vec<u64>]) -> ExactTest {
        fisher_exact_rxc(t, &ExactOptions::default()).unwrap()
    }

    /// Brute-force p-value: every table with the margins, enumerated.
    fn brute(t: &[Vec<u64>]) -> f64 {
        fn go(j: usize, rem: &[u64], cols: &[u64], acc: f64, obs: f64, k: f64, p: &mut f64) {
            if j == cols.len() {
                if rem.iter().all(|&r| r == 0) && acc >= obs - 1e-7 {
                    *p += (k - acc).exp();
                }
                return;
            }
            for_each_split(rem, cols[j], &mut |x| {
                let a: f64 = x.iter().map(|&v| ln_fact(v)).sum();
                let next: Vec<u64> = rem.iter().zip(x).map(|(r, v)| r - v).collect();
                go(j + 1, &next, cols, acc + a, obs, k, p);
                true
            });
        }
        let rows: Vec<u64> = t.iter().map(|r| r.iter().sum()).collect();
        let cols: Vec<u64> = (0..t[0].len()).map(|j| t.iter().map(|r| r[j]).sum()).collect();
        let n: u64 = rows.iter().sum();
        let k =
            rows.iter().map(|&r| ln_fact(r)).sum::<f64>() + cols.iter().map(|&c| ln_fact(c)).sum::<f64>() - ln_fact(n);
        let obs: f64 = t.iter().flatten().map(|&x| ln_fact(x)).sum();
        let mut p = 0.0;
        go(0, &rows, &cols, 0.0, obs, k, &mut p);
        p
    }

    #[test]
    fn network_matches_brute_force_enumeration() {
        let tables: Vec<Vec<Vec<u64>>> = vec![
            vec![vec![3, 1, 4], vec![1, 5, 2]],
            vec![vec![2, 0, 3], vec![1, 4, 0], vec![0, 2, 5]],
            vec![vec![5, 0], vec![0, 5]],
            vec![vec![1, 2, 3, 4], vec![4, 3, 2, 1]],
            vec![vec![0, 3, 1], vec![2, 2, 2], vec![3, 0, 1], vec![1, 1, 4]],
        ];
        for t in tables {
            let e = exact(&t);
            let b = brute(&t);
            assert!((e.p_value - b).abs() < 1e-12, "{t:?}: {} vs {b}", e.p_value);
            assert_eq!(e.method, ExactMethod::Network);
        }
    }

    #[test]
    fn splits_respect_caps() {
        let mut seen = Vec::new();
        for_each_split(&[1, 2], 2, &mut |x| {
            seen.push(x.to_vec());
            true
        });
        assert_eq!(seen, vec![vec![0, 2], vec![1, 1]]);
        let mut n = 0;
        for_each_split(&[1, 1], 3, &mut |_| {
            n += 1;
            true
        });
        assert_eq!(n, 0);
    }

    #[test]
    fn degenerate_tables_have_p_one() {
        let t = exact(&[vec![3, 4], vec![0, 0]]);
        assert_eq!((t.p_value, t.dimensions), (1.0, (1, 2)));
        assert!(fisher_exact_rxc(&[vec![1, 2], vec![3]], &ExactOptions::default()).is_err());
        assert!(fisher_exact_rxc(&[], &ExactOptions::default()).is_err());
    }

    #[test]
    fn monte_carlo_fallback_is_flagged_and_close() {
        let t = vec![vec![3, 1, 4], vec![1, 5, 2], vec![2, 2, 6]];
        let e = exact(&t);
        let opts = ExactOptions { work_limit: 0, monte_carlo_tables: 20_000, seed: 7 };
        let mc = fisher_exact_rxc(&t, &opts).unwrap();
        let ExactMethod::MonteCarlo { standard_error, tables, .. } = mc.method else { panic!("not Monte Carlo") };
        assert_eq!(tables, 20_000);
        assert!(mc.is_estimate());
        assert!((mc.p_value - e.p_value).abs() < 4.0 * standard_error + 1e-4, "{} vs {}", mc.p_value, e.p_value);
        // A tiny work limit also falls back.
        let small = fisher_exact_rxc(&t, &ExactOptions { work_limit: 3, ..ExactOptions::default() }).unwrap();
        assert!(small.is_estimate());
        // Same seed, same estimate.
        assert_eq!(fisher_exact_rxc(&t, &opts).unwrap().p_value, mc.p_value);
    }

    #[test]
    fn transposed_table_gives_the_same_p_value() {
        let t = vec![vec![2, 0, 3, 1], vec![1, 4, 0, 2]];
        let tt: Vec<Vec<u64>> = (0..4).map(|j| t.iter().map(|r| r[j]).collect()).collect();
        assert!((exact(&t).p_value - exact(&tt).p_value).abs() < 1e-13);
    }

    /// Every table with the given margins and its multiple hypergeometric
    /// probability.
    fn all_tables(rows: &[u64], cols: &[u64]) -> Vec<(Vec<u64>, f64)> {
        let n: u64 = rows.iter().sum();
        let k =
            rows.iter().map(|&r| ln_fact(r)).sum::<f64>() + cols.iter().map(|&c| ln_fact(c)).sum::<f64>() - ln_fact(n);
        let mut out = Vec::new();
        // Fill row by row: each row splits across the column totals left.
        fn go(i: usize, left: &[u64], rows: &[u64], cell: &mut Vec<u64>, k: f64, out: &mut Vec<(Vec<u64>, f64)>) {
            if i == rows.len() {
                let lp: f64 = cell.iter().map(|&x| ln_fact(x)).sum();
                out.push((cell.clone(), (k - lp).exp()));
                return;
            }
            for_each_split(left, rows[i], &mut |x| {
                let next: Vec<u64> = left.iter().zip(x).map(|(l, v)| l - v).collect();
                let at = cell.len();
                cell.extend_from_slice(x);
                go(i + 1, &next, rows, cell, k, out);
                cell.truncate(at);
                true
            });
        }
        go(0, cols, rows, &mut Vec::new(), k, &mut out);
        out
    }

    #[test]
    fn random_tables_follow_the_multiple_hypergeometric() {
        use stat_core::distributions::{ChiSquared, ContinuousDistribution};
        let (rows, cols) = ([4u64, 3, 3], [3u64, 5, 2]);
        let tables = all_tables(&rows, &cols);
        let total: f64 = tables.iter().map(|t| t.1).sum();
        assert!((total - 1.0).abs() < 1e-12);
        let index: HashMap<Vec<u64>, usize> = tables.iter().enumerate().map(|(i, t)| (t.0.clone(), i)).collect();
        let draws = 200_000usize;
        let mut counts = vec![0usize; tables.len()];
        let mut rng = Rng::new(11);
        let mut t = vec![0u64; 9];
        for _ in 0..draws {
            random_table(&rows, &cols, &mut rng, &mut t);
            counts[index[&t]] += 1;
        }
        let chi2: f64 = tables
            .iter()
            .zip(&counts)
            .map(|((_, p), &o)| {
                let e = p * draws as f64;
                (o as f64 - e).powi(2) / e
            })
            .sum();
        let df = (tables.len() - 1) as f64;
        let p = ChiSquared::new(df).unwrap().sf(chi2).unwrap();
        assert!(p > 0.001, "chi-square {chi2} on {df} df, p = {p}");
    }

    #[test]
    fn hypergeometric_draws_have_the_right_moments() {
        // Population 150 000 with 40 000 successes, 70 000 draws.
        let (n, k, m) = (150_000u64, 40_000u64, 70_000u64);
        let mut rng = Rng::new(3);
        let draws = 20_000;
        let xs: Vec<f64> = (0..draws).map(|_| hypergeometric(n, k, m, &mut rng) as f64).collect();
        let mean = xs.iter().sum::<f64>() / draws as f64;
        let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (draws - 1) as f64;
        let (nf, kf, mf) = (n as f64, k as f64, m as f64);
        let mu = mf * kf / nf;
        let sigma2 = mu * (1.0 - kf / nf) * (nf - mf) / (nf - 1.0);
        assert!((mean - mu).abs() < 4.0 * (sigma2 / draws as f64).sqrt(), "{mean} vs {mu}");
        assert!((var / sigma2 - 1.0).abs() < 0.05, "{var} vs {sigma2}");
    }

    #[test]
    fn monte_carlo_agrees_with_enumeration_on_small_tables() {
        let tables: Vec<Vec<Vec<u64>>> = vec![
            vec![vec![3, 1, 4], vec![1, 5, 2]],
            vec![vec![2, 0, 3], vec![1, 4, 0], vec![0, 2, 5]],
            vec![vec![1, 2, 3, 4], vec![4, 3, 2, 1]],
            vec![vec![0, 3, 1], vec![2, 2, 2], vec![3, 0, 1], vec![1, 1, 4]],
        ];
        for (s, t) in tables.iter().enumerate() {
            let b = brute(t);
            let opts = ExactOptions { work_limit: 0, monte_carlo_tables: 40_000, seed: 100 + s as u64 };
            let mc = fisher_exact_rxc(t, &opts).unwrap();
            let ExactMethod::MonteCarlo { standard_error, .. } = mc.method else { panic!("not Monte Carlo") };
            assert!((mc.p_value - b).abs() < 4.0 * standard_error + 1e-4, "{t:?}: {} vs {b}", mc.p_value);
        }
    }

    #[test]
    fn large_totals_are_quick_and_beyond_u32_supported() {
        let t = vec![
            vec![20_000, 15_000, 9_000, 8_000],
            vec![19_000, 14_500, 9_500, 8_200],
            vec![21_000, 14_800, 8_700, 8_300],
        ];
        let start = std::time::Instant::now();
        let e = exact(&t);
        assert!(e.is_estimate());
        assert!(e.p_value > 0.0 && e.p_value <= 1.0);
        assert!(start.elapsed().as_secs_f64() < 20.0, "{:?}", start.elapsed());
        // Proportional rows: the observed table is the most probable one.
        let big = vec![vec![3_000_000_000u64, 2_000_000_000], vec![3_000_000_000, 2_000_000_000]];
        let e = exact(&big);
        assert!(e.is_estimate() && e.p_value > 0.9, "{e:?}");
        let skewed = vec![vec![3_000_000_000u64, 2_000_000_000], vec![2_000_000_000, 3_000_000_000]];
        let e =
            fisher_exact_rxc(&skewed, &ExactOptions { monte_carlo_tables: 200, ..ExactOptions::default() }).unwrap();
        assert!(e.p_value < 0.01, "{e:?}");
        // Near the limit: a 2 × 3 table with N ≈ 4.5·10¹⁵, 10 000 tables.
        let near = vec![vec![1u64 << 50, 1 << 50, 1 << 49], vec![1 << 50, (1 << 50) + 400_000_000, 1 << 49]];
        let start = std::time::Instant::now();
        let e = exact(&near);
        assert!(start.elapsed().as_secs_f64() < 20.0, "{:?}", start.elapsed());
        assert!(e.is_estimate() && e.p_value < 0.01, "{e:?}");
        let too_big = vec![vec![MAX_TOTAL, 1], vec![1, 1]];
        assert!(fisher_exact_rxc(&too_big, &ExactOptions::default()).is_err());
    }

    #[test]
    fn rejection_draws_follow_the_hypergeometric() {
        use stat_core::distributions::{ChiSquared, ContinuousDistribution};
        // Variance ≈ 150 (rejection path): every value's probability exactly.
        let (n, k, m) = (2_000u64, 700u64, 900u64);
        let pmf = |x: u64| (ln_choose(k, x) + ln_choose(n - k, m - x) - ln_choose(n, m)).exp();
        let mut rng = Rng::new(5);
        let draws = 200_000usize;
        let mut counts = vec![0usize; (k.min(m) + 1) as usize];
        for _ in 0..draws {
            counts[hypergeometric(n, k, m, &mut rng) as usize] += 1;
        }
        // Pool the tails into cells expected to hold at least 5.
        let (mut chi2, mut cells, mut obs, mut exp) = (0.0, 0usize, 0.0, 0.0);
        for (x, &c) in counts.iter().enumerate() {
            obs += c as f64;
            exp += pmf(x as u64) * draws as f64;
            if exp >= 5.0 {
                chi2 += (obs - exp).powi(2) / exp;
                cells += 1;
                (obs, exp) = (0.0, 0.0);
            }
        }
        chi2 += (obs - exp).powi(2) / exp.max(1e-300);
        let p = ChiSquared::new((cells - 1) as f64).unwrap().sf(chi2).unwrap();
        assert!(p > 0.001, "chi-square {chi2} on {} cells, p = {p}", cells);
        // Both symmetries (successes > failures, draws > half).
        let mut rng = Rng::new(9);
        let (n, k, m) = (5_000u64, 3_800u64, 4_100u64);
        let xs: Vec<f64> = (0..20_000).map(|_| hypergeometric(n, k, m, &mut rng) as f64).collect();
        let mean = xs.iter().sum::<f64>() / xs.len() as f64;
        let mu = m as f64 * k as f64 / n as f64;
        assert!((mean - mu).abs() < 0.2, "{mean} vs {mu}");
        assert!(xs.iter().all(|&x| x >= (m - (n - k)) as f64 && x <= k as f64));
    }

    #[test]
    fn huge_hypergeometric_draws_are_quick_and_right() {
        // N = 2⁵², a third successes, half drawn: σ ≈ 2.2·10⁷.
        let n = 1u64 << 52;
        let (k, m) = (n / 3, n / 2);
        let mut rng = Rng::new(21);
        let start = std::time::Instant::now();
        let xs: Vec<f64> = (0..20_000).map(|_| hypergeometric(n, k, m, &mut rng) as f64).collect();
        assert!(start.elapsed().as_secs_f64() < 5.0, "{:?}", start.elapsed());
        let (nf, kf, mf) = (n as f64, k as f64, m as f64);
        let mu = mf * kf / nf;
        let sigma2 = mu * (1.0 - kf / nf) * (nf - mf) / (nf - 1.0);
        let mean = xs.iter().sum::<f64>() / xs.len() as f64;
        let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (xs.len() - 1) as f64;
        assert!((mean - mu).abs() < 4.0 * (sigma2 / xs.len() as f64).sqrt(), "{mean} vs {mu}");
        assert!((var / sigma2 - 1.0).abs() < 0.05, "{var} vs {sigma2}");
    }

    #[test]
    fn factorial_ratios_are_accurate() {
        for (a, b) in [(123_456u64, 120_000u64), (10_000, 10_001), (2_000_000, 1_999_000), (50_000, 9_999)] {
            let direct = ln_gamma(a as f64 + 1.0) - ln_gamma(b as f64 + 1.0);
            let r = ln_fact_ratio(a, b);
            assert!((r - direct).abs() < 1e-8 * direct.abs().max(1.0), "{a}, {b}: {r} vs {direct}");
            assert!((ln_fact_ratio(b, a) + r).abs() < 1e-9 * r.abs().max(1.0));
        }
        // Huge arguments: ln((n + 1)!/n!) = ln(n + 1).
        let n = 1u64 << 52;
        assert!((ln_fact_ratio(n + 1, n) - ((n + 1) as f64).ln()).abs() < 1e-9);
        assert!((ln_fact_ratio(n + 2, n) - ((n + 1) as f64).ln() - ((n + 2) as f64).ln()).abs() < 1e-9);
        assert!((ln_choose(10, 3) - 120f64.ln()).abs() < 1e-12);
    }

    /// A u64::MAX cell used to wrap the row total round to a small number
    /// (p = 1 in release builds); it now fails the total-size check.
    #[test]
    fn overflowing_totals_are_refused() {
        let r = fisher_exact_rxc(&[vec![u64::MAX, 1], vec![1, 1]], &ExactOptions::default());
        assert!(matches!(r, Err(StatError::InvalidArgument { .. })), "{r:?}");
    }
}
