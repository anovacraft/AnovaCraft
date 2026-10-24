//! # app-model
//!
//! Everything the desktop application does that is not drawing pixels:
//!
//! * [`glm_analysis`] — generalized linear models: binomial (logit, probit, complementary log-log),
//!   Poisson, normal and Gamma.
//! * [`categorical_logistic`] — ordinal (proportional-odds) and nominal (baseline-category) logistic
//!   regression for responses with more than two categories.
//! * [`dose_response_analysis`] — probit (dose–response) analysis with effective doses and fiducial limits.
//! * [`treatment_effects_analysis`] — treatment effects from observational data: propensity scores,
//!   covariate balance, matching, inverse-probability and doubly robust weighting, subgroup effects.
//! * [`nonlinear_analysis`] — nonlinear regression (library forms, a model formula with named parameters, a
//!   differential equation or an implicit curve; global fits, weights, robust fits, comparisons).
//! * [`method_comparison_analysis`] — Deming and Passing–Bablok regression for method comparison.
//! * [`shelf_life_analysis`] — shelf life from stability data (ICH Q1E poolability tests).
//! * [`multiway_table`] — counts, percentages and summary statistics by nested row and column classifications.
//! * [`scatter3d`] — 3-D scatter plots of three columns with colour, size and shape channels.
//! * [`data_surface`] — contour and surface plots of a response over two columns (triangulation or
//!   thin-plate spline through scattered data).
//! * [`stratified_analysis`] — stratified contingency tables: Cochran–Mantel–Haenszel tests, the
//!   Mantel–Haenszel common odds ratio, Breslow–Day homogeneity and Fisher's exact test for r × c tables.
//! * [`regularized`] — lasso, ridge and elastic net regression with cross-validation.
//! * [`linear_analysis`] — run a linear model on a table, honouring row exclusion,
//!   and produce report sections.
//! * [`control_chart`] — Shewhart control charts (including moving-average,
//!   three-way, zone and short-run charts) with run-rule flags, linked to
//!   dataset rows.
//! * [`gauge_analysis`] — ANOVA gauge R&R from part / operator / measurement
//!   columns.
//! * [`attribute_agreement`] — attribute agreement studies (kappas, concordance) and go/no-go gauge bias and
//!   repeatability.
//! * [`gauge_studies`] — gauge study run sheets, bias and linearity, single-gauge repeatability (Cg, Cgk)
//!   and intraclass-correlation evaluation.
//! * [`robust_design`] — orthogonal-array (inner × outer) experiments and their signal-to-noise analysis.
//! * [`declared_design`] — an existing table declared as a design: factors, ranges, runs and supported models.
//! * [`design_simulation`] — simulated responses for a design from random coefficients with effect sparsity, hierarchy and heredity.
//! * [`design_evaluation`] — evaluating a design before data collection: power, prediction variance, fraction of design space, aliasing, efficiencies, comparing two designs.
//! * [`factorial_analysis`] — effects, normal plots, Lenth's method, main-effect, interaction and cube plots
//!   and dispersion effects of a two-level factorial experiment.
//! * [`distribution`] — one column's summary statistics, quantiles, robust
//!   estimates, histogram and box plot (or frequencies for categorical data).
//! * [`bivariate`] — correlations or a contingency analysis for two columns.
//! * [`multivariate`] — principal components, clustering and correspondence analysis.
//! * [`correlation_analysis`] — correlation matrices with intervals, partial correlations and a heat map.
//! * [`manova_analysis`] — multivariate analysis of variance of several responses.
//! * [`outlier_analysis`] — Mahalanobis, jackknifed and T² outlier distances by row.
//! * [`item_analysis`] — reliability of a multi-item scale (Cronbach's alpha, item statistics).
//! * [`factor_analysis`] — exploratory factor analysis: extraction, rotation, loadings, factor scores.
//! * [`discriminant_analysis`] — linear and quadratic discriminant analysis with canonical functions.
//! * [`gaussian_mixture_analysis`] — Gaussian mixture clustering with BIC model choice.
//! * [`latent_class_analysis`] — latent class analysis of categorical items with BIC model choice.
//! * [`text_analysis`] — term frequencies, word clouds, semantic dimensions, topic models and sentiment of a text column.
//! * [`word_cloud`] — word cloud layout.
//! * [`embedding_analysis`] — maps of rows: classical and metric scaling, t-SNE, UMAP.
//! * [`imputation_analysis`] — missing-value patterns and imputation (column centre, multivariate
//!   normal by EM, low-rank SVD), saved as new columns.
//! * [`multiple_imputation`] — multiple imputation (chained equations, multivariate normal data
//!   augmentation) of continuous and categorical columns, with the stacked table it saves.
//! * [`pooling`] — reports on a stacked imputation table pooled across imputations by Rubin's rules.
//! * [`attribute_chart`] — p, np, c, u, Laney p′/u′, g and t charts from count columns.
//! * [`capability_analysis`] — process capability against specification limits,
//!   with a one-page summary layout and bootstrap intervals.
//! * [`pca_chart`] — T² and SPE charts from a principal-components model, with contributions and a change point.
//! * [`monitoring_limits`] — environmental monitoring alert and action levels by location, with early-warning rules.
//! * [`multivariate_chart`] — Hotelling T², generalized variance and MEWMA charts of several columns.
//! * [`anom_analysis`] — analysis of means for means, proportions and rates, one- and two-way.
//! * [`stepwise_analysis`] — stepwise selection of linear-model effects.
//! * [`mixture_analysis`] — Scheffé mixture models, blending tests and ternary contour plots.
//! * [`survival`] — Kaplan–Meier curves, log-rank test, Weibull and lognormal fits.
//! * [`reliability_growth`] — power-law (Crow–AMSAA) reliability growth, by phase.
//! * [`recurrence`] — repairable systems: mean cumulative function, process fits, trend tests.
//! * [`competing_risks`] — competing risks: cumulative incidence, Gray's test, Fine–Gray regression,
//!   failure modes combined into a series system.
//! * [`warranty`] — warranty returns: counts to censored life data, fits and return forecasts.
//! * [`time_series_data`] — equally spaced series from a table: rows, time stamps, seasonal period.
//! * [`series_explore`] — Series and Signals ▸ Explore: series description (ACF, PACF, unit-root tests,
//!   spectrum), decomposition, trend models and cross-correlation.
//! * [`signal_analysis`] — Series and Signals ▸ Signals and spectra: Fourier spectra, digital filters, convolution and
//!   correlation, and wavelets, on evenly spaced signals (resampled when uneven) by group.
//! * [`intensity_map`] — time–frequency maps (spectrograms, scalograms) as coloured cells.
//! * [`functional_analysis`] — functional data: curves from long data smoothed in a basis, registration,
//!   functional principal components and a model of the scores with a predicted-curve profiler.
//! * [`forecasting`] — shared parts of the time-series forecasting reports: the series and its time
//!   stamps, forecast plot and table, residual diagnostics, accuracy, the saved forecast table.
//! * [`arima_analysis`] — seasonal ARIMA, orders chosen or searched by AICc, optional Box–Cox.
//! * [`smoothing_analysis`] — exponential smoothing (ETS) models, chosen or selected by AICc.
//! * [`test_plan`] — demonstration, estimation and accelerated life test plans.
//! * [`proportional_hazards_analysis`] — Cox proportional hazards regression with strata, hazard ratios,
//!   Schoenfeld-residual checks and predicted survival.
//! * [`survival_regression_analysis`] — accelerated failure time models with censored and interval data.
//! * [`time_weighted_chart`] — EWMA and CUSUM control charts of one column.
//! * [`surface_analysis`] — second-order response surfaces: stationary point, canonical and ridge analysis, contours.
//! * [`compare`] — one-sample, independent-group and paired comparisons, from
//!   stacked (value + group column) or side-by-side data.
//! * [`counts`] — proportion and event-rate tests.
//! * [`goodness_analysis`] — chi-square goodness of fit of counts to given proportions or a Poisson distribution.
//! * [`multiple_response_analysis`] — check-all-that-apply columns: item frequencies, cross-tab by group and per-item tests.
//! * [`crossover_analysis`] — 2×2 crossover trials: treatment, period and carryover effects and equivalence.
//! * [`factorial_repeated`] — two- and three-way repeated-measures ANOVA with sphericity corrections and simple effects.
//! * [`nested_analysis`] — nested t test and nested one-way ANOVA (REML with Satterthwaite DF, classical nested ANOVA).
//! * [`distribution_calculator`] — probabilities, quantiles and random draws for any catalogued distribution.
//! * [`power_analysis`] — power, sample size and detectable difference.
//! * [`prediction`] — validation roles, fit measures by role and saved prediction columns for predictive models.
//! * [`forest_analysis`] — random forests with out-of-bag error and permutation importance.
//! * [`pls_analysis`] — partial least squares regression and discriminant analysis.
//! * [`mars_analysis`] — multivariate adaptive regression splines.
//! * [`neural_analysis`] — neural networks with one or two hidden layers.
//! * [`gaussian_process_analysis`] — Gaussian-process regression with
//!   prediction intervals.
//! * [`sequential_optimization`] — the next runs of an experiment by
//!   expected improvement or an upper confidence bound.
//! * [`model_search`] — every applicable predictive model fitted and ranked.
//! * [`svm_analysis`] — support vector classification and regression.
//! * [`boost_analysis`] — gradient-boosted trees with early stopping on validation rows.
//! * [`model_comparison`] — fit measures, ROC, lift, gains and calibration of saved prediction columns.
//! * [`tree_analysis`] — classification and regression trees with pruning chosen on held-out rows.
//! * [`report_run`] — run a saved report request: its title and sections (shared by
//!   the GUI, projects and scripts).
//! * [`report`] — GUI-independent report content: formatted tables and plot data
//!   whose points carry their dataset row.
//! * [`brushing`] — convert box and click gestures into changes to the shared
//!   [`stat_data::RowStateManager`].
//! * [`axis_settings`] — user settings for plot axes, tick positions and labels.
//! * [`plot_style`] — plot styles (settings a user sets on one plot) in saved files.
//! * [`bayes_analysis`], [`bayes_models`] — Bayesian estimation, Bayes factor tests, Bayesian
//!   linear models and ANOVA, and Bayesian generalized linear and hierarchical models.
//! * [`signal_curves`] — smoothing filters, derivatives and integrals of x–y data, and
//!   spectrum analysis (baseline correction, peak finding and multi-peak fitting).
//! * [`plot_view`] — plot limits and the axis-drag gestures (rescale / shift).
//! * [`doe`] — create designed experiments as data tables.
//! * [`editing`] — data edits (cells, rows, columns, types) with undo/redo.
//! * [`external`] — tables from SQL databases and the web, with refresh; credentials never saved.
//! * [`file_formats`] — the file types the application opens and exports.
//! * [`format`](mod@format) — number formatting for reports.
//! * [`project`] — project files: a dataset with its row states and saved
//!   analyses, which re-run identically when the project is reopened.
//! * [`sample`] — built-in sample datasets.
//! * [`recode`] — mapping a column's distinct values to new ones.
//! * [`search`] — finding text in the data grid's cells.
//! * [`settings`] — user preferences (light/dark appearance, text size) stored between sessions.
//!
//! Keeping this logic out of the `egui` crate means it is unit tested, and the
//! GUI layer stays thin.

