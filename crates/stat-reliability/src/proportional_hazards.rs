//! Proportional hazards (Cox) regression for right-censored times, with
//! optional strata.
//!
//! # Model
//! The hazard of a unit with covariate row `x` in stratum `s` is
//! `h(t | x) = h₀ₛ(t) exp(xᵀβ)`: covariates multiply an unspecified baseline
//! hazard by the **hazard ratio** `exp(xᵀβ)` (Cox 1972, "Regression models
//! and life-tables", *JRSS B* 34, 187–220). `β` is estimated by maximizing
//! the partial likelihood, which does not involve `h₀`. With `dⱼ` events at
//! the distinct event time `tⱼ`, `Dⱼ` the units failing there, `Rⱼ` the units
//! at risk (time ≥ `tⱼ`, same stratum), `S⁽⁰⁾ = Σ_R exp(xᵀβ)`,
//! `S⁽⁰⁾_D = Σ_D exp(xᵀβ)` and likewise `S⁽¹⁾`, `S⁽²⁾` with weights `x`,
//! `xxᵀ`, the log partial likelihood is
//!
//! ```text
//! Efron (1977):   ℓ(β) = Σⱼ [ Σ_{Dⱼ} xᵀβ − Σ_{l=0}^{dⱼ−1} ln(S⁽⁰⁾ − (l/dⱼ) S⁽⁰⁾_D) ]
//! Breslow (1974): ℓ(β) = Σⱼ [ Σ_{Dⱼ} xᵀβ − dⱼ ln S⁽⁰⁾ ]
//! ```
//!
//! (Efron, "The efficiency of Cox's likelihood function for censored data",
//! *JASA* 72, 557–565; Breslow, "Covariance analysis of censored survival
//! data", *Biometrics* 30, 89–99). The two agree without tied event times;
//! Efron's is the default because it is much closer to the exact discrete
//! likelihood when ties are common (Therneau & Grambsch, *Modeling Survival
//! Data*, 2000, §3.3). Its score and information follow by differentiating
//! each term: `U = Σⱼ [Σ_D x − Σₗ aₗ/φₗ]` and
//! `I = Σⱼ Σₗ [(S⁽²⁾ − cₗS⁽²⁾_D)/φₗ − aₗaₗᵀ/φₗ²]` with `cₗ = l/dⱼ`,
//! `φₗ = S⁽⁰⁾ − cₗS⁽⁰⁾_D`, `aₗ = S⁽¹⁾ − cₗS⁽¹⁾_D` (Breslow: `cₗ = 0`).
//! Covariates are centred at their means for the computation (which leaves
//! `β` unchanged) and the linear predictors shifted by their maximum, so the
//! exponentials never overflow. Strata have their own risk sets.
//!
//! # Estimation and inference
//! Newton–Raphson from `β = 0` with step halving; the covariance is
//! `I(β̂)⁻¹`. Each coefficient has a Wald interval `β̂ ± z·SE` and a
//! **profile-likelihood** interval `{b : 2[ℓ(β̂) − max_{β_j = b} ℓ(β)] ≤ χ²₁}`
//! (the other coefficients re-maximized at each `b`); hazard ratios are their
//! exponentials. The global null hypothesis `β = 0` has the likelihood-ratio
//! test `2[ℓ(β̂) − ℓ(0)]`, the Wald test `β̂ᵀ I(β̂) β̂` and the score
//! (log-rank-type) test `U(0)ᵀ I(0)⁻¹ U(0)`, each χ² with `p` degrees of
//! freedom (Klein & Moeschberger, *Survival Analysis*, 2nd ed., 2003, §8.3).
//! [`ProportionalHazardsFit::drop_columns_test`] refits without a set of
//! columns for the likelihood-ratio test of an effect.
//!
//! # Baseline and predicted survival
//! Breslow's estimator of the cumulative baseline hazard, at the covariate
//! means, is `Ĥ₀(t) = Σ_{tⱼ ≤ t} dⱼ / S⁽⁰⁾(tⱼ; β̂)` (per stratum), and the
//! survival of a unit with covariates `x` is
//! `Ŝ(t | x) = exp(−Ĥ₀(t) exp((x − x̄)ᵀβ̂))`. Its cumulative hazard has the
//! variance (Tsiatis 1981; Klein & Moeschberger §8.8)
//!
//! ```text
//! Var Ĥ(t|x) = exp(2zᵀβ̂) [ Σ_{tⱼ≤t} dⱼ/S⁽⁰⁾(tⱼ)² + qᵀ I(β̂)⁻¹ q ],
//! q = Σ_{tⱼ≤t} (z − S⁽¹⁾/S⁽⁰⁾) dⱼ/S⁽⁰⁾,   z = x − x̄,
//! ```
//!
//! and the pointwise limits are formed on the log cumulative-hazard scale,
//! `Ĥ exp(±z_{α/2} SE/Ĥ)`, then mapped to survival (Klein & Moeschberger
//! §4.3 for the transformation).
//!
//! # Checking proportional hazards
//! The **Schoenfeld residual** of a unit failing at `tⱼ` is `x − x̄(tⱼ)`, with
//! `x̄(tⱼ)` the risk-set mean `S⁽¹⁾/S⁽⁰⁾` (Breslow) or its Efron average
//! `(1/dⱼ) Σₗ aₗ/φₗ` shared by the tied failures (Schoenfeld 1982,
//! *Biometrika* 69, 239–241). Scaled residuals
//! `s*ₖ = β̂ + D I(β̂)⁻¹ rₖ` (`D` events) estimate a time-varying coefficient
//! `β(tₖ)`. With `gₖ = g(tₖ)` a transform of time, the Grambsch & Therneau
//! (1994, "Proportional hazards tests and diagnostics based on weighted
//! residuals", *Biometrika* 81, 515–526) score tests of `β_j(t) = β_j + θ_j g(t)`
//! are, with `u = Σₖ (gₖ − ḡ) rₖ` and `V = I(β̂)⁻¹`,
//!
//! ```text
//! per covariate:  T_j = D (V u)_j² / (V_jj Σ(gₖ − ḡ)²)   ~ χ²₁
//! global:         T   = D uᵀ V u / Σ(gₖ − ḡ)²           ~ χ²_p
//! ```
//!
//! (the approximation using `D·V` for the variance of the scaled residuals,
//! as in their paper). `g` is `1 − KM(t)` (the default), the rank of `t`
//! among the failure times, `t` itself, or `ln t`.
//!
//! # Residuals
//! Martingale residuals `Mᵢ = δᵢ − Ĥ₀ₛ(tᵢ) exp((xᵢ − x̄)ᵀβ̂)` and deviance
//! residuals `sign(Mᵢ) √(−2[Mᵢ + δᵢ ln(δᵢ − Mᵢ)])` (Therneau, Grambsch &
//! Fleming 1990, *Biometrika* 77, 147–160).
//!
//! # Failure modes
//! [`StatError::InvalidArgument`] for mismatched lengths, non-finite values,
//! no events, or collinear covariates (singular information);
//! [`StatError::ConvergenceFailure`] when the estimates diverge, which
//! happens when a covariate separates the failures from the survivors
//! (monotone likelihood: no finite maximum).

