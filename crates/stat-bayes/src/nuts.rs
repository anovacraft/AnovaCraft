//! The No-U-Turn sampler (NUTS) for a differentiable log density on ℝᵈ.
//!
//! Hamiltonian Monte Carlo with a diagonal Euclidean metric (Neal 2011,
//! "MCMC using Hamiltonian dynamics", *Handbook of MCMC*), with
//! trajectories built by repeated doubling until a U-turn (Hoffman & Gelman
//! 2014, *JMLR* 15) in the form described by Betancourt (2017, "A
//! conceptual introduction to Hamiltonian Monte Carlo", arXiv:1701.02434,
//! appendix A):
//!
//! * the next state is drawn from the whole trajectory with probabilities
//!   proportional to `exp(−H)` — uniformly (progressively) within each new
//!   subtree and biased toward the new subtree when it is merged into the
//!   trajectory;
//! * the generalized no-U-turn criterion `p♯₋·ρ > 0 and p♯₊·ρ > 0`, with `ρ`
//!   the summed momenta and `p♯ = M⁻¹p` at the ends, is checked on every
//!   subtree and on the two joins between the halves of each merge;
//! * a trajectory stops at a maximum tree depth (10: 1023 leapfrog steps)
//!   or at a **divergence**, when the energy error exceeds 1000.
//!
//! **Warm-up** adapts the step size by dual averaging toward a target mean
//! acceptance statistic (0.8; Nesterov 2009, as in Hoffman & Gelman 2014,
//! §3.2: `γ = 0.05`, `t₀ = 10`, `κ = 0.75`) and the diagonal inverse metric
//! from the draws' variances in doubling windows (an initial 15% buffer of
//! warm-up for the step size alone, windows of 25, 50, 100, … draws, and a
//! final 10% buffer), regularized as `(n/(n + 5)) var + 10⁻³ · 5/(n + 5)`;
//! the step size is re-initialized after each metric update (the windowed
//! scheme of the Stan reference manual, ch. 15). Chains start at uniform
//! draws in (−2, 2) on the unconstrained scale and run in parallel, each
//! with its own seed.

use rayon::prelude::*;
use stat_core::rng::Rng;
use stat_core::{Result, StatError};

/// A log density (up to a constant) on unconstrained parameters.
pub trait Target: Sync {
    /// Number of parameters.
    fn dim(&self) -> usize;
    /// Log density at `theta`, writing its gradient into `grad`. Return
    /// `NEG_INFINITY` (or NaN) outside the support.
    fn log_density_gradient(&self, theta: &[f64], grad: &mut [f64]) -> f64;
}

/// Sampler settings.
#[derive(Debug, Clone, PartialEq)]
pub struct NutsSettings {
    /// Number of chains (run in parallel).
    pub chains: usize,
    /// Warm-up iterations per chain (discarded).
    pub warmup: usize,
    /// Kept draws per chain.
    pub draws: usize,
    /// Seed; chain `c` uses a generator derived from `seed` and `c`.
    pub seed: u64,
    /// Target mean acceptance statistic of the step-size adaptation.
    pub target_accept: f64,
    /// Maximum tree depth.
    pub max_depth: usize,
}

impl Default for NutsSettings {
    fn default() -> Self {
        Self { chains: 4, warmup: 1000, draws: 1000, seed: 1, target_accept: 0.8, max_depth: 10 }
    }
}

/// The output of one chain.
#[derive(Debug, Clone, PartialEq)]
pub struct Chain {
    /// Kept draws (each a parameter vector).
    pub draws: Vec<Vec<f64>>,
    /// Log density of each draw.
    pub log_density: Vec<f64>,
    /// Hamiltonian at each draw (for the energy diagnostic).
    pub energy: Vec<f64>,
    /// Tree depth of each kept transition.
    pub depth: Vec<usize>,
    /// Whether each kept transition diverged.
    pub divergent: Vec<bool>,
    /// Mean acceptance statistic of each kept transition.
    pub accept: Vec<f64>,
    /// Adapted step size.
    pub step_size: f64,
    /// Adapted diagonal inverse metric.
    pub inverse_metric: Vec<f64>,
}

