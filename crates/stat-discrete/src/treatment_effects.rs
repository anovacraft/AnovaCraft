//! Treatment effects from observational data: propensity scores, covariate
//! balance, nearest-neighbour matching, inverse-probability weighting and
//! augmented (doubly robust) weighting.
//!
//! # Setting
//! Each row has a two-level treatment `tᵢ ∈ {0, 1}`, an outcome `yᵢ` and
//! covariates `xᵢ` measured before treatment. Under no unmeasured
//! confounding and overlap (`0 < e(x) < 1`), the potential-outcome means
//! `μ₁ = E[Y(1)]`, `μ₀ = E[Y(0)]` (or their means among the treated) can be
//! estimated from the propensity score `e(x) = P(t = 1 | x)` (Rosenbaum &
//! Rubin 1983, *Biometrika* 70).
//!
//! # Propensity score
//! A logistic regression of `t` on the covariates ([`fit_glm`], binomial
//! family, logit link), `êᵢ` its fitted probabilities.
//!
//! # Estimands
//! * **ATE**, the average treatment effect over everyone: `E[Y(1)] − E[Y(0)]`.
//! * **ATT**, the average effect among the treated: `E[Y(1) − Y(0) | t = 1]`.
//!
//! Effects are reported as the difference `μ₁ − μ₀`, and for binary and
//! count outcomes also as the ratio `μ₁/μ₀` (risk or rate ratio), and for
//! binary outcomes the odds ratio `[μ₁/(1−μ₁)]/[μ₀/(1−μ₀)]`. Ratios get
//! their intervals on the log scale by the delta method.
//!
//! # Methods
//! * **Inverse-probability weighting** (normalized, or Hájek, form;
//!   Lunceford & Davidian 2004, *Statistics in Medicine* 23, estimator
//!   `Δ̂₂`): ATE weights `t/ê + (1−t)/(1−ê)`, ATT weights `t + (1−t)ê/(1−ê)`,
//!   and `μ̂_g` the weighted mean of the outcome in group `g`. Stabilized
//!   weights multiply the treated weights by `P̂(t = 1)` and the control
//!   weights by `P̂(t = 0)` (ATE), or the control weights by
//!   `P̂(t = 0)/P̂(t = 1)` (ATT); this changes the saved weights, not the
//!   estimate.
//! * **Augmented IPW** (doubly robust; Lunceford & Davidian 2004, `Δ̂_DR`;
//!   Hernán & Robins 2020, *Causal Inference: What If*, §13): outcome models
//!   `m₁(x)`, `m₀(x)` fitted in each group (normal linear, logistic or
//!   Poisson regression for continuous, binary or count outcomes), and for
//!   the ATE `μ̂₁ = n⁻¹ Σ [tᵢyᵢ/êᵢ − (tᵢ − êᵢ) m₁(xᵢ)/êᵢ]`,
//!   `μ̂₀ = n⁻¹ Σ [(1−tᵢ)yᵢ/(1−êᵢ) + (tᵢ − êᵢ) m₀(xᵢ)/(1−êᵢ)]`; for the ATT
//!   `μ̂₁` is the treated mean and
//!   `μ̂₀ = Σ [tᵢ m₀(xᵢ) + (1−tᵢ) êᵢ/(1−êᵢ) (yᵢ − m₀(xᵢ))] / Σ tᵢ`. The
//!   estimate is consistent when either the propensity model or the outcome
//!   models are correct.
//! * **Nearest-neighbour matching** on the logit of the propensity score
//!   (Rosenbaum & Rubin 1985; Austin 2011, *Multivariate Behavioral Research*
//!   46). For the ATT each treated row, in decreasing order of its score, is
//!   given its `k` nearest controls; without replacement every treated row
//!   receives its first match before any receives a second, and a matched
//!   control is not used again. A caliper (in standard deviations of the
//!   logit over all rows) leaves treated rows with no control close enough
//!   unmatched. For the ATE (with replacement only) every row is also
//!   matched to its `k` nearest rows of the other group (Abadie & Imbens
//!   2006, *Econometrica* 74). Matched rows get weights — 1 for each row
//!   matched in its own right and, for a matched row of the other group,
//!   the sum over the rows it serves of `1/(their number of matches)`,
//!   rescaled to sum to the number of such rows — and `μ̂_g` is the
//!   weighted group mean.
//!
//! Trimming (Crump et al. 2009, *Biometrika* 96) leaves out rows whose
//! score lies outside `[α, 1 − α]`; the propensity model is not refitted.
//!
//! # Intervals
//! * **Sandwich** (weighting and doubly robust): the propensity model's
//!   score equations, the outcome models' score equations and the equations
//!   for `μ₁`, `μ₀` are stacked as one M-estimator `Σ ψᵢ(θ) = 0`, and
//!   `cov θ̂ = A⁻¹ B A⁻ᵀ` with `A = Σ ∂ψᵢ/∂θ` (central differences) and
//!   `B = Σ ψᵢψᵢᵀ` (Stefanski & Boos 2002, *The American Statistician* 56;
//!   Lunceford & Davidian 2004 §3). This accounts for estimating the
//!   propensity score.
//! * **Robust, matched data** (matching): the weighted means' sandwich with
//!   matched sets as clusters (without replacement) or rows as clusters
//!   (with replacement), with the small-sample factor
//!   `G/(G−1) · (N−1)/(N−2)` (`G` clusters, `N` matched rows), as
//!   `sandwich::vcovCL(type = "HC1")` on the weighted regression
//!   `y ~ treatment` that matching software recommends (Austin & Small 2014,
//!   *Statistics in Medicine* 33).
//! * **Bootstrap**: rows resampled with replacement within each treatment
//!   group and the whole analysis (propensity model, matching or weights,
//!   outcome models) repeated; percentile intervals, and the replicates'
//!   standard deviation as the standard error.
//!
//! # Covariate balance
//! Standardized mean differences `(x̄₁ − x̄₀)/s` with `s` from the unadjusted
//! sample — `√((s₁² + s₀²)/2)` for the ATE, `s₁` for the ATT, with
//! `s² = p(1−p)` for a 0/1 variable — before and after adjustment (weighted
//! means), and variance ratios `s₁²/s₀²` of continuous variables, with
//! weighted variances `Σw/((Σw)² − Σw²) · Σ w(x − x̄_w)²` (Austin 2009,
//! *Statistics in Medicine* 28; Austin & Stuart 2015, *Statistics in
//! Medicine* 34).
//!
//! # Failure modes
//! Treatment values other than 0/1, a group with fewer than two rows,
//! propensity scores of 0 or 1 (separation) among the rows kept, an outcome
//! outside the range of its kind, no matches within the caliper, and a
//! singular outcome model are errors.

