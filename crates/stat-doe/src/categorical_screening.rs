//! Screening designs that mix two-level continuous factors with categorical
//! factors of any number of levels, built by D-optimal coordinate exchange.
//!
//! # Model and coding
//! A categorical factor with `L` levels enters the model through `L − 1`
//! **normalized Helmert contrasts**: contrast `j = 1, …, L − 1` is −1 at the
//! levels before `j`, `j` at level `j` and 0 after it, scaled by
//! `√(L / (j(j + 1)))`. The contrasts are mutually orthogonal and, over a
//! design that uses every level equally often, each has mean 0 and mean
//! square 1, like a balanced ±1 column. The model is the intercept, a column
//! per continuous factor and the contrasts of each categorical factor (main
//! effects), optionally with every two-factor interaction (the products of
//! the columns of two different factors). With this coding an orthogonal
//! array has `XᵀX = nI`, so its D-efficiency `100 · det(XᵀX)^{1/p} / n` is
//! 100%, and every other design scores below it (Goos & Jones 2011, *Optimal
//! Design of Experiments: A Case Study Approach*, ch. 2–3; D-efficiency is
//! invariant to the choice of contrasts up to that scale).
//!
//! # Search
//! Coordinate exchange (Meyer & Nachtsheim 1995): from a random design, each
//! coordinate of each run is replaced in turn by the candidate that most
//! increases `det(XᵀX)`: −1 and +1 for a continuous factor, each level for a
//! categorical one. The determinant ratio of replacing model row `f` by `g`
//! is `(1 + gᵀM⁻¹g)(1 − fᵀM⁻¹f) + (fᵀM⁻¹g)²` (the determinant lemma applied
//! twice), so a candidate costs `O(p²)`. Passes repeat until no coordinate
//! improves; the best of several seeded starts is kept. A singular start is
//! improved for `det(XᵀX + δI)` with a small ridge `δ` until it becomes
//! non-singular.
//!
//! Runs are stored in coded units: a continuous factor at −1 or +1, and a
//! categorical factor's level `ℓ = 0, …, L − 1` at `−1 + 2ℓ/(L − 1)`, its
//! [`Factor`] running from 1 to `L` so that actual units give the level
//! number (see [`level_of`]).

use crate::design::{Design, Factor, PointType, Rng, default_factor_names};
use crate::optimal::{cholesky, invert, log_det_from_cholesky};
use stat_core::{Result, StatError};

/// One factor of a mixed screening design.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScreeningFactor {
    /// A continuous factor run at its low and high settings.
    Continuous,
    /// A categorical factor with this many levels (2 to [`MAX_LEVELS`]).
    Categorical {
        /// Number of levels.
        levels: usize,
    },
}

impl ScreeningFactor {
    /// Model columns of the factor's main effect: 1, or `L − 1`.
    #[must_use]
    pub fn columns(self) -> usize {
        match self {
            Self::Continuous => 1,
            Self::Categorical { levels } => levels.saturating_sub(1),
        }
    }

    /// Candidate coded values in the search.
    fn candidates(self) -> Vec<f64> {
        match self {
            Self::Continuous => vec![-1.0, 1.0],
            Self::Categorical { levels } => (0..levels).map(|l| level_code(l, levels)).collect(),
        }
    }
}

/// Most levels of a categorical factor.
pub const MAX_LEVELS: usize = 12;

/// Coded value of level `level` (0-based) of a factor with `levels` levels.
#[must_use]
pub fn level_code(level: usize, levels: usize) -> f64 {
    if levels < 2 { 0.0 } else { -1.0 + 2.0 * level as f64 / (levels - 1) as f64 }
}

/// The 0-based level whose code is nearest `code` for a factor with
/// `levels` levels.
#[must_use]
pub fn level_of(code: f64, levels: usize) -> usize {
    if levels < 2 {
        return 0;
    }
    let l = ((code + 1.0) * (levels - 1) as f64 / 2.0).round();
    (l.max(0.0) as usize).min(levels - 1)
}