use stat_core::{Matrix, Result, StatError};

use crate::support::ChiSquareTest;
use crate::support::{find_crossing, normal_quantile, normal_two_sided, quadratic, spd_inverse, spd_solve};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

const MAX_ITERATIONS: usize = 200;

/// How tied event times enter the partial likelihood.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TieMethod {
    /// Efron's (1977) approximation (the default).
    #[default]
    Efron,
    /// Breslow's (1974) approximation.
    Breslow,
}

impl TieMethod {
    /// Key in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Efron => "efron",
            Self::Breslow => "breslow",
        }
    }

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Efron => "Efron",
            Self::Breslow => "Breslow",
        }
    }

    /// The method with key `key`.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        [Self::Efron, Self::Breslow].into_iter().find(|m| m.key() == key)
    }
}

/// The transform of time in the test of proportional hazards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TimeTransform {
    /// `1 − KM(t)`, the Kaplan–Meier estimate of the failure fraction.
    #[default]
    KaplanMeier,
    /// Rank of the failure time (ties share their average rank).
    Rank,
    /// Time itself.
    Identity,
    /// Natural log of time.
    Log,
}

impl TimeTransform {
    /// Every transform.
    pub const ALL: [Self; 4] = [Self::KaplanMeier, Self::Rank, Self::Identity, Self::Log];

    /// Key in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::KaplanMeier => "km",
            Self::Rank => "rank",
            Self::Identity => "identity",
            Self::Log => "log",
        }
    }

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::KaplanMeier => "1 − Kaplan–Meier",
            Self::Rank => "Rank of time",
            Self::Identity => "Time",
            Self::Log => "Log time",
        }
    }

    /// The transform with key `key`.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }
}

/// Right-censored times with covariates and optional strata.
#[derive(Debug, Clone, PartialEq)]
pub struct ProportionalHazardsData {
    time: Vec<f64>,
    event: Vec<bool>,
    /// Covariate rows, centred at `means`.
    xc: Vec<Vec<f64>>,
    means: Vec<f64>,
    strata: Vec<usize>,
    n_strata: usize,
}

impl ProportionalHazardsData {
    /// Times, event indicators (`true` = failure, `false` = censored), the
    /// covariate matrix (`n × p`, no intercept column) and optional stratum
    /// codes (`0..k`).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for mismatched lengths, non-finite
    /// times or covariates, no covariates, no failures, or a stratum code
    /// whose stratum is empty.
    pub fn new(time: Vec<f64>, event: Vec<bool>, x: &Matrix, strata: Option<Vec<usize>>) -> Result<Self> {
        let n = time.len();
        if event.len() != n || x.nrows() != n {
            return Err(StatError::DimensionMismatch {
                context: "times, events and covariate rows",
                expected: n,
                found: x.nrows().min(event.len()),
            });
        }
        if x.ncols() == 0 {
            return Err(invalid("a proportional hazards model needs at least one covariate"));
        }
        if time.iter().any(|t| !t.is_finite()) || !x.is_finite() {
            return Err(StatError::NonFiniteInput { context: "proportional hazards data" });
        }
        if !event.iter().any(|&e| e) {
            return Err(invalid("there are no failures (every time is censored)"));
        }
        let strata = strata.unwrap_or_else(|| vec![0; n]);
        if strata.len() != n {
            return Err(StatError::DimensionMismatch { context: "strata", expected: n, found: strata.len() });
        }
        let n_strata = strata.iter().copied().max().map_or(1, |m| m + 1);
        if (0..n_strata).any(|s| !strata.contains(&s)) {
            return Err(invalid("stratum codes must be 0, 1, …, k − 1 with every stratum used"));
        }
        let p = x.ncols();
        let means: Vec<f64> = (0..p).map(|j| x.col(j).iter().sum::<f64>() / n as f64).collect();
        let xc = (0..n).map(|i| (0..p).map(|j| x.col(j)[i] - means[j]).collect()).collect();
        Ok(Self { time, event, xc, means, strata, n_strata })
    }

    /// Number of units.
    #[must_use]
    pub fn n(&self) -> usize {
        self.time.len()
    }

    /// Number of failures.
    #[must_use]
    pub fn n_events(&self) -> usize {
        self.event.iter().filter(|&&e| e).count()
    }

    /// Number of covariates.
    #[must_use]
    pub fn n_covariates(&self) -> usize {
        self.means.len()
    }

