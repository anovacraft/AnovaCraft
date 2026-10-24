//! # stat-reliability
//!
//! Time-to-event (survival, reliability) analysis with right censoring. An
//! observation is either an **event** (failure, death, relapse) at its time, or
//! **censored**: the unit was still running when observation stopped, so its
//! event time is only known to exceed the recorded time.
//!
//! * [`kaplan_meier`](mod@kaplan_meier) — the nonparametric product-limit
//!   estimate of the survival function, Greenwood standard errors, pointwise
//!   confidence bands, quantiles (median survival) with Brookmeyer–Crowley
//!   intervals, and the log-rank test comparing groups.
//! * [`parametric`] — maximum-likelihood fits of the two-parameter Weibull and
//!   lognormal life distributions to censored data, with the observed-information
//!   covariance matrix, parameter intervals and fitted survival curves.
//! * [`censored_fit`] — maximum-likelihood fits of every family in
//!   `stat_core::distribution_fit` (including threshold forms) to data with
//!   exact, right-, left- and interval-censored times; Wald and profile
//!   likelihood intervals for parameters, percentiles and failure
//!   probabilities; likelihood-ratio tests of nested forms; and the
//!   known-shape Weibull scale limits (Nelson 1985).
//! * [`competing_risks`] — competing risks: Aalen–Johansen cumulative
//!   incidence per cause, Gray's K-sample test, Fine–Gray subdistribution
//!   hazards regression, and failure modes combined into a series system.
//! * [`warranty`] — warranty returns: shipment and return counts as
//!   interval- and right-censored life data, and forecasts of future returns
//!   with prediction limits.
//! * [`life_data`] — censored life data, Turnbull's nonparametric estimate
//!   (which reduces to Kaplan–Meier for right censoring) and the censored
//!   Anderson–Darling distance of a fit from it.
//! * [`growth`] — reliability growth: the power-law (Crow–AMSAA) process for
//!   one developing system, time- or failure-terminated, with exact and
//!   unbiased shape estimates, intensity and MTBF intervals, the
//!   Cramér–von Mises test and phases.
//! * [`repairable`] — repairable systems: Nelson's mean cumulative function
//!   with confidence limits, two-population differences, constant-rate and
//!   power-law fits, and the Laplace and military handbook trend tests.
//! * [`proportional_hazards`] — Cox proportional hazards regression with
//!   Efron or Breslow ties and optional strata: hazard ratios with Wald and
//!   profile-likelihood intervals, likelihood-ratio, Wald and score tests,
//!   Breslow baseline and predicted survival with limits, Schoenfeld
//!   residuals and the Grambsch–Therneau test of proportional hazards,
//!   martingale and deviance residuals.
//! * [`survival_regression`] — accelerated failure time models (Weibull,
//!   lognormal, log-logistic, exponential) with covariates for exact, right-,
//!   left- and interval-censored times: maximum likelihood with Wald and
//!   profile intervals, time ratios, quantile and failure-probability
//!   predictions, Cox–Snell residuals.
//! * [`test_plans`] — demonstration plans for a Weibull shape, sample sizes
//!   for a percentile's precision under censoring, and optimum and
//!   compromise accelerated life test plans.
//! * [`life_regression`] — censored location–scale regression (location and
//!   log scale linear in covariates) with Wald and likelihood-ratio intervals.
//! * [`life_stress`] — life–stress models for accelerated life tests:
//!   Arrhenius, Eyring, inverse power and exponential relationships in one or
//!   two stresses, activation energies, acceleration factors, the life
//!   distribution at use conditions and comparisons with per-level fits.
//! * [`degradation`] — degradation paths per unit (linear on transformed
//!   scales, asymptotic, power) with pseudo failure times, the random-effects
//!   linear path model with its implied life distribution, and destructive
//!   degradation.
//! * [`fatigue`] — stress–life (S–N) curves for fatigue tests with run-outs:
//!   Basquin, semi-log, quadratic log–log and endurance-limit (Stromeyer)
//!   forms with lognormal, Weibull or log-logistic scatter, the endurance
//!   limit by profile likelihood, quantile curves and fatigue strength at a
//!   given life.
//! * [`censored_count`] — Poisson and negative binomial fits to counts that
//!   are censored (below a detection limit, too numerous to count, or in a
//!   range), with Wald and likelihood-ratio intervals.
//!
//! Results are cross-validated in the tests against published values for the
//! Freireich et al. (1963) leukemia remission data (Kaplan–Meier estimates and
//! Greenwood standard errors as tabulated by Klein & Moeschberger, *Survival
//! Analysis*, 2nd ed., §4.2; log-rank χ² = 16.79) and against independent
//! numerical maximization of the censored likelihoods.

pub mod censored_count;
pub mod censored_fit;
pub mod competing_risks;
pub mod degradation;
pub mod fatigue;
pub mod growth;
pub mod kaplan_meier;
pub mod life_data;
pub mod life_regression;
pub mod life_stress;
pub mod parametric;
pub mod proportional_hazards;
pub mod repairable;
mod support;
pub mod survival_regression;
pub mod test_plans;
pub mod warranty;

pub use censored_count::{CountFamily, CountFit, CountObservation, fit_censored_counts};
pub use censored_fit::{
    CensoredFit, IntervalEstimate, KnownShapeWeibull, NESTED_FAMILIES, NestedComparison, censored_log_likelihood,
    fit_censored, nested_comparison, weibull_known_shape,
};
pub use fatigue::{EnduranceLimit, FatigueData, FatigueFit, FatigueLevel, FatigueModel, SnForm, fit_fatigue};
pub use kaplan_meier::{
    BandTransform, KaplanMeier, LogRankTest, LogRankTrend, ProductLimitStep, kaplan_meier, log_rank_test,
};
pub use life_data::{
    CensorKind, LifeData, LifeObservation, NonparametricEstimate, TurnbullInterval, censored_anderson_darling, turnbull,
};
pub use parametric::{LifeDistribution, ParameterEstimate, ParametricFit, fit_life_distribution};
pub use proportional_hazards::{
    BaselineStep, HazardCoefficient, PredictedSurvival, ProportionalHazardsData, ProportionalHazardsFit,
    ProportionalHazardsOptions, ProportionalityTest, SchoenfeldResidual, TieMethod, TimeTransform,
    fit_proportional_hazards,
};
pub use support::ChiSquareTest;
pub use survival_regression::{
    AftDistribution, LimitedPrediction, RegressionParameter, SurvivalRegressionFit, SurvivalRegressionOptions,
    fit_survival_regression,
};