impl Chain {
    /// Number of divergent transitions after warm-up.
    #[must_use]
    pub fn divergences(&self) -> usize {
        self.divergent.iter().filter(|&&d| d).count()
    }

    /// Transitions that hit the maximum tree depth.
    #[must_use]
    pub fn max_depth_hits(&self, max_depth: usize) -> usize {
        self.depth.iter().filter(|&&d| d >= max_depth).count()
    }

    /// Energy Bayesian fraction of missing information,
    /// `Σ(Eₙ − Eₙ₋₁)² / Σ(Eₙ − Ē)²` (Betancourt 2016, arXiv:1604.00695);
    /// below 0.3 suggests the sampler explores the energy poorly.
    #[must_use]
    pub fn e_bfmi(&self) -> f64 {
        let e = &self.energy;
        if e.len() < 2 {
            return f64::NAN;
        }
        let m = e.iter().sum::<f64>() / e.len() as f64;
        let num: f64 = e.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum();
        let den: f64 = e.iter().map(|v| (v - m).powi(2)).sum();
        num / den
    }
}

#[derive(Clone)]
struct State {
    q: Vec<f64>,
    p: Vec<f64>,
    grad: Vec<f64>,
    logp: f64,
}

struct Subtree {
    p_beg: Vec<f64>,
    sharp_beg: Vec<f64>,
    p_end: Vec<f64>,
    sharp_end: Vec<f64>,
    rho: Vec<f64>,
    log_weight: f64,
    proposal: State,
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn log_add(a: f64, b: f64) -> f64 {
    if a == f64::NEG_INFINITY {
        return b;
    }
    if b == f64::NEG_INFINITY {
        return a;
    }
    let m = a.max(b);
    m + ((a - m).exp() + (b - m).exp()).ln()
}

fn no_u_turn(sharp_a: &[f64], sharp_b: &[f64], rho: &[f64]) -> bool {
    dot(sharp_a, rho) > 0.0 && dot(sharp_b, rho) > 0.0
}

struct Sampler<'a, T: Target + ?Sized> {
    target: &'a T,
    inv_metric: Vec<f64>,
    eps: f64,
    max_depth: usize,
    rng: Rng,
    // Per-transition statistics.
    n_leapfrog: usize,
    sum_accept: f64,
    divergent: bool,
}

