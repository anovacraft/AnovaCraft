//! Support vector machines: C-support vector classification and ε-support
//! vector regression, solved by sequential minimal optimization (SMO).
//!
//! **Problem.** Both tasks are the dual quadratic program
//!
//! ```text
//! minimize ½ αᵀQα + pᵀα   subject to yᵀα = 0, 0 ≤ αᵢ ≤ C,
//! ```
//!
//! with `Qᵢⱼ = yᵢyⱼK(xᵢ, xⱼ)` and `yᵢ = ±1` (Cortes & Vapnik 1995; Vapnik
//! 1998). Classification has one variable per row with `p = −1`; ε-regression
//! has two per row (`α` and `α*`), with `p = ε ∓ zᵢ` (Smola & Schölkopf 2004).
//!
//! **Solver.** SMO (Platt 1998) updates two variables at a time, chosen by
//! the second-order working-set selection of Fan, Chen & Lin (2005): the
//! first maximally violates the optimality conditions, the second gives the
//! largest decrease of the objective. It stops when the largest violation
//! `m(α) − M(α)` is below the tolerance (10⁻³ by default). The offset ρ is
//! the mean of `yᵢ∇ᵢ` over free variables (the midpoint of its feasible
//! range when none is free). Kernel rows are computed on demand and cached.
//!
//! **Several classes.** One machine per pair of classes (one-against-one,
//! Knerr et al. 1990; Hsu & Lin 2002); the predicted class wins the most
//! pairwise votes.
//!
//! **Probabilities.** Each pairwise decision value is mapped to a
//! probability by Platt's sigmoid `1 / (1 + exp(A f + B))`, fitted to
//! decision values from an internal 5-fold cross-validation by Newton's
//! method with backtracking (Platt 1999; Lin, Lin & Weng 2007). Pairwise
//! probabilities are combined by the coupling method of Wu, Lin & Weng
//! (2004, method 2).
//!
//! **Kernels.** Linear `uᵀv`, radial basis `exp(−γ‖u − v‖²)` and polynomial
//! `(γuᵀv + c₀)ᵈ`.

use std::collections::VecDeque;
use std::rc::Rc;

use rayon::prelude::*;
use stat_core::{Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// A kernel function.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kernel {
    /// `uᵀv`.
    Linear,
    /// `exp(−γ‖u − v‖²)`.
    Rbf {
        /// Width parameter γ > 0.
        gamma: f64,
    },
    /// `(γuᵀv + c₀)ᵈ`.
    Polynomial {
        /// Degree d ≥ 1.
        degree: u32,
        /// Scale γ > 0.
        gamma: f64,
        /// Offset c₀ ≥ 0.
        coef0: f64,
    },
}

impl Kernel {
    /// `K(u, v)`.
    #[must_use]
    pub fn eval(&self, u: &[f64], v: &[f64]) -> f64 {
        match *self {
            Self::Linear => dot(u, v),
            Self::Rbf { gamma } => {
                let d2: f64 = u.iter().zip(v).map(|(a, b)| (a - b) * (a - b)).sum();
                (-gamma * d2).exp()
            }
            Self::Polynomial { degree, gamma, coef0 } => (gamma * dot(u, v) + coef0).powi(degree as i32),
        }
    }

    /// The kernel's γ (`None` for the linear kernel).
    #[must_use]
    pub fn gamma(&self) -> Option<f64> {
        match *self {
            Self::Linear => None,
            Self::Rbf { gamma } | Self::Polynomial { gamma, .. } => Some(gamma),
        }
    }

    fn check(&self) -> Result<()> {
        let ok = match *self {
            Self::Linear => true,
            Self::Rbf { gamma } => gamma > 0.0 && gamma.is_finite(),
            Self::Polynomial { degree, gamma, coef0 } => {
                degree >= 1 && gamma > 0.0 && gamma.is_finite() && coef0 >= 0.0 && coef0.is_finite()
            }
        };
        if ok { Ok(()) } else { Err(invalid("invalid kernel parameters (γ must be positive, degree at least 1)")) }
    }
}

fn dot(u: &[f64], v: &[f64]) -> f64 {
    u.iter().zip(v).map(|(a, b)| a * b).sum()
}

/// Solver settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SvmOptions {
    /// Stopping tolerance on the largest violation of the optimality
    /// conditions.
    pub tolerance: f64,
    /// Most SMO iterations (at least 100 per variable are allowed).
    pub max_iter: usize,
    /// Kernel values kept in the row cache.
    pub cache_entries: usize,
}

impl Default for SvmOptions {
    fn default() -> Self {
        Self { tolerance: 1e-3, max_iter: 10_000_000, cache_entries: 16_000_000 }
    }
}

/// Kernel rows over a set of points, computed on demand and cached.
struct KernelRows<'a> {
    x: &'a [&'a [f64]],
    kernel: Kernel,
    rows: Vec<Option<Rc<Vec<f64>>>>,
    order: VecDeque<usize>,
    capacity: usize,
}

impl<'a> KernelRows<'a> {
    fn new(x: &'a [&'a [f64]], kernel: Kernel, entries: usize) -> Self {
        let n = x.len();
        Self { x, kernel, rows: vec![None; n], order: VecDeque::new(), capacity: (entries / n.max(1)).max(2) }
    }

    fn row(&mut self, i: usize) -> Rc<Vec<f64>> {
        if let Some(r) = &self.rows[i] {
            return Rc::clone(r);
        }
        let xi = self.x[i];
        let r: Rc<Vec<f64>> = Rc::new(self.x.iter().map(|xj| self.kernel.eval(xi, xj)).collect());
        if self.order.len() >= self.capacity {
            if let Some(old) = self.order.pop_front() {
                self.rows[old] = None;
            }
        }
        self.order.push_back(i);
        self.rows[i] = Some(Rc::clone(&r));
        r
    }
}

