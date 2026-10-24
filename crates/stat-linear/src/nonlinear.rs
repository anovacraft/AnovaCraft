//! Nonlinear least-squares regression with inference.
//!
//! # Model
//! `yᵢ = f(xᵢ; θ) + εᵢ`, `εᵢ` independent `N(0, σ²)`, for a model function
//! `f` with `P` parameters `θ`. The estimates minimise
//! `S(θ) = Σ (yᵢ − f(xᵢ; θ))²` ([`stat_core::optimize::levenberg_marquardt`]).
//! The model is given as a [`ModelFunction`]: either one of the library
//! forms in [`forms`] (analytic derivatives and automatic starting values),
//! or any function of the parameters (central-difference derivatives).
//!
//! # Inference (Bates & Watts, *Nonlinear Regression Analysis and Its
//! Applications*, Wiley 1988, ch. 2 and 6)
//! With `V = ∂f/∂θ` (`n × P`) at `θ̂`, `s² = S(θ̂)/(n − P)` and
//! `(VᵀV)⁻¹` from the QR decomposition of `V`. Here `P` counts the
//! *estimated* parameters: one fixed by equal lower and upper bounds is a
//! known constant of the model, not estimated, so it uses no degree of
//! freedom (the fit equals the fit of the model with that constant written
//! in):
//!
//! * standard errors `se(θ̂ⱼ) = s·√[(VᵀV)⁻¹]ⱼⱼ`, `t = θ̂ⱼ / se`, two-sided
//!   p-values on `n − P` df, and linearisation (Wald) intervals
//!   `θ̂ⱼ ± t(n − P; α/2)·se`;
//! * the correlation of the estimates from `(VᵀV)⁻¹`;
//! * profile t intervals (§6.1): with `S̃(θⱼ)` the least sum of squares when
//!   `θⱼ` is held fixed and the other parameters re-estimated,
//!   `τ(θⱼ) = sign(θⱼ − θ̂ⱼ)·√(S̃(θⱼ) − S(θ̂))/s`, and the interval is
//!   `{θⱼ : |τ(θⱼ)| ≤ t(n − P; α/2)}`. This is the likelihood-ratio (F-test)
//!   interval `S̃(θⱼ) − S(θ̂) ≤ s²·F(1, n − P; α)`; unlike the Wald interval it
//!   follows the curvature of the model and need not be symmetric;
//! * the fitted mean at a point with gradient `g = ∂f/∂θ` has delta-method
//!   standard error `s·√(gᵀ(VᵀV)⁻¹g)`; confidence limits for the mean are
//!   `f ± t·se`, prediction limits for a new observation
//!   `f ± t·√(se² + s²)` ([`NonlinearFit::mean_interval`],
//!   [`NonlinearFit::prediction_interval`]).
//!
//! # Weights
//! With weights `wᵢ > 0` ([`NonlinearOptions::weights`]) the estimates
//! minimise `S(θ) = Σ wᵢ (yᵢ − f(xᵢ; θ))²`: the problem is solved for the
//! scaled residuals `√wᵢ (f − yᵢ)` and every statistic above uses the scaled
//! Jacobian `W^{1/2}V` (Bates & Watts §1.3.2 / Seber & Wild, *Nonlinear
//! Regression*, Wiley 1989, §2.2.2), so `s²` estimates the variance of an
//! observation of weight 1, `Var(yᵢ) = σ²/wᵢ`. Prediction limits for a new
//! observation of weight `w₀` are `f ± t·√(se² + s²/w₀)`
//! ([`NonlinearFit::prediction_interval_weighted`]).
//!
//! # Curvature diagnostic (Hougaard 1985)
//! The asymptotic skewness of each estimate (Hougaard, "The appropriateness of
//! the asymptotic distribution in a nonlinear regression model in relation to
//! curvature", *J. R. Statist. Soc. B* 47, 1985; in the form of Ratkowsky,
//! *Handbook of Nonlinear Regression Models*, Dekker 1990, §2.5): with
//! `L = (VᵀV)⁻¹`, `Hₖₗ = ∂²f/∂θₖ∂θₗ` and `Wⱼₖₗ = Σᵤ Vᵤⱼ Hᵤₖₗ`,
//! `E(θ̂ᵢ − θᵢ)³ ≈ −s⁴ Σⱼₖₗ Lᵢⱼ Lᵢₖ Lᵢₗ (Wⱼₖₗ + Wₖⱼₗ + Wₗⱼₖ)` and the
//! standardised skewness is `g₁ᵢ = E(θ̂ᵢ − θᵢ)³ / (s² Lᵢᵢ)^{3/2}`. Ratkowsky's
//! guide: `|g₁| < 0.1` the estimator is very close to linear, `0.1–0.25`
//! reasonably close, above `0.25` the nonlinearity is apparent, and above 1
//! it is considerable. Second derivatives are central differences of the
//! model values (step `ε^{1/4}·max(|θ|, 10⁻⁸)`), whatever the first
//! derivatives.
//!
//! # Failure modes
//! * **No convergence** — the iteration limit is reached or no step lowers
//!   `S`: the fit is still returned, with [`NonlinearFit::termination`] saying
//!   why and a warning; its estimates and statistics are those of the last
//!   iterate and should not be relied on. Try other starting values.
//! * **Singular Jacobian** at the solution (parameters not separately
//!   identifiable, e.g. a redundant parameter or a rate whose curve is flat
//!   over the data): [`StatError::RankDeficient`].
//! * **Parameter at a bound** — it is reported as such, held fixed for the
//!   inference (no standard error; the residual df still count it, since it
//!   was estimated and the data put it there), and a warning says so.
//! * **Parameter fixed by equal bounds** — not estimated: no standard error,
//!   not counted in `P`, and a warning says so.
//! * Fewer observations than estimated parameters + 1, or non-finite responses:
//!   [`StatError::InsufficientData`] / [`StatError::NonFiniteInput`].

