//! Choosing the next runs of an experiment from Gaussian-process models
//! ([`crate::gaussian_process`]) of its responses: sequential design by
//! expected improvement or an upper confidence bound.
//!
//! **Goals.** Each response is maximized, minimized, or brought to a target
//! `T`. Its *score* is `y`, `−y` or `−|y − T|`, so the best run has the
//! largest score.
//!
//! **Expected improvement** (Jones, Schonlau & Welch, "Efficient global
//! optimization of expensive black-box functions", *J. Global Optimization*
//! 13, 1998, eq. 15). With the predicted mean `μ` and standard deviation `σ`
//! of the latent response at a point and the best observed value `y*`
//! (largest when maximizing), `z = (μ − y*)/σ` and
//!
//! ```text
//! EI = (μ − y*) Φ(z) + σ φ(z)
//! ```
//!
//! (mirrored when minimizing). For a target, the improvement is
//! `max(0, b − |Y − T|)` with `b` the smallest observed distance from the
//! target; integrating it against `Y ~ N(μ, σ²)` over `(T − b, T + b)` gives
//! a closed form in Φ and φ (see [`expected_improvement`]).
//!
//! **Upper confidence bound** (Srinivas, Krause, Kakade & Seeger, "Gaussian
//! process optimization in the bandit setting", *ICML* 2010): the most
//! optimistic score within `κ` standard deviations, `μ + κσ` when
//! maximizing, `−(μ − κσ)` when minimizing, `−max(0, |μ − T| − κσ)` for a
//! target.
//!
//! **Several responses** are combined by an overall desirability `D(y₁, …)`
//! supplied by the caller (Derringer & Suich 1980). Its expected improvement
//! over the best observed `D*` is estimated by Monte Carlo over independent
//! draws of the responses' predictive distributions, with the same draws at
//! every point (common random numbers, so the criterion is a smooth
//! function of the point): the multivariate expected-improvement criterion
//! of Henkenjohann & Kunert ("An efficient sequential optimization approach
//! based on the multivariate expected improvement criterion", *Quality
//! Engineering* 19, 2007). The upper confidence bound uses each response's
//! most favourable value within `κ` standard deviations (for a target, the
//! value in `μ ± κσ` nearest it).
//!
//! **Batches** use the constant liar (Ginsbourger, Le Riche & Carraro,
//! "Kriging is well-suited to parallelize optimization", 2010): after a
//! point is chosen, each model is conditioned on a pretend observation
//! there equal to the response at the best run so far, with the same
//! hyperparameters, and the next point is chosen; the lie removes the
//! chosen point's improvement so the next one goes elsewhere. Runs already
//! in the table but not yet measured (*pending*) are lied about first.
//!
//! **Search.** Continuous factors live in the unit cube, cut by a
//! feasibility test (linear constraints); categorical factors take one of
//! their levels and enter the models as one 0/1 indicator per level. The
//! criterion is evaluated at Latin-hypercube candidates (rejecting
//! infeasible ones) with random levels; the best few are polished by
//! Nelder–Mead in the continuous factors and by trying each other level of
//! each categorical factor. Where expected improvement underflows to zero
//! everywhere the point of largest predictive spread is taken.

use rayon::prelude::*;
use stat_core::optimize::{latin_hypercube, minimize_in_box};
use stat_core::rng::Rng;
use stat_core::special::erfc;
use stat_core::{Result, StatError};

use crate::gaussian_process::GaussianProcess;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// What to do with a response.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Goal {
    /// Larger is better.
    Maximize,
    /// Smaller is better.
    Minimize,
    /// Closer to this value is better.
    Target(f64),
}

impl Goal {
    /// The score of a response value (larger is better).
    #[must_use]
    pub fn score(self, y: f64) -> f64 {
        match self {
            Self::Maximize => y,
            Self::Minimize => -y,
            Self::Target(t) => -(y - t).abs(),
        }
    }