/// The solution of the dual problem.
struct Solution {
    alpha: Vec<f64>,
    rho: f64,
    iterations: usize,
    converged: bool,
}

/// SMO with second-order working-set selection (LIBSVM's `Solver`, without
/// shrinking). Variable `t` belongs to point `point[t]` with label `y[t]`.
fn solve(kernel: &mut KernelRows<'_>, point: &[usize], y: &[f64], p: &[f64], c: f64, options: &SvmOptions) -> Solution {
    const TAU: f64 = 1e-12;
    let n = y.len();
    let qd: Vec<f64> = point.iter().map(|&s| kernel.kernel.eval(kernel.x[s], kernel.x[s])).collect();
    let mut alpha = vec![0.0; n];
    let mut grad = p.to_vec();
    let max_iter = options.max_iter.max(100 * n);
    let upper = |a: f64| a >= c;
    let lower = |a: f64| a <= 0.0;
    let mut iterations = 0;
    let mut converged = false;
    while iterations < max_iter {
        // i: the most violating variable among those that can move up.
        let mut gmax = f64::NEG_INFINITY;
        let mut i = usize::MAX;
        for t in 0..n {
            let can = if y[t] > 0.0 { !upper(alpha[t]) } else { !lower(alpha[t]) };
            if can && -y[t] * grad[t] >= gmax {
                gmax = -y[t] * grad[t];
                i = t;
            }
        }
        // j: the largest second-order decrease among those that can move down.
        let mut gmax2 = f64::NEG_INFINITY;
        let mut j = usize::MAX;
        let mut best = f64::INFINITY;
        let ki = (i != usize::MAX).then(|| kernel.row(point[i]));
        for t in 0..n {
            let can = if y[t] > 0.0 { !lower(alpha[t]) } else { !upper(alpha[t]) };
            if !can {
                continue;
            }
            let yg = y[t] * grad[t];
            if yg >= gmax2 {
                gmax2 = yg;
            }
            let Some(ki) = &ki else { continue };
            let diff = gmax + yg;
            if diff > 0.0 {
                // ‖φ(xᵢ) − φ(xₜ)‖² = Kᵢᵢ + Kₜₜ − 2Kᵢₜ for either label of t
                // (LIBSVM's QDᵢ + QDₜ ∓ 2yᵢQᵢₜ with Qᵢₜ = yᵢyₜKᵢₜ).
                let quad = qd[i] + qd[t] - 2.0 * ki[point[t]];
                let obj = -(diff * diff) / if quad > 0.0 { quad } else { TAU };
                if obj <= best {
                    best = obj;
                    j = t;
                }
            }
        }
        if gmax + gmax2 < options.tolerance || i == usize::MAX || j == usize::MAX {
            converged = true;
            break;
        }
        iterations += 1;
        let ki = ki.unwrap_or_else(|| kernel.row(point[i]));
        let kj = kernel.row(point[j]);
        let qij = y[i] * y[j] * ki[point[j]];
        let (old_i, old_j) = (alpha[i], alpha[j]);
        if y[i] != y[j] {
            let quad = qd[i] + qd[j] + 2.0 * qij;
            let delta = (-grad[i] - grad[j]) / if quad > 0.0 { quad } else { TAU };
            let diff = alpha[i] - alpha[j];
            alpha[i] += delta;
            alpha[j] += delta;
            if diff > 0.0 {
                if alpha[j] < 0.0 {
                    alpha[j] = 0.0;
                    alpha[i] = diff;
                }
            } else if alpha[i] < 0.0 {
                alpha[i] = 0.0;
                alpha[j] = -diff;
            }
            if diff > 0.0 {
                if alpha[i] > c {
                    alpha[i] = c;
                    alpha[j] = c - diff;
                }
            } else if alpha[j] > c {
                alpha[j] = c;
                alpha[i] = c + diff;
            }
        } else {
            let quad = qd[i] + qd[j] - 2.0 * qij;
            let delta = (grad[i] - grad[j]) / if quad > 0.0 { quad } else { TAU };
            let sum = alpha[i] + alpha[j];
            alpha[i] -= delta;
            alpha[j] += delta;
            if sum > c {
                if alpha[i] > c {
                    alpha[i] = c;
                    alpha[j] = sum - c;
                }
            } else if alpha[j] < 0.0 {
                alpha[j] = 0.0;
                alpha[i] = sum;
            }
            if sum > c {
                if alpha[j] > c {
                    alpha[j] = c;
                    alpha[i] = sum - c;
                }
            } else if alpha[i] < 0.0 {
                alpha[i] = 0.0;
                alpha[j] = sum;
            }
        }
        let (di, dj) = (alpha[i] - old_i, alpha[j] - old_j);
        for t in 0..n {
            grad[t] += y[t] * (y[i] * ki[point[t]] * di + y[j] * kj[point[t]] * dj);
        }
    }
    // Offset.
    let (mut ub, mut lb) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut sum_free, mut n_free) = (0.0, 0usize);
    for t in 0..n {
        let yg = y[t] * grad[t];
        if upper(alpha[t]) {
            if y[t] < 0.0 { ub = ub.min(yg) } else { lb = lb.max(yg) }
        } else if lower(alpha[t]) {
            if y[t] > 0.0 { ub = ub.min(yg) } else { lb = lb.max(yg) }
        } else {
            n_free += 1;
            sum_free += yg;
        }
    }
    let rho = if n_free > 0 {
        sum_free / n_free as f64
    } else if ub.is_finite() && lb.is_finite() {
        (ub + lb) / 2.0
    } else if ub.is_finite() {
        ub
    } else if lb.is_finite() {
        lb
    } else {
        0.0
    };
    Solution { alpha, rho, iterations, converged }
}