    /// Number of strata.
    #[must_use]
    pub fn n_strata(&self) -> usize {
        self.n_strata
    }

    /// Covariate means (the centre of the baseline).
    #[must_use]
    pub fn means(&self) -> &[f64] {
        &self.means
    }

    /// Times.
    #[must_use]
    pub fn times(&self) -> &[f64] {
        &self.time
    }

    /// Event indicators.
    #[must_use]
    pub fn events(&self) -> &[bool] {
        &self.event
    }

    /// Stratum of each unit.
    #[must_use]
    pub fn strata(&self) -> &[usize] {
        &self.strata
    }
}

/// Settings of a proportional hazards fit.
#[derive(Debug, Clone, PartialEq)]
pub struct ProportionalHazardsOptions {
    /// Treatment of tied event times.
    pub ties: TieMethod,
    /// Confidence level of the intervals.
    pub level: f64,
    /// Covariate names (default `x1`, `x2`, …).
    pub names: Vec<String>,
    /// Compute profile-likelihood intervals (one constrained refit per end).
    pub profile_intervals: bool,
}

impl Default for ProportionalHazardsOptions {
    fn default() -> Self {
        Self { ties: TieMethod::Efron, level: 0.95, names: Vec::new(), profile_intervals: true }
    }
}

/// One coefficient of the fit.
#[derive(Debug, Clone, PartialEq)]
pub struct HazardCoefficient {
    /// Covariate name.
    pub name: String,
    /// `β̂`.
    pub estimate: f64,
    /// Standard error from `I(β̂)⁻¹`.
    pub std_error: f64,
    /// Wald `z = β̂/SE`.
    pub z: f64,
    /// Two-sided p-value of `z`.
    pub p_value: f64,
    /// Wald interval for `β`.
    pub wald: (f64, f64),
    /// Profile-likelihood interval for `β` (an end is infinite when the
    /// likelihood does not fall far enough; `None` when not computed).
    pub profile: Option<(f64, f64)>,
}

impl HazardCoefficient {
    /// Hazard ratio `exp(β̂)` for a one-unit increase.
    #[must_use]
    pub fn hazard_ratio(&self) -> f64 {
        self.estimate.exp()
    }
}

/// One distinct failure time of the Breslow baseline (one stratum).
#[derive(Debug, Clone, PartialEq)]
pub struct BaselineStep {
    /// Failure time.
    pub time: f64,
    /// Units at risk just before it.
    pub at_risk: usize,
    /// Failures at it.
    pub events: usize,
    /// Cumulative baseline hazard `Ĥ₀(t)` at the covariate means.
    pub cumulative_hazard: f64,
    /// `Σ dⱼ / S⁽⁰⁾²` up to `t` (variance term).
    q1: f64,
    /// `Σ (S⁽¹⁾/S⁽⁰⁾) dⱼ / S⁽⁰⁾` up to `t` (variance term).
    a: Vec<f64>,
}

impl BaselineStep {
    /// Baseline survival `exp(−Ĥ₀(t))` at the covariate means.
    #[must_use]
    pub fn survival(&self) -> f64 {
        (-self.cumulative_hazard).exp()
    }
}

/// Predicted survival at one time, with pointwise limits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PredictedSurvival {
    /// Time.
    pub time: f64,
    /// Cumulative hazard `Ĥ(t | x)`.
    pub cumulative_hazard: f64,
    /// Standard error of `Ĥ(t | x)`.
    pub std_error: f64,
    /// Survival `exp(−Ĥ)`.
    pub survival: f64,
    /// Lower limit of the survival.
    pub lower: f64,
    /// Upper limit of the survival.
    pub upper: f64,
}

/// The Schoenfeld residual of one failure.
#[derive(Debug, Clone, PartialEq)]
pub struct SchoenfeldResidual {
    /// Unit (index into the data).
    pub unit: usize,
    /// Failure time.
    pub time: f64,
    /// `x − x̄(t)`, one value per covariate.
    pub residual: Vec<f64>,
    /// Scaled residual `β̂ + D I⁻¹ r` (an estimate of `β(t)`).
    pub scaled: Vec<f64>,
}

/// Grambsch–Therneau tests of proportional hazards.
#[derive(Debug, Clone, PartialEq)]
pub struct ProportionalityTest {
    /// Transform of time used.
    pub transform: TimeTransform,
    /// Transformed time of each Schoenfeld residual (in time order).
    pub transformed_times: Vec<f64>,
    /// One test per covariate (χ², 1 df).
    pub covariates: Vec<ChiSquareTest>,
    /// Global test (χ², `p` df).
    pub global: ChiSquareTest,
}

/// Evaluation of the log partial likelihood.
#[derive(Debug, Clone)]
struct Evaluation {
    log_likelihood: f64,
    score: Vec<f64>,
    information: Vec<Vec<f64>>,
}

/// Units sharing one time in one stratum (descending time order).
#[derive(Debug, Clone)]
struct TimeGroup {
    time: f64,
    members: Vec<usize>,
    failures: Vec<usize>,
}

/// The likelihood machinery over a fixed dataset.
#[derive(Debug, Clone)]
struct PartialLikelihood {
    groups: Vec<Vec<TimeGroup>>,
    ties: TieMethod,
}

impl PartialLikelihood {
    fn new(data: &ProportionalHazardsData, ties: TieMethod) -> Self {
        let mut groups = Vec::with_capacity(data.n_strata);
        for s in 0..data.n_strata {
            let mut idx: Vec<usize> = (0..data.n()).filter(|&i| data.strata[i] == s).collect();
            idx.sort_by(|&a, &b| data.time[b].total_cmp(&data.time[a]));
            let mut out: Vec<TimeGroup> = Vec::new();
            for i in idx {
                match out.last_mut() {
                    Some(g) if g.time == data.time[i] => {
                        g.members.push(i);
                        if data.event[i] {
                            g.failures.push(i);
                        }
                    }
                    _ => out.push(TimeGroup {
                        time: data.time[i],
                        members: vec![i],
                        failures: if data.event[i] { vec![i] } else { Vec::new() },
                    }),
                }
            }
            groups.push(out);
        }
        Self { groups, ties }
    }

