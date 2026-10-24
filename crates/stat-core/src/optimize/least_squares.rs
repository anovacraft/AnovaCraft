//! Nonlinear least squares by Levenberg–Marquardt with Gauss–Newton steps.
//!
//! # Problem
//! Minimise `S(θ) = Σᵢ rᵢ(θ)²` over parameters `θ` (length `p`) inside an
//! optional box `lower ≤ θ ≤ upper`, for a residual vector `r(θ)` of length
//! `n ≥ p` with Jacobian `J = ∂r/∂θ` (`n × p`).
//!
//! # Method
//! Each iteration solves the damped linearised problem
//!
//! ```text
//! min_δ ‖ [ J ; √λ D ] δ + [ r ; 0 ] ‖²
//! ```
//!
//! by pivoted Householder QR of the augmented matrix (never forming `JᵀJ`).
//! `D` is diagonal with `Dⱼⱼ` the largest column norm `‖J·ⱼ‖` seen so far,
//! which makes the step invariant to the units of each parameter (Moré, "The
//! Levenberg–Marquardt algorithm: implementation and theory", *Lecture Notes
//! in Mathematics* 630, 1978). `λ = 0` is the Gauss–Newton step. A step that
//! lowers `S` is accepted and `λ` divided by 10 (down to exactly zero, so the
//! iterations end in pure Gauss–Newton steps and keep its quadratic rate on
//! small-residual problems); a step that does not is rejected and `λ`
//! multiplied by 10 (Marquardt, "An algorithm for least-squares estimation of
//! nonlinear parameters", *J. SIAM* 11, 1963).
//!
//! The iterations start cautiously: `λ = 100` and steps no longer than the
//! scaled parameter vector, `‖Dδ‖ ≤ ‖Dθ‖` (a fixed trust region in the
//! spirit of Moré's step bound), which keeps a poor start from jumping into a
//! region where the model is flat in some parameter. If that run does not
//! converge, the iterations are repeated from the start with `λ = 10⁻³` and
//! unbounded steps, and the better result is kept.
//!
//! # Bounds
//! Trial points are projected onto the box. A parameter sitting on a bound
//! whose gradient component pushes it outward is held fixed for the step
//! (an active-set projected Gauss–Newton step); at the solution such
//! parameters are reported in [`LeastSquaresResult::at_bound`].
//!
//! # Convergence
//! The primary criterion is Bates & Watts' relative offset ("A relative
//! offset orthogonality convergence criterion for nonlinear least squares",
//! *Technometrics* 23, 1981): with `QᵀR` the QR of the Jacobian over the
//! free parameters and `k` their number,
//!
//! ```text
//! relative offset = √( ‖Q₁ᵀ r‖² / k ) / √( (‖r‖² − ‖Q₁ᵀ r‖²) / (n − k) )
//! ```
//!
//! — the length of the remaining Gauss–Newton increment in the expectation
//! plane relative to the residual standard error. It is scale free and
//! measures the distance from the minimum in statistical units rather than
//! the change between iterations. Two further criteria cover cases where it
//! is undefined: an exactly zero residual sum of squares, and (when `n = k`)
//! a relative parameter change below [`LeastSquaresOptions::step_tolerance`].
//! Very close to the minimum the decrease of `S` a step achieves can fall
//! below the rounding error of `S` itself; when no damped step then lowers
//! `S` and the offset is already below 10⁻³, plain Gauss–Newton steps are
//! taken while `S` stays within its rounding error and the offset keeps
//! halving, and a final offset below 10⁻⁵ counts as converged.
//!
//! # Derivatives
//! [`LeastSquaresProblem::jacobian`] may supply analytic derivatives; when it
//! returns `None` they are computed by central differences with step
//! `hⱼ = ε^{1/3}·max(|θⱼ|, 10⁻⁸)` (one-sided with step `ε^{1/2}·max(|θⱼ|, 10⁻⁸)`
//! next to a bound or where the residuals are undefined on one side), see
//! [`numeric_jacobian`].

use crate::error::{Result, StatError};
use crate::matrix::Matrix;
use crate::qr::QrDecomposition;

/// A residual vector and (optionally) its Jacobian, the input of
/// [`levenberg_marquardt`].
pub trait LeastSquaresProblem {
    /// Number of residuals `n`.
    fn n_residuals(&self) -> usize;

    /// Write `r(θ)` into `out` (length `n`). Return `false` when the residuals
    /// cannot be computed at `θ` (outside the model's domain); a non-finite
    /// value counts the same way.
    fn residuals(&mut self, theta: &[f64], out: &mut [f64]) -> bool;