/// A fitted machine: `f(x) = Σ coefᵢ K(svᵢ, x) − ρ`.
#[derive(Debug, Clone, PartialEq)]
pub struct SvmMachine {
    /// Kernel.
    pub kernel: Kernel,
    /// Support vectors (points with a nonzero coefficient).
    pub support: Vec<Vec<f64>>,
    /// Indices of the support vectors among the training points.
    pub support_index: Vec<usize>,
    /// Coefficient of each support vector (`yᵢαᵢ`, or `αᵢ − αᵢ*`).
    pub coef: Vec<f64>,
    /// Offset ρ.
    pub rho: f64,
    /// Support vectors at the bound C.
    pub bounded: usize,
    /// SMO iterations.
    pub iterations: usize,
    /// Whether the solver met its tolerance.
    pub converged: bool,
}

impl SvmMachine {
    /// Decision value `f(x)`.
    #[must_use]
    pub fn decision(&self, x: &[f64]) -> f64 {
        self.support.iter().zip(&self.coef).map(|(s, c)| c * self.kernel.eval(s, x)).sum::<f64>() - self.rho
    }

    fn from_coefficients(x: &[&[f64]], kernel: Kernel, coef: &[f64], c: f64, s: &Solution) -> Self {
        let keep: Vec<usize> = (0..coef.len()).filter(|&i| coef[i] != 0.0).collect();
        Self {
            kernel,
            support: keep.iter().map(|&i| x[i].to_vec()).collect(),
            support_index: keep.clone(),
            coef: keep.iter().map(|&i| coef[i]).collect(),
            rho: s.rho,
            bounded: keep.iter().filter(|&&i| coef[i].abs() >= c).count(),
            iterations: s.iterations,
            converged: s.converged,
        }
    }

    /// The weight vector `w = Σ coefᵢ svᵢ` of a linear machine (`None` for
    /// other kernels).
    #[must_use]
    pub fn linear_weights(&self) -> Option<Vec<f64>> {
        if self.kernel != Kernel::Linear {
            return None;
        }
        let p = self.support.first().map_or(0, Vec::len);
        let mut w = vec![0.0; p];
        for (s, c) in self.support.iter().zip(&self.coef) {
            for (wk, sk) in w.iter_mut().zip(s) {
                *wk += c * sk;
            }
        }
        Some(w)
    }
}

fn check_points(x: &[&[f64]], cost: f64, kernel: Kernel) -> Result<()> {
    if x.is_empty() {
        return Err(invalid("no training rows"));
    }
    let p = x[0].len();
    if x.iter().any(|r| r.len() != p || r.iter().any(|v| !v.is_finite())) {
        return Err(invalid("every training row needs a finite value for every predictor"));
    }
    if !(cost > 0.0 && cost.is_finite()) {
        return Err(invalid("the cost C must be positive"));
    }
    kernel.check()
}

/// Fit a two-class C-SVC: `positive[i]` is whether point `i` is in the +1
/// class.
///
/// # Errors
/// No points, unequal or non-finite rows, a single class, `cost ≤ 0` or
/// invalid kernel parameters.
pub fn fit_svc(x: &[&[f64]], positive: &[bool], cost: f64, kernel: Kernel, options: &SvmOptions) -> Result<SvmMachine> {
    check_points(x, cost, kernel)?;
    if positive.len() != x.len() {
        return Err(invalid("one label per training row is needed"));
    }
    if positive.iter().all(|&b| b) || positive.iter().all(|&b| !b) {
        return Err(invalid("both classes need training rows"));
    }
    let n = x.len();
    let y: Vec<f64> = positive.iter().map(|&b| if b { 1.0 } else { -1.0 }).collect();
    let point: Vec<usize> = (0..n).collect();
    let mut rows = KernelRows::new(x, kernel, options.cache_entries);
    let s = solve(&mut rows, &point, &y, &vec![-1.0; n], cost, options);
    let coef: Vec<f64> = (0..n).map(|i| y[i] * s.alpha[i]).collect();
    Ok(SvmMachine::from_coefficients(x, kernel, &coef, cost, &s))
}

/// Fit an ε-SVR to targets `z`.
///
/// # Errors
/// No points, unequal or non-finite rows or targets, `cost ≤ 0`, `ε < 0` or
/// invalid kernel parameters.
pub fn fit_svr(
    x: &[&[f64]],
    z: &[f64],
    cost: f64,
    epsilon: f64,
    kernel: Kernel,
    options: &SvmOptions,
) -> Result<SvmMachine> {
    check_points(x, cost, kernel)?;
    if z.len() != x.len() || z.iter().any(|v| !v.is_finite()) {
        return Err(invalid("one finite target per training row is needed"));
    }
    if !(epsilon >= 0.0 && epsilon.is_finite()) {
        return Err(invalid("ε must not be negative"));
    }
    let n = x.len();
    let point: Vec<usize> = (0..2 * n).map(|t| t % n).collect();
    let y: Vec<f64> = (0..2 * n).map(|t| if t < n { 1.0 } else { -1.0 }).collect();
    let p: Vec<f64> = (0..2 * n).map(|t| if t < n { epsilon - z[t] } else { epsilon + z[t - n] }).collect();
    let mut rows = KernelRows::new(x, kernel, options.cache_entries);
    let s = solve(&mut rows, &point, &y, &p, cost, options);
    let coef: Vec<f64> = (0..n).map(|i| s.alpha[i] - s.alpha[i + n]).collect();
    Ok(SvmMachine::from_coefficients(x, kernel, &coef, cost, &s))
}