pub mod assay;
pub mod compare;
pub mod forms;
pub mod global;
pub mod implicit;
pub mod inverse;
pub mod robust;

use stat_core::distributions::{ContinuousDistribution, StudentsT};
use stat_core::optimize::{
    BoundState, LeastSquaresOptions, LeastSquaresProblem, Termination, levenberg_marquardt, numeric_jacobian,
};
use stat_core::{Matrix, QrDecomposition, Result, StatError};

/// A model function `f(xᵢ; θ)` evaluated at every observation.
pub trait ModelFunction {
    /// Number of observations.
    fn n_observations(&self) -> usize;

    /// Write `f(xᵢ; θ)` for every observation into `out`; `false` when the
    /// model cannot be computed at `θ`.
    fn values(&mut self, theta: &[f64], out: &mut [f64]) -> bool;

    /// Write `∂f(xᵢ; θ)/∂θⱼ` into `out` (`n × P`) and return `Some(true)`;
    /// `Some(false)` where undefined; `None` (default) for central differences.
    fn derivatives(&mut self, _theta: &[f64], _out: &mut Matrix) -> Option<bool> {
        None
    }
}

/// Settings of [`fit_nonlinear`].
#[derive(Debug, Clone, PartialEq)]
pub struct NonlinearOptions {
    /// Parameter names (one per parameter).
    pub names: Vec<String>,
    /// Lower bounds (empty for none; `-∞` for an unbounded parameter).
    pub lower: Vec<f64>,
    /// Upper bounds (empty for none; `+∞` for an unbounded parameter).
    pub upper: Vec<f64>,
    /// Iteration limit.
    pub max_iterations: usize,
    /// Relative offset convergence tolerance (see
    /// [`LeastSquaresOptions::relative_offset`]).
    pub relative_offset: f64,
    /// Confidence level of the intervals and bands.
    pub confidence_level: f64,
    /// Compute profile t intervals (one constrained refit per search point).
    pub profile_intervals: bool,
    /// Weight of each observation (empty for equal weights); all must be
    /// finite and positive.
    pub weights: Vec<f64>,
    /// Compute the Hougaard skewness of each estimate.
    pub skewness: bool,
}

impl NonlinearOptions {
    /// Defaults for parameters `names`: no bounds, 200 iterations, relative
    /// offset 10⁻⁸, 95% intervals, with profile intervals.
    #[must_use]
    pub fn new(names: Vec<String>) -> Self {
        Self {
            names,
            lower: Vec::new(),
            upper: Vec::new(),
            max_iterations: 200,
            relative_offset: 1e-8,
            confidence_level: 0.95,
            profile_intervals: true,
            weights: Vec::new(),
            skewness: true,
        }
    }
}

