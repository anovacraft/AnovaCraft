//! The state recursion of the innovations state-space models.

use super::{ErrorType, EtsSpec, SeasonalType, SmoothingParameters, StateVector, TrendType};

/// Smoothing coefficients in the form the recursion uses: `beta = 0`
/// without a trend, `gamma = 0` without seasonality, `phi = 1` for an
/// undamped trend.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Coefficients {
    pub alpha: f64,
    pub beta: f64,
    pub gamma: f64,
    pub phi: f64,
}

impl Coefficients {
    pub fn from_parameters(spec: &EtsSpec, p: &SmoothingParameters) -> Self {
        Self {
            alpha: p.alpha,
            beta: if spec.has_trend() { p.beta.unwrap_or(0.0) } else { 0.0 },
            gamma: if spec.has_season() { p.gamma.unwrap_or(0.0) } else { 0.0 },
            phi: if spec.trend == TrendType::DampedAdditive { p.phi.unwrap_or(1.0) } else { 1.0 },
        }
    }

    pub fn to_parameters(self, spec: &EtsSpec) -> SmoothingParameters {
        SmoothingParameters {
            alpha: self.alpha,
            beta: spec.has_trend().then_some(self.beta),
            gamma: spec.has_season().then_some(self.gamma),
            phi: (spec.trend == TrendType::DampedAdditive).then_some(self.phi),
        }
    }
}

/// The state during filtering: `season` is a ring whose element `pos` is
/// the seasonal component of the next observation.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct State {
    pub level: f64,
    pub slope: f64,
    pub season: Vec<f64>,
    pub pos: usize,
    trend: TrendType,
    seasonal: SeasonalType,
}

/// One-step quantities: forecast `μ`, base `B = ℓ + φb` and the seasonal
/// component used.
#[derive(Debug, Clone, Copy)]
pub(crate) struct OneStep {
    pub mu: f64,
    pub base: f64,
    pub season: f64,
}

impl State {
    pub fn from_vector(v: &StateVector, spec: &EtsSpec) -> Self {
        Self {
            level: v.level,
            slope: if spec.has_trend() { v.slope } else { 0.0 },
            season: if spec.has_season() { v.seasonal.clone() } else { Vec::new() },
            pos: 0,
            trend: spec.trend,
            seasonal: spec.seasonal,
        }
    }

    /// The state as a vector, seasonal states in the order of the next `m`
    /// observations.
    pub fn to_vector(&self) -> StateVector {
        let m = self.season.len();
        StateVector {
            level: self.level,
            slope: self.slope,
            seasonal: (0..m).map(|j| self.season[(self.pos + j) % m]).collect(),
        }
    }

    pub fn one_step(&self, c: &Coefficients) -> OneStep {
        let base = match self.trend {
            TrendType::None => self.level,
            TrendType::Additive => self.level + self.slope,
            TrendType::DampedAdditive => self.level + c.phi * self.slope,
        };
        match self.seasonal {
            SeasonalType::None => OneStep { mu: base, base, season: 0.0 },
            SeasonalType::Additive => {
                let s = self.season[self.pos];
                OneStep { mu: base + s, base, season: s }
            }
            SeasonalType::Multiplicative => {
                let s = self.season[self.pos];
                OneStep { mu: base * s, base, season: s }
            }
        }
    }

    /// Correct the state by the one-step error `d = y − μ`.
    pub fn update(&mut self, c: &Coefficients, step: OneStep, d: f64) {
        let scaled = if self.seasonal == SeasonalType::Multiplicative { d / step.season } else { d };
        self.level = step.base + c.alpha * scaled;
        self.slope = match self.trend {
            TrendType::None => 0.0,
            TrendType::Additive => self.slope + c.beta * scaled,
            TrendType::DampedAdditive => c.phi * self.slope + c.beta * scaled,
        };
        match self.seasonal {
            SeasonalType::None => {}
            SeasonalType::Additive => {
                self.season[self.pos] = step.season + c.gamma * d;
            }
            SeasonalType::Multiplicative => {
                self.season[self.pos] = step.season + c.gamma * d / step.base;
            }
        }
        if !self.season.is_empty() {
            self.pos = (self.pos + 1) % self.season.len();
        }
    }

    /// Point forecasts `1..=h` steps ahead (future errors zero).
    pub fn point_forecasts(&self, c: &Coefficients, h: usize) -> Vec<f64> {
        let mut s = self.clone();
        (0..h)
            .map(|_| {
                let step = s.one_step(c);
                s.update(c, step, 0.0);
                step.mu
            })
            .collect()
    }
}

/// Output of a filter pass.
#[derive(Debug, Clone)]
pub(crate) struct FilterOutput {
    pub fitted: Vec<f64>,
    pub innovations: Vec<f64>,
    /// Sum of squared innovations.
    pub sse: f64,
    /// `2 Σ ln μ_t` for multiplicative errors, else 0.
    pub log_mu_twice: f64,
    pub final_state: State,
}

/// Whether the multiplicative parts of a one-step forecast are usable.
fn valid_step(spec: &EtsSpec, step: &OneStep) -> bool {
    if !step.mu.is_finite() {
        return false;
    }
    if spec.error == ErrorType::Multiplicative && step.mu <= 0.0 {
        return false;
    }
    if spec.seasonal == SeasonalType::Multiplicative && (step.base <= 0.0 || step.season <= 0.0) {
        return false;
    }
    true
}

/// Run the recursion over `y` from `state`. `None` when a multiplicative
/// component becomes non-positive or a value overflows.
pub(crate) fn filter(y: &[f64], spec: &EtsSpec, c: &Coefficients, mut state: State) -> Option<FilterOutput> {
    let mut fitted = Vec::with_capacity(y.len());
    let mut innovations = Vec::with_capacity(y.len());
    let mut sse = 0.0;
    let mut log_mu_twice = 0.0;
    for &v in y {
        let step = state.one_step(c);
        if !valid_step(spec, &step) {
            return None;
        }
        let d = v - step.mu;
        let e = match spec.error {
            ErrorType::Additive => d,
            ErrorType::Multiplicative => {
                log_mu_twice += 2.0 * step.mu.ln();
                d / step.mu
            }
        };
        sse += e * e;
        fitted.push(step.mu);
        innovations.push(e);
        state.update(c, step, d);
    }
    if !sse.is_finite() {
        return None;
    }
    Some(FilterOutput { fitted, innovations, sse, log_mu_twice, final_state: state })
}

/// The concentrated criterion `n ln SSE + 2 Σ ln μ_t` (+∞ where the
/// recursion breaks down), which maximum likelihood minimises.
pub(crate) fn criterion(y: &[f64], spec: &EtsSpec, c: &Coefficients, mut state: State) -> f64 {
    let mut sse = 0.0;
    let mut log_mu_twice = 0.0;
    for &v in y {
        let step = state.one_step(c);
        if !valid_step(spec, &step) {
            return f64::INFINITY;
        }
        let d = v - step.mu;
        let e = match spec.error {
            ErrorType::Additive => d,
            ErrorType::Multiplicative => {
                log_mu_twice += 2.0 * step.mu.ln();
                d / step.mu
            }
        };
        sse += e * e;
        state.update(c, step, d);
    }
    let value = y.len() as f64 * sse.max(f64::MIN_POSITIVE).ln() + log_mu_twice;
    if value.is_finite() { value } else { f64::INFINITY }
}
