//! Initial value problems for ordinary differential equations.
//!
//! [`solve_initial_value`] solves `y' = g(x, y)`, `y(x₀) = y₀`, `y ∈ ℝᵐ`, at a
//! list of output points. It is used to fit models defined by differential
//! equations: the model is solved once per evaluation of the objective, at the
//! observed `x` values.
//!
//! # Method
//!
//! The explicit Runge–Kutta pair of order 5(4) of Dormand & Prince (1980),
//! "A family of embedded Runge–Kutta formulae", *J. Comput. Appl. Math.*
//! 6:19–26, implemented as described for the code DOPRI5 by Hairer, Nørsett &
//! Wanner, *Solving Ordinary Differential Equations I* (2nd ed., 1993), §II.4
//! and §II.5:
//!
//! * **Local extrapolation and FSAL.** The step is advanced with the
//!   fifth-order solution; the embedded fourth-order solution only estimates
//!   the error. The last stage is evaluated at the new point, so it is the
//!   first stage of the next step ("first same as last"): six evaluations of
//!   `g` per accepted step.
//! * **Error norm.** With the error estimate `errᵢ = h Σⱼ eⱼ kⱼ,ᵢ` (`e = b − b̂`)
//!   and scale `scᵢ = atol + rtol · max(|y₀ᵢ|, |y₁ᵢ|)`, the step is accepted
//!   when `‖err‖ = √(m⁻¹ Σ (errᵢ/scᵢ)²) ≤ 1` (Hairer et al. eq. II.4.11).
//! * **Step-size control.** The PI (proportional–integral) controller of
//!   DOPRI5 (Hairer et al. §IV.2, eq. 2.43c, after Gustafsson 1991):
//!   `h_new = h · min(facmax, max(facmin, 0.9 · ‖err‖^{−α} · ‖err_old‖^{β}))`
//!   with `α = 1/5 − 0.75 β`, `β = 0.04`, safety factor 0.9, `facmin = 0.2`,
//!   `facmax = 10`, and `‖err_old‖` the norm of the previous accepted step
//!   (at least 10⁻⁴). After a rejection the step shrinks by
//!   `max(facmin, 0.9 ‖err‖^{−α})` and the following accepted step may not
//!   grow.
//! * **Initial step.** Unless one is given, the starting step comes from the
//!   algorithm of Hairer et al. §II.4 ("Starting step size", steps (a)–(f)):
//!   an explicit Euler step of size `h₀ = 0.01 ‖y₀‖/‖g(x₀, y₀)‖` estimates the
//!   second derivative, and `h₁ = (0.01 / max(‖y′‖, ‖y″‖))^{1/5}`;
//!   the start is `min(100 h₀, h₁)`.
//! * **Dense output.** Outputs that fall inside a step are interpolated with
//!   the continuous extension of order 4 of Hairer et al. §II.6 (the
//!   coefficients of Shampine 1986 used by DOPRI5), so output points never
//!   shorten the steps: asking for a thousand outputs costs the same number
//!   of steps as asking for the last one.
//!
//! Outputs on both sides of `x₀` are allowed: the solver integrates forward
//! from `x₀` through the outputs above it and, separately, backward from `x₀`
//! through those below it.
//!
//! The method is explicit, so it is meant for non-stiff problems. On a stiff
//! problem the step size collapses and the routine stops with a step-limit or
//! step-size error rather than returning an inaccurate answer.

use crate::error::{Result, StatError, invalid};

/// Accuracy and work limits for [`solve_initial_value`].
#[derive(Debug, Clone, PartialEq)]
pub struct OdeOptions {
    /// Relative error tolerance per step (default `1e-8`). Must be finite and
    /// non-negative; values below `1e-14` are raised to `1e-14`, the smallest
    /// a fifth-order method in double precision can honour.
    pub relative_tolerance: f64,
    /// Absolute error tolerance per step (default `1e-10`), which governs
    /// components close to zero. Must be finite and non-negative, and not
    /// zero when the relative tolerance is zero.
    pub absolute_tolerance: f64,
    /// Maximum number of attempted steps (accepted plus rejected) over the
    /// whole solve, both directions together (default `100_000`).
    pub max_steps: usize,
    /// Magnitude of the first step in each direction; `None` (the default)
    /// chooses it automatically (Hairer et al. §II.4). Must be finite and
    /// positive when given.
    pub initial_step: Option<f64>,
}

impl Default for OdeOptions {
    fn default() -> Self {
        Self { relative_tolerance: 1e-8, absolute_tolerance: 1e-10, max_steps: 100_000, initial_step: None }
    }
}

/// The right-hand side `g` of `y' = g(x, y)`: `rhs(x, y, dydx)` writes
/// `g(x, y)` into `dydx` and returns `false` where `g` cannot be computed.
pub type RightHandSide<'a> = dyn FnMut(f64, &[f64], &mut [f64]) -> bool + 'a;