    /// Write the analytic Jacobian `∂r/∂θ` (`n × p`) into `out` and return
    /// `Some(true)`, `Some(false)` when it cannot be computed at `θ`, or `None`
    /// (the default) to use central differences.
    fn jacobian(&mut self, _theta: &[f64], _out: &mut Matrix) -> Option<bool> {
        None
    }
}

/// A [`LeastSquaresProblem`] from a residual closure (numeric derivatives).
#[derive(Debug)]
pub struct ResidualFn<F> {
    n: usize,
    f: F,
}

impl<F: FnMut(&[f64], &mut [f64]) -> bool> ResidualFn<F> {
    /// `n` residuals computed by `f(θ, out)`.
    pub fn new(n: usize, f: F) -> Self {
        Self { n, f }
    }
}

impl<F: FnMut(&[f64], &mut [f64]) -> bool> LeastSquaresProblem for ResidualFn<F> {
    fn n_residuals(&self) -> usize {
        self.n
    }
    fn residuals(&mut self, theta: &[f64], out: &mut [f64]) -> bool {
        (self.f)(theta, out)
    }
}

/// Settings of [`levenberg_marquardt`].
#[derive(Debug, Clone, PartialEq)]
pub struct LeastSquaresOptions {
    /// Lower bound of each parameter (empty: none; `-∞` for an unbounded one).
    pub lower: Vec<f64>,
    /// Upper bound of each parameter (empty: none; `+∞` for an unbounded one).
    pub upper: Vec<f64>,
    /// Iteration limit (each iteration recomputes the Jacobian).
    pub max_iterations: usize,
    /// Convergence when the relative offset falls below this (default 10⁻⁸;
    /// Bates & Watts suggest 10⁻³ for inference, smaller values cost only a
    /// few Gauss–Newton steps more).
    pub relative_offset: f64,
    /// Convergence by step size when the relative offset is undefined
    /// (as many parameters as residuals): `‖Dδ‖ ≤ tol·‖Dθ‖` (default 10⁻¹²).
    pub step_tolerance: f64,
}

impl Default for LeastSquaresOptions {
    fn default() -> Self {
        Self { lower: Vec::new(), upper: Vec::new(), max_iterations: 200, relative_offset: 1e-8, step_tolerance: 1e-12 }
    }
}

/// Why [`levenberg_marquardt`] stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Termination {
    /// The relative offset fell below its tolerance.
    RelativeOffset,
    /// The residuals are zero (a perfect fit, to rounding error).
    ZeroResidual,
    /// The parameters stopped changing (used when the relative offset is
    /// undefined because there are as many free parameters as residuals).
    SmallStep,
    /// The iteration limit was reached first.
    IterationLimit,
    /// No damping produced a smaller sum of squares (the iterations stalled
    /// away from a point meeting the relative offset criterion).
    NoProgress,
}

impl Termination {
    /// `true` for the three convergence criteria.
    #[must_use]
    pub fn converged(self) -> bool {
        matches!(self, Self::RelativeOffset | Self::ZeroResidual | Self::SmallStep)
    }

    /// A short description.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::RelativeOffset => "converged (relative offset criterion)",
            Self::ZeroResidual => "converged (residuals zero to rounding error)",
            Self::SmallStep => "converged (parameters stopped changing)",
            Self::IterationLimit => "did not converge (iteration limit reached)",
            Self::NoProgress => "did not converge (no step reduced the sum of squares)",
        }
    }
}

/// Which bound, if any, a parameter ends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundState {
    /// Strictly inside its bounds (or unbounded).
    Free,
    /// On its lower bound (also a parameter fixed by equal lower and upper
    /// bounds).
    Lower,
    /// On its upper bound.
    Upper,
}

/// The result of [`levenberg_marquardt`].
#[derive(Debug, Clone, PartialEq)]
pub struct LeastSquaresResult {
    /// Final parameters.
    pub theta: Vec<f64>,
    /// Residuals `r(θ)` there.
    pub residuals: Vec<f64>,
    /// Sum of squared residuals `S(θ)`.
    pub sse: f64,
    /// Jacobian `∂r/∂θ` there (`n × p`).
    pub jacobian: Matrix,
    /// Iterations (Jacobian evaluations) used, over both runs when the first
    /// did not converge.
    pub iterations: usize,
    /// Residual evaluations used (including those of numeric derivatives).
    pub evaluations: usize,
    /// Relative offset at the final point over the free parameters (`NaN`
    /// where undefined).
    pub relative_offset: f64,
    /// Why the iterations stopped.
    pub termination: Termination,
    /// Bound state of each parameter.
    pub at_bound: Vec<BoundState>,
}

impl LeastSquaresResult {
    /// Whether a convergence criterion was met.
    #[must_use]
    pub fn converged(&self) -> bool {
        self.termination.converged()
    }
}

struct Counted<'a> {
    problem: &'a mut dyn LeastSquaresProblem,
    evaluations: usize,
}

