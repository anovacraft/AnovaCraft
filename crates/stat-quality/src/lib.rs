//! # stat-quality
//!
//! Statistical quality control:
//!
//! * [`constants`] — control chart constants (d₂, d₃, c₄ and the A/B/D factors),
//!   derived from their definitions.
//! * [`charts`] — Shewhart X̄ & R, X̄ & S, and Individuals & Moving Range charts.
//! * [`time_weighted`] — EWMA and tabular CUSUM charts for small sustained shifts.
//! * [`phased`] — variables charts with phases and subgroups of varying size.
//! * [`extended`] — moving-average, three-way (between/within), zone and short-run Z charts.
//! * [`rules`] — Nelson's eight run rules for special-cause detection.
//! * [`attribute`] — p, np, c and u charts, Laney p′ and u′ charts for
//!   overdispersed counts, and the rare-event g and t charts.
//! * [`capability`] — process capability indices (Cp, Cpk, Pp, Ppk, Cpm) with intervals and nonconforming rates.
//! * [`nonnormal`] — percentile-based capability of non-normal processes (fitted distributions, Johnson,
//!   sample), with bootstrap percentile intervals.
//! * [`attribute_capability`] — binomial (fraction defective) and Poisson (defects per unit) capability with exact intervals.
//! * [`between_within`] — capability of several batches or streams with between- and within-subgroup σ separated.
//! * [`acceptance`] — acceptance sampling: single, double and variables plans from risk points; OC, AOQ, ATI and ASN.
//! * [`gauge`] — measurement systems analysis: ANOVA gauge R&R.
//! * [`gauge_nested`] — nested (destructive-test) gauge R&R: parts within operators.
//! * [`gauge_expanded`] — expanded gauge R&R with extra fixed or random factors, crossed or nested,
//!   unbalanced data, by REML.
//! * [`gauge_worksheet`] — randomized run sheets for crossed or nested gauge studies.
//! * [`gauge_bias`] — bias and linearity against reference values; single-gauge repeatability (Cg, Cgk).
//! * [`icc`] — Shrout & Fleiss intraclass correlations and Wheeler's evaluation of a measurement process.
//! * [`pareto`] — Pareto ranking of defect categories with cumulative shares and an "Other" pool.
//! * [`multivari`] — multi-vari breakdown of a response by nested or crossed factors.
//! * [`multivariate`] — Hotelling T² (subgroups or individuals, reference and monitoring limits) with
//!   variable contributions, the generalized variance |S| chart and the MEWMA chart.
//! * [`pca_chart`] — T² and squared prediction error (SPE) charts from a principal-components model of a
//!   reference period, with cross-validated components, contributions and a mean change-point estimate.
//! * [`monitoring_limits`] — environmental monitoring alert and action limits from the best-fitting count,
//!   positive-sample or continuous family, with early-warning rules.
//! * [`attribute_agreement`] — agreement of appraisers rating parts into categories: percent agreement
//!   with exact intervals, Cohen's (weighted) and Fleiss' kappa, Kendall's W and τ_b.
//! * [`attribute_gauge`] — bias and repeatability of a go/no-go gauge from acceptance counts (AIAG analytic method).

pub mod acceptance;
pub mod attribute;
pub mod attribute_agreement;
pub mod attribute_capability;
pub mod attribute_gauge;
pub mod between_within;
pub mod capability;
pub mod charts;
pub mod constants;
pub mod extended;
pub mod gauge;
pub mod gauge_bias;
pub mod gauge_expanded;
pub mod gauge_nested;
pub mod gauge_worksheet;
pub mod icc;
pub mod monitoring_limits;
pub mod multivari;
pub mod multivariate;
pub mod nonnormal;
pub mod pareto;
pub mod pca_chart;
pub mod phased;
pub mod rules;
pub mod time_weighted;

