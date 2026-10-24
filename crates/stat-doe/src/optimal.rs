//! Model formulas and D- and I-optimal designs by coordinate exchange.
//!
//! # Model formulas
//! A [`ModelFormula`] is a polynomial in the coded factors: an intercept plus
//! terms such as `A`, `A*B` and `A^2`. [`ModelFormula::parse`] reads text like
//! `"A + B + C + A*B + A^2"` (factor names as given; `*` or `:` joins factors,
//! `^n` raises a factor to a power, and `A*A` equals `A^2`). The presets
//! [`ModelFormula::main_effects`], [`ModelFormula::interactions`] (main effects
//! and all two-factor interactions) and [`ModelFormula::quadratic`] (the full
//! second-order response-surface model) cover the usual cases.
//!
//! # D-optimality
//! For `n` runs and model matrix `X` (one row `f(x)ᵀ` per run), a D-optimal
//! design maximizes `det(XᵀX)`, which minimizes the volume of the joint
//! confidence region of the coefficients. The reported **D-efficiency** is
//! `100 · det(XᵀX)^{1/p} / n` for `p` model terms, which is 100% for an
//! orthogonal two-level design (e.g. a full factorial for a model with main
//! effects and interactions).
//!
//! # I-optimality
//! An I-optimal design minimizes the average variance of the predicted mean
//! over the cuboidal region `[−1, 1]^k`, `I = tr(M (XᵀX)⁻¹)` in units of `σ²`,
//! where `M = 2^{−k} ∫ f(x) f(x)ᵀ dx` is the region's moment matrix (Box &
//! Draper 1959; Atkinson, Donev & Tobias, *Optimum Experimental Designs*, §10.6).
//! For monomials the moments are exact: `2^{−1}∫₋₁¹ xᵃ dx = 1/(a + 1)` for even
//! `a` and 0 for odd `a`. I-optimal designs suit response-surface work, where
//! the aim is prediction rather than estimating each coefficient. Each start
//! is first improved for D (which also makes it non-singular), then for I,
//! where replacing row `x` by `y` updates the trace by two Sherman–Morrison
//! steps in `O(p²)`.
//!
//! # Coordinate exchange
//! Meyer & Nachtsheim (1995), "The coordinate-exchange algorithm for
//! constructing exact optimal experimental designs", *Technometrics* 37(1):
//! starting from a random design, each coordinate (run `i`, factor `j`) in turn is
//! replaced by the candidate level that most increases `det(XᵀX)`; passes over all
//! coordinates repeat until none improves, and the best of several random starts
//! is kept. It is a heuristic: more starts make finding the global optimum
//! more likely. Candidate levels for a factor whose highest power in the model is
//! `d` are `d + 1` equally spaced values from −1 to +1 (−1, +1 for linear terms;
//! −1, 0, +1 for quadratic ones).
//!
//! Replacing row `x` by `y` multiplies the determinant by
//! `(1 + yᵀA⁻¹y)(1 − xᵀA⁻¹x) + (xᵀA⁻¹y)²` with `A = XᵀX` (Fedorov 1972,
//! *Theory of Optimal Experiments*), so each candidate costs `O(p²)`; `A⁻¹` is
//! recomputed by Cholesky factorization after every accepted change. A singular
//! starting design is handled by maximizing `det(XᵀX + δI)` with a small `δ`
//! until the design becomes non-singular.

use crate::constraints::Region;
use crate::design::{Design, Factor, PointType, Rng, default_factor_names};
use stat_core::text::superscript;
use stat_core::{Result, StatError};

/// One model term: the power of each factor (all zero for the intercept).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Term {
    /// `powers[j]` is the exponent of factor `j`.
    pub powers: Vec<u8>,
}

impl Term {
    /// Total degree.
    #[must_use]
    pub fn degree(&self) -> u32 {
        self.powers.iter().map(|&p| u32::from(p)).sum()
    }

    /// Value of the term at coded point `x`.
    #[must_use]
    pub fn eval(&self, x: &[f64]) -> f64 {
        self.powers.iter().zip(x).filter(|(p, _)| **p > 0).map(|(&p, &v)| v.powi(i32::from(p))).product()
    }

    /// Label such as `"A*B"`, `"A^2"` or `"Intercept"`.
    #[must_use]
    pub fn label(&self, names: &[String]) -> String {
        let parts: Vec<String> = self
            .powers
            .iter()
            .enumerate()
            .filter(|(_, p)| **p > 0)
            .map(
                |(j, &p)| {
                    if p == 1 { names[j].clone() } else { format!("{}{}", names[j], superscript(&p.to_string())) }
                },
            )
            .collect();
        if parts.is_empty() { "Intercept".to_owned() } else { parts.join(" × ") }
    }
}

/// A polynomial model in `k` coded factors, always with an intercept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFormula {
    k: usize,
    terms: Vec<Term>,
}

impl ModelFormula {
    fn from_terms(k: usize, extra: impl IntoIterator<Item = Vec<u8>>) -> Self {
        let mut terms = vec![Term { powers: vec![0; k] }];
        for powers in extra {
            let t = Term { powers };
            if !terms.contains(&t) {
                terms.push(t);
            }
        }
        // Intercept, then by degree, keeping first-mention order within a degree.
        let intercept = terms.remove(0);
        terms.sort_by_key(Term::degree);
        terms.insert(0, intercept);
        Self { k, terms }
    }

    fn unit(k: usize, j: usize, p: u8) -> Vec<u8> {
        let mut v = vec![0; k];
        v[j] = p;
        v
    }

    /// Intercept and main effects.
    #[must_use]
    pub fn main_effects(k: usize) -> Self {
        Self::from_terms(k, (0..k).map(|j| Self::unit(k, j, 1)))
    }

    /// Intercept, main effects and all two-factor interactions.
    #[must_use]
    pub fn interactions(k: usize) -> Self {
        let mains = (0..k).map(|j| Self::unit(k, j, 1));
        let pairs = (0..k).flat_map(|a| {
            (a + 1..k).map(move |b| {
                let mut v = vec![0; k];
                v[a] = 1;
                v[b] = 1;
                v
            })
        });
        Self::from_terms(k, mains.chain(pairs).collect::<Vec<_>>())
    }

    /// The full second-order model: main effects, two-factor interactions and
    /// pure quadratic terms.
    #[must_use]
    pub fn quadratic(k: usize) -> Self {
        let mut f = Self::interactions(k);
        let squares: Vec<Vec<u8>> = (0..k).map(|j| Self::unit(k, j, 2)).collect();
        let existing: Vec<Vec<u8>> = f.terms.drain(1..).map(|t| t.powers).collect();
        f = Self::from_terms(k, existing.into_iter().chain(squares));
        f
    }