    /// The response value whose score is the best of `ys`, or `None` when
    /// `ys` is empty.
    #[must_use]
    pub fn best(self, ys: &[f64]) -> Option<f64> {
        ys.iter().copied().filter(|v| v.is_finite()).max_by(|a, b| self.score(*a).total_cmp(&self.score(*b)))
    }
}

/// Standard normal density.
fn phi(z: f64) -> f64 {
    (-0.5 * z * z).exp() / (2.0 * std::f64::consts::PI).sqrt()
}

/// Standard normal distribution function.
fn cdf(z: f64) -> f64 {
    0.5 * erfc(-z * std::f64::consts::FRAC_1_SQRT_2)
}

/// Expected improvement of `Y ~ N(mean, sd²)` over the best observed value
/// `best` (the response value with the best score) for `goal`.
#[must_use]
pub fn expected_improvement(goal: Goal, mean: f64, sd: f64, best: f64) -> f64 {
    match goal {
        Goal::Maximize | Goal::Minimize => {
            let gain = goal.score(mean) - goal.score(best);
            if sd <= 0.0 {
                return gain.max(0.0);
            }
            let z = gain / sd;
            (gain * cdf(z) + sd * phi(z)).max(0.0)
        }
        Goal::Target(t) => {
            // E[(b − |Z|)⁺], Z = Y − T ~ N(m, s²).
            let b = (best - t).abs();
            let m = mean - t;
            if sd <= 0.0 {
                return (b - m.abs()).max(0.0);
            }
            // ∫_lo^hi (p + q z) f(z) dz with f the N(m, s²) density.
            let piece = |lo: f64, hi: f64, p: f64, q: f64| {
                let (a, c) = ((lo - m) / sd, (hi - m) / sd);
                let mass = cdf(c) - cdf(a);
                p * mass + q * (m * mass - sd * (phi(c) - phi(a)))
            };
            (piece(0.0, b, b, -1.0) + piece(-b, 0.0, b, 1.0)).max(0.0)
        }
    }
}

/// The most optimistic score of `N(mean, sd²)` within `kappa` standard
/// deviations.
#[must_use]
pub fn upper_confidence_bound(goal: Goal, mean: f64, sd: f64, kappa: f64) -> f64 {
    goal.score(optimistic_value(goal, mean, sd, kappa))
}

/// The response value in `mean ± kappa·sd` with the best score.
fn optimistic_value(goal: Goal, mean: f64, sd: f64, kappa: f64) -> f64 {
    match goal {
        Goal::Maximize => mean + kappa * sd,
        Goal::Minimize => mean - kappa * sd,
        Goal::Target(t) => t.clamp(mean - kappa * sd, mean + kappa * sd),
    }
}

/// The criterion that ranks candidate runs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AcquisitionCriterion {
    /// Expected improvement over the best run.
    ExpectedImprovement,
    /// Most optimistic value within `kappa` standard deviations.
    UpperConfidenceBound {
        /// Width in standard deviations (`√β` of Srinivas et al.).
        kappa: f64,
    },
}

/// What the runs should achieve.
pub enum Objective<'a> {
    /// One response (the first model) with its goal.
    Single(Goal),
    /// Several responses (one model each) combined by an overall
    /// desirability in `[0, 1]`, larger better; `goals` give each response's
    /// direction for the upper confidence bound.
    Desirability {
        /// Overall desirability of one value per response.
        desirability: &'a (dyn Fn(&[f64]) -> f64 + Sync),
        /// Each response's goal.
        goals: Vec<Goal>,
        /// Monte Carlo draws of the expected improvement.
        samples: usize,
    },
}

impl std::fmt::Debug for Objective<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Single(goal) => f.debug_tuple("Single").field(goal).finish(),
            Self::Desirability { goals, samples, .. } => {
                f.debug_struct("Desirability").field("goals", goals).field("samples", samples).finish_non_exhaustive()
            }
        }
    }
}

/// A run: continuous settings in `[0, 1]` and a level index per categorical
/// factor.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// Continuous settings (unit cube).
    pub continuous: Vec<f64>,
    /// Level of each categorical factor.
    pub levels: Vec<usize>,
}