impl Counted<'_> {
    fn residuals(&mut self, theta: &[f64], out: &mut [f64]) -> bool {
        self.evaluations += 1;
        self.problem.residuals(theta, out) && out.iter().all(|v| v.is_finite())
    }
}

fn sum_sq(r: &[f64]) -> f64 {
    crate::summation::dot(r, r)
}

/// Central-difference Jacobian of `problem`'s residuals at `theta` into `out`
/// (`n × p`), respecting the box (`lower`/`upper`, empty for none); `r0` are
/// the residuals at `theta`. Returns the number of residual evaluations, or
/// `None` when some column cannot be computed.
///
/// Step `hⱼ = ε^{1/3}·max(|θⱼ|, 10⁻⁸)`, truncation error `O(h²)`; where
/// `θⱼ ± hⱼ` leaves the box or the residuals are undefined on one side, a
/// one-sided difference with `hⱼ = ε^{1/2}·max(|θⱼ|, 10⁻⁸)` is used.
pub fn numeric_jacobian(
    problem: &mut dyn LeastSquaresProblem,
    theta: &[f64],
    r0: &[f64],
    lower: &[f64],
    upper: &[f64],
    out: &mut Matrix,
) -> Option<usize> {
    let mut c = Counted { problem, evaluations: 0 };
    let ok = numeric_jacobian_counted(&mut c, theta, r0, lower, upper, out);
    ok.then_some(c.evaluations)
}

fn numeric_jacobian_counted(
    c: &mut Counted<'_>,
    theta: &[f64],
    r0: &[f64],
    lower: &[f64],
    upper: &[f64],
    out: &mut Matrix,
) -> bool {
    let n = r0.len();
    let p = theta.len();
    let lo = |j: usize| lower.get(j).copied().unwrap_or(f64::NEG_INFINITY);
    let hi = |j: usize| upper.get(j).copied().unwrap_or(f64::INFINITY);
    let (mut plus, mut minus) = (vec![0.0; n], vec![0.0; n]);
    let mut t = theta.to_vec();
    for j in 0..p {
        // A parameter fixed by equal bounds has no room for a difference
        // step; it never enters a step or the covariance, so its column is 0.
        if lo(j) == hi(j) {
            for i in 0..n {
                out[(i, j)] = 0.0;
            }
            continue;
        }
        let scale = theta[j].abs().max(1e-8);
        let h = f64::EPSILON.cbrt() * scale;
        let up = theta[j] + h;
        let down = theta[j] - h;
        let mut done = false;
        if up <= hi(j) && down >= lo(j) {
            t[j] = up;
            let a = c.residuals(&t, &mut plus);
            t[j] = down;
            let b = c.residuals(&t, &mut minus);
            t[j] = theta[j];
            if a && b {
                let width = up - down;
                for i in 0..n {
                    out[(i, j)] = (plus[i] - minus[i]) / width;
                }
                done = true;
            }
        }
        if !done {
            let h = f64::EPSILON.sqrt() * scale;
            for dir in [1.0, -1.0] {
                let x = theta[j] + dir * h;
                if x < lo(j) || x > hi(j) {
                    continue;
                }
                t[j] = x;
                let ok = c.residuals(&t, &mut plus);
                t[j] = theta[j];
                if ok {
                    let width = x - theta[j];
                    for i in 0..n {
                        out[(i, j)] = (plus[i] - r0[i]) / width;
                    }
                    done = true;
                    break;
                }
            }
        }
        if !done {
            return false;
        }
    }
    true
}

fn jacobian_at(c: &mut Counted<'_>, theta: &[f64], r: &[f64], options: &LeastSquaresOptions, out: &mut Matrix) -> bool {
    match c.problem.jacobian(theta, out) {
        Some(ok) => ok && out.is_finite(),
        None => numeric_jacobian_counted(c, theta, r, &options.lower, &options.upper, out),
    }
}

/// The columns of `j` listed in `free`.
fn columns(j: &Matrix, free: &[usize]) -> Matrix {
    let n = j.nrows();
    let mut data = Vec::with_capacity(n * free.len());
    for &k in free {
        data.extend_from_slice(j.col(k));
    }
    Matrix::from_col_major(n, free.len(), data).unwrap_or_else(|_| Matrix::zeros(n, 0))
}