    /// A model from explicit terms: `powers[t][j]` is the exponent of factor
    /// `j` in term `t`. The intercept is added; repeated terms are dropped;
    /// terms are ordered by degree, keeping their given order within a degree.
    #[must_use]
    pub fn from_powers(k: usize, powers: Vec<Vec<u8>>) -> Self {
        Self::from_terms(k, powers.into_iter().filter(|p| p.len() == k && p.iter().any(|&v| v > 0)))
    }

    /// Intercept and every product of up to `order` distinct factors (main
    /// effects for `order = 1`; `order ≥ k` gives the saturated two-level
    /// factorial model with `2^k` terms), ordered by degree and then
    /// lexicographically by factor.
    #[must_use]
    pub fn interactions_up_to(k: usize, order: usize) -> Self {
        let mut terms = Vec::new();
        for degree in 1..=order.min(k) {
            let mut pick: Vec<usize> = (0..degree).collect();
            loop {
                let mut v = vec![0u8; k];
                pick.iter().for_each(|&j| v[j] = 1);
                terms.push(v);
                // Next combination of `degree` factors out of `k`.
                let Some(i) = (0..degree).rev().find(|&i| pick[i] < k - degree + i) else { break };
                pick[i] += 1;
                for t in i + 1..degree {
                    pick[t] = pick[t - 1] + 1;
                }
            }
        }
        Self::from_terms(k, terms)
    }

    /// Parse a formula over the given factor names. Besides explicit terms,
    /// the whole text may be one of `main effects`, `interactions` or
    /// `quadratic` (also `response surface`).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for an unknown factor name, a bad power,
    /// or an empty term.
    pub fn parse(text: &str, names: &[String]) -> Result<Self> {
        let k = names.len();
        match text.trim().to_ascii_lowercase().as_str() {
            "main effects" | "linear" => return Ok(Self::main_effects(k)),
            "interactions" | "two-factor interactions" => return Ok(Self::interactions(k)),
            "quadratic" | "response surface" | "second order" => return Ok(Self::quadratic(k)),
            _ => {}
        }
        let mut terms = Vec::new();
        for raw in text.split('+') {
            let raw = raw.trim();
            if raw.is_empty() {
                return Err(invalid(format!("empty term in {text:?}")));
            }
            if raw == "1" {
                continue; // the intercept is always present
            }
            let mut powers = vec![0u8; k];
            for factor in raw.split(['*', ':', '×']) {
                let factor = factor.trim().replace('²', "^2").replace('³', "^3");
                let (name, power) = match factor.split_once('^') {
                    Some((n, p)) => {
                        let p: u8 = p.trim().parse().map_err(|_| invalid(format!("bad power in {raw:?}")))?;
                        if !(1..=3).contains(&p) {
                            return Err(invalid(format!("powers from 1 to 3 are supported ({raw:?})")));
                        }
                        (n.trim().to_owned(), p)
                    }
                    None => (factor.clone(), 1),
                };
                let j = names
                    .iter()
                    .position(|n| n.eq_ignore_ascii_case(&name))
                    .ok_or_else(|| invalid(format!("unknown factor {name:?} in {raw:?}")))?;
                powers[j] = powers[j].saturating_add(power);
                if powers[j] > 3 {
                    return Err(invalid(format!("powers from 1 to 3 are supported ({raw:?})")));
                }
            }
            terms.push(powers);
        }
        Ok(Self::from_terms(k, terms))
    }

    /// Number of factors.
    #[must_use]
    pub fn n_factors(&self) -> usize {
        self.k
    }

    /// The terms, intercept first.
    #[must_use]
    pub fn terms(&self) -> &[Term] {
        &self.terms
    }

    /// Number of terms `p` (including the intercept).
    #[must_use]
    pub fn n_terms(&self) -> usize {
        self.terms.len()
    }

    /// The model row `f(x)` for coded point `x`.
    #[must_use]
    pub fn expand(&self, x: &[f64]) -> Vec<f64> {
        self.terms.iter().map(|t| t.eval(x)).collect()
    }

    /// Readable formula, e.g. `"A + B + A*B"`.
    #[must_use]
    pub fn describe(&self, names: &[String]) -> String {
        let parts: Vec<String> = self.terms[1..].iter().map(|t| t.label(names)).collect();
        if parts.is_empty() { "Intercept only".to_owned() } else { parts.join(" + ") }
    }

    /// Highest power of each factor in the model.
    #[must_use]
    pub fn max_powers(&self) -> Vec<u8> {
        (0..self.k).map(|j| self.terms.iter().map(|t| t.powers[j]).max().unwrap_or(0)).collect()
    }
}

/// Options for [`d_optimal`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OptimalOptions {
    /// Number of runs (at least the number of model terms).
    pub runs: usize,
    /// Number of random starts; the best design is kept.
    pub starts: usize,
    /// Seed of the random starts.
    pub seed: u64,
    /// Most passes over all coordinates per start.
    pub max_passes: usize,
}

impl OptimalOptions {
    /// `runs` runs, 20 starts, seed 1, at most 100 passes.
    #[must_use]
    pub fn new(runs: usize) -> Self {
        Self { runs, starts: 20, seed: 1, max_passes: 100 }
    }
}

/// A D-optimal design and its quality.
#[derive(Debug, Clone, PartialEq)]
pub struct OptimalDesign {
    /// The design (point type [`PointType::Other`]).
    pub design: Design,
    /// The model it was built for.
    pub formula: ModelFormula,
    /// `ln det(XᵀX)`.
    pub log_det: f64,
    /// `100 · det(XᵀX)^{1/p} / n`.
    pub d_efficiency: f64,
    /// Average prediction variance over the region, `tr(M (XᵀX)⁻¹)` (units of σ²).
    pub average_variance: f64,
}

/// `ln det(XᵀX)` of `runs` for `formula` (−∞ if singular).
#[must_use]
pub fn log_det_information(formula: &ModelFormula, runs: &[Vec<f64>]) -> f64 {
    let a = information(formula, runs, 0.0);
    cholesky(&a).map_or(f64::NEG_INFINITY, |l| log_det_from_cholesky(&l))
}

/// D-efficiency `100 · det(XᵀX)^{1/p} / n` of `runs` for `formula`.
#[must_use]
pub fn d_efficiency(formula: &ModelFormula, runs: &[Vec<f64>]) -> f64 {
    let p = formula.n_terms() as f64;
    100.0 * (log_det_information(formula, runs) / p).exp() / runs.len() as f64
}