    /// Linear predictors `offset + x_c[cols]·β` and their maximum.
    fn eta(data: &ProportionalHazardsData, cols: &[usize], beta: &[f64], offset: Option<&[f64]>) -> (Vec<f64>, f64) {
        let eta: Vec<f64> = (0..data.n())
            .map(|i| offset.map_or(0.0, |o| o[i]) + cols.iter().zip(beta).map(|(&c, b)| data.xc[i][c] * b).sum::<f64>())
            .collect();
        let shift = eta.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        (eta, if shift.is_finite() { shift } else { 0.0 })
    }

    fn evaluate(
        &self,
        data: &ProportionalHazardsData,
        cols: &[usize],
        beta: &[f64],
        offset: Option<&[f64]>,
    ) -> Evaluation {
        let k = cols.len();
        let (eta, shift) = Self::eta(data, cols, beta, offset);
        let r: Vec<f64> = eta.iter().map(|e| (e - shift).exp()).collect();
        let mut ll = 0.0;
        let mut score = vec![0.0; k];
        let mut info = vec![vec![0.0; k]; k];
        for stratum in &self.groups {
            let mut s0 = 0.0;
            let mut s1 = vec![0.0; k];
            let mut s2 = vec![vec![0.0; k]; k];
            for g in stratum {
                for &i in &g.members {
                    s0 += r[i];
                    for a in 0..k {
                        let xa = data.xc[i][cols[a]];
                        s1[a] += r[i] * xa;
                        for b in 0..=a {
                            s2[a][b] += r[i] * xa * data.xc[i][cols[b]];
                        }
                    }
                }
                if g.failures.is_empty() {
                    continue;
                }
                let d = g.failures.len();
                let mut d0 = 0.0;
                let mut d1 = vec![0.0; k];
                let mut d2 = vec![vec![0.0; k]; k];
                for &i in &g.failures {
                    ll += eta[i];
                    d0 += r[i];
                    for a in 0..k {
                        let xa = data.xc[i][cols[a]];
                        score[a] += xa;
                        d1[a] += r[i] * xa;
                        for b in 0..=a {
                            d2[a][b] += r[i] * xa * data.xc[i][cols[b]];
                        }
                    }
                }
                for l in 0..d {
                    let c = match self.ties {
                        TieMethod::Efron => l as f64 / d as f64,
                        TieMethod::Breslow => 0.0,
                    };
                    let phi = s0 - c * d0;
                    ll -= phi.ln() + shift;
                    let av: Vec<f64> = (0..k).map(|a| s1[a] - c * d1[a]).collect();
                    for a in 0..k {
                        score[a] -= av[a] / phi;
                        for b in 0..=a {
                            info[a][b] += (s2[a][b] - c * d2[a][b]) / phi - av[a] * av[b] / (phi * phi);
                        }
                    }
                }
            }
        }
        for a in 0..k {
            for b in 0..a {
                info[b][a] = info[a][b];
            }
        }
        Evaluation { log_likelihood: ll, score, information: info }
    }

    /// Maximize over the coefficients of `cols` (Newton–Raphson with step
    /// halving) from `start`.
    fn maximize(
        &self,
        data: &ProportionalHazardsData,
        cols: &[usize],
        offset: Option<&[f64]>,
        start: &[f64],
    ) -> Result<(Vec<f64>, Evaluation, usize)> {
        let mut beta = start.to_vec();
        let mut ev = self.evaluate(data, cols, &beta, offset);
        if cols.is_empty() {
            return Ok((beta, ev, 0));
        }
        let sd: Vec<f64> = cols
            .iter()
            .map(|&c| {
                let v = data.xc.iter().map(|row| row[c] * row[c]).sum::<f64>() / data.n() as f64;
                v.sqrt().max(1e-300)
            })
            .collect();
        for iteration in 1..=MAX_ITERATIONS {
            let step = spd_solve(&ev.information, &ev.score).ok_or_else(|| {
                invalid("the covariates are collinear or constant among the units at risk (singular information)")
            })?;
            let mut t = 1.0;
            let mut accepted = None;
            for _ in 0..60 {
                let cand: Vec<f64> = beta.iter().zip(&step).map(|(b, s)| b + t * s).collect();
                let ec = self.evaluate(data, cols, &cand, offset);
                if ec.log_likelihood.is_finite()
                    && ec.log_likelihood >= ev.log_likelihood - 1e-12 * (1.0 + ev.log_likelihood.abs())
                {
                    accepted = Some((cand, ec));
                    break;
                }
                t *= 0.5;
            }
            let Some((cand, ec)) = accepted else {
                // No step improves the likelihood: at the maximum to rounding.
                return Ok((beta, ev, iteration));
            };
            let small = step.iter().zip(&cand).all(|(s, b)| (t * s).abs() <= 1e-10 * (1.0 + b.abs()));
            let gain = ec.log_likelihood - ev.log_likelihood;
            beta = cand;
            ev = ec;
            if beta.iter().zip(&sd).any(|(b, s)| (b * s).abs() > 60.0 || !b.is_finite()) {
                return Err(StatError::ConvergenceFailure {
                    routine: "proportional hazards (estimates diverge: a covariate separates failures from survivors)",
                    iterations: iteration,
                });
            }
            if small && gain.abs() <= 1e-9 * (1.0 + ev.log_likelihood.abs()) {
                return Ok((beta, ev, iteration));
            }
        }
        Err(StatError::ConvergenceFailure { routine: "proportional hazards", iterations: MAX_ITERATIONS })
    }
}