/// The normalized Helmert contrasts of level `level` of `levels` levels.
#[must_use]
pub fn helmert_row(level: usize, levels: usize) -> Vec<f64> {
    (1..levels)
        .map(|j| {
            let scale = (levels as f64 / (j * (j + 1)) as f64).sqrt();
            scale
                * match level.cmp(&j) {
                    std::cmp::Ordering::Less => -1.0,
                    std::cmp::Ordering::Equal => j as f64,
                    std::cmp::Ordering::Greater => 0.0,
                }
        })
        .collect()
}

/// Model row of coded run `x` for factors `kinds`: intercept, main-effect
/// columns, and with `interactions` the products of every two factors'
/// columns.
#[must_use]
pub fn model_row(kinds: &[ScreeningFactor], x: &[f64], interactions: bool) -> Vec<f64> {
    let blocks: Vec<Vec<f64>> = kinds
        .iter()
        .zip(x)
        .map(|(k, &v)| match *k {
            ScreeningFactor::Continuous => vec![v],
            ScreeningFactor::Categorical { levels } => helmert_row(level_of(v, levels), levels),
        })
        .collect();
    let mut row = vec![1.0];
    for b in &blocks {
        row.extend_from_slice(b);
    }
    if interactions {
        for a in 0..blocks.len() {
            for b in a + 1..blocks.len() {
                for u in &blocks[a] {
                    for v in &blocks[b] {
                        row.push(u * v);
                    }
                }
            }
        }
    }
    row
}

/// Number of model terms for `kinds`.
#[must_use]
pub fn model_terms(kinds: &[ScreeningFactor], interactions: bool) -> usize {
    let cols: Vec<usize> = kinds.iter().map(|k| k.columns()).collect();
    let mut p = 1 + cols.iter().sum::<usize>();
    if interactions {
        for a in 0..cols.len() {
            for b in a + 1..cols.len() {
                p += cols[a] * cols[b];
            }
        }
    }
    p
}

/// `ln det(XᵀX)` of coded `runs` (−∞ if singular).
#[must_use]
pub fn screening_log_det(kinds: &[ScreeningFactor], runs: &[Vec<f64>], interactions: bool) -> f64 {
    let rows: Vec<Vec<f64>> = runs.iter().map(|r| model_row(kinds, r, interactions)).collect();
    cholesky(&information(&rows, 0.0)).map_or(f64::NEG_INFINITY, |l| log_det_from_cholesky(&l))
}

/// D-efficiency `100 · det(XᵀX)^{1/p} / n` of coded `runs` (0 if singular).
#[must_use]
pub fn screening_d_efficiency(kinds: &[ScreeningFactor], runs: &[Vec<f64>], interactions: bool) -> f64 {
    let p = model_terms(kinds, interactions) as f64;
    100.0 * (screening_log_det(kinds, runs, interactions) / p).exp() / runs.len() as f64
}

fn information(rows: &[Vec<f64>], delta: f64) -> Vec<Vec<f64>> {
    let p = rows.first().map_or(0, Vec::len);
    let mut m = vec![vec![0.0; p]; p];
    for r in rows {
        for a in 0..p {
            for b in 0..=a {
                m[a][b] += r[a] * r[b];
            }
        }
    }
    for a in 0..p {
        for b in 0..a {
            m[b][a] = m[a][b];
        }
        m[a][a] += delta;
    }
    m
}

/// Options for [`categorical_screening`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CategoricalScreeningOptions {
    /// Number of runs (at least the number of model terms).
    pub runs: usize,
    /// Include every two-factor interaction in the model.
    pub interactions: bool,
    /// Random starts.
    pub starts: usize,
    /// Seed of the starts.
    pub seed: u64,
    /// Most passes per start.
    pub max_passes: usize,
}

impl CategoricalScreeningOptions {
    /// Main effects in `runs` runs, 20 starts, seed 1.
    #[must_use]
    pub fn new(runs: usize) -> Self {
        Self { runs, interactions: false, starts: 20, seed: 1, max_passes: 100 }
    }
}

/// A mixed screening design and its quality.
#[derive(Debug, Clone, PartialEq)]
pub struct CategoricalScreening {
    /// The design (categorical factors coded as described in the module
    /// notes; point type [`PointType::Other`]).
    pub design: Design,
    /// The factors.
    pub kinds: Vec<ScreeningFactor>,
    /// Whether the model has two-factor interactions.
    pub interactions: bool,
    /// Model terms `p`.
    pub terms: usize,
    /// `ln det(XᵀX)`.
    pub log_det: f64,
    /// `100 · det(XᵀX)^{1/p} / n`.
    pub d_efficiency: f64,
}