/// A profile t interval; a side is `None` when `|τ|` did not reach the
/// critical value inside the parameter's bounds and the search range (the
/// interval is open on that side).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProfileInterval {
    /// Lower limit.
    pub lower: Option<f64>,
    /// Upper limit.
    pub upper: Option<f64>,
}

/// One estimated parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct NonlinearParameter {
    /// Name.
    pub name: String,
    /// Estimate `θ̂ⱼ`.
    pub estimate: f64,
    /// Asymptotic standard error (`NaN` for a parameter on a bound).
    pub std_error: f64,
    /// `θ̂ⱼ / se`.
    pub t_ratio: f64,
    /// Two-sided p-value of `θⱼ = 0` on the residual df.
    pub p_value: f64,
    /// Linearisation (Wald) interval `θ̂ⱼ ± t·se`.
    pub wald_interval: (f64, f64),
    /// Profile t interval, when requested and the parameter is free.
    pub profile_interval: Option<ProfileInterval>,
    /// Whether the estimate is on a bound.
    pub bound: BoundState,
    /// Whether the parameter was fixed by equal lower and upper bounds (a
    /// known constant: not estimated, not counted in the residual df).
    pub fixed: bool,
    /// Hougaard's standardised skewness `g₁` of the estimate (`NaN` when not
    /// computed or the parameter is not free).
    pub skewness: f64,
}

/// A fitted nonlinear regression.
#[derive(Debug, Clone)]
pub struct NonlinearFit {
    /// Parameters, in model order.
    pub parameters: Vec<NonlinearParameter>,
    /// Correlation of the estimates (`P × P`; `NaN` rows and columns for
    /// parameters on a bound).
    pub correlation: Matrix,
    /// Covariance of the estimates `s²(VᵀV)⁻¹` (`NaN` for bound parameters).
    pub covariance: Matrix,
    /// Fitted values `f(xᵢ; θ̂)`.
    pub fitted: Vec<f64>,
    /// Residuals `yᵢ − f(xᵢ; θ̂)` (unweighted).
    pub residuals: Vec<f64>,
    /// Residual sum of squares `S(θ̂)` (weighted, `Σ wᵢ rᵢ²`, when weights
    /// are given).
    pub sse: f64,
    /// Whether the fit was weighted.
    pub weighted: bool,
    /// Number of observations.
    pub n: usize,
    /// Residual degrees of freedom `n − P`, `P` the number of estimated
    /// parameters (those not fixed by equal bounds).
    pub df: usize,
    /// Number of estimated parameters `P` (all but those fixed by equal bounds).
    pub n_estimated: usize,
    /// Residual standard error `s`.
    pub residual_se: f64,
    /// Iterations used.
    pub iterations: usize,
    /// Model evaluations used (including numeric derivatives, excluding
    /// the profile searches).
    pub evaluations: usize,
    /// Relative offset at the solution.
    pub relative_offset: f64,
    /// Why the iterations stopped.
    pub termination: Termination,
    /// Plain-language warnings (no convergence, bounds, open profile limits).
    pub warnings: Vec<String>,
    /// Confidence level used.
    pub confidence_level: f64,
    /// `t(n − P; α/2)`.
    pub t_critical: f64,
    /// Whether derivatives were analytic.
    pub analytic_derivatives: bool,
    /// Indices of the free parameters and the QR of their columns of `V`.
    free: Vec<usize>,
    qr: QrDecomposition,
}

impl NonlinearFit {
    /// Estimates in model order.
    #[must_use]
    pub fn estimates(&self) -> Vec<f64> {
        self.parameters.iter().map(|p| p.estimate).collect()
    }

    /// Whether a convergence criterion was met.
    #[must_use]
    pub fn converged(&self) -> bool {
        self.termination.converged()
    }