/// Build a D-optimal design for `formula` by coordinate exchange over the
/// whole cube `[−1, 1]^k` (see [`d_optimal_in`] for a constrained region).
///
/// # Errors
/// [`StatError::InvalidArgument`] if there are fewer runs than model terms,
/// no starts, or more than 10 000 runs; [`StatError::RankDeficient`] if no
/// start reached a non-singular design.
pub fn d_optimal(formula: &ModelFormula, options: &OptimalOptions) -> Result<OptimalDesign> {
    d_optimal_in(formula, &Region::cube(formula.n_factors()), options)
}

/// Build a D-optimal design for `formula` inside `region` by coordinate
/// exchange. Random starts are drawn inside the region; each coordinate's
/// candidates are the grid levels inside its feasible set, the set's closed
/// end points and (for a factor with a squared or higher term) its midpoint
/// ([`Region::candidates`]); a run on an active linear constraint may also
/// slide along it ([`Region::slide_directions`]), with the best step found
/// on a grid of 17 steps refined by golden-section search. Every run of the
/// result lies in the region. For the whole cube this is exactly
/// [`d_optimal`]. The average prediction variance is over the region (an
/// estimate for a constrained region, see [`region_moments_in`]; NaN if
/// the region is too small to estimate it).
///
/// # Errors
/// As [`d_optimal`]; also [`StatError::InvalidArgument`] if the region has a
/// different number of factors, or no point of the region could be found.
pub fn d_optimal_in(formula: &ModelFormula, region: &Region, options: &OptimalOptions) -> Result<OptimalDesign> {
    let (k, p, n) = (formula.n_factors(), formula.n_terms(), options.runs);
    validate(formula, region, n, options)?;
    let space = SearchSpace::new(formula, region);
    let (log_det, runs) = d_search(&space, &[], n, options)?;
    let d_eff = 100.0 * (log_det / p as f64).exp() / n as f64;
    let names = default_factor_names(k);
    let factors = names.iter().cloned().map(Factor::coded).collect();
    let mut design = Design::new(
        format!("D-optimal, {k} factors, {p}-term model{} ({n} runs)", constraint_count(region)),
        factors,
        runs,
        vec![PointType::Other; n],
    )?;
    design.notes.push(format!("Model: {}", formula.describe(&names)));
    design.notes.push(format!(
        "D-efficiency {d_eff:.2}%; ln det(XᵀX) = {log_det:.6}; best of {} coordinate-exchange starts (seed {}).",
        options.starts, options.seed
    ));
    push_constraint_notes(&mut design.notes, region);
    let average_variance =
        region_moments_in(formula, region).map_or(f64::NAN, |(m, _)| average_variance_with(formula, &design.runs, &m));
    Ok(OptimalDesign { design, formula: formula.clone(), log_det, d_efficiency: d_eff, average_variance })
}

/// Number of Halton points used to estimate the moments of a constrained
/// region ([`region_moments_in`]).
pub const MOMENT_SAMPLE: usize = 20_000;

/// Fewest points of the region the moment estimate accepts.
const MIN_MOMENT_SAMPLE: usize = 1_000;

/// Moment matrix `M` of `formula` over `[−1, 1]^k` with uniform weight.
#[must_use]
pub fn region_moments(formula: &ModelFormula) -> Vec<Vec<f64>> {
    let terms = formula.terms();
    terms
        .iter()
        .map(|a| {
            terms
                .iter()
                .map(|b| {
                    a.powers
                        .iter()
                        .zip(&b.powers)
                        .map(|(&pa, &pb)| {
                            let e = u32::from(pa) + u32::from(pb);
                            if e % 2 == 1 { 0.0 } else { 1.0 / f64::from(e + 1) }
                        })
                        .product()
                })
                .collect()
        })
        .collect()
}

/// Moment matrix `M = E[f(x) f(x)ᵀ]` of `formula` for `x` uniform over
/// `region`, and the number of sample points used (`None` when exact). For
/// the whole cube the moments are exact ([`region_moments`]); for a
/// constrained region they are **estimated** as the average of `f(x) f(x)ᵀ`
/// over the points of the region among the Halton sequence, stopping at
/// [`MOMENT_SAMPLE`] (20 000) points or after 4 000 000 draws.
///
/// # Errors
/// [`StatError::InvalidArgument`] if fewer than 1 000 points fall in the
/// region (it covers less than about 0.025% of the cube).
pub fn region_moments_in(formula: &ModelFormula, region: &Region) -> Result<(Vec<Vec<f64>>, Option<usize>)> {
    if region.is_unconstrained() {
        return Ok((region_moments(formula), None));
    }
    let points = region.quasi_random_points(MOMENT_SAMPLE);
    if points.len() < MIN_MOMENT_SAMPLE {
        return Err(invalid(
            "the constrained region is too small to estimate its average prediction variance (fewer than 1 000 of \
             4 000 000 quasi-random points fall inside it)",
        ));
    }
    let rows: Vec<Vec<f64>> = points.iter().map(|x| formula.expand(x)).collect();
    let mut m = information_from_rows(rows.iter(), 0.0);
    let count = points.len() as f64;
    m.iter_mut().flatten().for_each(|v| *v /= count);
    Ok((m, Some(points.len())))
}

/// Average prediction variance `tr(M (XᵀX)⁻¹)` of `runs` for `formula` over
/// `[−1, 1]^k` (∞ if `XᵀX` is singular).
#[must_use]
pub fn average_prediction_variance(formula: &ModelFormula, runs: &[Vec<f64>]) -> f64 {
    average_variance_with(formula, runs, &region_moments(formula))
}

/// `tr(M (XᵀX)⁻¹)` for a given moment matrix (∞ if singular).
fn average_variance_with(formula: &ModelFormula, runs: &[Vec<f64>], m: &[Vec<f64>]) -> f64 {
    invert(&information(formula, runs, 0.0)).map_or(f64::INFINITY, |inv| trace_product(m, &inv))
}

pub(crate) fn trace_product(m: &[Vec<f64>], b: &[Vec<f64>]) -> f64 {
    m.iter().enumerate().map(|(i, row)| row.iter().enumerate().map(|(j, v)| v * b[j][i]).sum::<f64>()).sum()
}

/// Build an I-optimal design for `formula` by coordinate exchange over the
/// whole cube (see [`i_optimal_in`] for a constrained region).
///
/// # Errors
/// As [`d_optimal`].
pub fn i_optimal(formula: &ModelFormula, options: &OptimalOptions) -> Result<OptimalDesign> {
    i_optimal_in(formula, &Region::cube(formula.n_factors()), options)
}

