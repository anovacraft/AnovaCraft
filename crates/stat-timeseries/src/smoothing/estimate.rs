//! Maximum-likelihood estimation of smoothing parameters and initial states.

use stat_core::optimize::minimize_in_box;

use super::admissible::is_admissible;
use super::recursion::{Coefficients, State, criterion};
use super::{EtsOptions, EtsSpec, SeasonalType, StateVector, TrendType};
use crate::{Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Heuristic initial states (Hyndman et al. 2008, §2.6.1).
///
/// Seasonal indices: a centred 2×m moving average (an m-term one for odd
/// `m`) over the first `min(⌊n/m⌋, 4)` cycles; the detrended values
/// (`y − MA` or `y / MA`) are averaged by season and normalised to sum to 0
/// (or average 1). Level and slope: least-squares line through the first
/// `min(10, n)` seasonally adjusted values against `t = 1, 2, …`, with the
/// intercept (the value at `t = 0`) as level; without a trend the level is
/// their mean.
pub(crate) fn heuristic_initial(y: &[f64], spec: &EtsSpec, m: usize) -> StateVector {
    let n = y.len();
    let mut seasonal = Vec::new();
    if spec.has_season() {
        let cycles = (n / m).clamp(2, 4);
        let len = (cycles * m).min(n);
        let x = &y[..len];
        let half = m / 2;
        let mut sums = vec![0.0; m];
        let mut counts = vec![0usize; m];
        for t in half..len - half {
            let ma = if m % 2 == 0 {
                (0.5 * x[t - half] + x[t + 1 - half..t + half].iter().sum::<f64>() + 0.5 * x[t + half]) / m as f64
            } else {
                x[t - half..=t + half].iter().sum::<f64>() / m as f64
            };
            let detrended = if spec.seasonal == SeasonalType::Multiplicative { x[t] / ma } else { x[t] - ma };
            sums[t % m] += detrended;
            counts[t % m] += 1;
        }
        let neutral = if spec.seasonal == SeasonalType::Multiplicative { 1.0 } else { 0.0 };
        seasonal =
            sums.iter().zip(&counts).map(|(s, &c)| if c > 0 { s / c as f64 } else { neutral }).collect::<Vec<_>>();
        let mean = seasonal.iter().sum::<f64>() / m as f64;
        for s in &mut seasonal {
            if spec.seasonal == SeasonalType::Multiplicative {
                *s /= mean;
            } else {
                *s -= mean;
            }
        }
    }
    let k = n.min(10);
    let adjusted: Vec<f64> = (0..k)
        .map(|t| match spec.seasonal {
            SeasonalType::None => y[t],
            SeasonalType::Additive => y[t] - seasonal[t % m],
            SeasonalType::Multiplicative => y[t] / seasonal[t % m],
        })
        .collect();
    let mean = adjusted.iter().sum::<f64>() / k as f64;
    if !spec.has_trend() {
        return StateVector { level: mean, slope: 0.0, seasonal };
    }
    let tbar = (k as f64 + 1.0) / 2.0;
    let (mut sxy, mut sxx) = (0.0, 0.0);
    for (i, v) in adjusted.iter().enumerate() {
        let dt = (i + 1) as f64 - tbar;
        sxy += dt * (v - mean);
        sxx += dt * dt;
    }
    let slope = if sxx > 0.0 { sxy / sxx } else { 0.0 };
    StateVector { level: mean - slope * tbar, slope, seasonal }
}

/// What is estimated, and how the search box maps to the model.
struct Layout<'a> {
    spec: &'a EtsSpec,
    m: usize,
    options: &'a EtsOptions,
    estimate_alpha: bool,
    estimate_beta: bool,
    estimate_gamma: bool,
    estimate_phi: bool,
    alpha_range: (f64, f64),
    /// Initial states (fixed, or the heuristic start).
    initial: StateVector,
    estimate_states: bool,
}