/// The factors' space.
pub struct SearchSpace<'a> {
    /// Number of continuous factors.
    pub continuous: usize,
    /// Number of levels of each categorical factor.
    pub levels: Vec<usize>,
    /// Whether continuous settings satisfy the constraints.
    pub feasible: &'a (dyn Fn(&[f64]) -> bool + Sync),
}

impl std::fmt::Debug for SearchSpace<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchSpace")
            .field("continuous", &self.continuous)
            .field("levels", &self.levels)
            .finish_non_exhaustive()
    }
}

impl SearchSpace<'_> {
    /// Model inputs of a run: the continuous settings, then one 0/1
    /// indicator per level of each categorical factor.
    #[must_use]
    pub fn encode(&self, c: &Candidate) -> Vec<f64> {
        let mut x = c.continuous.clone();
        for (&level, &count) in c.levels.iter().zip(&self.levels) {
            x.extend((0..count).map(|k| if k == level { 1.0 } else { 0.0 }));
        }
        x
    }

    /// Number of model inputs.
    #[must_use]
    pub fn inputs(&self) -> usize {
        self.continuous + self.levels.iter().sum::<usize>()
    }
}

/// Settings of a proposal.
#[derive(Debug, Clone, PartialEq)]
pub struct ProposalOptions {
    /// Ranking criterion.
    pub criterion: AcquisitionCriterion,
    /// Runs to propose.
    pub batch: usize,
    /// Random candidates per proposed run.
    pub candidates: usize,
    /// Seed of the candidates and of the Monte Carlo draws.
    pub seed: u64,
}

impl Default for ProposalOptions {
    /// Expected improvement, one run, 2,000 candidates, seed 1.
    fn default() -> Self {
        Self { criterion: AcquisitionCriterion::ExpectedImprovement, batch: 1, candidates: 2000, seed: 1 }
    }
}

/// A proposed run.
#[derive(Debug, Clone, PartialEq)]
pub struct Proposal {
    /// The run.
    pub point: Candidate,
    /// AcquisitionCriterion value when it was chosen (after the lies of the runs
    /// before it).
    pub acquisition: f64,
    /// Each response's predicted mean (from the models given, without lies).
    pub means: Vec<f64>,
    /// Each response's predicted standard deviation of the mean.
    pub sds: Vec<f64>,
}

/// The best observed run: its score (or desirability) and responses.
fn best_run(models: &[GaussianProcess], objective: &Objective<'_>) -> Result<(f64, Vec<f64>)> {
    match objective {
        Objective::Single(goal) => {
            let y = goal.best(&models[0].y).ok_or_else(|| invalid("no observed response"))?;
            Ok((goal.score(y), vec![y]))
        }
        Objective::Desirability { desirability, .. } => {
            let n = models[0].y.len();
            if models.iter().any(|m| m.y.len() != n) {
                return Err(invalid("every response model needs the same runs"));
            }
            (0..n)
                .map(|i| {
                    let ys: Vec<f64> = models.iter().map(|m| m.y[i]).collect();
                    (desirability(&ys), ys)
                })
                .max_by(|a, b| a.0.total_cmp(&b.0))
                .ok_or_else(|| invalid("no observed response"))
        }
    }
}