/// Build an I-optimal design for `formula` inside `region`: as
/// [`d_optimal_in`], with each start improved for D and then for the
/// average prediction variance over the region, whose moment matrix is
/// exact for the cube and estimated from quasi-random points of a
/// constrained region ([`region_moments_in`]).
///
/// # Errors
/// As [`d_optimal_in`], and if the region is too small to estimate its
/// moments.
pub fn i_optimal_in(formula: &ModelFormula, region: &Region, options: &OptimalOptions) -> Result<OptimalDesign> {
    let (k, p, n) = (formula.n_factors(), formula.n_terms(), options.runs);
    validate(formula, region, n, options)?;
    let space = SearchSpace::new(formula, region);
    let (moments, sample) = region_moments_in(formula, region)?;
    let mut rng = Rng::new(options.seed);
    let mut best: Option<(f64, Vec<Vec<f64>>)> = None;
    for _ in 0..options.starts {
        let mut runs = space.random_runs(n, &mut rng)?;
        exchange(&space, Criterion::D, &[], &mut runs, options.max_passes);
        exchange(&space, Criterion::I(&moments), &[], &mut runs, options.max_passes);
        let iv = average_variance_with(formula, &runs, &moments);
        if iv.is_finite() && best.as_ref().is_none_or(|(b, _)| iv < *b) {
            best = Some((iv, runs));
        }
    }
    let (average_variance, runs) = best.ok_or(StatError::RankDeficient { rank: 0, columns: p })?;
    let log_det = log_det_information(formula, &runs);
    let d_eff = 100.0 * (log_det / p as f64).exp() / n as f64;
    let names = default_factor_names(k);
    let factors = names.iter().cloned().map(Factor::coded).collect();
    let mut design = Design::new(
        format!("I-optimal, {k} factors, {p}-term model{} ({n} runs)", constraint_count(region)),
        factors,
        runs,
        vec![PointType::Other; n],
    )?;
    design.notes.push(format!("Model: {}", formula.describe(&names)));
    let over = match sample {
        None => "over the cube".to_owned(),
        Some(s) => format!("over the constrained region (estimated from {s} quasi-random points)"),
    };
    design.notes.push(format!(
        "Average prediction variance {average_variance:.6} σ² {over}; D-efficiency {d_eff:.2}%; best of {} \
         coordinate-exchange starts (seed {}).",
        options.starts, options.seed
    ));
    push_constraint_notes(&mut design.notes, region);
    Ok(OptimalDesign { design, formula: formula.clone(), log_det, d_efficiency: d_eff, average_variance })
}

fn constraint_count(region: &Region) -> String {
    match region.describe().len() {
        0 => String::new(),
        1 => ", 1 constraint".to_owned(),
        c => format!(", {c} constraints"),
    }
}

fn push_constraint_notes(notes: &mut Vec<String>, region: &Region) {
    if !region.describe().is_empty() {
        notes.push(format!("Constraints (every run satisfies them): {}.", region.describe().join("; ")));
    }
}

pub(crate) fn validate(
    formula: &ModelFormula,
    region: &Region,
    total_runs: usize,
    options: &OptimalOptions,
) -> Result<()> {
    let (k, p) = (formula.n_factors(), formula.n_terms());
    if k == 0 {
        return Err(invalid("a design needs at least one factor"));
    }
    if region.n_factors() != k {
        return Err(invalid(format!("the region has {} factors but the model {k}", region.n_factors())));
    }
    if total_runs < p {
        return Err(invalid(format!("the model has {p} terms, so at least {p} runs are needed (got {total_runs})")));
    }
    if options.starts == 0 || total_runs > 10_000 {
        return Err(invalid("use at least one start and at most 10 000 runs"));
    }
    Ok(())
}

/// The best of `options.starts` D-criterion coordinate-exchange searches
/// for `n` runs added to the fixed coded runs `fixed`: `ln det` of the
/// information of all runs, and the new runs.
pub(crate) fn d_search(
    space: &SearchSpace<'_>,
    fixed: &[Vec<f64>],
    n: usize,
    options: &OptimalOptions,
) -> Result<(f64, Vec<Vec<f64>>)> {
    let fixed_rows: Vec<Vec<f64>> = fixed.iter().map(|r| space.formula.expand(r)).collect();
    let mut rng = Rng::new(options.seed);
    let mut best: Option<(f64, Vec<Vec<f64>>)> = None;
    for _ in 0..options.starts {
        let mut runs = space.random_runs(n, &mut rng)?;
        exchange(space, Criterion::D, &fixed_rows, &mut runs, options.max_passes);
        let all: Vec<Vec<f64>> = fixed.iter().chain(&runs).cloned().collect();
        let ld = log_det_information(space.formula, &all);
        if ld.is_finite() && best.as_ref().is_none_or(|(b, _)| ld > *b) {
            best = Some((ld, runs));
        }
    }
    best.ok_or(StatError::RankDeficient { rank: 0, columns: space.formula.n_terms() })
}

/// Where coordinate exchange may place runs: the model's grid levels and a
/// region.
pub(crate) struct SearchSpace<'a> {
    /// The model.
    pub formula: &'a ModelFormula,
    /// The region.
    pub region: &'a Region,
    /// Grid levels per factor.
    levels: Vec<Vec<f64>>,
    /// Whether a factor's feasible midpoints are candidates (squared terms).
    midpoints: Vec<bool>,
}

impl<'a> SearchSpace<'a> {
    pub(crate) fn new(formula: &'a ModelFormula, region: &'a Region) -> Self {
        Self {
            formula,
            region,
            levels: candidate_levels(formula),
            midpoints: formula.max_powers().iter().map(|&d| d >= 2).collect(),
        }
    }

    fn candidates(&self, point: &[f64], j: usize) -> Vec<f64> {
        if self.region.is_unconstrained() {
            self.levels[j].clone()
        } else {
            self.region.candidates(point, j, &self.levels[j], self.midpoints[j])
        }
    }

    /// A random grid point (up to 50 tries inside the region), else a random
    /// point of the region.
    fn random_run(&self, rng: &mut Rng) -> Result<Vec<f64>> {
        let grid = |rng: &mut Rng| -> Vec<f64> { self.levels.iter().map(|l| l[rng.below(l.len())]).collect() };
        if self.region.is_unconstrained() {
            return Ok(grid(rng));
        }
        for _ in 0..50 {
            let x = grid(rng);
            if self.region.contains(&x) {
                return Ok(x);
            }
        }
        self.region.random_point(rng)
    }