/// A fitted proportional hazards model.
#[derive(Debug, Clone)]
pub struct ProportionalHazardsFit {
    data: ProportionalHazardsData,
    likelihood: PartialLikelihood,
    options: ProportionalHazardsOptions,
    /// Coefficients, one per covariate.
    pub coefficients: Vec<HazardCoefficient>,
    /// Covariance `I(β̂)⁻¹`.
    pub covariance: Vec<Vec<f64>>,
    /// Log partial likelihood at `β̂`.
    pub log_likelihood: f64,
    /// Log partial likelihood at `β = 0`.
    pub null_log_likelihood: f64,
    /// Likelihood-ratio test of `β = 0`.
    pub likelihood_ratio: ChiSquareTest,
    /// Wald test of `β = 0`.
    pub wald: ChiSquareTest,
    /// Score test of `β = 0`.
    pub score: ChiSquareTest,
    /// Newton iterations used.
    pub iterations: usize,
    /// Breslow baseline per stratum.
    pub baseline: Vec<Vec<BaselineStep>>,
}

/// Fit the proportional hazards model to `data`.
///
/// # Errors
/// See the module documentation: collinear covariates, or no finite maximum.
pub fn fit_proportional_hazards(
    data: &ProportionalHazardsData,
    options: &ProportionalHazardsOptions,
) -> Result<ProportionalHazardsFit> {
    if !(options.level > 0.0 && options.level < 1.0) {
        return Err(invalid("the confidence level must be between 0 and 1"));
    }
    let p = data.n_covariates();
    let cols: Vec<usize> = (0..p).collect();
    let likelihood = PartialLikelihood::new(data, options.ties);
    let null = likelihood.evaluate(data, &cols, &vec![0.0; p], None);
    let (beta, ev, iterations) = likelihood.maximize(data, &cols, None, &vec![0.0; p])?;
    let covariance = spd_inverse(&ev.information)
        .ok_or_else(|| invalid("the information matrix is singular at the estimates (collinear covariates)"))?;
    let z = normal_quantile(0.5 + options.level / 2.0);
    let names: Vec<String> =
        (0..p).map(|j| options.names.get(j).cloned().unwrap_or_else(|| format!("x{}", j + 1))).collect();
    let mut coefficients: Vec<HazardCoefficient> = (0..p)
        .map(|j| {
            let se = covariance[j][j].max(0.0).sqrt();
            let zj = beta[j] / se;
            HazardCoefficient {
                name: names[j].clone(),
                estimate: beta[j],
                std_error: se,
                z: zj,
                p_value: normal_two_sided(zj),
                wald: (beta[j] - z * se, beta[j] + z * se),
                profile: None,
            }
        })
        .collect();
    let lr = ChiSquareTest::new(2.0 * (ev.log_likelihood - null.log_likelihood), p);
    let wald = ChiSquareTest::new(quadratic(&ev.information, &beta, &beta), p);
    let score = match spd_inverse(&null.information) {
        Some(inv0) => ChiSquareTest::new(quadratic(&inv0, &null.score, &null.score), p),
        None => ChiSquareTest { statistic: f64::NAN, df: p, p_value: f64::NAN },
    };
    let mut fit = ProportionalHazardsFit {
        data: data.clone(),
        likelihood,
        options: options.clone(),
        coefficients: Vec::new(),
        covariance,
        log_likelihood: ev.log_likelihood,
        null_log_likelihood: null.log_likelihood,
        likelihood_ratio: lr,
        wald,
        score,
        iterations,
        baseline: Vec::new(),
    };
    fit.baseline = fit.compute_baseline(&beta);
    if options.profile_intervals {
        for j in 0..p {
            coefficients[j].profile = Some(fit.profile_interval(j, &beta, coefficients[j].std_error));
        }
    }
    fit.coefficients = coefficients;
    Ok(fit)
}

impl ProportionalHazardsFit {
    /// The data fitted.
    #[must_use]
    pub fn data(&self) -> &ProportionalHazardsData {
        &self.data
    }

    /// The options used.
    #[must_use]
    pub fn options(&self) -> &ProportionalHazardsOptions {
        &self.options
    }

    /// `β̂`.
    #[must_use]
    pub fn beta(&self) -> Vec<f64> {
        self.coefficients.iter().map(|c| c.estimate).collect()
    }

    /// Akaike's criterion on the partial likelihood, `−2ℓ + 2p`.
    #[must_use]
    pub fn aic(&self) -> f64 {
        -2.0 * self.log_likelihood + 2.0 * self.coefficients.len() as f64
    }

    /// Profile-likelihood interval of coefficient `j`.
    fn profile_interval(&self, j: usize, beta: &[f64], se: f64) -> (f64, f64) {
        let crit = stat_core::ChiSquared::new(1.0)
            .and_then(|c| stat_core::ContinuousDistribution::quantile(&c, self.options.level))
            .unwrap_or(f64::NAN);
        let p = beta.len();
        let others: Vec<usize> = (0..p).filter(|&k| k != j).collect();
        let start: Vec<f64> = others.iter().map(|&k| beta[k]).collect();
        let warm = std::cell::RefCell::new(start.clone());
        let g = |b: f64| -> Option<f64> {
            let offset: Vec<f64> = self.data.xc.iter().map(|row| b * row[j]).collect();
            let s = warm.borrow().clone();
            let (bb, ev, _) = self.likelihood.maximize(&self.data, &others, Some(&offset), &s).ok()?;
            *warm.borrow_mut() = bb;
            Some(2.0 * (self.log_likelihood - ev.log_likelihood) - crit)
        };
        let step = if se.is_finite() && se > 0.0 { se } else { 0.1 };
        let lower = find_crossing(&g, beta[j], -step);
        *warm.borrow_mut() = start;
        let upper = find_crossing(&g, beta[j], step);
        (lower.unwrap_or(f64::NEG_INFINITY), upper.unwrap_or(f64::INFINITY))
    }