use crate::glm::{Family, GlmFit, GlmOptions, fit_glm};
use stat_core::distributions::{ChiSquared, ContinuousDistribution, Normal};
use stat_core::rng::Rng;
use stat_core::{Matrix, QrDecomposition, Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Kind of outcome, which sets the outcome models and the effect scales.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutcomeKind {
    /// Continuous: normal linear outcome models; differences only.
    Continuous,
    /// 0/1: logistic outcome models; risk difference, risk ratio, odds ratio.
    Binary,
    /// Non-negative counts: Poisson outcome models; rate difference and ratio.
    Count,
}

impl OutcomeKind {
    /// Every kind, in menu order.
    pub const ALL: [Self; 3] = [Self::Continuous, Self::Binary, Self::Count];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Continuous => "Continuous",
            Self::Binary => "Binary (0/1)",
            Self::Count => "Count",
        }
    }

    /// Key used in saved files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Continuous => "continuous",
            Self::Binary => "binary",
            Self::Count => "count",
        }
    }

    /// The kind with key `key`.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.key() == key)
    }

    fn family(self) -> Family {
        match self {
            Self::Continuous => Family::Gaussian,
            Self::Binary => Family::Binomial,
            Self::Count => Family::Poisson,
        }
    }

    /// Canonical inverse link.
    fn mean(self, eta: f64) -> f64 {
        match self {
            Self::Continuous => eta,
            Self::Binary => logistic(eta),
            Self::Count => eta.exp(),
        }
    }

    fn check(self, y: &[f64]) -> Result<()> {
        let ok = |v: f64| match self {
            Self::Continuous => v.is_finite(),
            Self::Binary => v == 0.0 || v == 1.0,
            Self::Count => v.is_finite() && v >= 0.0,
        };
        if y.iter().all(|&v| ok(v)) {
            Ok(())
        } else {
            Err(invalid(match self {
                Self::Continuous => "the outcome must be finite",
                Self::Binary => "a binary outcome must be 0 or 1",
                Self::Count => "a count outcome must be non-negative",
            }))
        }
    }
}

/// Which population the effect averages over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Estimand {
    /// Average treatment effect over all rows.
    Ate,
    /// Average treatment effect among the treated.
    Att,
}

impl Estimand {
    /// Both, in menu order.
    pub const ALL: [Self; 2] = [Self::Ate, Self::Att];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Ate => "Average effect (ATE)",
            Self::Att => "Average effect among the treated (ATT)",
        }
    }

    /// Short name.
    #[must_use]
    pub fn short(self) -> &'static str {
        match self {
            Self::Ate => "ATE",
            Self::Att => "ATT",
        }
    }

    /// Key used in saved files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Ate => "ate",
            Self::Att => "att",
        }
    }

    /// The estimand with key `key`.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.key() == key)
    }
}

/// Nearest-neighbour matching settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MatchingOptions {
    /// Matches per row (1 for pair matching).
    pub ratio: usize,
    /// Whether a row may serve as a match more than once.
    pub replace: bool,
    /// Caliper in standard deviations of the logit of the score; `None`
    /// for no caliper.
    pub caliper: Option<f64>,
}

impl Default for MatchingOptions {
    /// 1:1 matching without replacement, caliper 0.2 SD of the logit
    /// (Austin 2011).
    fn default() -> Self {
        Self { ratio: 1, replace: false, caliper: Some(0.2) }
    }
}

/// How the effect is estimated.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EffectMethod {
    /// Nearest-neighbour matching on the logit of the score.
    Matching(MatchingOptions),
    /// Inverse-probability weighting.
    Weighting,
    /// Augmented inverse-probability weighting with outcome models.
    DoublyRobust,
}

impl EffectMethod {
    /// Display name.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Matching(_) => "Nearest-neighbour matching",
            Self::Weighting => "Inverse-probability weighting",
            Self::DoublyRobust => "Augmented inverse-probability weighting (doubly robust)",
        }
    }

    /// Key used in saved files and scripts.
    #[must_use]
    pub fn key(&self) -> &'static str {
        match self {
            Self::Matching(_) => "matching",
            Self::Weighting => "weighting",
            Self::DoublyRobust => "doubly-robust",
        }
    }
}

/// How intervals are computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntervalMethod {
    /// Sandwich (stacked M-estimation, or robust for matched data).
    Sandwich,
    /// Stratified nonparametric bootstrap with percentile intervals.
    Bootstrap {
        /// Number of resamples.
        replicates: usize,
        /// Random seed.
        seed: u64,
    },
}

/// Options for [`estimate_treatment_effect`].
#[derive(Debug, Clone, PartialEq)]
pub struct TreatmentEffectOptions {
    /// Method.
    pub method: EffectMethod,
    /// ATE or ATT.
    pub estimand: Estimand,
    /// Kind of outcome.
    pub outcome: OutcomeKind,
    /// Stabilize inverse-probability weights (saved weights only).
    pub stabilized: bool,
    /// Leave out rows with a score below `trim` or above `1 − trim`
    /// (0 for none; at most 0.25).
    pub trim: f64,
    /// Interval method.
    pub intervals: IntervalMethod,
    /// Confidence level.
    pub confidence_level: f64,
}

impl TreatmentEffectOptions {
    /// Defaults: inverse-probability weighting for the ATE, stabilized
    /// weights, no trimming, sandwich 95% intervals.
    #[must_use]
    pub fn new(outcome: OutcomeKind) -> Self {
        Self {
            method: EffectMethod::Weighting,
            estimand: Estimand::Ate,
            outcome,
            stabilized: true,
            trim: 0.0,
            intervals: IntervalMethod::Sandwich,
            confidence_level: 0.95,
        }
    }
}

/// A fitted propensity-score model.
#[derive(Debug, Clone, PartialEq)]
pub struct PropensityModel {
    /// Logistic regression of the treatment on the covariates.
    pub fit: GlmFit,
    /// Fitted probabilities of treatment `êᵢ`.
    pub scores: Vec<f64>,
}