/// Platt's sigmoid `P(+1 | f) = 1 / (1 + exp(A f + B))`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlattScaling {
    /// Slope A (negative when larger decision values favour +1).
    pub a: f64,
    /// Offset B.
    pub b: f64,
}

impl PlattScaling {
    /// `P(+1 | f)`.
    #[must_use]
    pub fn probability(&self, f: f64) -> f64 {
        let t = f * self.a + self.b;
        if t >= 0.0 { (-t).exp() / (1.0 + (-t).exp()) } else { 1.0 / (1.0 + t.exp()) }
    }
}

/// Fit Platt's sigmoid to decision values and labels by Newton's method with
/// backtracking, on the regularized targets `(N₊ + 1)/(N₊ + 2)` and
/// `1/(N₋ + 2)` (Lin, Lin & Weng 2007).
#[must_use]
pub fn fit_platt(decision: &[f64], positive: &[bool]) -> PlattScaling {
    let prior1 = positive.iter().filter(|&&b| b).count() as f64;
    let prior0 = positive.len() as f64 - prior1;
    let hi = (prior1 + 1.0) / (prior1 + 2.0);
    let lo = 1.0 / (prior0 + 2.0);
    let t: Vec<f64> = positive.iter().map(|&b| if b { hi } else { lo }).collect();
    let objective = |a: f64, b: f64| -> f64 {
        decision
            .iter()
            .zip(&t)
            .map(|(&f, &ti)| {
                let z = f * a + b;
                if z >= 0.0 { ti * z + (-z).exp().ln_1p() } else { (ti - 1.0) * z + z.exp().ln_1p() }
            })
            .sum()
    };
    let (mut a, mut b) = (0.0, ((prior0 + 1.0) / (prior1 + 1.0)).ln());
    let mut fval = objective(a, b);
    for _ in 0..100 {
        let (mut h11, mut h22, mut h21, mut g1, mut g2) = (1e-12, 1e-12, 0.0, 0.0, 0.0);
        for (&f, &ti) in decision.iter().zip(&t) {
            let z = f * a + b;
            let (p, q) = if z >= 0.0 {
                let e = (-z).exp();
                (e / (1.0 + e), 1.0 / (1.0 + e))
            } else {
                let e = z.exp();
                (1.0 / (1.0 + e), e / (1.0 + e))
            };
            let d2 = p * q;
            h11 += f * f * d2;
            h22 += d2;
            h21 += f * d2;
            let d1 = ti - p;
            g1 += f * d1;
            g2 += d1;
        }
        if g1.abs() < 1e-5 && g2.abs() < 1e-5 {
            break;
        }
        let det = h11 * h22 - h21 * h21;
        let da = -(h22 * g1 - h21 * g2) / det;
        let db = -(-h21 * g1 + h11 * g2) / det;
        let gd = g1 * da + g2 * db;
        let mut step = 1.0;
        while step >= 1e-10 {
            let (na, nb) = (a + step * da, b + step * db);
            let nf = objective(na, nb);
            if nf < fval + 1e-4 * step * gd {
                (a, b, fval) = (na, nb, nf);
                break;
            }
            step /= 2.0;
        }
        if step < 1e-10 {
            break;
        }
    }
    PlattScaling { a, b }
}

/// The flat scaling `A = 0`, `P(+1) = (N₊ + 1)/(N + 2)`, for a machine whose
/// decision does not vary.
fn constant_scaling(positive: &[bool]) -> PlattScaling {
    let n1 = positive.iter().filter(|&&b| b).count() as f64;
    let n0 = positive.len() as f64 - n1;
    PlattScaling { a: 0.0, b: ((n0 + 1.0) / (n1 + 1.0)).ln() }
}

/// Combine pairwise probabilities `r[i][j] = P(i | i or j)` into class
/// probabilities (Wu, Lin & Weng 2004, method 2).
#[must_use]
pub fn pairwise_coupling(r: &[Vec<f64>]) -> Vec<f64> {
    let k = r.len();
    if k == 1 {
        return vec![1.0];
    }
    let mut q = vec![vec![0.0; k]; k];
    for t in 0..k {
        for j in 0..k {
            if j != t {
                q[t][t] += r[j][t] * r[j][t];
                q[t][j] = -r[j][t] * r[t][j];
            }
        }
    }
    let mut p = vec![1.0 / k as f64; k];
    let mut qp = vec![0.0; k];
    let eps = 0.005 / k as f64;
    for _ in 0..100.max(k) {
        let mut pqp = 0.0;
        for t in 0..k {
            qp[t] = (0..k).map(|j| q[t][j] * p[j]).sum();
            pqp += p[t] * qp[t];
        }
        if qp.iter().all(|&v| (v - pqp).abs() < eps) {
            break;
        }
        for t in 0..k {
            let diff = (-qp[t] + pqp) / q[t][t];
            p[t] += diff;
            pqp = (pqp + diff * (diff * q[t][t] + 2.0 * qp[t])) / (1.0 + diff) / (1.0 + diff);
            for j in 0..k {
                qp[j] = (qp[j] + diff * q[t][j]) / (1.0 + diff);
                p[j] /= 1.0 + diff;
            }
        }
    }
    p
}

/// One machine of a one-against-one classifier: class `first` is +1.
#[derive(Debug, Clone, PartialEq)]
pub struct PairMachine {
    /// The +1 class.
    pub first: usize,
    /// The −1 class.
    pub second: usize,
    /// The machine.
    pub machine: SvmMachine,
    /// Its probability scaling.
    pub platt: Option<PlattScaling>,
    /// `true` when the machine's decision is (numerically) the same for every
    /// training point: the optimum puts the weights at zero and predicts one
    /// class everywhere, so its decision values rank nothing.
    pub constant: bool,
}