    /// Delta-method standard error of the fitted mean at a point whose model
    /// gradient `∂f/∂θ` is `gradient` (one entry per parameter).
    ///
    /// # Errors
    /// [`StatError::DimensionMismatch`] if `gradient` has the wrong length.
    pub fn mean_std_error(&self, gradient: &[f64]) -> Result<f64> {
        if gradient.len() != self.parameters.len() {
            return Err(StatError::DimensionMismatch {
                context: "gradient vs. parameters",
                expected: self.parameters.len(),
                found: gradient.len(),
            });
        }
        let g: Vec<f64> = self.free.iter().map(|&j| gradient[j]).collect();
        if g.is_empty() {
            return Ok(0.0);
        }
        let w = self.qr.solve_r_transpose(&g)?;
        Ok(self.residual_se * stat_core::summation::dot(&w, &w).sqrt())
    }

    /// Confidence limits for the mean `value ± t·se` at a point with model
    /// value `value` and gradient `gradient`.
    ///
    /// # Errors
    /// As [`mean_std_error`](Self::mean_std_error).
    pub fn mean_interval(&self, value: f64, gradient: &[f64]) -> Result<(f64, f64)> {
        let h = self.t_critical * self.mean_std_error(gradient)?;
        Ok((value - h, value + h))
    }

    /// Prediction limits for one new observation of weight 1,
    /// `value ± t·√(se² + s²)`.
    ///
    /// # Errors
    /// As [`mean_std_error`](Self::mean_std_error).
    pub fn prediction_interval(&self, value: f64, gradient: &[f64]) -> Result<(f64, f64)> {
        self.prediction_interval_weighted(value, gradient, 1.0)
    }

    /// Prediction limits for one new observation of weight `weight`,
    /// `value ± t·√(se² + s²/weight)`.
    ///
    /// # Errors
    /// As [`mean_std_error`](Self::mean_std_error); [`StatError::InvalidArgument`]
    /// for a weight that is not positive.
    pub fn prediction_interval_weighted(&self, value: f64, gradient: &[f64], weight: f64) -> Result<(f64, f64)> {
        if !(weight > 0.0 && weight.is_finite()) {
            return Err(StatError::InvalidArgument { message: "a prediction weight must be positive".into() });
        }
        let se = self.mean_std_error(gradient)?;
        let h = self.t_critical * (se * se + self.residual_se * self.residual_se / weight).sqrt();
        Ok((value - h, value + h))
    }
}

/// `r(θ) = f(θ) − y` for the solver.
struct Residuals<'a> {
    model: &'a mut dyn ModelFunction,
    y: &'a [f64],
    /// Fixed parameter values inserted between the free ones (profiling).
    fixed: Option<(usize, f64)>,
    full: Vec<f64>,
    jac: Matrix,
    /// `√wᵢ` (empty for equal weights).
    root_w: &'a [f64],
}

impl Residuals<'_> {
    fn expand(&mut self, theta: &[f64]) {
        match self.fixed {
            None => self.full.copy_from_slice(theta),
            Some((k, v)) => {
                let mut it = theta.iter();
                for j in 0..self.full.len() {
                    self.full[j] = if j == k { v } else { *it.next().unwrap_or(&f64::NAN) };
                }
            }
        }
    }
}

impl LeastSquaresProblem for Residuals<'_> {
    fn n_residuals(&self) -> usize {
        self.y.len()
    }
    fn residuals(&mut self, theta: &[f64], out: &mut [f64]) -> bool {
        self.expand(theta);
        if !self.model.values(&self.full, out) {
            return false;
        }
        for (o, y) in out.iter_mut().zip(self.y) {
            *o -= y;
        }
        for (o, w) in out.iter_mut().zip(self.root_w) {
            *o *= w;
        }
        true
    }
    fn jacobian(&mut self, theta: &[f64], out: &mut Matrix) -> Option<bool> {
        self.expand(theta);
        let ok = self.model.derivatives(&self.full, &mut self.jac)?;
        if !ok {
            return Some(false);
        }
        let mut col = 0;
        for j in 0..self.full.len() {
            if self.fixed.is_some_and(|(k, _)| k == j) {
                continue;
            }
            out.col_mut(col).copy_from_slice(self.jac.col(j));
            for (o, w) in out.col_mut(col).iter_mut().zip(self.root_w) {
                *o *= w;
            }
            col += 1;
        }
        Some(true)
    }
}