    /// Likelihood-ratio test of dropping the covariates `drop` (a refit
    /// without them; the null log-likelihood when none remain).
    ///
    /// # Errors
    /// If the reduced model cannot be fitted.
    pub fn drop_columns_test(&self, drop: &[usize]) -> Result<ChiSquareTest> {
        let p = self.coefficients.len();
        let keep: Vec<usize> = (0..p).filter(|j| !drop.contains(j)).collect();
        let df = p - keep.len();
        let reduced = if keep.is_empty() {
            self.null_log_likelihood
        } else {
            let start: Vec<f64> = keep.iter().map(|&k| self.coefficients[k].estimate).collect();
            self.likelihood.maximize(&self.data, &keep, None, &start)?.1.log_likelihood
        };
        Ok(ChiSquareTest::new(2.0 * (self.log_likelihood - reduced), df))
    }

    fn compute_baseline(&self, beta: &[f64]) -> Vec<Vec<BaselineStep>> {
        let p = beta.len();
        let cols: Vec<usize> = (0..p).collect();
        let (eta, _) = PartialLikelihood::eta(&self.data, &cols, beta, None);
        let r: Vec<f64> = eta.iter().map(|e| e.exp()).collect();
        self.likelihood
            .groups
            .iter()
            .map(|stratum| {
                // Risk-set sums at each failure time (descending), then cumulate ascending.
                let mut s0 = 0.0;
                let mut s1 = vec![0.0; p];
                let mut at_risk = 0;
                let mut steps = Vec::new();
                for g in stratum {
                    for &i in &g.members {
                        s0 += r[i];
                        at_risk += 1;
                        for (a, v) in s1.iter_mut().enumerate() {
                            *v += r[i] * self.data.xc[i][a];
                        }
                    }
                    if !g.failures.is_empty() {
                        steps.push((g.time, at_risk, g.failures.len(), s0, s1.clone()));
                    }
                }
                steps.reverse();
                let mut h = 0.0;
                let mut q1 = 0.0;
                let mut a = vec![0.0; p];
                steps
                    .into_iter()
                    .map(|(time, at_risk, d, s0, s1)| {
                        let d = d as f64;
                        h += d / s0;
                        q1 += d / (s0 * s0);
                        for (av, s) in a.iter_mut().zip(&s1) {
                            *av += s / s0 * d / s0;
                        }
                        BaselineStep { time, at_risk, events: d as usize, cumulative_hazard: h, q1, a: a.clone() }
                    })
                    .collect()
            })
            .collect()
    }

    /// Covariates centred at the data means.
    fn centred(&self, x: &[f64]) -> Vec<f64> {
        x.iter().zip(&self.data.means).map(|(v, m)| v - m).collect()
    }

    /// Linear predictor `(x − x̄)ᵀβ̂` of a covariate row.
    #[must_use]
    pub fn linear_predictor(&self, x: &[f64]) -> f64 {
        self.centred(x).iter().zip(&self.coefficients).map(|(z, c)| z * c.estimate).sum()
    }

    /// Hazard ratio of covariate row `x` against `reference`, with Wald
    /// limits `exp((x − r)ᵀβ̂ ± z·SE)`.
    #[must_use]
    pub fn hazard_ratio(&self, x: &[f64], reference: &[f64], level: f64) -> (f64, f64, f64) {
        let d: Vec<f64> = x.iter().zip(reference).map(|(a, b)| a - b).collect();
        let lp: f64 = d.iter().zip(&self.coefficients).map(|(z, c)| z * c.estimate).sum();
        let se = quadratic(&self.covariance, &d, &d).max(0.0).sqrt();
        let z = normal_quantile(0.5 + level / 2.0);
        (lp.exp(), (lp - z * se).exp(), (lp + z * se).exp())
    }

    fn predicted(&self, step: Option<&BaselineStep>, z: &[f64], lp: f64, time: f64, crit: f64) -> PredictedSurvival {
        let Some(step) = step else {
            return PredictedSurvival {
                time,
                cumulative_hazard: 0.0,
                std_error: 0.0,
                survival: 1.0,
                lower: 1.0,
                upper: 1.0,
            };
        };
        let scale = lp.exp();
        let h = step.cumulative_hazard * scale;
        let q: Vec<f64> = z.iter().zip(&step.a).map(|(zv, av)| zv * step.cumulative_hazard - av).collect();
        let var = scale * scale * (step.q1 + quadratic(&self.covariance, &q, &q));
        let se = var.max(0.0).sqrt();
        let (lower, upper) = if h > 0.0 {
            let f = (crit * se / h).exp();
            ((-h * f).exp(), (-h / f).exp())
        } else {
            (1.0, 1.0)
        };
        PredictedSurvival { time, cumulative_hazard: h, std_error: se, survival: (-h).exp(), lower, upper }
    }

    /// Predicted survival of covariate row `x` in `stratum` at every failure
    /// time of that stratum (ascending), with pointwise limits at `level`.
    #[must_use]
    pub fn survival_curve(&self, x: &[f64], stratum: usize, level: f64) -> Vec<PredictedSurvival> {
        let z = self.centred(x);
        let lp = self.linear_predictor(x);
        let crit = normal_quantile(0.5 + level / 2.0);
        self.baseline
            .get(stratum)
            .map(|steps| steps.iter().map(|s| self.predicted(Some(s), &z, lp, s.time, crit)).collect())
            .unwrap_or_default()
    }

