//! # stat-linear
//!
//! Linear models built on [`stat_core`]: [`one_way_anova`] for comparing group means,
//! [`group_tests`] (Welch, equal-variance and rank tests), [`design`] (continuous and
//! categorical predictors, interaction and polynomial effects, effect tests), [`stepwise`]
//! selection by p-values, AIC or BIC, [`best_subsets`](mod@best_subsets) regression by branch and bound,
//! and ordinary least squares
//! multiple regression ([`fit_linear_model`]) with parameter estimates, standard
//! errors, t-tests, confidence intervals, an ANOVA table, R²/adjusted R²,
//! AIC/AICc/BIC, residual diagnostics and predictions with intervals;
//! [`method_comparison`] (Deming and Passing–Bablok regression) and [`stability`]
//! (shelf life from stability data by ICH Q1E poolability tests); [`nonlinear`]
//! least-squares regression (library forms or any model function) with
//! asymptotic and profile t intervals and delta-method bands; [`manova`]
//! (multivariate analysis of variance: Wilks, Pillai, Hotelling–Lawley and
//! Roy tests of each effect, canonical analysis, contrasts, Box's M).

pub mod anova;
pub mod best_subsets;
pub mod design;
pub mod group_tests;
pub mod hierarchy;
pub mod manova;
pub mod marginal;
pub mod method_comparison;
pub mod nonlinear;
pub mod ols;
pub mod spline;
pub mod stability;
pub mod stepwise;

pub use anova::{GroupSummary, OneWayAnova, one_way_anova};
pub use best_subsets::{BestSubsetsOptions, BestSubsetsResult, SubsetCriterion, SubsetModel, best_subsets};
pub use design::{
    CategoricalCoding, EffectTest, MAX_POWER, ModelDesign, ModelEffect, ModelTerm, Predictor, PredictorValues,
    TermKind, TermValue, effect_name, effect_tests, main_effects, natural_order, spline_effect_name,
};
pub use group_tests::{
    ChiSquareTest, FTest, MeanDifference, RankSumTest, TwoSampleTests, VarianceTests, kruskal_wallis, rank_sum_test,
    two_sample_t_tests, variance_tests, welch_anova,
};
pub use hierarchy::{hierarchical_closure, hierarchy_violations, parent_indices};
pub use method_comparison::{
    ComparisonEstimate, DemingFit, LinearityTest, PassingBablokFit, deming_regression, passing_bablok,
};
pub use nonlinear::forms::{CurveForm, FormModel};
pub use nonlinear::{
    ModelFunction, NonlinearFit, NonlinearOptions, NonlinearParameter, ProfileInterval, fit_nonlinear, numeric_gradient,
};
pub use ols::{
    AnovaRow, AnovaTable, FitSummary, INTERCEPT_NAME, LinearModelFit, OlsOptions, ParameterEstimate, Prediction,
    ResidualDiagnostics, fit_linear_model,
};
pub use spline::{SplineBasis, SplineKind, SplineKnots, SplineSpec};
pub use stability::{
    BatchLine, ExtrapolationRule, PoolingModel, PoolingTest, ShelfLifeOptions, ShelfLifeResult, StabilityTimeUnit,
    shelf_life,
};
pub use stepwise::{
    Heredity, StepAction, StepCriterion, StepDirection, StepRecord, StepwiseOptions, StepwiseResult,
    stepwise_selection, stepwise_selection_with_parents,
};