/// Fit `y = f(x; θ) + ε` by least squares from `start` and compute the
/// inference described in the module documentation.
///
/// # Errors
/// * [`StatError::DimensionMismatch`] if `y`, `start` or the names disagree
///   with the model's sizes.
/// * [`StatError::NonFiniteInput`] for a non-finite response.
/// * [`StatError::InsufficientData`] with fewer than `P + 1` observations
///   (`P` the parameters not fixed by equal bounds).
/// * [`StatError::RankDeficient`] if the Jacobian is singular at the solution.
/// * Errors of [`levenberg_marquardt`] (bad bounds or starting values, a
///   model that cannot be computed at the start).
pub fn fit_nonlinear(
    model: &mut dyn ModelFunction,
    y: &[f64],
    start: &[f64],
    options: &NonlinearOptions,
) -> Result<NonlinearFit> {
    let n = model.n_observations();
    let p = start.len();
    if y.len() != n {
        return Err(StatError::DimensionMismatch { context: "response vs. observations", expected: n, found: y.len() });
    }
    if options.names.len() != p {
        return Err(StatError::DimensionMismatch {
            context: "parameter names vs. starting values",
            expected: p,
            found: options.names.len(),
        });
    }
    if y.iter().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "response" });
    }
    if !options.weights.is_empty() {
        if options.weights.len() != n {
            return Err(StatError::DimensionMismatch {
                context: "weights vs. observations",
                expected: n,
                found: options.weights.len(),
            });
        }
        if options.weights.iter().any(|w| !(w.is_finite() && *w > 0.0)) {
            return Err(StatError::InvalidArgument { message: "every weight must be a positive number".into() });
        }
    }
    let root_w: Vec<f64> = options.weights.iter().map(|w| w.sqrt()).collect();
    // A parameter whose lower and upper bounds are equal is a known constant.
    let fixed: Vec<bool> = (0..p)
        .map(|j| match (options.lower.get(j), options.upper.get(j)) {
            (Some(lo), Some(hi)) => lo.is_finite() && lo == hi,
            _ => false,
        })
        .collect();
    let n_estimated = fixed.iter().filter(|f| !**f).count();
    if n <= n_estimated {
        return Err(StatError::InsufficientData { required: n_estimated + 1, found: n });
    }
    if !(options.confidence_level > 0.0 && options.confidence_level < 1.0) {
        return Err(StatError::InvalidArgument { message: "the confidence level must be between 0 and 1".into() });
    }
    let solver = LeastSquaresOptions {
        lower: options.lower.clone(),
        upper: options.upper.clone(),
        max_iterations: options.max_iterations,
        relative_offset: options.relative_offset,
        ..LeastSquaresOptions::default()
    };
    let mut probe = Matrix::zeros(n, p);
    let analytic = model.derivatives(start, &mut probe).is_some();
    let mut problem =
        Residuals { model, y, fixed: None, full: vec![0.0; p], jac: Matrix::zeros(n, p), root_w: &root_w };
    let ls = levenberg_marquardt(&mut problem, start, &solver)?;
    let theta = ls.theta.clone();
    let free: Vec<usize> = (0..p).filter(|&j| ls.at_bound[j] == BoundState::Free).collect();
    let df = n - n_estimated;
    let sse = ls.sse;
    let s = (sse / df as f64).sqrt();
    // V over the free parameters (the solver's Jacobian of r = f − y is V).
    let mut data = Vec::with_capacity(n * free.len());
    for &j in &free {
        data.extend_from_slice(ls.jacobian.col(j));
    }
    let v = Matrix::from_col_major(n, free.len(), data)?;
    let qr = QrDecomposition::new(v)?;
    // Rank test on the column-scaled matrix, so units do not matter.
    let scaled = {
        let mut m = Matrix::zeros(n, free.len());
        for (c, &j) in free.iter().enumerate() {
            let norm = stat_core::qr::norm2(ls.jacobian.col(j));
            for i in 0..n {
                m[(i, c)] = if norm > 0.0 { ls.jacobian[(i, j)] / norm } else { 0.0 };
            }
        }
        QrDecomposition::new(m)?
    };
    let rank = scaled.rank(1e-10_f64.max(scaled.default_rank_tolerance()));
    if rank < free.len() {
        return Err(StatError::RankDeficient { rank, columns: free.len() });
    }
    let inv = qr.inverse_gram()?;
    let mut covariance = Matrix::zeros(p, p);
    let mut correlation = Matrix::zeros(p, p);
    for i in 0..p {
        for j in 0..p {
            covariance[(i, j)] = f64::NAN;
            correlation[(i, j)] = f64::NAN;
        }
    }
    for (a, &i) in free.iter().enumerate() {
        for (b, &j) in free.iter().enumerate() {
            covariance[(i, j)] = s * s * inv[(a, b)];
            correlation[(i, j)] = inv[(a, b)] / (inv[(a, a)] * inv[(b, b)]).sqrt();
        }
    }
    let t_dist = StudentsT::new(df as f64)?;
    let alpha = 1.0 - options.confidence_level;
    let t_critical = t_dist.quantile(1.0 - alpha / 2.0)?;
    // Unweighted residuals: the solver's are √wᵢ (f − yᵢ).
    let residuals: Vec<f64> =
        ls.residuals.iter().enumerate().map(|(i, r)| -r / root_w.get(i).copied().unwrap_or(1.0)).collect();
    let fitted: Vec<f64> = residuals.iter().zip(y).map(|(r, y)| y - r).collect();
    let mut warnings = Vec::new();
    if !ls.converged() {
        warnings.push(format!(
            "The fit {} after {} iterations; the estimates below are the last iterate. Try other starting values.",
            ls.termination.label(),
            ls.iterations
        ));
    }
    let mut parameters = Vec::with_capacity(p);
    for j in 0..p {
        let se = covariance[(j, j)].sqrt();
        let t = theta[j] / se;
        let p_value = if se.is_finite() && se > 0.0 { t_dist.two_sided_p_value(t)? } else { f64::NAN };
        if fixed[j] {
            warnings.push(format!(
                "{} is fixed at {} (equal lower and upper bounds); it is not estimated and does not count \
                 in the residual degrees of freedom.",
                options.names[j], theta[j]
            ));
        } else if ls.at_bound[j] != BoundState::Free {
            warnings.push(format!(
                "{} is on its {} bound ({}); it is held there for the standard errors and intervals.",
                options.names[j],
                if ls.at_bound[j] == BoundState::Lower { "lower" } else { "upper" },
                theta[j]
            ));
        }
        parameters.push(NonlinearParameter {
            name: options.names[j].clone(),
            estimate: theta[j],
            std_error: se,
            t_ratio: t,
            p_value,
            wald_interval: (theta[j] - t_critical * se, theta[j] + t_critical * se),
            profile_interval: None,
            bound: ls.at_bound[j],
            fixed: fixed[j],
            skewness: f64::NAN,
        });
    }
    if options.skewness && ls.converged() && !free.is_empty() {
        if let Some(g) = hougaard_skewness(&mut problem, &theta, &free, &ls.jacobian, &inv, s) {
            for (a, &j) in free.iter().enumerate() {
                parameters[j].skewness = g[a];
            }
        }
    }
    if options.profile_intervals && ls.converged() {
        for &j in &free {
            let interval =
                profile_interval(&mut problem, &theta, sse, s, j, parameters[j].std_error, t_critical, &solver);
            for (side, limit) in [("lower", interval.lower), ("upper", interval.upper)] {
                if limit.is_none() {
                    warnings.push(format!(
                        "The profile interval of {} is open on the {side} side: the sum of squares does not rise far enough \
                         within the search range or the parameter's bounds.",
                        options.names[j]
                    ));
                }
            }
            parameters[j].profile_interval = Some(interval);
        }
    }
    Ok(NonlinearFit {
        parameters,
        correlation,
        covariance,
        fitted,
        residuals,
        sse,
        weighted: !root_w.is_empty(),
        n,
        df,
        n_estimated,
        residual_se: s,
        iterations: ls.iterations,
        evaluations: ls.evaluations,
        relative_offset: ls.relative_offset,
        termination: ls.termination,
        warnings,
        confidence_level: options.confidence_level,
        t_critical,
        analytic_derivatives: analytic,
        free,
        qr,
    })
}