/// The default number of runs for `kinds`: the model terms rounded up to a
/// multiple of the levels' least common multiple when that adds at most
/// half again, otherwise the terms plus 4 (Goos & Jones 2011, ch. 2: a few
/// runs beyond the minimum for an error estimate).
#[must_use]
pub fn default_runs(kinds: &[ScreeningFactor], interactions: bool) -> usize {
    let p = model_terms(kinds, interactions);
    let gcd = |mut a: usize, mut b: usize| {
        while b != 0 {
            (a, b) = (b, a % b);
        }
        a
    };
    let lcm = kinds
        .iter()
        .map(|k| match k {
            ScreeningFactor::Continuous => 2,
            ScreeningFactor::Categorical { levels } => *levels,
        })
        .fold(1usize, |a, b| a / gcd(a, b) * b);
    let wanted = p + 4;
    let rounded = wanted.div_ceil(lcm) * lcm;
    if rounded <= wanted + wanted / 2 { rounded } else { wanted }
}

/// Build a D-optimal screening design for continuous and categorical
/// factors (see the module documentation).
///
/// # Errors
/// [`StatError::InvalidArgument`] for no factors, a categorical factor with
/// fewer than 2 or more than [`MAX_LEVELS`] levels, fewer runs than model
/// terms, no starts or more than 10 000 runs; [`StatError::RankDeficient`]
/// if no start reached a non-singular design.
pub fn categorical_screening(
    kinds: &[ScreeningFactor],
    options: &CategoricalScreeningOptions,
) -> Result<CategoricalScreening> {
    let k = kinds.len();
    if k == 0 {
        return Err(invalid("a design needs at least one factor"));
    }
    if kinds.iter().any(|f| matches!(f, ScreeningFactor::Categorical { levels } if !(2..=MAX_LEVELS).contains(levels)))
    {
        return Err(invalid(format!("a categorical factor needs 2 to {MAX_LEVELS} levels")));
    }
    let (p, n) = (model_terms(kinds, options.interactions), options.runs);
    if n < p {
        return Err(invalid(format!("the model has {p} terms, so at least {p} runs are needed (got {n})")));
    }
    if options.starts == 0 || n > 10_000 {
        return Err(invalid("use at least one start and at most 10 000 runs"));
    }
    let candidates: Vec<Vec<f64>> = kinds.iter().map(|f| f.candidates()).collect();
    let mut rng = Rng::new(options.seed);
    let mut best: Option<(f64, Vec<Vec<f64>>)> = None;
    for _ in 0..options.starts {
        // A random start that uses the levels in turn (shuffled), so it is
        // balanced and rarely singular.
        let mut runs = vec![vec![0.0; k]; n];
        for (j, c) in candidates.iter().enumerate() {
            let mut order: Vec<usize> = (0..n).collect();
            rng.shuffle(&mut order);
            for (i, &r) in order.iter().enumerate() {
                runs[r][j] = c[i % c.len()];
            }
        }
        exchange(kinds, &candidates, options.interactions, &mut runs, options.max_passes);
        let ld = screening_log_det(kinds, &runs, options.interactions);
        if ld.is_finite() && best.as_ref().is_none_or(|(b, _)| ld > *b + 1e-10 * b.abs().max(1.0)) {
            best = Some((ld, runs));
        }
    }
    let (log_det, runs) = best.ok_or(StatError::RankDeficient { rank: 0, columns: p })?;
    let d_efficiency = 100.0 * (log_det / p as f64).exp() / n as f64;
    let names = default_factor_names(k);
    let factors: Vec<Factor> = names
        .iter()
        .zip(kinds)
        .map(|(name, f)| match f {
            ScreeningFactor::Continuous => Factor::coded(name.clone()),
            ScreeningFactor::Categorical { levels } => Factor::new(name.clone(), 1.0, *levels as f64),
        })
        .collect();
    let categorical = kinds.iter().filter(|f| matches!(f, ScreeningFactor::Categorical { .. })).count();
    let mut design = Design::new(
        format!(
            "Screening, {} continuous and {categorical} categorical factors, {p}-term model ({n} runs)",
            k - categorical
        ),
        factors,
        runs,
        vec![PointType::Other; n],
    )?;
    design.notes.push(format!(
        "Model: {}; categorical factors coded by normalized Helmert contrasts.",
        if options.interactions { "main effects and two-factor interactions" } else { "main effects" }
    ));
    design.notes.push(format!(
        "D-efficiency {d_efficiency:.2}% (100% for an orthogonal design); ln det(XᵀX) = {log_det:.6}; best of {} \
         coordinate-exchange starts (seed {}).",
        options.starts, options.seed
    ));
    for (j, f) in kinds.iter().enumerate() {
        if let ScreeningFactor::Categorical { levels } = *f {
            let mut counts = vec![0usize; levels];
            for r in &design.runs {
                counts[level_of(r[j], levels)] += 1;
            }
            let list: Vec<String> = counts.iter().map(ToString::to_string).collect();
            design.notes.push(format!("{}: levels 1 to {levels} run {} times.", names[j], list.join(", ")));
        }
    }
    Ok(CategoricalScreening {
        design,
        kinds: kinds.to_vec(),
        interactions: options.interactions,
        terms: p,
        log_det,
        d_efficiency,
    })
}