impl Layout<'_> {
    fn n_smoothing(&self) -> usize {
        usize::from(self.estimate_alpha)
            + usize::from(self.estimate_beta)
            + usize::from(self.estimate_gamma)
            + usize::from(self.estimate_phi)
    }

    fn n_states(&self) -> usize {
        if !self.estimate_states {
            return 0;
        }
        1 + usize::from(self.spec.has_trend()) + if self.spec.has_season() { self.m - 1 } else { 0 }
    }

    /// Box coordinates to coefficients and initial states; `None` when the
    /// implied normalised seasonal state is not positive (multiplicative).
    fn decode(&self, x: &[f64]) -> Option<(Coefficients, StateVector)> {
        let o = self.options;
        let (lo, hi) = o.bounds.smoothing;
        let mut i = 0;
        let mut take = || {
            let v = x[i];
            i += 1;
            v
        };
        let alpha = if self.estimate_alpha {
            self.alpha_range.0 + take() * (self.alpha_range.1 - self.alpha_range.0)
        } else {
            o.alpha.unwrap_or(0.0)
        };
        let beta = if self.estimate_beta {
            let lb = lo.min(alpha);
            lb + take() * (alpha.min(hi) - lb)
        } else {
            o.beta.unwrap_or(0.0)
        };
        let gamma = if self.estimate_gamma {
            let ub = (1.0 - alpha).min(hi);
            let lg = lo.min(ub);
            lg + take() * (ub - lg)
        } else {
            o.gamma.unwrap_or(0.0)
        };
        let phi = if self.estimate_phi {
            let (pl, ph) = o.bounds.damping;
            pl + take() * (ph - pl)
        } else {
            o.phi.unwrap_or(1.0)
        };
        let c = Coefficients {
            alpha,
            beta: if self.spec.has_trend() { beta } else { 0.0 },
            gamma: if self.spec.has_season() { gamma } else { 0.0 },
            phi: if self.spec.trend == TrendType::DampedAdditive { phi } else { 1.0 },
        };
        if !self.estimate_states {
            return Some((c, self.initial.clone()));
        }
        let level = take();
        let slope = if self.spec.has_trend() { take() } else { 0.0 };
        let mut seasonal = Vec::new();
        if self.spec.has_season() {
            seasonal = (0..self.m - 1).map(|_| take()).collect();
            let sum: f64 = seasonal.iter().sum();
            let last = if self.spec.seasonal == SeasonalType::Multiplicative { self.m as f64 - sum } else { -sum };
            if self.spec.seasonal == SeasonalType::Multiplicative && last <= 0.0 {
                return None;
            }
            seasonal.push(last);
        }
        Some((c, StateVector { level, slope, seasonal }))
    }

    /// Search box and the state part of the start point.
    fn state_box(&self, y: &[f64]) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        let (mut lower, mut upper, mut start) = (Vec::new(), Vec::new(), Vec::new());
        if !self.estimate_states {
            return (lower, upper, start);
        }
        let (min, max) = y.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
        let biggest = min.abs().max(max.abs());
        let scale = (max - min).max(1e-6 * biggest).max(1e-12);
        let init = &self.initial;
        let mut push = |s: f64, lo: f64, hi: f64| {
            lower.push(lo.min(s));
            upper.push(hi.max(s));
            start.push(s);
        };
        push(init.level, init.level - scale, init.level + scale);
        if self.spec.has_trend() {
            let half = (3.0 * init.slope.abs()).max(0.25 * scale);
            push(init.slope, init.slope - half, init.slope + half);
        }
        if self.spec.has_season() {
            for &s in &init.seasonal[..self.m - 1] {
                if self.spec.seasonal == SeasonalType::Multiplicative {
                    push(s, (s - 0.75).max(1e-3), s + 0.75);
                } else {
                    push(s, s - scale, s + scale);
                }
            }
        }
        (lower, upper, start)
    }
}

fn check_unit(name: &str, v: Option<f64>, allowed: bool, what: &str) -> Result<()> {
    if let Some(v) = v {
        if !allowed {
            return Err(invalid(format!("{name} is given but the model has no {what}")));
        }
        if !(0.0..=1.0).contains(&v) {
            return Err(invalid(format!("{name} must lie in [0, 1]")));
        }
    }
    Ok(())
}