/// Hougaard's standardised skewness of each free estimate (module
/// documentation). `v` is the (weighted) Jacobian over all parameters, `inv`
/// `(VᵀV)⁻¹` over the free ones. `None` if a model value cannot be computed.
fn hougaard_skewness(
    problem: &mut Residuals<'_>,
    theta: &[f64],
    free: &[usize],
    v: &Matrix,
    inv: &Matrix,
    s: f64,
) -> Option<Vec<f64>> {
    let n = problem.y.len();
    let q = free.len();
    let h: Vec<f64> = free.iter().map(|&j| f64::EPSILON.powf(0.25) * theta[j].abs().max(1e-8)).collect();
    let mut eval = |shift: &[(usize, f64)]| -> Option<Vec<f64>> {
        let mut t = theta.to_vec();
        for &(a, d) in shift {
            t[free[a]] += d;
        }
        let mut r = vec![0.0; n];
        problem.residuals(&t, &mut r).then_some(r)
    };
    let r0 = eval(&[])?;
    // Second derivatives of the (weighted) model values: r = √w (f − y), so
    // differences of r are differences of √w·f.
    let mut second = vec![vec![Vec::new(); q]; q];
    for k in 0..q {
        for l in k..q {
            let d2: Vec<f64> = if k == l {
                let (rp, rm) = (eval(&[(k, h[k])])?, eval(&[(k, -h[k])])?);
                (0..n).map(|u| (rp[u] - 2.0 * r0[u] + rm[u]) / (h[k] * h[k])).collect()
            } else {
                let pp = eval(&[(k, h[k]), (l, h[l])])?;
                let pm = eval(&[(k, h[k]), (l, -h[l])])?;
                let mp = eval(&[(k, -h[k]), (l, h[l])])?;
                let mm = eval(&[(k, -h[k]), (l, -h[l])])?;
                (0..n).map(|u| (pp[u] - pm[u] - mp[u] + mm[u]) / (4.0 * h[k] * h[l])).collect()
            };
            second[l][k].clone_from(&d2);
            second[k][l] = d2;
        }
    }
    // W[j][k][l] = Σᵤ V_uj H_ukl.
    let mut w = vec![0.0; q * q * q];
    for j in 0..q {
        let vj = v.col(free[j]);
        for k in 0..q {
            for l in 0..q {
                w[(j * q + k) * q + l] = stat_core::summation::dot(vj, &second[k][l]);
            }
        }
    }
    let s2 = s * s;
    let out = (0..q)
        .map(|i| {
            let mut sum = 0.0;
            for j in 0..q {
                for k in 0..q {
                    for l in 0..q {
                        let c = inv[(i, j)] * inv[(i, k)] * inv[(i, l)];
                        sum += c * (w[(j * q + k) * q + l] + w[(k * q + j) * q + l] + w[(l * q + j) * q + k]);
                    }
                }
            }
            let third = -s2 * s2 * sum;
            third / (s2 * inv[(i, i)]).powf(1.5)
        })
        .collect();
    Some(out)
}