pub use acceptance::{
    OcModel, PlanContext, PlanPoint, RiskPoints, SamplingPlan, SigmaKnowledge, double_plan, single_plan, variables_plan,
};
pub use attribute::{AttributeChart, AttributeChartKind, AttributePoint, attribute_chart};
pub use attribute_agreement::{
    AgreementOptions, AgreementRatings, AttributeAgreement, BetweenAppraisers, Concordance, FleissKappa, KappaEstimate,
    KappaWeights, KendallTau, PairwiseKappa, PercentAgreement, VersusStandard, WithinAppraiser, attribute_agreement,
    category_kappa, cohen_kappa, fleiss_kappa, kendall_tau_b, kendall_w,
};
pub use attribute_capability::{BinomialCapability, PoissonCapability, binomial_capability, poisson_capability};
pub use attribute_gauge::{
    AcceptanceEstimate, AcceptancePoint, AttributeGaugeStudy, LimitSide, adjusted_acceptance, attribute_gauge_study,
};
pub use between_within::{
    BetweenEstimate, BetweenWithin, BetweenWithinOptions, WithinEstimate, between_within_capability,
};
pub use capability::{Capability, CapabilityIndices, CapabilityOptions, SpecLimits, WithinSigma, process_capability};
pub use charts::{
    ChartKind, ChartPair, ChartStatistic, ControlChart, ControlLimits, SubgroupUse, consecutive_subgroups,
    individuals_chart, mean_and_range_chart, mean_and_sd_chart,
};
pub use constants::ChartConstants;
pub use extended::{PartSummary, ZONE_SIGNAL, ZONE_WEIGHTS, ZoneScore, zone_scores};
pub use gauge::{GaugeObservation, GaugeOptions, GaugeSource, GaugeStudy, gauge_rr_anova};
pub use gauge_bias::{
    BiasTest, Drift, Hysteresis, LineCoefficient, LinearityStudy, ReferenceBias, RepeatabilityOptions,
    RepeatabilityStudy, bias_linearity, drift, hysteresis, repeatability_study,
};
pub use gauge_expanded::{
    ComponentRole, ExpandedComponent, ExpandedGaugeOptions, ExpandedGaugeStudy, ExpandedTotal, FixedTermTest,
    GaugeFactor, gauge_rr_expanded,
};
pub use gauge_nested::gauge_rr_nested;
pub use gauge_worksheet::{GaugeLayout, RunOrder, WorksheetPlan, WorksheetRun, gauge_worksheet};
pub use icc::{
    IccEstimate, IccForm, IncrementVerdict, MeasurementEvaluation, MonitorClass, ShroutFleiss, decimal_increment,
    evaluate_measurement, shrout_fleiss,
};
pub use monitoring_limits::{
    Candidate, CountFamily, CountFit, MonitoringFit, ReadingKind, WarningRules, best_candidate, fit_candidates,
    fit_counts, fit_positives, warning_rules,
};
pub use multivari::{CellRangeLimits, FactorVariation, MAX_MULTI_VARI_FACTORS, MultiVari, MultiVariCell, multi_vari};
pub use multivariate::{
    ChartPhase, GeneralizedVarianceChart, HotellingChart, MewmaChart, MewmaLimit, MultivariatePoint,
    generalized_variance_chart, hotelling_t2_chart, mewma_chart, mewma_limit_for_arl, t2_contributions,
};
pub use nonnormal::{
    BootstrapIntervals, CapabilityModel, PercentileCapability, PercentileSource, REFERENCE_PROBABILITIES,
    bootstrap_percentile_intervals, percentile_capability,
};
pub use pareto::{ParetoCategory, ParetoOptions, ParetoTable, pareto_table};
pub use pca_chart::{
    ChangePoint, ComponentChoice, PcaChart, PcaChartOptions, SpeLimitMethod, mean_change_point, pca_chart,
    pca_cross_validation, pca_t2_limits, spe_limit,
};
pub use phased::{
    ChartSettings, PhaseSummary, PhasedChart, StandardValues, VaryingChart, phased_chart, phased_chart_with,
    varying_violations, varying_violations_at,
};
pub use rules::{RuleViolation, RunRule, check_rules};
pub use time_weighted::{
    CusumChart, CusumPoint, EwmaChart, EwmaPoint, ProcessTarget, cusum_chart, ewma_chart, moving_range_sigma,
};