/// Spread of the training decision values below which a machine counts as
/// constant. Free support vectors sit on the margins `f = ±1`, so a machine
/// with any free support vector of each class spreads by at least 2.
pub const CONSTANT_DECISION_SPREAD: f64 = 1e-2;

/// A one-against-one classifier over `classes` classes.
#[derive(Debug, Clone, PartialEq)]
pub struct SvmClassifier {
    /// Number of classes.
    pub classes: usize,
    /// One machine per pair of classes present in the training rows.
    pub pairs: Vec<PairMachine>,
    /// Number of distinct support vectors over all machines.
    pub support_vectors: usize,
}

impl SvmClassifier {
    /// The class with the most pairwise votes (ties to the lower class).
    #[must_use]
    pub fn vote(&self, x: &[f64]) -> usize {
        let mut votes = vec![0usize; self.classes];
        for m in &self.pairs {
            votes[if m.machine.decision(x) > 0.0 { m.first } else { m.second }] += 1;
        }
        (0..self.classes).max_by_key(|&k| (votes[k], usize::MAX - k)).unwrap_or(0)
    }

    /// Class probabilities by Platt scaling and pairwise coupling (one-hot
    /// votes when the machines carry no scaling). Classes absent from
    /// training get probability 0.
    #[must_use]
    pub fn probabilities(&self, x: &[f64]) -> Vec<f64> {
        if self.pairs.iter().any(|m| m.platt.is_none()) {
            let mut p = vec![0.0; self.classes];
            p[self.vote(x)] = 1.0;
            return p;
        }
        let mut present: Vec<usize> = self.pairs.iter().flat_map(|m| [m.first, m.second]).collect();
        present.sort_unstable();
        present.dedup();
        let at = |c: usize| present.iter().position(|&k| k == c).unwrap_or(0);
        let k = present.len();
        let mut r = vec![vec![0.0; k]; k];
        for m in &self.pairs {
            let pr = m.platt.map_or(0.5, |s| s.probability(m.machine.decision(x))).clamp(1e-7, 1.0 - 1e-7);
            r[at(m.first)][at(m.second)] = pr;
            r[at(m.second)][at(m.first)] = 1.0 - pr;
        }
        let coupled = if k == 2 { vec![r[0][1], r[1][0]] } else { pairwise_coupling(&r) };
        let mut p = vec![0.0; self.classes];
        for (i, &c) in present.iter().enumerate() {
            p[c] = coupled[i];
        }
        p
    }
}

/// Fit a one-against-one classifier: `class[i] < classes` is point `i`'s
/// class. With `probability`, each machine also gets a Platt scaling fitted
/// to 5-fold cross-validated decision values (folds from `seed`).
///
/// # Errors
/// Fewer than two classes in the training rows, or the errors of
/// [`fit_svc`].
pub fn fit_classifier(
    x: &[&[f64]],
    class: &[usize],
    classes: usize,
    cost: f64,
    kernel: Kernel,
    probability: Option<u64>,
    options: &SvmOptions,
) -> Result<SvmClassifier> {
    if class.len() != x.len() || class.iter().any(|&c| c >= classes) {
        return Err(invalid("one class per training row is needed"));
    }
    let mut counts = vec![0usize; classes];
    for &c in class {
        counts[c] += 1;
    }
    let present: Vec<usize> = (0..classes).filter(|&c| counts[c] > 0).collect();
    if present.len() < 2 {
        return Err(invalid("at least two classes need training rows"));
    }
    let pairs: Vec<(usize, usize)> =
        present.iter().enumerate().flat_map(|(a, &i)| present[a + 1..].iter().map(move |&j| (i, j))).collect();
    let fitted: Vec<Result<PairMachine>> = pairs
        .par_iter()
        .map(|&(first, second)| {
            let rows: Vec<usize> = (0..x.len()).filter(|&r| class[r] == first || class[r] == second).collect();
            let px: Vec<&[f64]> = rows.iter().map(|&r| x[r]).collect();
            let pos: Vec<bool> = rows.iter().map(|&r| class[r] == first).collect();
            let machine = fit_svc(&px, &pos, cost, kernel, options)?;
            let (lo, hi) = px
                .iter()
                .map(|v| machine.decision(v))
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), f| (a.min(f), b.max(f)));
            let constant = hi - lo < CONSTANT_DECISION_SPREAD;
            let platt = probability.map(|seed| {
                if constant {
                    // No ranking to calibrate: every point gets the
                    // regularized share of the +1 class (slope zero), rather
                    // than a slope fitted to rounding noise.
                    return constant_scaling(&pos);
                }
                let decision = cross_validated_decisions(&px, &pos, cost, kernel, seed, options)
                    .unwrap_or_else(|| px.iter().map(|v| machine.decision(v)).collect());
                fit_platt(&decision, &pos)
            });
            Ok(PairMachine { first, second, machine, platt, constant })
        })
        .collect();
    let pairs = fitted.into_iter().collect::<Result<Vec<_>>>()?;
    let support_vectors = {
        let mut all: Vec<usize> = Vec::new();
        for m in &pairs {
            let rows: Vec<usize> = (0..x.len()).filter(|&r| class[r] == m.first || class[r] == m.second).collect();
            all.extend(m.machine.support_index.iter().map(|&i| rows[i]));
        }
        all.sort_unstable();
        all.dedup();
        all.len()
    };
    Ok(SvmClassifier { classes, pairs, support_vectors })
}