pub mod acceptance_sampling;
pub mod action;
pub mod anom_analysis;
pub mod area_analysis;
pub mod arima_analysis;
pub mod attribute_agreement;
pub mod attribute_capability;
pub mod attribute_chart;
pub use stat_viz::axis_settings;
pub mod plot_breaks;
pub mod plot_style;
pub mod style_paste;
pub mod bayes_analysis;
pub mod bayes_models;
pub mod best_subsets_analysis;
pub mod between_within_capability;
pub mod bivariate;
pub mod boost_analysis;
pub mod brushing;
pub mod capability_analysis;
pub mod categorical_logistic;
pub mod cause_effect;
pub mod clipboard;
pub mod compare;
pub mod comparison_report;
pub mod competing_risks;
pub mod control_chart;
pub mod correlation_analysis;
pub mod counts;
pub mod crossover_analysis;
pub mod curve_templates;
pub mod dashboard;
pub mod data_surface;
pub mod declared_design;
pub mod degradation;
pub mod design_augment;
pub mod design_simulation;
pub mod design_evaluation;
pub mod discriminant_analysis;
pub mod censored_distribution;
pub mod distance_analysis;
pub mod distribution;
pub mod distribution_calculator;
pub mod distribution_fit_analysis;
pub mod distribution_tests;
pub mod doe;
pub mod dose_response_analysis;
pub mod editing;
pub mod effect_size_sections;
pub mod embedding_analysis;
pub mod external;
pub mod factor_analysis;
pub mod fatigue_analysis;
pub mod factorial_analysis;
pub mod factorial_repeated;
pub mod fdr;
pub mod file_formats;
pub mod files;
pub mod forecasting;
pub mod forest_analysis;
pub mod format;
pub mod formula;
pub mod formula_typeset;
pub mod functional_analysis;
pub mod gauge_analysis;
pub mod gauge_design;
pub mod gauge_studies;
pub mod gaussian_mixture_analysis;
pub mod glm_analysis;
pub mod goodness_analysis;
pub mod graph_json;
pub mod heading_chart;
pub mod heat_map;
pub mod image_plot;
pub mod image_table;
pub mod html_export;
pub mod imputation_analysis;
pub mod intensity_map;
pub mod item_analysis;
pub mod latent_class_analysis;
pub mod life_distribution;
pub mod life_stress;
pub mod linear_analysis;
pub mod manova_analysis;
pub mod many_tests;
pub mod marginal_sections;
pub mod mars_analysis;
pub mod method_comparison_analysis;
pub mod mixed_analysis;
pub mod mixture_analysis;
pub mod model_comparison;
mod model_data;
pub mod model_search;
pub mod monitoring_limits;
pub mod multi_vari;
pub mod multiple_imputation;
pub mod multiple_response_analysis;
pub mod multiple_responses;
pub mod multivariate;
pub mod multivariate_chart;
pub mod multiway_table;
pub mod nested_analysis;
pub mod neural_analysis;
pub mod gaussian_process_analysis;
pub mod sequential_optimization;
pub mod nonlinear_analysis;
pub mod notebook;
pub mod open_data;
pub mod outlier_analysis;
pub mod pareto_chart;
pub mod pca_chart;
pub mod pk_analysis;
pub mod permutation_analysis;
pub mod plot_view;
pub mod pls_analysis;
pub mod pooling;
pub mod power_analysis;
pub mod prediction;
pub mod prediction_formula;
pub mod project;
pub mod project_template;
pub mod proportional_hazards_analysis;
pub mod pvalue_analysis;
pub mod recode;
pub mod recovery;
pub mod recording;
pub mod recurrence;
pub mod regularized;
pub mod reliability_growth;
mod reliability_json;
pub mod report;
pub mod report_columns;
pub mod report_export;
pub mod report_figure;
pub mod report_run;
pub mod report_scope;
pub mod report_transfer;
pub mod resampling;
pub mod robust_design;
pub mod sample;
pub mod scatter3d;
pub mod screening;
pub mod script_text;
pub mod search;
pub mod seed;
pub mod series_explore;
pub mod settings;
pub mod shelf_life_analysis;
pub mod signal_curves;
pub mod signal_analysis;
pub mod image_analysis;
pub mod smoothing_analysis;
pub mod stepwise_analysis;
pub mod stratified_analysis;
pub mod surface_analysis;
pub mod surface_profiler;
pub mod survival;
pub mod survival_regression_analysis;
pub mod svm_analysis;
pub mod table_ops;
pub mod test_plan;
pub mod text_analysis;
pub mod time_series_data;
pub mod time_weighted_chart;
pub mod treatment_effects_analysis;
pub mod tree_analysis;
pub mod warranty;
pub mod web_publish;
pub mod word_cloud;