/// Least sum of squares with parameter `j` fixed at `value` (others
/// re-estimated from `warm`, which is updated). `None` if the refit fails.
fn constrained_sse(
    problem: &mut Residuals<'_>,
    warm: &mut [f64],
    j: usize,
    value: f64,
    solver: &LeastSquaresOptions,
) -> Option<f64> {
    let p = problem.full.len();
    problem.fixed = Some((j, value));
    let without = |v: &[f64]| -> Vec<f64> { (0..p).filter(|&k| k != j).map(|k| v[k]).collect() };
    let result = if p == 1 {
        let mut r = vec![0.0; problem.y.len()];
        problem.residuals(&[], &mut r).then(|| stat_core::summation::dot(&r, &r))
    } else {
        let options = LeastSquaresOptions {
            lower: if solver.lower.is_empty() { Vec::new() } else { without(&solver.lower) },
            upper: if solver.upper.is_empty() { Vec::new() } else { without(&solver.upper) },
            max_iterations: solver.max_iterations,
            ..solver.clone()
        };
        let start = without(warm);
        match levenberg_marquardt(problem, &start, &options) {
            Ok(fit) if fit.converged() => {
                let mut it = fit.theta.iter();
                for k in 0..p {
                    if k != j {
                        warm[k] = *it.next().unwrap_or(&warm[k]);
                    }
                }
                Some(fit.sse)
            }
            _ => None,
        }
    };
    problem.fixed = None;
    result
}