    /// Predicted survival of covariate row `x` in `stratum` at time `t`.
    #[must_use]
    pub fn survival_at(&self, x: &[f64], stratum: usize, t: f64, level: f64) -> PredictedSurvival {
        let z = self.centred(x);
        let lp = self.linear_predictor(x);
        let crit = normal_quantile(0.5 + level / 2.0);
        let step = self.baseline.get(stratum).and_then(|steps| steps.iter().rev().find(|s| s.time <= t));
        self.predicted(step, &z, lp, t, crit)
    }

    /// Martingale residuals `δᵢ − Ĥ₀ₛ(tᵢ) exp((xᵢ − x̄)ᵀβ̂)`, one per unit.
    #[must_use]
    pub fn martingale_residuals(&self) -> Vec<f64> {
        let beta = self.beta();
        (0..self.data.n())
            .map(|i| {
                let lp: f64 = self.data.xc[i].iter().zip(&beta).map(|(x, b)| x * b).sum();
                let h = self.baseline[self.data.strata[i]]
                    .iter()
                    .rev()
                    .find(|s| s.time <= self.data.time[i])
                    .map_or(0.0, |s| s.cumulative_hazard);
                f64::from(u8::from(self.data.event[i])) - h * lp.exp()
            })
            .collect()
    }

    /// Deviance residuals `sign(M) √(−2[M + δ ln(δ − M)])`, one per unit.
    #[must_use]
    pub fn deviance_residuals(&self) -> Vec<f64> {
        self.martingale_residuals()
            .iter()
            .zip(&self.data.event)
            .map(|(&m, &e)| {
                let d = f64::from(u8::from(e));
                let inner = if e { m + d * (d - m).ln() } else { m };
                m.signum() * (-2.0 * inner).max(0.0).sqrt()
            })
            .collect()
    }

    /// Schoenfeld residuals of every failure, in ascending time order
    /// (failures at one time in data order).
    #[must_use]
    pub fn schoenfeld_residuals(&self) -> Vec<SchoenfeldResidual> {
        let beta = self.beta();
        let p = beta.len();
        let cols: Vec<usize> = (0..p).collect();
        let (eta, shift) = PartialLikelihood::eta(&self.data, &cols, &beta, None);
        let r: Vec<f64> = eta.iter().map(|e| (e - shift).exp()).collect();
        let total = self.data.n_events() as f64;
        let mut out: Vec<SchoenfeldResidual> = Vec::new();
        for stratum in &self.likelihood.groups {
            let mut s0 = 0.0;
            let mut s1 = vec![0.0; p];
            for g in stratum {
                for &i in &g.members {
                    s0 += r[i];
                    for (a, v) in s1.iter_mut().enumerate() {
                        *v += r[i] * self.data.xc[i][a];
                    }
                }
                if g.failures.is_empty() {
                    continue;
                }
                let d = g.failures.len();
                let mut d0 = 0.0;
                let mut d1 = vec![0.0; p];
                for &i in &g.failures {
                    d0 += r[i];
                    for (a, v) in d1.iter_mut().enumerate() {
                        *v += r[i] * self.data.xc[i][a];
                    }
                }
                let mut mean = vec![0.0; p];
                for l in 0..d {
                    let c = match self.options.ties {
                        TieMethod::Efron => l as f64 / d as f64,
                        TieMethod::Breslow => 0.0,
                    };
                    let phi = s0 - c * d0;
                    for a in 0..p {
                        mean[a] += (s1[a] - c * d1[a]) / phi / d as f64;
                    }
                }
                for &i in g.failures.iter().rev() {
                    let residual: Vec<f64> = (0..p).map(|a| self.data.xc[i][a] - mean[a]).collect();
                    let scaled = (0..p)
                        .map(|a| {
                            beta[a] + total * self.covariance[a].iter().zip(&residual).map(|(v, x)| v * x).sum::<f64>()
                        })
                        .collect();
                    out.push(SchoenfeldResidual { unit: i, time: g.time, residual, scaled });
                }
            }
        }
        out.reverse();
        // Stable by time across strata.
        out.sort_by(|a, b| a.time.total_cmp(&b.time));
        out
    }

