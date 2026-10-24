//! # stat-bayes
//!
//! Bayesian analysis, implemented from the published methods cited in each
//! module:
//!
//! * [`integrate`] — adaptive Gauss–Kronrod quadrature, including
//!   log-scale integration of peaked marginal-likelihood integrands.
//! * [`univariate`] — closed-form posteriors (beta, gamma, scaled t, the
//!   SD of a normal), contrasts of two independent posteriors by quadrature,
//!   and posterior summaries with equal-tailed and HPD intervals.
//! * [`conjugate`] — proportions, event rates and normal means with
//!   conjugate or Jeffreys priors, one or two groups.
//! * [`bayes_factor`] — default Bayes factors: JZS t tests, correlation,
//!   contingency tables.
//! * [`linear`] — Bayesian linear regression and ANOVA with g-priors:
//!   Bayes factors for every model, posterior model and inclusion
//!   probabilities, model-averaged estimates.
//! * [`nuts`] — the No-U-Turn Hamiltonian Monte Carlo sampler with
//!   step-size and diagonal mass-matrix adaptation, chains in parallel.
//! * [`diagnostics`] — rank-normalized R̂, bulk and tail effective sample
//!   sizes, Monte Carlo standard errors.
//! * [`loo`] — Pareto-smoothed importance-sampling leave-one-out
//!   cross-validation and WAIC.
//! * [`glm`] — Bayesian generalized linear and hierarchical models on the
//!   sampler.

pub mod bayes_factor;
pub mod conjugate;
pub mod diagnostics;
pub mod glm;
pub mod integrate;
pub mod linear;
pub mod loo;
pub mod nuts;
pub mod univariate;
pub(crate) mod util;

pub use univariate::{Summary, Univariate, hpd_from_draws, summarize};