/// Fit the propensity score: logistic regression of `t` (0/1) on the
/// columns of `x` (an intercept is added), named `names`.
///
/// # Errors
/// Treatment values other than 0/1, fewer than two rows in a group, and
/// the errors of [`fit_glm`].
pub fn fit_propensity(x: &Matrix, names: &[String], t: &[f64]) -> Result<PropensityModel> {
    check_treatment(t)?;
    let mut options = GlmOptions::new(Family::Binomial);
    options.names = names.to_vec();
    let fit = fit_glm(x, t, &options)?;
    let scores = fit.fitted.clone();
    Ok(PropensityModel { fit, scores })
}

fn check_treatment(t: &[f64]) -> Result<()> {
    if t.iter().any(|&v| v != 0.0 && v != 1.0) {
        return Err(invalid("the treatment must be coded 0/1"));
    }
    let treated = t.iter().filter(|&&v| v == 1.0).count();
    if treated < 2 || t.len() - treated < 2 {
        return Err(invalid("each treatment group needs at least two rows"));
    }
    Ok(())
}

fn logistic(eta: f64) -> f64 {
    if eta >= 0.0 {
        1.0 / (1.0 + (-eta).exp())
    } else {
        let e = eta.exp();
        e / (1.0 + e)
    }
}

fn logit(p: f64) -> f64 {
    (p / (1.0 - p)).ln()
}

/// Scale on which an effect is reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectScale {
    /// `μ₁ − μ₀`.
    Difference,
    /// `μ₁ / μ₀` (risk or rate ratio).
    Ratio,
    /// Odds ratio of a binary outcome.
    OddsRatio,
}

impl EffectScale {
    /// Display name for an outcome kind, e.g. "Risk difference".
    #[must_use]
    pub fn label(self, outcome: OutcomeKind) -> &'static str {
        match (self, outcome) {
            (Self::Difference, OutcomeKind::Continuous) => "Mean difference",
            (Self::Difference, OutcomeKind::Binary) => "Risk difference",
            (Self::Difference, OutcomeKind::Count) => "Rate difference",
            (Self::Ratio, OutcomeKind::Binary) => "Risk ratio",
            (Self::Ratio, _) => "Rate ratio",
            (Self::OddsRatio, _) => "Odds ratio",
        }
    }

    /// Whether the scale is a ratio (estimated on the log scale; no effect is 1).
    #[must_use]
    pub fn is_ratio(self) -> bool {
        self != Self::Difference
    }

    /// The scales reported for an outcome kind.
    #[must_use]
    pub fn for_outcome(outcome: OutcomeKind) -> &'static [Self] {
        match outcome {
            OutcomeKind::Continuous => &[Self::Difference],
            OutcomeKind::Binary => &[Self::Difference, Self::Ratio, Self::OddsRatio],
            OutcomeKind::Count => &[Self::Difference, Self::Ratio],
        }
    }

    /// The effect on this scale and its gradient in `(μ₁, μ₀)`; ratios on
    /// the log scale. `None` when the means are outside the scale's range.
    fn transform(self, m1: f64, m0: f64) -> Option<(f64, [f64; 2])> {
        match self {
            Self::Difference => Some((m1 - m0, [1.0, -1.0])),
            Self::Ratio => (m1 > 0.0 && m0 > 0.0).then(|| ((m1 / m0).ln(), [1.0 / m1, -1.0 / m0])),
            Self::OddsRatio => (m1 > 0.0 && m1 < 1.0 && m0 > 0.0 && m0 < 1.0)
                .then(|| (logit(m1) - logit(m0), [1.0 / (m1 * (1.0 - m1)), -1.0 / (m0 * (1.0 - m0))])),
        }
    }
}

/// One effect estimate.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectEstimate {
    /// Scale.
    pub scale: EffectScale,
    /// Estimate (a ratio itself, not its log).
    pub estimate: f64,
    /// Standard error, of the log for ratios.
    pub std_error: f64,
    /// `z` statistic for no effect (log scale for ratios).
    pub z: f64,
    /// Two-sided p-value of the `z` test (sandwich), or the share of
    /// bootstrap replicates on the other side of no effect, doubled.
    pub p_value: f64,
    /// Confidence interval (on the ratio scale for ratios).
    pub interval: (f64, f64),
}

impl EffectEstimate {
    /// Estimate on the analysis scale (log for ratios).
    #[must_use]
    pub fn analysis_estimate(&self) -> f64 {
        if self.scale.is_ratio() { self.estimate.ln() } else { self.estimate }
    }
}

/// A treatment-effect estimate with what produced it.
#[derive(Debug, Clone, PartialEq)]
pub struct TreatmentEffectFit {
    /// Options used.
    pub options: TreatmentEffectOptions,
    /// Propensity model fitted on every row.
    pub propensity: PropensityModel,
    /// Analysis weight of each row (0 for rows left out: trimmed, or not
    /// matched). For weighting, stabilized when asked.
    pub weights: Vec<f64>,
    /// Matched set of each row for matching without replacement (the
    /// set's number, from 1), `None` otherwise.
    pub matched_set: Vec<Option<usize>>,
    /// Rows the estimate uses.
    pub used: Vec<bool>,
    /// Estimated potential-outcome mean under treatment.
    pub mean_treated: f64,
    /// Estimated potential-outcome mean under control.
    pub mean_control: f64,
    /// Covariance of `(μ̂₁, μ̂₀)` (sandwich, or from the bootstrap).
    pub covariance: [[f64; 2]; 2],
    /// Effects on each scale of the outcome kind.
    pub effects: Vec<EffectEstimate>,
    /// Outcome models (treated, control) for the doubly robust method; the
    /// treated model is `None` for the ATT.
    pub outcome_models: Option<(Option<GlmFit>, GlmFit)>,
    /// Rows in each group `(treated, control)`.
    pub n: (usize, usize),
    /// Rows used in each group.
    pub n_used: (usize, usize),
    /// Kish effective sample size `(Σw)²/Σw²` of each group.
    pub effective_n: (f64, f64),
    /// Rows left out by trimming.
    pub n_trimmed: usize,
    /// Bootstrap replicates that failed (left out of the intervals).
    pub failed_replicates: usize,
    /// Warnings.
    pub warnings: Vec<String>,
}

/// Point estimates of one analysis, before intervals.
struct PointFit {
    propensity: PropensityModel,
    weights: Vec<f64>,
    matched_set: Vec<Option<usize>>,
    clusters: Vec<Option<usize>>,
    used: Vec<bool>,
    m1: f64,
    m0: f64,
    /// Stacked parameters and estimating functions (weighting, doubly robust).
    stacked: Option<Stacked>,
    outcome_models: Option<(Option<GlmFit>, GlmFit)>,
    n_trimmed: usize,
    warnings: Vec<String>,
}