/// The criterion at a point, from each model's mean and sd there.
fn criterion_value(
    objective: &Objective<'_>,
    criterion: AcquisitionCriterion,
    best: &(f64, Vec<f64>),
    draws: &[Vec<f64>],
    pred: &[(f64, f64)],
) -> f64 {
    match (objective, criterion) {
        (Objective::Single(goal), AcquisitionCriterion::ExpectedImprovement) => {
            expected_improvement(*goal, pred[0].0, pred[0].1, best.1[0])
        }
        (Objective::Single(goal), AcquisitionCriterion::UpperConfidenceBound { kappa }) => {
            upper_confidence_bound(*goal, pred[0].0, pred[0].1, kappa)
        }
        (Objective::Desirability { desirability, .. }, AcquisitionCriterion::ExpectedImprovement) => {
            let mut ys = vec![0.0; pred.len()];
            let total: f64 = draws
                .iter()
                .map(|z| {
                    for (k, (m, s)) in pred.iter().enumerate() {
                        ys[k] = m + s * z[k];
                    }
                    (desirability(&ys) - best.0).max(0.0)
                })
                .sum();
            total / draws.len().max(1) as f64
        }
        (Objective::Desirability { desirability, goals, .. }, AcquisitionCriterion::UpperConfidenceBound { kappa }) => {
            let ys: Vec<f64> = pred.iter().zip(goals).map(|(&(m, s), &g)| optimistic_value(g, m, s, kappa)).collect();
            desirability(&ys)
        }
    }
}

/// Up to `count` feasible random candidates.
fn sample_candidates(space: &SearchSpace<'_>, count: usize, rng: &mut Rng) -> Vec<Candidate> {
    let mut out = Vec::new();
    let random_levels = |rng: &mut Rng| space.levels.iter().map(|&k| rng.below(k.max(1))).collect::<Vec<_>>();
    if space.continuous == 0 {
        for _ in 0..count {
            out.push(Candidate { continuous: Vec::new(), levels: random_levels(rng) });
        }
        return out;
    }
    for _ in 0..50 {
        for u in latin_hypercube(count, space.continuous, rng) {
            if out.len() < count && (space.feasible)(&u) {
                out.push(Candidate { continuous: u, levels: random_levels(rng) });
            }
        }
        if out.len() >= count {
            break;
        }
    }
    out
}