/// Profile t interval of parameter `j` (Bates & Watts §6.1): search outward
/// from `θ̂ⱼ` in steps growing by half until `|τ|` exceeds `t_critical`, then
/// refine the crossing by the Illinois variant of regula falsi.
#[allow(clippy::too_many_arguments)]
fn profile_interval(
    problem: &mut Residuals<'_>,
    theta: &[f64],
    sse: f64,
    s: f64,
    j: usize,
    se: f64,
    t_critical: f64,
    solver: &LeastSquaresOptions,
) -> ProfileInterval {
    let lo = solver.lower.get(j).copied().unwrap_or(f64::NEG_INFINITY);
    let hi = solver.upper.get(j).copied().unwrap_or(f64::INFINITY);
    let mut limits = [None, None];
    for (side, limit) in [-1.0_f64, 1.0].into_iter().zip(limits.iter_mut()) {
        let mut warm = theta.to_vec();
        // g(c) = |τ| − t at θ̂ + side·c·se.
        let mut tau_at = |c: f64, warm: &mut [f64]| -> Option<f64> {
            let value = theta[j] + side * c * se;
            let st = constrained_sse(problem, warm, j, value, solver)?;
            Some((st - sse).max(0.0).sqrt() / s - t_critical)
        };
        let (mut a, mut ga) = (0.0_f64, -t_critical);
        let mut c = 1.0_f64;
        let mut bracket = None;
        while c <= 200.0 {
            let value = theta[j] + side * c * se;
            let at_bound = value <= lo || value >= hi;
            let c_eval = if at_bound { ((if side < 0.0 { lo } else { hi }) - theta[j]).abs() / se } else { c };
            if c_eval.is_nan() || c_eval <= a {
                break;
            }
            let Some(g) = tau_at(c_eval, &mut warm) else { break };
            if g >= 0.0 {
                bracket = Some((a, ga, c_eval, g));
                break;
            }
            (a, ga) = (c_eval, g);
            if at_bound {
                break;
            }
            c *= 1.5;
        }
        let Some((mut a, mut ga, mut b, mut gb)) = bracket else { continue };
        let mut side_kept = 0i8;
        let mut best = b;
        for _ in 0..100 {
            let m = (a * gb - b * ga) / (gb - ga);
            let m = if m.is_finite() && m > a.min(b) && m < a.max(b) { m } else { 0.5 * (a + b) };
            let Some(gm) = tau_at(m, &mut warm) else { break };
            best = m;
            if gm.abs() < 1e-10 || (b - a).abs() < 1e-12 * (1.0 + m) {
                break;
            }
            if gm < 0.0 {
                (a, ga) = (m, gm);
                if side_kept == -1 {
                    gb /= 2.0;
                }
                side_kept = -1;
            } else {
                (b, gb) = (m, gm);
                if side_kept == 1 {
                    ga /= 2.0;
                }
                side_kept = 1;
            }
        }
        *limit = Some(theta[j] + side * best * se);
    }
    ProfileInterval { lower: limits[0], upper: limits[1] }
}

/// Central-difference gradient `∂f/∂θ` of a scalar model value at `theta`
/// (used for bands of models without analytic derivatives). `value(θ)`
/// returns `None` where the model is undefined. `None` if a component cannot
/// be computed.
pub fn numeric_gradient(value: &mut dyn FnMut(&[f64]) -> Option<f64>, theta: &[f64]) -> Option<Vec<f64>> {
    let f0 = value(theta)?;
    let mut problem = stat_core::optimize::ResidualFn::new(1, |t: &[f64], out: &mut [f64]| match value(t) {
        Some(v) => {
            out[0] = v;
            true
        }
        None => false,
    });
    let mut out = Matrix::zeros(1, theta.len());
    numeric_jacobian(&mut problem, theta, &[f0], &[], &[], &mut out)?;
    Some(out.row(0))
}