/// Estimate the free parameters. Returns coefficients, initial states and
/// the number of parameters counted as estimated.
pub(crate) fn estimate(y: &[f64], spec: &EtsSpec, o: &EtsOptions) -> Result<(Coefficients, StateVector, usize)> {
    let m = if spec.has_season() { o.period } else { 1 };
    check_unit("alpha", o.alpha, true, "level")?;
    check_unit("beta", o.beta, spec.has_trend(), "trend")?;
    check_unit("gamma", o.gamma, spec.has_season(), "seasonal component")?;
    if let Some(phi) = o.phi {
        if spec.trend != TrendType::DampedAdditive {
            return Err(invalid("phi is given but the model has no damped trend"));
        }
        if !(phi > 0.0 && phi <= 1.0) {
            return Err(invalid("phi must lie in (0, 1]"));
        }
    }
    let (lo, hi) = o.bounds.smoothing;
    let (pl, ph) = o.bounds.damping;
    if !(lo.is_finite() && hi.is_finite() && 0.0 <= lo && lo <= hi && hi <= 1.0) {
        return Err(invalid("smoothing-parameter bounds must satisfy 0 ≤ lower ≤ upper ≤ 1"));
    }
    if !(pl.is_finite() && ph.is_finite() && 0.0 < pl && pl <= ph && ph <= 1.0) {
        return Err(invalid("damping bounds must satisfy 0 < lower ≤ upper ≤ 1"));
    }
    let initial = match &o.initial {
        Some(v) => {
            let want = if spec.has_season() { m } else { 0 };
            if v.seasonal.len() != want {
                return Err(StatError::DimensionMismatch {
                    context: "initial seasonal states vs. seasonal period",
                    expected: want,
                    found: v.seasonal.len(),
                });
            }
            if !v.level.is_finite() || !v.slope.is_finite() || v.seasonal.iter().any(|s| !s.is_finite()) {
                return Err(StatError::NonFiniteInput { context: "initial states" });
            }
            let mut v = v.clone();
            if !spec.has_trend() {
                v.slope = 0.0;
            }
            v
        }
        None => heuristic_initial(y, spec, m),
    };
    let mut alpha_range = (lo, hi);
    if let Some(b) = o.beta {
        alpha_range.0 = alpha_range.0.max(b);
    }
    if let (Some(g), true) = (o.gamma, spec.has_season()) {
        alpha_range.1 = alpha_range.1.min(1.0 - g);
    }
    let layout = Layout {
        spec,
        m,
        options: o,
        estimate_alpha: o.alpha.is_none(),
        estimate_beta: spec.has_trend() && o.beta.is_none(),
        estimate_gamma: spec.has_season() && o.gamma.is_none(),
        estimate_phi: spec.trend == TrendType::DampedAdditive && o.phi.is_none(),
        alpha_range,
        initial,
        estimate_states: o.initial.is_none(),
    };
    if layout.estimate_alpha && alpha_range.0 > alpha_range.1 {
        return Err(invalid("the fixed beta and gamma leave no admissible value for alpha"));
    }
    let n_smoothing = layout.n_smoothing();
    let estimated = if o.count_supplied_as_estimated {
        1 + usize::from(spec.has_trend())
            + usize::from(spec.has_season())
            + usize::from(spec.trend == TrendType::DampedAdditive)
            + 1
            + usize::from(spec.has_trend())
            + if spec.has_season() { m - 1 } else { 0 }
    } else {
        n_smoothing + layout.n_states()
    };
    let (state_lower, state_upper, state_start) = layout.state_box(y);
    let dims = n_smoothing + state_start.len();
    if dims == 0 {
        let (c, v) = layout.decode(&[]).ok_or_else(|| invalid("invalid initial states"))?;
        return Ok((c, v, estimated));
    }
    let check_admissible = o.bounds.admissible && spec.has_season() && n_smoothing > 0;
    let mut objective = |x: &[f64]| -> f64 {
        let Some((c, v)) = layout.decode(x) else { return f64::INFINITY };
        if check_admissible && !is_admissible(spec, &c, m) {
            return f64::INFINITY;
        }
        criterion(y, spec, &c, State::from_vector(&v, spec))
    };
    let mut lower = vec![0.0; n_smoothing];
    let mut upper = vec![1.0; n_smoothing];
    lower.extend(&state_lower);
    upper.extend(&state_upper);
    let budget = o.max_evaluations.unwrap_or(4000 * dims).max(10);
    // Starting smoothing values in box coordinates: (alpha, beta/alpha,
    // gamma/(1 − alpha), phi position).
    let starts: [[f64; 4]; 3] = [[0.2, 0.1, 0.1, 0.9], [0.5, 0.1, 0.1, 0.5], [0.8, 0.05, 0.05, 0.9]];
    let mut best: Option<(Vec<f64>, f64)> = None;
    for s in &starts {
        let mut x0 = Vec::with_capacity(dims);
        if layout.estimate_alpha {
            x0.push(s[0]);
        }
        if layout.estimate_beta {
            x0.push(s[1]);
        }
        if layout.estimate_gamma {
            x0.push(s[2]);
        }
        if layout.estimate_phi {
            x0.push(s[3]);
        }
        x0.extend(&state_start);
        let r = minimize_in_box(&mut objective, &x0, &lower, &upper, budget);
        if r.value.is_finite() && best.as_ref().is_none_or(|b| r.value < b.1) {
            best = Some((r.x, r.value));
        }
        if n_smoothing == 0 {
            break;
        }
    }
    let Some((x, _)) = best else {
        return Err(StatError::ConvergenceFailure { routine: "exponential smoothing estimation", iterations: budget });
    };
    let polished = minimize_in_box(&mut objective, &x, &lower, &upper, budget);
    let x = if polished.value.is_finite() { polished.x } else { x };
    let (c, v) = layout.decode(&x).ok_or_else(|| invalid("invalid initial states"))?;
    Ok((c, v, estimated))
}