pub use acceptance_sampling::{AcceptanceAnalysis, AcceptanceRequest, PlanDesign};
pub use anom_analysis::{AnomAnalysis, AnomKind, AnomRequest};
pub use area_analysis::{AreaAnalysis, AreaRequest, CurveTransform, curve_transform};
pub use arima_analysis::{ArimaAnalysis, ArimaOrders, ArimaRequest, TransferRole};
pub use attribute_agreement::{
    AgreementAnalysis, AgreementRequest, AttributeGaugeAnalysis, AttributeGaugeRequest, agreement_categories,
};
pub use attribute_capability::{AttributeCapabilityAnalysis, AttributeCapabilityKind, AttributeCapabilityRequest};
pub use attribute_chart::{AttributeChartAnalysis, AttributeChartRequest, SubgroupSize};
pub use bayes_analysis::{
    BayesEstimationRequest, BayesFactorRequest, BayesPrior, BayesReport, BayesRequest, BayesTestKind,
    EstimatedParameter,
};
pub use bayes_models::{BayesLinearRequest, BayesModelRequest};
pub use signal_curves::{
    CalculusRequest, DerivativeMethod, PeakDetection, PeakFitSettings, ResampleSettings, SignalReport, SignalRequest,
    SignalSmoothingRequest, SpectrumBaseline, SpectrumRequest,
};
pub use best_subsets_analysis::{BestSubsetsAnalysis, BestSubsetsRequest};
pub use between_within_capability::{BetweenWithinAnalysis, BetweenWithinRequest, SubgroupBy};
pub use bivariate::{BivariateAnalysis, BivariateRequest};
pub use boost_analysis::{BoostAnalysis, BoostRequest};
pub use capability_analysis::{BootstrapSettings, CapabilityAnalysis, CapabilityRequest, GroupOrder};
pub use categorical_logistic::{CategoricalLogisticAnalysis, CategoricalLogisticRequest, CategoricalModel};
pub use cause_effect::{CauseEffectAnalysis, CauseEffectDiagram, CauseEffectRequest, CauseNode};
pub use compare::{
    GroupComparison, GroupComparisonRequest, GroupLayout, OneSampleAnalysis, OneSampleRequest, PairedComparison,
    PairedLayout, PairedRequest,
};
pub use comparison_report::ComparisonReport;
pub use competing_risks::{CompetingRisksAnalysis, CompetingRisksRequest};
pub use control_chart::{
    ControlChartAnalysis, ControlChartOptions, ControlChartRequest, LastSubgroups, RuleSet, RunCenter,
};
pub use correlation_analysis::{CorrelationAnalysis, CorrelationRequest};
pub use counts::{ProportionAnalysis, ProportionComparison, ProportionRequest, RateAnalysis, RateRequest};
pub use crossover_analysis::{CrossoverReport, CrossoverRequest};
pub use dashboard::{
    Dashboard, DashboardEdit, DashboardHistory, DashboardId, DashboardResults, DashboardTile, GridArea, SharedSwitcher,
    TileContent, TileId, TileOutput, compute_dashboard,
};
pub use data_surface::{DataSurfaceAnalysis, DataSurfaceOptions, DataSurfaceRequest, SurfaceFit};
pub use declared_design::{DeclaredDesign, DeclaredFactor, DesignDeclarationRequest, split_plot_analysis};
pub use degradation::{DegradationAnalysis, DegradationLayout, DegradationMethod, DegradationRequest};
pub use design_evaluation::{DesignEvaluationAnalysis, DesignEvaluationRequest};
pub use discriminant_analysis::{DiscriminantAnalysis, DiscriminantRequest, PriorChoice};
pub use censored_distribution::{CensorDirection, CensoredDistribution, Censoring};
pub use distance_analysis::{DistanceSpec, PcoaAnalysis, PcoaInput, PcoaRequest};
pub use fatigue_analysis::{FatigueAnalysis, FatigueRequest};
pub use distribution::{DistributionAnalysis, DistributionOptions, DistributionRequest};
pub use pk_analysis::{PkAnalysis, PkDose, PkRequest};
pub use distribution_calculator::{CalculatorReport, CalculatorRequest, ProbabilityRegion, random_column};
pub use distribution_fit_analysis::{DistributionFitAnalysis, DistributionFitRequest};
pub use distribution_tests::{OutlierMethod, OutlierSettings, RunsSettings, ToleranceSettings};
pub use dose_response_analysis::{DoseResponseAnalysis, DoseResponseRequest};
pub use embedding_analysis::{EmbeddingAnalysis, EmbeddingMethod, EmbeddingRequest};
pub use factor_analysis::{FactorAnalysisReport, FactorRequest};
pub use factorial_analysis::{
    FactorialAnalysis, FactorialAnalysisRequest, FactorialResponse, FactorialTerms, MeansSource,
};
pub use factorial_repeated::{FactorialRepeatedReport, FactorialRepeatedRequest};
pub use fdr::FdrMethod;
pub use forecasting::ForecastSettings;
pub use forest_analysis::{ForestAnalysis, ForestRequest};
pub use functional_analysis::{
    FunctionalAnalysis, FunctionalBasis, FunctionalLandmark, FunctionalRequest, FunctionalSmoothing,
};
pub use gauge_analysis::{GaugeAnalysis, GaugeReportOptions, GaugeStudyRequest};
pub use gauge_design::{GaugeExtraFactor, GaugeFactorNesting, GaugeStudyDesign, GaugeStudyKind};
pub use gauge_studies::{
    GaugeWorksheetRequest, IccAnalysis, IccRequest, LinearityAnalysis, LinearityRequest, RepeatabilityAnalysis,
    RepeatabilityRequest, SummaryStat, SummaryStats, create_gauge_worksheet,
};
pub use gaussian_mixture_analysis::{GaussianMixtureAnalysis, GaussianMixtureRequest};
pub use glm_analysis::{GlmAnalysis, GlmRequest};
pub use goodness_analysis::{GoodnessAnalysis, GoodnessModel, GoodnessRequest};
pub use heat_map::HeatMapSpec;
pub use imputation_analysis::{ImputationMethod, ImputationReport, ImputationRequest};
pub use item_analysis::{ItemAnalysisReport, ItemAnalysisRequest};
pub use latent_class_analysis::{LatentClassAnalysis, LatentClassRequest};
pub use life_distribution::{LifeDataLayout, LifeDistributionAnalysis, LifeDistributionRequest};
pub use life_stress::{LifeStressAnalysis, LifeStressRequest, StressColumn};
pub use linear_analysis::{LinearModelAnalysis, LinearModelSpec, Profile, ProfilePanel};
pub use manova_analysis::{ManovaAnalysis, ManovaContrast, ManovaRequest};
pub use many_tests::{ManyTestKind, ManyTestsAnalysis, ManyTestsOrder, ManyTestsRequest};
pub use mars_analysis::{MarsAnalysis, MarsRequest};
pub use method_comparison_analysis::{MethodComparisonAnalysis, MethodComparisonRequest};
pub use mixed_analysis::{MixedAnalysis, MixedRequest, RandomEffectSpec};
pub use mixture_analysis::{MixtureAnalysis, MixtureRequest};
pub use model_comparison::{ModelComparison, ModelComparisonRequest};
pub use model_search::{ModelKind, ModelSearch, ModelSearchRequest};
pub use monitoring_limits::{LevelMethod, LimitsBy, MonitoringAnalysis, MonitoringRequest, TypedLevels};
pub use multi_vari::{MultiVariAnalysis, MultiVariOptions, MultiVariRequest, ReplicateLayout};
pub use multiple_response_analysis::{
    ItemOrder, MultipleResponseAnalysis, MultipleResponseRequest, ResponseSource, split_items,
};
pub use multiple_responses::{
    FactorSetting, FactorVariation, ModelForm, MultipleResponsesAnalysis, MultipleResponsesRequest, Simulation,
    ResponseModel, SimulationSetup, Solution, TermSelection,
};
pub use multivariate::{
    ChooseClusters, ClusterAnalysis, ClusterDistance, ClusterMethod, ClusterRequest, CorrespondenceReport,
    CorrespondenceRequest, PcaAnalysis, PcaRequest, StartingCenters,
};
pub use multivariate_chart::{MultivariateChartAnalysis, MultivariateChartKind, MultivariateChartRequest};
pub use multiway_table::{CellFrequency, CellSummary, MultiwayTableAnalysis, MultiwayTableRequest};
pub use nested_analysis::{NestedReport, NestedRequest};
pub use neural_analysis::{NeuralAnalysis, NeuralRequest};
pub use gaussian_process_analysis::{GaussianProcessAnalysis, GaussianProcessRequest};
pub use sequential_optimization::{ResponseGoal, SequentialAnalysis, SequentialRequest};
pub use nonlinear_analysis::{
    CurveForm, FitMethod, NonlinearAnalysis, NonlinearModel, NonlinearRequest, ParameterSetting, Weighting,
};
pub use outlier_analysis::{OutlierAnalysis, OutlierDistance, OutlierRequest};
pub use pareto_chart::{ParetoAnalysis, ParetoBar, ParetoChartOptions, ParetoChartSpec, ParetoPanel, ParetoRequest};
pub use pca_chart::{PcaChartAnalysis, PcaChartRequest};
pub use permutation_analysis::{PermutationAnalysis, PermutationDesign, PermutationRequest};
pub use pls_analysis::{PlsAnalysis, PlsRequest};
pub use power_analysis::{PowerAnalysis, PowerRequest, PowerSolve};
pub use prediction::ValidationMethod;
pub use project::{
    Project, ProjectFile, ProjectScript, ProjectTable, SavedReport, TableId, checked_report_to_json, load_project,
    load_project_file, load_project_file_noting, load_project_noting, report_from_json, report_to_json, save_project,
    save_project_file,
};
pub use proportional_hazards_analysis::{HazardProfile, ProportionalHazardsAnalysis, ProportionalHazardsRequest};
pub use pvalue_analysis::{PValueAnalysis, PValueRequest};
pub use recurrence::{RecurrenceAnalysis, RecurrenceRequest};
pub use regularized::{RegularizedAnalysis, RegularizedRequest};
pub use reliability_growth::{GrowthAnalysis, GrowthRequest};
pub use report::{
    Annotations, BarChartSpec, BarItem, BoxPlotSpec, ChartPoint, ControlChartSpec, EffectPareto, EffectSource,
    LimitStep, LinePattern, PhaseMark, PlotPane, PlotSpec, PointGroups, Polyline, ReportOptions, ReportSection,
    RiskTable, RowPoint, SectionContent, SectionKind, SurvivalCurveSpec, SurvivalPlotSpec, TextTable, TreeDiagram,
    TreeDiagramNode,
};
pub use report_columns::{ReportColumn, report_columns};
pub use report_run::{ComputedReport, analysis_key, graph_figure, graph_title, run_report};
pub use report_scope::{ReportScope, SwitchSlot, plot_style_key};
pub use resampling::{BootstrapAnalysis, BootstrapRequest};
pub use robust_design::{
    ArrayFactor, RobustDesignRequest, SignalToNoiseAnalysis, SignalToNoiseRequest, create_robust_design,
};
pub use scatter3d::{ColorChannel, ColorValue, Scatter3dAnalysis, Scatter3dOptions, Scatter3dPoint, Scatter3dRequest};
pub use screening::{ScreeningAnalysis, ScreeningModel, ScreeningOrder, ScreeningRequest};
pub use series_explore::{
    AdfDeterministic, BoxCoxChoice, CrossSettings, DecomposeMethod, DecomposeSettings, DescribeSettings, ExploreMethod,
    Prewhiten, SeriesExploreAnalysis, SeriesExploreRequest, TrendChoice, TrendSettings,
};
pub use shelf_life_analysis::{ShelfLifeAnalysis, ShelfLifeRequest};
pub use signal_analysis::{SignalAnalysis, SignalInput, SignalMethod, SignalProcessingRequest};
pub use image_analysis::{ImageAnalysis, ImageAnalysisRequest, ImageInput, ImageMethod};
pub use smoothing_analysis::{FixedSmoothing, SmoothingAnalysis, SmoothingModel, SmoothingRequest};
pub use stat_core::distribution_fit::FitFamily;
pub use stat_discrete::treatment_effects::{
    EffectMethod, EffectScale, Estimand, IntervalMethod, MatchingOptions, OutcomeKind as TreatmentOutcome,
};
pub use stat_discrete::{DoseScale, Family, Link};
pub use stat_doe::ScheffeModel;
pub use stat_doe::{Desirability, Goal};
pub use stat_inference::Alternative;
pub use stat_inference::area::AreaBaseline;
pub use stat_inference::best::BestDirection;
pub use stat_inference::multiple::ComparisonMethod;
pub use stat_inference::outliers::DixonRatio;
pub use stat_inference::power::PowerDesign;
pub use stat_inference::pvalues::AdjustMethod;
pub use stat_inference::tolerance::ToleranceSides;
pub use stat_linear::StabilityTimeUnit;
pub use stat_linear::SubsetCriterion;
pub use stat_linear::TermValue;
pub use stat_linear::hierarchy;
pub use stat_linear::manova::MultivariateStatistic;
pub use stat_linear::marginal::PairAdjust;
pub use stat_linear::spline::{MAX_DEGREE as MAX_SPLINE_DEGREE, MAX_KNOTS as MAX_SPLINE_KNOTS};
pub use stat_linear::{Heredity, StepCriterion, StepDirection};
pub use stat_linear::{SplineKind, SplineKnots, SplineSpec};
pub use stat_mixed::{DfMethod, MixedFamily};
pub use stat_multivariate::MissingRows;
pub use stat_multivariate::{CovarianceStructure, MixtureStart};
pub use stat_multivariate::{DiscriminantRule, Extraction, Rotation, ScoreMethod};
pub use stat_multivariate::distance::DistanceMetric;
pub use stat_multivariate::pcoa::NegativeCorrection;
pub use stat_multivariate::{GapReference, Linkage};
pub use stat_predictive::{Criterion, PlsAlgorithm, RegressionLoss};
pub use stat_quality::AttributeChartKind;
pub use stat_quality::{
    BetweenEstimate, MewmaLimit, OcModel, RiskPoints, SamplingPlan, SigmaKnowledge, WithinEstimate,
};
pub use stat_quality::{CapabilityModel, SpecLimits, WithinSigma};
pub use stat_reliability::BandTransform;
pub use stat_reliability::LifeDistribution;
pub use stat_reliability::degradation::{FailureDirection, PathModel, ScaleTransform};
pub use stat_reliability::growth::BetaEstimator;
pub use stat_reliability::life_regression::RegressionFamily;
pub use stat_reliability::fatigue::SnForm;
pub use stat_inference::nca::{Administration, AucMethod, LambdaZRule};
pub use stat_reliability::life_stress::{StressTransform, TemperatureUnit};
pub use stat_reliability::repairable::{McfLimits, McfVariance};
pub use stat_reliability::{AftDistribution, TieMethod, TimeTransform};
pub use stat_timeseries::smoothing::{ErrorType, EtsSpec, SeasonalType, TrendType};
pub use stat_viz::{FilterCondition, GraphSpec};
pub use stepwise_analysis::{StepwiseAnalysis, StepwiseRequest};
pub use stratified_analysis::{StratifiedAnalysis, StratifiedRequest};
pub use surface_analysis::{SurfaceAnalysis, SurfaceOptions, SurfaceRequest};
pub use surface_profiler::{
    Camera, ProfileFactor, ProfileFactorKind, ProfilerOptions, SurfaceMesh, SurfaceModel, mesh_from_grid, surface_mesh,
};
pub use survival::{StatusCoding, SurvivalAnalysis, SurvivalOptions, SurvivalRequest};
pub use survival_regression_analysis::{SurvivalRegressionAnalysis, SurvivalRegressionRequest};
pub use svm_analysis::{SvmAnalysis, SvmKernel, SvmRequest};
pub use test_plan::{
    AcceleratedRequest, DemonstrationRequest, DemonstrationSolve, EstimationRequest, StressRelationship,
    TestPlanAnalysis, TestPlanRequest,
};
pub use text_analysis::{
    SentimentAnalysis, SentimentRequest, TextExploration, TextExplorationRequest, TextPreparation, TopicModelAnalysis,
    TopicModelRequest,
};
pub use time_series_data::{Series, SeriesInput};
pub use time_weighted_chart::{TimeWeightedAnalysis, TimeWeightedKind, TimeWeightedRequest};
pub use treatment_effects_analysis::{SubgroupEstimate, TreatmentEffectsAnalysis, TreatmentEffectsRequest};
pub use tree_analysis::{Pruning, TreeAnalysis, TreeRequest};
pub use warranty::{WarrantyAnalysis, WarrantyLayout, WarrantyRequest};
pub use word_cloud::WordCloudSpec;