/// The solution of an initial value problem at the requested outputs.
#[derive(Debug, Clone, PartialEq)]
pub struct OdeSolution {
    /// One state vector `y(xᵢ)` per requested output `xᵢ`, in the order the
    /// outputs were given.
    pub values: Vec<Vec<f64>>,
    /// Number of accepted steps.
    pub steps: usize,
    /// Number of rejected steps (error too large, or `g` could not be
    /// evaluated at a trial point).
    pub rejected: usize,
    /// Number of evaluations of the right-hand side `g`.
    pub evaluations: usize,
}

// Dormand–Prince 5(4) coefficients (Hairer et al., Table II.5.2).
const C2: f64 = 1.0 / 5.0;
const C3: f64 = 3.0 / 10.0;
const C4: f64 = 4.0 / 5.0;
const C5: f64 = 8.0 / 9.0;
const A21: f64 = 1.0 / 5.0;
const A31: f64 = 3.0 / 40.0;
const A32: f64 = 9.0 / 40.0;
const A41: f64 = 44.0 / 45.0;
const A42: f64 = -56.0 / 15.0;
const A43: f64 = 32.0 / 9.0;
const A51: f64 = 19372.0 / 6561.0;
const A52: f64 = -25360.0 / 2187.0;
const A53: f64 = 64448.0 / 6561.0;
const A54: f64 = -212.0 / 729.0;
const A61: f64 = 9017.0 / 3168.0;
const A62: f64 = -355.0 / 33.0;
const A63: f64 = 46732.0 / 5247.0;
const A64: f64 = 49.0 / 176.0;
const A65: f64 = -5103.0 / 18656.0;
const A71: f64 = 35.0 / 384.0;
const A73: f64 = 500.0 / 1113.0;
const A74: f64 = 125.0 / 192.0;
const A75: f64 = -2187.0 / 6784.0;
const A76: f64 = 11.0 / 84.0;
// Error coefficients e = b − b̂.
const E1: f64 = 71.0 / 57600.0;
const E3: f64 = -71.0 / 16695.0;
const E4: f64 = 71.0 / 1920.0;
const E5: f64 = -17253.0 / 339200.0;
const E6: f64 = 22.0 / 525.0;
const E7: f64 = -1.0 / 40.0;
// Continuous extension of order 4 (Hairer et al. §II.6, as in DOPRI5).
const D1: f64 = -12715105075.0 / 11282082432.0;
const D3: f64 = 87487479700.0 / 32700410799.0;
const D4: f64 = -10690763975.0 / 1880347072.0;
const D5: f64 = 701980252875.0 / 199316789632.0;
const D6: f64 = -1453857185.0 / 822651844.0;
const D7: f64 = 69997945.0 / 29380423.0;

const SAFETY: f64 = 0.9;
const FAC_MIN: f64 = 0.2;
const FAC_MAX: f64 = 10.0;
const BETA: f64 = 0.04;
const ALPHA: f64 = 0.2 - BETA * 0.75;
const MIN_RELATIVE_TOLERANCE: f64 = 1e-14;
/// Factor by which the step shrinks when `g` cannot be evaluated at a trial point.
const FAILURE_SHRINK: f64 = 0.25;