/// `aᵀ M b`.
fn quad(m: &[Vec<f64>], a: &[f64], b: &[f64]) -> f64 {
    m.iter().zip(a).map(|(row, x)| x * row.iter().zip(b).map(|(v, y)| v * y).sum::<f64>()).sum()
}

/// One coordinate-exchange search from `runs`, improving it in place.
fn exchange(
    kinds: &[ScreeningFactor],
    candidates: &[Vec<f64>],
    interactions: bool,
    runs: &mut [Vec<f64>],
    max_passes: usize,
) {
    let mut rows: Vec<Vec<f64>> = runs.iter().map(|r| model_row(kinds, r, interactions)).collect();
    let mut delta = 0.0;
    let inverse = |rows: &[Vec<f64>], delta: f64| invert(&information(rows, delta));
    let mut inv = match inverse(&rows, 0.0) {
        Some(m) if cholesky(&information(&rows, 0.0)).is_some() => m,
        _ => {
            delta = 1e-6 * runs.len() as f64;
            match inverse(&rows, delta) {
                Some(m) => m,
                None => return,
            }
        }
    };
    for _ in 0..max_passes {
        let mut improved = false;
        for i in 0..runs.len() {
            for j in 0..kinds.len() {
                let now = runs[i][j];
                let f = &rows[i];
                let ff = quad(&inv, f, f);
                let mut best: Option<(f64, f64, Vec<f64>)> = None;
                for &v in &candidates[j] {
                    if v == now {
                        continue;
                    }
                    let mut x = runs[i].clone();
                    x[j] = v;
                    let g = model_row(kinds, &x, interactions);
                    let ratio = (1.0 + quad(&inv, &g, &g)) * (1.0 - ff) + quad(&inv, f, &g).powi(2);
                    if ratio > best.as_ref().map_or(1.0 + 1e-9, |b| b.0) {
                        best = Some((ratio, v, g));
                    }
                }
                if let Some((_, v, g)) = best {
                    runs[i][j] = v;
                    rows[i] = g;
                    match inverse(&rows, delta) {
                        Some(m) => inv = m,
                        None => return,
                    }
                    improved = true;
                }
            }
        }
        if !improved {
            if delta > 0.0 {
                if let Some(m) = inverse(&rows, 0.0).filter(|_| cholesky(&information(&rows, 0.0)).is_some()) {
                    delta = 0.0;
                    inv = m;
                    continue;
                }
            }
            break;
        }
    }
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimal::{ModelFormula, OptimalOptions, d_optimal};

    use ScreeningFactor::{Categorical, Continuous};

    #[test]
    fn helmert_contrasts_are_orthonormal_over_balanced_levels() {
        for levels in 2..=MAX_LEVELS {
            let rows: Vec<Vec<f64>> = (0..levels).map(|l| helmert_row(l, levels)).collect();
            for a in 0..levels - 1 {
                assert!(rows.iter().map(|r| r[a]).sum::<f64>().abs() < 1e-12);
                for b in 0..levels - 1 {
                    let s: f64 = rows.iter().map(|r| r[a] * r[b]).sum();
                    let want = if a == b { levels as f64 } else { 0.0 };
                    assert!((s - want).abs() < 1e-12, "L = {levels}");
                }
            }
            for l in 0..levels {
                assert_eq!(level_of(level_code(l, levels), levels), l);
            }
        }
        // Two levels: the usual ±1 column.
        assert_eq!(helmert_row(0, 2), [-1.0]);
        assert_eq!(helmert_row(1, 2), [1.0]);
    }

    /// Orthogonal arrays exist for these mixed-level layouts, so the search
    /// must reach 100% D-efficiency: a 2 × 3 full factorial (6 runs), the
    /// 4¹2⁴ array in 8 runs (Wu & Hamada 2009, Table 7A.1, by replacing three
    /// columns of the 2⁷⁻⁴ array with a four-level factor) and 2¹3³ in 18 runs
    /// (Taguchi's L18, columns 1–4).
    #[test]
    fn reaches_orthogonal_arrays() {
        for (kinds, runs) in [
            (vec![Continuous, Categorical { levels: 3 }], 6),
            (vec![Categorical { levels: 4 }, Continuous, Continuous, Continuous, Continuous], 8),
            (vec![Continuous, Categorical { levels: 3 }, Categorical { levels: 3 }, Categorical { levels: 3 }], 18),
        ] {
            let d = categorical_screening(
                &kinds,
                &CategoricalScreeningOptions { starts: 40, ..CategoricalScreeningOptions::new(runs) },
            )
            .unwrap();
            assert!((d.d_efficiency - 100.0).abs() < 1e-6, "{kinds:?}: {}", d.d_efficiency);
            assert_eq!(d.design.n_runs(), runs);
            // Each categorical factor uses its levels equally often.
            for (j, k) in kinds.iter().enumerate() {
                if let Categorical { levels } = *k {
                    let mut counts = vec![0; levels];
                    d.design.runs.iter().for_each(|r| counts[level_of(r[j], levels)] += 1);
                    assert!(counts.iter().all(|&c| c == runs / levels), "{counts:?}");
                }
            }
        }
    }

    /// With continuous factors only, the main-effects-plus-interactions
    /// optimum is the same as the general D-optimal search finds.
    #[test]
    fn matches_the_general_d_optimal_search_for_continuous_factors() {
        for (k, n) in [(3, 10), (4, 12)] {
            let kinds = vec![Continuous; k];
            let d = categorical_screening(
                &kinds,
                &CategoricalScreeningOptions { interactions: true, starts: 30, ..CategoricalScreeningOptions::new(n) },
            )
            .unwrap();
            let general = d_optimal(
                &ModelFormula::interactions(k),
                &OptimalOptions { runs: n, starts: 30, seed: 1, max_passes: 100 },
            )
            .unwrap();
            assert!((d.log_det - general.log_det).abs() < 1e-8, "k = {k}: {} vs {}", d.log_det, general.log_det);
        }
    }

    #[test]
    fn interactions_and_validation() {
        let kinds = [Categorical { levels: 3 }, Continuous, Continuous];
        assert_eq!(model_terms(&kinds, false), 5);
        assert_eq!(model_terms(&kinds, true), 5 + 2 + 2 + 1);
        let d = categorical_screening(
            &kinds,
            &CategoricalScreeningOptions { interactions: true, ..CategoricalScreeningOptions::new(12) },
        )
        .unwrap();
        // The 3 × 2 × 2 full factorial is orthogonal for this model.
        assert!((d.d_efficiency - 100.0).abs() < 1e-6, "{}", d.d_efficiency);
        assert!(d.design.notes.iter().any(|n| n.contains("A: levels 1 to 3 run 4, 4, 4 times")));
        assert_eq!(d.design.factors[0].actual(d.design.runs[0][0]).fract(), 0.0, "actual units give the level");
        assert!(categorical_screening(&kinds, &CategoricalScreeningOptions::new(4)).is_err());
        assert!(categorical_screening(&[Categorical { levels: 1 }], &CategoricalScreeningOptions::new(4)).is_err());
        assert!(categorical_screening(&[], &CategoricalScreeningOptions::new(4)).is_err());
        assert_eq!(default_runs(&kinds, false), 12);
        assert_eq!(default_runs(&[Continuous; 5], false), 10);
    }
}