/// Relative offset of `r` against the column space of `j` (`NaN` when
/// undefined: no residual degrees of freedom, a zero Jacobian or a QR failure).
fn relative_offset(j: &Matrix, r: &[f64]) -> f64 {
    let (n, k) = (j.nrows(), j.ncols());
    if k == 0 || n <= k {
        return f64::NAN;
    }
    let Ok(qr) = QrDecomposition::new(j.clone()) else { return f64::NAN };
    // A zero Jacobian spans nothing, so no offset can signal convergence.
    if qr.rank(qr.default_rank_tolerance()) == 0 {
        return f64::NAN;
    }
    let mut qtr = r.to_vec();
    if qr.apply_qt(&mut qtr).is_err() {
        return f64::NAN;
    }
    let along = sum_sq(&qtr[..k]);
    let across = sum_sq(&qtr[k..]);
    if across == 0.0 {
        return if along == 0.0 { 0.0 } else { f64::INFINITY };
    }
    ((along / k as f64) / (across / (n - k) as f64)).sqrt()
}

/// Minimise `Σ rᵢ(θ)²` from `start` by Levenberg–Marquardt with Gauss–Newton
/// steps (see the module documentation for the method, bounds and
/// convergence criteria).
///
/// A start outside the box is moved onto it. Not converging is not an error:
/// the result's [`termination`](LeastSquaresResult::termination) says why the
/// iterations stopped, and the caller decides what to report.
///
/// # Errors
/// * [`StatError::DimensionMismatch`] if the bounds do not have one entry per
///   parameter (when given).
/// * [`StatError::InvalidArgument`] for an empty or non-finite start, a lower
///   bound above its upper bound, or residuals that cannot be computed at the
///   start.
/// * [`StatError::InsufficientData`] with fewer residuals than parameters.
pub fn levenberg_marquardt(
    problem: &mut dyn LeastSquaresProblem,
    start: &[f64],
    options: &LeastSquaresOptions,
) -> Result<LeastSquaresResult> {
    let p = start.len();
    let n = problem.n_residuals();
    if p == 0 {
        return Err(StatError::InvalidArgument { message: "the model has no parameters".into() });
    }
    for (bound, context) in
        [(&options.lower, "lower bounds vs. parameters"), (&options.upper, "upper bounds vs. parameters")]
    {
        if !bound.is_empty() && bound.len() != p {
            return Err(StatError::DimensionMismatch { context, expected: p, found: bound.len() });
        }
    }
    if n < p {
        return Err(StatError::InsufficientData { required: p, found: n });
    }
    if start.iter().any(|v| !v.is_finite()) {
        return Err(StatError::InvalidArgument { message: "starting values must be finite numbers".into() });
    }
    let lo = |j: usize| options.lower.get(j).copied().unwrap_or(f64::NEG_INFINITY);
    let hi = |j: usize| options.upper.get(j).copied().unwrap_or(f64::INFINITY);
    if (0..p).any(|j| lo(j).is_nan() || hi(j).is_nan() || lo(j) > hi(j)) {
        return Err(StatError::InvalidArgument { message: "each lower bound must be at most its upper bound".into() });
    }
    // First a cautious start (strong initial damping, steps no longer than
    // the scaled parameter vector); if that does not converge, the classical
    // settings (light damping, unbounded steps). The better result is kept.
    let first = attempt(problem, start, options, 1e2, Some(1.0))?;
    if first.converged() {
        return Ok(first);
    }
    let second = attempt(problem, start, options, 1e-3, None)?;
    let mut total = (first.iterations + second.iterations, first.evaluations + second.evaluations);
    let mut best = if second.converged() || (!first.converged() && second.sse < first.sse) { second } else { first };
    // Last, for starts far from the solution in scale (several orders of
    // magnitude, as in NIST's MGH10 from its first start): iterate on
    // φⱼ = ln|θⱼ| with each parameter's sign fixed, where such steps are
    // small, then finish on θ itself. Only without bounds and with no zero
    // in the start.
    if !best.converged()
        && options.lower.iter().chain(&options.upper).all(|b| b.is_infinite())
        && start.iter().all(|v| *v != 0.0)
    {
        let signs: Vec<f64> = start.iter().map(|v| v.signum()).collect();
        let phi0: Vec<f64> = start.iter().map(|v| v.abs().ln()).collect();
        let mut log_problem = LogScaled { inner: &mut *problem, signs: &signs };
        let log_options = LeastSquaresOptions { lower: Vec::new(), upper: Vec::new(), ..options.clone() };
        if let Ok(on_log) = attempt(&mut log_problem, &phi0, &log_options, 1e-3, None) {
            total.0 += on_log.iterations;
            total.1 += on_log.evaluations;
            let theta: Vec<f64> = on_log.theta.iter().zip(&signs).map(|(f, s)| s * f.exp()).collect();
            if on_log.sse < best.sse && theta.iter().all(|v| v.is_finite()) {
                if let Ok(finish) = attempt(problem, &theta, options, 1e-3, None) {
                    total.0 += finish.iterations;
                    total.1 += finish.evaluations;
                    if finish.converged() || finish.sse < best.sse {
                        best = finish;
                    }
                }
            }
        }
    }
    (best.iterations, best.evaluations) = total;
    Ok(best)
}