    /// Grambsch–Therneau tests of proportional hazards with time transform
    /// `transform`.
    #[must_use]
    pub fn proportionality_test(&self, transform: TimeTransform) -> ProportionalityTest {
        let residuals = self.schoenfeld_residuals();
        let p = self.coefficients.len();
        let times: Vec<f64> = residuals.iter().map(|r| r.time).collect();
        let g: Vec<f64> = match transform {
            TimeTransform::Identity => times.clone(),
            TimeTransform::Log => times.iter().map(|t| t.ln()).collect(),
            TimeTransform::Rank => {
                // Average ranks (ties share the mean of their positions).
                let n = times.len();
                let mut ranks = vec![0.0; n];
                let mut i = 0;
                while i < n {
                    let mut j = i;
                    while j + 1 < n && times[j + 1] == times[i] {
                        j += 1;
                    }
                    let avg = (i + j) as f64 / 2.0 + 1.0;
                    for r in &mut ranks[i..=j] {
                        *r = avg;
                    }
                    i = j + 1;
                }
                ranks
            }
            TimeTransform::KaplanMeier => match crate::kaplan_meier::kaplan_meier(&self.data.time, &self.data.event) {
                Ok(km) => times.iter().map(|&t| 1.0 - km.survival_at(t)).collect(),
                Err(_) => times.clone(),
            },
        };
        let total = self.data.n_events() as f64;
        let gbar = g.iter().sum::<f64>() / g.len().max(1) as f64;
        let ss: f64 = g.iter().map(|v| (v - gbar) * (v - gbar)).sum();
        let mut u = vec![0.0; p];
        for (r, gk) in residuals.iter().zip(&g) {
            for a in 0..p {
                u[a] += (gk - gbar) * r.residual[a];
            }
        }
        let vu: Vec<f64> = (0..p).map(|a| self.covariance[a].iter().zip(&u).map(|(v, x)| v * x).sum()).collect();
        let covariates =
            (0..p).map(|a| ChiSquareTest::new(total * vu[a] * vu[a] / (self.covariance[a][a] * ss), 1)).collect();
        let global = ChiSquareTest::new(total * quadratic(&self.covariance, &u, &u) / ss, p);
        ProportionalityTest { transform, transformed_times: g, covariates, global }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Freireich et al. (1963): weeks in remission, 6-MP (x = 0) and placebo
    /// (x = 1); `false` = censored.
    fn gehan() -> (Vec<f64>, Vec<bool>, Vec<f64>) {
        let mp = [
            (6.0, true),
            (6.0, true),
            (6.0, true),
            (6.0, false),
            (7.0, true),
            (9.0, false),
            (10.0, true),
            (10.0, false),
            (11.0, false),
            (13.0, true),
            (16.0, true),
            (17.0, false),
            (19.0, false),
            (20.0, false),
            (22.0, true),
            (23.0, true),
            (25.0, false),
            (32.0, false),
            (32.0, false),
            (34.0, false),
            (35.0, false),
        ];
        let placebo = [
            1.0, 1.0, 2.0, 2.0, 3.0, 4.0, 4.0, 5.0, 5.0, 8.0, 8.0, 8.0, 8.0, 11.0, 11.0, 12.0, 12.0, 15.0, 17.0, 22.0,
            23.0,
        ];
        let mut t = Vec::new();
        let mut e = Vec::new();
        let mut x = Vec::new();
        for (ti, ei) in mp {
            t.push(ti);
            e.push(ei);
            x.push(0.0);
        }
        for ti in placebo {
            t.push(ti);
            e.push(true);
            x.push(1.0);
        }
        (t, e, x)
    }

    fn fit(ties: TieMethod) -> ProportionalHazardsFit {
        let (t, e, x) = gehan();
        let data = ProportionalHazardsData::new(t, e, &Matrix::from_columns(&[x]).unwrap(), None).unwrap();
        fit_proportional_hazards(&data, &ProportionalHazardsOptions { ties, ..Default::default() }).unwrap()
    }

    /// Venables & Ripley, *Modern Applied Statistics with S*, 4th ed., §13.3
    /// (the `gehan` data, from Freireich et al. 1963): Efron β̂ = 1.572
    /// (SE 0.412), Breslow β̂ = 1.509 (SE 0.410).
    #[test]
    fn freireich_published_coefficients() {
        let efron = fit(TieMethod::Efron);
        assert!((efron.coefficients[0].estimate - 1.572).abs() < 5e-4, "{}", efron.coefficients[0].estimate);
        assert!((efron.coefficients[0].std_error - 0.412).abs() < 5e-4);
        let breslow = fit(TieMethod::Breslow);
        assert!((breslow.coefficients[0].estimate - 1.509).abs() < 5e-4, "{}", breslow.coefficients[0].estimate);
        assert!((breslow.coefficients[0].std_error - 0.410).abs() < 5e-4);
        // Same source, Efron ties: likelihood ratio 16.35, Wald 14.53, score 17.25 (1 df).
        assert!((efron.likelihood_ratio.statistic - 16.35).abs() < 5e-3, "{}", efron.likelihood_ratio.statistic);
        assert!((efron.wald.statistic - 14.53).abs() < 5e-3, "{}", efron.wald.statistic);
        assert!((efron.score.statistic - 17.25).abs() < 5e-3, "{}", efron.score.statistic);
        let pr = efron.coefficients[0].profile.unwrap();
        assert!(pr.0 < efron.coefficients[0].estimate && pr.1 > efron.coefficients[0].estimate);
    }

    #[test]
    fn survival_and_residuals_are_consistent() {
        let f = fit(TieMethod::Efron);
        let curve = f.survival_curve(&[1.0], 0, 0.95);
        assert!(curve.windows(2).all(|w| w[1].survival <= w[0].survival));
        assert!(curve.iter().all(|s| s.lower <= s.survival && s.survival <= s.upper));
        // Martingale residuals sum to zero (Breslow baseline).
        let m: f64 = f.martingale_residuals().iter().sum();
        assert!(m.abs() < 1e-9, "{m}");
        // Schoenfeld residuals sum to the score at β̂ (zero) with Breslow ties.
        let b = fit(TieMethod::Breslow);
        let s: f64 = b.schoenfeld_residuals().iter().map(|r| r.residual[0]).sum();
        assert!(s.abs() < 1e-8, "{s}");
        let test = f.proportionality_test(TimeTransform::KaplanMeier);
        assert_eq!(test.covariates.len(), 1);
        assert!((test.global.statistic - test.covariates[0].statistic).abs() < 1e-12);
        let drop = f.drop_columns_test(&[0]).unwrap();
        assert!((drop.statistic - f.likelihood_ratio.statistic).abs() < 1e-9);
    }

    #[test]
    fn errors() {
        let x = Matrix::from_columns(&[vec![0.0, 1.0, 0.0]]).unwrap();
        assert!(ProportionalHazardsData::new(vec![1.0, 2.0, 3.0], vec![false; 3], &x, None).is_err());
        // Perfect separation: every x = 1 unit fails before every x = 0 unit.
        let x = Matrix::from_columns(&[vec![1.0, 1.0, 1.0, 0.0, 0.0, 0.0]]).unwrap();
        let d = ProportionalHazardsData::new(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![true; 6], &x, None).unwrap();
        assert!(fit_proportional_hazards(&d, &ProportionalHazardsOptions::default()).is_err());
    }
}