/// Solve the initial value problem `y' = g(x, y)`, `y(x₀) = y₀` at the points
/// `outputs`, by the Dormand–Prince 5(4) method with adaptive steps and dense
/// output (see the [module documentation](self) for the method and its
/// references).
///
/// * `rhs(x, y, dydx)` writes `g(x, y)` into `dydx` (every component; it has
///   the length of `y₀`) and returns `true`, or returns `false` where `g`
///   cannot be computed (for example a logarithm of a negative number). A
///   failure, or a non-finite derivative, at a trial point of a step rejects
///   that step and retries with a quarter of the step, so `g` may be
///   undefined beyond the solution's path.
/// * `x0`, `y0` — the initial point and state (at least one component).
/// * `outputs` — where the solution is wanted, on either side of `x0` and in
///   any order (repeats allowed). An output equal to `x0` returns `y0`.
/// * `options` — tolerances and limits ([`OdeOptions::default`]: relative
///   `1e-8`, absolute `1e-10`).
///
/// The tolerances bound the error committed in each step; the global error
/// at an output is usually of the same order but can be larger on long or
/// unstable integrations.
///
/// # Errors
/// * [`StatError::NonFiniteInput`] when `x0`, `y0` or an output is NaN or
///   infinite.
/// * [`StatError::InvalidArgument`] when `y0` is empty, a tolerance is
///   negative or not finite, both tolerances are zero, `max_steps` is zero or
///   `initial_step` is not finite and positive; when `g` cannot be evaluated
///   at `(x0, y0)`; and, with a message saying where, when the solution
///   cannot be continued: the step size falls below the precision of `x`
///   (the solution blows up, `g` is undefined there, or the problem is
///   stiff), or `max_steps` steps were not enough.
pub fn solve_initial_value(
    rhs: &mut RightHandSide<'_>,
    x0: f64,
    y0: &[f64],
    outputs: &[f64],
    options: &OdeOptions,
) -> Result<OdeSolution> {
    if !x0.is_finite() {
        return Err(StatError::NonFiniteInput { context: "initial point of the differential equation" });
    }
    if y0.is_empty() {
        return Err(invalid("give at least one initial value for the differential equation"));
    }
    if y0.iter().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "initial values of the differential equation" });
    }
    if outputs.iter().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "points where the differential equation is solved" });
    }
    let OdeOptions { relative_tolerance: rtol, absolute_tolerance: atol, max_steps, initial_step } = *options;
    if !(rtol.is_finite() && rtol >= 0.0) {
        return Err(invalid(format!(
            "the relative tolerance of the differential equation solver must be a non-negative number, not {rtol}"
        )));
    }
    if !(atol.is_finite() && atol >= 0.0) {
        return Err(invalid(format!(
            "the absolute tolerance of the differential equation solver must be a non-negative number, not {atol}"
        )));
    }
    if rtol == 0.0 && atol == 0.0 {
        return Err(invalid(
            "the relative and absolute tolerances of the differential equation solver cannot both be zero",
        ));
    }
    if max_steps == 0 {
        return Err(invalid("the differential equation solver needs a step limit of at least 1"));
    }
    if let Some(h) = initial_step
        && !(h.is_finite() && h > 0.0)
    {
        return Err(invalid(format!("the initial step of the differential equation solver must be positive, not {h}")));
    }
    let rtol = if rtol > 0.0 { rtol.max(MIN_RELATIVE_TOLERANCE) } else { 0.0 };

    let m = y0.len();
    let mut solver = Solver {
        rhs,
        rtol,
        atol,
        max_steps,
        initial_step,
        steps: 0,
        rejected: 0,
        evaluations: 0,
        k: std::array::from_fn(|_| vec![0.0; m]),
        ytmp: vec![0.0; m],
        ynew: vec![0.0; m],
    };
    let mut values: Vec<Vec<f64>> = vec![Vec::new(); outputs.len()];
    let mut forward: Vec<usize> = Vec::new();
    let mut backward: Vec<usize> = Vec::new();
    for (i, &x) in outputs.iter().enumerate() {
        if x > x0 {
            forward.push(i);
        } else if x < x0 {
            backward.push(i);
        } else {
            values[i] = y0.to_vec();
        }
    }
    forward.sort_by(|&a, &b| outputs[a].total_cmp(&outputs[b]));
    backward.sort_by(|&a, &b| outputs[b].total_cmp(&outputs[a]));

    let mut f0 = vec![0.0; m];
    if (!forward.is_empty() || !backward.is_empty()) && !solver.eval(x0, y0, &mut f0) {
        return Err(invalid(format!(
            "the differential equation could not be evaluated at its starting point x = {}; check the \
             equation and the initial values",
            show(x0)
        )));
    }
    for order in [&forward, &backward] {
        if !order.is_empty() {
            solver.integrate(x0, y0, &f0, outputs, order, &mut values)?;
        }
    }
    Ok(OdeSolution { values, steps: solver.steps, rejected: solver.rejected, evaluations: solver.evaluations })
}

/// Integration state shared by the forward and backward passes.
struct Solver<'a, 'b> {
    rhs: &'a mut RightHandSide<'b>,
    rtol: f64,
    atol: f64,
    max_steps: usize,
    initial_step: Option<f64>,
    steps: usize,
    rejected: usize,
    evaluations: usize,
    /// Stages k₁ … k₇ (k₇ = g at the new point, the next step's k₁).
    k: [Vec<f64>; 7],
    ytmp: Vec<f64>,
    ynew: Vec<f64>,
}

impl Solver<'_, '_> {
    /// Evaluate `g(x, y)` into `out`; `false` when it fails or is not finite.
    fn eval(&mut self, x: f64, y: &[f64], out: &mut [f64]) -> bool {
        self.evaluations += 1;
        (self.rhs)(x, y, out) && out.iter().all(|v| v.is_finite())
    }

    /// Evaluate stage `stage` at `x` from `self.ytmp`.
    fn stage(&mut self, x: f64, stage: usize) -> bool {
        let mut k = std::mem::take(&mut self.k[stage]);
        let ytmp = std::mem::take(&mut self.ytmp);
        let ok = self.eval(x, &ytmp, &mut k);
        self.ytmp = ytmp;
        self.k[stage] = k;
        ok
    }

    /// `ytmp = y + h Σ coef[j] k[j]`.
    fn combine(&mut self, y: &[f64], h: f64, coef: &[f64]) {
        for i in 0..y.len() {
            let mut s = 0.0;
            for (j, c) in coef.iter().enumerate() {
                s += c * self.k[j][i];
            }
            self.ytmp[i] = y[i] + h * s;
        }
    }

