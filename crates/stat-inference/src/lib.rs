//! # stat-inference
//!
//! Hypothesis tests and confidence intervals for comparing samples, and the
//! power and sample-size calculations that go with them. Every routine follows
//! a published definition (cited in each module) and reports its test as a
//! [`TestResult`]: estimate, statistic, degrees of freedom, p-value for the
//! chosen [`Alternative`], and a confidence interval matching that alternative.
//!
//! * [`means`] — one-sample t and z, paired t, pooled and Welch two-sample t,
//!   standardized effect sizes.
//! * [`equivalence`] — two one-sided tests (TOST) for equivalence of means,
//!   differences or ratios.
//! * [`crossover`] — the 2×2 crossover: treatment, period and carryover
//!   effects and average equivalence on the difference or ratio scale.
//! * [`nonparametric`] — sign test, Wilcoxon signed-rank and rank-sum tests
//!   (exact or normal), Hodges–Lehmann estimates, Friedman test.
//! * [`kolmogorov`] — the two-sample Kolmogorov–Smirnov test, exact or
//!   asymptotic.
//! * [`variances`] — chi-square test of one standard deviation, F test of a
//!   variance ratio.
//! * [`proportions`] — exact binomial and score tests, two-proportion z and
//!   Fisher exact tests, relative risk and odds ratio, McNemar's test.
//! * [`goodness`] — chi-square goodness of fit of counts to given proportions
//!   or a Poisson distribution.
//! * [`rates`] — exact Poisson tests of one rate and of a rate ratio.
//! * [`multiple`] — Tukey–Kramer, Games–Howell, Bonferroni, Šidák, Holm,
//!   Holm–Šidák, Fisher LSD, Newman–Keuls, Tamhane T2, Dunnett T3 and Dunn
//!   comparisons of all pairs of groups, and the linear-trend contrast.
//! * [`control`] — Dunnett's comparisons of treatments with a control.
//! * [`best`] — Hsu's multiple comparisons with the best.
//! * [`anom`] — analysis of means for normal, binomial and Poisson data and
//!   two-way layouts, with exact critical values (Nelson 1982).
//! * [`tolerance`] — normal and distribution-free tolerance intervals.
//! * [`outliers`] — Grubbs', Dixon's and the generalized ESD outlier tests.
//! * [`runs`] — Mood's median test, runs above and below a value, runs up and
//!   down, and the run-chart tests built on them.
//! * [`repeated`] — repeated-measures (within-subject) ANOVA, one-way and
//!   factorial with between-subject factors; Mauchly's test of sphericity
//!   and Greenhouse–Geisser and Huynh–Feldt corrections.
//! * [`area`] — trapezoid area under a sampled curve above a baseline, with
//!   Bailer's standard error from replicates, and the peaks above it.
//! * [`nca`] — non-compartmental pharmacokinetics of a concentration–time
//!   profile: Cmax, AUC and AUMC (linear or linear-up/log-down), terminal
//!   λz by best-fit adjusted R², half-life, AUC∞, MRT, clearance and volume;
//!   geometric summaries across subjects.
//! * [`bootstrap`] — bootstrap resampling (optionally stratified, seeded per
//!   replicate, in parallel), the jackknife, bootstrap standard error and
//!   bias, percentile and BCa intervals.
//! * [`permutation`] — permutation tests for two groups, paired data and
//!   correlation, exact or by seeded Monte Carlo.
//! * [`pooling`] — Rubin's rules for multiply imputed data: pooled
//!   estimates with Barnard–Rubin degrees of freedom, the D₁ (Wald) and D₃
//!   (likelihood-ratio) tests of several parameters, pooled correlations
//!   and R² on Fisher's z scale.
//! * [`pvalues`] — multiple-testing adjustment (Bonferroni, Šidák, Holm,
//!   Hochberg, Benjamini–Hochberg, Benjamini–Yekutieli, Storey q-values) and
//!   Fisher's and Stouffer's combined tests.
//! * [`effect_sizes`] — η², partial η², ω² and Cohen's f with noncentral-F
//!   intervals; φ and Cramér's V with noncentral-χ² intervals.
//! * [`power`] — power, sample size and detectable difference for the t, F,
//!   proportion, variance, rate and equivalence tests.

pub mod anom;
pub mod area;
pub mod best;
pub mod bootstrap;
pub mod common;
pub mod control;
pub mod crossover;
pub mod effect_sizes;
pub mod equivalence;
pub mod goodness;
pub mod kolmogorov;
pub mod means;
pub mod multiple;
pub mod nca;
pub mod nonparametric;
pub mod outliers;
pub mod permutation;
pub mod pooling;
pub mod power;
pub mod proportions;
pub mod pvalues;
pub mod rates;
pub mod repeated;
pub mod runs;
pub mod tolerance;
pub mod variances;

pub use common::{Alternative, ChiSquareResult, TestResult};
pub use nca::{
    Administration, AucMethod, GeometricSummary, LambdaZRule, NcaResult, NcaSettings, TerminalPhase, geometric_summary,
    non_compartmental,
};