/// Inputs to the stacked estimating equations.
struct Stacked {
    theta: Vec<f64>,
    /// Columns of the outcome models' design (with intercept): `q`.
    q: usize,
    keep: Vec<bool>,
    has_m1: bool,
    has_m0: bool,
}

/// Estimate the treatment effect: `x` the covariates (one row per
/// observation, no intercept column), `names` their names, `t` the
/// treatment (0/1), `y` the outcome.
///
/// # Errors
/// See the module documentation.
pub fn estimate_treatment_effect(
    x: &Matrix,
    names: &[String],
    t: &[f64],
    y: &[f64],
    options: &TreatmentEffectOptions,
) -> Result<TreatmentEffectFit> {
    let n = x.nrows();
    if t.len() != n || y.len() != n {
        return Err(StatError::DimensionMismatch {
            context: "treatment and outcome vs. covariate rows",
            expected: n,
            found: t.len().min(y.len()),
        });
    }
    if !(options.confidence_level > 0.0 && options.confidence_level < 1.0) {
        return Err(invalid("the confidence level must be between 0 and 1"));
    }
    if !(0.0..=0.25).contains(&options.trim) {
        return Err(invalid("the trimming threshold must be between 0 and 0.25"));
    }
    if let EffectMethod::Matching(m) = options.method {
        if m.ratio == 0 {
            return Err(invalid("matching needs at least one match per row"));
        }
        if m.caliper.is_some_and(|c| !(c > 0.0 && c.is_finite())) {
            return Err(invalid("the caliper must be positive"));
        }
        if options.estimand == Estimand::Ate && !m.replace {
            return Err(invalid("matching for the average effect over everyone (ATE) needs matching with replacement"));
        }
    }
    options.outcome.check(y)?;
    let point = point_fit(x, names, t, y, options)?;
    let level = options.confidence_level;
    let scales = EffectScale::for_outcome(options.outcome);
    let n_treated = t.iter().filter(|&&v| v == 1.0).count();
    let mut warnings = point.warnings.clone();
    let mut failed = 0;
    let (covariance, effects) = match options.intervals {
        IntervalMethod::Sandwich => {
            let cov = match &point.stacked {
                Some(s) => stacked_covariance(x, t, y, options, s)?,
                None => matched_covariance(t, y, &point.weights, &point.clusters),
            };
            let z = Normal::standard().quantile(0.5 + level / 2.0)?;
            let effects = scales
                .iter()
                .filter_map(|&scale| {
                    let (est, g) = scale.transform(point.m1, point.m0)?;
                    let var = g[0] * g[0] * cov[0][0] + g[1] * g[1] * cov[1][1] + 2.0 * g[0] * g[1] * cov[0][1];
                    let se = var.max(0.0).sqrt();
                    let stat = est / se;
                    let p = 2.0 * Normal::standard().sf(stat.abs()).unwrap_or(f64::NAN);
                    let (lo, hi) = (est - z * se, est + z * se);
                    Some(if scale.is_ratio() {
                        EffectEstimate {
                            scale,
                            estimate: est.exp(),
                            std_error: se,
                            z: stat,
                            p_value: p,
                            interval: (lo.exp(), hi.exp()),
                        }
                    } else {
                        EffectEstimate { scale, estimate: est, std_error: se, z: stat, p_value: p, interval: (lo, hi) }
                    })
                })
                .collect();
            (cov, effects)
        }
        IntervalMethod::Bootstrap { replicates, seed } => {
            if replicates < 20 {
                return Err(invalid("the bootstrap needs at least 20 replicates"));
            }
            let treated: Vec<usize> = (0..n).filter(|&i| t[i] == 1.0).collect();
            let control: Vec<usize> = (0..n).filter(|&i| t[i] == 0.0).collect();
            let mut rng = Rng::new(seed);
            let mut means: Vec<(f64, f64)> = Vec::with_capacity(replicates);
            let quiet = TreatmentEffectOptions { intervals: IntervalMethod::Sandwich, ..options.clone() };
            for _ in 0..replicates {
                let mut rows = Vec::with_capacity(n);
                rows.extend((0..treated.len()).map(|_| treated[rng.below(treated.len())]));
                rows.extend((0..control.len()).map(|_| control[rng.below(control.len())]));
                let xb = Matrix::from_rows(&rows.iter().map(|&i| x.row(i)).collect::<Vec<_>>())?;
                let tb: Vec<f64> = rows.iter().map(|&i| t[i]).collect();
                let yb: Vec<f64> = rows.iter().map(|&i| y[i]).collect();
                match point_fit(&xb, names, &tb, &yb, &quiet) {
                    Ok(p) if p.m1.is_finite() && p.m0.is_finite() => means.push((p.m1, p.m0)),
                    _ => failed += 1,
                }
            }
            if means.len() < replicates / 2 {
                return Err(invalid(format!("{failed} of {replicates} bootstrap replicates failed")));
            }
            if failed > 0 {
                warnings.push(format!(
                    "{failed} of {replicates} bootstrap replicates could not be fitted and were left out"
                ));
            }
            let b = means.len() as f64;
            let (a1, a0) = means.iter().fold((0.0, 0.0), |(s1, s0), m| (s1 + m.0 / b, s0 + m.1 / b));
            let mut cov = [[0.0; 2]; 2];
            for &(m1, m0) in &means {
                let d = [m1 - a1, m0 - a0];
                for r in 0..2 {
                    for c in 0..2 {
                        cov[r][c] += d[r] * d[c] / (b - 1.0);
                    }
                }
            }
            let alpha = 1.0 - level;
            let effects = scales
                .iter()
                .filter_map(|&scale| {
                    let (est, _) = scale.transform(point.m1, point.m0)?;
                    let mut reps: Vec<f64> =
                        means.iter().filter_map(|&(m1, m0)| scale.transform(m1, m0).map(|v| v.0)).collect();
                    if reps.len() < 2 {
                        return None;
                    }
                    reps.sort_by(f64::total_cmp);
                    let k = reps.len() as f64;
                    let mean = reps.iter().sum::<f64>() / k;
                    let se = (reps.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (k - 1.0)).sqrt();
                    let (lo, hi) = (percentile(&reps, alpha / 2.0), percentile(&reps, 1.0 - alpha / 2.0));
                    let below = reps.iter().filter(|&&r| r <= 0.0).count() as f64 / k;
                    let p = (2.0 * below.min(1.0 - below)).min(1.0);
                    let stat = est / se;
                    Some(if scale.is_ratio() {
                        EffectEstimate {
                            scale,
                            estimate: est.exp(),
                            std_error: se,
                            z: stat,
                            p_value: p,
                            interval: (lo.exp(), hi.exp()),
                        }
                    } else {
                        EffectEstimate { scale, estimate: est, std_error: se, z: stat, p_value: p, interval: (lo, hi) }
                    })
                })
                .collect();
            (cov, effects)
        }
    };
    let group_n = |g: f64| (0..n).filter(|&i| t[i] == g && point.used[i]).count();
    let ess = |g: f64| {
        let (s, s2) = (0..n)
            .filter(|&i| t[i] == g)
            .fold((0.0, 0.0), |(s, s2), i| (s + point.weights[i], s2 + point.weights[i].powi(2)));
        if s2 > 0.0 { s * s / s2 } else { 0.0 }
    };
    Ok(TreatmentEffectFit {
        options: options.clone(),
        weights: point.weights.clone(),
        matched_set: point.matched_set.clone(),
        used: point.used.clone(),
        mean_treated: point.m1,
        mean_control: point.m0,
        covariance,
        effects,
        n: (n_treated, n - n_treated),
        n_used: (group_n(1.0), group_n(0.0)),
        effective_n: (ess(1.0), ess(0.0)),
        n_trimmed: point.n_trimmed,
        failed_replicates: failed,
        warnings,
        outcome_models: point.outcome_models,
        propensity: point.propensity,
    })
}