impl<T: Target + ?Sized> Sampler<'_, T> {
    fn kinetic(&self, p: &[f64]) -> f64 {
        0.5 * p.iter().zip(&self.inv_metric).map(|(p, m)| p * p * m).sum::<f64>()
    }

    fn hamiltonian(&self, s: &State) -> f64 {
        let h = -s.logp + self.kinetic(&s.p);
        if h.is_nan() { f64::INFINITY } else { h }
    }

    fn sharp(&self, p: &[f64]) -> Vec<f64> {
        p.iter().zip(&self.inv_metric).map(|(p, m)| p * m).collect()
    }

    fn draw_momentum(&mut self, s: &mut State) {
        for (p, m) in s.p.iter_mut().zip(&self.inv_metric) {
            *p = self.rng.normal() / m.sqrt();
        }
    }

    fn evaluate(&self, q: &[f64], grad: &mut [f64]) -> f64 {
        let v = self.target.log_density_gradient(q, grad);
        if v.is_finite() && grad.iter().all(|g| g.is_finite()) { v } else { f64::NEG_INFINITY }
    }

    fn leapfrog(&self, s: &mut State, eps: f64) {
        for (p, g) in s.p.iter_mut().zip(&s.grad) {
            *p += 0.5 * eps * g;
        }
        for ((q, p), m) in s.q.iter_mut().zip(&s.p).zip(&self.inv_metric) {
            *q += eps * m * p;
        }
        s.logp = self.evaluate(&s.q, &mut s.grad);
        if s.logp.is_finite() {
            for (p, g) in s.p.iter_mut().zip(&s.grad) {
                *p += 0.5 * eps * g;
            }
        }
    }

    /// A subtree of `2^depth` leapfrog steps from `z` (advanced to its far
    /// end) in direction `dir`; `None` on a divergence or U-turn.
    fn build(&mut self, depth: usize, z: &mut State, dir: f64, h0: f64) -> Option<Subtree> {
        if depth == 0 {
            self.leapfrog(z, dir * self.eps);
            self.n_leapfrog += 1;
            let h = self.hamiltonian(z);
            if (h - h0).is_nan() || h - h0 > 1000.0 {
                self.divergent = true;
                return None;
            }
            let delta = h0 - h;
            self.sum_accept += if delta > 0.0 { 1.0 } else { delta.exp() };
            let sharp = self.sharp(&z.p);
            return Some(Subtree {
                p_beg: z.p.clone(),
                sharp_beg: sharp.clone(),
                p_end: z.p.clone(),
                sharp_end: sharp,
                rho: z.p.clone(),
                log_weight: delta,
                proposal: z.clone(),
            });
        }
        let first = self.build(depth - 1, z, dir, h0)?;
        let second = self.build(depth - 1, z, dir, h0)?;
        let log_weight = log_add(first.log_weight, second.log_weight);
        // Uniform progressive sampling within the subtree.
        let take_second = self.rng.next_f64().ln() < second.log_weight - log_weight;
        let rho: Vec<f64> = first.rho.iter().zip(&second.rho).map(|(a, b)| a + b).collect();
        let ok = no_u_turn(&first.sharp_beg, &second.sharp_end, &rho)
            && no_u_turn(
                &first.sharp_beg,
                &second.sharp_beg,
                &first.rho.iter().zip(&second.p_beg).map(|(a, b)| a + b).collect::<Vec<_>>(),
            )
            && no_u_turn(
                &first.sharp_end,
                &second.sharp_end,
                &first.p_end.iter().zip(&second.rho).map(|(a, b)| a + b).collect::<Vec<_>>(),
            );
        if !ok {
            return None;
        }
        Some(Subtree {
            p_beg: first.p_beg,
            sharp_beg: first.sharp_beg,
            p_end: second.p_end,
            sharp_end: second.sharp_end,
            rho,
            log_weight,
            proposal: if take_second { second.proposal } else { first.proposal },
        })
    }

    /// One NUTS transition from `current`: (new state, depth reached).
    fn transition(&mut self, current: &State) -> (State, usize) {
        let mut z0 = current.clone();
        self.draw_momentum(&mut z0);
        let h0 = self.hamiltonian(&z0);
        self.n_leapfrog = 0;
        self.sum_accept = 0.0;
        self.divergent = false;
        let sharp0 = self.sharp(&z0.p);
        // Trajectory ends (minus, plus) and their momenta.
        let mut minus = z0.clone();
        let mut plus = z0.clone();
        let (mut p_minus, mut sharp_minus) = (z0.p.clone(), sharp0.clone());
        let (mut p_plus, mut sharp_plus) = (z0.p.clone(), sharp0);
        let mut rho = z0.p.clone();
        let mut log_weight = 0.0;
        let mut sample = z0.clone();
        let mut depth = 0;
        while depth < self.max_depth {
            let forward = self.rng.next_f64() > 0.5;
            let (dir, start) = if forward { (1.0, &mut plus) } else { (-1.0, &mut minus) };
            let Some(sub) = self.build(depth, start, dir, h0) else {
                break;
            };
            depth += 1;
            // Biased progressive sampling toward the new subtree.
            if sub.log_weight > log_weight || self.rng.next_f64().ln() < sub.log_weight - log_weight {
                sample = sub.proposal.clone();
            }
            log_weight = log_add(log_weight, sub.log_weight);
            // The old tree oriented in the direction of integration.
            let (old_sharp_beg, old_p_end, old_sharp_end) =
                if forward { (&sharp_minus, &p_plus, &sharp_plus) } else { (&sharp_plus, &p_minus, &sharp_minus) };
            let merged: Vec<f64> = rho.iter().zip(&sub.rho).map(|(a, b)| a + b).collect();
            let ok = no_u_turn(old_sharp_beg, &sub.sharp_end, &merged)
                && no_u_turn(
                    old_sharp_beg,
                    &sub.sharp_beg,
                    &rho.iter().zip(&sub.p_beg).map(|(a, b)| a + b).collect::<Vec<_>>(),
                )
                && no_u_turn(
                    old_sharp_end,
                    &sub.sharp_end,
                    &old_p_end.iter().zip(&sub.rho).map(|(a, b)| a + b).collect::<Vec<_>>(),
                );
            rho = merged;
            if forward {
                p_plus = sub.p_end;
                sharp_plus = sub.sharp_end;
            } else {
                p_minus = sub.p_end;
                sharp_minus = sub.sharp_end;
            }
            if !ok {
                break;
            }
        }
        (sample, depth)
    }

    fn accept_stat(&self) -> f64 {
        if self.n_leapfrog == 0 { 0.0 } else { self.sum_accept / self.n_leapfrog as f64 }
    }

    /// Heuristic initial step size: double or halve until the acceptance
    /// probability of one leapfrog step crosses 0.8.
    fn init_step_size(&mut self, at: &State) {
        let mut z = at.clone();
        self.draw_momentum(&mut z);
        let h0 = self.hamiltonian(&z);
        let mut trial = z.clone();
        self.leapfrog(&mut trial, self.eps);
        let delta = h0 - self.hamiltonian(&trial);
        let up = delta > 0.8f64.ln();
        for _ in 0..100 {
            let mut z = at.clone();
            self.draw_momentum(&mut z);
            let h0 = self.hamiltonian(&z);
            let mut trial = z.clone();
            self.leapfrog(&mut trial, self.eps);
            let delta = h0 - self.hamiltonian(&trial);
            let target = 0.8f64.ln();
            if delta.is_nan() || (up && delta <= target) || (!up && delta >= target) {
                break;
            }
            self.eps = if up { self.eps * 2.0 } else { self.eps / 2.0 };
            if !(self.eps > 1e-8 && self.eps < 1e7) {
                self.eps = self.eps.clamp(1e-8, 1e7);
                break;
            }
        }
    }
}