    /// Weighted RMS norm `√(m⁻¹ Σ (vᵢ/scᵢ)²)` with `scᵢ = atol + rtol |yᵢ|`.
    fn norm(&self, v: &[f64], y: &[f64]) -> f64 {
        let s: f64 = v.iter().zip(y).map(|(a, b)| (a / (self.atol + self.rtol * b.abs())).powi(2)).sum();
        (s / v.len() as f64).sqrt()
    }

    /// Starting step (Hairer et al. §II.4), magnitude only.
    fn starting_step(&mut self, x0: f64, y0: &[f64], f0: &[f64], dir: f64, hmax: f64) -> f64 {
        let d0 = self.norm(y0, y0);
        let d1 = self.norm(f0, y0);
        let mut h0 = if d0 < 1e-5 || d1 < 1e-5 { 1e-6 } else { 0.01 * d0 / d1 };
        h0 = h0.min(hmax);
        for (i, t) in self.ytmp.iter_mut().enumerate() {
            *t = y0[i] + dir * h0 * f0[i];
        }
        let d2 = if self.stage(x0 + dir * h0, 1) {
            let diff: Vec<f64> = self.k[1].iter().zip(f0).map(|(a, b)| a - b).collect();
            self.norm(&diff, y0) / h0
        } else {
            // g failed at the trial point: fall back to a cautious start.
            return h0 * 1e-3;
        };
        let d = d1.max(d2);
        let h1 = if d <= 1e-15 { (h0 * 1e-3).max(1e-6) } else { (0.01 / d).powf(0.2) };
        (100.0 * h0).min(h1).min(hmax)
    }

    /// Integrate from `(x0, y0)` (with `f0 = g(x0, y0)`) through the outputs
    /// `order` (indices into `outputs`, sorted away from `x0`), storing each
    /// state in `values`.
    fn integrate(
        &mut self,
        x0: f64,
        y0: &[f64],
        f0: &[f64],
        outputs: &[f64],
        order: &[usize],
        values: &mut [Vec<f64>],
    ) -> Result<()> {
        let m = y0.len();
        let xend = outputs[*order.last().expect("non-empty")];
        let dir = if xend > x0 { 1.0 } else { -1.0 };
        let hmax = (xend - x0).abs();
        let mut x = x0;
        let mut y = y0.to_vec();
        self.k[0].copy_from_slice(f0);
        let mut h = match self.initial_step {
            Some(h) => h.min(hmax),
            None => self.starting_step(x0, y0, f0, dir, hmax),
        } * dir;
        let mut err_old = 1e-4_f64;
        let mut last_rejected = false;
        let mut last_failed = false;
        let mut next = 0;
        let mut dense = vec![0.0; m];
        loop {
            if self.steps + self.rejected >= self.max_steps {
                return Err(invalid(format!(
                    "the differential equation needed more than {} steps to reach x = {} (it reached x = {}); \
                     the solution may change very quickly or blow up there, or the problem may be stiff; \
                     loosen the tolerances or raise the step limit",
                    self.max_steps,
                    show(xend),
                    show(x)
                )));
            }
            if h == 0.0 || 0.1 * h.abs() <= x.abs() * f64::EPSILON {
                return Err(if last_failed {
                    invalid(format!(
                        "the differential equation could not be solved past x = {}: its right-hand side has no \
                         finite value just beyond that point (for example a square root or logarithm of a \
                         negative number), or the solution blows up there",
                        show(x)
                    ))
                } else {
                    invalid(format!(
                        "the differential equation could not be solved past x = {}; the solution may blow up \
                         (become infinite) there, or the equation may be too stiff for this solver",
                        show(x)
                    ))
                });
            }
            let last = (x + 1.01 * h - xend) * dir >= 0.0;
            if last {
                h = xend - x;
            }
            let xnew = if last { xend } else { x + h };

            // Stages 2–7.
            let ok = 'stages: {
                self.combine(&y, h, &[A21]);
                if !self.stage(x + C2 * h, 1) {
                    break 'stages false;
                }
                self.combine(&y, h, &[A31, A32]);
                if !self.stage(x + C3 * h, 2) {
                    break 'stages false;
                }
                self.combine(&y, h, &[A41, A42, A43]);
                if !self.stage(x + C4 * h, 3) {
                    break 'stages false;
                }
                self.combine(&y, h, &[A51, A52, A53, A54]);
                if !self.stage(x + C5 * h, 4) {
                    break 'stages false;
                }
                self.combine(&y, h, &[A61, A62, A63, A64, A65]);
                if !self.stage(xnew, 5) {
                    break 'stages false;
                }
                self.combine(&y, h, &[A71, 0.0, A73, A74, A75, A76]);
                std::mem::swap(&mut self.ytmp, &mut self.ynew);
                if self.ynew.iter().any(|v| !v.is_finite()) {
                    break 'stages false;
                }
                let ynew = std::mem::take(&mut self.ynew);
                let mut k7 = std::mem::take(&mut self.k[6]);
                let ok = self.eval(xnew, &ynew, &mut k7);
                self.ynew = ynew;
                self.k[6] = k7;
                ok
            };
            if !ok {
                self.rejected += 1;
                last_rejected = true;
                last_failed = true;
                h *= FAILURE_SHRINK;
                continue;
            }

            // Error estimate and its norm (scale uses max(|y₀|, |y₁|)).
            let mut sum = 0.0;
            for i in 0..m {
                let k = &self.k;
                let e = h * (E1 * k[0][i] + E3 * k[2][i] + E4 * k[3][i] + E5 * k[4][i] + E6 * k[5][i] + E7 * k[6][i]);
                let sc = self.atol + self.rtol * y[i].abs().max(self.ynew[i].abs());
                sum += (e / sc).powi(2);
            }
            let err = (sum / m as f64).sqrt();
            if !err.is_finite() {
                self.rejected += 1;
                last_rejected = true;
                last_failed = true;
                h *= FAILURE_SHRINK;
                continue;
            }
            last_failed = false;
            let fac_err = err.powf(ALPHA);
            if err <= 1.0 {
                self.steps += 1;
                // Dense output for every output inside (x, xnew].
                let mut have_dense = false;
                while next < order.len() {
                    let idx = order[next];
                    let t = outputs[idx];
                    if (t - xnew) * dir > 0.0 {
                        break;
                    }
                    if t == xnew {
                        values[idx] = self.ynew.clone();
                    } else {
                        if !have_dense {
                            let k = &self.k;
                            for (i, d) in dense.iter_mut().enumerate() {
                                *d = h
                                    * (D1 * k[0][i]
                                        + D3 * k[2][i]
                                        + D4 * k[3][i]
                                        + D5 * k[4][i]
                                        + D6 * k[5][i]
                                        + D7 * k[6][i]);
                            }
                            have_dense = true;
                        }
                        let theta = (t - x) / h;
                        let theta1 = 1.0 - theta;
                        values[idx] = (0..m)
                            .map(|i| {
                                let ydiff = self.ynew[i] - y[i];
                                let bspl = h * self.k[0][i] - ydiff;
                                let r4 = ydiff - h * self.k[6][i] - bspl;
                                y[i] + theta * (ydiff + theta1 * (bspl + theta * (r4 + theta1 * dense[i])))
                            })
                            .collect();
                    }
                    next += 1;
                }
                if last {
                    return Ok(());
                }
                // Advance; FSAL: the last stage is the next first stage.
                x = xnew;
                y.copy_from_slice(&self.ynew);
                self.k.swap(0, 6);
                let fac = (fac_err / err_old.powf(BETA) / SAFETY).clamp(1.0 / FAC_MAX, 1.0 / FAC_MIN);
                let mut hnew = h / fac;
                if last_rejected {
                    hnew = dir * hnew.abs().min(h.abs());
                }
                err_old = err.max(1e-4);
                last_rejected = false;
                h = dir * hnew.abs().min(hmax);
            } else {
                self.rejected += 1;
                last_rejected = true;
                h /= (fac_err / SAFETY).min(1.0 / FAC_MIN);
            }
        }
    }
}