/// The `p` quantile of sorted values (linear interpolation, R type 7).
fn percentile(sorted: &[f64], p: f64) -> f64 {
    let h = (sorted.len() - 1) as f64 * p;
    let (lo, hi) = (h.floor() as usize, h.ceil() as usize);
    sorted[lo] + (h - lo as f64) * (sorted[hi] - sorted[lo])
}

fn point_fit(x: &Matrix, names: &[String], t: &[f64], y: &[f64], options: &TreatmentEffectOptions) -> Result<PointFit> {
    let n = x.nrows();
    let propensity = fit_propensity(x, names, t)?;
    let mut warnings = propensity.fit.warnings.iter().map(|w| format!("Propensity model: {w}")).collect::<Vec<_>>();
    let e = &propensity.scores;
    let keep: Vec<bool> = e.iter().map(|&p| p >= options.trim && p <= 1.0 - options.trim).collect();
    let n_trimmed = keep.iter().filter(|&&k| !k).count();
    for g in [1.0, 0.0] {
        if (0..n).filter(|&i| keep[i] && t[i] == g).count() < 2 {
            return Err(invalid("trimming leaves fewer than two rows in a treatment group"));
        }
    }
    if !matches!(options.method, EffectMethod::Matching(_)) && (0..n).any(|i| keep[i] && !(e[i] > 0.0 && e[i] < 1.0)) {
        return Err(invalid(
            "some propensity scores are 0 or 1 (the covariates separate the groups); trim them or drop covariates",
        ));
    }
    match options.method {
        EffectMethod::Matching(m) => {
            let (weights, matched_set, clusters) = nearest_neighbour_match(e, t, &keep, m, options.estimand)?;
            let used: Vec<bool> = weights.iter().map(|&w| w > 0.0).collect();
            let mean = |g: f64| {
                let (s, sw) = (0..n)
                    .filter(|&i| t[i] == g)
                    .fold((0.0, 0.0), |(s, sw), i| (s + weights[i] * y[i], sw + weights[i]));
                s / sw
            };
            Ok(PointFit {
                m1: mean(1.0),
                m0: mean(0.0),
                propensity,
                weights,
                matched_set,
                clusters,
                used,
                stacked: None,
                outcome_models: None,
                n_trimmed,
                warnings,
            })
        }
        EffectMethod::Weighting => {
            let w = ipw_weights(e, t, &keep, options.estimand, false);
            let mean = |g: f64| {
                let (s, sw) = (0..n).filter(|&i| t[i] == g).fold((0.0, 0.0), |(s, sw), i| (s + w[i] * y[i], sw + w[i]));
                s / sw
            };
            let (m1, m0) = (mean(1.0), mean(0.0));
            let mut theta: Vec<f64> = propensity.fit.coefficients.iter().map(|c| c.estimate).collect();
            theta.extend([m1, m0]);
            let weights = ipw_weights(e, t, &keep, options.estimand, options.stabilized);
            Ok(PointFit {
                m1,
                m0,
                weights,
                matched_set: vec![None; n],
                clusters: vec![None; n],
                used: keep.clone(),
                stacked: Some(Stacked { theta, q: x.ncols() + 1, keep, has_m1: false, has_m0: false }),
                outcome_models: None,
                propensity,
                n_trimmed,
                warnings,
            })
        }
        EffectMethod::DoublyRobust => {
            let fit_group = |g: f64| -> Result<GlmFit> {
                let rows: Vec<usize> = (0..n).filter(|&i| keep[i] && t[i] == g).collect();
                let xg = Matrix::from_rows(&rows.iter().map(|&i| x.row(i)).collect::<Vec<_>>())?;
                let yg: Vec<f64> = rows.iter().map(|&i| y[i]).collect();
                let mut o = GlmOptions::new(options.outcome.family());
                o.names = names.to_vec();
                fit_glm(&xg, &yg, &o).map_err(|err| {
                    invalid(format!(
                        "the outcome model for the {} rows failed: {err}",
                        if g == 1.0 { "treated" } else { "control" }
                    ))
                })
            };
            let ate = options.estimand == Estimand::Ate;
            let model1 = if ate { Some(fit_group(1.0)?) } else { None };
            let model0 = fit_group(0.0)?;
            for (m, g) in [(model1.as_ref(), "treated"), (Some(&model0), "control")] {
                if let Some(m) = m {
                    warnings.extend(m.warnings.iter().map(|w| format!("Outcome model ({g}): {w}")));
                }
            }
            let coef = |m: &GlmFit| m.coefficients.iter().map(|c| c.estimate).collect::<Vec<f64>>();
            let predict = |b: &[f64], i: usize| options.outcome.mean(linear(b, x, i));
            let b0 = coef(&model0);
            let b1 = model1.as_ref().map(coef);
            let n_keep = keep.iter().filter(|&&k| k).count() as f64;
            let (m1, m0) = if ate {
                let b1 = b1.as_ref().expect("fitted for the ATE");
                let (mut s1, mut s0) = (0.0, 0.0);
                for i in (0..n).filter(|&i| keep[i]) {
                    let (p1, p0) = (predict(b1, i), predict(&b0, i));
                    s1 += t[i] * y[i] / e[i] - (t[i] - e[i]) * p1 / e[i];
                    s0 += (1.0 - t[i]) * y[i] / (1.0 - e[i]) + (t[i] - e[i]) * p0 / (1.0 - e[i]);
                }
                (s1 / n_keep, s0 / n_keep)
            } else {
                let (mut s1, mut s0, mut nt) = (0.0, 0.0, 0.0);
                for i in (0..n).filter(|&i| keep[i]) {
                    let p0 = predict(&b0, i);
                    s1 += t[i] * y[i];
                    s0 += t[i] * p0 + (1.0 - t[i]) * e[i] / (1.0 - e[i]) * (y[i] - p0);
                    nt += t[i];
                }
                (s1 / nt, s0 / nt)
            };
            let mut theta: Vec<f64> = propensity.fit.coefficients.iter().map(|c| c.estimate).collect();
            if let Some(b1) = &b1 {
                theta.extend(b1);
            }
            theta.extend(&b0);
            theta.extend([m1, m0]);
            let weights = ipw_weights(e, t, &keep, options.estimand, options.stabilized);
            Ok(PointFit {
                m1,
                m0,
                weights,
                matched_set: vec![None; n],
                clusters: vec![None; n],
                used: keep.clone(),
                stacked: Some(Stacked { theta, q: x.ncols() + 1, keep, has_m1: ate, has_m0: true }),
                outcome_models: Some((model1, model0)),
                propensity,
                n_trimmed,
                warnings,
            })
        }
    }
}