struct DualAveraging {
    mu: f64,
    h_bar: f64,
    log_eps_bar: f64,
    count: f64,
    delta: f64,
}

impl DualAveraging {
    fn new(eps: f64, delta: f64) -> Self {
        Self { mu: (10.0 * eps).ln(), h_bar: 0.0, log_eps_bar: 0.0, count: 0.0, delta }
    }

    fn update(&mut self, accept: f64) -> f64 {
        self.count += 1.0;
        let m = self.count;
        let w = 1.0 / (m + 10.0);
        self.h_bar = (1.0 - w) * self.h_bar + w * (self.delta - accept);
        let log_eps = self.mu - m.sqrt() / 0.05 * self.h_bar;
        let eta = m.powf(-0.75);
        self.log_eps_bar = eta * log_eps + (1.0 - eta) * self.log_eps_bar;
        log_eps.exp()
    }
}

/// End points (exclusive) of the metric-adaptation windows within warm-up.
fn windows(warmup: usize) -> (usize, Vec<usize>) {
    let (mut init, mut term, base) = (75, 50, 25);
    if warmup < init + term + base {
        init = (0.15 * warmup as f64) as usize;
        term = (0.1 * warmup as f64) as usize;
    }
    let end_slow = warmup.saturating_sub(term);
    let mut ends = Vec::new();
    let mut start = init;
    let mut size = if warmup < 150 { end_slow.saturating_sub(init) } else { base };
    while start < end_slow && size > 0 {
        let mut end = start + size;
        // The last window stretches to the terminal buffer.
        if end + 2 * size > end_slow {
            end = end_slow;
        }
        ends.push(end);
        start = end;
        size *= 2;
    }
    (init, ends)
}