/// Choose the next `options.batch` runs.
///
/// `pending` runs are in the experiment but not yet measured; they are
/// lied about before the search so the proposals avoid them.
///
/// # Errors
/// No models, models with the wrong number of inputs, a desirability
/// objective whose models differ in runs, no feasible candidate (the
/// constraints leave no room), or a zero batch.
pub fn propose_batch(
    models: &[GaussianProcess],
    objective: &Objective<'_>,
    space: &SearchSpace<'_>,
    pending: &[Candidate],
    options: &ProposalOptions,
) -> Result<Vec<Proposal>> {
    if models.is_empty() {
        return Err(invalid("no response model"));
    }
    if models.iter().any(|m| m.dims() != space.inputs()) {
        return Err(invalid("the models do not match the factors"));
    }
    if options.batch == 0 {
        return Err(invalid("propose at least one run"));
    }
    if let Objective::Single(_) = objective
        && models.len() != 1
    {
        return Err(invalid("a single goal needs one response model"));
    }
    let best = best_run(models, objective)?;
    let mut rng = Rng::new(options.seed);
    let draws: Vec<Vec<f64>> = match objective {
        Objective::Desirability { samples, .. } => {
            (0..(*samples).max(1)).map(|_| (0..models.len()).map(|_| rng.normal()).collect()).collect()
        }
        Objective::Single(_) => Vec::new(),
    };
    let lie = |current: &[GaussianProcess], x: &[f64]| -> Result<Vec<GaussianProcess>> {
        current.iter().zip(&best.1).map(|(m, &y)| m.condition_on(&[x.to_vec()], &[y])).collect()
    };
    let mut current = models.to_vec();
    for p in pending {
        current = lie(&current, &space.encode(p))?;
    }
    let mut out: Vec<Proposal> = Vec::new();
    for _ in 0..options.batch {
        // The criterion, with a vanishing tie-break towards large spread.
        let models_now = &current;
        let scales: Vec<f64> = models_now.iter().map(|m| m.y_scale).collect();
        let value = |c: &Candidate| -> (f64, f64) {
            let x = space.encode(c);
            let pred: Vec<(f64, f64)> = models_now.iter().map(|m| m.predict(&x)).collect();
            let spread: f64 = pred.iter().zip(&scales).map(|(p, s)| p.1 / s).sum();
            (criterion_value(objective, options.criterion, &best, &draws, &pred), spread)
        };
        let key = |v: (f64, f64)| v.0 + 1e-12 * v.1;
        let candidates = sample_candidates(space, options.candidates.max(10), &mut rng);
        if candidates.is_empty() {
            return Err(invalid("no setting within the factor ranges satisfies the constraints"));
        }
        let mut scored: Vec<(f64, Candidate)> = candidates.into_par_iter().map(|c| (key(value(&c)), c)).collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        scored.truncate(5);
        let polished: Vec<(f64, Candidate)> = scored
            .into_par_iter()
            .map(|(mut v, mut c)| {
                if space.continuous > 0 {
                    let levels = c.levels.clone();
                    let mut f = |u: &[f64]| {
                        if !(space.feasible)(u) {
                            return f64::INFINITY;
                        }
                        -key(value(&Candidate { continuous: u.to_vec(), levels: levels.clone() }))
                    };
                    let lower = vec![0.0; space.continuous];
                    let upper = vec![1.0; space.continuous];
                    let m = minimize_in_box(&mut f, &c.continuous, &lower, &upper, 100 * (space.continuous + 1));
                    if -m.value > v {
                        (v, c.continuous) = (-m.value, m.x);
                    }
                }
                for j in 0..space.levels.len() {
                    for k in 0..space.levels[j] {
                        let mut t = c.clone();
                        t.levels[j] = k;
                        let w = key(value(&t));
                        if w > v {
                            (v, c) = (w, t);
                        }
                    }
                }
                (v, c)
            })
            .collect();
        let Some((_, chosen)) = polished.into_iter().max_by(|a, b| a.0.total_cmp(&b.0)) else {
            return Err(invalid("no setting within the factor ranges satisfies the constraints"));
        };
        let x = space.encode(&chosen);
        let acquisition = value(&chosen).0;
        let (means, sds) = models.iter().map(|m| m.predict(&x)).unzip();
        out.push(Proposal { point: chosen, acquisition, means, sds });
        current = lie(&current, &x)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gaussian_process::{GpOptions, Nugget, fit_gaussian_process};

    /// ∫ improvement · density by the trapezoid rule over ±12 sd.
    fn numeric_ei(goal: Goal, mean: f64, sd: f64, best: f64) -> f64 {
        let n = 200_000;
        let (lo, hi) = (mean - 12.0 * sd, mean + 12.0 * sd);
        let h = (hi - lo) / n as f64;
        (0..=n)
            .map(|i| {
                let y = lo + i as f64 * h;
                let w = if i == 0 || i == n { 0.5 } else { 1.0 };
                w * (goal.score(y) - goal.score(best)).max(0.0) * phi((y - mean) / sd) / sd
            })
            .sum::<f64>()
            * h
    }

    #[test]
    fn closed_forms_match_integration() {
        for (goal, mean, sd, best) in [
            (Goal::Maximize, 1.0, 0.5, 1.3),
            (Goal::Maximize, 2.0, 0.1, 1.0),
            (Goal::Minimize, 1.0, 0.7, 0.4),
            (Goal::Target(5.0), 5.4, 0.3, 4.8),
            (Goal::Target(5.0), 6.5, 1.2, 5.3),
            (Goal::Target(0.0), -0.1, 0.05, 0.2),
        ] {
            let a = expected_improvement(goal, mean, sd, best);
            let b = numeric_ei(goal, mean, sd, best);
            assert!((a - b).abs() < 1e-8, "{goal:?} {mean} {sd} {best}: {a} vs {b}");
        }
        // σ = 0: the plain improvement.
        assert_eq!(expected_improvement(Goal::Maximize, 3.0, 0.0, 1.0), 2.0);
        assert!((expected_improvement(Goal::Target(1.0), 1.2, 0.0, 1.5) - 0.3).abs() < 1e-12);
        assert!((upper_confidence_bound(Goal::Minimize, 2.0, 0.5, 2.0) + 1.0).abs() < 1e-12);
        assert_eq!(upper_confidence_bound(Goal::Target(1.0), 2.0, 0.5, 2.0), 0.0);
        assert_eq!(Goal::Target(3.0).best(&[1.0, 2.5, 4.0]), Some(2.5));
    }

    fn branin(u: &[f64]) -> f64 {
        let (x1, x2) = (-5.0 + 15.0 * u[0], 15.0 * u[1]);
        let pi = std::f64::consts::PI;
        let b = 5.1 / (4.0 * pi * pi);
        (x2 - b * x1 * x1 + 5.0 / pi * x1 - 6.0).powi(2) + 10.0 * (1.0 - 1.0 / (8.0 * pi)) * x1.cos() + 10.0
    }

    fn hartmann3(x: &[f64]) -> f64 {
        const A: [[f64; 3]; 4] = [[3.0, 10.0, 30.0], [0.1, 10.0, 35.0], [3.0, 10.0, 30.0], [0.1, 10.0, 35.0]];
        const P: [[f64; 3]; 4] =
            [[0.3689, 0.1170, 0.2673], [0.4699, 0.4387, 0.7470], [0.1091, 0.8732, 0.5547], [0.0381, 0.5743, 0.8828]];
        const C: [f64; 4] = [1.0, 1.2, 3.0, 3.2];
        (0..4).map(|i| C[i] * (-(0..3).map(|j| A[i][j] * (x[j] - P[i][j]).powi(2)).sum::<f64>()).exp()).sum()
    }

    /// Sequential runs, one at a time, from a Latin hypercube start.
    fn optimize(f: &dyn Fn(&[f64]) -> f64, dims: usize, goal: Goal, start: usize, rounds: usize) -> f64 {
        let mut rng = Rng::new(5);
        let mut x = latin_hypercube(start, dims, &mut rng);
        let mut y: Vec<f64> = x.iter().map(|p| f(p)).collect();
        let feasible = |_: &[f64]| true;
        let space = SearchSpace { continuous: dims, levels: Vec::new(), feasible: &feasible };
        for round in 0..rounds {
            let options =
                GpOptions { nugget: Nugget::Fixed(1e-6), starts: 4, seed: round as u64, ..GpOptions::default() };
            let gp = fit_gaussian_process(&x, &y, &options).unwrap();
            let options = ProposalOptions { candidates: 1000, seed: round as u64, ..ProposalOptions::default() };
            let p = propose_batch(&[gp], &Objective::Single(goal), &space, &[], &options).unwrap();
            let next = p[0].point.continuous.clone();
            y.push(f(&next));
            x.push(next);
        }
        goal.best(&y).unwrap()
    }

    #[test]
    fn expected_improvement_finds_known_optima() {
        // Branin: global minimum 0.397887 (three places).
        let best = optimize(&branin, 2, Goal::Minimize, 8, 22);
        assert!(best < 0.41, "Branin best {best}");
        // Hartmann 3: global maximum 3.86278 at (0.1146, 0.5556, 0.8525).
        let best = optimize(&hartmann3, 3, Goal::Maximize, 10, 25);
        assert!(best > 3.84, "Hartmann 3 best {best}");
    }

    #[test]
    fn batches_spread_and_respect_constraints() {
        let mut rng = Rng::new(9);
        let x = latin_hypercube(10, 2, &mut rng);
        let y: Vec<f64> = x.iter().map(|p| branin(p)).collect();
        let gp = fit_gaussian_process(&x, &y, &GpOptions { starts: 3, ..GpOptions::default() }).unwrap();
        let feasible = |u: &[f64]| u[0] + u[1] <= 1.0;
        let space = SearchSpace { continuous: 2, levels: Vec::new(), feasible: &feasible };
        for criterion in
            [AcquisitionCriterion::ExpectedImprovement, AcquisitionCriterion::UpperConfidenceBound { kappa: 2.0 }]
        {
            let options = ProposalOptions { criterion, batch: 4, ..ProposalOptions::default() };
            let p = propose_batch(std::slice::from_ref(&gp), &Objective::Single(Goal::Minimize), &space, &[], &options)
                .unwrap();
            assert_eq!(p.len(), 4);
            for (i, a) in p.iter().enumerate() {
                assert!(a.point.continuous[0] + a.point.continuous[1] <= 1.0 + 1e-12);
                for b in &p[..i] {
                    let d =
                        a.point.continuous.iter().zip(&b.point.continuous).map(|(u, v)| (u - v).powi(2)).sum::<f64>();
                    assert!(d.sqrt() > 0.01, "{criterion:?}: repeated run {:?}", a.point);
                }
            }
        }
        // A pending run is avoided: the first proposal moves away from it.
        let options = ProposalOptions::default();
        let objective = Objective::Single(Goal::Minimize);
        let first = propose_batch(std::slice::from_ref(&gp), &objective, &space, &[], &options).unwrap();
        let again =
            propose_batch(std::slice::from_ref(&gp), &objective, &space, &[first[0].point.clone()], &options).unwrap();
        let d: f64 =
            first[0].point.continuous.iter().zip(&again[0].point.continuous).map(|(u, v)| (u - v).powi(2)).sum();
        assert!(d.sqrt() > 0.01);
        // Nothing feasible.
        let never = |_: &[f64]| false;
        let empty = SearchSpace { continuous: 2, levels: Vec::new(), feasible: &never };
        assert!(propose_batch(&[gp], &objective, &empty, &[], &options).is_err());
    }

    #[test]
    fn categorical_levels_and_desirability() {
        // y1 = 4 − (u − 0.3)² · 8 + 2·[level 1]; y2 = u (minimize). Level 1
        // is better for y1 everywhere.
        let mut rng = Rng::new(2);
        let mut runs = Vec::new();
        for i in 0..16 {
            runs.push(Candidate { continuous: vec![rng.next_f64()], levels: vec![i % 2] });
        }
        let feasible = |_: &[f64]| true;
        let space = SearchSpace { continuous: 1, levels: vec![2], feasible: &feasible };
        let x: Vec<Vec<f64>> = runs.iter().map(|c| space.encode(c)).collect();
        let y1: Vec<f64> = runs
            .iter()
            .map(|c| 4.0 - 8.0 * (c.continuous[0] - 0.3).powi(2) + if c.levels[0] == 1 { 2.0 } else { 0.0 })
            .collect();
        let y2: Vec<f64> = runs.iter().map(|c| c.continuous[0]).collect();
        let options = GpOptions { starts: 3, nugget: Nugget::Fixed(1e-6), ..GpOptions::default() };
        let g1 = fit_gaussian_process(&x, &y1, &options).unwrap();
        let g2 = fit_gaussian_process(&x, &y2, &options).unwrap();
        let p = propose_batch(
            std::slice::from_ref(&g1),
            &Objective::Single(Goal::Maximize),
            &space,
            &[],
            &ProposalOptions::default(),
        )
        .unwrap();
        assert_eq!(p[0].point.levels, vec![1]);
        assert!((p[0].point.continuous[0] - 0.3).abs() < 0.1, "{:?}", p[0]);
        // Desirability of y1 (larger better, 0 at 0 and 1 at 6) and y2
        // (smaller better): the optimum moves left of 0.3.
        let d = |ys: &[f64]| ((ys[0] / 6.0).clamp(0.0, 1.0) * (1.0 - ys[1]).clamp(0.0, 1.0)).sqrt();
        let objective =
            Objective::Desirability { desirability: &d, goals: vec![Goal::Maximize, Goal::Minimize], samples: 64 };
        let p = propose_batch(&[g1, g2], &objective, &space, &[], &ProposalOptions::default()).unwrap();
        assert_eq!(p[0].point.levels, vec![1]);
        assert!(p[0].point.continuous[0] < 0.3, "{:?}", p[0]);
        assert_eq!(p[0].means.len(), 2);
    }
}