/// `b₀ + Σ bⱼ xᵢⱼ`.
fn linear(b: &[f64], x: &Matrix, i: usize) -> f64 {
    b[0] + (0..x.ncols()).map(|j| b[j + 1] * x[(i, j)]).sum::<f64>()
}

/// Inverse-probability weights (0 for rows not kept).
#[must_use]
pub fn ipw_weights(e: &[f64], t: &[f64], keep: &[bool], estimand: Estimand, stabilized: bool) -> Vec<f64> {
    let kept = |g: f64| (0..e.len()).filter(|&i| keep[i] && t[i] == g).count() as f64;
    let total = kept(1.0) + kept(0.0);
    let (p1, p0) = (kept(1.0) / total, kept(0.0) / total);
    (0..e.len())
        .map(|i| {
            if !keep[i] {
                return 0.0;
            }
            match (estimand, t[i] == 1.0) {
                (Estimand::Ate, true) => (if stabilized { p1 } else { 1.0 }) / e[i],
                (Estimand::Ate, false) => (if stabilized { p0 } else { 1.0 }) / (1.0 - e[i]),
                (Estimand::Att, true) => 1.0,
                (Estimand::Att, false) => (if stabilized { p0 / p1 } else { 1.0 }) * e[i] / (1.0 - e[i]),
            }
        })
        .collect()
}

/// Per-row estimating functions `ψᵢ(θ)` (rows of the result) for weighting
/// and the doubly robust estimator.
fn estimating_functions(
    x: &Matrix,
    t: &[f64],
    y: &[f64],
    options: &TreatmentEffectOptions,
    s: &Stacked,
    theta: &[f64],
) -> Vec<Vec<f64>> {
    let (n, q) = (x.nrows(), s.q);
    let beta = &theta[..q];
    let mut at = q;
    let a1 = if s.has_m1 {
        at += q;
        Some(&theta[at - q..at])
    } else {
        None
    };
    let a0 = if s.has_m0 {
        at += q;
        Some(&theta[at - q..at])
    } else {
        None
    };
    let (m1, m0) = (theta[at], theta[at + 1]);
    let kind = options.outcome;
    (0..n)
        .map(|i| {
            let e = logistic(linear(beta, x, i));
            let xi = |j: usize| if j == 0 { 1.0 } else { x[(i, j - 1)] };
            let mut psi = Vec::with_capacity(theta.len());
            psi.extend((0..q).map(|j| xi(j) * (t[i] - e)));
            let k = if s.keep[i] { 1.0 } else { 0.0 };
            let p1 = a1.map(|a| kind.mean(linear(a, x, i)));
            let p0 = a0.map(|a| kind.mean(linear(a, x, i)));
            if let Some(p1) = p1 {
                psi.extend((0..q).map(|j| k * t[i] * xi(j) * (y[i] - p1)));
            }
            if let Some(p0) = p0 {
                psi.extend((0..q).map(|j| k * (1.0 - t[i]) * xi(j) * (y[i] - p0)));
            }
            let (f1, f0) = match (options.method, options.estimand) {
                (EffectMethod::DoublyRobust, Estimand::Ate) => {
                    let (p1, p0) = (p1.unwrap_or_default(), p0.unwrap_or_default());
                    (
                        t[i] * y[i] / e - (t[i] - e) * p1 / e - m1,
                        (1.0 - t[i]) * y[i] / (1.0 - e) + (t[i] - e) * p0 / (1.0 - e) - m0,
                    )
                }
                (EffectMethod::DoublyRobust, Estimand::Att) => {
                    let p0 = p0.unwrap_or_default();
                    (t[i] * (y[i] - m1), t[i] * (p0 - m0) + (1.0 - t[i]) * e / (1.0 - e) * (y[i] - p0))
                }
                (_, Estimand::Ate) => (t[i] * (y[i] - m1) / e, (1.0 - t[i]) * (y[i] - m0) / (1.0 - e)),
                (_, Estimand::Att) => (t[i] * (y[i] - m1), (1.0 - t[i]) * e / (1.0 - e) * (y[i] - m0)),
            };
            psi.push(k * f1);
            psi.push(k * f0);
            psi
        })
        .collect()
}