    fn random_runs(&self, n: usize, rng: &mut Rng) -> Result<Vec<Vec<f64>>> {
        (0..n).map(|_| self.random_run(rng)).collect()
    }
}

/// Candidate levels per factor: `d + 1` equally spaced values in [−1, 1] for a
/// factor of highest power `d`.
pub(crate) fn candidate_levels(formula: &ModelFormula) -> Vec<Vec<f64>> {
    formula
        .max_powers()
        .iter()
        .map(|&d| {
            let m = usize::from(d.max(1)) + 1;
            (0..m).map(|i| -1.0 + 2.0 * i as f64 / (m - 1) as f64).collect()
        })
        .collect()
}

/// The exchange criterion.
#[derive(Clone, Copy)]
pub(crate) enum Criterion<'m> {
    /// Maximize `det(XᵀX)`.
    D,
    /// Minimize `tr(M (XᵀX)⁻¹)` for the moment matrix `M`.
    I(&'m [Vec<f64>]),
}

impl Criterion<'_> {
    /// Score of replacing model row `x` by `y` (larger is better), given
    /// `A⁻¹`, `A⁻¹x` and `xᵀA⁻¹x`; `None` if the swap makes `A` singular.
    /// D: the determinant ratio `(1 + yᵀA⁻¹y)(1 − xᵀA⁻¹x) + (xᵀA⁻¹y)²`.
    /// I: minus the change in the trace, by two Sherman–Morrison steps.
    fn score(self, a_inv: &[Vec<f64>], x: &[f64], ax: &[f64], dx: f64, y: &[f64]) -> Option<f64> {
        match self {
            Self::D => {
                let ay = mat_vec(a_inv, y);
                let (dy, dxy) = (dot(y, &ay), dot(x, &ay));
                Some((1.0 + dy) * (1.0 - dx) + dxy * dxy)
            }
            Self::I(moments) => {
                let quad = |u: &[f64]| dot(u, &mat_vec(moments, u));
                // Add y: A₁⁻¹ = A⁻¹ + c u uᵀ, u = A⁻¹y, c = −1/(1 + yᵀu).
                let u = mat_vec(a_inv, y);
                let c = -1.0 / (1.0 + dot(y, &u));
                // Remove x: A₂⁻¹ = A₁⁻¹ + d w wᵀ, w = A₁⁻¹x, d = 1/(1 − xᵀw).
                let ux = dot(&u, x);
                let w: Vec<f64> = ax.iter().zip(&u).map(|(a, b)| a + c * b * ux).collect();
                let denom = 1.0 - dot(x, &w);
                if denom <= 1e-10 {
                    return None; // the swap would make the design singular
                }
                Some(-(c * quad(&u) + quad(&w) / denom))
            }
        }
    }

    /// The score a change must beat, and the margin it must beat it by: D
    /// needs a ratio above 1 + 10⁻⁹, I a trace reduction above 10⁻¹².
    fn threshold(self) -> (f64, f64) {
        match self {
            Self::D => (1.0 + 1e-9, 0.0),
            Self::I(_) => (0.0, 1e-12),
        }
    }
}

/// The best change offered so far.
struct Best {
    score: f64,
    margin: f64,
    change: Option<(Vec<f64>, Vec<f64>)>,
}

impl Best {
    fn new(criterion: Criterion<'_>) -> Self {
        let (score, margin) = criterion.threshold();
        Self { score, margin, change: None }
    }

    fn offer(&mut self, score: Option<f64>, point: Vec<f64>, row: Vec<f64>) {
        if let Some(s) = score
            && s > self.score + self.margin
        {
            self.score = s;
            self.change = Some((point, row));
        }
    }
}

/// One coordinate-exchange search from `runs`, improving it in place for
/// `criterion`, with the model rows `fixed` of runs that stay as they are
/// (augmentation) counted in the information matrix. Under D a singular
/// start is handled with a ridge `δI`, `δ = 10⁻⁶ · runs`, dropped once the
/// design is non-singular; under I the start must be non-singular.
pub(crate) fn exchange(
    space: &SearchSpace<'_>,
    criterion: Criterion<'_>,
    fixed: &[Vec<f64>],
    runs: &mut [Vec<f64>],
    max_passes: usize,
) {
    let formula = space.formula;
    let n_total = fixed.len() + runs.len();
    let mut rows: Vec<Vec<f64>> = runs.iter().map(|r| formula.expand(r)).collect();
    let info = |rows: &[Vec<f64>], delta: f64| information_from_rows(fixed.iter().chain(rows), delta);
    let mut delta = 0.0;
    let mut inverse = invert(&info(&rows, delta));
    if inverse.is_none() {
        if matches!(criterion, Criterion::I(_)) {
            return;
        }
        delta = 1e-6 * n_total as f64;
        inverse = invert(&info(&rows, delta));
    }
    let Some(mut a_inv) = inverse else { return };
    let slides = !space.region.linear().is_empty();
    for _ in 0..max_passes {
        let mut improved = false;
        for i in 0..runs.len() {
            for j in 0..runs[i].len() {
                let current = runs[i][j];
                let x = &rows[i];
                let ax = mat_vec(&a_inv, x);
                let dx = dot(x, &ax);
                let mut best = Best::new(criterion);
                for v in space.candidates(&runs[i], j) {
                    if v == current {
                        continue;
                    }
                    let mut point = runs[i].clone();
                    point[j] = v;
                    let y = formula.expand(&point);
                    best.offer(criterion.score(&a_inv, x, &ax, dx, &y), point, y);
                }
                if let Some((point, y)) = best.change {
                    runs[i] = point;
                    rows[i] = y;
                    improved = true;
                    match invert(&info(&rows, delta)) {
                        Some(inv) => a_inv = inv,
                        None => return,
                    }
                }
            }
            if slides && let Some((point, y)) = best_slide(space, criterion, &a_inv, &rows[i], &runs[i]) {
                runs[i] = point;
                rows[i] = y;
                improved = true;
                match invert(&info(&rows, delta)) {
                    Some(inv) => a_inv = inv,
                    None => return,
                }
            }
        }
        if !improved {
            if delta > 0.0 {
                // Drop the ridge once the design is non-singular, then continue.
                if let Some(inv) = invert(&info(&rows, 0.0)) {
                    delta = 0.0;
                    a_inv = inv;
                    continue;
                }
            }
            break;
        }
    }
}