/// `problem` in terms of `φⱼ = ln|θⱼ|`: `θⱼ = signⱼ·exp(φⱼ)`, so
/// `∂r/∂φⱼ = θⱼ·∂r/∂θⱼ`.
struct LogScaled<'a> {
    inner: &'a mut dyn LeastSquaresProblem,
    signs: &'a [f64],
}

impl LogScaled<'_> {
    fn theta(&self, phi: &[f64]) -> Vec<f64> {
        phi.iter().zip(self.signs).map(|(f, s)| s * f.exp()).collect()
    }
}

impl LeastSquaresProblem for LogScaled<'_> {
    fn n_residuals(&self) -> usize {
        self.inner.n_residuals()
    }
    fn residuals(&mut self, phi: &[f64], out: &mut [f64]) -> bool {
        let theta = self.theta(phi);
        theta.iter().all(|v| v.is_finite() && *v != 0.0) && self.inner.residuals(&theta, out)
    }
    fn jacobian(&mut self, phi: &[f64], out: &mut Matrix) -> Option<bool> {
        let theta = self.theta(phi);
        let ok = self.inner.jacobian(&theta, out)?;
        for (j, t) in theta.iter().enumerate() {
            for i in 0..out.nrows() {
                out[(i, j)] *= t;
            }
        }
        Some(ok)
    }
}