/// Sandwich covariance of `(μ̂₁, μ̂₀)` from the stacked equations.
fn stacked_covariance(
    x: &Matrix,
    t: &[f64],
    y: &[f64],
    options: &TreatmentEffectOptions,
    s: &Stacked,
) -> Result<[[f64; 2]; 2]> {
    let d = s.theta.len();
    let total = |theta: &[f64]| -> Vec<f64> {
        let mut sum = vec![0.0; d];
        for row in estimating_functions(x, t, y, options, s, theta) {
            for (a, b) in sum.iter_mut().zip(row) {
                *a += b;
            }
        }
        sum
    };
    // A = Σ ∂ψᵢ/∂θ by central differences.
    let mut a = Matrix::zeros(d, d);
    for j in 0..d {
        let h = 1e-6 * s.theta[j].abs().max(1.0);
        let mut up = s.theta.clone();
        let mut down = s.theta.clone();
        up[j] += h;
        down[j] -= h;
        let (fu, fd) = (total(&up), total(&down));
        for r in 0..d {
            a[(r, j)] = (fu[r] - fd[r]) / (2.0 * h);
        }
    }
    let mut b = Matrix::zeros(d, d);
    for row in estimating_functions(x, t, y, options, s, &s.theta) {
        for r in 0..d {
            for c in 0..d {
                b[(r, c)] += row[r] * row[c];
            }
        }
    }
    // cov = A⁻¹ B A⁻ᵀ; only the last two rows of A⁻¹ are needed.
    let qr = QrDecomposition::new(a.transpose())?;
    let mut rows = Vec::with_capacity(2);
    for k in [d - 2, d - 1] {
        let mut unit = vec![0.0; d];
        unit[k] = 1.0;
        // Row k of A⁻¹ solves Aᵀ r = e_k.
        rows.push(qr.solve_least_squares(&unit)?);
    }
    let mut cov = [[0.0; 2]; 2];
    for (r, ra) in rows.iter().enumerate() {
        for (c, rc) in rows.iter().enumerate() {
            let bc = b.mul_vec(rc)?;
            cov[r][c] = ra.iter().zip(&bc).map(|(p, q)| p * q).sum();
        }
    }
    if !(cov[0][0].is_finite() && cov[1][1].is_finite()) {
        return Err(invalid("the sandwich covariance could not be computed"));
    }
    Ok(cov)
}

/// Robust covariance of the weighted group means of matched data, with
/// `clusters` (matched sets) or, where `None`, each row its own cluster.
fn matched_covariance(t: &[f64], y: &[f64], w: &[f64], clusters: &[Option<usize>]) -> [[f64; 2]; 2] {
    let n = t.len();
    let mean_sum = |g: f64| {
        let (s, sw) = (0..n).filter(|&i| t[i] == g).fold((0.0, 0.0), |(s, sw), i| (s + w[i] * y[i], sw + w[i]));
        (s / sw, sw)
    };
    let ((m1, w1), (m0, w0)) = (mean_sum(1.0), mean_sum(0.0));
    let mut sums: std::collections::BTreeMap<(bool, usize), [f64; 2]> = std::collections::BTreeMap::new();
    let mut n_used = 0usize;
    for i in (0..n).filter(|&i| w[i] > 0.0) {
        n_used += 1;
        let psi = if t[i] == 1.0 { [w[i] * (y[i] - m1) / w1, 0.0] } else { [0.0, w[i] * (y[i] - m0) / w0] };
        let key = clusters[i].map_or((false, i), |c| (true, c));
        let s = sums.entry(key).or_insert([0.0; 2]);
        s[0] += psi[0];
        s[1] += psi[1];
    }
    let g = sums.len() as f64;
    let nn = n_used as f64;
    let adjust = g / (g - 1.0) * (nn - 1.0) / (nn - 2.0);
    let mut cov = [[0.0; 2]; 2];
    for s in sums.values() {
        for r in 0..2 {
            for c in 0..2 {
                cov[r][c] += adjust * s[r] * s[c];
            }
        }
    }
    cov
}

/// Nearest-neighbour matching on the logit of `e`. Returns each row's
/// weight, its matched set (without replacement) and the clusters for the
/// robust covariance.
#[allow(clippy::type_complexity)]
fn nearest_neighbour_match(
    e: &[f64],
    t: &[f64],
    keep: &[bool],
    m: MatchingOptions,
    estimand: Estimand,
) -> Result<(Vec<f64>, Vec<Option<usize>>, Vec<Option<usize>>)> {
    let n = e.len();
    let clamp = |p: f64| p.clamp(1e-15, 1.0 - 1e-15);
    let d: Vec<f64> = e.iter().map(|&p| logit(clamp(p))).collect();
    let caliper = m.caliper.map(|c| {
        let mean = d.iter().sum::<f64>() / n as f64;
        c * (d.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n as f64 - 1.0)).sqrt()
    });
    let within = |a: usize, b: usize| caliper.is_none_or(|c| (d[a] - d[b]).abs() <= c);
    // matches[i]: the rows matched to row i (an anchor).
    let mut matches: Vec<Vec<usize>> = vec![Vec::new(); n];
    let anchors_of = |g: f64| {
        let mut a: Vec<usize> = (0..n).filter(|&i| keep[i] && t[i] == g).collect();
        // Decreasing score, ties by row.
        a.sort_by(|&i, &j| d[j].total_cmp(&d[i]).then(i.cmp(&j)));
        a
    };
    let groups: &[f64] = if estimand == Estimand::Ate { &[1.0, 0.0] } else { &[1.0] };
    for &g in groups {
        let anchors = anchors_of(g);
        let pool: Vec<usize> = (0..n).filter(|&i| keep[i] && t[i] != g).collect();
        if m.replace {
            for &a in &anchors {
                let mut near: Vec<usize> = pool.iter().copied().filter(|&c| within(a, c)).collect();
                near.sort_by(|&i, &j| (d[i] - d[a]).abs().total_cmp(&(d[j] - d[a]).abs()).then(i.cmp(&j)));
                near.truncate(m.ratio);
                matches[a] = near;
            }
        } else {
            let mut available = vec![false; n];
            for &c in &pool {
                available[c] = true;
            }
            for _round in 0..m.ratio {
                for &a in &anchors {
                    // A row left without a first match gets no later ones.
                    if _round > 0 && matches[a].len() < _round {
                        continue;
                    }
                    let best = pool
                        .iter()
                        .copied()
                        .filter(|&c| available[c] && within(a, c))
                        .min_by(|&i, &j| (d[i] - d[a]).abs().total_cmp(&(d[j] - d[a]).abs()).then(i.cmp(&j)));
                    if let Some(c) = best {
                        available[c] = false;
                        matches[a].push(c);
                    }
                }
            }
        }
    }
    let mut weights = vec![0.0; n];
    let mut matched_set = vec![None; n];
    let mut set = 0;
    let mut anchors_total = [0usize; 2];
    for &g in groups {
        for i in (0..n).filter(|&i| t[i] == g && !matches[i].is_empty()) {
            anchors_total[usize::from(g == 1.0)] += 1;
            if !m.replace {
                set += 1;
                matched_set[i] = Some(set);
                for &c in &matches[i] {
                    matched_set[c] = Some(set);
                }
            }
        }
    }
    if anchors_total[1] == 0 {
        return Err(invalid("no treated row has a match within the caliper"));
    }
    // Weights: an anchor counts 1 in its own group; each match receives
    // 1/(its anchor's number of matches), summed over anchors. Weights of
    // rows serving as matches are then rescaled, within each group, to sum to
    // the number of such rows.
    let mut own = vec![0.0; n];
    let mut served = vec![0.0; n];
    for i in 0..n {
        if !matches[i].is_empty() {
            own[i] = 1.0;
            let k = matches[i].len() as f64;
            for &c in &matches[i] {
                served[c] += 1.0 / k;
            }
        }
    }
    for g in [1.0, 0.0] {
        let rows: Vec<usize> = (0..n).filter(|&i| t[i] == g && served[i] > 0.0).collect();
        let total: f64 = rows.iter().map(|&i| served[i]).sum();
        if estimand == Estimand::Ate {
            // Each row is an anchor and may also serve: weight 1 + K/k, the
            // Abadie–Imbens form, scaled to the group size.
            for i in (0..n).filter(|&i| t[i] == g) {
                weights[i] = own[i] + served[i];
            }
        } else {
            for &i in &rows {
                weights[i] = served[i] * rows.len() as f64 / total;
            }
            for i in (0..n).filter(|&i| t[i] == g && own[i] > 0.0) {
                weights[i] = own[i];
            }
        }
    }
    if (0..n).all(|i| t[i] == 1.0 || weights[i] == 0.0) {
        return Err(invalid("no control row was matched"));
    }
    let clusters = if m.replace { vec![None; n] } else { matched_set.clone() };
    Ok((weights, matched_set, clusters))
}