/// The best move of `run` along an active linear constraint, if it improves
/// the criterion: for each slide direction the score is evaluated at 17
/// evenly spaced steps and refined by golden-section search around the best
/// one; a step whose new coordinate is a multiple of 10⁻⁶ is preferred when
/// it scores within 10⁻¹² (relative) of the refined step.
fn best_slide(
    space: &SearchSpace<'_>,
    criterion: Criterion<'_>,
    a_inv: &[Vec<f64>],
    x: &[f64],
    run: &[f64],
) -> Option<(Vec<f64>, Vec<f64>)> {
    let ax = mat_vec(a_inv, x);
    let dx = dot(x, &ax);
    let mut best = Best::new(criterion);
    for slide in space.region.slide_directions(run) {
        let point_at = |t: f64| -> Vec<f64> {
            run.iter().zip(&slide.direction).map(|(v, d)| (v + t * d).clamp(-1.0, 1.0)).collect()
        };
        let eval = |t: f64| -> Option<(f64, Vec<f64>, Vec<f64>)> {
            let point = point_at(t);
            if !space.region.contains(&point) {
                return None;
            }
            let y = space.formula.expand(&point);
            criterion.score(a_inv, x, &ax, dx, &y).map(|s| (s, point, y))
        };
        let score = |t: f64| eval(t).map_or(f64::NEG_INFINITY, |e| e.0);
        const STEPS: usize = 16;
        let (t0, t1) = (slide.t_min, slide.t_max);
        let grid: Vec<f64> = (0..=STEPS).map(|s| t0 + (t1 - t0) * s as f64 / STEPS as f64).collect();
        let scores: Vec<f64> = grid.iter().map(|&t| score(t)).collect();
        let Some(ib) = (0..=STEPS).filter(|&s| scores[s].is_finite()).max_by(|&a, &b| scores[a].total_cmp(&scores[b]))
        else {
            continue;
        };
        // Golden-section search on the bracket around the best grid step.
        let (mut lo, mut hi) = (grid[ib.saturating_sub(1)], grid[(ib + 1).min(STEPS)]);
        let g = (5f64.sqrt() - 1.0) / 2.0;
        let (mut c, mut d) = (hi - g * (hi - lo), lo + g * (hi - lo));
        let (mut fc, mut fd) = (score(c), score(d));
        for _ in 0..80 {
            if fc >= fd {
                hi = d;
                d = c;
                fd = fc;
                c = hi - g * (hi - lo);
                fc = score(c);
            } else {
                lo = c;
                c = d;
                fc = fd;
                d = lo + g * (hi - lo);
                fd = score(d);
            }
        }
        let refined = if fc.max(fd) > scores[ib] { if fc >= fd { c } else { d } } else { grid[ib] };
        let fr = score(refined);
        // Prefer a round value of the moved coordinate.
        let j = slide.direction.iter().position(|&v| v == 1.0).unwrap_or(0);
        let snapped = ((run[j] + refined) * 1e6).round() / 1e6 - run[j];
        let t = if score(snapped) >= fr - 1e-12 * fr.abs().max(1.0) { snapped } else { refined };
        if t.abs() > 1e-12
            && let Some((s, point, y)) = eval(t)
        {
            best.offer(Some(s), point, y);
        }
    }
    best.change
}

fn information(formula: &ModelFormula, runs: &[Vec<f64>], delta: f64) -> Vec<Vec<f64>> {
    let rows: Vec<Vec<f64>> = runs.iter().map(|r| formula.expand(r)).collect();
    information_from_rows(&rows, delta)
}

/// `XᵀX + δI`.
fn information_from_rows<'r>(rows: impl IntoIterator<Item = &'r Vec<f64>>, delta: f64) -> Vec<Vec<f64>> {
    let mut rows = rows.into_iter().peekable();
    let p = rows.peek().map_or(0, |r| r.len());
    let mut a = vec![vec![0.0; p]; p];
    for r in rows {
        for i in 0..p {
            for j in 0..=i {
                a[i][j] += r[i] * r[j];
            }
        }
    }
    for i in 0..p {
        a[i][i] += delta;
        for j in 0..i {
            a[j][i] = a[i][j];
        }
    }
    a
}

/// Lower Cholesky factor of a symmetric matrix, or `None` unless it is
/// numerically positive definite.
pub(crate) fn cholesky(a: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let p = a.len();
    let scale = (0..p).map(|i| a[i][i].abs()).fold(0.0, f64::max).max(f64::MIN_POSITIVE);
    let mut l = vec![vec![0.0; p]; p];
    for i in 0..p {
        for j in 0..=i {
            let s: f64 = a[i][j] - (0..j).map(|m| l[i][m] * l[j][m]).sum::<f64>();
            if i == j {
                if s <= 1e-10 * scale {
                    return None;
                }
                l[i][i] = s.sqrt();
            } else {
                l[i][j] = s / l[j][j];
            }
        }
    }
    Some(l)
}

pub(crate) fn log_det_from_cholesky(l: &[Vec<f64>]) -> f64 {
    2.0 * l.iter().enumerate().map(|(i, r)| r[i].ln()).sum::<f64>()
}

/// Inverse of a symmetric positive definite matrix via Cholesky.
pub(crate) fn invert(a: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let l = cholesky(a)?;
    let p = a.len();
    let mut inv = vec![vec![0.0; p]; p];
    for col in 0..p {
        // Solve L z = e_col, then Lᵀ x = z.
        let mut z = vec![0.0; p];
        for i in 0..p {
            let e = if i == col { 1.0 } else { 0.0 };
            z[i] = (e - (0..i).map(|m| l[i][m] * z[m]).sum::<f64>()) / l[i][i];
        }
        let mut x = vec![0.0; p];
        for i in (0..p).rev() {
            x[i] = (z[i] - (i + 1..p).map(|m| l[m][i] * x[m]).sum::<f64>()) / l[i][i];
        }
        for i in 0..p {
            inv[i][col] = x[i];
        }
    }
    Some(inv)
}