fn run_chain<T: Target + ?Sized>(target: &T, settings: &NutsSettings, chain: usize) -> Result<Chain> {
    let d = target.dim();
    let mut rng = Rng::new(settings.seed ^ (0xD1B5_4A32_D192_ED03_u64.wrapping_mul(chain as u64 + 1)));
    // Initial point.
    let mut grad = vec![0.0; d];
    let mut q = vec![0.0; d];
    let mut logp = f64::NEG_INFINITY;
    for _ in 0..100 {
        for v in &mut q {
            *v = 4.0 * rng.next_f64() - 2.0;
        }
        logp = target.log_density_gradient(&q, &mut grad);
        if logp.is_finite() && grad.iter().all(|g| g.is_finite()) {
            break;
        }
    }
    if !logp.is_finite() {
        return Err(StatError::InvalidArgument {
            message: "the sampler found no starting point with a finite log density".into(),
        });
    }
    let mut s = Sampler {
        target,
        inv_metric: vec![1.0; d],
        eps: 1.0,
        max_depth: settings.max_depth,
        rng,
        n_leapfrog: 0,
        sum_accept: 0.0,
        divergent: false,
    };
    let mut current = State { q, p: vec![0.0; d], grad, logp };
    s.init_step_size(&current);
    let mut da = DualAveraging::new(s.eps, settings.target_accept);
    let (init, ends) = windows(settings.warmup);
    let mut window: Vec<Vec<f64>> = Vec::new();
    let mut next_end = ends.iter().copied().peekable();
    for it in 0..settings.warmup {
        stat_core::interrupt::check()?;
        stat_core::interrupt::step();
        let (next, _) = s.transition(&current);
        current = next;
        s.eps = da.update(s.accept_stat());
        if it >= init && next_end.peek().is_some() {
            window.push(current.q.clone());
            if next_end.peek() == Some(&(it + 1)) {
                next_end.next();
                let n = window.len() as f64;
                for j in 0..d {
                    let m = window.iter().map(|w| w[j]).sum::<f64>() / n;
                    let v = window.iter().map(|w| (w[j] - m).powi(2)).sum::<f64>() / (n - 1.0).max(1.0);
                    s.inv_metric[j] = (n / (n + 5.0)) * v + 1e-3 * (5.0 / (n + 5.0));
                }
                window.clear();
                s.init_step_size(&current);
                da = DualAveraging::new(s.eps, settings.target_accept);
            }
        }
    }
    if settings.warmup > 0 {
        s.eps = da.log_eps_bar.exp();
    }
    let n = settings.draws;
    let mut out = Chain {
        draws: Vec::with_capacity(n),
        log_density: Vec::with_capacity(n),
        energy: Vec::with_capacity(n),
        depth: Vec::with_capacity(n),
        divergent: Vec::with_capacity(n),
        accept: Vec::with_capacity(n),
        step_size: s.eps,
        inverse_metric: s.inv_metric.clone(),
    };
    for _ in 0..n {
        stat_core::interrupt::check()?;
        stat_core::interrupt::step();
        let (next, depth) = s.transition(&current);
        current = next;
        out.draws.push(current.q.clone());
        out.log_density.push(current.logp);
        out.energy.push(-current.logp + s.kinetic(&current.p));
        out.depth.push(depth);
        out.divergent.push(s.divergent);
        out.accept.push(s.accept_stat());
    }
    Ok(out)
}