/// Balance of one variable before and after adjustment.
#[derive(Debug, Clone, PartialEq)]
pub struct BalanceRow {
    /// Variable name (a categorical covariate gives one row per level).
    pub name: String,
    /// Whether the variable is 0/1.
    pub binary: bool,
    /// Unadjusted means `(treated, control)`.
    pub means: (f64, f64),
    /// Adjusted (weighted) means.
    pub adjusted_means: (f64, f64),
    /// Standardized mean difference before adjustment.
    pub smd: f64,
    /// After adjustment.
    pub adjusted_smd: f64,
    /// Variance ratio treated/control before adjustment (continuous only).
    pub variance_ratio: Option<f64>,
    /// After adjustment.
    pub adjusted_variance_ratio: Option<f64>,
}

/// Covariate balance of `variables` (name, value per row) between the
/// treatment groups `t`, unweighted and with `weights` (rows with weight 0
/// left out). See the module documentation for the formulas.
#[must_use]
pub fn covariate_balance(
    variables: &[(String, Vec<f64>)],
    t: &[f64],
    weights: &[f64],
    estimand: Estimand,
) -> Vec<BalanceRow> {
    let ones = vec![1.0; t.len()];
    variables
        .iter()
        .map(|(name, v)| {
            let binary = v.iter().all(|&x| x == 0.0 || x == 1.0);
            let stats = |w: &[f64], g: f64| weighted_stats(v, w, t, g, binary);
            let (u1, u0) = (stats(&ones, 1.0), stats(&ones, 0.0));
            let (a1, a0) = (stats(weights, 1.0), stats(weights, 0.0));
            let sd = match estimand {
                Estimand::Ate => ((u1.1 + u0.1) / 2.0).sqrt(),
                Estimand::Att => u1.1.sqrt(),
            };
            let smd = |m1: f64, m0: f64| if sd > 0.0 { (m1 - m0) / sd } else { 0.0 };
            let ratio = |s1: f64, s0: f64| (!binary && s0 > 0.0).then(|| s1 / s0);
            BalanceRow {
                name: name.clone(),
                binary,
                means: (u1.0, u0.0),
                adjusted_means: (a1.0, a0.0),
                smd: smd(u1.0, u0.0),
                adjusted_smd: smd(a1.0, a0.0),
                variance_ratio: ratio(u1.1, u0.1),
                adjusted_variance_ratio: ratio(a1.1, a0.1),
            }
        })
        .collect()
}

/// Weighted mean and variance of `v` in group `g`.
fn weighted_stats(v: &[f64], w: &[f64], t: &[f64], g: f64, binary: bool) -> (f64, f64) {
    let rows = (0..v.len()).filter(|&i| t[i] == g && w[i] > 0.0);
    let (sw, sw2, swx) =
        rows.clone().fold((0.0, 0.0, 0.0), |(a, b, c), i| (a + w[i], b + w[i] * w[i], c + w[i] * v[i]));
    let mean = swx / sw;
    let var = if binary {
        mean * (1.0 - mean)
    } else {
        let ss: f64 = rows.map(|i| w[i] * (v[i] - mean).powi(2)).sum();
        sw / (sw * sw - sw2) * ss
    };
    (mean, var)
}

/// Test of equal effects across independent subgroups: Cochran's
/// `Q = Σ (θ̂_g − θ̄)²/v_g` with `θ̄` the inverse-variance weighted mean,
/// referred to χ² on `G − 1` df (the subgroup-by-treatment interaction).
/// `estimates` are `(estimate, standard error)` on the analysis scale.
/// Returns `(Q, df, p)`, or `None` with fewer than two usable subgroups.
#[must_use]
pub fn heterogeneity_test(estimates: &[(f64, f64)]) -> Option<(f64, usize, f64)> {
    let ok: Vec<(f64, f64)> =
        estimates.iter().copied().filter(|&(e, s)| e.is_finite() && s.is_finite() && s > 0.0).collect();
    if ok.len() < 2 {
        return None;
    }
    let (sw, swe) = ok.iter().fold((0.0, 0.0), |(a, b), &(e, s)| (a + 1.0 / (s * s), b + e / (s * s)));
    let mean = swe / sw;
    let q: f64 = ok.iter().map(|&(e, s)| (e - mean).powi(2) / (s * s)).sum();
    let df = ok.len() - 1;
    let p = ChiSquared::new(df as f64).ok()?.sf(q).ok()?;
    Some((q, df, p))
}

#[cfg(test)]
mod tests;