/// One run of the iterations from `start` (already validated), with initial
/// damping `lambda0` and, if given, steps limited to `‖Dδ‖ ≤ bound·‖Dθ‖`.
fn attempt(
    problem: &mut dyn LeastSquaresProblem,
    start: &[f64],
    options: &LeastSquaresOptions,
    lambda0: f64,
    step_bound: Option<f64>,
) -> Result<LeastSquaresResult> {
    let p = start.len();
    let n = problem.n_residuals();
    let lo = |j: usize| options.lower.get(j).copied().unwrap_or(f64::NEG_INFINITY);
    let hi = |j: usize| options.upper.get(j).copied().unwrap_or(f64::INFINITY);
    let clamp = |theta: &mut [f64]| {
        for (j, v) in theta.iter_mut().enumerate() {
            *v = v.clamp(lo(j), hi(j));
        }
    };
    let mut c = Counted { problem, evaluations: 0 };
    let mut theta = start.to_vec();
    clamp(&mut theta);
    let mut r = vec![0.0; n];
    if !c.residuals(&theta, &mut r) {
        return Err(StatError::InvalidArgument {
            message: "the model cannot be computed at the starting values (a residual is missing or infinite)".into(),
        });
    }
    let mut sse = sum_sq(&r);
    let mut jac = Matrix::zeros(n, p);
    if !jacobian_at(&mut c, &theta, &r, options, &mut jac) {
        return Err(StatError::InvalidArgument {
            message: "the model's derivatives cannot be computed at the starting values".into(),
        });
    }
    let mut d = vec![0.0_f64; p];
    let mut lambda = lambda0;
    let mut iterations = 0usize;
    let mut trial = vec![0.0; n];
    let bound_state = |theta: &[f64]| -> Vec<BoundState> {
        (0..p)
            .map(|j| {
                // A parameter fixed by equal bounds counts as on its lower
                // bound: it was never free, so it must not enter the
                // inference as an estimated parameter.
                if theta[j] <= lo(j) {
                    BoundState::Lower
                } else if theta[j] >= hi(j) && lo(j) < hi(j) {
                    BoundState::Upper
                } else {
                    BoundState::Free
                }
            })
            .collect()
    };
    let termination = loop {
        // Scaling: the largest column norm seen so far (Moré 1978).
        for (j, dj) in d.iter_mut().enumerate() {
            let norm = crate::qr::norm2(jac.col(j));
            *dj = dj.max(norm);
        }
        // Free parameters: not fixed (lower = upper), and not on a bound that
        // the gradient pushes against.
        let grad: Vec<f64> = (0..p).map(|j| crate::summation::dot(jac.col(j), &r)).collect();
        let free: Vec<usize> = (0..p)
            .filter(|&j| {
                if lo(j) == hi(j) {
                    return false;
                }
                !(theta[j] <= lo(j) && grad[j] > 0.0 || theta[j] >= hi(j) && grad[j] < 0.0)
            })
            .collect();
        if sse == 0.0 {
            break Termination::ZeroResidual;
        }
        if free.is_empty() {
            break Termination::SmallStep;
        }
        let jf = columns(&jac, &free);
        let offset = relative_offset(&jf, &r);
        if offset < options.relative_offset {
            break Termination::RelativeOffset;
        }
        if iterations >= options.max_iterations {
            break Termination::IterationLimit;
        }
        iterations += 1;
        let k = free.len();
        let mut accepted = false;
        let mut small_step = false;
        for _attempt in 0..60 {
            // Augmented least squares [J_F; √λ D_F] δ = [−r; 0].
            let rows = if lambda > 0.0 { n + k } else { n };
            let mut data = vec![0.0; rows * k];
            for (col, &j) in free.iter().enumerate() {
                let dst = &mut data[col * rows..(col + 1) * rows];
                dst[..n].copy_from_slice(jac.col(j));
                if lambda > 0.0 {
                    dst[n + col] = lambda.sqrt() * if d[j] > 0.0 { d[j] } else { 1.0 };
                }
            }
            let mut rhs: Vec<f64> = r.iter().map(|v| -v).collect();
            rhs.resize(rows, 0.0);
            let step = Matrix::from_col_major(rows, k, data)
                .and_then(QrDecomposition::new)
                .and_then(|qr| qr.solve_least_squares(&rhs))
                .ok()
                .filter(|s| s.iter().all(|v| v.is_finite()));
            let Some(mut step) = step else {
                lambda = if lambda == 0.0 { 1e-6 } else { lambda * 10.0 };
                continue;
            };
            if let Some(factor) = step_bound {
                let weight = |j: usize| if d[j] > 0.0 { d[j] } else { 1.0 };
                let length = free.iter().zip(&step).map(|(&j, s)| (weight(j) * s).powi(2)).sum::<f64>().sqrt();
                let size = free.iter().map(|&j| (weight(j) * theta[j]).powi(2)).sum::<f64>().sqrt();
                let size = if size > 0.0 { size } else { free.iter().map(|&j| weight(j).powi(2)).sum::<f64>().sqrt() };
                if length > factor * size {
                    let shrink = factor * size / length;
                    step.iter_mut().for_each(|s| *s *= shrink);
                }
            }
            let mut candidate = theta.clone();
            for (col, &j) in free.iter().enumerate() {
                candidate[j] += step[col];
            }
            clamp(&mut candidate);
            let scaled = |v: &[f64]| -> f64 {
                free.iter().map(|&j| (v[j] * if d[j] > 0.0 { d[j] } else { 1.0 }).powi(2)).sum::<f64>().sqrt()
            };
            let moved: Vec<f64> = (0..p).map(|j| candidate[j] - theta[j]).collect();
            // A tiny Gauss–Newton step means the iterations have converged; a
            // tiny damped step only means λ is too large.
            if lambda == 0.0 && scaled(&moved) <= options.step_tolerance * (scaled(&theta) + options.step_tolerance) {
                small_step = true;
                break;
            }
            if c.residuals(&candidate, &mut trial) {
                let s_new = sum_sq(&trial);
                if s_new < sse {
                    theta = candidate;
                    std::mem::swap(&mut r, &mut trial);
                    sse = s_new;
                    lambda = if lambda < 1e-9 { 0.0 } else { lambda / 10.0 };
                    accepted = true;
                    break;
                }
            }
            lambda = if lambda == 0.0 { 1e-6 } else { lambda * 10.0 };
            if lambda > 1e30 {
                break;
            }
        }
        if small_step {
            break Termination::SmallStep;
        }
        if !accepted {
            if offset >= 1e-3 {
                break Termination::NoProgress;
            }
            // Close to the minimum the decrease of S a step achieves can be
            // smaller than the rounding error in S itself, so no step looks
            // like progress. Finish with plain Gauss–Newton steps, accepted
            // while S does not grow beyond its rounding error and the offset
            // keeps shrinking.
            let mut offset = offset;
            for _ in 0..10 {
                let jf = columns(&jac, &free);
                let rhs: Vec<f64> = r.iter().map(|v| -v).collect();
                let Some(step) = QrDecomposition::new(jf).and_then(|qr| qr.solve_least_squares(&rhs)).ok() else {
                    break;
                };
                let mut candidate = theta.clone();
                for (col, &j) in free.iter().enumerate() {
                    candidate[j] += step[col];
                }
                clamp(&mut candidate);
                if !c.residuals(&candidate, &mut trial) {
                    break;
                }
                let s_new = sum_sq(&trial);
                if s_new.is_nan() || s_new > sse * (1.0 + 1e-10) {
                    break;
                }
                let mut jac_new = Matrix::zeros(n, p);
                if !jacobian_at(&mut c, &candidate, &trial, options, &mut jac_new) {
                    break;
                }
                let offset_new = relative_offset(&columns(&jac_new, &free), &trial);
                if offset_new.is_nan() || offset_new >= 0.5 * offset {
                    break;
                }
                theta = candidate;
                std::mem::swap(&mut r, &mut trial);
                sse = s_new;
                jac = jac_new;
                offset = offset_new;
                if offset < options.relative_offset {
                    break;
                }
            }
            // Far below Bates & Watts' 10⁻³ the point is a minimum to within
            // rounding error.
            break if offset < 1e-5 { Termination::RelativeOffset } else { Termination::NoProgress };
        }
        if !jacobian_at(&mut c, &theta, &r, options, &mut jac) {
            break Termination::NoProgress;
        }
    };
    let at_bound = bound_state(&theta);
    let free: Vec<usize> = (0..p).filter(|&j| at_bound[j] == BoundState::Free).collect();
    let relative = relative_offset(&columns(&jac, &free), &r);
    Ok(LeastSquaresResult {
        theta,
        residuals: r,
        sse,
        jacobian: jac,
        iterations,
        evaluations: c.evaluations,
        relative_offset: relative,
        termination,
        at_bound,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A zero Jacobian spans nothing, so the offset is undefined rather than
    /// a false convergence signal (Antigravity review F-30).
    #[test]
    fn relative_offset_is_undefined_for_a_zero_jacobian() {
        assert!(relative_offset(&Matrix::zeros(5, 2), &[1.0, -2.0, 0.5, 3.0, 1.0]).is_nan());
        let j = Matrix::from_columns(&[vec![1.0, 2.0, 3.0, 4.0, 5.0]]).unwrap();
        assert!(relative_offset(&j, &[1.0, -2.0, 0.5, 3.0, 1.0]).is_finite());
    }

    /// y = a·exp(b·x) through exact points: the minimum is known exactly.
    struct Exponential {
        x: Vec<f64>,
        y: Vec<f64>,
        analytic: bool,
    }

    impl LeastSquaresProblem for Exponential {
        fn n_residuals(&self) -> usize {
            self.x.len()
        }
        fn residuals(&mut self, t: &[f64], out: &mut [f64]) -> bool {
            for i in 0..self.x.len() {
                out[i] = t[0] * (t[1] * self.x[i]).exp() - self.y[i];
            }
            true
        }
        fn jacobian(&mut self, t: &[f64], out: &mut Matrix) -> Option<bool> {
            if !self.analytic {
                return None;
            }
            for i in 0..self.x.len() {
                let e = (t[1] * self.x[i]).exp();
                out[(i, 0)] = e;
                out[(i, 1)] = t[0] * self.x[i] * e;
            }
            Some(true)
        }
    }

    fn data(noise: bool) -> (Vec<f64>, Vec<f64>) {
        let x: Vec<f64> = (0..10).map(f64::from).collect();
        let wiggle = [0.3, -0.2, 0.1, 0.25, -0.4, 0.05, -0.15, 0.2, -0.1, 0.12];
        let y =
            x.iter().zip(wiggle).map(|(&x, w)| 5.0 * (-0.3 * x).exp() + if noise { w * 0.1 } else { 0.0 }).collect();
        (x, y)
    }

    #[test]
    fn exact_data_recovers_the_parameters() {
        for analytic in [true, false] {
            let (x, y) = data(false);
            let mut prob = Exponential { x, y, analytic };
            let fit = levenberg_marquardt(&mut prob, &[1.0, 0.0], &LeastSquaresOptions::default()).unwrap();
            assert!(fit.converged(), "{:?}", fit.termination);
            assert!((fit.theta[0] - 5.0).abs() < 1e-9 && (fit.theta[1] + 0.3).abs() < 1e-10, "{:?}", fit.theta);
        }
    }

    #[test]
    fn minimum_is_stationary_and_offset_small() {
        let (x, y) = data(true);
        let mut prob = Exponential { x: x.clone(), y: y.clone(), analytic: true };
        let fit = levenberg_marquardt(&mut prob, &[1.0, 0.0], &LeastSquaresOptions::default()).unwrap();
        assert_eq!(fit.termination, Termination::RelativeOffset);
        assert!(fit.relative_offset < 1e-8);
        // Gradient Jᵀr vanishes relative to its scale.
        for j in 0..2 {
            let g = crate::summation::dot(fit.jacobian.col(j), &fit.residuals);
            assert!(g.abs() < 1e-9 * crate::qr::norm2(fit.jacobian.col(j)), "{g}");
        }
        // Numeric and analytic derivatives give the same minimum.
        let mut num = Exponential { x, y, analytic: false };
        let f2 = levenberg_marquardt(&mut num, &[1.0, 0.0], &LeastSquaresOptions::default()).unwrap();
        for j in 0..2 {
            assert!((f2.theta[j] - fit.theta[j]).abs() < 1e-9 * fit.theta[j].abs());
        }
    }

    #[test]
    fn rosenbrock_as_least_squares() {
        // r = (10(x₂ − x₁²), 1 − x₁): the minimum is (1, 1) with zero residual.
        let mut prob = ResidualFn::new(2, |t: &[f64], out: &mut [f64]| {
            out[0] = 10.0 * (t[1] - t[0] * t[0]);
            out[1] = 1.0 - t[0];
            true
        });
        let fit = levenberg_marquardt(&mut prob, &[-1.2, 1.0], &LeastSquaresOptions::default()).unwrap();
        assert!(fit.converged(), "{:?}", fit.termination);
        assert!((fit.theta[0] - 1.0).abs() < 1e-8 && (fit.theta[1] - 1.0).abs() < 1e-8, "{:?}", fit.theta);
    }

    #[test]
    fn bounds_are_respected_and_reported() {
        let (x, y) = data(true);
        let mut prob = Exponential { x, y, analytic: true };
        let options =
            LeastSquaresOptions { lower: vec![0.0, -0.2], upper: vec![10.0, 1.0], ..LeastSquaresOptions::default() };
        let fit = levenberg_marquardt(&mut prob, &[1.0, 0.0], &options).unwrap();
        assert!(fit.converged(), "{:?}", fit.termination);
        assert_eq!(fit.theta[1], -0.2);
        assert_eq!(fit.at_bound, vec![BoundState::Free, BoundState::Lower]);
        // With b fixed at the bound, a is the one-parameter least-squares value.
        let e: Vec<f64> = prob.x.iter().map(|&x| (-0.2 * x).exp()).collect();
        let a = crate::summation::dot(&e, &prob.y) / crate::summation::dot(&e, &e);
        assert!((fit.theta[0] - a).abs() < 1e-7, "{} vs {a}", fit.theta[0]);
    }

    /// Equal bounds fix a parameter: it is reported as on its (lower) bound,
    /// not as free, so callers do not treat it as estimated.
    #[test]
    fn fixed_parameter_is_not_reported_free() {
        let (x, y) = data(true);
        let mut prob = Exponential { x, y, analytic: true };
        let options =
            LeastSquaresOptions { lower: vec![0.0, -0.3], upper: vec![10.0, -0.3], ..LeastSquaresOptions::default() };
        let fit = levenberg_marquardt(&mut prob, &[1.0, -0.3], &options).unwrap();
        assert!(fit.converged(), "{:?}", fit.termination);
        assert_eq!(fit.theta[1], -0.3);
        assert_eq!(fit.at_bound, vec![BoundState::Free, BoundState::Lower]);
    }

    #[test]
    fn failures_are_reported() {
        let (x, y) = data(true);
        let mut prob = Exponential { x, y, analytic: true };
        let short = LeastSquaresOptions { max_iterations: 1, ..LeastSquaresOptions::default() };
        let fit = levenberg_marquardt(&mut prob, &[1.0, 0.0], &short).unwrap();
        assert_eq!(fit.termination, Termination::IterationLimit);
        assert!(!fit.converged());
        assert!(levenberg_marquardt(&mut prob, &[f64::NAN, 0.0], &LeastSquaresOptions::default()).is_err());
        let bad =
            LeastSquaresOptions { lower: vec![1.0, 0.0], upper: vec![0.0, 1.0], ..LeastSquaresOptions::default() };
        assert!(levenberg_marquardt(&mut prob, &[1.0, 0.0], &bad).is_err());
        let mut undefined = ResidualFn::new(3, |t: &[f64], out: &mut [f64]| {
            out.fill(t[0].ln());
            t[0] > 0.0
        });
        assert!(levenberg_marquardt(&mut undefined, &[-1.0], &LeastSquaresOptions::default()).is_err());
    }

    #[test]
    fn numeric_jacobian_matches_analytic() {
        let (x, y) = data(true);
        let mut prob = Exponential { x, y, analytic: true };
        let t = [4.0, -0.25];
        let mut r = vec![0.0; 10];
        prob.residuals(&t, &mut r);
        let mut exact = Matrix::zeros(10, 2);
        prob.jacobian(&t, &mut exact);
        let mut approx = Matrix::zeros(10, 2);
        let evals = numeric_jacobian(&mut prob, &t, &r, &[], &[], &mut approx).unwrap();
        assert_eq!(evals, 4);
        for i in 0..10 {
            for j in 0..2 {
                assert!((exact[(i, j)] - approx[(i, j)]).abs() < 1e-8 * (1.0 + exact[(i, j)].abs()));
            }
        }
        // At a bound the one-sided difference is used.
        let mut side = Matrix::zeros(10, 2);
        numeric_jacobian(&mut prob, &t, &r, &[0.0, -0.25], &[10.0, 0.0], &mut side).unwrap();
        for i in 0..10 {
            assert!((exact[(i, 1)] - side[(i, 1)]).abs() < 1e-5 * (1.0 + exact[(i, 1)].abs()));
        }
    }
}