/// `x` with six significant digits, without trailing zeros, for messages.
fn show(x: f64) -> String {
    if x == 0.0 {
        return "0".into();
    }
    let magnitude = x.abs().log10().floor();
    if (-4.0..6.0).contains(&magnitude) {
        let decimals = (5.0 - magnitude).max(0.0) as usize;
        let s = format!("{x:.decimals$}");
        let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
        if s == "-0" { "0".into() } else { s }
    } else {
        format!("{x:.5e}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solve(
        mut f: impl FnMut(f64, &[f64], &mut [f64]),
        x0: f64,
        y0: &[f64],
        outputs: &[f64],
        options: &OdeOptions,
    ) -> Result<OdeSolution> {
        let mut rhs = |x: f64, y: &[f64], d: &mut [f64]| {
            f(x, y, d);
            true
        };
        solve_initial_value(&mut rhs, x0, y0, outputs, options)
    }

    fn rel(a: f64, b: f64) -> f64 {
        (a - b).abs() / b.abs()
    }

    #[test]
    fn exponential_decay_matches_exact_solution_both_directions() {
        let k = 0.5;
        let outputs: Vec<f64> = (0..=40).map(|i| -4.0 + 0.25 * f64::from(i)).collect();
        let sol = solve(|_, y, d| d[0] = -k * y[0], 0.0, &[2.0], &outputs, &OdeOptions::default()).unwrap();
        let mut worst = 0.0_f64;
        for (x, v) in outputs.iter().zip(&sol.values) {
            worst = worst.max(rel(v[0], 2.0 * (-k * x).exp()));
        }
        assert!(worst < 1e-8, "worst relative error {worst:e}");
        // x = 0 is returned exactly.
        assert_eq!(sol.values[16], vec![2.0]);
    }

    #[test]
    fn logistic_growth_matches_exact_solution() {
        let (r, cap, y0) = (0.8, 50.0, 2.0);
        let exact = |x: f64| cap / (1.0 + (cap / y0 - 1.0) * (-r * x).exp());
        let outputs = [7.5, -3.0, 0.1, 1.0, 2.0, 4.0, 6.0, 10.0, 15.0, 20.0, -1.0];
        let sol = solve(|_, y, d| d[0] = r * y[0] * (1.0 - y[0] / cap), 0.0, &[y0], &outputs, &OdeOptions::default())
            .unwrap();
        for (x, v) in outputs.iter().zip(&sol.values) {
            let e = rel(v[0], exact(*x));
            // The global error of a 1e-8 per-step tolerance; SciPy's RK45 at the
            // same tolerances is off by 8e-8 at x = 4.
            assert!(e < 5e-8, "x = {x}: {} vs {}", v[0], exact(*x));
        }
    }

    #[test]
    fn harmonic_oscillator_matches_exact_solution() {
        let w = 2.0;
        let outputs: Vec<f64> = (1..=50).map(|i| 0.2 * f64::from(i)).collect();
        let sol = solve(
            |_, y, d| {
                d[0] = y[1];
                d[1] = -w * w * y[0];
            },
            0.0,
            &[1.0, 0.0],
            &outputs,
            &OdeOptions::default(),
        )
        .unwrap();
        for (x, v) in outputs.iter().zip(&sol.values) {
            assert!((v[0] - (w * x).cos()).abs() < 2e-8, "x = {x}");
            assert!((v[1] + w * (w * x).sin()).abs() < 4e-8, "x = {x}");
        }
    }

    /// Reference: `crates/stat-core/tests/data/ode_reference.py` (SciPy
    /// `solve_ivp`, DOP853, rtol = 1e-13, atol = 1e-14).
    #[test]
    fn lotka_volterra_matches_scipy_dop853() {
        let reference: [(f64, [f64; 2]); 8] = [
            (0.5, [0.45410356437244037, 5.379621279539642]),
            (1.0, [0.2185389322323087, 1.3765927010135046]),
            (2.0, [0.611979793607326, 0.09725106959719129]),
            (3.5, [5.510443610752143, 0.030046101705408702]),
            (5.0, [0.2641867721186669, 3.1341331610700722]),
            (7.5, [3.5535061740332345, 0.019462172970027235]),
            (10.0, [0.2872129642021005, 0.44977746350616554]),
            (15.0, [0.7137513780977341, 0.07540779624080225]),
        ];
        let (a, b, c, d) = (1.5, 1.0, 3.0, 1.0);
        let outputs: Vec<f64> = reference.iter().map(|r| r.0).collect();
        let sol = solve(
            |_, y, dy| {
                dy[0] = a * y[0] - b * y[0] * y[1];
                dy[1] = -c * y[1] + d * y[0] * y[1];
            },
            0.0,
            &[10.0, 5.0],
            &outputs,
            &OdeOptions::default(),
        )
        .unwrap();
        for ((x, want), got) in reference.iter().zip(&sol.values) {
            for j in 0..2 {
                assert!(rel(got[j], want[j]) < 2e-7, "x = {x}, component {j}: {} vs {}", got[j], want[j]);
            }
        }
        // Tighter tolerances get closer to the reference.
        let tight = OdeOptions { relative_tolerance: 1e-12, absolute_tolerance: 1e-14, ..OdeOptions::default() };
        let sol = solve(
            |_, y, dy| {
                dy[0] = a * y[0] - b * y[0] * y[1];
                dy[1] = -c * y[1] + d * y[0] * y[1];
            },
            0.0,
            &[10.0, 5.0],
            &outputs,
            &tight,
        )
        .unwrap();
        for ((x, want), got) in reference.iter().zip(&sol.values) {
            for j in 0..2 {
                assert!(rel(got[j], want[j]) < 1e-10, "x = {x}, component {j}: {} vs {}", got[j], want[j]);
            }
        }
    }

    /// Reference: `crates/stat-core/tests/data/ode_reference.py` (SciPy
    /// `solve_ivp`, DOP853, rtol = 1e-13, atol = 1e-14).
    #[test]
    fn nonlinear_scalar_equation_matches_scipy_dop853() {
        let reference: [(f64, f64); 8] = [
            (-2.0, -0.5559293909277262),
            (-0.7, -0.028274410087923963),
            (0.3, 0.7103681941279119),
            (1.0, 1.0471844260678052),
            (2.5, 1.1797785246324934),
            (5.0, 0.7616049096517793),
            (10.0, 0.9709792902584256),
            (20.0, 1.1364661200656028),
        ];
        let outputs: Vec<f64> = reference.iter().map(|r| r.0).collect();
        let sol =
            solve(|x, y, d| d[0] = 1.0 + 0.5 * x.sin() - y[0] * y[0], 0.0, &[0.5], &outputs, &OdeOptions::default())
                .unwrap();
        for ((x, want), got) in reference.iter().zip(&sol.values) {
            assert!((got[0] - want).abs() < 1e-8 * want.abs().max(0.1), "x = {x}: {} vs {want}", got[0]);
        }
    }

    #[test]
    fn dense_output_matches_steps_ending_on_each_output_and_costs_no_steps() {
        let f = |x: f64, y: &[f64], d: &mut [f64]| d[0] = 1.0 + 0.5 * x.sin() - y[0] * y[0];
        let options = OdeOptions::default();
        let many: Vec<f64> = (1..=400).map(|i| 0.025 * f64::from(i)).collect();
        let dense = solve(f, 0.0, &[0.5], &many, &options).unwrap();
        let last_only = solve(f, 0.0, &[0.5], &[10.0], &options).unwrap();
        // Outputs do not shorten steps.
        assert_eq!(dense.steps, last_only.steps);
        assert_eq!(dense.values[399], last_only.values[0]);
        // Each output interpolated inside a step agrees with a solve whose
        // last step ends exactly on it, run with much tighter tolerances
        // (small steps).
        let tight = OdeOptions { relative_tolerance: 1e-13, absolute_tolerance: 1e-15, ..OdeOptions::default() };
        let mut worst = 0.0_f64;
        for (i, &x) in many.iter().enumerate().step_by(7) {
            let exact = solve(f, 0.0, &[0.5], &[x], &tight).unwrap();
            worst = worst.max((dense.values[i][0] - exact.values[0][0]).abs());
        }
        assert!(worst < 1e-8, "worst dense-output error {worst:e}");
    }

    #[test]
    fn tightening_the_tolerance_reduces_the_error_in_proportion() {
        let error_at = |rtol: f64| {
            let options = OdeOptions { relative_tolerance: rtol, absolute_tolerance: 1e-16, ..OdeOptions::default() };
            let sol = solve(
                |_, y, d| {
                    d[0] = y[1];
                    d[1] = -y[0];
                },
                0.0,
                &[1.0, 0.0],
                &[10.0],
                &options,
            )
            .unwrap();
            ((sol.values[0][0] - 10f64.cos()).abs() + (sol.values[0][1] + 10f64.sin()).abs(), sol.steps)
        };
        let (e4, s4) = error_at(1e-4);
        let (e6, s6) = error_at(1e-6);
        let (e8, s8) = error_at(1e-8);
        // Global error roughly proportional to the tolerance (h ∝ tol^{1/5},
        // error ∝ h⁵), and steps growing by about 100^{1/5} ≈ 2.5.
        assert!(e4 / e6 > 20.0 && e6 / e8 > 20.0, "{e4:e} {e6:e} {e8:e}");
        assert!(2 * s6 > 3 * s4 && 2 * s8 > 3 * s6 && s8 < 4 * s6, "{s4} {s6} {s8}");
    }

    #[test]
    fn outputs_at_the_start_and_in_any_order() {
        let outputs = [3.0, 0.0, -1.0, 3.0, 1.0, -2.0];
        let sol = solve(|_, y, d| d[0] = y[0], 0.0, &[1.0], &outputs, &OdeOptions::default()).unwrap();
        for (x, v) in outputs.iter().zip(&sol.values) {
            assert!(rel(v[0], x.exp()) < 1e-8, "x = {x}");
        }
        assert_eq!(sol.values[1], vec![1.0]);
        assert_eq!(sol.values[0], sol.values[3]);
        // Only x0 requested: g is never evaluated.
        let mut calls = 0;
        let mut rhs = |_: f64, y: &[f64], d: &mut [f64]| {
            calls += 1;
            d[0] = y[0];
            true
        };
        let sol = solve_initial_value(&mut rhs, 1.0, &[4.0], &[1.0], &OdeOptions::default()).unwrap();
        assert_eq!((sol.values, sol.evaluations), (vec![vec![4.0]], 0));
        assert_eq!(calls, 0);
        // No outputs at all.
        let sol = solve(|_, y, d| d[0] = y[0], 0.0, &[1.0], &[], &OdeOptions::default()).unwrap();
        assert!(sol.values.is_empty());
    }

    #[test]
    fn evaluation_count_reflects_fsal() {
        let sol = solve(|_, y, d| d[0] = -y[0], 0.0, &[1.0], &[5.0], &OdeOptions::default()).unwrap();
        // One evaluation at x0, one for the starting-step estimate, and six
        // per attempted step (the seventh stage is reused: FSAL).
        assert_eq!(sol.evaluations, 1 + 1 + 6 * (sol.steps + sol.rejected));
    }

    #[test]
    fn given_initial_step_is_used() {
        let options = OdeOptions { initial_step: Some(1e-3), ..OdeOptions::default() };
        let sol = solve(|_, y, d| d[0] = -y[0], 0.0, &[1.0], &[-2.0, 2.0], &options).unwrap();
        assert!(rel(sol.values[0][0], 2f64.exp()) < 1e-8 && rel(sol.values[1][0], (-2f64).exp()) < 1e-8);
        assert_eq!(sol.evaluations, 1 + 6 * (sol.steps + sol.rejected));
    }

    #[test]
    fn blow_up_reports_where_the_solution_could_not_be_continued() {
        // y' = y², y(0) = 1 has y = 1/(1 − x), infinite at x = 1.
        let err = solve(|_, y, d| d[0] = y[0] * y[0], 0.0, &[1.0], &[0.5, 2.0], &OdeOptions::default()).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("could not be solved past x = 1") && message.contains("blow up"), "{message}");
        // Before the singularity it is fine.
        let sol = solve(|_, y, d| d[0] = y[0] * y[0], 0.0, &[1.0], &[0.9], &OdeOptions::default()).unwrap();
        assert!(rel(sol.values[0][0], 10.0) < 1e-8);
    }

    #[test]
    fn right_hand_side_failure_is_reported() {
        // g undefined beyond x = 2 (say, the log of a negative number).
        let mut rhs = |x: f64, y: &[f64], d: &mut [f64]| {
            d[0] = -y[0];
            x <= 2.0
        };
        let err = solve_initial_value(&mut rhs, 0.0, &[1.0], &[1.0, 3.0], &OdeOptions::default()).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("could not be solved past x = 2") && message.contains("no finite value"), "{message}");
        // Failure at the start.
        let mut never = |_: f64, _: &[f64], _: &mut [f64]| false;
        let err = solve_initial_value(&mut never, 0.0, &[1.0], &[1.0], &OdeOptions::default()).unwrap_err();
        assert!(err.to_string().contains("could not be evaluated at its starting point x = 0"), "{err}");
        // A non-finite derivative counts as a failure.
        let mut nan = |_: f64, y: &[f64], d: &mut [f64]| {
            d[0] = if y[0] > 1.5 { f64::NAN } else { 1.0 };
            true
        };
        let err = solve_initial_value(&mut nan, 0.0, &[1.0], &[2.0], &OdeOptions::default()).unwrap_err();
        assert!(err.to_string().contains("could not be solved past x = 0.5"), "{err}");
    }

    #[test]
    fn step_limit_is_reported() {
        let options = OdeOptions { max_steps: 10, ..OdeOptions::default() };
        let err = solve(|x, _, d| d[0] = (50.0 * x).cos(), 0.0, &[0.0], &[20.0], &options).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("more than 10 steps to reach x = 20"), "{message}");
    }

    #[test]
    fn invalid_inputs_are_rejected() {
        let f = |_: f64, y: &[f64], d: &mut [f64]| d[0] = y[0];
        let ok = OdeOptions::default();
        assert!(matches!(solve(f, f64::NAN, &[1.0], &[1.0], &ok), Err(StatError::NonFiniteInput { .. })));
        assert!(matches!(solve(f, 0.0, &[f64::INFINITY], &[1.0], &ok), Err(StatError::NonFiniteInput { .. })));
        assert!(matches!(solve(f, 0.0, &[1.0], &[1.0, f64::NAN], &ok), Err(StatError::NonFiniteInput { .. })));
        assert!(matches!(solve(f, 0.0, &[], &[1.0], &ok), Err(StatError::InvalidArgument { .. })));
        let bad = [
            OdeOptions { relative_tolerance: -1e-8, ..OdeOptions::default() },
            OdeOptions { relative_tolerance: f64::NAN, ..OdeOptions::default() },
            OdeOptions { absolute_tolerance: f64::INFINITY, ..OdeOptions::default() },
            OdeOptions { relative_tolerance: 0.0, absolute_tolerance: 0.0, ..OdeOptions::default() },
            OdeOptions { max_steps: 0, ..OdeOptions::default() },
            OdeOptions { initial_step: Some(0.0), ..OdeOptions::default() },
            OdeOptions { initial_step: Some(f64::NAN), ..OdeOptions::default() },
        ];
        for options in &bad {
            assert!(matches!(solve(f, 0.0, &[1.0], &[1.0], options), Err(StatError::InvalidArgument { .. })));
        }
        // Absolute tolerance alone is allowed; a tiny relative one is raised.
        let abs_only = OdeOptions { relative_tolerance: 0.0, absolute_tolerance: 1e-10, ..OdeOptions::default() };
        assert!(rel(solve(f, 0.0, &[1.0], &[1.0], &abs_only).unwrap().values[0][0], 1f64.exp()) < 1e-8);
        let tiny = OdeOptions { relative_tolerance: 1e-20, ..OdeOptions::default() };
        assert!(
            rel(solve(f, 0.0, &[1.0], &[1.0], &tiny).unwrap().values[0][0], 1f64.exp()) < 1e-11,
            "{}",
            rel(solve(f, 0.0, &[1.0], &[1.0], &tiny).unwrap().values[0][0], 1f64.exp())
        );
    }

    #[test]
    fn show_formats_plainly() {
        assert_eq!(show(1.0), "1");
        assert_eq!(show(0.99999999999), "1");
        assert_eq!(show(2.5), "2.5");
        assert_eq!(show(-0.000123456789), "-0.000123457");
        assert_eq!(show(0.0), "0");
        assert_eq!(show(1.5e9), "1.50000e9");
    }
}