/// Decision values of each point from a 5-fold cross-validation (`None`
/// when a fold leaves a single class or there are too few points).
fn cross_validated_decisions(
    x: &[&[f64]],
    positive: &[bool],
    cost: f64,
    kernel: Kernel,
    seed: u64,
    options: &SvmOptions,
) -> Option<Vec<f64>> {
    let n = x.len();
    if n < 10 {
        return None;
    }
    let fold = crate::validation::fold_assignment(n, 5, seed).ok()?;
    let mut out = vec![0.0; n];
    for f in 0..5 {
        let train: Vec<usize> = (0..n).filter(|&i| fold[i] != f).collect();
        let tx: Vec<&[f64]> = train.iter().map(|&i| x[i]).collect();
        let ty: Vec<bool> = train.iter().map(|&i| positive[i]).collect();
        let m = fit_svc(&tx, &ty, cost, kernel, options).ok()?;
        for i in (0..n).filter(|&i| fold[i] == f) {
            out[i] = m.decision(x[i]);
        }
    }
    Some(out)
}

/// What a support vector machine predicts.
#[derive(Debug, Clone, PartialEq)]
pub enum SvmTask {
    /// Classes `0..classes` (one per point).
    Classification {
        /// Class of each point.
        class: Vec<usize>,
        /// Number of classes.
        classes: usize,
    },
    /// A continuous target with insensitive-zone half-width ε.
    Regression {
        /// Target of each point.
        target: Vec<f64>,
        /// ε.
        epsilon: f64,
    },
}

/// Held-out rows for tuning.
#[derive(Debug, Clone, PartialEq)]
pub enum TuningSplit {
    /// Fit on `train`, measure on `valid` (point indices).
    Holdout {
        /// Fitting points.
        train: Vec<usize>,
        /// Measuring points.
        valid: Vec<usize>,
    },
    /// The fold of each point.
    Folds(Vec<usize>),
}

/// Held-out error of a model fitted on `train` and measured on `valid`:
/// misclassification count (by vote) or sum of squared errors.
fn held_out_loss(
    x: &[&[f64]],
    task: &SvmTask,
    train: &[usize],
    valid: &[usize],
    kernel: Kernel,
    cost: f64,
    options: &SvmOptions,
) -> Result<f64> {
    let tx: Vec<&[f64]> = train.iter().map(|&i| x[i]).collect();
    Ok(match task {
        SvmTask::Classification { class, classes } => {
            let ty: Vec<usize> = train.iter().map(|&i| class[i]).collect();
            let m = fit_classifier(&tx, &ty, *classes, cost, kernel, None, options)?;
            valid.iter().filter(|&&i| m.vote(x[i]) != class[i]).count() as f64
        }
        SvmTask::Regression { target, epsilon } => {
            let ty: Vec<f64> = train.iter().map(|&i| target[i]).collect();
            let m = fit_svr(&tx, &ty, cost, *epsilon, kernel, options)?;
            valid.iter().map(|&i| (m.decision(x[i]) - target[i]).powi(2)).sum()
        }
    })
}