fn mat_vec(a: &[Vec<f64>], x: &[f64]) -> Vec<f64> {
    a.iter().map(|r| dot(r, x)).collect()
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factorial::three_level_factorial;
    use crate::response_surface::box_behnken;

    fn names(k: usize) -> Vec<String> {
        default_factor_names(k)
    }

    #[test]
    fn formulas_parse_and_expand() {
        let n = names(3);
        let f = ModelFormula::parse("A + B + A*B + C^2 + B:A", &n).unwrap();
        assert_eq!(f.n_terms(), 5, "B:A duplicates A*B");
        assert_eq!(f.describe(&n), "A + B + A × B + C²");
        assert_eq!(f.expand(&[2.0, 3.0, -1.0]), [1.0, 2.0, 3.0, 6.0, 1.0]);
        assert_eq!(ModelFormula::parse("a*a", &n).unwrap().describe(&n), "A²");
        assert_eq!(ModelFormula::parse("A + C²", &n).unwrap().max_powers(), [1, 0, 2]);
        let shown = f.describe(&n);
        assert_eq!(ModelFormula::parse(&shown, &n).unwrap(), f, "the description parses back");
        assert!(ModelFormula::parse("A + Q", &n).is_err());
        assert!(ModelFormula::parse("A + + B", &n).is_err());
        assert!(ModelFormula::parse("A^4", &n).is_err());
        assert_eq!(ModelFormula::main_effects(3).n_terms(), 4);
        assert_eq!(ModelFormula::interactions(3).n_terms(), 7);
        assert_eq!(ModelFormula::quadratic(3).n_terms(), 10);
        assert_eq!(ModelFormula::parse("Quadratic", &n).unwrap(), ModelFormula::quadratic(3));
        assert_eq!(ModelFormula::quadratic(2).describe(&names(2)), "A + B + A × B + A² + B²", "ordered by degree");
    }

    /// Where an orthogonal two-level design exists, coordinate exchange finds
    /// one: 100% D-efficiency.
    #[test]
    fn finds_orthogonal_designs() {
        let f = ModelFormula::main_effects(7);
        let d = d_optimal(&f, &OptimalOptions::new(8)).unwrap();
        assert!((d.d_efficiency - 100.0).abs() < 1e-9, "{}", d.d_efficiency);
        let f = ModelFormula::interactions(3);
        let d = d_optimal(&f, &OptimalOptions::new(8)).unwrap();
        assert!((d.d_efficiency - 100.0).abs() < 1e-9);
        let mut runs = d.design.runs.clone();
        runs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        runs.dedup();
        assert_eq!(runs.len(), 8, "the 2³ factorial");
        // Coordinate exchange is a heuristic: for 11 factors in 12 runs it gets
        // close to (but need not reach) the Plackett–Burman optimum.
        let f = ModelFormula::main_effects(11);
        let d = d_optimal(&f, &OptimalOptions::new(12)).unwrap();
        assert!(d.d_efficiency > 95.0, "{}", d.d_efficiency);
    }

    #[test]
    fn quadratic_designs_beat_classical_designs_of_the_same_size() {
        let f = ModelFormula::quadratic(2);
        let classical = log_det_information(&f, &three_level_factorial(2).unwrap().runs);
        let d = d_optimal(&f, &OptimalOptions::new(9)).unwrap();
        assert!(d.log_det >= classical - 1e-9, "{} vs 3² factorial {classical}", d.log_det);
        assert!(d.design.runs.iter().flatten().all(|v| [-1.0, 0.0, 1.0].contains(v)));
        let f = ModelFormula::quadratic(3);
        let bbd = box_behnken(3, Some(3)).unwrap();
        let d = d_optimal(&f, &OptimalOptions::new(bbd.n_runs())).unwrap();
        assert!(d.log_det > log_det_information(&f, &bbd.runs), "D-optimal beats the Box–Behnken on D");
        // Saturated designs work too.
        let f = ModelFormula::quadratic(4);
        let d = d_optimal(&f, &OptimalOptions::new(15)).unwrap();
        assert!(d.log_det.is_finite());
        assert_eq!(d.design.n_runs(), 15);
    }

    #[test]
    fn deterministic_and_validated() {
        let f = ModelFormula::interactions(4);
        let a = d_optimal(&f, &OptimalOptions::new(12)).unwrap();
        let b = d_optimal(&f, &OptimalOptions::new(12)).unwrap();
        assert_eq!(a.design.runs, b.design.runs, "same seed, same design");
        assert!(a.design.notes[1].starts_with("D-efficiency"));
        assert!(d_optimal(&f, &OptimalOptions::new(10)).is_err(), "11 terms need 11 runs");
        assert!(d_optimal(&f, &OptimalOptions { starts: 0, ..OptimalOptions::new(12) }).is_err());
        // The determinant-update formula agrees with direct recomputation.
        let eff = d_efficiency(&f, &a.design.runs);
        assert!((eff - a.d_efficiency).abs() < 1e-9);
    }

    #[test]
    fn linear_algebra_helpers() {
        let a = vec![vec![4.0, 2.0], vec![2.0, 3.0]];
        let inv = invert(&a).unwrap();
        assert!((inv[0][0] - 3.0 / 8.0).abs() < 1e-15 && (inv[0][1] + 0.25).abs() < 1e-15);
        assert!((log_det_from_cholesky(&cholesky(&a).unwrap()) - 8f64.ln()).abs() < 1e-14);
        assert!(cholesky(&[vec![1.0, 1.0], vec![1.0, 1.0]]).is_none());
    }

    #[test]
    fn i_criterion_and_moments() {
        let f = ModelFormula::quadratic(2);
        // Moments: E[x²] = 1/3, E[x⁴] = 1/5, odd moments 0.
        let m = region_moments(&f);
        assert!((m[0][0] - 1.0).abs() < 1e-15 && (m[1][1] - 1.0 / 3.0).abs() < 1e-15);
        assert!((m[4][4] - 0.2).abs() < 1e-15 && (m[4][5] - 1.0 / 9.0).abs() < 1e-15 && m[0][1] == 0.0);
        // 3² factorial: I = 0.45 (NumPy with Gauss–Legendre moments).
        let grid: Vec<Vec<f64>> = [-1.0, 0.0, 1.0].iter().flat_map(|&a| [-1.0, 0.0, 1.0].map(|b| vec![a, b])).collect();
        assert!((average_prediction_variance(&f, &grid) - 0.45).abs() < 1e-12);
        assert!(average_prediction_variance(&f, &grid[..3]).is_infinite());
    }

    #[test]
    fn i_optimal_reaches_the_exhaustive_optimum() {
        // One factor, quadratic model, 4 runs on {−1, 0, 1}: the exhaustive
        // optimum (I = 8/15) replicates the centre, unlike the D-optimal design.
        let f1 = ModelFormula::quadratic(1);
        let d = i_optimal(&f1, &OptimalOptions::new(4)).unwrap();
        assert!((d.average_variance - 8.0 / 15.0).abs() < 1e-12, "{}", d.average_variance);
        let mut levels: Vec<f64> = d.design.runs.iter().map(|r| r[0]).collect();
        levels.sort_by(f64::total_cmp);
        assert_eq!(levels, [-1.0, 0.0, 0.0, 1.0]);
        // Two factors, quadratic model: exhaustive optima over the 3² grid are
        // 0.96667 (6 runs) and 0.66556 (7 runs), from NumPy.
        let f2 = ModelFormula::quadratic(2);
        for (n, best) in [(6, 0.966_666_666_666_667), (7, 0.665_555_555_555_556)] {
            let d = i_optimal(&f2, &OptimalOptions::new(n)).unwrap();
            assert!((d.average_variance - best).abs() < 1e-9, "{n} runs: {}", d.average_variance);
            // At least as good for prediction as the D-optimal design of the same size.
            let dopt = d_optimal(&f2, &OptimalOptions::new(n)).unwrap();
            assert!(d.average_variance <= dopt.average_variance + 1e-12);
        }
        assert!(i_optimal(&f2, &OptimalOptions::new(5)).is_err(), "fewer runs than terms");
    }

    /// The A + B ≤ 0.5 triangle-cut square: coordinate exchange with slides
    /// along the constraint matches the exhaustive optimum
    /// (`tests/data/optimal_reference.py`, case 1), whose best run off the
    /// corners is (0.25, 0.25) on the slanted edge.
    #[test]
    fn constrained_d_optimal_matches_exhaustive_search() {
        let f = ModelFormula::interactions(2);
        let mut region = Region::cube(2);
        region.add_linear(vec![1.0, 1.0], 0.5, "A + B ≤ 0.5").unwrap();
        for (n, reference) in [(4, 3.665_162_927_496_62), (5, 4.358_310_108_056_566), (6, 5.051_457_288_616_512)] {
            let d = d_optimal_in(&f, &region, &OptimalOptions::new(n)).unwrap();
            assert!((d.log_det - reference).abs() < 1e-9, "{n} runs: {} vs {reference}", d.log_det);
            assert!(d.design.runs.iter().all(|r| region.contains(r)), "{:?}", d.design.runs);
            assert!(d.design.runs.iter().any(|r| (r[0] - 0.25).abs() < 1e-9 && (r[1] - 0.25).abs() < 1e-9));
            assert!(
                d.design.notes.iter().any(|n| n.starts_with("Constraints (every run satisfies them): A + B ≤ 0.5"))
            );
            assert!(d.design.description.contains("1 constraint"));
        }
        // The same constraint in actual units.
        let factors = [Factor::new("Temp", 100.0, 200.0), Factor::new("Time", 10.0, 20.0)];
        let parsed = Region::parse("Temp/50 + Time/5 <= 6.5", &factors).unwrap();
        let d = d_optimal_in(&f, &parsed, &OptimalOptions::new(4)).unwrap();
        assert!((d.log_det - 3.665_162_927_496_62).abs() < 1e-9);
    }

    #[test]
    fn disallowed_combinations_and_empty_regions() {
        use crate::constraints::{Comparison, Condition};
        // Exclude the (+, +) corner: the 2² factorial is impossible, and the
        // corner never appears.
        let f = ModelFormula::interactions(2);
        let mut region = Region::cube(2);
        let corner = vec![
            Condition { factor: 0, comparison: Comparison::Greater, value: 0.5 },
            Condition { factor: 1, comparison: Comparison::Greater, value: 0.5 },
        ];
        region.add_exclusion(corner, "Exclude A > 0.5 and B > 0.5").unwrap();
        let d = d_optimal_in(&f, &region, &OptimalOptions::new(8)).unwrap();
        assert!(d.design.runs.iter().all(|r| !(r[0] > 0.5 && r[1] > 0.5)), "{:?}", d.design.runs);
        assert!(d.design.runs.iter().any(|r| r == &[1.0, 0.5] || r == &[0.5, 1.0]), "uses the corner's edges");
        assert!(d.log_det.is_finite());
        // I-optimal over the same region.
        let q = ModelFormula::quadratic(2);
        let i = i_optimal_in(&q, &region, &OptimalOptions { starts: 5, ..OptimalOptions::new(9) }).unwrap();
        assert!(i.design.runs.iter().all(|r| region.contains(r)));
        assert!(i.average_variance.is_finite() && i.average_variance > 0.0);
        assert!(i.design.notes[1].contains("estimated from 20000 quasi-random points"), "{}", i.design.notes[1]);
        // An empty region is reported.
        let mut empty = Region::cube(2);
        empty.add_linear(vec![1.0, 1.0], -2.5, "A + B ≤ −2.5").unwrap();
        let err = d_optimal_in(&f, &empty, &OptimalOptions::new(4)).unwrap_err().to_string();
        assert!(err.contains("no room"), "{err}");
        assert!(i_optimal_in(&q, &empty, &OptimalOptions::new(9)).is_err());
        assert!(d_optimal_in(&f, &Region::cube(3), &OptimalOptions::new(4)).is_err(), "wrong dimension");
    }

    /// Unconstrained entry points give exactly the designs of the plain ones.
    #[test]
    fn unconstrained_region_changes_nothing() {
        let f = ModelFormula::quadratic(3);
        let o = OptimalOptions { starts: 4, ..OptimalOptions::new(14) };
        assert_eq!(d_optimal(&f, &o).unwrap(), d_optimal_in(&f, &Region::cube(3), &o).unwrap());
        assert_eq!(i_optimal(&f, &o).unwrap(), i_optimal_in(&f, &Region::cube(3), &o).unwrap());
        let (m, sample) = region_moments_in(&f, &Region::cube(3)).unwrap();
        assert_eq!((m, sample), (region_moments(&f), None));
    }

    /// Estimated moments of the half cube A ≤ 0 against the exact values
    /// E[A] = −½, E[A²] = ⅓, E[A·B] = 0, E[A²B²] = 1/9.
    #[test]
    fn sampled_region_moments() {
        let f = ModelFormula::parse("A + B + A*B + A^2", &names(2)).unwrap();
        let mut half = Region::cube(2);
        half.add_linear(vec![1.0, 0.0], 0.0, "A ≤ 0").unwrap();
        let (m, sample) = region_moments_in(&f, &half).unwrap();
        assert_eq!(sample, Some(MOMENT_SAMPLE));
        assert!((m[0][0] - 1.0).abs() < 1e-15);
        assert!((m[0][1] + 0.5).abs() < 1e-3, "{}", m[0][1]);
        assert!((m[1][1] - 1.0 / 3.0).abs() < 1e-3);
        assert!(m[1][2].abs() < 1e-3);
        assert!((m[3][3] - 1.0 / 9.0).abs() < 1e-3);
    }
}