/// Run the chains in parallel.
///
/// # Errors
/// Zero chains or draws, a target of dimension 0, or no finite starting
/// point.
pub fn sample<T: Target + ?Sized>(target: &T, settings: &NutsSettings) -> Result<Vec<Chain>> {
    if settings.chains == 0 || settings.draws < 4 {
        return Err(StatError::InvalidArgument {
            message: "the sampler needs at least one chain and four draws".into(),
        });
    }
    if target.dim() == 0 {
        return Err(StatError::InvalidArgument { message: "the model has no parameters to sample".into() });
    }
    if !(settings.target_accept > 0.0 && settings.target_accept < 1.0) {
        return Err(StatError::InvalidArgument { message: "the target acceptance must lie between 0 and 1".into() });
    }
    // Chains run on rayon's threads: each gets the caller's interrupt.
    let interrupt = stat_core::interrupt::current();
    if let Some(i) = &interrupt {
        i.begin(settings.chains * (settings.warmup + settings.draws));
    }
    (0..settings.chains)
        .into_par_iter()
        .map(|c| match &interrupt {
            Some(i) => stat_core::interrupt::with(std::sync::Arc::clone(i), || run_chain(target, settings, c)),
            None => run_chain(target, settings, c),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::diagnose;

    /// Independent normals with means μ and SDs σ.
    struct Normals {
        mu: Vec<f64>,
        sd: Vec<f64>,
    }

    impl Target for Normals {
        fn dim(&self) -> usize {
            self.mu.len()
        }
        fn log_density_gradient(&self, theta: &[f64], grad: &mut [f64]) -> f64 {
            let mut lp = 0.0;
            for j in 0..theta.len() {
                let z = (theta[j] - self.mu[j]) / self.sd[j];
                lp -= 0.5 * z * z;
                grad[j] = -z / self.sd[j];
            }
            lp
        }
    }

    /// The sampler counts warm-up and draws on the caller's interrupt and
    /// stops once it is cancelled.
    #[test]
    fn interrupt_counts_and_cancels_sampling() {
        use stat_core::interrupt::{Interrupt, with};
        let t = Normals { mu: vec![0.0], sd: vec![1.0] };
        let settings = NutsSettings { chains: 2, warmup: 50, draws: 50, ..NutsSettings::default() };
        let i = Interrupt::new();
        let plain = sample(&t, &settings).unwrap();
        let watched = with(std::sync::Arc::clone(&i), || sample(&t, &settings)).unwrap();
        assert_eq!(i.progress(), (200, 200));
        assert_eq!(plain[1].draws, watched[1].draws);
        i.cancel();
        assert!(matches!(with(i, || sample(&t, &settings)), Err(StatError::Cancelled)));
    }

    #[test]
    fn recovers_independent_normals() {
        let t = Normals { mu: vec![1.0, -3.0, 50.0], sd: vec![1.0, 0.01, 20.0] };
        let chains = sample(&t, &NutsSettings::default()).unwrap();
        for j in 0..3 {
            let per: Vec<Vec<f64>> = chains.iter().map(|c| c.draws.iter().map(|d| d[j]).collect()).collect();
            let dg = diagnose(&per);
            let all: Vec<f64> = per.iter().flatten().copied().collect();
            let m = all.iter().sum::<f64>() / all.len() as f64;
            let sd = (all.iter().map(|v| (v - m).powi(2)).sum::<f64>() / (all.len() - 1) as f64).sqrt();
            assert!(dg.rhat < 1.01, "R̂ {}", dg.rhat);
            assert!(dg.ess_bulk > 1000.0, "ESS {}", dg.ess_bulk);
            assert!((m - t.mu[j]).abs() < 4.0 * dg.mcse_mean, "mean {m} vs {}", t.mu[j]);
            assert!((sd / t.sd[j] - 1.0).abs() < 0.08, "sd {sd} vs {}", t.sd[j]);
        }
        assert_eq!(chains.iter().map(Chain::divergences).sum::<usize>(), 0);
        // The adapted metric matches the variances.
        let m = &chains[0].inverse_metric;
        assert!((m[2] / 400.0 - 1.0).abs() < 0.5 && (m[1] / 1e-4 - 1.0).abs() < 0.5);
    }

    #[test]
    fn warmup_windows() {
        let (init, ends) = windows(1000);
        assert_eq!(init, 75);
        assert_eq!(ends, vec![100, 150, 250, 450, 950]);
        let (init, ends) = windows(100);
        assert_eq!(init, 15);
        assert_eq!(ends, vec![90]);
    }
}