/// The held-out error of each candidate `(kernel, cost)`: misclassification
/// rate (by vote) or mean squared error, on the validation points or over
/// the folds. Candidates are fitted in parallel.
///
/// # Errors
/// An empty split, or a fit that fails (for example a fold whose training
/// points hold a single class).
pub fn tune(
    x: &[&[f64]],
    task: &SvmTask,
    candidates: &[(Kernel, f64)],
    split: &TuningSplit,
    options: &SvmOptions,
) -> Result<Vec<f64>> {
    let all: Vec<usize> = (0..x.len()).collect();
    let pieces: Vec<(Vec<usize>, Vec<usize>)> = match split {
        TuningSplit::Holdout { train, valid } => vec![(train.clone(), valid.clone())],
        TuningSplit::Folds(fold) => {
            let k = fold.iter().copied().max().map_or(0, |m| m + 1);
            (0..k)
                .map(|f| {
                    let (v, t): (Vec<usize>, Vec<usize>) = all.iter().partition(|&&i| fold[i] == f);
                    (t, v)
                })
                .filter(|(_, v)| !v.is_empty())
                .collect()
        }
    };
    let measured: usize = pieces.iter().map(|(_, v)| v.len()).sum();
    if measured == 0 || pieces.iter().any(|(t, _)| t.is_empty()) {
        return Err(invalid("tuning needs rows to fit and rows to measure"));
    }
    candidates
        .par_iter()
        .map(|&(kernel, cost)| {
            let mut total = 0.0;
            for (t, v) in &pieces {
                total += held_out_loss(x, task, t, v, kernel, cost, options)?;
            }
            Ok(total / measured as f64)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refs(x: &[Vec<f64>]) -> Vec<&[f64]> {
        x.iter().map(Vec::as_slice).collect()
    }

    /// Two separable points: the maximum-margin solution is known in closed
    /// form (w = (1, 0), b = 0 for points at x = ±1, margin 1).
    #[test]
    fn two_points_closed_form() {
        let x = vec![vec![1.0, 0.0], vec![-1.0, 0.0]];
        let m = fit_svc(&refs(&x), &[true, false], 10.0, Kernel::Linear, &SvmOptions::default()).unwrap();
        let w = m.linear_weights().unwrap();
        assert!((w[0] - 1.0).abs() < 1e-9 && w[1].abs() < 1e-9, "{w:?}");
        assert!(m.rho.abs() < 1e-9);
        assert!((m.decision(&[1.0, 0.0]) - 1.0).abs() < 1e-9);
        assert_eq!(m.support.len(), 2);
    }

    /// Four separable points: hard-margin solution (Burges 1998 style
    /// example): positives at (2, 2), (2, 3); negatives at (0, 0), (1, 0).
    /// The support vectors and margin satisfy the KKT conditions: every
    /// support vector lies on |f| = 1 and every other point outside.
    #[test]
    fn kkt_conditions_on_separable_data() {
        let x = vec![vec![2.0, 2.0], vec![2.0, 3.0], vec![0.0, 0.0], vec![1.0, 0.0], vec![3.0, 4.0]];
        let pos = [true, true, false, false, true];
        let m = fit_svc(&refs(&x), &pos, 1e6, Kernel::Linear, &SvmOptions { tolerance: 1e-8, ..Default::default() })
            .unwrap();
        for (i, xi) in x.iter().enumerate() {
            let yf = if pos[i] { 1.0 } else { -1.0 } * m.decision(xi);
            if m.support_index.contains(&i) {
                assert!((yf - 1.0).abs() < 1e-6, "support vector {i}: {yf}");
            } else {
                assert!(yf > 1.0 - 1e-6, "point {i}: {yf}");
            }
        }
        // Σ coef = 0 (the equality constraint).
        assert!(m.coef.iter().sum::<f64>().abs() < 1e-9);
        // Margin: w = (0.4, 0.8)·… check with the known solution
        // through (1,0) and (2,2): w ∝ (1, 2), f = (2/5)(x + 2y) − 7/5.
        let w = m.linear_weights().unwrap();
        assert!((w[0] - 0.4).abs() < 1e-5 && (w[1] - 0.8).abs() < 1e-5, "{w:?}");
        assert!((m.rho - 1.4).abs() < 1e-5, "{}", m.rho);
    }

    /// The RBF kernel separates a ring that no line can.
    #[test]
    fn rbf_separates_a_ring() {
        let mut rng = stat_core::rng::Rng::new(7);
        let mut x = Vec::new();
        let mut pos = Vec::new();
        for _ in 0..200 {
            let (a, b) = (rng.next_f64() * 4.0 - 2.0, rng.next_f64() * 4.0 - 2.0);
            x.push(vec![a, b]);
            pos.push(a * a + b * b < 1.2);
        }
        let opts = SvmOptions::default();
        let rbf = fit_svc(&refs(&x), &pos, 10.0, Kernel::Rbf { gamma: 1.0 }, &opts).unwrap();
        let lin = fit_svc(&refs(&x), &pos, 10.0, Kernel::Linear, &opts).unwrap();
        let errors = |m: &SvmMachine| x.iter().zip(&pos).filter(|(xi, p)| (m.decision(xi) > 0.0) != **p).count();
        assert!(errors(&rbf) <= 6, "rbf {}", errors(&rbf));
        assert!(errors(&lin) > 30, "linear {}", errors(&lin));
        assert!(rbf.converged);
    }

    /// ε-SVR with a linear kernel on exact linear data recovers the line, and
    /// every residual is within ε.
    #[test]
    fn svr_recovers_a_line() {
        let x: Vec<Vec<f64>> = (0..30).map(|i| vec![i as f64 / 10.0]).collect();
        let z: Vec<f64> = x.iter().map(|v| 2.0 * v[0] - 1.0).collect();
        let m = fit_svr(&refs(&x), &z, 100.0, 0.05, Kernel::Linear, &SvmOptions::default()).unwrap();
        for (xi, zi) in x.iter().zip(&z) {
            assert!((m.decision(xi) - zi).abs() <= 0.05 + 2e-3, "{} vs {zi}", m.decision(xi));
        }
        let w = m.linear_weights().unwrap();
        assert!((w[0] - 2.0).abs() < 0.05, "{w:?}");
    }

    /// Data symmetric under x → −x with the classes nested (inner +1, outer
    /// −1): by symmetry and convexity the linear optimum has w = 0, so the
    /// machine is constant and its probabilities are the flat prior
    /// (N₊ + 1)/(N + 2), not a slope fitted to rounding noise. (LIBSVM, via
    /// scikit-learn 1.x `SVC(kernel="linear")`, gives w = 0 on such data too.)
    #[test]
    fn constant_machine_gets_flat_probabilities() {
        let xs: Vec<f64> = (0..24).map(|i| f64::from(i - 12) + 0.5).collect();
        let x: Vec<Vec<f64>> = xs.iter().map(|&v| vec![v]).collect();
        let class: Vec<usize> = xs.iter().map(|v| usize::from(v.abs() > 6.0)).collect();
        let c = fit_classifier(&refs(&x), &class, 2, 1.0, Kernel::Linear, Some(3), &SvmOptions::default()).unwrap();
        assert!(c.pairs[0].constant);
        let p0 = c.probabilities(&[0.0])[0];
        assert!((p0 - 13.0 / 26.0).abs() < 1e-12, "12 inner points: (12 + 1)/(24 + 2)");
        assert!((c.probabilities(&[11.5])[0] - p0).abs() < 1e-12, "flat");
        let r = fit_classifier(&refs(&x), &class, 2, 1.0, Kernel::Rbf { gamma: 0.05 }, Some(3), &SvmOptions::default())
            .unwrap();
        assert!(!r.pairs[0].constant, "a radial kernel separates the nested classes");
    }

    /// Platt scaling: A < 0, and the probability rises with the decision.
    #[test]
    fn platt_and_coupling() {
        let f: Vec<f64> = (0..100).map(|i| (i as f64 - 50.0) / 10.0).collect();
        let y: Vec<bool> = f.iter().enumerate().map(|(i, v)| *v > 0.0 || i % 7 == 0).collect();
        let s = fit_platt(&f, &y);
        assert!(s.a < 0.0, "{s:?}");
        assert!(s.probability(3.0) > 0.9 && s.probability(-3.0) < 0.3);
        // Coupling of consistent pairwise probabilities from p = (0.5, 0.3, 0.2).
        let p = [0.5, 0.3, 0.2];
        let r: Vec<Vec<f64>> =
            (0..3).map(|i| (0..3).map(|j| if i == j { 0.0 } else { p[i] / (p[i] + p[j]) }).collect()).collect();
        let q = pairwise_coupling(&r);
        for (a, b) in p.iter().zip(&q) {
            assert!((a - b).abs() < 0.01, "{q:?}");
        }
        assert!((q.iter().sum::<f64>() - 1.0).abs() < 1e-9);
    }

    /// Three Gaussian classes: one-against-one voting and coupled
    /// probabilities; tuning prefers a sensible cost; seeded fits repeat.
    #[test]
    fn three_classes_and_tuning() {
        let mut rng = stat_core::rng::Rng::new(3);
        let centres = [(0.0, 0.0), (3.0, 0.0), (0.0, 3.0)];
        let mut x = Vec::new();
        let mut class = Vec::new();
        for i in 0..150 {
            let (a, b) = centres[i % 3];
            x.push(vec![a + rng.normal(), b + rng.normal()]);
            class.push(i % 3);
        }
        let opts = SvmOptions::default();
        let c = fit_classifier(&refs(&x), &class, 3, 1.0, Kernel::Rbf { gamma: 0.5 }, Some(1), &opts).unwrap();
        assert_eq!(c.pairs.len(), 3);
        let wrong = x.iter().zip(&class).filter(|(xi, k)| c.vote(xi) != **k).count();
        assert!(wrong < 20, "{wrong}");
        let p = c.probabilities(&[3.0, 0.0]);
        assert!((p.iter().sum::<f64>() - 1.0).abs() < 1e-6 && p[1] > 0.8, "{p:?}");
        let again = fit_classifier(&refs(&x), &class, 3, 1.0, Kernel::Rbf { gamma: 0.5 }, Some(1), &opts).unwrap();
        assert_eq!(again, c);
        let task = SvmTask::Classification { class: class.clone(), classes: 3 };
        let folds = crate::validation::fold_assignment(150, 5, 1).unwrap();
        let cand = [(Kernel::Rbf { gamma: 0.5 }, 1e-3), (Kernel::Rbf { gamma: 0.5 }, 1.0)];
        let e = tune(&refs(&x), &task, &cand, &TuningSplit::Folds(folds), &opts).unwrap();
        assert!(e[1] < e[0] && e[1] < 0.2, "{e:?}");
        // Errors.
        assert!(fit_svc(&refs(&x[..2]), &[true, true], 1.0, Kernel::Linear, &opts).is_err());
        assert!(fit_svc(&refs(&x[..2]), &[true, false], 0.0, Kernel::Linear, &opts).is_err());
        assert!(fit_svc(&refs(&x[..2]), &[true, false], 1.0, Kernel::Rbf { gamma: 0.0 }, &opts).is_err());
    }

    /// The second working variable is chosen by the second-order gain
    /// `(Gmax + yₜ∇ₜ)² / (Kᵢᵢ + Kₜₜ − 2Kᵢₜ)` for both labels of `t` (Fan, Chen &
    /// Lin 2005). Reference: scikit-learn 1.9.1 `SVC(kernel="rbf", gamma=0.5,
    /// C=1, tol=1e-3, shrinking=False)` (LIBSVM) on the same points, given in
    /// LIBSVM's internal order (first class first): 34 iterations, intercept
    /// −0.097589548547; `SVR(C=10, epsilon=0.1, …)`: 143 iterations, intercept
    /// 3.74285155477. With the curvature of a −1 variable taken as
    /// `Kᵢᵢ + Kₜₜ + 2Kᵢₜ`, the same fits took 58 and 403 iterations.
    #[test]
    fn working_set_selection_matches_libsvm() {
        let n = 40;
        let order: Vec<usize> = (0..n).step_by(2).chain((1..n).step_by(2)).collect();
        let x: Vec<Vec<f64>> = order
            .iter()
            .map(|&i| {
                let f = i as f64;
                let odd = (i % 2) as f64;
                vec![(f * 1.3).sin() * 2.0 + odd * 0.8, (f * 0.7).cos() * 1.5 - odd * 0.5]
            })
            .collect();
        let pos: Vec<bool> = order.iter().map(|&i| i % 2 == 0).collect();
        let m = fit_svc(&refs(&x), &pos, 1.0, Kernel::Rbf { gamma: 0.5 }, &SvmOptions::default()).unwrap();
        assert!(m.iterations.abs_diff(34) <= 2, "{} iterations", m.iterations);
        // scikit-learn labels the other class +1, so its intercept is ρ here.
        // At termination every free yᵢ∇ᵢ lies in [M(α), m(α)], an interval
        // narrower than the stopping tolerance, and ρ is their mean. A one-ulp
        // change in the inputs (the platform's sin, cos or exp) can move a
        // variable on or off its bound and so shift ρ within that interval:
        // perturbing these points by one ulp moved it by up to 1.2·10⁻⁴ in
        // the same 34 iterations. ρ is therefore checked to the tolerance.
        let tol = SvmOptions::default().tolerance;
        assert!((m.rho - -0.097_589_548_547).abs() < tol, "{}", m.rho);
        let xs: Vec<Vec<f64>> = (0..n).map(|i| vec![i as f64 / 10.0, (i as f64).cos()]).collect();
        let z: Vec<f64> = (0..n).map(|i| (i as f64 * 0.37).sin() * 3.0 + i as f64 * 0.1).collect();
        let r = fit_svr(&refs(&xs), &z, 10.0, 0.1, Kernel::Rbf { gamma: 0.5 }, &SvmOptions::default()).unwrap();
        assert!(r.iterations < 170, "{} iterations", r.iterations);
        assert!((-r.rho - 3.742_851_554_77).abs() < 1e-3, "{}", r.rho);
    }
}
