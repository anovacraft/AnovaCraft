//! # stat-mixed
//!
//! Linear and generalized linear mixed models: fixed effects plus random
//! intercepts and slopes for one or more grouping columns (crossed or
//! nested), for normal, binomial, Poisson, Gamma and negative binomial
//! responses.
//!
//! * **Normal** responses are fitted by restricted maximum likelihood (REML,
//!   the default) or maximum likelihood, profiling the fixed effects and the
//!   residual variance out of the likelihood and minimising over the
//!   covariance parameters θ (Bates et al. 2015, *J. Stat. Softw.* 67(1)).
//!   Fixed effects get t and F tests with Satterthwaite's denominator degrees
//!   of freedom (Satterthwaite 1946; Kuznetsova et al. 2017).
//! * **Other families** maximise the Laplace approximation to the marginal
//!   likelihood, or, with a single random intercept, adaptive Gauss–Hermite
//!   quadrature (Pinheiro & Bates, *J. Comput. Graph. Stat.* 4, 1995). The
//!   conditional modes come from penalized iteratively reweighted least
//!   squares. A first pass optimises θ with β solved alongside the random
//!   effects; a second optimises θ and β together. Fixed effects get Wald z
//!   tests from the curvature of the approximate likelihood.
//!
//! The numerical core eliminates the random-effects term with
//! the most effects level by level, so a model with thousands of levels of
//! one grouping column stays cheap.

mod dense;
pub mod family;
pub mod fit;
pub mod nested;
pub mod ordinal;
mod optimize;
pub mod shelf_life;
mod system;

pub use family::MixedFamily;
pub use fit::{
    ConditionalModes, ContrastTest, DfMethod, FixedEffect, MixedFit, MixedModel, MixedOptions, RandomCorrelation,
    RandomTerm, ResidualGroups, RobustSummary, ScaleParameter, VarianceComponent, VarianceTest, fit_mixed,
    satterthwaite_limits,
};
pub use ordinal::category_probabilities;
pub use shelf_life::{RandomBatchOptions, RandomBatchShelfLife, random_batch_shelf_life};
pub use nested::{NestedAnova, NestedComparison, NestedEstimate, nested_comparison};
