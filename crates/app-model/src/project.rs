//! Project files: a dataset together with its row states and the analyses run
//! on it, so that reopening the file brings every report back.
//!
//! # Format
//! A project is a UTF-8 JSON document (extension `.acraft`):
//!
//! ```text
//! {
//!   "format": "statistical-engine-project",
//!   "version": 1 | 2,
//!   "table": {
//!     "name": "…",
//!     "columns": [ {"name": "…", "type": "numeric" | "text" | "datetime",
//!                   "level": "continuous" | "nominal" | "ordinal",
//!                   "value_labels": [["1", "Treatment"], …],     (optional)
//!                   "hover": true,                                (optional)
//!                   "values": [ … ]}, … ]
//!   },
//!   "rows": { "excluded": [row, …], "hidden": [row, …], "selected": [row, …],
//!             "colors": [[row, "#rrggbb"], …],
//!             "markers": [[row, "square"], …],                   (optional)
//!             "style_column": "…",                               (optional)
//!             "subsets": [{"name": "…", "rows": [row, …]}, …],   (optional)
//!             "active_subset": "…" },                            (optional)
//!   "reports": [ {"analysis": "linear-model", "response": "y", …,
//!                 "options": {…}}, … ],
//!   "origin": {"operation": "stack", "sources": ["…"], …}   (optional)
//!   "scripts": [ {"name": "…", "source": "…", "run_on_open": true}, … ]
//!                                                           (optional)
//! }
//! ```
//!
//! Numbers are written in shortest round-trip form and missing numeric values
//! as `null`, so the data reload bit for bit.
//!
//! A `"datetime"` column (version 2) stores its values as numbers of seconds
//! since 1970-01-01T00:00:00 UTC (`null` when missing), exactly like a numeric
//! column, plus how they are shown: `"format"` (`"date"`, `"datetime"`,
//! `"time"` or `"duration"`), `"fraction_digits"` (0–6) and `"offset_minutes"`
//! (the display offset from UTC). A project is written as version 1 unless it
//! has a date-time column, so files without dates still open in builds that
//! predate version 2; version 1 files always load. Reports store what was asked for
//! (columns by name, settings, display options), not results: on loading, each
//! analysis is re-run on the restored data and row states, which reproduces the
//! report exactly because every analysis is deterministic.
//!
//! `"origin"`, present only for a table made by a table operation, records
//! the operation and its options ([`crate::table_ops::TableOrigin`]). It is
//! optional, so it does not change the format version.
//!
//! `"external_source"`, present only for a table read from a database or the
//! web, records the connection and query, or the URL and headers, so the
//! table can be refreshed ([`crate::external::ExternalSource`]). It never
//! holds a password or a secret header value. Also optional.
//!
//! `"scripts"`, present only when the project carries scripts, holds Rhai
//! scripts ([`ProjectScript`]) kept with the data. Those marked
//! `"run_on_open"` are offered to run when the project is opened; the
//! application asks before running them. Also optional.
//!
//! # Several tables (version 3)
//! A project holding more than one table, dashboards or a window layout is written by
//! [`save_project_file`] in a second form, version 3:
//!
//! ```text
//! {
//!   "format": "statistical-engine-project",
//!   "version": 3,
//!   "tables": [
//!     { "id": "table-1",
//!       "table": {…}, "rows": {…}, "reports": [ … ],   (as in the single-table form)
//!       "origin": {…},                                 (optional, as above)
//!       "source_tables": [ {"id": "table-2", "fingerprint": "89ab…"} | null, … ],
//!                                                      (with "origin" only)
//!       "external_source": {…} },                      (optional, as above)
//!     …
//!   ],
//!   "scripts": [ … ],                                  (optional, as above)
//!   "name": "…",                                       (optional: the name the user gave the
//!                                                      project, shown instead of the file's)
//!   "notebooks": [                                     (optional; crate::notebook)
//!     { "name": "…",
//!       "cells": [ {"kind": "text" | "code", "source": "…",
//!                   "run": n,                          (optional: run number)
//!                   "outputs": [ {"kind": "printed" | "value" | "note", "text": "…"}
//!                              | {"kind": "error", "text": "…", "line": n}
//!                              | {"kind": "table", "title": "…", "columns": […], "rows": [[…], …]}
//!                              | {"kind": "report", "table": "…", "title": "…"}, … ]}, … ]},
//!     … ],
//!   "folders": [                                       (optional; folders::ProjectFolder)
//!     { "name": "…",
//!       "items": [ {"table": "table-1"} | {"table": "table-1", "report": index}
//!                | {"dashboard": "dashboard-1"} | {"notebook": "…"}, … ],
//!       "folders": [ … ] }, … ],                       (both optional)
//!   "dashboards": [                                    (optional)
//!     { "id": "dashboard-1", "name": "…",
//!       "grid_columns": 12,                            (optional; default 12)
//!       "filters": [ {"column": "…", "levels": ["…", …]}
//!                  | {"column": "…", "min": x, "max": y}, … ],   (optional)
//!       "tiles": [                                     (optional)
//!         { "id": 1, "column": 0, "row": 0, "width": 6, "height": 4,
//!           "table": "table-1", "report": {…} },        (a report tile)
//!         { "id": 2, "column": 6, "row": 0, "width": 6, "height": 1,
//!           "text": "…", "style": "heading" | "body" }, (a text tile; style optional)
//!         … ] },
//!     …
//!   ],
//!   "layout": {                                        (optional; every member optional)
//!     "active_table": "table-1",
//!     "views": [ {"table": "table-1", "mode": "data" | "results" | "side-by-side"}, … ],
//!     "windows": [ {"table": "table-2", "view": "data" | "results" | {"report": index},
//!                   "position": [x, y], "size": [width, height]}, … ],
//!     "active_dashboard": "dashboard-1",
//!     "dashboard_windows": [ {"dashboard": "dashboard-1",
//!                             "position": [x, y], "size": [width, height]}, … ]
//!   }
//! }
//! ```
//!
//! Each table's `"id"` is unique within the file, non-empty, and stays the
//! same across saves (names can change). Date-time columns are allowed in
//! every table (version 3 includes version 2).
//!
//! `"source_tables"` has one entry per name in the origin's `"sources"`, in
//! the same order: the id of that source table within the project and a
//! fingerprint (16 hexadecimal digits of a 64-bit FNV-1a hash; see
//! [`DataFingerprint::of_source`]) of its data when the derived table was
//! made, or `null` when that source is not in the project. Comparing the
//! fingerprints with the sources' current data tells that a source changed
//! since ([`ProjectFile::origin_is_stale`]); [`ProjectFile::rerun_origin`]
//! makes the table again from the current sources.
//!
//! `"dashboards"` ([`crate::dashboard`]) arranges report and text tiles on a
//! grid `"grid_columns"` wide (1–48 columns, rows as needed). Each dashboard
//! `"id"` is unique and non-empty; each tile `"id"` is a positive whole number
//! unique within its dashboard. A tile covers `"width"` × `"height"` cells
//! (each at least 1) from its top-left cell (`"column"`, `"row"`, counted from
//! 0), stays within the grid's columns and never overlaps another tile. A
//! report tile holds its own copy of a report request, in the form of a
//! `"reports"` entry with columns named in its `"table"` (the id of one of
//! the file's tables; a tile naming another table makes the file invalid).
//! `"filters"` holds at most one condition per column name: a list of
//! accepted levels (cell text) or an inclusive range with finite limits. A
//! filter applies to every report tile whose table has a column of that name
//! and is skipped for tables without one; it never changes the tables' own
//! row states. Like reports, tiles store requests, not results: they are
//! computed again when shown ([`crate::dashboard::compute_dashboard`]).
//!
//! `"layout"` is plain data for the application: which table is shown, each
//! table's view mode, windows popped out of the main window (positions and
//! sizes in points, outer frame), the dashboard shown in the main window in
//! place of a table (`"active_dashboard"`) and dashboards in windows of
//! their own (`"dashboard_windows"`). Layout entries naming a table not in
//! the file, a report the table does not have, or a dashboard the file does
//! not have, are ignored on loading.
//!
//! A project with exactly one table, no layout and no dashboards is always
//! written in the single-table form (version 1 or 2, byte for byte what
//! [`save_project`] writes), so builds that predate version 3 still open it.
//! [`load_project_file`] reads both forms; a single-table file becomes a
//! one-table [`ProjectFile`] whose table has the id `table-1`.
//! [`load_project`] also reads a version-3 file holding exactly one table
//! (its dashboards and layout are not part of a [`Project`] and are left out).

use crate::compare::{GroupComparisonRequest, GroupLayout, MedianTies, OneSampleRequest, PairedLayout, PairedRequest};
use crate::counts::{ProportionComparison, ProportionRequest, RateRequest};
use crate::power_analysis::{PowerRequest, PowerSolve};
use crate::table_ops::TableOrigin;
use crate::{
    AttributeChartKind, AttributeChartRequest, CapabilityModel, CapabilityRequest, SpecLimits, SubgroupSize,
    WithinSigma,
};
use crate::{BestSubsetsRequest, SubsetCriterion};
use crate::{
    BivariateRequest, ClusterMethod, ClusterRequest, ControlChartOptions, ControlChartRequest, CorrespondenceRequest,
    DistributionOptions, DistributionRequest, Family, GaugeReportOptions, GaugeStudyRequest, GlmRequest,
    LinearModelSpec, Linkage, PcaRequest, RegularizedRequest, ReportOptions, RuleSet, StatusCoding, SurfaceOptions,
    SurfaceRequest, SurvivalOptions, SurvivalRequest, TimeWeightedKind, TimeWeightedRequest,
};
use crate::{DistributionFitRequest, FitFamily};
use crate::{DixonRatio, OutlierMethod, OutlierSettings, RunsSettings, ToleranceSettings, ToleranceSides};
use crate::{GraphSpec, MixtureRequest, ProfilerOptions, ScheffeModel, StepwiseRequest};
use stat_core::descriptive::QuantileMethod;
use stat_core::{Result, StatError};
use stat_data::{
    Column, ColumnData, DataTable, JsonValue, MeasurementLevel, RowColor, RowMarker, RowStateManager, parse_json,
};
use stat_inference::Alternative;
use stat_inference::multiple::ComparisonMethod;
use stat_inference::power::PowerDesign;
use stat_quality::ChartKind;
use stat_reliability::BandTransform;

mod folders;
mod tables;
pub use folders::{FolderItem, MAX_FOLDER_DEPTH, ProjectFolder};
pub use tables::{
    DashboardWindow, DataFingerprint, PROJECT_FILE_VERSION, PoppedOutWindow, ProjectFile, ProjectLayout, ProjectTable,
    Rerun, SourceSnapshot, TableId, TableLineage, TableView, ViewMode, WindowContent, load_project_file,
    load_project_file_noting, save_project_file,
};

/// Value of the `format` member identifying a project file.
pub const PROJECT_FORMAT: &str = "statistical-engine-project";
/// Newest version of the single-table project form ([`save_project`]).
/// Version 2 added date-time columns. [`load_project`] also reads a
/// one-table file of [`PROJECT_FILE_VERSION`]; newer files are rejected.
pub const PROJECT_VERSION: usize = 2;
/// Project file extension (without the dot).
pub const PROJECT_EXTENSION: &str = "acraft";

/// One saved analysis: what to compute and how to display it.
#[derive(Debug, Clone, PartialEq)]
pub enum SavedReport {
    /// A linear model.
    Linear {
        /// Model columns.
        spec: LinearModelSpec,
        /// Display options.
        options: ReportOptions,
    },
    /// A control chart.
    ControlChart {
        /// Chart settings.
        request: ControlChartRequest,
        /// Display options.
        options: ControlChartOptions,
    },
    /// A gauge R&R study.
    Gauge {
        /// Study settings.
        request: GaugeStudyRequest,
        /// Display options.
        options: GaugeReportOptions,
    },
    /// One column's distribution.
    Distribution {
        /// Column and quantile method.
        request: DistributionRequest,
        /// Display options.
        options: DistributionOptions,
    },
    /// Two columns together.
    Bivariate {
        /// Columns.
        request: BivariateRequest,
    },
    /// Principal component analysis.
    Pca {
        /// Columns and scaling.
        request: PcaRequest,
    },
    /// Cluster analysis.
    Cluster {
        /// Columns and method.
        request: ClusterRequest,
    },
    /// Generalized linear model.
    Glm {
        /// Columns and family.
        request: GlmRequest,
        /// Profilers shown.
        profilers: ProfilerOptions,
    },
    /// Linear or generalized linear mixed model.
    Mixed {
        /// Columns, random effects, family and estimation.
        request: crate::MixedRequest,
        /// Profilers shown.
        profilers: ProfilerOptions,
    },
    /// Several responses on the same factors: linked profiles,
    /// desirability, best settings and simulation.
    MultipleResponses {
        /// Columns, model form, goals, settings and simulation.
        request: Box<crate::multiple_responses::MultipleResponsesRequest>,
    },
    /// Penalized regression.
    Regularized {
        /// Columns and penalty settings.
        request: RegularizedRequest,
        /// Profilers shown.
        profilers: ProfilerOptions,
    },
    /// A classification or regression tree.
    DecisionTree {
        /// Columns, validation, growth limits and pruning.
        request: crate::TreeRequest,
        /// Profilers shown.
        profilers: ProfilerOptions,
    },
    /// A random forest of classification or regression trees.
    RandomForest {
        /// Columns, validation, forest size and tree limits.
        request: crate::ForestRequest,
        /// Profilers shown.
        profilers: ProfilerOptions,
    },
    /// Gradient-boosted trees.
    BoostedTrees {
        /// Columns, validation, rounds and tree settings.
        request: crate::BoostRequest,
        /// Profilers shown.
        profilers: ProfilerOptions,
    },
    /// Partial least squares regression or discriminant analysis.
    PartialLeastSquares {
        /// Columns, validation and factor settings.
        request: crate::PlsRequest,
        /// Profilers shown.
        profilers: ProfilerOptions,
    },
    /// Support vector classification or regression.
    SupportVectorMachine {
        /// Columns, validation, kernel and tuning settings.
        request: crate::SvmRequest,
        /// Profilers shown.
        profilers: ProfilerOptions,
    },
    /// Multivariate adaptive regression splines.
    Mars {
        /// Columns, validation, degree and pruning settings.
        request: crate::MarsRequest,
        /// Profilers shown.
        profilers: ProfilerOptions,
    },
    /// A neural network.
    NeuralNetwork {
        /// Columns, validation, layers, penalty and solver settings.
        request: crate::NeuralRequest,
        /// Profilers shown.
        profilers: ProfilerOptions,
    },
    /// A Gaussian-process regression.
    GaussianProcess {
        /// Columns, validation, kernel, nugget and starts.
        request: crate::GaussianProcessRequest,
        /// Profilers shown.
        profilers: ProfilerOptions,
    },
    /// Next runs of an experiment by expected improvement or an upper
    /// confidence bound.
    SequentialOptimization {
        /// Responses and goals, factors and bounds, constraints and settings
        /// (boxed: the request is much larger than most).
        request: Box<crate::SequentialRequest>,
    },
    /// Every applicable predictive model fitted and ranked.
    ModelSearch {
        /// Columns, validation, model kinds and seed.
        request: crate::ModelSearchRequest,
    },
    /// Functional data analysis: smoothed curves, registration, principal
    /// components and a model of the scores.
    FunctionalData {
        /// Curve, time, value and factor columns and the settings.
        request: crate::FunctionalRequest,
        /// Profilers shown.
        profilers: ProfilerOptions,
    },
    /// Fit measures and plots comparing saved prediction columns.
    ModelComparison {
        /// Response, prediction columns, event class and validation column.
        request: crate::ModelComparisonRequest,
    },
    /// Correspondence analysis of two categorical columns.
    Correspondence {
        /// The two columns.
        request: CorrespondenceRequest,
    },
    /// Correlations of several columns.
    Correlations {
        /// Columns, missing-value handling and tables shown.
        request: crate::CorrelationRequest,
    },
    /// Multivariate outlier distances.
    OutlierDistances {
        /// Columns, plotted distance and α.
        request: crate::OutlierRequest,
    },
    /// Multivariate analysis of variance of several responses.
    Manova {
        /// Responses, model effects, contrasts and plot settings.
        request: crate::ManovaRequest,
    },
    /// Item analysis (scale reliability).
    ItemAnalysis {
        /// Items, reversed items and scale.
        request: crate::ItemAnalysisRequest,
    },
    /// Exploratory factor analysis.
    FactorAnalysis {
        /// Columns, extraction, number of factors, rotation and scores.
        request: crate::FactorRequest,
    },
    /// Linear or quadratic discriminant analysis.
    DiscriminantAnalysis {
        /// Response, predictors, rule, priors and validation roles.
        request: crate::DiscriminantRequest,
    },
    /// Gaussian mixture clustering.
    GaussianMixture {
        /// Columns, candidate models and starts.
        request: crate::GaussianMixtureRequest,
    },
    /// Latent class analysis of categorical items.
    LatentClass {
        /// Items, range of classes and starts.
        request: crate::LatentClassRequest,
    },
    /// Term frequencies, phrases, word cloud, semantic dimensions and clusters of a text column.
    TextExploration {
        /// Text column, term options, dimensions and clusters.
        request: crate::TextExplorationRequest,
    },
    /// Topic model (latent Dirichlet allocation) of a text column.
    TopicModel {
        /// Text column, term options, topics and sampler settings.
        request: crate::TopicModelRequest,
    },
    /// Lexicon sentiment scores of a text column.
    Sentiment {
        /// Text column, negation and lexicon entries.
        request: crate::SentimentRequest,
    },
    /// A map of the rows (multidimensional scaling, t-SNE or UMAP).
    Embedding {
        /// Method, input and settings.
        request: crate::EmbeddingRequest,
    },
    /// Missing-value imputation of numeric columns.
    Imputation {
        /// Columns, method and settings.
        request: crate::ImputationRequest,
    },
    /// Series and Signals ▸ Explore: series description, decomposition, trend
    /// model or cross-correlation.
    SeriesExplore {
        /// Series columns, period and the exploration's settings.
        request: crate::SeriesExploreRequest,
    },
    /// Series and Signals ▸ Signals and spectra: Fourier spectrum, digital filter, convolution
    /// and correlation, or wavelets.
    SignalProcessing {
        /// Signal columns, sampling and the analysis's settings.
        request: crate::SignalProcessingRequest,
    },
    /// Series and Signals ▸ Signals and spectra ▸ Images: image spectrum, frequency
    /// filter, or registration and deconvolution.
    ImageAnalysis {
        /// Pixel columns and the analysis's settings.
        request: crate::ImageAnalysisRequest,
    },
    /// Survival curves and life-distribution fits.
    Survival {
        /// Time, status and group columns.
        request: SurvivalRequest,
        /// Display options.
        options: SurvivalOptions,
    },
    /// Reliability growth (power-law process).
    Growth {
        /// Time, event and phase columns and settings.
        request: crate::GrowthRequest,
    },
    /// Repairable systems (mean cumulative function).
    Recurrence {
        /// System, age, event and population columns and settings.
        request: crate::RecurrenceRequest,
    },
    /// Life distributions fitted to censored failure times.
    LifeDistribution {
        /// Layout, frequency and group columns, families and settings.
        request: crate::LifeDistributionRequest,
    },
    /// Competing risks: cumulative incidence, Gray's test, Fine–Gray regression, failure modes.
    CompetingRisks {
        /// Time, cause, group, frequency and covariate columns and settings.
        request: crate::CompetingRisksRequest,
    },
    /// Warranty returns: life data from shipment and return counts, fits and forecasts.
    Warranty {
        /// Layout columns and settings.
        request: crate::WarrantyRequest,
    },
    /// Exponential smoothing (ETS) forecasts of a time series.
    ExponentialSmoothing {
        /// Series columns, model and forecast settings.
        request: crate::SmoothingRequest,
    },
    /// Seasonal ARIMA forecasts of a time series.
    Arima {
        /// Series columns, orders, transformation and forecast settings.
        request: crate::ArimaRequest,
    },
    /// Principal coordinates analysis of a distance matrix.
    PrincipalCoordinates {
        /// Variables and distance, or distance-matrix columns; correction and colour.
        request: crate::PcoaRequest,
    },
    /// Non-compartmental pharmacokinetic parameters by subject and treatment.
    Pharmacokinetics {
        /// Subject, time, concentration, dose and the area and terminal-phase settings.
        request: crate::PkRequest,
    },
    /// Fatigue stress–life (S–N) curves with run-outs.
    Fatigue {
        /// Cycles, run-out, stress columns, curve form and distribution.
        request: crate::FatigueRequest,
    },
    /// Life–stress (accelerated life test) regression.
    LifeStress {
        /// Layout, stress columns, model and use conditions.
        request: crate::LifeStressRequest,
    },
    /// Degradation analysis (repeated or destructive measurements).
    Degradation {
        /// Layout, path model, threshold and method.
        request: crate::DegradationRequest,
    },
    /// Proportional hazards (Cox) regression.
    ProportionalHazards {
        /// Time, censor, predictor and strata columns, effects and settings.
        request: crate::ProportionalHazardsRequest,
        /// Which profilers the report shows.
        profilers: ProfilerOptions,
    },
    /// Parametric survival (accelerated failure time) regression.
    SurvivalRegression {
        /// Layout, frequency and predictor columns, effects and settings.
        request: crate::SurvivalRegressionRequest,
        /// Which profilers the report shows.
        profilers: ProfilerOptions,
    },
    /// Reliability test plan (uses no columns).
    TestPlan {
        /// The plan's inputs.
        request: crate::TestPlanRequest,
    },
    /// 3-D scatter plot of three columns, with colour, size and shape channels.
    Scatter3d {
        /// Position and channel columns.
        request: crate::Scatter3dRequest,
        /// Drop lines and viewing angle.
        options: crate::Scatter3dOptions,
    },
    /// Contour and surface plot of a response column over two others.
    DataSurface {
        /// Columns, method and grid.
        request: crate::DataSurfaceRequest,
        /// Contour levels, sections, surface style and viewing angle.
        options: crate::DataSurfaceOptions,
    },
    /// Graph canvas: encodings, scales and layers (columns by name).
    Graph {
        /// The graph's specification.
        spec: Box<GraphSpec>,
    },
    /// Scheffé mixture model.
    Mixture {
        /// Response, components and model.
        request: MixtureRequest,
        /// Profilers shown.
        profilers: ProfilerOptions,
    },
    /// Stepwise selection of linear-model effects.
    Stepwise {
        /// Candidates, direction and criterion.
        request: StepwiseRequest,
        /// Profilers shown for the selected model.
        options: ProfilerOptions,
    },
    /// Best-subsets regression.
    BestSubsets {
        /// Candidates, forced effects, heredity and criterion.
        request: BestSubsetsRequest,
    },
    /// Distribution identification: many distributions fitted to one column.
    DistributionFit {
        /// Column, families, transformations and plots.
        request: DistributionFitRequest,
    },
    /// p, np, c or u chart.
    Attribute {
        /// Chart type, count column and subgroup sizes.
        request: AttributeChartRequest,
    },
    /// Multivariate control chart (Hotelling T², |S| or MEWMA).
    MultivariateChart {
        /// Columns, chart type, subgroups, reference sample and parameters.
        request: crate::MultivariateChartRequest,
    },
    /// Analysis of means.
    Anom {
        /// Data kind, response, sizes, group columns and α.
        request: crate::AnomRequest,
    },
    /// Ordinal (proportional-odds) or nominal (baseline-category) logistic
    /// regression.
    CategoricalLogistic {
        /// Columns, effects, model and category settings.
        request: crate::CategoricalLogisticRequest,
    },
    /// Dose–response (probit) analysis.
    DoseResponse {
        /// Dose, response and trials columns, link, scale and levels.
        request: crate::DoseResponseRequest,
    },
    /// Nonlinear regression.
    Nonlinear {
        /// Response, model, parameters and settings.
        request: crate::NonlinearRequest,
        /// Which profilers the report shows (off by default).
        profilers: ProfilerOptions,
    },
    /// Deming and Passing–Bablok method comparison.
    MethodComparison {
        /// The two methods' columns, the fits and their settings.
        request: crate::MethodComparisonRequest,
    },
    /// Shelf-life estimation from stability data.
    ShelfLife {
        /// Time, response and batch columns, limits and levels.
        request: crate::ShelfLifeRequest,
    },
    /// Process capability against specification limits.
    Capability {
        /// Column, limits, σ estimator and transform.
        request: CapabilityRequest,
    },
    /// Pareto chart of defect categories.
    Pareto {
        /// Category, frequency and grouping columns.
        request: crate::ParetoRequest,
        /// "Other" threshold and bar scale.
        options: crate::ParetoChartOptions,
    },
    /// Cause-and-effect (fishbone) diagram.
    CauseEffect {
        /// Cause columns and the effect.
        request: crate::CauseEffectRequest,
    },
    /// Multi-vari chart.
    MultiVari {
        /// Response and factor columns.
        request: crate::MultiVariRequest,
        /// Values, ranges view and cell table.
        options: crate::MultiVariOptions,
    },
    /// EWMA or CUSUM control chart.
    TimeWeighted {
        /// Column, chart type, parameters and target.
        request: TimeWeightedRequest,
    },
    /// Second-order response surface.
    Surface {
        /// Response and factor columns.
        request: SurfaceRequest,
        /// Display options.
        options: SurfaceOptions,
    },
    /// One-sample tests.
    OneSample {
        /// Column, group and hypotheses.
        request: OneSampleRequest,
    },
    /// Comparison of independent groups.
    Groups {
        /// Layout and settings.
        request: GroupComparisonRequest,
    },
    /// Paired or repeated-measures comparison.
    Paired {
        /// Layout and settings.
        request: PairedRequest,
    },
    /// Proportion tests.
    Proportions {
        /// Outcome, event and comparison.
        request: ProportionRequest,
    },
    /// Event-rate tests.
    Rates {
        /// Count, exposure and group columns.
        request: RateRequest,
    },
    /// Power and sample size (uses no columns).
    Power {
        /// The question.
        request: PowerRequest,
    },
    /// Binomial or Poisson capability from counts.
    AttributeCapability {
        /// Model, count and size columns, confidence level.
        request: crate::AttributeCapabilityRequest,
    },
    /// Capability with between- and within-subgroup variation separated.
    BetweenWithinCapability {
        /// Column, subgroups, limits and σ estimators.
        request: crate::BetweenWithinRequest,
    },
    /// Acceptance sampling plans (uses no columns).
    AcceptanceSampling {
        /// Risk points, lot, plan to design and plans to compare.
        request: crate::AcceptanceRequest,
    },
    /// Gauge bias at reference values and linearity.
    GaugeLinearity {
        /// Reference and measurement columns, confidence, process variation.
        request: crate::LinearityRequest,
    },
    /// Single-gauge repeatability study (Cg, Cgk).
    GaugeRepeatability {
        /// Measurement column, reference value, tolerance and indices' constants.
        request: crate::RepeatabilityRequest,
    },
    /// Intraclass-correlation evaluation of a crossed measurement study.
    IntraclassCorrelation {
        /// Part, operator and measurement columns, confidence, increment.
        request: crate::IccRequest,
    },
    /// Attribute agreement study of appraisers' categorical ratings.
    AttributeAgreement {
        /// Part, appraiser, rating, trial and standard columns; settings.
        request: crate::AgreementRequest,
    },
    /// T² and SPE charts from a principal-components model.
    PcaChart {
        /// Columns, reference period, components and limits.
        request: crate::PcaChartRequest,
    },
    /// Environmental monitoring alert and action limits.
    MonitoringLimits {
        /// Readings, locations, limit groups, level method and rules.
        request: crate::MonitoringRequest,
    },
    /// Bias and repeatability of a go/no-go gauge from acceptance counts.
    AttributeGauge {
        /// Reference, acceptance and checks columns; the limit.
        request: crate::AttributeGaugeRequest,
    },
    /// Multi-way table of counts, percentages and summary statistics.
    MultiwayTable {
        /// Row and column classifications, statistics and totals.
        request: crate::MultiwayTableRequest,
    },
    /// Stratified contingency tables with CMH, Mantel–Haenszel and exact tests.
    StratifiedTables {
        /// Row, column, stratum and frequency columns and test settings.
        request: crate::StratifiedRequest,
    },
    /// Signal-to-noise analysis of an orthogonal-array experiment.
    SignalToNoise {
        /// Factor and response columns and the ratio.
        request: crate::SignalToNoiseRequest,
    },
    /// An existing table declared as a design.
    DeclaredDesign {
        /// Factor columns with their ranges, and response columns.
        request: crate::DesignDeclarationRequest,
    },
    /// A design evaluated before data collection (power, prediction
    /// variance, aliasing, efficiencies), or two designs compared.
    DesignEvaluation {
        /// Factor columns, model, alias terms, coefficients, α and comparison column.
        request: crate::DesignEvaluationRequest,
    },
    /// Analysis of a two-level factorial experiment.
    FactorialAnalysis {
        /// Factors, response, model terms and display options.
        request: crate::FactorialAnalysisRequest,
    },
    /// Chi-square goodness of fit of counts.
    GoodnessOfFit {
        /// Column, frequency column and hypothesized distribution.
        request: crate::GoodnessRequest,
    },
    /// Two-period crossover trial with equivalence.
    Crossover {
        /// Response, subject, period and treatment columns; bounds.
        request: crate::CrossoverRequest,
    },
    /// Area under a sampled curve above a baseline, with peaks.
    AreaUnderCurve {
        /// x, response and group columns; baseline and level.
        request: crate::AreaRequest,
    },
    /// A column of p-values adjusted for multiple testing and combined.
    AdjustPValues {
        /// p-value, label and weight columns; α and λ.
        request: crate::PValueRequest,
    },
    /// The same two-group test on many variables, with adjusted p-values.
    ManyTests {
        /// Variables, grouping column, test and adjustment.
        request: crate::ManyTestsRequest,
    },
    /// Treatment effects from observational data: propensity scores,
    /// balance, matching or weighting, subgroups.
    TreatmentEffects {
        /// Treatment, outcome, covariates, method and subgroups.
        request: crate::TreatmentEffectsRequest,
    },
    /// One model per response column against the same factors, with
    /// false-discovery-rate adjusted p-values.
    Screening {
        /// Responses, factors, model and adjustment.
        request: crate::ScreeningRequest,
    },
    /// Two- and three-way repeated-measures ANOVA (long data).
    FactorialRepeated {
        /// Response, subject, within and between factors; simple effects.
        request: crate::FactorialRepeatedRequest,
    },
    /// Nested t test or nested one-way ANOVA.
    Nested {
        /// Response, group and subject columns; confidence level.
        request: crate::NestedRequest,
    },
    /// Bayesian estimation, Bayes factor test, linear model or GLM.
    Bayesian {
        /// Which analysis and its settings.
        request: crate::BayesRequest,
    },
    /// Multiple-response (check-all-that-apply) frequencies and cross-tab.
    MultipleResponseItems {
        /// Response columns, delimiter, grouping column and item order.
        request: crate::MultipleResponseRequest,
    },
    /// Signal smoothing, derivatives and integrals, or spectrum peaks.
    Signal {
        /// Which analysis and its settings.
        request: crate::SignalRequest,
    },
    /// A value column drawn as a picture over its x and y columns.
    ImagePlot {
        /// Columns and colours.
        request: crate::image_plot::ImagePlotRequest,
    },
    /// Distribution calculator (uses no columns).
    DistributionCalculator {
        /// Family, parameters and region.
        request: crate::CalculatorRequest,
    },
    /// Bootstrap of cells of another report's table.
    Bootstrap {
        /// The report, its table and cells, replicates, seed and level.
        request: crate::BootstrapRequest,
    },
    /// Permutation test for two groups, paired columns or a correlation.
    PermutationTest {
        /// Design, columns and Monte Carlo settings.
        request: crate::PermutationRequest,
    },
    /// Another report with its own row filter or column picker (see
    /// [`crate::report_scope`]). Never nested: build it with
    /// [`SavedReport::with_scope`].
    Scoped {
        /// Row filter, switcher columns and remembered limits.
        scope: crate::report_scope::ReportScope,
        /// The report request.
        report: Box<SavedReport>,
    },
}

impl SavedReport {
    /// Columns a graph uses, by name (other reports name columns by index;
    /// see [`SavedReport::columns`]).
    #[must_use]
    pub fn column_names(&self) -> Vec<&str> {
        match self {
            Self::Graph { spec } => spec.columns(),
            // The filter's columns are used; the switcher's choices are not.
            Self::Scoped { scope, report } => {
                let mut names = report.column_names();
                names.extend(scope.filter_columns());
                names
            }
            _ => Vec::new(),
        }
    }

    /// Follow a column rename (graphs refer to columns by name).
    pub fn rename_column(&mut self, old: &str, new: &str) {
        match self {
            Self::Graph { spec } => spec.rename(old, new),
            Self::Scoped { scope, report } => {
                scope.rename(old, new);
                report.rename_column(old, new);
            }
            _ => {}
        }
    }

    /// Every column index the analysis refers to.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        match self {
            Self::Linear { spec, .. } => {
                std::iter::once(spec.response).chain(spec.predictors.iter().copied()).collect()
            }
            Self::ControlChart { request, .. } => request.columns(),
            Self::Gauge { request, .. } => request.columns(),
            Self::Distribution { request, .. } => request.columns(),
            Self::Bivariate { request } => vec![request.x, request.y],
            Self::Pca { request } => request.columns.clone(),
            Self::Correspondence { request } => vec![request.row, request.column],
            Self::Correlations { request } => request.columns(),
            Self::OutlierDistances { request } => request.columns(),
            Self::ItemAnalysis { request } => request.columns(),
            Self::Manova { request } => request.columns(),
            Self::FactorAnalysis { request } => request.columns(),
            Self::DiscriminantAnalysis { request } => request.columns(),
            Self::GaussianMixture { request } => request.columns(),
            Self::LatentClass { request } => request.columns(),
            Self::TextExploration { request } => request.columns(),
            Self::TopicModel { request } => request.columns(),
            Self::Sentiment { request } => request.columns(),
            Self::Embedding { request } => request.columns(),
            Self::Imputation { request } => request.columns(),
            Self::SeriesExplore { request } => request.columns(),
            Self::SignalProcessing { request } => request.columns(),
            Self::ImageAnalysis { request } => request.columns(),
            Self::Survival { request, .. } => {
                std::iter::once(request.time).chain(request.status).chain(request.group).collect()
            }
            Self::Growth { request } => request.columns(),
            Self::Recurrence { request } => request.columns(),
            Self::LifeDistribution { request } => request.columns(),
            Self::CompetingRisks { request } => request.columns(),
            Self::Warranty { request } => request.columns(),
            Self::ExponentialSmoothing { request } => request.columns(),
            Self::Arima { request } => request.columns(),
            Self::PrincipalCoordinates { request } => request.columns(),
            Self::Pharmacokinetics { request } => request.columns(),
            Self::Fatigue { request } => request.columns(),
            Self::LifeStress { request } => request.columns(),
            Self::Degradation { request } => request.columns(),
            Self::ProportionalHazards { request, .. } => request.columns(),
            Self::SurvivalRegression { request, .. } => request.columns(),
            Self::TestPlan { .. } => Vec::new(),
            Self::Scoped { report, .. } => report.columns(),
            Self::Surface { request, .. } => {
                std::iter::once(request.response).chain(request.factors.iter().copied()).collect()
            }
            Self::TimeWeighted { request } => vec![request.column],
            Self::Capability { request } => request.columns(),
            Self::Mixture { request, .. } => {
                std::iter::once(request.response).chain(request.components.iter().copied()).collect()
            }
            Self::Scatter3d { request, .. } => request.columns(),
            Self::DataSurface { request, .. } => request.columns(),
            // Graphs name their columns; see `SavedReport::column_names`.
            Self::Graph { .. } => Vec::new(),
            Self::Stepwise { request, .. } => {
                let c = &request.candidates;
                std::iter::once(c.response).chain(c.predictors.iter().copied()).collect()
            }
            Self::BestSubsets { request } => {
                let c = &request.candidates;
                std::iter::once(c.response).chain(c.predictors.iter().copied()).collect()
            }
            Self::DistributionFit { request } => request.columns(),
            Self::Attribute { request } => request.columns(),
            Self::Pareto { request, .. } => {
                std::iter::once(request.category).chain(request.frequency).chain(request.group).collect()
            }
            Self::CauseEffect { request } => request.causes.clone(),
            Self::MultiVari { request, .. } => {
                std::iter::once(request.response).chain(request.factors.iter().copied()).collect()
            }
            Self::MultivariateChart { request } => request.columns(),
            Self::Anom { request } => request.columns(),
            Self::DoseResponse { request } => request.columns(),
            Self::CategoricalLogistic { request } => request.columns(),
            Self::Nonlinear { request, .. } => request.columns(),
            Self::MethodComparison { request } => request.columns(),
            Self::ShelfLife { request } => request.columns(),
            Self::OneSample { request } => std::iter::once(request.column).chain(request.group).collect(),
            Self::Groups { request } => request.columns(),
            Self::Paired { request } => request.columns(),
            Self::Proportions { request } => request.columns(),
            Self::Rates { request } => request.columns(),
            Self::Power { .. } | Self::AcceptanceSampling { .. } | Self::DistributionCalculator { .. } => Vec::new(),
            Self::GoodnessOfFit { request } => request.columns(),
            Self::Crossover { request } => request.columns(),
            Self::AdjustPValues { request } => request.columns(),
            Self::AreaUnderCurve { request } => request.columns(),
            Self::ManyTests { request } => request.all_columns(),
            Self::TreatmentEffects { request } => request.columns(),
            Self::Screening { request } => request.all_columns(),
            Self::FactorialRepeated { request } => request.columns(),
            Self::Nested { request } => request.columns(),
            Self::Bayesian { request } => request.columns(),
            Self::MultipleResponseItems { request } => request.columns(),
            Self::Signal { request } => request.columns(),
            Self::ImagePlot { request } => request.columns(),
            Self::AttributeCapability { request } => request.columns(),
            Self::BetweenWithinCapability { request } => request.columns(),
            Self::GaugeLinearity { request } => request.columns(),
            Self::GaugeRepeatability { request } => request.columns(),
            Self::IntraclassCorrelation { request } => request.columns(),
            Self::AttributeAgreement { request } => request.columns(),
            Self::AttributeGauge { request } => request.columns(),
            Self::PcaChart { request } => request.all_columns(),
            Self::MonitoringLimits { request } => request.columns(),
            Self::MultiwayTable { request } => request.columns(),
            Self::StratifiedTables { request } => request.columns(),
            Self::Bootstrap { request } => request.columns(),
            Self::PermutationTest { request } => request.columns(),
            Self::SignalToNoise { request } => request.columns(),
            Self::DeclaredDesign { request } => request.columns(),
            Self::DesignEvaluation { request } => request.columns(),
            Self::FactorialAnalysis { request } => request.columns(),
            Self::Cluster { request } => request.columns.clone(),
            Self::Glm { request, .. } => {
                std::iter::once(request.response).chain(request.predictors.iter().copied()).collect()
            }
            Self::Regularized { request, .. } => {
                std::iter::once(request.response).chain(request.predictors.iter().copied()).collect()
            }
            Self::MultipleResponses { request } => request.columns(),
            Self::Mixed { request, .. } => request.columns(),
            Self::DecisionTree { request, .. } => request.columns(),
            Self::RandomForest { request, .. } => request.columns(),
            Self::BoostedTrees { request, .. } => request.columns(),
            Self::PartialLeastSquares { request, .. } => request.columns(),
            Self::SupportVectorMachine { request, .. } => request.columns(),
            Self::Mars { request, .. } => request.columns(),
            Self::NeuralNetwork { request, .. } => request.columns(),
            Self::GaussianProcess { request, .. } => request.columns(),
            Self::SequentialOptimization { request } => request.columns(),
            Self::ModelSearch { request } => request.columns(),
            Self::FunctionalData { request, .. } => request.columns(),
            Self::ModelComparison { request } => request.columns(),
        }
    }

    /// The same report with every column index passed through `f` (used when
    /// columns move because one was inserted or removed).
    #[must_use]
    pub fn map_columns(mut self, f: impl Fn(usize) -> usize) -> Self {
        if let Self::Scoped { scope, report } = self {
            // The scope names its columns, so only the request moves.
            return Self::Scoped { scope, report: Box::new(report.map_columns(f)) };
        }
        match &mut self {
            Self::Linear { spec, .. } => {
                spec.response = f(spec.response);
                spec.predictors.iter_mut().for_each(|p| *p = f(*p));
            }
            Self::ControlChart { request, .. } => {
                request.column = f(request.column);
                for c in [&mut request.subgroup, &mut request.phase, &mut request.label, &mut request.part] {
                    *c = c.map(&f);
                }
            }
            Self::Gauge { request, .. } => {
                request.part = f(request.part);
                request.operator = f(request.operator);
                request.measurement = f(request.measurement);
                request.design.map_columns(&f);
            }
            Self::Distribution { request, .. } => request.map_columns(&f),
            Self::Bivariate { request } => {
                request.x = f(request.x);
                request.y = f(request.y);
            }
            Self::Pca { request } => request.columns.iter_mut().for_each(|c| *c = f(*c)),
            Self::Correspondence { request } => {
                request.row = f(request.row);
                request.column = f(request.column);
            }
            Self::Correlations { request } => request.map_columns(&f),
            Self::OutlierDistances { request } => request.map_columns(&f),
            Self::ItemAnalysis { request } => request.map_columns(&f),
            Self::Manova { request } => request.map_columns(&f),
            Self::FactorAnalysis { request } => request.map_columns(&f),
            Self::DiscriminantAnalysis { request } => request.map_columns(&f),
            Self::GaussianMixture { request } => request.map_columns(&f),
            Self::LatentClass { request } => request.map_columns(&f),
            Self::TextExploration { request } => request.map_columns(&f),
            Self::TopicModel { request } => request.map_columns(&f),
            Self::Sentiment { request } => request.map_columns(&f),
            Self::Embedding { request } => request.map_columns(&f),
            Self::Imputation { request } => request.map_columns(&f),
            Self::SeriesExplore { request } => request.map_columns(&f),
            Self::SignalProcessing { request } => request.map_columns(&f),
            Self::ImageAnalysis { request } => request.map_columns(&f),
            Self::TimeWeighted { request } => request.column = f(request.column),
            Self::Capability { request } => {
                request.column = f(request.column);
                request.by = request.by.map(&f);
            }
            Self::Mixture { request, .. } => {
                request.response = f(request.response);
                request.components.iter_mut().for_each(|c| *c = f(*c));
            }
            Self::Scatter3d { request, .. } => *request = request.map_columns(&f),
            Self::DataSurface { request, .. } => *request = request.map_columns(&f),
            // Columns by name: unaffected by columns moving.
            Self::Graph { .. } => {}
            Self::Stepwise { request, .. } => {
                let c = &mut request.candidates;
                c.response = f(c.response);
                c.predictors.iter_mut().for_each(|p| *p = f(*p));
            }
            Self::BestSubsets { request } => {
                let c = &mut request.candidates;
                c.response = f(c.response);
                c.predictors.iter_mut().for_each(|p| *p = f(*p));
            }
            Self::DistributionFit { request } => request.column = f(request.column),
            Self::Attribute { request } => {
                request.count = f(request.count);
                if let SubgroupSize::Column(c) = &mut request.size {
                    *c = f(*c);
                }
                request.phase = request.phase.map(&f);
            }
            Self::Pareto { request, .. } => {
                request.category = f(request.category);
                request.frequency = request.frequency.map(&f);
                request.group = request.group.map(&f);
            }
            Self::CauseEffect { request } => request.causes.iter_mut().for_each(|c| *c = f(*c)),
            Self::MultiVari { request, .. } => {
                request.response = f(request.response);
                request.factors.iter_mut().for_each(|c| *c = f(*c));
            }
            Self::MultivariateChart { request } => request.map_columns(&f),
            Self::Anom { request } => request.map_columns(&f),
            Self::DoseResponse { request } => request.map_columns(&f),
            Self::CategoricalLogistic { request } => request.map_columns(&f),
            Self::Nonlinear { request, .. } => request.map_columns(&f),
            Self::MethodComparison { request } => request.map_columns(&f),
            Self::ShelfLife { request } => request.map_columns(&f),
            Self::Surface { request, .. } => {
                request.response = f(request.response);
                request.factors.iter_mut().for_each(|c| *c = f(*c));
            }
            Self::Survival { request, .. } => {
                request.time = f(request.time);
                request.status = request.status.map(&f);
                request.group = request.group.map(&f);
            }
            Self::Growth { request } => request.map_columns(&f),
            Self::Recurrence { request } => request.map_columns(&f),
            Self::LifeDistribution { request } => request.map_columns(&f),
            Self::CompetingRisks { request } => request.map_columns(&f),
            Self::Warranty { request } => request.map_columns(&f),
            Self::ExponentialSmoothing { request } => request.map_columns(&f),
            Self::Arima { request } => request.map_columns(&f),
            Self::PrincipalCoordinates { request } => request.map_columns(&f),
            Self::Pharmacokinetics { request } => request.map_columns(&f),
            Self::Fatigue { request } => request.map_columns(&f),
            Self::LifeStress { request } => request.map_columns(&f),
            Self::Degradation { request } => request.map_columns(&f),
            Self::ProportionalHazards { request, .. } => request.map_columns(&f),
            Self::SurvivalRegression { request, .. } => request.map_columns(&f),
            Self::TestPlan { .. } => {}
            Self::Scoped { .. } => unreachable!("handled above"),
            Self::OneSample { request } => {
                request.column = f(request.column);
                request.group = request.group.map(&f);
            }
            Self::Groups { request } => match &mut request.layout {
                GroupLayout::Stacked { response, group } => {
                    *response = f(*response);
                    *group = f(*group);
                }
                GroupLayout::Columns(c) => c.iter_mut().for_each(|c| *c = f(*c)),
            },
            Self::Paired { request } => match &mut request.layout {
                PairedLayout::Columns(c) => c.iter_mut().for_each(|c| *c = f(*c)),
                PairedLayout::Stacked { response, condition, subject } => {
                    *response = f(*response);
                    *condition = f(*condition);
                    *subject = f(*subject);
                }
            },
            Self::Proportions { request } => {
                request.outcome = f(request.outcome);
                match &mut request.comparison {
                    ProportionComparison::OneSample { .. } => {}
                    ProportionComparison::Groups { group } => *group = f(*group),
                    ProportionComparison::Paired { second } => *second = f(*second),
                }
            }
            Self::Rates { request } => {
                request.count = f(request.count);
                request.exposure = request.exposure.map(&f);
                request.group = request.group.map(&f);
            }
            Self::Power { .. } | Self::AcceptanceSampling { .. } | Self::DistributionCalculator { .. } => {}
            Self::GoodnessOfFit { request } => *request = request.map_columns(&f),
            Self::Crossover { request } => *request = request.map_columns(&f),
            Self::AdjustPValues { request } => *request = request.map_columns(&f),
            Self::AreaUnderCurve { request } => *request = request.map_columns(&f),
            Self::ManyTests { request } => *request = request.map_columns(&f),
            Self::TreatmentEffects { request } => *request = request.map_columns(&f),
            Self::Screening { request } => *request = request.map_columns(&f),
            Self::FactorialRepeated { request } => *request = request.map_columns(&f),
            Self::Nested { request } => *request = request.map_columns(&f),
            Self::Bayesian { request } => *request = request.map_columns(&f),
            Self::MultipleResponseItems { request } => *request = request.map_columns(&f),
            Self::Signal { request } => *request = request.map_columns(&f),
            Self::ImagePlot { request } => *request = request.map_columns(&f),
            Self::AttributeCapability { request } => *request = request.map_columns(&f),
            Self::BetweenWithinCapability { request } => *request = request.map_columns(&f),
            Self::GaugeLinearity { request } => *request = request.map_columns(&f),
            Self::GaugeRepeatability { request } => *request = request.map_columns(&f),
            Self::IntraclassCorrelation { request } => *request = request.map_columns(&f),
            Self::AttributeAgreement { request } => *request = request.clone().map_columns(&f),
            Self::AttributeGauge { request } => *request = request.clone().map_columns(&f),
            Self::PcaChart { request } => request.map_columns(&f),
            Self::MonitoringLimits { request } => request.map_columns(&f),
            Self::MultiwayTable { request } => *request = request.clone().map_columns(&f),
            Self::StratifiedTables { request } => *request = request.clone().map_columns(&f),
            Self::Bootstrap { request } => *request = request.clone().map_columns(&f),
            Self::PermutationTest { request } => *request = request.map_columns(&f),
            Self::SignalToNoise { request } => *request = request.clone().map_columns(&f),
            Self::DeclaredDesign { request } => *request = request.clone().map_columns(&f),
            Self::DesignEvaluation { request } => *request = request.clone().map_columns(&f),
            Self::FactorialAnalysis { request } => *request = request.clone().map_columns(&f),
            Self::Cluster { request } => request.columns.iter_mut().for_each(|c| *c = f(*c)),
            Self::Glm { request, .. } => {
                request.response = f(request.response);
                request.predictors.iter_mut().for_each(|c| *c = f(*c));
            }
            Self::Regularized { request, .. } => {
                request.response = f(request.response);
                request.predictors.iter_mut().for_each(|c| *c = f(*c));
            }
            Self::MultipleResponses { request } => **request = (**request).clone().map_columns(&f),
            Self::Mixed { request, .. } => *request = request.clone().map_columns(&f),
            Self::DecisionTree { request, .. } => *request = request.clone().map_columns(&f),
            Self::RandomForest { request, .. } => *request = request.clone().map_columns(&f),
            Self::BoostedTrees { request, .. } => *request = request.clone().map_columns(&f),
            Self::PartialLeastSquares { request, .. } => *request = request.clone().map_columns(&f),
            Self::SupportVectorMachine { request, .. } => *request = request.clone().map_columns(&f),
            Self::Mars { request, .. } => *request = request.clone().map_columns(&f),
            Self::NeuralNetwork { request, .. } => *request = request.clone().map_columns(&f),
            Self::GaussianProcess { request, .. } => *request = request.clone().map_columns(&f),
            Self::SequentialOptimization { request } => **request = request.as_ref().clone().map_columns(&f),
            Self::ModelSearch { request } => *request = request.clone().map_columns(&f),
            Self::FunctionalData { request, .. } => *request = request.clone().map_columns(&f),
            Self::ModelComparison { request } => *request = request.clone().map_columns(&f),
        }
        self
    }
}

/// A dataset, its row states and its saved reports.
#[derive(Debug, Clone)]
pub struct Project {
    /// The data.
    pub table: DataTable,
    /// Selection, exclusion, hiding and colors.
    pub rows: RowStateManager,
    /// Analyses, in display order.
    pub reports: Vec<SavedReport>,
    /// How the table was made, when a table operation made it.
    pub origin: Option<TableOrigin>,
    /// The database or web source the table was read from, when it was
    /// (without credentials; see [`crate::external`]).
    pub external: Option<crate::external::ExternalSource>,
    /// Scripts kept with the project.
    pub scripts: Vec<ProjectScript>,
}

/// A Rhai script kept in a project file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectScript {
    /// Name shown in menus (unique within the project).
    pub name: String,
    /// The script's source text.
    pub source: String,
    /// Offer to run the script when the project opens (the application
    /// always asks first).
    pub run_on_open: bool,
}

impl ProjectScript {
    fn to_json(&self) -> JsonValue {
        obj([
            ("name", JsonValue::String(self.name.clone())),
            ("source", JsonValue::String(self.source.clone())),
            ("run_on_open", JsonValue::Bool(self.run_on_open)),
        ])
    }

    fn from_json(v: &JsonValue) -> Result<Self> {
        let text = |key: &str| {
            member(v, key)?.as_str().map(str::to_owned).ok_or_else(|| invalid(format!("scripts: {key:?} must be text")))
        };
        let run_on_open = match v.get("run_on_open") {
            None | Some(JsonValue::Null) => false,
            Some(JsonValue::Bool(b)) => *b,
            Some(_) => return Err(invalid("scripts: \"run_on_open\" must be true or false")),
        };
        Ok(Self { name: text("name")?, source: text("source")?, run_on_open })
    }
}

/// Serialize a project to JSON text.
#[must_use]
pub fn save_project(project: &Project) -> String {
    let table = &project.table;
    let version = if table.columns().iter().any(|c| c.temporal().is_some()) { 2.0 } else { 1.0 };
    let mut members =
        vec![("format".to_owned(), s(PROJECT_FORMAT)), ("version".to_owned(), JsonValue::Number(version))];
    members.extend(table_members(
        table,
        &project.rows,
        &project.reports,
        project.origin.as_ref(),
        project.external.as_ref(),
    ));
    if !project.scripts.is_empty() {
        members.push(("scripts".to_owned(), scripts_json(&project.scripts)));
    }
    JsonValue::Object(members).to_pretty()
}

/// The members describing one table in a project file: `table`, `rows`,
/// `reports`, then `origin` and `external_source` when present.
pub(crate) fn table_members(
    table: &DataTable,
    rows: &RowStateManager,
    reports: &[SavedReport],
    origin: Option<&TableOrigin>,
    external: Option<&crate::external::ExternalSource>,
) -> Vec<(String, JsonValue)> {
    let columns = table.columns().iter().map(column_to_json).collect();
    let indices = |pred: &dyn Fn(usize) -> bool| {
        JsonValue::Array((0..rows.len()).filter(|&r| pred(r)).map(|r| JsonValue::Number(r as f64)).collect())
    };
    let colors = (0..rows.len())
        .filter_map(|r| {
            rows.color(r).map(|c| {
                JsonValue::Array(vec![JsonValue::Number(r as f64), s(&format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b))])
            })
        })
        .collect();
    let mut row_members = vec![
        ("excluded".to_owned(), indices(&|r| rows.is_excluded_flag(r))),
        ("hidden".to_owned(), indices(&|r| rows.is_hidden_flag(r))),
        ("selected".to_owned(), indices(&|r| rows.is_selected(r))),
        ("colors".to_owned(), JsonValue::Array(colors)),
    ];
    // Row markers and the column they came from, only when set.
    let markers: Vec<JsonValue> = (0..rows.len())
        .filter_map(|r| rows.marker(r).map(|m| JsonValue::Array(vec![JsonValue::Number(r as f64), s(m.key())])))
        .collect();
    if !markers.is_empty() {
        row_members.push(("markers".to_owned(), JsonValue::Array(markers)));
    }
    if let Some(c) = rows.style_column() {
        row_members.push(("style_column".to_owned(), s(c)));
    }
    // Named row subsets, only when there are any (older builds ignore them).
    if !rows.subset_names().is_empty() {
        let subsets = rows
            .subset_names()
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let list = rows.subset_rows(i).into_iter().map(|r| JsonValue::Number(r as f64)).collect();
                obj([("name", s(name)), ("rows", JsonValue::Array(list))])
            })
            .collect();
        row_members.push(("subsets".to_owned(), JsonValue::Array(subsets)));
        if let Some(active) = rows.active_subset() {
            row_members.push(("active_subset".to_owned(), s(&rows.subset_names()[active])));
        }
    }
    let mut members = vec![
        ("table".to_owned(), obj([("name", s(table.name())), ("columns", JsonValue::Array(columns))])),
        ("rows".to_owned(), JsonValue::Object(row_members)),
        // A report naming a column the table does not have (which would be
        // written as "") is left out rather than saved unreadable.
        (
            "reports".to_owned(),
            JsonValue::Array(reports.iter().filter_map(|r| checked_report_to_json(r, table).ok()).collect()),
        ),
    ];
    if let Some(origin) = origin {
        members.push(("origin".to_owned(), origin.to_json()));
    }
    if let Some(external) = external {
        members.push(("external_source".to_owned(), external.to_json()));
    }
    members
}

/// Parse a project written by [`save_project`] (or a version-3 project file
/// holding exactly one table; its layout and source links are ignored).
/// A report that cannot be read is left out; [`load_project_noting`] also
/// returns a note for each such report.
///
/// # Errors
/// [`StatError::InvalidArgument`] describing the first problem: invalid JSON,
/// a file that is not a project, a newer format version, malformed table or
/// row data, or a version-3 file holding several tables (read those with
/// [`load_project_file`]).
pub fn load_project(text: &str) -> Result<Project> {
    load_project_noting(text).map(|(project, _)| project)
}

/// [`load_project`], also returning notes on what could not be restored:
/// a report that cannot be read (for example one naming a column the table
/// does not have) is left out, and a formula column whose formula is on a
/// cycle or no longer compiles keeps its saved values; each gets a note
/// naming it and why, so the table and the other reports still open.
///
/// # Errors
/// As for [`load_project`], except that unreadable reports are notes, not
/// errors.
pub fn load_project_noting(text: &str) -> Result<(Project, Vec<String>)> {
    let doc = parse_json(text)?;
    if doc.get("format").and_then(JsonValue::as_str) != Some(PROJECT_FORMAT) {
        return Err(invalid("this file is not a statistical-engine project"));
    }
    let version = doc.get("version").and_then(JsonValue::as_usize).ok_or_else(|| invalid("missing version"))?;
    if version == PROJECT_FILE_VERSION {
        let (file, notes) = tables::from_version_3(&doc)?;
        return match &file.tables[..] {
            [only] => Ok((only.to_project(file.scripts.clone()), notes)),
            many => Err(invalid(format!("this project holds {} tables; open it as a project file", many.len()))),
        };
    }
    if version > PROJECT_VERSION {
        return Err(invalid(format!(
            "project version {version} is newer than this application supports ({PROJECT_FILE_VERSION})"
        )));
    }
    let LoadedTable { table, rows, reports, origin, external, notes, .. } = read_table_members(&doc)?;
    let scripts = read_scripts(&doc)?;
    Ok((Project { table, rows, reports, origin, external, scripts }, notes))
}

/// One table's parts as read by [`read_table_members`].
pub(crate) struct LoadedTable {
    pub(crate) table: DataTable,
    pub(crate) rows: RowStateManager,
    pub(crate) reports: Vec<SavedReport>,
    pub(crate) origin: Option<TableOrigin>,
    pub(crate) external: Option<crate::external::ExternalSource>,
    /// What could not be restored (reports left out, formula columns that
    /// could not be computed), one note each.
    pub(crate) notes: Vec<String>,
    /// Indices (in the file) of the reports left out, ascending.
    pub(crate) skipped_reports: Vec<usize>,
}

/// Read the members [`table_members`] wrote from `doc`. A report that cannot
/// be read is left out with a note (see [`load_project_noting`]).
pub(crate) fn read_table_members(doc: &JsonValue) -> Result<LoadedTable> {
    let t = member(doc, "table")?;
    let name = member(t, "name")?.as_str().ok_or_else(|| invalid("table name must be a string"))?;
    let mut columns = array(t, "columns")?.iter().map(column_from_json).collect::<Result<Vec<_>>>()?;
    // A formula column saved without values is sized to the table here and
    // computed below.
    let n_rows = columns.iter().map(|c| c.data.len()).max().unwrap_or(0);
    for c in &mut columns {
        if c.formula.is_some() && c.data.is_empty() && n_rows > 0 {
            *c =
                Column { level: c.level, formula: c.formula.take(), ..Column::empty(c.name.clone(), c.kind(), n_rows) };
        }
    }
    let mut table = DataTable::new(name, columns)?;
    // Formula columns hold what their formulas compute on the loaded data.
    let mut notes: Vec<String> = crate::formula::refresh_all_noting(&mut table)
        .into_iter()
        .map(|note| format!("Table {}: {note}", table.name()))
        .collect();

    let n = table.n_rows();
    let mut rows = RowStateManager::new(n);
    let r = member(doc, "rows")?;
    let row_list = |key: &str| -> Result<Vec<usize>> {
        array(r, key)?
            .iter()
            .map(|v| v.as_usize().filter(|&i| i < n).ok_or_else(|| invalid(format!("rows.{key}: invalid row index"))))
            .collect()
    };
    rows.set_excluded(row_list("excluded")?, true);
    rows.set_hidden(row_list("hidden")?, true);
    rows.select(row_list("selected")?, stat_data::SelectionMode::Replace);
    for pair in array(r, "colors")? {
        let parsed = pair.as_array().and_then(|p| match p {
            [row, color] => Some((row.as_usize().filter(|&i| i < n)?, parse_color(color.as_str()?)?)),
            _ => None,
        });
        let (row, color) = parsed.ok_or_else(|| invalid("rows.colors: expected [row, \"#rrggbb\"] pairs"))?;
        rows.set_color([row], Some(color));
    }
    if let Some(JsonValue::Array(markers)) = r.get("markers") {
        for pair in markers {
            let parsed = pair.as_array().and_then(|p| match p {
                [row, marker] => Some((row.as_usize().filter(|&i| i < n)?, RowMarker::from_key(marker.as_str()?)?)),
                _ => None,
            });
            let (row, marker) = parsed.ok_or_else(|| invalid("rows.markers: expected [row, \"shape\"] pairs"))?;
            rows.set_marker([row], Some(marker));
        }
    }
    if let Some(c) = r.get("style_column").and_then(JsonValue::as_str) {
        rows.set_style_column(Some(c.to_owned()));
    }
    if let Some(JsonValue::Array(subsets)) = r.get("subsets") {
        for subset in subsets {
            let name = member(subset, "name")?
                .as_str()
                .filter(|n| !n.is_empty())
                .ok_or_else(|| invalid("rows.subsets: each subset needs a name"))?;
            if rows.subset_names().iter().any(|n| n == name) {
                return Err(invalid(format!("rows.subsets: two subsets are called {name:?}")));
            }
            let list = array(subset, "rows")?
                .iter()
                .map(|v| v.as_usize().filter(|&i| i < n).ok_or_else(|| invalid("rows.subsets: invalid row index")))
                .collect::<Result<Vec<usize>>>()?;
            rows.save_subset(name, list).ok_or_else(|| invalid("rows.subsets: too many subsets"))?;
        }
    }
    if let Some(active) = r.get("active_subset").filter(|v| !matches!(v, JsonValue::Null)) {
        let name = active.as_str().ok_or_else(|| invalid("rows.active_subset must be a subset's name"))?;
        let index = rows
            .subset_names()
            .iter()
            .position(|n| n == name)
            .ok_or_else(|| invalid(format!("rows.active_subset: no subset is called {name:?}")))?;
        rows.set_active_subset(Some(index));
    }

    let mut reports = Vec::new();
    let mut skipped_reports = Vec::new();
    for (i, v) in array(doc, "reports")?.iter().enumerate() {
        match report_from_json(v, &table) {
            Ok(report) => reports.push(report),
            Err(e) => {
                let kind = v.get("analysis").and_then(JsonValue::as_str).unwrap_or("unknown");
                notes.push(format!(
                    "Table {}: report {} ({kind}) could not be restored and was left out: {e}",
                    table.name(),
                    i + 1
                ));
                skipped_reports.push(i);
            }
        }
    }
    let origin = match doc.get("origin") {
        None | Some(JsonValue::Null) => None,
        Some(v) => Some(TableOrigin::from_json(v)?),
    };
    let external = match doc.get("external_source") {
        None | Some(JsonValue::Null) => None,
        Some(v) => Some(crate::external::ExternalSource::from_json(v)?),
    };
    Ok(LoadedTable { table, rows, reports, origin, external, notes, skipped_reports })
}

/// The `scripts` member of a project document (empty when absent).
pub(crate) fn read_scripts(doc: &JsonValue) -> Result<Vec<ProjectScript>> {
    match doc.get("scripts") {
        None | Some(JsonValue::Null) => Ok(Vec::new()),
        Some(_) => array(doc, "scripts")?.iter().map(ProjectScript::from_json).collect(),
    }
}

/// The JSON form of a project's scripts.
pub(crate) fn scripts_json(scripts: &[ProjectScript]) -> JsonValue {
    JsonValue::Array(scripts.iter().map(ProjectScript::to_json).collect())
}

/// A column in the project-file form: `name`, `type` (`"numeric"`,
/// `"text"` or `"datetime"` with its display settings), `level` and
/// `values` (missing numbers as `null`).
#[must_use]
pub fn column_to_json(c: &Column) -> JsonValue {
    let (kind, values) = match &c.data {
        ColumnData::Numeric(v) | ColumnData::DateTime { values: v, .. } => (
            if c.temporal().is_some() { "datetime" } else { "numeric" },
            v.iter().map(|&x| JsonValue::number(x)).collect(),
        ),
        ColumnData::Text(v) => ("text", v.iter().map(|s| JsonValue::String(s.clone())).collect()),
    };
    let mut members = vec![
        ("name".to_owned(), s(&c.name)),
        ("type".to_owned(), s(kind)),
        ("level".to_owned(), s(level_name(c.level))),
    ];
    if let Some(d) = c.temporal() {
        members.push(("format".to_owned(), s(d.format.key())));
        members.push(("fraction_digits".to_owned(), JsonValue::Number(f64::from(d.fraction_digits))));
        members.push(("offset_minutes".to_owned(), JsonValue::Number(f64::from(d.offset_minutes))));
    }
    if let Some(f) = &c.formula {
        members.push(("formula".to_owned(), s(f)));
    }
    // Value labels and the hover choice only when set (older builds ignore them).
    if !c.value_labels.is_empty() {
        let pairs = c.value_labels.iter().map(|(code, label)| JsonValue::Array(vec![s(code), s(label)])).collect();
        members.push(("value_labels".to_owned(), JsonValue::Array(pairs)));
    }
    if c.hover {
        members.push(("hover".to_owned(), JsonValue::Bool(true)));
    }
    members.push(("values".to_owned(), JsonValue::Array(values)));
    JsonValue::Object(members)
}

/// Parse a column written by [`column_to_json`].
///
/// # Errors
/// [`StatError::InvalidArgument`] naming the problem: a missing member, a
/// value of the wrong type, an unknown type or level, or a continuous text
/// column.
pub fn column_from_json(c: &JsonValue) -> Result<Column> {
    let cname = member(c, "name")?.as_str().ok_or_else(|| invalid("column name must be a string"))?;
    let formula = match c.get("formula") {
        None | Some(JsonValue::Null) => None,
        Some(JsonValue::String(f)) => Some(f.clone()),
        Some(_) => return Err(invalid(format!("column {cname:?}: the formula must be text"))),
    };
    // A formula column may leave out its values (they are computed).
    let no_values = Vec::new();
    let values = if formula.is_some() && c.get("values").is_none() { &no_values[..] } else { array(c, "values")? };
    let numbers = || {
        values
            .iter()
            .map(|v| match v {
                JsonValue::Null => Ok(f64::NAN),
                JsonValue::Number(x) => Ok(*x),
                _ => Err(invalid(format!("column {cname:?}: numeric values must be numbers or null"))),
            })
            .collect::<Result<Vec<f64>>>()
    };
    let mut column = match member(c, "type")?.as_str() {
        Some("numeric") => Column::numeric(cname, numbers()?),
        Some("datetime") => Column::date_time(cname, numbers()?, read_temporal_display(c, cname)?),
        Some("text") => Column::text(
            cname,
            values
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| invalid(format!("column {cname:?}: text values must be strings")))
                })
                .collect::<Result<_>>()?,
        ),
        _ => return Err(invalid(format!("column {cname:?}: type must be \"numeric\", \"text\" or \"datetime\""))),
    };
    column.level = member(c, "level")?
        .as_str()
        .and_then(parse_level)
        .ok_or_else(|| invalid(format!("column {cname:?}: unknown measurement level")))?;
    if column.level == MeasurementLevel::Continuous && column.as_numeric().is_none() {
        return Err(invalid(format!("text column {cname:?} cannot be continuous")));
    }
    column.formula = formula;
    column.value_labels = read_value_labels(c.get("value_labels"), cname)?;
    column.hover = c.get("hover").and_then(JsonValue::as_bool).unwrap_or(false);
    Ok(column)
}

/// Value labels saved as `[[code, label], …]` pairs (none when absent).
///
/// # Errors
/// [`StatError::InvalidArgument`] when the member is not a list of pairs of
/// text.
pub fn read_value_labels(v: Option<&JsonValue>, cname: &str) -> Result<std::collections::BTreeMap<String, String>> {
    let mut labels = std::collections::BTreeMap::new();
    let Some(v) = v.filter(|v| !matches!(v, JsonValue::Null)) else { return Ok(labels) };
    let bad = || invalid(format!("column {cname:?}: value_labels must be [value, label] pairs of text"));
    for pair in v.as_array().ok_or_else(bad)? {
        match pair.as_array() {
            Some([JsonValue::String(code), JsonValue::String(label)]) => {
                if !code.is_empty() && !label.trim().is_empty() {
                    labels.insert(code.clone(), label.clone());
                }
            }
            _ => return Err(bad()),
        }
    }
    Ok(labels)
}

/// How a saved date-time column is shown.
fn read_temporal_display(c: &JsonValue, cname: &str) -> Result<stat_data::TemporalDisplay> {
    let bad = |what: &str| invalid(format!("column {cname:?}: invalid {what}"));
    let format = member(c, "format")?
        .as_str()
        .and_then(stat_data::TemporalFormat::from_key)
        .ok_or_else(|| bad("date-time format"))?;
    let fraction_digits = member(c, "fraction_digits")?
        .as_usize()
        .and_then(|d| u8::try_from(d).ok())
        .filter(|&d| d <= stat_data::temporal::MAX_FRACTION_DIGITS)
        .ok_or_else(|| bad("fraction_digits"))?;
    let offset = member(c, "offset_minutes")?.as_f64().ok_or_else(|| bad("offset_minutes"))?;
    let max = f64::from(stat_data::temporal::MAX_OFFSET_MINUTES);
    if offset.fract() != 0.0 || offset.abs() > max {
        return Err(bad("offset_minutes"));
    }
    // Checked above to be a whole number within ±18 hours.
    let offset_minutes = offset as i16;
    Ok(stat_data::TemporalDisplay { format, fraction_digits, offset_minutes })
}

/// Profiler toggles of a saved report (defaults when absent).
fn read_profilers(v: &JsonValue) -> ProfilerOptions {
    let d = ProfilerOptions::default();
    let flag = |key: &str, default: bool| v.get(key).and_then(JsonValue::as_bool).unwrap_or(default);
    ProfilerOptions {
        show_profiler: flag("show_profiler", d.show_profiler),
        show_surface_profiler: flag("show_surface_profiler", d.show_surface_profiler),
        show_observed: flag("show_observed", d.show_observed),
    }
}

/// [`report_to_json`], refusing a report that refers to a column index the
/// table does not have (which [`report_to_json`] would write as an empty
/// name that no table can resolve).
///
/// # Errors
/// [`StatError::InvalidArgument`] naming the first out-of-range column.
pub fn checked_report_to_json(report: &SavedReport, table: &DataTable) -> Result<JsonValue> {
    if let Some(&bad) = report.columns().iter().find(|&&c| c >= table.n_cols()) {
        return Err(invalid(format!(
            "the report refers to column {} but the table has {} columns",
            bad + 1,
            table.n_cols()
        )));
    }
    Ok(report_to_json(report, table))
}

/// The JSON form of one report request, as stored in a project's `"reports"`
/// array: `{"analysis": key, …settings, "options": {…}}`, columns by name.
/// A column index out of range is written as an empty name; project files
/// use [`checked_report_to_json`], which refuses such a report.
#[must_use]
pub fn report_to_json(report: &SavedReport, table: &DataTable) -> JsonValue {
    let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
    let cols = |c: &[usize]| JsonValue::Array(c.iter().map(|&i| col(i)).collect());
    let b = JsonValue::Bool;
    match report {
        SavedReport::Scoped { scope, report } => {
            let mut v = report_to_json(report, table);
            if let JsonValue::Object(members) = &mut v {
                members.extend(scope.json_members());
            }
            v
        }
        SavedReport::Linear { spec, options } => obj([
            ("analysis", s("linear-model")),
            ("response", col(spec.response)),
            ("predictors", JsonValue::Array(spec.predictors.iter().map(|&p| col(p)).collect())),
            // Each effect: [[position in predictors, power], …]; empty = main effects only.
            ("effects", effects_json(&spec.effects)),
            ("splines", splines_json(&spec.splines)),
            (
                "options",
                obj([
                    ("show_anova", b(options.show_anova)),
                    ("show_confidence_limits", b(options.show_confidence_limits)),
                    ("show_fit_plot", b(options.show_fit_plot)),
                    ("show_confidence_band", b(options.show_confidence_band)),
                    ("show_residual_plot", b(options.show_residual_plot)),
                    ("show_diagnostics", b(options.show_diagnostics)),
                    ("show_profiler", b(options.show_profiler)),
                    ("show_surface_profiler", b(options.show_surface_profiler)),
                    ("show_surface_observed", b(options.show_surface_observed)),
                    ("pairwise", s(options.pairwise.map_or("none", method_name))),
                    ("control_level", options.control_level.map_or(JsonValue::Null, |c| JsonValue::number(c as f64))),
                    ("best", options.best.map_or(JsonValue::Null, |d| s(best_name(d)))),
                    ("show_equal_variances", b(options.show_equal_variances)),
                    ("show_effect_sizes", b(options.show_effect_sizes)),
                    ("linear_trend", b(options.linear_trend)),
                    ("compare_slices", adjust_json(options.slices)),
                    ("compare_slopes", adjust_json(options.slopes)),
                ]),
            ),
        ]),
        SavedReport::ControlChart { request, options } => obj([
            ("analysis", s("control-chart")),
            ("column", col(request.column)),
            ("chart", s(chart_name(request.kind))),
            ("subgroup_size", JsonValue::Number(request.subgroup_size as f64)),
            ("subgroup", request.subgroup.map_or(JsonValue::Null, col)),
            ("phase", request.phase.map_or(JsonValue::Null, col)),
            ("label", request.label.map_or(JsonValue::Null, col)),
            ("span", JsonValue::Number(request.span as f64)),
            ("part", request.part.map_or(JsonValue::Null, col)),
            ("sigma_multiple", JsonValue::number(request.sigma_multiple)),
            ("range_span", JsonValue::Number(request.range_span as f64)),
            (
                "standard",
                request.standard.map_or(JsonValue::Null, |sv| {
                    obj([("center", JsonValue::number(sv.center)), ("sigma", JsonValue::number(sv.sigma))])
                }),
            ),
            (
                "last",
                request.last.map_or(JsonValue::Null, |l| {
                    obj([
                        ("count", JsonValue::Number(l.count as f64)),
                        ("limits_from", s(if l.limits_from_last { "last" } else { "all" })),
                    ])
                }),
            ),
            (
                "options",
                obj([
                    ("show_dispersion_chart", b(options.show_dispersion_chart)),
                    ("show_zones", b(options.show_zones)),
                    ("location_rules", rule_numbers(options.location_rules)),
                    ("dispersion_rules", rule_numbers(options.dispersion_rules)),
                    ("within_rules", rule_numbers(options.within_rules)),
                    ("show_violation_table", b(options.show_violation_table)),
                    ("show_run_chart", b(options.show_run_chart)),
                    ("run_center", s(options.run_center.key())),
                ]),
            ),
        ]),
        SavedReport::Gauge { request, options } => obj([
            ("analysis", s("gauge-study")),
            ("part", col(request.part)),
            ("operator", col(request.operator)),
            ("measurement", col(request.measurement)),
            ("tolerance", request.tolerance.map_or(JsonValue::Null, JsonValue::number)),
            ("pool_interaction", b(request.pool_interaction)),
            ("study_multiplier", JsonValue::number(request.study_multiplier)),
            ("design", s(request.design.kind.key())),
            ("parts_within_operators", b(request.design.parts_within_operators)),
            ("interactions", b(request.design.interactions)),
            ("factors", request.design.factors_json(table)),
            ("run_order", request.design.run_order.map_or(JsonValue::Null, col)),
            (
                "options",
                obj([
                    ("show_anova", b(options.show_anova)),
                    ("show_part_plot", b(options.show_part_plot)),
                    ("show_operator_plot", b(options.show_operator_plot)),
                    ("show_run_chart", b(options.show_run_chart)),
                ]),
            ),
        ]),
        SavedReport::Distribution { request, options } => {
            let mut members: Vec<(&str, JsonValue)> = vec![
                ("analysis", s("distribution")),
                ("column", col(request.column)),
                ("quantile_type", JsonValue::Number(f64::from(request.quantile_method.type_number()))),
            ];
            if let Some(c) = &request.censoring {
                c.json_members(table, &mut members);
            }
            members.push((
                "options",
                obj([
                    ("show_quantiles", b(options.show_quantiles)),
                    ("show_robust", b(options.show_robust)),
                    ("show_chart", b(options.show_chart)),
                    ("show_box_plot", b(options.show_box_plot)),
                    ("show_normal_fit", b(options.show_normal_fit)),
                    ("show_quantile_plot", b(options.show_quantile_plot)),
                    ("show_box_cox", b(options.show_box_cox)),
                    ("tolerance", options.tolerance.map_or(JsonValue::Null, |t| tolerance_to_json(&t))),
                    ("outliers", options.outliers.map_or(JsonValue::Null, |o| outliers_to_json(&o))),
                    ("runs", options.runs.map_or(JsonValue::Null, |r| runs_to_json(&r))),
                ]),
            ));
            JsonValue::Object(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
        }
        SavedReport::Bivariate { request } => obj([
            ("analysis", s("bivariate")),
            ("x", col(request.x)),
            ("y", col(request.y)),
            ("pairwise", s(request.pairwise.map_or("none", method_name))),
        ]),
        SavedReport::Correspondence { request } => {
            obj([("analysis", s("correspondence")), ("rows", col(request.row)), ("columns", col(request.column))])
        }
        SavedReport::Correlations { request } => request.to_json(table),
        SavedReport::OutlierDistances { request } => request.to_json(table),
        SavedReport::ItemAnalysis { request } => request.to_json(table),
        SavedReport::Manova { request } => request.to_json(table),
        SavedReport::FactorAnalysis { request } => request.to_json(table),
        SavedReport::DiscriminantAnalysis { request } => request.to_json(table),
        SavedReport::GaussianMixture { request } => request.to_json(table),
        SavedReport::LatentClass { request } => request.to_json(table),
        SavedReport::TextExploration { request } => request.to_json(table),
        SavedReport::TopicModel { request } => request.to_json(table),
        SavedReport::Sentiment { request } => request.to_json(table),
        SavedReport::Embedding { request } => request.to_json(table),
        SavedReport::Imputation { request } => request.to_json(table),
        SavedReport::SeriesExplore { request } => request.to_json(table),
        SavedReport::SignalProcessing { request } => request.to_json(table),
        SavedReport::ImageAnalysis { request } => request.to_json(table),
        SavedReport::Survival { request, options } => obj([
            ("analysis", s("survival")),
            ("time", col(request.time)),
            ("status", request.status.map_or(JsonValue::Null, col)),
            (
                "status_coding",
                s(match request.coding {
                    StatusCoding::NonzeroIsCensored => "nonzero-censored",
                    StatusCoding::NonzeroIsEvent => "nonzero-event",
                }),
            ),
            ("group", request.group.map_or(JsonValue::Null, col)),
            (
                "options",
                obj([
                    ("show_band", b(options.show_band)),
                    ("band", s(band_name(options.band))),
                    ("show_weibull", b(options.show_weibull)),
                    ("show_lognormal", b(options.show_lognormal)),
                    ("show_table", b(options.show_table)),
                    ("trend", b(options.trend)),
                    ("risk_table", b(options.risk_table)),
                    ("risk_censored", b(options.risk_censored)),
                ]),
            ),
        ]),
        SavedReport::OneSample { .. }
        | SavedReport::Groups { .. }
        | SavedReport::Paired { .. }
        | SavedReport::Proportions { .. }
        | SavedReport::Rates { .. }
        | SavedReport::Power { .. } => comparison_to_json(report, table),
        SavedReport::AttributeCapability { request } => request.to_json(table),
        SavedReport::BetweenWithinCapability { request } => request.to_json(table),
        SavedReport::AcceptanceSampling { request } => request.to_json(),
        SavedReport::GaugeLinearity { request } => request.to_json(table),
        SavedReport::GaugeRepeatability { request } => request.to_json(table),
        SavedReport::IntraclassCorrelation { request } => request.to_json(table),
        SavedReport::AttributeAgreement { request } => request.to_json(table),
        SavedReport::AttributeGauge { request } => request.to_json(table),
        SavedReport::PcaChart { request } => request.to_json(table),
        SavedReport::MonitoringLimits { request } => request.to_json(table),
        SavedReport::MultiwayTable { request } => request.to_json(table),
        SavedReport::StratifiedTables { request } => request.to_json(table),
        SavedReport::Bootstrap { request } => request.to_json(table),
        SavedReport::PermutationTest { request } => request.to_json(table),
        SavedReport::SignalToNoise { request } => request.to_json(table),
        SavedReport::DeclaredDesign { request } => request.to_json(table),
        SavedReport::DesignEvaluation { request } => request.to_json(table),
        SavedReport::FactorialAnalysis { request } => request.to_json(table),
        SavedReport::GoodnessOfFit { request } => request.to_json(table),
        SavedReport::Crossover { request } => request.to_json(table),
        SavedReport::AdjustPValues { request } => request.to_json(table),
        SavedReport::AreaUnderCurve { request } => request.to_json(table),
        SavedReport::ManyTests { request } => request.to_json(table),
        SavedReport::TreatmentEffects { request } => request.to_json(table),
        SavedReport::Screening { request } => request.to_json(table),
        SavedReport::FactorialRepeated { request } => request.to_json(table),
        SavedReport::Nested { request } => request.to_json(table),
        SavedReport::Bayesian { request } => request.to_json(table),
        SavedReport::MultipleResponseItems { request } => request.to_json(table),
        SavedReport::Signal { request } => request.to_json(table),
        SavedReport::ImagePlot { request } => request.to_json(table),
        SavedReport::DistributionCalculator { request } => request.to_json(),
        SavedReport::Pca { request } => obj([
            ("analysis", s("principal-components")),
            ("columns", cols(&request.columns)),
            ("standardize", b(request.standardize)),
        ]),
        SavedReport::Cluster { request } => {
            let (method, k, seed, linkage) = match request.method {
                ClusterMethod::KMeans { k, seed } => ("k-means", k, seed, None),
                ClusterMethod::Hierarchical { linkage, k } => ("hierarchical", k, 0, Some(linkage)),
            };
            obj([
                ("analysis", s("clustering")),
                ("columns", cols(&request.columns)),
                (
                    "distance",
                    s(match request.distance {
                        crate::ClusterDistance::Euclidean => "euclidean",
                        crate::ClusterDistance::Matrix => "matrix",
                        crate::ClusterDistance::Metric(m) => m.key(),
                    }),
                ),
                ("standardize", b(request.standardize)),
                ("method", s(method)),
                ("k", JsonValue::Number(k as f64)),
                ("seed", JsonValue::Number(seed as f64)),
                ("linkage", linkage.map_or(JsonValue::Null, |l| s(linkage_name(l)))),
                (
                    "centers",
                    request.centers.as_ref().map_or(JsonValue::Null, |c| {
                        JsonValue::Array(
                            c.centers
                                .iter()
                                .map(|r| JsonValue::Array(r.iter().map(|&v| JsonValue::Number(v)).collect()))
                                .collect(),
                        )
                    }),
                ),
                ("centers_also_seeded", b(request.centers.as_ref().is_some_and(|c| c.also_seeded))),
                (
                    "choose_k",
                    request.choose.map_or(JsonValue::Null, |c| {
                        obj([
                            ("max_k", JsonValue::Number(c.max_k as f64)),
                            ("references", JsonValue::Number(c.references as f64)),
                            (
                                "reference",
                                s(match c.reference {
                                    crate::GapReference::DataRange => "data-range",
                                    crate::GapReference::PrincipalComponents => "principal-components",
                                }),
                            ),
                        ])
                    }),
                ),
            ])
        }
        SavedReport::Glm { request, profilers } => obj([
            ("analysis", s("generalized-linear-model")),
            ("show_profiler", b(profilers.show_profiler)),
            ("show_surface_profiler", b(profilers.show_surface_profiler)),
            ("show_observed", b(profilers.show_observed)),
            ("response", col(request.response)),
            ("predictors", cols(&request.predictors)),
            ("family", s(family_name(request.family))),
            ("link", request.link.map_or(JsonValue::Null, |l| s(l.key()))),
            ("event", request.event.clone().map_or(JsonValue::Null, JsonValue::String)),
            ("effects", effects_json(&request.effects)),
            ("splines", splines_json(&request.splines)),
            ("compare_slices", adjust_json(request.slices)),
            ("compare_slopes", adjust_json(request.slopes)),
        ]),
        SavedReport::Graph { spec } => crate::graph_json::graph_to_json(spec),
        SavedReport::Scatter3d { request, options } => crate::scatter3d::scatter3d_to_json(request, options, table),
        SavedReport::DataSurface { request, options } => {
            crate::data_surface::data_surface_to_json(request, options, table)
        }
        SavedReport::Mixture { request, profilers } => obj([
            ("analysis", s("mixture-model")),
            ("show_profiler", b(profilers.show_profiler)),
            ("response", col(request.response)),
            ("components", cols(&request.components)),
            (
                "model",
                s(match request.model {
                    ScheffeModel::Linear => "linear",
                    ScheffeModel::Quadratic => "quadratic",
                    ScheffeModel::SpecialCubic => "special-cubic",
                }),
            ),
        ]),
        SavedReport::Stepwise { request, options } => {
            let c = &request.candidates;
            let mut members = vec![
                ("analysis", s("stepwise")),
                ("show_profiler", b(options.show_profiler)),
                ("show_surface_profiler", b(options.show_surface_profiler)),
                ("show_observed", b(options.show_observed)),
                ("response", col(c.response)),
                ("predictors", JsonValue::Array(c.predictors.iter().map(|&p| col(p)).collect())),
                ("effects", effects_json(&c.effects)),
            ];
            members.extend(crate::stepwise_analysis::search_to_json(
                request.direction,
                request.criterion,
                request.heredity,
            ));
            JsonValue::Object(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
        }
        SavedReport::BestSubsets { request } => {
            let c = &request.candidates;
            obj([
                ("analysis", s("best-subsets")),
                ("response", col(c.response)),
                ("predictors", JsonValue::Array(c.predictors.iter().map(|&p| col(p)).collect())),
                ("effects", effects_json(&c.effects)),
                ("forced", effects_json(&request.forced)),
                ("heredity", s(request.heredity.key())),
                ("models_per_size", JsonValue::number(request.models_per_size as f64)),
                ("max_size", request.max_size.map_or(JsonValue::Null, |m| JsonValue::number(m as f64))),
                ("criterion", s(subset_criterion_name(request.criterion))),
            ])
        }
        SavedReport::DistributionFit { request } => {
            let mut members = vec![("analysis", s("distribution-identification")), ("column", col(request.column))];
            if request.families != FitFamily::ALL {
                members
                    .push(("distributions", JsonValue::Array(request.families.iter().map(|f| s(f.key())).collect())));
            }
            members.push(("transformations", JsonValue::Bool(request.transformations)));
            members.push(("plots", JsonValue::number(request.plots as f64)));
            JsonValue::Object(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
        }
        SavedReport::Attribute { request } => {
            let mut members = vec![
                ("analysis", s("attribute-chart")),
                ("chart", s(request.kind.key())),
                ("count", col(request.count)),
            ];
            match request.size {
                SubgroupSize::Column(c) => members.push(("size", col(c))),
                SubgroupSize::Constant(n) => members.push(("size", JsonValue::number(n))),
                SubgroupSize::None => {}
            }
            if let Some(p) = request.phase {
                members.push(("phase", col(p)));
            }
            if request.rules != RuleSet::LIMITS_ONLY {
                members.push(("rules", rule_numbers(request.rules)));
            }
            JsonValue::Object(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
        }
        SavedReport::Pareto { request, options } => obj([
            ("analysis", s("pareto-chart")),
            ("category", col(request.category)),
            ("frequency", request.frequency.map_or(JsonValue::Null, col)),
            ("group", request.group.map_or(JsonValue::Null, col)),
            (
                "options",
                obj([
                    ("other_percent", JsonValue::number(options.other_percent)),
                    ("percent_scale", b(options.percent_scale)),
                ]),
            ),
        ]),
        SavedReport::CauseEffect { request } => obj([
            ("analysis", s("cause-and-effect")),
            ("causes", cols(&request.causes)),
            ("effect", s(&request.effect)),
        ]),
        SavedReport::MultiVari { request, options } => obj([
            ("analysis", s("multi-vari")),
            ("response", col(request.response)),
            ("factors", cols(&request.factors)),
            (
                "options",
                obj([
                    ("show_values", b(options.show_values)),
                    ("show_ranges", b(options.show_ranges)),
                    ("show_cells", b(options.show_cells)),
                    ("replicates", JsonValue::String(options.replicates.key().to_owned())),
                ]),
            ),
        ]),
        SavedReport::MultivariateChart { request } => {
            let num = JsonValue::number;
            let mut members = vec![
                ("analysis", s("multivariate-chart")),
                ("chart", s(request.kind.key())),
                ("columns", cols(&request.columns)),
                ("subgroup_size", num(request.subgroup_size as f64)),
                ("alpha", num(request.alpha)),
                ("lambda", num(request.lambda)),
            ];
            if let Some(c) = request.subgroup {
                members.push(("subgroup", col(c)));
            }
            if let Some(c) = request.label {
                members.push(("label", col(c)));
            }
            if let Some(m) = request.reference {
                members.push(("reference", num(m as f64)));
            }
            match request.mewma_limit {
                crate::MewmaLimit::Value(h) => members.push(("mewma_limit", num(h))),
                crate::MewmaLimit::InControlArl(arl) => members.push(("mewma_arl", num(arl))),
            }
            JsonValue::Object(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
        }
        SavedReport::Growth { .. }
        | SavedReport::Recurrence { .. }
        | SavedReport::LifeDistribution { .. }
        | SavedReport::CompetingRisks { .. }
        | SavedReport::Warranty { .. }
        | SavedReport::LifeStress { .. }
        | SavedReport::Degradation { .. }
        | SavedReport::ProportionalHazards { .. }
        | SavedReport::SurvivalRegression { .. }
        | SavedReport::TestPlan { .. } => crate::reliability_json::to_json(report, table).unwrap_or(JsonValue::Null),
        SavedReport::ExponentialSmoothing { request } => request.to_json(table),
        SavedReport::Arima { request } => request.to_json(table),
        SavedReport::PrincipalCoordinates { request } => request.to_json(table),
        SavedReport::Pharmacokinetics { request } => request.to_json(table),
        SavedReport::Fatigue { request } => request.to_json(table),
        SavedReport::Anom { request } => {
            let num = JsonValue::number;
            let mut members = vec![
                ("analysis", s("analysis-of-means")),
                ("data", s(request.kind.key())),
                ("response", col(request.response)),
                ("alpha", num(request.alpha)),
            ];
            match request.size {
                SubgroupSize::Column(c) => members.push(("size", col(c))),
                SubgroupSize::Constant(n) => members.push(("size", num(n))),
                SubgroupSize::None => {}
            }
            if let Some(c) = request.group {
                members.push(("group", col(c)));
            }
            if let Some(c) = request.second {
                members.push(("second_group", col(c)));
            }
            JsonValue::Object(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
        }
        SavedReport::CategoricalLogistic { request } => crate::categorical_logistic::to_json(request, table),
        SavedReport::Nonlinear { request, profilers } => crate::nonlinear_analysis::to_json(request, *profilers, table),
        SavedReport::DoseResponse { request } => {
            let num = JsonValue::number;
            let mut members = vec![
                ("analysis", s("dose-response")),
                ("dose", col(request.dose)),
                ("response", col(request.response)),
                ("link", s(request.link.key())),
                ("scale", s(request.scale.key())),
                ("probabilities", JsonValue::Array(request.probabilities.iter().map(|&p| num(p)).collect())),
                ("level", num(request.confidence_level)),
                ("heterogeneity_alpha", num(request.heterogeneity_alpha)),
            ];
            if let Some(c) = request.trials {
                members.push(("trials", col(c)));
            }
            JsonValue::Object(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
        }
        SavedReport::MethodComparison { request } => obj([
            ("analysis", s("method-comparison")),
            ("reference", col(request.reference)),
            ("test", col(request.test)),
            ("deming", b(request.deming)),
            ("passing_bablok", b(request.passing_bablok)),
            ("variance_ratio", JsonValue::number(request.variance_ratio)),
            ("level", JsonValue::number(request.confidence_level)),
        ]),
        SavedReport::ShelfLife { request } => {
            let num = JsonValue::number;
            let mut members = vec![
                ("analysis", s("shelf-life")),
                ("time", col(request.time)),
                ("response", col(request.response)),
                ("pooling_alpha", num(request.pooling_alpha)),
                ("level", num(request.confidence_level)),
            ];
            if let Some(c) = request.batch {
                members.push(("batch", col(c)));
            }
            members.push(("time_unit", s(request.time_unit.map_or("none", stat_linear::StabilityTimeUnit::label))));
            for (key, v) in [("lower", request.lower), ("upper", request.upper), ("horizon", request.horizon)] {
                if let Some(v) = v {
                    members.push((key, num(v)));
                }
            }
            if request.random_batches {
                members.push(("batches", s("random")));
                members.push(("bound", s(if request.future_batch { "future-batch" } else { "mean" })));
            }
            JsonValue::Object(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
        }
        SavedReport::Capability { request } => {
            let num = JsonValue::number;
            let mut members = vec![("analysis", s("process-capability")), ("column", col(request.column))];
            let l = &request.limits;
            for (key, v) in [("lower", l.lower), ("upper", l.upper), ("target", l.target)] {
                if let Some(v) = v {
                    members.push((key, num(v)));
                }
            }
            let (method, size) = match request.within {
                WithinSigma::MovingRange => ("moving-range", None),
                WithinSigma::AverageRange { subgroup_size } => ("average-range", Some(subgroup_size)),
                WithinSigma::PooledStdDev { subgroup_size } => ("pooled-sd", Some(subgroup_size)),
            };
            members.push(("within", s(method)));
            if let Some(m) = size {
                members.push(("subgroup_size", JsonValue::Number(m as f64)));
            }
            if let Some(lambda) = request.box_cox {
                members.push(("box_cox", num(lambda)));
            }
            if request.model != CapabilityModel::Normal {
                members.push(("distribution", s(request.model.key())));
            }
            if request.summary {
                members.push(("layout", s("summary")));
            }
            if let Some(by) = request.by {
                members.push(("by", col(by)));
                if request.group_order != crate::GroupOrder::Labels {
                    members.push(("group_order", s(request.group_order.key())));
                }
            }
            if request.probability_plot {
                members.push(("probability_plot", JsonValue::Bool(true)));
            }
            if let Some(boot) = request.bootstrap {
                members.push((
                    "bootstrap",
                    obj([
                        ("resamples", JsonValue::Number(boot.resamples as f64)),
                        ("seed", JsonValue::Number(boot.seed as f64)),
                    ]),
                ));
            }
            JsonValue::Object(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
        }
        SavedReport::TimeWeighted { request } => {
            let num = JsonValue::number;
            let mut members = vec![("column", col(request.column))];
            match request.kind {
                TimeWeightedKind::Ewma { lambda, width } => {
                    members.push(("analysis", s("ewma-chart")));
                    members.push(("lambda", num(lambda)));
                    members.push(("width", num(width)));
                }
                TimeWeightedKind::Cusum { k, h } => {
                    members.push(("analysis", s("cusum-chart")));
                    members.push(("k", num(k)));
                    members.push(("h", num(h)));
                }
            }
            if let Some((mean, sigma)) = request.target {
                members.push(("target_mean", num(mean)));
                members.push(("target_sigma", num(sigma)));
            }
            JsonValue::Object(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
        }
        SavedReport::Surface { request, options } => obj([
            ("analysis", s("response-surface")),
            ("response", col(request.response)),
            ("factors", cols(&request.factors)),
            (
                "options",
                obj([
                    ("show_coefficients", b(options.show_coefficients)),
                    ("show_ridge", b(options.show_ridge)),
                    ("contour_x", JsonValue::Number(options.contour_x as f64)),
                    ("show_surface_profiler", b(options.show_surface_profiler)),
                    ("show_surface_observed", b(options.show_surface_observed)),
                    ("show_profiler", b(options.show_profiler)),
                    ("contour_y", JsonValue::Number(options.contour_y as f64)),
                ]),
            ),
        ]),
        SavedReport::Mixed { request, profilers } => obj([
            ("analysis", s("mixed-model")),
            ("show_profiler", b(profilers.show_profiler)),
            ("show_surface_profiler", b(profilers.show_surface_profiler)),
            ("show_observed", b(profilers.show_observed)),
            ("response", col(request.response)),
            ("predictors", cols(&request.predictors)),
            ("effects", effects_json(&request.effects)),
            ("compare_slices", adjust_json(request.slices)),
            ("compare_slopes", adjust_json(request.slopes)),
            (
                "random",
                JsonValue::Array(
                    request
                        .random
                        .iter()
                        .map(|r| {
                            obj([
                                ("group", col(r.group)),
                                ("within", cols(&r.within)),
                                ("intercept", b(r.intercept)),
                                ("slopes", cols(&r.slopes)),
                                ("correlated", b(r.correlated)),
                            ])
                        })
                        .collect(),
                ),
            ),
            ("family", s(request.family.key())),
            ("trials", request.trials.map_or(JsonValue::Null, col)),
            ("offset", request.offset.map_or(JsonValue::Null, col)),
            ("reml", b(request.reml)),
            ("quadrature_points", JsonValue::Number(request.quadrature_points as f64)),
            ("frequency", request.frequency.map_or(JsonValue::Null, col)),
            ("weight", request.weight.map_or(JsonValue::Null, col)),
            ("robust", b(request.robust)),
            ("residual_groups", request.residual_groups.map_or(JsonValue::Null, col)),
            (
                "df_method",
                s(match request.df_method {
                    stat_mixed::DfMethod::Satterthwaite => "satterthwaite",
                    stat_mixed::DfMethod::BetweenWithin => "between-within",
                }),
            ),
            ("order", JsonValue::Array(request.order.iter().map(|o| s(o)).collect())),
        ]),
        SavedReport::MultipleResponses { request } => {
            let mut members = vec![
                ("analysis".to_owned(), s("several-responses")),
                ("responses".to_owned(), cols(&request.responses)),
                ("predictors".to_owned(), cols(&request.predictors)),
            ];
            members.extend(
                crate::multiple_responses::request_members_to_json(request).into_iter().map(|(k, v)| (k.to_owned(), v)),
            );
            JsonValue::Object(members)
        }
        SavedReport::Regularized { request, profilers } => obj([
            ("analysis", s("penalized-regression")),
            ("show_profiler", b(profilers.show_profiler)),
            ("show_surface_profiler", b(profilers.show_surface_profiler)),
            ("show_observed", b(profilers.show_observed)),
            ("response", col(request.response)),
            ("predictors", cols(&request.predictors)),
            ("alpha", JsonValue::number(request.alpha)),
            ("folds", JsonValue::Number(request.folds as f64)),
            ("seed", JsonValue::Number(request.seed as f64)),
        ]),
        SavedReport::DecisionTree { request, profilers } => {
            let mut members = vec![
                ("analysis".to_owned(), s("decision-tree")),
                ("show_profiler".to_owned(), b(profilers.show_profiler)),
                ("show_surface_profiler".to_owned(), b(profilers.show_surface_profiler)),
                ("show_observed".to_owned(), b(profilers.show_observed)),
                ("response".to_owned(), col(request.response)),
                ("predictors".to_owned(), cols(&request.predictors)),
            ];
            members.extend(
                crate::tree_analysis::request_members_to_json(request, &col)
                    .into_iter()
                    .map(|(k, v)| (k.to_owned(), v)),
            );
            JsonValue::Object(members)
        }
        SavedReport::RandomForest { request, profilers } => {
            let mut members = vec![
                ("analysis".to_owned(), s("random-forest")),
                ("show_profiler".to_owned(), b(profilers.show_profiler)),
                ("show_surface_profiler".to_owned(), b(profilers.show_surface_profiler)),
                ("show_observed".to_owned(), b(profilers.show_observed)),
                ("response".to_owned(), col(request.response)),
                ("predictors".to_owned(), cols(&request.predictors)),
            ];
            members.extend(
                crate::forest_analysis::request_members_to_json(request, &col)
                    .into_iter()
                    .map(|(k, v)| (k.to_owned(), v)),
            );
            JsonValue::Object(members)
        }
        SavedReport::BoostedTrees { request, profilers } => {
            let mut members = vec![
                ("analysis".to_owned(), s("boosted-trees")),
                ("show_profiler".to_owned(), b(profilers.show_profiler)),
                ("show_surface_profiler".to_owned(), b(profilers.show_surface_profiler)),
                ("show_observed".to_owned(), b(profilers.show_observed)),
                ("response".to_owned(), col(request.response)),
                ("predictors".to_owned(), cols(&request.predictors)),
            ];
            members.extend(
                crate::boost_analysis::request_members_to_json(request, &col)
                    .into_iter()
                    .map(|(k, v)| (k.to_owned(), v)),
            );
            JsonValue::Object(members)
        }
        SavedReport::PartialLeastSquares { request, profilers } => {
            let mut members = vec![
                ("analysis".to_owned(), s("partial-least-squares")),
                ("show_profiler".to_owned(), b(profilers.show_profiler)),
                ("show_surface_profiler".to_owned(), b(profilers.show_surface_profiler)),
                ("show_observed".to_owned(), b(profilers.show_observed)),
                ("response".to_owned(), col(request.response)),
                ("predictors".to_owned(), cols(&request.predictors)),
            ];
            members.extend(
                crate::pls_analysis::request_members_to_json(request, &col).into_iter().map(|(k, v)| (k.to_owned(), v)),
            );
            JsonValue::Object(members)
        }
        SavedReport::SupportVectorMachine { request, profilers } => {
            let mut members = vec![
                ("analysis".to_owned(), s("support-vector-machine")),
                ("show_profiler".to_owned(), b(profilers.show_profiler)),
                ("show_surface_profiler".to_owned(), b(profilers.show_surface_profiler)),
                ("show_observed".to_owned(), b(profilers.show_observed)),
                ("response".to_owned(), col(request.response)),
                ("predictors".to_owned(), cols(&request.predictors)),
            ];
            members.extend(
                crate::svm_analysis::request_members_to_json(request, &col).into_iter().map(|(k, v)| (k.to_owned(), v)),
            );
            JsonValue::Object(members)
        }
        SavedReport::FunctionalData { request, profilers } => request.to_json(table, profilers),
        SavedReport::Mars { request, profilers } => {
            let mut members = vec![
                ("analysis".to_owned(), s("adaptive-splines")),
                ("show_profiler".to_owned(), b(profilers.show_profiler)),
                ("show_surface_profiler".to_owned(), b(profilers.show_surface_profiler)),
                ("show_observed".to_owned(), b(profilers.show_observed)),
                ("response".to_owned(), col(request.response)),
                ("predictors".to_owned(), cols(&request.predictors)),
            ];
            members.extend(
                crate::mars_analysis::request_members_to_json(request, &col)
                    .into_iter()
                    .map(|(k, v)| (k.to_owned(), v)),
            );
            JsonValue::Object(members)
        }
        SavedReport::NeuralNetwork { request, profilers } => {
            let mut members = vec![
                ("analysis".to_owned(), s("neural-network")),
                ("show_profiler".to_owned(), b(profilers.show_profiler)),
                ("show_surface_profiler".to_owned(), b(profilers.show_surface_profiler)),
                ("show_observed".to_owned(), b(profilers.show_observed)),
                ("response".to_owned(), col(request.response)),
                ("predictors".to_owned(), cols(&request.predictors)),
            ];
            members.extend(
                crate::neural_analysis::request_members_to_json(request, &col)
                    .into_iter()
                    .map(|(k, v)| (k.to_owned(), v)),
            );
            JsonValue::Object(members)
        }
        SavedReport::GaussianProcess { request, profilers } => {
            let mut members = vec![
                ("analysis".to_owned(), s("gaussian-process")),
                ("show_profiler".to_owned(), b(profilers.show_profiler)),
                ("show_surface_profiler".to_owned(), b(profilers.show_surface_profiler)),
                ("show_observed".to_owned(), b(profilers.show_observed)),
                ("response".to_owned(), col(request.response)),
                ("predictors".to_owned(), cols(&request.predictors)),
            ];
            members.extend(
                crate::gaussian_process_analysis::request_members_to_json(request, &col)
                    .into_iter()
                    .map(|(k, v)| (k.to_owned(), v)),
            );
            JsonValue::Object(members)
        }
        SavedReport::SequentialOptimization { request } => {
            let mut members = vec![
                ("analysis".to_owned(), s("sequential-optimization")),
                ("responses".to_owned(), cols(&request.responses)),
                ("factors".to_owned(), cols(&request.factors)),
            ];
            members.extend(
                crate::sequential_optimization::request_members_to_json(request, &col)
                    .into_iter()
                    .map(|(k, v)| (k.to_owned(), v)),
            );
            JsonValue::Object(members)
        }
        SavedReport::ModelSearch { request } => {
            let mut members = vec![
                ("analysis".to_owned(), s("model-search")),
                ("response".to_owned(), col(request.response)),
                ("predictors".to_owned(), cols(&request.predictors)),
            ];
            members.extend(
                crate::model_search::request_members_to_json(request, &col).into_iter().map(|(k, v)| (k.to_owned(), v)),
            );
            JsonValue::Object(members)
        }
        SavedReport::ModelComparison { request } => {
            let mut members =
                vec![("analysis".to_owned(), s("model-comparison")), ("response".to_owned(), col(request.response))];
            members.extend(
                crate::model_comparison::request_members_to_json(request, &col)
                    .into_iter()
                    .map(|(k, v)| (k.to_owned(), v)),
            );
            JsonValue::Object(members)
        }
    }
}

fn linkage_name(l: Linkage) -> &'static str {
    match l {
        Linkage::Single => "single",
        Linkage::Complete => "complete",
        Linkage::Average => "average",
        Linkage::Ward => "ward",
    }
}

fn band_name(t: BandTransform) -> &'static str {
    match t {
        BandTransform::Plain => "plain",
        BandTransform::Log => "log",
        BandTransform::LogLog => "log-log",
    }
}

fn family_name(f: Family) -> &'static str {
    match f {
        Family::Binomial => "binomial",
        Family::Poisson => "poisson",
        Family::Gaussian => "normal",
        Family::Gamma => "gamma",
    }
}

/// Read one report request written by [`report_to_json`], resolving column
/// names against `table`. A missing display option keeps its default.
///
/// # Errors
/// [`StatError::InvalidArgument`] for an unknown analysis, a missing or
/// malformed setting, or a column `table` does not have.
pub fn report_from_json(v: &JsonValue, table: &DataTable) -> Result<SavedReport> {
    let report = request_from_json(v, table)?;
    Ok(report.with_scope(crate::report_scope::ReportScope::from_json(v, table)?))
}

/// The request of a report object, without its row filter and switcher.
fn request_from_json(v: &JsonValue, table: &DataTable) -> Result<SavedReport> {
    let col = |key: &str| -> Result<usize> {
        let name = member(v, key)?.as_str().ok_or_else(|| invalid(format!("report {key}: expected a column name")))?;
        table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
    };
    let col_list = |key: &str| -> Result<Vec<usize>> {
        array(v, key)?
            .iter()
            .map(|p| {
                let name = p.as_str().ok_or_else(|| invalid(format!("{key} must be column names")))?;
                table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
            })
            .collect()
    };
    let opts = v.get("options");
    // A missing option keeps its default, so older files stay readable when options are added.
    let flag = |key: &str, default: bool| opts.and_then(|o| o.get(key)).and_then(JsonValue::as_bool).unwrap_or(default);
    let analysis = member(v, "analysis")?.as_str().unwrap_or_default();
    Ok(match analysis {
        "linear-model" => {
            let predictors = array(v, "predictors")?
                .iter()
                .map(|p| {
                    let name = p.as_str().ok_or_else(|| invalid("predictors must be column names"))?;
                    table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
                })
                .collect::<Result<_>>()?;
            let d = ReportOptions::default();
            SavedReport::Linear {
                spec: LinearModelSpec {
                    response: col("response")?,
                    predictors,
                    effects: linear_effects(v)?,
                    splines: parse_splines(v)?,
                },
                options: ReportOptions {
                    show_anova: flag("show_anova", d.show_anova),
                    show_confidence_limits: flag("show_confidence_limits", d.show_confidence_limits),
                    show_fit_plot: flag("show_fit_plot", d.show_fit_plot),
                    show_confidence_band: flag("show_confidence_band", d.show_confidence_band),
                    show_residual_plot: flag("show_residual_plot", d.show_residual_plot),
                    show_diagnostics: flag("show_diagnostics", d.show_diagnostics),
                    show_profiler: flag("show_profiler", d.show_profiler),
                    show_surface_profiler: flag("show_surface_profiler", d.show_surface_profiler),
                    show_surface_observed: flag("show_surface_observed", d.show_surface_observed),
                    pairwise: linear_pairwise(opts, d.pairwise)?,
                    control_level: opts.and_then(|o| o.get("control_level")).and_then(JsonValue::as_usize),
                    best: parse_best(opts.and_then(|o| o.get("best")))?,
                    show_equal_variances: flag("show_equal_variances", d.show_equal_variances),
                    show_effect_sizes: flag("show_effect_sizes", d.show_effect_sizes),
                    linear_trend: flag("linear_trend", d.linear_trend),
                    slices: parse_adjust(opts, "compare_slices")?,
                    slopes: parse_adjust(opts, "compare_slopes")?,
                },
            }
        }
        "control-chart" => {
            let kind = member(v, "chart")?
                .as_str()
                .and_then(parse_chart)
                .ok_or_else(|| invalid("control chart: unknown chart type"))?;
            let subgroup_size = member(v, "subgroup_size")?
                .as_usize()
                .ok_or_else(|| invalid("control chart: invalid subgroup size"))?;
            let d = ControlChartOptions::default();
            let optional_col = |key: &str| match v.get(key) {
                None | Some(JsonValue::Null) => Ok(None),
                Some(_) => col(key).map(Some),
            };
            let rules = |key: &str, default: RuleSet| {
                opts.and_then(|o| o.get(key)).and_then(JsonValue::as_array).map_or(default, |a| {
                    RuleSet::from_numbers(a.iter().filter_map(JsonValue::as_usize).filter_map(|n| u8::try_from(n).ok()))
                })
            };
            // Files from before rule sets were chosen per chart had one switch
            // for applying all rules to the dispersion chart.
            let old_dispersion = if flag("all_rules_on_dispersion", false) { RuleSet::ALL } else { d.dispersion_rules };
            SavedReport::ControlChart {
                request: ControlChartRequest {
                    column: col("column")?,
                    kind,
                    subgroup_size,
                    subgroup: optional_col("subgroup")?,
                    phase: optional_col("phase")?,
                    label: optional_col("label")?,
                    span: v.get("span").and_then(JsonValue::as_usize).unwrap_or(crate::control_chart::DEFAULT_SPAN),
                    part: optional_col("part")?,
                    sigma_multiple: v.get("sigma_multiple").and_then(JsonValue::as_f64).unwrap_or(3.0),
                    range_span: v.get("range_span").and_then(JsonValue::as_usize).unwrap_or(2),
                    standard: match v.get("standard") {
                        None | Some(JsonValue::Null) => None,
                        Some(sv) => {
                            let need = |key: &str| {
                                sv.get(key)
                                    .and_then(JsonValue::as_f64)
                                    .ok_or_else(|| invalid(format!("control chart: standard values need a {key}")))
                            };
                            Some(stat_quality::StandardValues { center: need("center")?, sigma: need("sigma")? })
                        }
                    },
                    last: match v.get("last") {
                        None | Some(JsonValue::Null) => None,
                        Some(l) => Some(crate::LastSubgroups {
                            count: l
                                .get("count")
                                .and_then(JsonValue::as_usize)
                                .ok_or_else(|| invalid("control chart: last needs a count"))?,
                            limits_from_last: match l.get("limits_from").and_then(JsonValue::as_str) {
                                None | Some("all") => false,
                                Some("last") => true,
                                Some(o) => return Err(invalid(format!("control chart: unknown limits_from {o:?}"))),
                            },
                        }),
                    },
                },
                options: ControlChartOptions {
                    show_dispersion_chart: flag("show_dispersion_chart", d.show_dispersion_chart),
                    show_zones: flag("show_zones", d.show_zones),
                    location_rules: rules("location_rules", d.location_rules),
                    dispersion_rules: rules("dispersion_rules", old_dispersion),
                    show_violation_table: flag("show_violation_table", d.show_violation_table),
                    within_rules: rules("within_rules", d.within_rules),
                    show_run_chart: flag("show_run_chart", d.show_run_chart),
                    run_center: match opts.and_then(|o| o.get("run_center")).and_then(JsonValue::as_str) {
                        None => d.run_center,
                        Some(k) => crate::RunCenter::from_key(k)
                            .ok_or_else(|| invalid(format!("control chart: unknown run center {k:?}")))?,
                    },
                },
            }
        }
        "gauge-study" => {
            let d = GaugeReportOptions::default();
            SavedReport::Gauge {
                request: GaugeStudyRequest {
                    part: col("part")?,
                    operator: col("operator")?,
                    measurement: col("measurement")?,
                    tolerance: v.get("tolerance").and_then(JsonValue::as_f64),
                    pool_interaction: v.get("pool_interaction").and_then(JsonValue::as_bool).unwrap_or(true),
                    study_multiplier: member(v, "study_multiplier")?
                        .as_f64()
                        .ok_or_else(|| invalid("gauge study: invalid study multiplier"))?,
                    design: crate::gauge_design::GaugeStudyDesign::from_json(v, table)?,
                },
                options: GaugeReportOptions {
                    show_anova: flag("show_anova", d.show_anova),
                    show_part_plot: flag("show_part_plot", d.show_part_plot),
                    show_operator_plot: flag("show_operator_plot", d.show_operator_plot),
                    show_run_chart: flag("show_run_chart", d.show_run_chart),
                },
            }
        }
        "distribution" => {
            let method = match v.get("quantile_type") {
                None => QuantileMethod::default(),
                Some(t) => t
                    .as_usize()
                    .and_then(|n| QUANTILE_METHODS.into_iter().find(|m| usize::from(m.type_number()) == n))
                    .ok_or_else(|| invalid("distribution: quantile type must be 1–9"))?,
            };
            let d = DistributionOptions::default();
            SavedReport::Distribution {
                request: DistributionRequest {
                    column: col("column")?,
                    quantile_method: method,
                    censoring: crate::censored_distribution::Censoring::from_json(v, table)?,
                },
                options: DistributionOptions {
                    show_quantiles: flag("show_quantiles", d.show_quantiles),
                    show_robust: flag("show_robust", d.show_robust),
                    show_chart: flag("show_chart", d.show_chart),
                    show_box_plot: flag("show_box_plot", d.show_box_plot),
                    show_normal_fit: flag("show_normal_fit", d.show_normal_fit),
                    show_quantile_plot: flag("show_quantile_plot", d.show_quantile_plot),
                    show_box_cox: flag("show_box_cox", d.show_box_cox),
                    tolerance: present(opts, "tolerance").map(tolerance_from_json).transpose()?,
                    outliers: present(opts, "outliers").map(outliers_from_json).transpose()?,
                    runs: present(opts, "runs").map(runs_from_json).transpose()?,
                },
            }
        }
        "bivariate" => SavedReport::Bivariate {
            request: BivariateRequest {
                pairwise: linear_pairwise(Some(v), Some(ComparisonMethod::TukeyKramer))?,
                ..BivariateRequest::new(col("x")?, col("y")?)
            },
        },
        "correspondence" => SavedReport::Correspondence {
            request: CorrespondenceRequest { row: col("rows")?, column: col("columns")? },
        },
        "correlations" => SavedReport::Correlations { request: crate::CorrelationRequest::from_json(v, table)? },
        "outlier-distances" => SavedReport::OutlierDistances { request: crate::OutlierRequest::from_json(v, table)? },
        "item-analysis" => SavedReport::ItemAnalysis { request: crate::ItemAnalysisRequest::from_json(v, table)? },
        "manova" => SavedReport::Manova { request: crate::ManovaRequest::from_json(v, table)? },
        "factor-analysis" => SavedReport::FactorAnalysis { request: crate::FactorRequest::from_json(v, table)? },
        "discriminant-analysis" => {
            SavedReport::DiscriminantAnalysis { request: crate::DiscriminantRequest::from_json(v, table)? }
        }
        "gaussian-mixture" => {
            SavedReport::GaussianMixture { request: crate::GaussianMixtureRequest::from_json(v, table)? }
        }
        "latent-class" => SavedReport::LatentClass { request: crate::LatentClassRequest::from_json(v, table)? },
        "text-exploration" => {
            SavedReport::TextExploration { request: crate::TextExplorationRequest::from_json(v, table)? }
        }
        "topic-model" => SavedReport::TopicModel { request: crate::TopicModelRequest::from_json(v, table)? },
        "sentiment" => SavedReport::Sentiment { request: crate::SentimentRequest::from_json(v, table)? },
        "embedding" => SavedReport::Embedding { request: crate::EmbeddingRequest::from_json(v, table)? },
        "imputation" => SavedReport::Imputation { request: crate::ImputationRequest::from_json(v, table)? },
        "series-description" | "decomposition" | "trend-models" | "cross-correlation" => {
            SavedReport::SeriesExplore { request: crate::SeriesExploreRequest::from_json(analysis, v, table)? }
        }
        "fourier-spectrum" | "digital-filter" | "signal-correlation" | "wavelet-analysis" => {
            SavedReport::SignalProcessing { request: crate::SignalProcessingRequest::from_json(analysis, v, table)? }
        }
        "image-spectrum" | "image-filter" | "image-restoration" => {
            SavedReport::ImageAnalysis { request: crate::ImageAnalysisRequest::from_json(analysis, v, table)? }
        }
        "survival" => {
            let optional_col = |key: &str| match v.get(key) {
                None | Some(JsonValue::Null) => Ok(None),
                Some(_) => col(key).map(Some),
            };
            let coding = match v.get("status_coding").and_then(JsonValue::as_str) {
                None | Some("nonzero-censored") => StatusCoding::NonzeroIsCensored,
                Some("nonzero-event") => StatusCoding::NonzeroIsEvent,
                Some(other) => return Err(invalid(format!("survival: unknown status coding {other:?}"))),
            };
            let d = SurvivalOptions::default();
            let band = match opts.and_then(|o| o.get("band")).and_then(JsonValue::as_str) {
                None => d.band,
                Some(name) => [BandTransform::Plain, BandTransform::Log, BandTransform::LogLog]
                    .into_iter()
                    .find(|&t| band_name(t) == name)
                    .ok_or_else(|| invalid(format!("survival: unknown band type {name:?}")))?,
            };
            SavedReport::Survival {
                request: SurvivalRequest {
                    time: col("time")?,
                    status: optional_col("status")?,
                    coding,
                    group: optional_col("group")?,
                },
                options: SurvivalOptions {
                    show_band: flag("show_band", d.show_band),
                    band,
                    show_weibull: flag("show_weibull", d.show_weibull),
                    show_lognormal: flag("show_lognormal", d.show_lognormal),
                    show_table: flag("show_table", d.show_table),
                    trend: flag("trend", d.trend),
                    risk_table: flag("risk_table", d.risk_table),
                    risk_censored: flag("risk_censored", d.risk_censored),
                },
            }
        }
        "principal-components" => SavedReport::Pca {
            request: PcaRequest {
                columns: col_list("columns")?,
                standardize: v.get("standardize").and_then(JsonValue::as_bool).unwrap_or(true),
            },
        },
        "clustering" => {
            let k = member(v, "k")?.as_usize().ok_or_else(|| invalid("clustering: invalid k"))?;
            let method = match member(v, "method")?.as_str() {
                Some("k-means") => {
                    ClusterMethod::KMeans { k, seed: v.get("seed").and_then(crate::seed::seed_from_json).unwrap_or(1) }
                }
                Some("hierarchical") => {
                    let name = v.get("linkage").and_then(JsonValue::as_str).unwrap_or("ward");
                    let linkage = [Linkage::Single, Linkage::Complete, Linkage::Average, Linkage::Ward]
                        .into_iter()
                        .find(|&l| linkage_name(l) == name)
                        .ok_or_else(|| invalid(format!("clustering: unknown linkage {name:?}")))?;
                    ClusterMethod::Hierarchical { linkage, k }
                }
                _ => return Err(invalid("clustering: method must be \"k-means\" or \"hierarchical\"")),
            };
            SavedReport::Cluster {
                request: ClusterRequest {
                    distance: match v.get("distance").and_then(JsonValue::as_str) {
                        None | Some("euclidean") => crate::ClusterDistance::Euclidean,
                        Some("matrix") => crate::ClusterDistance::Matrix,
                        Some(k) => crate::ClusterDistance::Metric(
                            stat_multivariate::distance::DistanceMetric::from_key(k)
                                .ok_or_else(|| invalid(format!("clustering: unknown distance {k:?}")))?,
                        ),
                    },
                    columns: col_list("columns")?,
                    method,
                    standardize: v.get("standardize").and_then(JsonValue::as_bool).unwrap_or(true),
                    centers: match v.get("centers") {
                        None | Some(JsonValue::Null) => None,
                        Some(JsonValue::Array(rows)) => {
                            let centers = rows
                                .iter()
                                .map(|r| {
                                    r.as_array()
                                        .map(|a| a.iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect::<Vec<f64>>())
                                        .ok_or_else(|| invalid("clustering: each center must be a list of numbers"))
                                })
                                .collect::<Result<Vec<_>>>()?;
                            if centers.len() != k {
                                return Err(invalid(format!(
                                    "clustering: {} centers given for k = {k}",
                                    centers.len()
                                )));
                            }
                            Some(crate::StartingCenters {
                                centers,
                                also_seeded: v.get("centers_also_seeded").and_then(JsonValue::as_bool).unwrap_or(false),
                            })
                        }
                        Some(_) => return Err(invalid("clustering: centers must be a list of lists of numbers")),
                    },
                    choose: match v.get("choose_k") {
                        None | Some(JsonValue::Null) => None,
                        Some(c) => {
                            let d = crate::ChooseClusters::default();
                            let reference = match c.get("reference").and_then(JsonValue::as_str) {
                                None | Some("data-range") => crate::GapReference::DataRange,
                                Some("principal-components") => crate::GapReference::PrincipalComponents,
                                Some(other) => {
                                    return Err(invalid(format!("clustering: unknown gap reference {other:?}")));
                                }
                            };
                            let max_k = c.get("max_k").and_then(JsonValue::as_usize).unwrap_or(d.max_k);
                            let references = c.get("references").and_then(JsonValue::as_usize).unwrap_or(d.references);
                            if max_k < 2 || references == 0 {
                                return Err(invalid("clustering: choose_k needs max_k of at least 2 and references"));
                            }
                            Some(crate::ChooseClusters { max_k, references, reference })
                        }
                    },
                },
            }
        }
        "generalized-linear-model" => {
            let name = member(v, "family")?.as_str().unwrap_or_default();
            let family = Family::ALL
                .into_iter()
                .find(|&f| family_name(f) == name)
                .ok_or_else(|| invalid(format!("unknown family {name:?}")))?;
            let link = match v.get("link") {
                None | Some(JsonValue::Null) => None,
                Some(l) => {
                    let key = l.as_str().unwrap_or_default();
                    Some(stat_discrete::Link::from_key(key).ok_or_else(|| invalid(format!("unknown link {key:?}")))?)
                }
            };
            SavedReport::Glm {
                request: GlmRequest {
                    response: col("response")?,
                    predictors: col_list("predictors")?,
                    family,
                    link,
                    event: v.get("event").and_then(JsonValue::as_str).map(str::to_owned),
                    effects: linear_effects(v)?,
                    splines: parse_splines(v)?,
                    slices: parse_adjust(Some(v), "compare_slices")?,
                    slopes: parse_adjust(Some(v), "compare_slopes")?,
                },
                profilers: read_profilers(v),
            }
        }
        "graph" => SavedReport::Graph { spec: Box::new(crate::graph_json::graph_from_json(v)?) },
        "scatter-3d" => {
            let (request, options) = crate::scatter3d::scatter3d_from_json(v, table)?;
            SavedReport::Scatter3d { request, options }
        }
        "data-surface" => {
            let (request, options) = crate::data_surface::data_surface_from_json(v, table)?;
            SavedReport::DataSurface { request, options }
        }
        "mixture-model" => {
            let model = match v.get("model").and_then(JsonValue::as_str) {
                Some("linear") => ScheffeModel::Linear,
                None | Some("quadratic") => ScheffeModel::Quadratic,
                Some("special-cubic") => ScheffeModel::SpecialCubic,
                Some(other) => return Err(invalid(format!("mixture model: unknown model {other:?}"))),
            };
            SavedReport::Mixture {
                request: MixtureRequest { response: col("response")?, components: col_list("components")?, model },
                profilers: ProfilerOptions { show_surface_profiler: false, ..read_profilers(v) },
            }
        }
        "stepwise" => {
            // Searches saved before heredity existed were unrestricted.
            let (direction, criterion, heredity) =
                crate::stepwise_analysis::search_from_json(v, stat_linear::Heredity::None)?;
            SavedReport::Stepwise {
                request: StepwiseRequest {
                    candidates: LinearModelSpec {
                        response: col("response")?,
                        predictors: col_list("predictors")?,
                        effects: linear_effects(v)?,
                        splines: Vec::new(),
                    },
                    direction,
                    criterion,
                    heredity,
                },
                options: read_profilers(v),
            }
        }
        "best-subsets" => {
            let criterion = match v.get("criterion").and_then(JsonValue::as_str) {
                None => SubsetCriterion::Bic,
                Some(name) => crate::best_subsets_analysis::SUBSET_CRITERIA
                    .into_iter()
                    .find(|&c| subset_criterion_name(c) == name)
                    .ok_or_else(|| invalid(format!("best subsets: unknown criterion {name:?}")))?,
            };
            let forced = match v.get("forced") {
                None => Vec::new(),
                Some(f) => linear_effects(&obj([("effects", f.clone())]))?,
            };
            SavedReport::BestSubsets {
                request: BestSubsetsRequest {
                    candidates: LinearModelSpec {
                        response: col("response")?,
                        predictors: col_list("predictors")?,
                        effects: linear_effects(v)?,
                        splines: Vec::new(),
                    },
                    forced,
                    heredity: match v.get("heredity").and_then(JsonValue::as_str) {
                        None => stat_linear::Heredity::Strong,
                        Some(k) => stat_linear::Heredity::from_key(k)
                            .ok_or_else(|| invalid(format!("best subsets: unknown heredity {k:?}")))?,
                    },
                    models_per_size: v.get("models_per_size").and_then(JsonValue::as_usize).unwrap_or(3),
                    max_size: v.get("max_size").and_then(JsonValue::as_usize),
                    criterion,
                },
            }
        }
        "distribution-identification" => {
            let families = match v.get("distributions") {
                None | Some(JsonValue::Null) => FitFamily::ALL.to_vec(),
                Some(_) => array(v, "distributions")?
                    .iter()
                    .map(|f| {
                        let key = f.as_str().unwrap_or_default();
                        FitFamily::from_key(key).ok_or_else(|| {
                            invalid(format!("distribution identification: unknown distribution {key:?}"))
                        })
                    })
                    .collect::<Result<_>>()?,
            };
            SavedReport::DistributionFit {
                request: DistributionFitRequest {
                    column: col("column")?,
                    families,
                    transformations: v.get("transformations").and_then(JsonValue::as_bool).unwrap_or(true),
                    plots: v.get("plots").and_then(JsonValue::as_usize).unwrap_or(4),
                },
            }
        }
        "attribute-chart" => {
            let name = v.get("chart").and_then(JsonValue::as_str).unwrap_or_default();
            let kind = AttributeChartKind::from_key(name)
                .ok_or_else(|| invalid(format!("attribute chart: unknown chart {name:?}")))?;
            let size = match v.get("size") {
                None | Some(JsonValue::Null) => SubgroupSize::None,
                Some(JsonValue::String(_)) => SubgroupSize::Column(col("size")?),
                Some(other) => SubgroupSize::Constant(
                    other.as_f64().ok_or_else(|| invalid("attribute chart: size must be a column or a number"))?,
                ),
            };
            let phase = match v.get("phase") {
                None | Some(JsonValue::Null) => None,
                Some(_) => Some(col("phase")?),
            };
            let rules = v.get("rules").and_then(JsonValue::as_array).map_or(RuleSet::LIMITS_ONLY, |a| {
                RuleSet::from_numbers(a.iter().filter_map(JsonValue::as_usize).filter_map(|n| u8::try_from(n).ok()))
            });
            SavedReport::Attribute { request: AttributeChartRequest { kind, count: col("count")?, size, phase, rules } }
        }
        "pareto-chart" => {
            let opt_col = |key: &str| match v.get(key) {
                None | Some(JsonValue::Null) => Ok(None),
                Some(_) => col(key).map(Some),
            };
            let d = crate::ParetoChartOptions::default();
            let other_percent =
                opts.and_then(|o| o.get("other_percent")).and_then(JsonValue::as_f64).unwrap_or(d.other_percent);
            if !(0.0..100.0).contains(&other_percent) {
                return Err(invalid("Pareto chart: the \"Other\" threshold must be from 0 to below 100 percent"));
            }
            SavedReport::Pareto {
                request: crate::ParetoRequest {
                    category: col("category")?,
                    frequency: opt_col("frequency")?,
                    group: opt_col("group")?,
                },
                options: crate::ParetoChartOptions {
                    other_percent,
                    percent_scale: flag("percent_scale", d.percent_scale),
                },
            }
        }
        "cause-and-effect" => SavedReport::CauseEffect {
            request: crate::CauseEffectRequest {
                causes: col_list("causes")?,
                effect: v.get("effect").and_then(JsonValue::as_str).unwrap_or_default().to_owned(),
            },
        },
        "multi-vari" => {
            let d = crate::MultiVariOptions::default();
            SavedReport::MultiVari {
                request: crate::MultiVariRequest { response: col("response")?, factors: col_list("factors")? },
                options: crate::MultiVariOptions {
                    show_values: flag("show_values", d.show_values),
                    show_ranges: flag("show_ranges", d.show_ranges),
                    show_cells: flag("show_cells", d.show_cells),
                    replicates: opts
                        .and_then(|o| o.get("replicates"))
                        .and_then(JsonValue::as_str)
                        .and_then(crate::ReplicateLayout::from_key)
                        .unwrap_or(d.replicates),
                },
            }
        }
        "multivariate-chart" => {
            let name = v.get("chart").and_then(JsonValue::as_str).unwrap_or_default();
            let kind = crate::MultivariateChartKind::from_key(name)
                .ok_or_else(|| invalid(format!("multivariate chart: unknown chart {name:?}")))?;
            let num = |key: &str| v.get(key).and_then(JsonValue::as_f64);
            let optional_col = |key: &str| -> Result<Option<usize>> {
                match v.get(key) {
                    None | Some(JsonValue::Null) => Ok(None),
                    Some(_) => col(key).map(Some),
                }
            };
            let d = crate::MultivariateChartRequest::new(col_list("columns")?, kind);
            let mewma_limit = match (num("mewma_limit"), num("mewma_arl")) {
                (Some(h), _) => crate::MewmaLimit::Value(h),
                (None, Some(arl)) => crate::MewmaLimit::InControlArl(arl),
                (None, None) => d.mewma_limit,
            };
            SavedReport::MultivariateChart {
                request: crate::MultivariateChartRequest {
                    subgroup: optional_col("subgroup")?,
                    subgroup_size: v.get("subgroup_size").and_then(JsonValue::as_usize).unwrap_or(d.subgroup_size),
                    label: optional_col("label")?,
                    reference: v.get("reference").and_then(JsonValue::as_usize),
                    alpha: num("alpha").unwrap_or(d.alpha),
                    lambda: num("lambda").unwrap_or(d.lambda),
                    mewma_limit,
                    ..d
                },
            }
        }
        "reliability-growth"
        | "repairable-systems"
        | "reliability-test-plan"
        | "life-distributions"
        | "competing-risks"
        | "warranty-returns"
        | "life-stress"
        | "degradation"
        | "proportional-hazards"
        | "survival-regression" => {
            crate::reliability_json::from_json(analysis, v, table).unwrap_or_else(|| Err(invalid("unknown report")))?
        }
        "exponential-smoothing" => {
            SavedReport::ExponentialSmoothing { request: crate::SmoothingRequest::from_json(v, table)? }
        }
        "arima" => SavedReport::Arima { request: crate::ArimaRequest::from_json(v, table)? },
        "principal-coordinates" => {
            SavedReport::PrincipalCoordinates { request: crate::PcoaRequest::from_json(v, table)? }
        }
        "pharmacokinetics" => SavedReport::Pharmacokinetics { request: crate::PkRequest::from_json(v, table)? },
        "fatigue" => SavedReport::Fatigue { request: crate::FatigueRequest::from_json(v, table)? },
        "analysis-of-means" => {
            let name = v.get("data").and_then(JsonValue::as_str).unwrap_or("means");
            let kind = crate::AnomKind::from_key(name)
                .ok_or_else(|| invalid(format!("analysis of means: unknown data kind {name:?}")))?;
            let optional_col = |key: &str| -> Result<Option<usize>> {
                match v.get(key) {
                    None | Some(JsonValue::Null) => Ok(None),
                    Some(_) => col(key).map(Some),
                }
            };
            let size = match v.get("size") {
                None | Some(JsonValue::Null) => SubgroupSize::None,
                Some(JsonValue::String(_)) => SubgroupSize::Column(col("size")?),
                Some(other) => SubgroupSize::Constant(
                    other.as_f64().ok_or_else(|| invalid("analysis of means: size must be a column or a number"))?,
                ),
            };
            SavedReport::Anom {
                request: crate::AnomRequest {
                    kind,
                    response: col("response")?,
                    size,
                    group: optional_col("group")?,
                    second: optional_col("second_group")?,
                    alpha: v.get("alpha").and_then(JsonValue::as_f64).unwrap_or(crate::AnomRequest::DEFAULT_ALPHA),
                },
            }
        }
        "ordinal-logistic" => crate::categorical_logistic::from_json(crate::CategoricalModel::Ordinal, v, table)?,
        "nominal-logistic" => crate::categorical_logistic::from_json(crate::CategoricalModel::Nominal, v, table)?,
        "nonlinear-regression" => crate::nonlinear_analysis::from_json(v, table)?,
        "dose-response" => {
            let optional_col = |key: &str| -> Result<Option<usize>> {
                match v.get(key) {
                    None | Some(JsonValue::Null) => Ok(None),
                    Some(_) => col(key).map(Some),
                }
            };
            let d = crate::DoseResponseRequest::probit(col("dose")?, col("response")?, optional_col("trials")?);
            let link = match v.get("link").and_then(JsonValue::as_str) {
                None => d.link,
                Some(key) => crate::Link::from_key(key)
                    .filter(|l| crate::DoseResponseRequest::LINKS.contains(l))
                    .ok_or_else(|| invalid(format!("dose-response: unknown link {key:?}")))?,
            };
            let scale = match v.get("scale").and_then(JsonValue::as_str) {
                None => d.scale,
                Some(key) => crate::DoseScale::from_key(key)
                    .ok_or_else(|| invalid(format!("dose-response: unknown scale {key:?}")))?,
            };
            let probabilities = match v.get("probabilities") {
                None | Some(JsonValue::Null) => d.probabilities.clone(),
                Some(JsonValue::Array(items)) => items
                    .iter()
                    .map(|p| p.as_f64().ok_or_else(|| invalid("dose-response: probabilities must be numbers")))
                    .collect::<Result<_>>()?,
                Some(_) => return Err(invalid("dose-response: probabilities must be a list")),
            };
            let num = |key: &str| v.get(key).and_then(JsonValue::as_f64);
            SavedReport::DoseResponse {
                request: crate::DoseResponseRequest {
                    link,
                    scale,
                    probabilities,
                    confidence_level: num("level").unwrap_or(d.confidence_level),
                    heterogeneity_alpha: num("heterogeneity_alpha").unwrap_or(d.heterogeneity_alpha),
                    ..d
                },
            }
        }
        "method-comparison" => {
            let d = crate::MethodComparisonRequest::new(col("reference")?, col("test")?);
            let flag = |key: &str, default: bool| v.get(key).and_then(JsonValue::as_bool).unwrap_or(default);
            let num = |key: &str| v.get(key).and_then(JsonValue::as_f64);
            SavedReport::MethodComparison {
                request: crate::MethodComparisonRequest {
                    deming: flag("deming", d.deming),
                    passing_bablok: flag("passing_bablok", d.passing_bablok),
                    variance_ratio: num("variance_ratio").unwrap_or(d.variance_ratio),
                    confidence_level: num("level").unwrap_or(d.confidence_level),
                    ..d
                },
            }
        }
        "shelf-life" => {
            let optional_col = |key: &str| -> Result<Option<usize>> {
                match v.get(key) {
                    None | Some(JsonValue::Null) => Ok(None),
                    Some(_) => col(key).map(Some),
                }
            };
            let num = |key: &str| v.get(key).and_then(JsonValue::as_f64);
            let d = crate::ShelfLifeRequest::lower(col("time")?, col("response")?, optional_col("batch")?, 0.0);
            SavedReport::ShelfLife {
                request: crate::ShelfLifeRequest {
                    lower: num("lower"),
                    upper: num("upper"),
                    pooling_alpha: num("pooling_alpha").unwrap_or(d.pooling_alpha),
                    confidence_level: num("level").unwrap_or(d.confidence_level),
                    horizon: num("horizon"),
                    // Absent in projects saved before the unit was asked for:
                    // they keep the twice-the-data limit only.
                    time_unit: match v.get("time_unit").and_then(JsonValue::as_str) {
                        None | Some("none") => None,
                        Some(name) => Some(stat_linear::StabilityTimeUnit::from_label(name).ok_or_else(|| {
                            invalid(format!(
                                "shelf life: unknown time unit \"{name}\" (days, weeks, months, years or none)"
                            ))
                        })?),
                    },
                    random_batches: match v.get("batches").and_then(JsonValue::as_str) {
                        None | Some("fixed") => false,
                        Some("random") => true,
                        Some(other) => {
                            return Err(invalid(format!("shelf life: unknown batches {other:?} (fixed or random)")));
                        }
                    },
                    future_batch: match v.get("bound").and_then(JsonValue::as_str) {
                        None | Some("future-batch") => true,
                        Some("mean") => false,
                        Some(other) => {
                            return Err(invalid(format!("shelf life: unknown bound {other:?} (future-batch or mean)")));
                        }
                    },
                    ..d
                },
            }
        }
        "process-capability" => {
            let num = |key: &str| v.get(key).and_then(JsonValue::as_f64);
            let size = || {
                v.get("subgroup_size")
                    .and_then(JsonValue::as_usize)
                    .ok_or_else(|| invalid("capability: missing subgroup size"))
            };
            let within = match v.get("within").and_then(JsonValue::as_str) {
                None | Some("moving-range") => WithinSigma::MovingRange,
                Some("average-range") => WithinSigma::AverageRange { subgroup_size: size()? },
                Some("pooled-sd") => WithinSigma::PooledStdDev { subgroup_size: size()? },
                Some(other) => return Err(invalid(format!("capability: unknown σ method {other:?}"))),
            };
            let summary = match v.get("layout").and_then(JsonValue::as_str) {
                None | Some("standard") => false,
                Some("summary") => true,
                Some(other) => return Err(invalid(format!("capability: unknown layout {other:?}"))),
            };
            let bootstrap = match v.get("bootstrap") {
                None | Some(JsonValue::Null) | Some(JsonValue::Bool(false)) => None,
                Some(b) => {
                    let d = crate::capability_analysis::BootstrapSettings::default();
                    Some(crate::capability_analysis::BootstrapSettings {
                        resamples: b.get("resamples").and_then(JsonValue::as_usize).unwrap_or(d.resamples),
                        seed: b.get("seed").and_then(crate::seed::seed_from_json).unwrap_or(d.seed),
                    })
                }
            };
            SavedReport::Capability {
                request: CapabilityRequest {
                    summary,
                    bootstrap,
                    column: col("column")?,
                    limits: SpecLimits { lower: num("lower"), upper: num("upper"), target: num("target") },
                    within,
                    box_cox: num("box_cox"),
                    model: match v.get("distribution").and_then(JsonValue::as_str) {
                        None => CapabilityModel::Normal,
                        Some(k) => CapabilityModel::from_key(k)
                            .ok_or_else(|| invalid(format!("capability: unknown distribution {k:?}")))?,
                    },
                    by: match v.get("by") {
                        None | Some(JsonValue::Null) => None,
                        Some(_) => Some(col("by")?),
                    },
                    group_order: match v.get("group_order").and_then(JsonValue::as_str) {
                        None => crate::GroupOrder::Labels,
                        Some(k) => crate::GroupOrder::from_key(k)
                            .ok_or_else(|| invalid(format!("capability: unknown group order {k:?}")))?,
                    },
                    probability_plot: v.get("probability_plot").and_then(JsonValue::as_bool).unwrap_or(false),
                },
            }
        }
        "ewma-chart" | "cusum-chart" => {
            let num = |key: &str| v.get(key).and_then(JsonValue::as_f64);
            let need = |key: &str| num(key).ok_or_else(|| invalid(format!("{analysis}: missing {key}")));
            let kind = if analysis == "ewma-chart" {
                TimeWeightedKind::Ewma { lambda: need("lambda")?, width: need("width")? }
            } else {
                TimeWeightedKind::Cusum { k: need("k")?, h: need("h")? }
            };
            let target = match (num("target_mean"), num("target_sigma")) {
                (Some(m), Some(sd)) => Some((m, sd)),
                _ => None,
            };
            SavedReport::TimeWeighted { request: TimeWeightedRequest { column: col("column")?, kind, target } }
        }
        "response-surface" => {
            let d = SurfaceOptions::default();
            let index = |key: &str, default: usize| {
                opts.and_then(|o| o.get(key)).and_then(JsonValue::as_usize).unwrap_or(default)
            };
            let factors = col_list("factors")?;
            let k = factors.len();
            SavedReport::Surface {
                request: SurfaceRequest { response: col("response")?, factors },
                options: SurfaceOptions {
                    show_coefficients: flag("show_coefficients", d.show_coefficients),
                    show_ridge: flag("show_ridge", d.show_ridge),
                    contour_x: index("contour_x", d.contour_x),
                    contour_y: index("contour_y", d.contour_y),
                    show_profiler: flag("show_profiler", d.show_profiler),
                    show_surface_profiler: flag("show_surface_profiler", d.show_surface_profiler),
                    show_surface_observed: flag("show_surface_observed", d.show_surface_observed),
                }
                .for_factors(k),
            }
        }
        "mixed-model" => {
            let name = member(v, "family")?.as_str().unwrap_or_default();
            let family =
                crate::MixedFamily::from_key(name).ok_or_else(|| invalid(format!("unknown family {name:?}")))?;
            let column = |j: &JsonValue| -> Result<usize> {
                let name = j.as_str().ok_or_else(|| invalid("mixed model: expected a column name"))?;
                table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
            };
            let columns = |j: Option<&JsonValue>| -> Result<Vec<usize>> {
                j.and_then(JsonValue::as_array).unwrap_or_default().iter().map(&column).collect()
            };
            let optional = |key: &str| -> Result<Option<usize>> {
                match v.get(key) {
                    None | Some(JsonValue::Null) => Ok(None),
                    Some(j) => column(j).map(Some),
                }
            };
            let random = array(v, "random")?
                .iter()
                .map(|r| {
                    Ok(crate::RandomEffectSpec {
                        group: column(member(r, "group")?)?,
                        within: columns(r.get("within"))?,
                        intercept: r.get("intercept").and_then(JsonValue::as_bool).unwrap_or(true),
                        slopes: columns(r.get("slopes"))?,
                        correlated: r.get("correlated").and_then(JsonValue::as_bool).unwrap_or(true),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            SavedReport::Mixed {
                request: crate::MixedRequest {
                    response: col("response")?,
                    predictors: col_list("predictors")?,
                    effects: linear_effects(v)?,
                    random,
                    family,
                    trials: optional("trials")?,
                    offset: optional("offset")?,
                    reml: v.get("reml").and_then(JsonValue::as_bool).unwrap_or(true),
                    quadrature_points: v
                        .get("quadrature_points")
                        .and_then(JsonValue::as_usize)
                        .filter(|q| (1..=25).contains(q))
                        .unwrap_or(1),
                    slices: parse_adjust(Some(v), "compare_slices")?,
                    slopes: parse_adjust(Some(v), "compare_slopes")?,
                    frequency: optional("frequency")?,
                    weight: optional("weight")?,
                    robust: v.get("robust").and_then(JsonValue::as_bool).unwrap_or(false),
                    residual_groups: optional("residual_groups")?,
                    df_method: match v.get("df_method").and_then(JsonValue::as_str) {
                        None | Some("satterthwaite") => stat_mixed::DfMethod::Satterthwaite,
                        Some("between-within") => stat_mixed::DfMethod::BetweenWithin,
                        Some(other) => {
                            return Err(invalid(format!(
                                "mixed model: unknown df_method {other:?} (satterthwaite or between-within)"
                            )));
                        }
                    },
                    order: v
                        .get("order")
                        .and_then(JsonValue::as_array)
                        .unwrap_or_default()
                        .iter()
                        .filter_map(|o| o.as_str().map(str::to_owned).or_else(|| o.as_f64().map(|x| x.to_string())))
                        .collect(),
                },
                profilers: read_profilers(v),
            }
        }
        "several-responses" => SavedReport::MultipleResponses {
            request: Box::new(crate::multiple_responses::request_from_json(
                v,
                col_list("responses")?,
                col_list("predictors")?,
            )?),
        },
        "penalized-regression" => SavedReport::Regularized {
            request: RegularizedRequest {
                response: col("response")?,
                predictors: col_list("predictors")?,
                alpha: member(v, "alpha")?
                    .as_f64()
                    .filter(|a| (0.0..=1.0).contains(a))
                    .ok_or_else(|| invalid("penalized regression: α must be in [0, 1]"))?,
                folds: v.get("folds").and_then(JsonValue::as_usize).unwrap_or(10),
                seed: v.get("seed").and_then(crate::seed::seed_from_json).unwrap_or(1),
            },
            profilers: read_profilers(v),
        },
        "decision-tree" => {
            let find = |c: &JsonValue| -> Result<usize> {
                let name = c.as_str().ok_or_else(|| invalid("decision tree: expected a column name"))?;
                table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
            };
            SavedReport::DecisionTree {
                request: crate::tree_analysis::request_from_json(v, col("response")?, col_list("predictors")?, &find)?,
                profilers: read_profilers(v),
            }
        }
        "random-forest" => {
            let find = |c: &JsonValue| -> Result<usize> {
                let name = c.as_str().ok_or_else(|| invalid("random forest: expected a column name"))?;
                table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
            };
            SavedReport::RandomForest {
                request: crate::forest_analysis::request_from_json(
                    v,
                    col("response")?,
                    col_list("predictors")?,
                    &find,
                )?,
                profilers: read_profilers(v),
            }
        }
        "boosted-trees" => {
            let find = |c: &JsonValue| -> Result<usize> {
                let name = c.as_str().ok_or_else(|| invalid("boosted trees: expected a column name"))?;
                table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
            };
            SavedReport::BoostedTrees {
                request: crate::boost_analysis::request_from_json(v, col("response")?, col_list("predictors")?, &find)?,
                profilers: read_profilers(v),
            }
        }
        "partial-least-squares" => {
            let find = |c: &JsonValue| -> Result<usize> {
                let name = c.as_str().ok_or_else(|| invalid("partial least squares: expected a column name"))?;
                table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
            };
            SavedReport::PartialLeastSquares {
                request: crate::pls_analysis::request_from_json(v, col("response")?, col_list("predictors")?, &find)?,
                profilers: read_profilers(v),
            }
        }
        "support-vector-machine" => {
            let find = |c: &JsonValue| -> Result<usize> {
                let name = c.as_str().ok_or_else(|| invalid("support vector machine: expected a column name"))?;
                table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
            };
            SavedReport::SupportVectorMachine {
                request: crate::svm_analysis::request_from_json(v, col("response")?, col_list("predictors")?, &find)?,
                profilers: read_profilers(v),
            }
        }
        "functional-data" => {
            let (request, profilers) = crate::FunctionalRequest::from_json(v, table)?;
            SavedReport::FunctionalData { request, profilers }
        }
        // Older scripts and project files call it "mars".
        "adaptive-splines" | "mars" => {
            let find = |c: &JsonValue| -> Result<usize> {
                let name = c.as_str().ok_or_else(|| invalid("adaptive splines: expected a column name"))?;
                table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
            };
            SavedReport::Mars {
                request: crate::mars_analysis::request_from_json(v, col("response")?, col_list("predictors")?, &find)?,
                profilers: read_profilers(v),
            }
        }
        "neural-network" => {
            let find = |c: &JsonValue| -> Result<usize> {
                let name = c.as_str().ok_or_else(|| invalid("neural network: expected a column name"))?;
                table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
            };
            SavedReport::NeuralNetwork {
                request: crate::neural_analysis::request_from_json(
                    v,
                    col("response")?,
                    col_list("predictors")?,
                    &find,
                )?,
                profilers: read_profilers(v),
            }
        }
        "gaussian-process" => {
            let find = |c: &JsonValue| -> Result<usize> {
                let name = c.as_str().ok_or_else(|| invalid("Gaussian process: expected a column name"))?;
                table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
            };
            SavedReport::GaussianProcess {
                request: crate::gaussian_process_analysis::request_from_json(
                    v,
                    col("response")?,
                    col_list("predictors")?,
                    &find,
                )?,
                profilers: read_profilers(v),
            }
        }
        "sequential-optimization" => {
            let find = |c: &JsonValue| -> Result<usize> {
                let name = c.as_str().ok_or_else(|| invalid("sequential optimization: expected a column name"))?;
                table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
            };
            SavedReport::SequentialOptimization {
                request: Box::new(crate::sequential_optimization::request_from_json(
                    v,
                    col_list("responses")?,
                    col_list("factors")?,
                    &find,
                )?),
            }
        }
        "model-search" => {
            let find = |c: &JsonValue| -> Result<usize> {
                let name = c.as_str().ok_or_else(|| invalid("model search: expected a column name"))?;
                table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
            };
            SavedReport::ModelSearch {
                request: crate::model_search::request_from_json(v, col("response")?, col_list("predictors")?, &find)?,
            }
        }
        "model-comparison" => {
            let find = |c: &JsonValue| -> Result<usize> {
                let name = c.as_str().ok_or_else(|| invalid("model comparison: expected a column name"))?;
                table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
            };
            SavedReport::ModelComparison {
                request: crate::model_comparison::request_from_json(v, col("response")?, &find)?,
            }
        }
        "one-sample" | "compare-groups" | "paired" | "proportions" | "rates" | "power" => {
            comparison_from_json(analysis, v, table)?
        }
        "attribute-capability" => {
            SavedReport::AttributeCapability { request: crate::AttributeCapabilityRequest::from_json(v, table)? }
        }
        "between-within-capability" => {
            SavedReport::BetweenWithinCapability { request: crate::BetweenWithinRequest::from_json(v, table)? }
        }
        "acceptance-sampling" => SavedReport::AcceptanceSampling { request: crate::AcceptanceRequest::from_json(v)? },
        "gauge-linearity" => SavedReport::GaugeLinearity { request: crate::LinearityRequest::from_json(v, table)? },
        "gauge-repeatability" => {
            SavedReport::GaugeRepeatability { request: crate::RepeatabilityRequest::from_json(v, table)? }
        }
        "intraclass-correlation" => {
            SavedReport::IntraclassCorrelation { request: crate::IccRequest::from_json(v, table)? }
        }
        "attribute-agreement" => {
            SavedReport::AttributeAgreement { request: crate::AgreementRequest::from_json(v, table)? }
        }
        "attribute-gauge" => {
            SavedReport::AttributeGauge { request: crate::AttributeGaugeRequest::from_json(v, table)? }
        }
        "pca-chart" => SavedReport::PcaChart { request: crate::PcaChartRequest::from_json(v, table)? },
        "monitoring-limits" => {
            SavedReport::MonitoringLimits { request: crate::MonitoringRequest::from_json(v, table)? }
        }
        "multiway-table" => SavedReport::MultiwayTable { request: crate::MultiwayTableRequest::from_json(v, table)? },
        "stratified-tables" => {
            SavedReport::StratifiedTables { request: crate::StratifiedRequest::from_json(v, table)? }
        }
        "bootstrap" => SavedReport::Bootstrap { request: crate::BootstrapRequest::from_json(v, table)? },
        "permutation-test" => SavedReport::PermutationTest { request: crate::PermutationRequest::from_json(v, table)? },
        "signal-to-noise" => SavedReport::SignalToNoise { request: crate::SignalToNoiseRequest::from_json(v, table)? },
        "declared-design" => {
            SavedReport::DeclaredDesign { request: crate::DesignDeclarationRequest::from_json(v, table)? }
        }
        "design-evaluation" => {
            SavedReport::DesignEvaluation { request: crate::DesignEvaluationRequest::from_json(v, table)? }
        }
        "factorial-analysis" => {
            SavedReport::FactorialAnalysis { request: crate::FactorialAnalysisRequest::from_json(v, table)? }
        }
        "goodness-of-fit" => SavedReport::GoodnessOfFit { request: crate::GoodnessRequest::from_json(v, table)? },
        "crossover" => SavedReport::Crossover { request: crate::CrossoverRequest::from_json(v, table)? },
        "adjust-p-values" => SavedReport::AdjustPValues { request: crate::PValueRequest::from_json(v, table)? },
        "area-under-curve" => SavedReport::AreaUnderCurve { request: crate::AreaRequest::from_json(v, table)? },
        "many-tests" => SavedReport::ManyTests { request: crate::ManyTestsRequest::from_json(v, table)? },
        "treatment-effects" => {
            SavedReport::TreatmentEffects { request: crate::TreatmentEffectsRequest::from_json(v, table)? }
        }
        "screening" => SavedReport::Screening { request: crate::ScreeningRequest::from_json(v, table)? },
        "factorial-repeated-measures" => {
            SavedReport::FactorialRepeated { request: crate::FactorialRepeatedRequest::from_json(v, table)? }
        }
        "nested-comparison" => SavedReport::Nested { request: crate::NestedRequest::from_json(v, table)? },
        key @ ("bayesian-estimation" | "bayes-factor" | "bayesian-linear-model" | "bayesian-model") => {
            SavedReport::Bayesian { request: crate::BayesRequest::from_json(key, v, table)? }
        }
        "multiple-response" => {
            SavedReport::MultipleResponseItems { request: crate::MultipleResponseRequest::from_json(v, table)? }
        }
        key @ ("signal-smoothing" | "curve-calculus" | "spectrum-peaks") => {
            SavedReport::Signal { request: crate::SignalRequest::from_json(key, v, table)? }
        }
        "distribution-calculator" => {
            SavedReport::DistributionCalculator { request: crate::CalculatorRequest::from_json(v)? }
        }
        "image-plot" => SavedReport::ImagePlot { request: crate::image_plot::ImagePlotRequest::from_json(v, table)? },
        other => return Err(invalid(format!("unknown analysis {other:?}"))),
    })
}

/// `opts[key]` unless it is missing or null.
fn present<'a>(opts: Option<&'a JsonValue>, key: &str) -> Option<&'a JsonValue> {
    opts.and_then(|o| o.get(key)).filter(|v| !matches!(v, JsonValue::Null))
}

fn parse_alternative(v: &JsonValue, key: &str) -> Result<Alternative> {
    match v.get(key).and_then(JsonValue::as_str) {
        None | Some("two-sided") => Ok(Alternative::TwoSided),
        Some("less") => Ok(Alternative::Less),
        Some("greater") => Ok(Alternative::Greater),
        Some(other) => Err(invalid(format!("unknown alternative {other:?}"))),
    }
}

fn tolerance_to_json(t: &ToleranceSettings) -> JsonValue {
    let sides = match t.sides {
        ToleranceSides::TwoSided => "two-sided",
        ToleranceSides::Lower => "lower",
        ToleranceSides::Upper => "upper",
    };
    obj([
        ("proportion", JsonValue::number(t.proportion)),
        ("confidence", JsonValue::number(t.confidence)),
        ("sides", s(sides)),
        ("method", s(if t.distribution_free { "distribution-free" } else { "normal" })),
    ])
}

fn tolerance_from_json(v: &JsonValue) -> Result<ToleranceSettings> {
    let d = ToleranceSettings::default();
    let sides = match v.get("sides").and_then(JsonValue::as_str) {
        None | Some("two-sided") => ToleranceSides::TwoSided,
        Some("lower") => ToleranceSides::Lower,
        Some("upper") => ToleranceSides::Upper,
        Some(other) => return Err(invalid(format!("tolerance interval: unknown sides {other:?}"))),
    };
    let distribution_free = match v.get("method").and_then(JsonValue::as_str) {
        None | Some("normal") => false,
        Some("distribution-free") => true,
        Some(other) => return Err(invalid(format!("tolerance interval: unknown method {other:?}"))),
    };
    Ok(ToleranceSettings {
        proportion: v.get("proportion").and_then(JsonValue::as_f64).unwrap_or(d.proportion),
        confidence: v.get("confidence").and_then(JsonValue::as_f64).unwrap_or(d.confidence),
        sides,
        distribution_free,
    })
}

fn outlier_method_name(m: OutlierMethod) -> &'static str {
    match m {
        OutlierMethod::Grubbs => "grubbs",
        OutlierMethod::Dixon => "dixon",
        OutlierMethod::GeneralizedEsd => "generalized-esd",
    }
}

fn outliers_to_json(o: &OutlierSettings) -> JsonValue {
    obj([
        ("method", s(outlier_method_name(o.method))),
        ("side", s(alternative_name(o.side))),
        ("alpha", JsonValue::number(o.alpha)),
        ("dixon_ratio", o.dixon_ratio.map_or(JsonValue::Null, |r| s(r.label()))),
        ("max_outliers", o.max_outliers.map_or(JsonValue::Null, |m| JsonValue::number(m as f64))),
    ])
}

fn outliers_from_json(v: &JsonValue) -> Result<OutlierSettings> {
    let d = OutlierSettings::default();
    let method = match v.get("method").and_then(JsonValue::as_str) {
        None => d.method,
        Some(name) => OutlierMethod::ALL
            .into_iter()
            .find(|&m| outlier_method_name(m) == name)
            .ok_or_else(|| invalid(format!("unknown outlier test {name:?}")))?,
    };
    let dixon_ratio = match v.get("dixon_ratio").and_then(JsonValue::as_str) {
        None => None,
        Some(name) => Some(
            DixonRatio::ALL
                .into_iter()
                .find(|r| r.label() == name)
                .ok_or_else(|| invalid(format!("unknown Dixon ratio {name:?}")))?,
        ),
    };
    Ok(OutlierSettings {
        method,
        side: parse_alternative(v, "side")?,
        alpha: v.get("alpha").and_then(JsonValue::as_f64).unwrap_or(d.alpha),
        dixon_ratio,
        max_outliers: v.get("max_outliers").and_then(JsonValue::as_usize),
    })
}

fn runs_to_json(r: &RunsSettings) -> JsonValue {
    obj([
        ("reference", r.reference.map_or(JsonValue::Null, JsonValue::number)),
        ("alternative", s(alternative_name(r.alternative))),
    ])
}

fn runs_from_json(v: &JsonValue) -> Result<RunsSettings> {
    Ok(RunsSettings {
        reference: v.get("reference").and_then(JsonValue::as_f64),
        alternative: parse_alternative(v, "alternative")?,
    })
}

fn alternative_name(a: Alternative) -> &'static str {
    match a {
        Alternative::TwoSided => "two-sided",
        Alternative::Less => "less",
        Alternative::Greater => "greater",
    }
}

/// Pairwise method of a linear-model report: `"none"` or a method name. Files
/// from before the choice was added carry `show_games_howell` instead.
fn linear_pairwise(opts: Option<&JsonValue>, default: Option<ComparisonMethod>) -> Result<Option<ComparisonMethod>> {
    let get = |key: &str| opts.and_then(|o| o.get(key));
    if let Some(name) = get("pairwise").and_then(JsonValue::as_str) {
        if name == "none" {
            return Ok(None);
        }
        return ComparisonMethod::MEANS
            .into_iter()
            .find(|&m| method_name(m) == name)
            .map(Some)
            .ok_or_else(|| invalid(format!("unknown pairwise method {name:?}")));
    }
    Ok(match get("show_games_howell").and_then(JsonValue::as_bool) {
        Some(true) => Some(ComparisonMethod::GamesHowell),
        Some(false) => None,
        None => default,
    })
}

fn median_ties_name(t: MedianTies) -> &'static str {
    match t {
        MedianTies::Below => "below",
        MedianTies::Above => "above",
        MedianTies::Ignore => "ignore",
    }
}

fn method_name(m: ComparisonMethod) -> &'static str {
    m.key()
}

/// JSON name of a comparisons-with-the-best direction.
fn best_name(d: stat_inference::best::BestDirection) -> &'static str {
    match d {
        stat_inference::best::BestDirection::Largest => "largest",
        stat_inference::best::BestDirection::Smallest => "smallest",
    }
}

/// Parses an optional comparisons-with-the-best direction (`null`: off).
fn parse_best(v: Option<&JsonValue>) -> Result<Option<stat_inference::best::BestDirection>> {
    match v {
        None | Some(JsonValue::Null) => Ok(None),
        Some(v) => {
            let name = v.as_str().unwrap_or_default();
            [stat_inference::best::BestDirection::Largest, stat_inference::best::BestDirection::Smallest]
                .into_iter()
                .find(|&d| best_name(d) == name)
                .map(Some)
                .ok_or_else(|| invalid(format!("unknown best direction {name:?}")))
        }
    }
}

/// Design name and parameters of a power design.
fn design_parts(d: &PowerDesign) -> (&'static str, Vec<(&'static str, f64)>) {
    match *d {
        PowerDesign::OneSampleT { difference, std_dev } => {
            ("one-sample-t", vec![("difference", difference), ("std_dev", std_dev)])
        }
        PowerDesign::TwoSampleT { difference, std_dev, ratio } => {
            ("two-sample-t", vec![("difference", difference), ("std_dev", std_dev), ("ratio", ratio)])
        }
        PowerDesign::OneWayAnova { groups, max_difference, std_dev } => {
            ("one-way-anova", vec![("groups", groups as f64), ("max_difference", max_difference), ("std_dev", std_dev)])
        }
        PowerDesign::OneProportion { null, actual } => ("one-proportion", vec![("null", null), ("actual", actual)]),
        PowerDesign::TwoProportions { first, second, ratio } => {
            ("two-proportions", vec![("first", first), ("second", second), ("ratio", ratio)])
        }
        PowerDesign::OneStdDev { null, actual } => ("one-std-dev", vec![("null", null), ("actual", actual)]),
        PowerDesign::TwoVariances { variance_ratio, ratio } => {
            ("two-variances", vec![("variance_ratio", variance_ratio), ("ratio", ratio)])
        }
        PowerDesign::EquivalenceOneSample { lower, upper, difference, std_dev } => (
            "equivalence-one-sample",
            vec![("lower", lower), ("upper", upper), ("difference", difference), ("std_dev", std_dev)],
        ),
        PowerDesign::EquivalenceTwoSample { lower, upper, difference, std_dev, ratio } => (
            "equivalence-two-sample",
            vec![
                ("lower", lower),
                ("upper", upper),
                ("difference", difference),
                ("std_dev", std_dev),
                ("ratio", ratio),
            ],
        ),
        PowerDesign::OneSampleZ { difference, std_dev } => {
            ("one-sample-z", vec![("difference", difference), ("std_dev", std_dev)])
        }
        PowerDesign::PairedT { difference, std_dev, correlation } => {
            ("paired-t", vec![("difference", difference), ("std_dev", std_dev), ("correlation", correlation)])
        }
        PowerDesign::OneRate { null, actual, exposure } => {
            ("one-rate", vec![("null", null), ("actual", actual), ("exposure", exposure)])
        }
        PowerDesign::TwoRates { first, second, exposure, ratio } => {
            ("two-rates", vec![("first", first), ("second", second), ("exposure", exposure), ("ratio", ratio)])
        }
        PowerDesign::MeanMargin { margin, std_dev } => ("margin-mean", vec![("margin", margin), ("std_dev", std_dev)]),
        PowerDesign::ProportionMargin { margin, proportion } => {
            ("margin-proportion", vec![("margin", margin), ("proportion", proportion)])
        }
        PowerDesign::StdDevMargin { margin, std_dev } => {
            ("margin-std-dev", vec![("margin", margin), ("std_dev", std_dev)])
        }
        PowerDesign::RateMargin { margin, rate, exposure } => {
            ("margin-rate", vec![("margin", margin), ("rate", rate), ("exposure", exposure)])
        }
        PowerDesign::ToleranceInterval { proportion, confidence, max_proportion, two_sided } => (
            "tolerance-interval",
            vec![
                ("proportion", proportion),
                ("confidence", confidence),
                ("max_proportion", max_proportion),
                ("two_sided", f64::from(u8::from(two_sided))),
            ],
        ),
        PowerDesign::DistributionFreeTolerance { proportion, removed, two_sided } => (
            "tolerance-distribution-free",
            vec![
                ("proportion", proportion),
                ("removed", removed as f64),
                ("two_sided", f64::from(u8::from(two_sided))),
            ],
        ),
        PowerDesign::TwoLevelFactorial { factors, fraction, center_points, interactions, effect, std_dev } => (
            "two-level-factorial",
            vec![
                ("factors", factors as f64),
                ("fraction", fraction as f64),
                ("center_points", center_points as f64),
                ("interactions", f64::from(u8::from(interactions))),
                ("effect", effect),
                ("std_dev", std_dev),
            ],
        ),
        PowerDesign::Screening { runs, factors, center_points, effect, std_dev } => (
            "screening",
            vec![
                ("runs", runs as f64),
                ("factors", factors as f64),
                ("center_points", center_points as f64),
                ("effect", effect),
                ("std_dev", std_dev),
            ],
        ),
        PowerDesign::GeneralFactorial { levels, other_cells, max_difference, std_dev } => (
            "general-factorial",
            vec![
                ("levels", levels as f64),
                ("other_cells", other_cells as f64),
                ("max_difference", max_difference),
                ("std_dev", std_dev),
            ],
        ),
    }
}

fn comparison_to_json(report: &SavedReport, table: &DataTable) -> JsonValue {
    let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
    let opt_col = |i: Option<usize>| i.map_or(JsonValue::Null, col);
    let cols = |c: &[usize]| JsonValue::Array(c.iter().map(|&i| col(i)).collect());
    let num = JsonValue::number;
    let opt_num = |v: Option<f64>| v.map_or(JsonValue::Null, JsonValue::number);
    let bounds = |v: Option<(f64, f64)>| v.map_or(JsonValue::Null, |(a, b)| JsonValue::Array(vec![num(a), num(b)]));
    let mut members: Vec<(String, JsonValue)> = Vec::new();
    let mut put = |k: &str, v: JsonValue| members.push((k.to_owned(), v));
    let common = |put: &mut dyn FnMut(&str, JsonValue), alt: Alternative, level: f64| {
        put("alternative", s(alternative_name(alt)));
        put("level", num(level));
    };
    match report {
        SavedReport::OneSample { request: r } => {
            put("analysis", s("one-sample"));
            put("column", col(r.column));
            put("group", opt_col(r.group));
            put("null_mean", num(r.null_mean));
            put("known_std_dev", opt_num(r.known_std_dev));
            put("null_std_dev", opt_num(r.null_std_dev));
            put("equivalence", bounds(r.equivalence));
            common(&mut put, r.alternative, r.level);
        }
        SavedReport::Groups { request: r } => {
            put("analysis", s("compare-groups"));
            match &r.layout {
                GroupLayout::Stacked { response, group } => {
                    put("layout", s("stacked"));
                    put("response", col(*response));
                    put("group", col(*group));
                }
                GroupLayout::Columns(c) => {
                    put("layout", s("columns"));
                    put("columns", cols(c));
                }
            }
            put("null_difference", num(r.null_difference));
            put("method", s(method_name(r.method)));
            put("equivalence", bounds(r.equivalence));
            put("control", r.control.as_deref().map_or(JsonValue::Null, s));
            put("median_ties", s(median_ties_name(r.median_ties)));
            put("best", r.best.map_or(JsonValue::Null, |d| s(best_name(d))));
            put("linear_trend", JsonValue::Bool(r.linear_trend));
            put("ratio_of_means", JsonValue::Bool(r.ratio_of_means));
            common(&mut put, r.alternative, r.level);
        }
        SavedReport::Paired { request: r } => {
            put("analysis", s("paired"));
            match &r.layout {
                PairedLayout::Columns(c) => {
                    put("layout", s("columns"));
                    put("columns", cols(c));
                }
                PairedLayout::Stacked { response, condition, subject } => {
                    put("layout", s("stacked"));
                    put("response", col(*response));
                    put("condition", col(*condition));
                    put("subject", col(*subject));
                }
            }
            put("null_difference", num(r.null_difference));
            put("equivalence", bounds(r.equivalence));
            put("ratio_equivalence", JsonValue::Bool(r.ratio_equivalence));
            common(&mut put, r.alternative, r.level);
        }
        SavedReport::Proportions { request: r } => {
            put("analysis", s("proportions"));
            put("outcome", col(r.outcome));
            put("event", r.event.as_deref().map_or(JsonValue::Null, s));
            match r.comparison {
                ProportionComparison::OneSample { null } => {
                    put("comparison", s("one-sample"));
                    put("null", num(null));
                }
                ProportionComparison::Groups { group } => {
                    put("comparison", s("groups"));
                    put("group", col(group));
                }
                ProportionComparison::Paired { second } => {
                    put("comparison", s("paired"));
                    put("second", col(second));
                }
            }
            common(&mut put, r.alternative, r.level);
        }
        SavedReport::Rates { request: r } => {
            put("analysis", s("rates"));
            put("count", col(r.count));
            put("exposure", opt_col(r.exposure));
            put("group", opt_col(r.group));
            put("null_rate", num(r.null_rate));
            common(&mut put, r.alternative, r.level);
        }
        SavedReport::Power { request: r } => {
            put("analysis", s("power"));
            let (name, params) = design_parts(&r.design);
            put("design", s(name));
            put("parameters", JsonValue::Object(params.into_iter().map(|(k, v)| (k.to_owned(), num(v))).collect()));
            put("alpha", num(r.alpha));
            put("alternative", s(alternative_name(r.alternative)));
            match r.solve {
                PowerSolve::Power { n } => {
                    put("solve", s("power"));
                    put("n", num(n as f64));
                }
                PowerSolve::SampleSize { target } => {
                    put("solve", s("sample-size"));
                    put("target", num(target));
                }
                PowerSolve::Effect { n, target } => {
                    put("solve", s("effect"));
                    put("n", num(n as f64));
                    put("target", num(target));
                }
            }
        }
        _ => unreachable!("comparison_to_json is only called for comparison reports"),
    }
    JsonValue::Object(members)
}

fn comparison_from_json(analysis: &str, v: &JsonValue, table: &DataTable) -> Result<SavedReport> {
    let col = |key: &str| -> Result<usize> {
        let name = member(v, key)?.as_str().ok_or_else(|| invalid(format!("report {key}: expected a column name")))?;
        table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
    };
    let opt_col = |key: &str| match v.get(key) {
        None | Some(JsonValue::Null) => Ok(None),
        Some(_) => col(key).map(Some),
    };
    let col_list = |key: &str| -> Result<Vec<usize>> {
        array(v, key)?
            .iter()
            .map(|p| {
                let name = p.as_str().ok_or_else(|| invalid(format!("{key} must be column names")))?;
                table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
            })
            .collect()
    };
    let num = |key: &str, default: f64| v.get(key).and_then(JsonValue::as_f64).unwrap_or(default);
    let opt_num = |key: &str| v.get(key).and_then(JsonValue::as_f64);
    let bounds = |key: &str| -> Option<(f64, f64)> {
        match v.get(key)?.as_array()? {
            [a, b] => Some((a.as_f64()?, b.as_f64()?)),
            _ => None,
        }
    };
    let alternative = match v.get("alternative").and_then(JsonValue::as_str) {
        None | Some("two-sided") => Alternative::TwoSided,
        Some("less") => Alternative::Less,
        Some("greater") => Alternative::Greater,
        Some(other) => return Err(invalid(format!("unknown alternative {other:?}"))),
    };
    let level = num("level", 0.95);
    let layout = || member(v, "layout").map(|l| l.as_str().unwrap_or_default().to_owned());
    Ok(match analysis {
        "one-sample" => SavedReport::OneSample {
            request: OneSampleRequest {
                column: col("column")?,
                group: opt_col("group")?,
                null_mean: num("null_mean", 0.0),
                known_std_dev: opt_num("known_std_dev"),
                null_std_dev: opt_num("null_std_dev"),
                equivalence: bounds("equivalence"),
                alternative,
                level,
            },
        },
        "compare-groups" => {
            let layout = match layout()?.as_str() {
                "stacked" => GroupLayout::Stacked { response: col("response")?, group: col("group")? },
                "columns" => GroupLayout::Columns(col_list("columns")?),
                other => return Err(invalid(format!("unknown layout {other:?}"))),
            };
            let name = v.get("method").and_then(JsonValue::as_str).unwrap_or("tukey-kramer");
            let method = ComparisonMethod::from_key(name)
                .ok_or_else(|| invalid(format!("unknown comparison method {name:?}")))?;
            SavedReport::Groups {
                request: GroupComparisonRequest {
                    layout,
                    null_difference: num("null_difference", 0.0),
                    alternative,
                    level,
                    method,
                    equivalence: bounds("equivalence"),
                    control: v.get("control").and_then(JsonValue::as_str).map(str::to_owned),
                    median_ties: match v.get("median_ties").and_then(JsonValue::as_str) {
                        None => MedianTies::Below,
                        Some(name) => [MedianTies::Below, MedianTies::Above, MedianTies::Ignore]
                            .into_iter()
                            .find(|&t| median_ties_name(t) == name)
                            .ok_or_else(|| invalid(format!("unknown median ties rule {name:?}")))?,
                    },
                    best: parse_best(v.get("best"))?,
                    linear_trend: v.get("linear_trend").and_then(JsonValue::as_bool).unwrap_or(false),
                    ratio_of_means: v.get("ratio_of_means").and_then(JsonValue::as_bool).unwrap_or(false),
                },
            }
        }
        "paired" => {
            let layout = match layout()?.as_str() {
                "columns" => PairedLayout::Columns(col_list("columns")?),
                "stacked" => PairedLayout::Stacked {
                    response: col("response")?,
                    condition: col("condition")?,
                    subject: col("subject")?,
                },
                other => return Err(invalid(format!("unknown layout {other:?}"))),
            };
            SavedReport::Paired {
                request: PairedRequest {
                    layout,
                    null_difference: num("null_difference", 0.0),
                    alternative,
                    level,
                    equivalence: bounds("equivalence"),
                    ratio_equivalence: v.get("ratio_equivalence").and_then(JsonValue::as_bool).unwrap_or(false),
                },
            }
        }
        "proportions" => {
            let comparison = match member(v, "comparison")?.as_str().unwrap_or_default() {
                "one-sample" => ProportionComparison::OneSample { null: num("null", 0.5) },
                "groups" => ProportionComparison::Groups { group: col("group")? },
                "paired" => ProportionComparison::Paired { second: col("second")? },
                other => return Err(invalid(format!("unknown proportion comparison {other:?}"))),
            };
            SavedReport::Proportions {
                request: ProportionRequest {
                    outcome: col("outcome")?,
                    event: v.get("event").and_then(JsonValue::as_str).map(str::to_owned),
                    comparison,
                    alternative,
                    level,
                },
            }
        }
        "rates" => SavedReport::Rates {
            request: RateRequest {
                count: col("count")?,
                exposure: opt_col("exposure")?,
                group: opt_col("group")?,
                null_rate: num("null_rate", 1.0),
                alternative,
                level,
            },
        },
        _ => {
            let p = member(v, "parameters")?;
            let get = |k: &str| -> Result<f64> {
                p.get(k).and_then(JsonValue::as_f64).ok_or_else(|| invalid(format!("power: missing parameter {k:?}")))
            };
            let get_count = |k: &str| -> Result<usize> {
                p.get(k).and_then(JsonValue::as_usize).ok_or_else(|| invalid(format!("power: {k:?} must be a count")))
            };
            let design = match member(v, "design")?.as_str().unwrap_or_default() {
                "one-sample-t" => PowerDesign::OneSampleT { difference: get("difference")?, std_dev: get("std_dev")? },
                "two-sample-t" => PowerDesign::TwoSampleT {
                    difference: get("difference")?,
                    std_dev: get("std_dev")?,
                    ratio: get("ratio")?,
                },
                "one-way-anova" => PowerDesign::OneWayAnova {
                    groups: get("groups")? as usize,
                    max_difference: get("max_difference")?,
                    std_dev: get("std_dev")?,
                },
                "one-proportion" => PowerDesign::OneProportion { null: get("null")?, actual: get("actual")? },
                "two-proportions" => {
                    PowerDesign::TwoProportions { first: get("first")?, second: get("second")?, ratio: get("ratio")? }
                }
                "one-std-dev" => PowerDesign::OneStdDev { null: get("null")?, actual: get("actual")? },
                "two-variances" => {
                    PowerDesign::TwoVariances { variance_ratio: get("variance_ratio")?, ratio: get("ratio")? }
                }
                "equivalence-one-sample" => PowerDesign::EquivalenceOneSample {
                    lower: get("lower")?,
                    upper: get("upper")?,
                    difference: get("difference")?,
                    std_dev: get("std_dev")?,
                },
                "equivalence-two-sample" => PowerDesign::EquivalenceTwoSample {
                    lower: get("lower")?,
                    upper: get("upper")?,
                    difference: get("difference")?,
                    std_dev: get("std_dev")?,
                    ratio: get("ratio")?,
                },
                "one-sample-z" => PowerDesign::OneSampleZ { difference: get("difference")?, std_dev: get("std_dev")? },
                "paired-t" => PowerDesign::PairedT {
                    difference: get("difference")?,
                    std_dev: get("std_dev")?,
                    correlation: get("correlation")?,
                },
                "one-rate" => {
                    PowerDesign::OneRate { null: get("null")?, actual: get("actual")?, exposure: get("exposure")? }
                }
                "two-rates" => PowerDesign::TwoRates {
                    first: get("first")?,
                    second: get("second")?,
                    exposure: get("exposure")?,
                    ratio: get("ratio")?,
                },
                "margin-mean" => PowerDesign::MeanMargin { margin: get("margin")?, std_dev: get("std_dev")? },
                "margin-proportion" => {
                    PowerDesign::ProportionMargin { margin: get("margin")?, proportion: get("proportion")? }
                }
                "margin-std-dev" => PowerDesign::StdDevMargin { margin: get("margin")?, std_dev: get("std_dev")? },
                "margin-rate" => {
                    PowerDesign::RateMargin { margin: get("margin")?, rate: get("rate")?, exposure: get("exposure")? }
                }
                "tolerance-interval" => PowerDesign::ToleranceInterval {
                    proportion: get("proportion")?,
                    confidence: get("confidence")?,
                    max_proportion: get("max_proportion")?,
                    two_sided: get("two_sided")? != 0.0,
                },
                "tolerance-distribution-free" => PowerDesign::DistributionFreeTolerance {
                    proportion: get("proportion")?,
                    removed: get_count("removed")?,
                    two_sided: get("two_sided")? != 0.0,
                },
                "two-level-factorial" => PowerDesign::TwoLevelFactorial {
                    factors: get_count("factors")?,
                    fraction: get_count("fraction")?,
                    center_points: get_count("center_points")?,
                    interactions: get("interactions")? != 0.0,
                    effect: get("effect")?,
                    std_dev: get("std_dev")?,
                },
                "screening" => PowerDesign::Screening {
                    runs: get_count("runs")?,
                    factors: get_count("factors")?,
                    center_points: get_count("center_points")?,
                    effect: get("effect")?,
                    std_dev: get("std_dev")?,
                },
                "general-factorial" => PowerDesign::GeneralFactorial {
                    levels: get_count("levels")?,
                    other_cells: get_count("other_cells")?,
                    max_difference: get("max_difference")?,
                    std_dev: get("std_dev")?,
                },
                other => return Err(invalid(format!("unknown power design {other:?}"))),
            };
            let n = || v.get("n").and_then(JsonValue::as_usize).ok_or_else(|| invalid("power: missing n"));
            let target = || num("target", 0.8);
            let solve = match member(v, "solve")?.as_str().unwrap_or_default() {
                "power" => PowerSolve::Power { n: n()? },
                "sample-size" => PowerSolve::SampleSize { target: target() },
                "effect" => PowerSolve::Effect { n: n()?, target: target() },
                other => return Err(invalid(format!("unknown power question {other:?}"))),
            };
            SavedReport::Power { request: PowerRequest { design, alpha: num("alpha", 0.05), alternative, solve } }
        }
    })
}

pub(crate) const QUANTILE_METHODS: [QuantileMethod; 9] = [
    QuantileMethod::InverseCdf,
    QuantileMethod::AveragedInverseCdf,
    QuantileMethod::NearestEven,
    QuantileMethod::LinearCdf,
    QuantileMethod::Midpoint,
    QuantileMethod::Weibull,
    QuantileMethod::Linear,
    QuantileMethod::MedianUnbiased,
    QuantileMethod::NormalUnbiased,
];

fn rule_numbers(rules: RuleSet) -> JsonValue {
    JsonValue::Array(rules.numbers().into_iter().map(|n| JsonValue::Number(f64::from(n))).collect())
}

/// The project-file name of a control chart type.
#[must_use]
pub fn chart_name(kind: ChartKind) -> &'static str {
    match kind {
        ChartKind::MeanAndRange => "mean-range",
        ChartKind::MeanAndStdDev => "mean-std-dev",
        ChartKind::IndividualsAndMovingRange => "individuals-moving-range",
        ChartKind::MovingAverage => "moving-average",
        ChartKind::ThreeWayRange => "three-way-range",
        ChartKind::ThreeWayStdDev => "three-way-std-dev",
        ChartKind::Zone => "zone",
        ChartKind::ShortRun => "short-run",
    }
}

fn parse_chart(name: &str) -> Option<ChartKind> {
    ChartKind::ALL.into_iter().find(|&k| chart_name(k) == name)
}

/// The project-file name of a measurement level.
#[must_use]
pub fn level_name(level: MeasurementLevel) -> &'static str {
    match level {
        MeasurementLevel::Continuous => "continuous",
        MeasurementLevel::Nominal => "nominal",
        MeasurementLevel::Ordinal => "ordinal",
    }
}

fn parse_level(name: &str) -> Option<MeasurementLevel> {
    [MeasurementLevel::Continuous, MeasurementLevel::Nominal, MeasurementLevel::Ordinal]
        .into_iter()
        .find(|&l| level_name(l) == name)
}

/// A row colour from `"#rrggbb"` text.
#[must_use]
pub fn parse_color(text: &str) -> Option<RowColor> {
    let hex = text.strip_prefix('#').filter(|h| h.len() == 6 && h.is_ascii())?;
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some(RowColor { r: byte(0)?, g: byte(2)?, b: byte(4)? })
}

pub(crate) fn s(text: &str) -> JsonValue {
    JsonValue::String(text.to_owned())
}

/// Effects as `[[[position, power], …], …]`.
fn subset_criterion_name(c: SubsetCriterion) -> &'static str {
    match c {
        SubsetCriterion::RSquared => "r-squared",
        SubsetCriterion::AdjustedRSquared => "adjusted-r-squared",
        SubsetCriterion::Cp => "cp",
        SubsetCriterion::Aicc => "aicc",
        SubsetCriterion::Bic => "bic",
    }
}

pub(crate) fn effects_json(effects: &[Vec<(usize, u8)>]) -> JsonValue {
    JsonValue::Array(
        effects
            .iter()
            .map(|e| {
                JsonValue::Array(
                    e.iter()
                        .map(|&(i, p)| {
                            JsonValue::Array(vec![JsonValue::Number(i as f64), JsonValue::Number(f64::from(p))])
                        })
                        .collect(),
                )
            })
            .collect(),
    )
}

/// The `splines` member of a model report: one entry per predictor position,
/// `null` or `{"kind": "natural" | "b-spline", "degree": d, "knots": m or [t₁, …]}`
/// (absent when no predictor is a spline).
pub(crate) fn splines_json(splines: &[Option<stat_linear::SplineSpec>]) -> JsonValue {
    use stat_linear::{SplineKind, SplineKnots};
    if splines.iter().all(Option::is_none) {
        return JsonValue::Null;
    }
    JsonValue::Array(
        splines
            .iter()
            .map(|sp| match sp {
                None => JsonValue::Null,
                Some(sp) => {
                    let mut members = vec![(
                        "kind".to_owned(),
                        JsonValue::String(
                            if sp.kind == SplineKind::Natural { "natural" } else { "b-spline" }.to_owned(),
                        ),
                    )];
                    if let SplineKind::BSpline { degree } = sp.kind {
                        members.push(("degree".to_owned(), JsonValue::Number(f64::from(degree))));
                    }
                    members.push((
                        "knots".to_owned(),
                        match &sp.knots {
                            SplineKnots::Count(m) => JsonValue::Number(*m as f64),
                            SplineKnots::At(k) => JsonValue::Array(k.iter().map(|&v| JsonValue::number(v)).collect()),
                        },
                    ));
                    JsonValue::Object(members)
                }
            })
            .collect(),
    )
}

/// Read the `splines` member written by [`splines_json`].
pub(crate) fn parse_splines(v: &JsonValue) -> Result<Vec<Option<stat_linear::SplineSpec>>> {
    use stat_linear::{SplineKind, SplineKnots, SplineSpec};
    let bad = |m: &str| invalid(format!("splines: {m}"));
    let Some(list) = v.get("splines").filter(|l| !matches!(l, JsonValue::Null)) else { return Ok(Vec::new()) };
    let list = list.as_array().ok_or_else(|| bad("expected a list with one entry per predictor"))?;
    list.iter()
        .map(|e| {
            if matches!(e, JsonValue::Null) {
                return Ok(None);
            }
            let kind = match e.get("kind").and_then(JsonValue::as_str) {
                Some("natural") => SplineKind::Natural,
                Some("b-spline") => {
                    let d = e.get("degree").and_then(JsonValue::as_usize).unwrap_or(3);
                    SplineKind::BSpline { degree: u8::try_from(d).map_err(|_| bad("degree too large"))? }
                }
                _ => return Err(bad("kind must be \"natural\" or \"b-spline\"")),
            };
            let knots = match e.get("knots") {
                None => SplineKnots::Count(3),
                Some(JsonValue::Array(k)) => SplineKnots::At(
                    k.iter().map(|x| x.as_f64().ok_or_else(|| bad("knots must be numbers"))).collect::<Result<_>>()?,
                ),
                Some(n) => SplineKnots::Count(n.as_usize().ok_or_else(|| bad("knots must be a count or a list"))?),
            };
            Ok(Some(SplineSpec { kind, knots }))
        })
        .collect()
}

/// A pairwise adjustment setting: `null` (off) or its key.
pub(crate) fn adjust_json(a: Option<stat_linear::marginal::PairAdjust>) -> JsonValue {
    a.map_or(JsonValue::Null, |a| JsonValue::String(a.key().to_owned()))
}

/// Read an adjustment written by [`adjust_json`] from `opts[key]` (absent: off).
pub(crate) fn parse_adjust(opts: Option<&JsonValue>, key: &str) -> Result<Option<stat_linear::marginal::PairAdjust>> {
    match opts.and_then(|o| o.get(key)).and_then(JsonValue::as_str) {
        None => Ok(None),
        Some(k) => stat_linear::marginal::PairAdjust::from_key(k)
            .map(Some)
            .ok_or_else(|| invalid(format!("unknown pairwise adjustment {k:?}"))),
    }
}

/// The `effects` member of a linear-model report (absent in older files: main effects).
pub(crate) fn linear_effects(v: &JsonValue) -> Result<Vec<Vec<(usize, u8)>>> {
    let bad = || invalid("linear model: effects must be lists of [predictor position, power]");
    let Some(list) = v.get("effects") else { return Ok(Vec::new()) };
    let list = list.as_array().ok_or_else(bad)?;
    list.iter()
        .map(|e| {
            e.as_array()
                .ok_or_else(bad)?
                .iter()
                .map(|pair| match pair.as_array() {
                    Some([i, p]) => {
                        let i = i.as_usize().ok_or_else(bad)?;
                        let p = p.as_usize().and_then(|p| u8::try_from(p).ok()).ok_or_else(bad)?;
                        Ok((i, p))
                    }
                    _ => Err(bad()),
                })
                .collect()
        })
        .collect()
}

pub(crate) fn obj<const N: usize>(members: [(&str, JsonValue); N]) -> JsonValue {
    JsonValue::Object(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

pub(crate) fn member<'a>(v: &'a JsonValue, key: &str) -> Result<&'a JsonValue> {
    v.get(key).ok_or_else(|| invalid(format!("missing {key:?}")))
}

pub(crate) fn array<'a>(v: &'a JsonValue, key: &str) -> Result<&'a [JsonValue]> {
    member(v, key)?.as_array().ok_or_else(|| invalid(format!("{key:?} must be an array")))
}

pub(crate) fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::{gauge_study, longley};
    use crate::{BivariateAnalysis, ControlChartAnalysis, DistributionAnalysis, GaugeAnalysis, LinearModelAnalysis};
    use crate::{StepCriterion, StepDirection};

    fn every_report(table: &DataTable) -> Vec<SavedReport> {
        let first = table.column(0).unwrap().name.clone();
        vec![
            // A report with its own row filter and column picker.
            SavedReport::Distribution {
                request: DistributionRequest { column: 1, quantile_method: QuantileMethod::default(), censoring: None },
                options: DistributionOptions::default(),
            }
            .with_scope(crate::ReportScope {
                filter: vec![stat_viz::FilterCondition::Range { column: first.clone(), min: -1e9, max: 1e9 }],
                switcher: Some(vec![first]),
                spec_limits: Vec::new(),
                pool: None,
                contour: Some(stat_viz::ContourStyle {
                    labels: false,
                    scale: stat_viz::palette::ColorScale::Heat,
                    reverse: true,
                    levels: Some(7),
                    range: Some((1.0, 9.5)),
                    centre: Some(4.0),
                    opacity: 0.6,
                    ..stat_viz::ContourStyle::default()
                }),
                plot_styles: vec![(crate::report_scope::plot_style_key(crate::report::SectionKind::Histogram, 0), {
                    let mut style = crate::plot_style::PlotStyle::default();
                    style.axes.x.major = stat_viz::axis_settings::MajorTicks::Every(0.5);
                    style.axes.show_key = false;
                    style
                })],
            }),
            SavedReport::Linear {
                spec: LinearModelSpec {
                    response: table.n_cols() - 1,
                    predictors: vec![0, 1],
                    effects: vec![vec![(0, 1)], vec![(1, 1)], vec![(0, 1), (1, 1)], vec![(1, 2)]],
                    splines: Vec::new(),
                },
                options: ReportOptions {
                    show_residual_plot: true,
                    show_anova: false,
                    show_diagnostics: true,
                    show_profiler: false,
                    show_surface_profiler: true,
                    show_surface_observed: true,
                    pairwise: Some(ComparisonMethod::GamesHowell),
                    show_equal_variances: false,
                    control_level: Some(1),
                    best: Some(stat_inference::best::BestDirection::Smallest),
                    ..ReportOptions::default()
                },
            },
            SavedReport::ControlChart {
                request: ControlChartRequest {
                    label: Some(0),
                    ..ControlChartRequest::new(2, ChartKind::MeanAndStdDev, 4)
                },
                options: ControlChartOptions {
                    show_zones: false,
                    location_rules: RuleSet::from_numbers([1, 2, 5]),
                    ..ControlChartOptions::default()
                },
            },
            SavedReport::Distribution {
                request: DistributionRequest { column: 3, quantile_method: QuantileMethod::Weibull, censoring: None },
                options: DistributionOptions {
                    show_box_plot: false,
                    show_quantile_plot: true,
                    ..DistributionOptions::default()
                },
            },
            SavedReport::Distribution {
                request: DistributionRequest { column: 2, quantile_method: QuantileMethod::default(), censoring: None },
                options: DistributionOptions {
                    tolerance: Some(ToleranceSettings {
                        proportion: 0.99,
                        confidence: 0.9,
                        sides: ToleranceSides::Upper,
                        distribution_free: true,
                    }),
                    outliers: Some(OutlierSettings {
                        method: OutlierMethod::Dixon,
                        side: Alternative::Greater,
                        alpha: 0.01,
                        dixon_ratio: Some(DixonRatio::R21),
                        max_outliers: Some(4),
                    }),
                    runs: Some(RunsSettings { reference: Some(2.5), alternative: Alternative::Less }),
                    ..DistributionOptions::default()
                },
            },
            SavedReport::Bivariate { request: BivariateRequest::new(1, 4) },
            SavedReport::Pca { request: PcaRequest { columns: vec![1, 2, 3], standardize: true } },
            SavedReport::Correspondence { request: CorrespondenceRequest { row: 1, column: 3 } },
            SavedReport::Correlations {
                request: crate::CorrelationRequest {
                    missing: stat_multivariate::MissingRows::Listwise,
                    level: 0.9,
                    spearman: true,
                    covariance: true,
                    ..crate::CorrelationRequest::new(vec![1, 2, 3])
                },
            },
            SavedReport::OutlierDistances {
                request: crate::OutlierRequest {
                    distance: crate::OutlierDistance::Jackknife,
                    alpha: 0.01,
                    ..crate::OutlierRequest::new(vec![1, 2, 3])
                },
            },
            SavedReport::ItemAnalysis {
                request: crate::ItemAnalysisRequest {
                    reversed: vec![3],
                    scale: Some((1.0, 5.0)),
                    level: 0.9,
                    ..crate::ItemAnalysisRequest::new(vec![1, 2, 3])
                },
            },
            SavedReport::ItemAnalysis { request: crate::ItemAnalysisRequest::new(vec![2, 4]) },
            SavedReport::Manova {
                request: crate::ManovaRequest {
                    effects: vec![vec![(0, 1)], vec![(1, 1)], vec![(0, 1), (1, 1)]],
                    statistic: stat_linear::manova::MultivariateStatistic::Wilks,
                    ..crate::ManovaRequest::new(vec![table.n_cols() - 2, table.n_cols() - 1], vec![0, 1])
                },
            },
            SavedReport::FactorAnalysis {
                request: crate::FactorRequest {
                    factors: Some(1),
                    extraction: stat_multivariate::Extraction::PrincipalAxis,
                    rotation: stat_multivariate::Rotation::Promax,
                    covariance: true,
                    scores: stat_multivariate::ScoreMethod::Bartlett,
                    promax_power: 3.0,
                    ..crate::FactorRequest::new(vec![1, 2, 3, 4])
                },
            },
            SavedReport::FactorAnalysis { request: crate::FactorRequest::new(vec![1, 2, 3]) },
            SavedReport::DiscriminantAnalysis {
                request: crate::DiscriminantRequest {
                    rule: stat_multivariate::DiscriminantRule::Quadratic,
                    priors: crate::PriorChoice::Equal,
                    validation: crate::ValidationMethod::KFold { folds: 3, seed: 5 },
                    ..crate::DiscriminantRequest::new(0, vec![1, 2])
                },
            },
            SavedReport::GaussianMixture {
                request: crate::GaussianMixtureRequest {
                    structures: vec![crate::CovarianceStructure::Eii, crate::CovarianceStructure::Vvv],
                    max_components: 3,
                    standardize: true,
                    start: crate::MixtureStart::Hierarchical,
                    seed: 4,
                    ..crate::GaussianMixtureRequest::new(vec![1, 2])
                },
            },
            SavedReport::TextExploration {
                request: crate::TextExplorationRequest {
                    dimensions: 3,
                    clusters: 2,
                    seed: 4,
                    ..crate::TextExplorationRequest::new(1)
                },
            },
            SavedReport::TopicModel {
                request: crate::TopicModelRequest { topics: 3, ..crate::TopicModelRequest::new(1) },
            },
            SavedReport::Sentiment {
                request: crate::SentimentRequest { threshold: 0.2, ..crate::SentimentRequest::new(1) },
            },
            SavedReport::LatentClass {
                request: crate::LatentClassRequest {
                    min_classes: 2,
                    max_classes: 3,
                    starts: 3,
                    smoothing: 0.25,
                    seed: 6,
                    ..crate::LatentClassRequest::new(vec![1, 2, 3])
                },
            },
            SavedReport::Embedding {
                request: crate::EmbeddingRequest {
                    perplexity: 5.0,
                    color: Some(3),
                    iterations: Some(250),
                    ..crate::EmbeddingRequest::new(crate::EmbeddingMethod::Tsne, vec![1, 2])
                },
            },
            SavedReport::Imputation { request: crate::ImputationRequest::new(vec![1, 2, 3]) },
            SavedReport::SeriesExplore {
                request: crate::SeriesExploreRequest {
                    series: crate::SeriesInput { column: 1, time: Some(table.n_cols() - 1), period: Some(4) },
                    method: crate::ExploreMethod::Describe(crate::DescribeSettings {
                        box_cox: crate::BoxCoxChoice::Guerrero,
                        differences: 1,
                        max_lag: Some(5),
                        bartlett: true,
                        spectrum: true,
                        spans: vec![3],
                        period_axis: true,
                        adf: stat_timeseries::unit_root::AdfDeterministic::ConstantTrend,
                        ..crate::DescribeSettings::default()
                    }),
                },
            },
            SavedReport::SeriesExplore {
                request: crate::SeriesExploreRequest {
                    series: crate::SeriesInput { column: 2, time: None, period: Some(4) },
                    method: crate::ExploreMethod::Decompose(crate::DecomposeSettings {
                        method: crate::DecomposeMethod::Stl,
                        seasonal_window: Some(7),
                        robust: true,
                    }),
                },
            },
            SavedReport::SeriesExplore {
                request: crate::SeriesExploreRequest {
                    series: crate::SeriesInput::new(2),
                    method: crate::ExploreMethod::Trend(crate::TrendSettings {
                        model: crate::TrendChoice::MovingAverage,
                        span: 4,
                        horizon: 3,
                        confidence: 0.9,
                    }),
                },
            },
            SavedReport::SeriesExplore {
                request: crate::SeriesExploreRequest {
                    series: crate::SeriesInput { column: 1, time: None, period: None },
                    method: crate::ExploreMethod::CrossCorrelation(crate::CrossSettings {
                        max_lag: Some(3),
                        prewhiten: crate::Prewhiten::Order(1),
                        ..crate::CrossSettings::new(2)
                    }),
                },
            },
            SavedReport::SignalProcessing { request: crate::SignalProcessingRequest::spectrum(1) },
            SavedReport::SignalProcessing {
                request: crate::SignalProcessingRequest {
                    input: crate::SignalInput { column: 2, x: Some(1), group: None, sampling_rate: None },
                    method: crate::SignalMethod::Filter(crate::signal_analysis::FilterSettings::lowpass(0.1)),
                },
            },
            SavedReport::SignalProcessing {
                request: crate::SignalProcessingRequest {
                    input: crate::SignalInput { column: 1, x: None, group: None, sampling_rate: Some(4.0) },
                    method: crate::SignalMethod::Correlation(crate::signal_analysis::CorrelationSettings {
                        operation: crate::signal_analysis::CorrelationOperation::Cross {
                            other: 2,
                            scaling: stat_signal::convolve::CorrelationScaling::Coefficient,
                            max_lag: Some(4),
                        },
                    }),
                },
            },
            SavedReport::SignalProcessing {
                request: crate::SignalProcessingRequest {
                    input: crate::SignalInput::new(3),
                    method: crate::SignalMethod::Wavelet(crate::signal_analysis::WaveletSettings {
                        wavelet: "haar".to_owned(),
                        scalogram: true,
                        ..crate::signal_analysis::WaveletSettings::default()
                    }),
                },
            },
            SavedReport::Imputation {
                request: crate::ImputationRequest {
                    method: crate::ImputationMethod::LowRank,
                    rank: Some(1),
                    shrinkage: 0.25,
                    standardize: false,
                    ..crate::ImputationRequest::new(vec![1, 2, 3])
                },
            },
            SavedReport::Cluster {
                request: ClusterRequest {
                    distance: Default::default(),
                    columns: vec![1, 2],
                    method: ClusterMethod::Hierarchical { linkage: Linkage::Average, k: 3 },
                    standardize: true,
                    choose: None,
                    centers: None,
                },
            },
            SavedReport::Cluster {
                request: ClusterRequest {
                    distance: Default::default(),
                    columns: vec![1, 3],
                    method: ClusterMethod::KMeans { k: 2, seed: 9 },
                    standardize: false,
                    choose: None,
                    centers: Some(crate::StartingCenters {
                        centers: vec![vec![1.0, 2.5], vec![-0.5, 4.0]],
                        also_seeded: true,
                    }),
                },
            },
            SavedReport::Glm {
                request: GlmRequest {
                    response: 0,
                    predictors: vec![1],
                    family: Family::Gaussian,
                    link: None,
                    event: None,
                    effects: Vec::new(),
                    splines: Vec::new(),
                    slices: None,
                    slopes: None,
                },
                profilers: ProfilerOptions { show_profiler: false, show_surface_profiler: true, show_observed: true },
            },
            SavedReport::Glm {
                request: GlmRequest {
                    response: 0,
                    predictors: vec![1],
                    family: Family::Gamma,
                    link: Some(stat_discrete::Link::Log),
                    event: None,
                    effects: Vec::new(),
                    splines: Vec::new(),
                    slices: None,
                    slopes: None,
                },
                profilers: ProfilerOptions { show_profiler: false, show_surface_profiler: true, show_observed: true },
            },
            SavedReport::Regularized {
                request: RegularizedRequest { response: 0, predictors: vec![1, 2, 4], alpha: 0.5, folds: 4, seed: 3 },
                profilers: ProfilerOptions::default(),
            },
            SavedReport::DecisionTree {
                request: crate::TreeRequest {
                    validation: crate::ValidationMethod::KFold { folds: 4, seed: 2 },
                    min_split: 6,
                    min_leaf: 3,
                    ..crate::TreeRequest::new(0, vec![1, 2])
                },
                profilers: ProfilerOptions::default(),
            },
            SavedReport::RandomForest {
                request: crate::ForestRequest {
                    trees: 12,
                    features_per_split: 1,
                    sample_fraction: 0.8,
                    min_split: 4,
                    min_leaf: 2,
                    seed: 5,
                    ..crate::ForestRequest::new(0, vec![1, 2, 3])
                },
                profilers: ProfilerOptions { show_profiler: true, show_surface_profiler: false, show_observed: true },
            },
            SavedReport::BoostedTrees {
                request: crate::BoostRequest {
                    rounds: 15,
                    learning_rate: 0.3,
                    max_depth: 2,
                    min_leaf: 2,
                    lambda: 0.5,
                    subsample: 0.9,
                    early_stopping: 5,
                    regression_loss: stat_predictive::RegressionLoss::Absolute,
                    seed: 4,
                    ..crate::BoostRequest::new(0, vec![1, 2])
                },
                profilers: ProfilerOptions { show_profiler: false, show_surface_profiler: true, show_observed: false },
            },
            SavedReport::PartialLeastSquares {
                request: crate::PlsRequest {
                    max_factors: 3,
                    factors: Some(2),
                    algorithm: crate::PlsAlgorithm::Nipals,
                    scale: false,
                    ..crate::PlsRequest::new(0, vec![1, 2, 3])
                },
                profilers: ProfilerOptions { show_profiler: true, show_surface_profiler: false, show_observed: true },
            },
            SavedReport::SupportVectorMachine {
                request: crate::SvmRequest {
                    kernel: crate::SvmKernel::Polynomial { degree: 2 },
                    cost: Some(4.0),
                    gamma: Some(0.5),
                    epsilon: 0.2,
                    seed: 9,
                    ..crate::SvmRequest::new(0, vec![1, 2, 3])
                },
                profilers: ProfilerOptions { show_profiler: true, show_surface_profiler: false, show_observed: true },
            },
            SavedReport::FunctionalData {
                request: crate::FunctionalRequest {
                    factors: vec![3],
                    basis: crate::FunctionalBasis::Fourier { functions: Some(5), period: Some(12.5) },
                    smoothing: crate::FunctionalSmoothing::Fixed(0.25),
                    landmarks: vec![
                        crate::FunctionalLandmark { maximum: true, window: Some((1.0, 4.0)) },
                        crate::FunctionalLandmark { maximum: false, window: None },
                    ],
                    continuous: true,
                    warp_functions: 5,
                    warp_penalty: 1e-3,
                    components: Some(2),
                    interactions: true,
                    confidence: 0.9,
                    ..crate::FunctionalRequest::new(0, 1, 2)
                },
                profilers: ProfilerOptions { show_profiler: false, show_surface_profiler: true, show_observed: true },
            },
            SavedReport::Mars {
                request: crate::MarsRequest {
                    linear: vec![2],
                    max_degree: 2,
                    max_terms: 11,
                    penalty: Some(2.5),
                    threshold: 0.01,
                    min_span: 2,
                    end_span: 3,
                    validation: crate::ValidationMethod::KFold { folds: 4, seed: 3 },
                    ..crate::MarsRequest::new(0, vec![1, 2, 3])
                },
                profilers: ProfilerOptions { show_profiler: true, show_surface_profiler: false, show_observed: true },
            },
            SavedReport::NeuralNetwork {
                request: crate::NeuralRequest {
                    hidden: vec![4, 2],
                    activation: stat_predictive::Activation::Logistic,
                    penalty: Some(0.1),
                    solver: Some(stat_predictive::Solver::Lbfgs),
                    starts: 2,
                    max_iter: 200,
                    seed: 5,
                    ..crate::NeuralRequest::new(0, vec![1, 2, 3])
                },
                profilers: ProfilerOptions { show_profiler: true, show_surface_profiler: false, show_observed: true },
            },
            SavedReport::GaussianProcess {
                request: crate::GaussianProcessRequest {
                    kernel: stat_predictive::CovarianceKernel::SquaredExponential,
                    nugget: Some(0.001),
                    starts: 3,
                    seed: 4,
                    ..crate::GaussianProcessRequest::new(0, vec![1, 2, 3])
                },
                profilers: ProfilerOptions { show_profiler: true, show_surface_profiler: true, show_observed: false },
            },
            SavedReport::SequentialOptimization {
                request: Box::new(crate::SequentialRequest {
                    batch: 3,
                    constraints: "GNP <= 500000".to_owned(),
                    criterion: stat_predictive::AcquisitionCriterion::UpperConfidenceBound { kappa: 1.5 },
                    stop_value: Some(4.0),
                    seed: 9,
                    ..crate::SequentialRequest::new(vec![0], vec![1, 2])
                }),
            },
            SavedReport::ModelSearch {
                request: crate::ModelSearchRequest {
                    models: vec![crate::ModelKind::DecisionTree, crate::ModelKind::Mars],
                    validation: crate::ValidationMethod::KFold { folds: 3, seed: 2 },
                    seed: 6,
                    ..crate::ModelSearchRequest::new(0, vec![1, 2, 3])
                },
            },
            SavedReport::ModelComparison {
                request: crate::ModelComparisonRequest {
                    response: 0,
                    models: vec![1, 2],
                    event: None,
                    validation: None,
                },
            },
            SavedReport::Mixed {
                request: crate::MixedRequest {
                    response: 0,
                    predictors: vec![1],
                    effects: Vec::new(),
                    random: vec![crate::RandomEffectSpec {
                        group: 3,
                        within: Vec::new(),
                        intercept: true,
                        slopes: Vec::new(),
                        correlated: true,
                    }],
                    family: crate::MixedFamily::Normal,
                    trials: None,
                    offset: None,
                    reml: true,
                    quadrature_points: 1,
                    slices: None,
                    slopes: None,
                    frequency: None,
                    weight: None,
                    robust: false,
                    residual_groups: None,
                    df_method: stat_mixed::DfMethod::Satterthwaite,
                    order: Vec::new(),
                },
                profilers: ProfilerOptions { show_profiler: true, show_surface_profiler: false, show_observed: true },
            },
            SavedReport::TimeWeighted {
                request: TimeWeightedRequest { column: 0, kind: TimeWeightedKind::EWMA_DEFAULT, target: None },
            },
            SavedReport::Attribute {
                request: AttributeChartRequest::new(AttributeChartKind::C, 6, SubgroupSize::None),
            },
            SavedReport::Attribute {
                request: AttributeChartRequest {
                    phase: None,
                    rules: RuleSet::from_numbers([1, 2, 5]),
                    ..AttributeChartRequest::new(AttributeChartKind::LaneyP, 6, SubgroupSize::Column(5))
                },
            },
            SavedReport::Attribute {
                request: AttributeChartRequest::new(AttributeChartKind::G, 6, SubgroupSize::None),
            },
            SavedReport::Attribute {
                request: AttributeChartRequest::new(AttributeChartKind::T, 6, SubgroupSize::None),
            },
            SavedReport::ControlChart {
                request: ControlChartRequest {
                    span: 4,
                    phase: None,
                    ..ControlChartRequest::new(2, ChartKind::MovingAverage, 1)
                },
                options: ControlChartOptions::default(),
            },
            SavedReport::ControlChart {
                request: ControlChartRequest::new(2, ChartKind::ThreeWayStdDev, 4),
                options: ControlChartOptions { within_rules: RuleSet::ALL, ..ControlChartOptions::default() },
            },
            SavedReport::ControlChart {
                request: ControlChartRequest::new(2, ChartKind::Zone, 1),
                options: ControlChartOptions::default(),
            },
            SavedReport::Pareto {
                request: crate::ParetoRequest { category: 1, frequency: Some(6), group: Some(2) },
                options: crate::ParetoChartOptions { other_percent: 7.5, percent_scale: true },
            },
            SavedReport::Pareto {
                request: crate::ParetoRequest { category: 3, frequency: None, group: None },
                options: crate::ParetoChartOptions::default(),
            },
            SavedReport::CauseEffect {
                request: crate::CauseEffectRequest { causes: vec![3, 1, 2], effect: "Employment".to_owned() },
            },
            SavedReport::MultiVari {
                request: crate::MultiVariRequest { response: 6, factors: vec![3, 1] },
                options: crate::MultiVariOptions {
                    show_values: false,
                    show_ranges: true,
                    show_cells: true,
                    replicates: crate::ReplicateLayout::Stacked,
                },
            },
            SavedReport::MultivariateChart {
                request: crate::MultivariateChartRequest {
                    alpha: 0.01,
                    ..crate::MultivariateChartRequest::new(vec![0, 1, 2], crate::MultivariateChartKind::HotellingT2)
                },
            },
            SavedReport::MultivariateChart {
                request: crate::MultivariateChartRequest {
                    reference: Some(8),
                    label: Some(6),
                    lambda: 0.25,
                    mewma_limit: crate::MewmaLimit::Value(12.0),
                    ..crate::MultivariateChartRequest::new(vec![0, 2], crate::MultivariateChartKind::Mewma)
                },
            },
            SavedReport::MultivariateChart {
                request: crate::MultivariateChartRequest {
                    subgroup_size: 4,
                    ..crate::MultivariateChartRequest::new(
                        vec![0, 1],
                        crate::MultivariateChartKind::GeneralizedVariance,
                    )
                },
            },
            SavedReport::Anom {
                request: crate::AnomRequest {
                    kind: crate::AnomKind::Rates,
                    response: 4,
                    size: SubgroupSize::Column(5),
                    group: None,
                    second: None,
                    alpha: 0.01,
                },
            },
            SavedReport::DoseResponse {
                request: crate::DoseResponseRequest {
                    link: crate::Link::ComplementaryLogLog,
                    scale: crate::DoseScale::Ln,
                    probabilities: vec![0.25, 0.5],
                    confidence_level: 0.9,
                    ..crate::DoseResponseRequest::probit(6, 5, Some(2))
                },
            },
            SavedReport::Scatter3d {
                request: crate::Scatter3dRequest {
                    color: Some(3),
                    size: Some(4),
                    ..crate::Scatter3dRequest::new(0, 1, 2)
                },
                options: crate::Scatter3dOptions {
                    drop_lines: true,
                    camera: crate::Camera { azimuth: 70.0, elevation: 15.0 },
                },
            },
            SavedReport::DataSurface {
                request: crate::DataSurfaceRequest {
                    method: crate::SurfaceFit::Spline,
                    grid: 20,
                    ..crate::DataSurfaceRequest::new(1, 2, 3)
                },
                options: crate::DataSurfaceOptions { levels: 5, filled: false, ..crate::DataSurfaceOptions::default() },
            },
            SavedReport::MethodComparison {
                request: crate::MethodComparisonRequest {
                    passing_bablok: false,
                    variance_ratio: 2.0,
                    ..crate::MethodComparisonRequest::new(1, 2)
                },
            },
            SavedReport::ShelfLife {
                request: crate::ShelfLifeRequest {
                    upper: Some(80000.0),
                    horizon: Some(4000.0),
                    time_unit: Some(stat_linear::StabilityTimeUnit::Weeks),
                    ..crate::ShelfLifeRequest::lower(6, 0, None, 50000.0)
                },
            },
            SavedReport::Mixture {
                request: MixtureRequest { response: 6, components: vec![1, 2, 3], model: ScheffeModel::Quadratic },
                profilers: ProfilerOptions { show_profiler: false, show_surface_profiler: false, show_observed: false },
            },
            SavedReport::Graph {
                spec: Box::new(GraphSpec {
                    encoding: stat_viz::EncodingPlan {
                        x: vec![table.column(1).unwrap().name.clone()],
                        y: vec![table.column(0).unwrap().name.clone(), table.column(2).unwrap().name.clone()],
                        color: Some(table.column(3).unwrap().name.clone()),
                        page: Some(table.column(3).unwrap().name.clone()),
                        weight: Some(table.column(0).unwrap().name.clone()),
                        label: Some(table.column(3).unwrap().name.clone()),
                        y2: vec![table.column(2).unwrap().name.clone()],
                        ..Default::default()
                    },
                    options: stat_viz::FrameOptions {
                        x_scale: stat_viz::AxisScale::Log10,
                        y_scale: stat_viz::AxisScale::Reciprocal,
                        page: 1,
                        filter: stat_viz::LocalFilter {
                            conditions: vec![
                                stat_viz::FilterCondition::Range {
                                    column: table.column(0).unwrap().name.clone(),
                                    min: -1.5,
                                    max: 1e6,
                                },
                                stat_viz::FilterCondition::Levels {
                                    column: table.column(3).unwrap().name.clone(),
                                    levels: vec!["a".into(), "b c".into()],
                                },
                            ],
                            show_only: false,
                            include_only: true,
                            select: true,
                        },
                        style: stat_viz::MarkerStyle {
                            size: Some(5.0),
                            color: Some(stat_viz::Rgba::rgb(40, 90, 160)),
                            shape: Some(stat_viz::MarkShape::Diamond),
                            level_colors: [(stat_viz::MarkerStyle::key("g", "a"), stat_viz::Rgba::rgb(200, 0, 0))]
                                .into(),
                            level_shapes: [(stat_viz::MarkerStyle::key("g", "a"), stat_viz::MarkShape::Plus)].into(),
                            level_sizes: [(stat_viz::MarkerStyle::key("g", "a"), 8.0)].into(),
                            panels: [(
                                1,
                                stat_viz::MarkerStyle { shape: Some(stat_viz::MarkShape::Cross), ..Default::default() },
                            )]
                            .into(),
                        },
                    },
                    switchers: vec![stat_viz::ColumnSwitcher {
                        role: stat_viz::Role::Y,
                        slot: 1,
                        choices: vec![table.column(2).unwrap().name.clone(), table.column(1).unwrap().name.clone()],
                    }],
                    points: Some(stat_viz::PointsElement {
                        jitter: stat_viz::JitterKind::Normal,
                        jitter_width: 0.4,
                        fade_unselected: false,
                        label_selected_only: true,
                    }),
                    smoother: Some(stat_viz::SmootherElement {
                        method: stat_viz::SmoothMethod::Loess,
                        lambda: 0.05,
                        span: 0.4,
                        local_degree: 1,
                        band: true,
                        level: 0.9,
                    }),
                    fit: Some(stat_viz::FitLineElement {
                        degree: 2,
                        band: false,
                        prediction: true,
                        level: 0.9,
                        labels: stat_viz::FitLabels { equation: true, r_squared: false, p_value: true },
                    }),
                    box_plot: Some(stat_viz::BoxPlotElement { outliers: false, notches: true }),
                    violin: Some(stat_viz::ViolinElement { bandwidth_scale: 1.5, quartiles: false }),
                    summary: Some(stat_viz::SummaryElement {
                        stat: stat_viz::summary::SummaryStat::Percentile(0.25),
                        error_bars: stat_viz::summary::ErrorBars::StdDev,
                        level: 0.99,
                        error_look: stat_viz::summary::ErrorBarLook {
                            color: Some(stat_viz::Rgba::rgb(10, 20, 30)),
                            width: 2.5,
                            cap: 0.5,
                            direction: stat_viz::summary::ErrorBarDirection::Lower,
                        },
                    }),
                    ellipse: Some(stat_viz::EllipseElement { levels: vec![0.5, 0.99], correlation: false }),
                    contour: Some(stat_viz::ContourElement {
                        fractions: vec![0.3],
                        bandwidth_scale: 0.7,
                        filled: true,
                    }),
                    bins: Some(stat_viz::BinElement { hexagonal: false, bins: 9 }),
                    function: Some(stat_viz::FunctionElement { formulas: vec!["2*x".into(), "sqrt(x)".into()] }),
                    bars: Some(stat_viz::BarElement {
                        stat: stat_viz::summary::SummaryStat::Sum,
                        layout: stat_viz::BarLayout::Stacked,
                        error_bars: stat_viz::summary::ErrorBars::StdErr,
                        level: 0.9,
                        labels: true,
                        error_look: stat_viz::summary::ErrorBarLook {
                            direction: stat_viz::summary::ErrorBarDirection::Upper,
                            ..Default::default()
                        },
                    }),
                    line: Some(stat_viz::LineElement {
                        connect: stat_viz::LineConnect::Summary,
                        stat: stat_viz::summary::SummaryStat::Median,
                    }),
                    heatmap: Some(stat_viz::HeatmapElement {
                        stat: stat_viz::summary::SummaryStat::Max,
                        labels: false,
                    }),
                    histogram: Some(stat_viz::HistogramElement {
                        bins: stat_viz::HistogramBins::Width(2.5),
                        stacked: true,
                        density: true,
                    }),
                    dot_plot: Some(stat_viz::DotPlotElement { width: Some(0.4) }),
                    probability_plot: Some(stat_viz::ProbabilityPlotElement {
                        family: FitFamily::Weibull3,
                        band: false,
                        level: 0.9,
                    }),
                    form: stat_viz::GraphForm::Panels,
                    legend: stat_viz::LegendPlacement::Right,
                    label_positions: vec![stat_viz::LabelPosition { panel: 0, id: "fit/0".into(), at: [0.1, 0.9] }],
                    axes: stat_viz::GraphAxes {
                        title: Some("Growth of $dN/dt$".into()),
                        y2_title: Some("\\mu g".into()),
                        y_breaks: stat_viz::AxisBreaks::single(10.0, 100.0),
                        style: Some({
                            let mut style = crate::plot_style::PlotStyle::default();
                            style.axes.x.format = stat_viz::axis_settings::TickFormat::Decimals(2);
                            style.annotations.items.push(stat_viz::Annotation {
                                shape: stat_viz::AnnotationShape::Arrow,
                                anchor: stat_viz::AnnotationAnchor::Data,
                                panel: 0,
                                at: [1.0, 2.0],
                                to: [3.0, 4.5],
                                text: "outlier".into(),
                            });
                            style.key.renamed.push(("fit/Fit".into(), "Linear fit".into()));
                            style
                        }),
                        ..Default::default()
                    },
                    hull: Some(stat_viz::HullElement { fill: false }),
                    brackets: Some(stat_viz::BracketElement {
                        test: stat_viz::BracketTest::GamesHowell,
                        label: stat_viz::BracketLabel::PValue,
                        only_significant: true,
                        alpha: 0.01,
                        pairs: vec![("a".into(), "b".into())],
                        thresholds: [0.05, 0.02, 0.005, 0.0005],
                        ns_label: true,
                        gap: 0.1,
                    }),
                    estimation: stat_viz::EstimationSettings {
                        interval: stat_viz::EstimationInterval::Welch,
                        resamples: 1000,
                        seed: 7,
                        level: 0.9,
                        reference: Some("b".into()),
                    },
                    layer_order: vec![stat_viz::ElementKind::Hull, stat_viz::ElementKind::Points],
                }),
            },
            SavedReport::Stepwise {
                request: StepwiseRequest {
                    candidates: LinearModelSpec {
                        response: 6,
                        predictors: vec![1, 2, 3, 4],
                        effects: Vec::new(),
                        splines: Vec::new(),
                    },
                    direction: StepDirection::Mixed,
                    criterion: StepCriterion::PValue { enter: 0.1, remove: 0.2 },
                    heredity: stat_linear::Heredity::Weak,
                },
                options: ProfilerOptions { show_profiler: true, show_surface_profiler: true, show_observed: true },
            },
            SavedReport::Stepwise {
                request: StepwiseRequest {
                    candidates: LinearModelSpec {
                        response: 6,
                        predictors: vec![1, 2],
                        effects: vec![vec![(0, 1)], vec![(1, 1)], vec![(0, 1), (1, 1)]],
                        splines: Vec::new(),
                    },
                    direction: StepDirection::Backward,
                    criterion: StepCriterion::Bic,
                    heredity: stat_linear::Heredity::Strong,
                },
                options: ProfilerOptions::default(),
            },
            SavedReport::BestSubsets {
                request: BestSubsetsRequest {
                    candidates: LinearModelSpec {
                        response: 6,
                        predictors: vec![1, 2],
                        effects: vec![vec![(0, 1)], vec![(1, 1)], vec![(0, 1), (1, 1)]],
                        splines: Vec::new(),
                    },
                    forced: vec![vec![(1, 1)]],
                    heredity: stat_linear::Heredity::Weak,
                    models_per_size: 2,
                    max_size: Some(2),
                    criterion: SubsetCriterion::Cp,
                },
            },
            SavedReport::Attribute {
                request: AttributeChartRequest::new(AttributeChartKind::U, 6, SubgroupSize::Column(5)),
            },
            SavedReport::Attribute {
                request: AttributeChartRequest::new(AttributeChartKind::Np, 6, SubgroupSize::Constant(4000.0)),
            },
            SavedReport::DistributionFit { request: DistributionFitRequest::new(0) },
            SavedReport::DistributionFit {
                request: DistributionFitRequest {
                    column: 1,
                    families: vec![FitFamily::Weibull3, FitFamily::Normal],
                    transformations: false,
                    plots: 1,
                },
            },
            SavedReport::Capability {
                request: CapabilityRequest {
                    column: 0,
                    limits: SpecLimits { lower: Some(80.0), upper: Some(120.0), target: Some(100.0) },
                    within: WithinSigma::PooledStdDev { subgroup_size: 4 },
                    box_cox: None,
                    model: CapabilityModel::BestFit,
                    summary: true,
                    bootstrap: Some(crate::capability_analysis::BootstrapSettings { resamples: 300, seed: 42 }),
                    by: None,
                    group_order: crate::GroupOrder::Labels,
                    probability_plot: false,
                },
            },
            SavedReport::Capability {
                request: CapabilityRequest {
                    column: 1,
                    limits: SpecLimits { lower: None, upper: Some(500_000.0), target: None },
                    within: WithinSigma::MovingRange,
                    box_cox: Some(0.5),
                    model: CapabilityModel::Normal,
                    summary: false,
                    bootstrap: None,
                    by: Some(2),
                    group_order: crate::GroupOrder::Descending,
                    probability_plot: true,
                },
            },
            SavedReport::TimeWeighted {
                request: TimeWeightedRequest {
                    column: 2,
                    kind: TimeWeightedKind::Cusum { k: 0.75, h: 4.5 },
                    target: Some((3000.0, 400.0)),
                },
            },
            SavedReport::Surface {
                request: SurfaceRequest { response: 0, factors: vec![1, 2] },
                options: SurfaceOptions {
                    show_ridge: false,
                    contour_x: 1,
                    contour_y: 0,
                    show_surface_profiler: true,
                    show_surface_observed: true,
                    ..SurfaceOptions::default()
                },
            },
            SavedReport::Survival {
                request: SurvivalRequest {
                    time: 3,
                    status: None,
                    coding: StatusCoding::NonzeroIsCensored,
                    group: None,
                },
                options: SurvivalOptions { show_lognormal: true, show_table: true, ..SurvivalOptions::default() },
            },
            SavedReport::Survival {
                request: SurvivalRequest {
                    time: 1,
                    status: Some(0),
                    coding: StatusCoding::NonzeroIsEvent,
                    group: Some(table.n_cols() - 1),
                },
                options: SurvivalOptions { band: BandTransform::Plain, show_band: false, ..SurvivalOptions::default() },
            },
            SavedReport::OneSample {
                request: OneSampleRequest {
                    group: Some(6),
                    known_std_dev: Some(2.5),
                    null_std_dev: None,
                    equivalence: Some((-1.0, 2.0)),
                    alternative: Alternative::Greater,
                    level: 0.9,
                    ..OneSampleRequest::new(2)
                },
            },
            SavedReport::Groups {
                request: GroupComparisonRequest {
                    method: ComparisonMethod::GamesHowell,
                    equivalence: Some((-3.0, 3.0)),
                    control: Some("B".to_owned()),
                    median_ties: MedianTies::Ignore,
                    best: Some(stat_inference::best::BestDirection::Largest),
                    ..GroupComparisonRequest::new(GroupLayout::Columns(vec![1, 2, 3]))
                },
            },
            SavedReport::Groups {
                request: GroupComparisonRequest {
                    null_difference: 1.5,
                    alternative: Alternative::Less,
                    ..GroupComparisonRequest::new(GroupLayout::Stacked { response: 1, group: 6 })
                },
            },
            SavedReport::Paired { request: PairedRequest::new(PairedLayout::Columns(vec![2, 3])) },
            SavedReport::Paired {
                request: PairedRequest {
                    equivalence: Some((-0.5, 0.5)),
                    ratio_equivalence: true,
                    ..PairedRequest::new(PairedLayout::Stacked { response: 1, condition: 6, subject: 5 })
                },
            },
            SavedReport::Proportions {
                request: ProportionRequest::new(6, ProportionComparison::OneSample { null: 0.3 }),
            },
            SavedReport::Proportions {
                request: ProportionRequest {
                    event: Some("1950".into()),
                    ..ProportionRequest::new(6, ProportionComparison::Groups { group: 5 })
                },
            },
            SavedReport::Proportions { request: ProportionRequest::new(5, ProportionComparison::Paired { second: 6 }) },
            SavedReport::Rates {
                request: RateRequest { exposure: Some(2), group: Some(6), null_rate: 0.5, ..RateRequest::new(5) },
            },
            SavedReport::Power { request: PowerRequest::default() },
            SavedReport::Power {
                request: PowerRequest {
                    design: PowerDesign::EquivalenceTwoSample {
                        lower: -1.0,
                        upper: 1.0,
                        difference: 0.2,
                        std_dev: 2.0,
                        ratio: 1.5,
                    },
                    alpha: 0.1,
                    alternative: Alternative::TwoSided,
                    solve: PowerSolve::Power { n: 30 },
                },
            },
            SavedReport::Power {
                request: PowerRequest {
                    design: PowerDesign::OneProportion { null: 0.2, actual: 0.3 },
                    alpha: 0.05,
                    alternative: Alternative::Greater,
                    solve: PowerSolve::Effect { n: 150, target: 0.9 },
                },
            },
            SavedReport::AttributeCapability {
                request: crate::AttributeCapabilityRequest {
                    level: 0.9,
                    ..crate::AttributeCapabilityRequest::new(
                        crate::AttributeCapabilityKind::Binomial,
                        6,
                        SubgroupSize::Constant(4000.0),
                    )
                },
            },
            SavedReport::AttributeCapability {
                request: crate::AttributeCapabilityRequest::new(
                    crate::AttributeCapabilityKind::Poisson,
                    6,
                    SubgroupSize::Column(5),
                ),
            },
            SavedReport::BetweenWithinCapability {
                request: crate::BetweenWithinRequest {
                    column: 2,
                    subgroup: crate::SubgroupBy::Size(4),
                    limits: SpecLimits { lower: Some(200_000.0), upper: Some(600_000.0), target: None },
                    within: crate::WithinEstimate::AverageRange,
                    between: crate::BetweenEstimate::MovingRange,
                },
            },
            SavedReport::BetweenWithinCapability {
                request: crate::BetweenWithinRequest {
                    column: 2,
                    subgroup: crate::SubgroupBy::Size(3),
                    limits: SpecLimits { lower: None, upper: Some(600_000.0), target: Some(400_000.0) },
                    within: crate::WithinEstimate::PooledStdDev,
                    between: crate::BetweenEstimate::VarianceComponents,
                },
            },
            SavedReport::AcceptanceSampling { request: crate::AcceptanceRequest::default() },
            SavedReport::AcceptanceSampling {
                request: crate::AcceptanceRequest {
                    design: crate::PlanDesign::Double { second_ratio: 1 },
                    model: crate::OcModel::Hypergeometric,
                    lot_size: 900,
                    plans: vec![crate::SamplingPlan::Single { n: 60, c: 1 }],
                    ..crate::AcceptanceRequest::default()
                },
            },
            SavedReport::GaugeLinearity {
                request: crate::LinearityRequest { process_variation: Some(6.0), ..crate::LinearityRequest::new(0, 2) },
            },
            SavedReport::GaugeRepeatability { request: crate::RepeatabilityRequest::new(2, 400_000.0, 100_000.0) },
            SavedReport::IntraclassCorrelation { request: crate::IccRequest::new(0, 1, 2) },
            SavedReport::AttributeAgreement {
                request: crate::AgreementRequest {
                    trial: Some(3),
                    standard: Some(4),
                    confidence: 0.9,
                    ordinal: true,
                    order: vec!["b".to_owned(), "a".to_owned()],
                    ..crate::AgreementRequest::new(table, 0, 1, 2)
                },
            },
            SavedReport::AttributeGauge { request: crate::AttributeGaugeRequest::counts(0, 1, 2, 12.5) },
            SavedReport::AttributeGauge {
                request: crate::AttributeGaugeRequest {
                    checks: None,
                    accept_label: "1".to_owned(),
                    ..crate::AttributeGaugeRequest::counts(0, 1, 2, -1.0)
                },
            },
            SavedReport::MultiwayTable {
                request: crate::MultiwayTableRequest {
                    measures: vec![2, 3],
                    frequencies: crate::CellFrequency::ALL.to_vec(),
                    statistics: crate::CellSummary::ALL.to_vec(),
                    frequency: Some(4),
                    subtotals: true,
                    ..crate::MultiwayTableRequest::new(vec![0, 5], vec![1])
                },
            },
            SavedReport::StratifiedTables {
                request: crate::StratifiedRequest {
                    frequency: Some(3),
                    confidence: 0.9,
                    exact: false,
                    monte_carlo_tables: 500,
                    seed: 7,
                    fdr: Some(crate::FdrMethod::Storey),
                    ..crate::StratifiedRequest::new(0, 1, vec![2])
                },
            },
            SavedReport::SignalToNoise {
                request: crate::SignalToNoiseRequest {
                    factors: vec![0],
                    responses: vec![2, 3],
                    ratio: stat_doe::SignalToNoise::LargerTheBetter,
                },
            },
            SavedReport::DeclaredDesign {
                request: crate::DesignDeclarationRequest {
                    factors: vec![crate::DeclaredFactor { column: 1, low: Some(80.0), high: None }],
                    responses: vec![6],
                    whole_plot: None,
                    subplot: None,
                    model: String::new(),
                },
            },
            SavedReport::DesignEvaluation {
                request: crate::DesignEvaluationRequest {
                    model: "main effects".to_owned(),
                    alias_terms: String::new(),
                    coefficients: vec![1.5],
                    alpha: 0.1,
                    ..crate::DesignEvaluationRequest::new(&[1, 2])
                },
            },
            SavedReport::FactorialAnalysis {
                request: crate::FactorialAnalysisRequest {
                    factors: vec![
                        crate::DeclaredFactor { column: 1, low: Some(80.0), high: None },
                        crate::DeclaredFactor { column: 2, low: None, high: None },
                    ],
                    response: 6,
                    terms: crate::FactorialTerms::Formula("GNP deflator + GNP".into()),
                    response_type: crate::FactorialResponse::Count,
                    alpha: 0.1,
                    means: crate::MeansSource::Model,
                    cube: vec![2, 1],
                },
            },
            SavedReport::Power {
                request: PowerRequest {
                    design: PowerDesign::TwoLevelFactorial {
                        factors: 5,
                        fraction: 1,
                        center_points: 3,
                        interactions: true,
                        effect: 1.5,
                        std_dev: 1.0,
                    },
                    solve: PowerSolve::Power { n: 2 },
                    ..PowerRequest::default()
                },
            },
            SavedReport::Power {
                request: PowerRequest {
                    design: PowerDesign::ToleranceInterval {
                        proportion: 0.9,
                        confidence: 0.95,
                        max_proportion: 0.97,
                        two_sided: false,
                    },
                    ..PowerRequest::default()
                },
            },
            SavedReport::Power {
                request: PowerRequest {
                    design: PowerDesign::DistributionFreeTolerance { proportion: 0.95, removed: 3, two_sided: true },
                    solve: PowerSolve::SampleSize { target: 0.9 },
                    ..PowerRequest::default()
                },
            },
            SavedReport::Power {
                request: PowerRequest {
                    design: PowerDesign::TwoRates { first: 3.0, second: 2.0, exposure: 2.0, ratio: 1.5 },
                    alternative: Alternative::Greater,
                    ..PowerRequest::default()
                },
            },
            SavedReport::GoodnessOfFit { request: crate::GoodnessRequest::new(6) },
            SavedReport::GoodnessOfFit {
                request: crate::GoodnessRequest {
                    column: 2,
                    frequency: Some(3),
                    model: crate::GoodnessModel::Poisson { min_expected: 3.0 },
                },
            },
            SavedReport::Crossover {
                request: crate::CrossoverRequest {
                    reference: Some("B".to_owned()),
                    ratio: false,
                    bounds: (-2.0, 2.0),
                    ..crate::CrossoverRequest::new(1, 2, 3, 4)
                },
            },
            SavedReport::FactorialRepeated {
                request: crate::FactorialRepeatedRequest {
                    response: 1,
                    subject: 2,
                    within: vec![3, 4],
                    between: vec![5],
                    simple: Some((3, 5)),
                },
            },
            SavedReport::Nested { request: crate::NestedRequest { level: 0.9, ..crate::NestedRequest::new(1, 2, 3) } },
            SavedReport::Bayesian {
                request: crate::BayesRequest::Estimation(crate::BayesEstimationRequest {
                    second: Some(2),
                    prior: crate::BayesPrior::Informative { mean: 1.0, spread: 2.0 },
                    rope: Some((-1.0, 1.0)),
                    ..crate::BayesEstimationRequest::new(crate::EstimatedParameter::Mean, 1)
                }),
            },
            SavedReport::Bayesian {
                request: crate::BayesRequest::Estimation(crate::BayesEstimationRequest {
                    denominator: Some(2),
                    ..crate::BayesEstimationRequest::new(crate::EstimatedParameter::Rate, 1)
                }),
            },
            SavedReport::Bayesian {
                request: crate::BayesRequest::Factor(crate::BayesFactorRequest {
                    direction: stat_bayes::bayes_factor::Direction::Less,
                    ..crate::BayesFactorRequest::new(crate::BayesTestKind::Correlation { x: 1, y: 2 })
                }),
            },
            SavedReport::Bayesian {
                request: crate::BayesRequest::Factor(crate::BayesFactorRequest::new(crate::BayesTestKind::OneSample {
                    column: 3,
                    null: 2.5,
                })),
            },
            SavedReport::Bayesian {
                request: crate::BayesRequest::Linear(crate::BayesLinearRequest {
                    effects: vec![vec![(0, 1)], vec![(1, 1)], vec![(0, 1), (1, 1)]],
                    model_prior: stat_bayes::linear::ModelPrior::BetaBinomial,
                    ..crate::BayesLinearRequest::new(1, vec![2, 3])
                }),
            },
            SavedReport::Bayesian {
                request: crate::BayesRequest::Model(Box::new(crate::BayesModelRequest {
                    warmup: 150,
                    draws: 150,
                    priors: stat_bayes::glm::Priors {
                        coefficient: stat_bayes::glm::CoefficientPrior::StudentT(4.0),
                        sd: stat_bayes::glm::ScalePrior::HalfT3,
                        ..stat_bayes::glm::Priors::default()
                    },
                    ..crate::BayesModelRequest::new(1, vec![2], stat_bayes::glm::GlmFamily::Normal)
                })),
            },
            SavedReport::Signal {
                request: crate::SignalRequest::Smoothing(crate::SignalSmoothingRequest {
                    x: Some(0),
                    method: stat_signal::Smoother::Gaussian { sigma: 1.5 },
                    ..crate::SignalSmoothingRequest::new(1)
                }),
            },
            SavedReport::Signal {
                request: crate::SignalRequest::Calculus(crate::CalculusRequest {
                    resample: Some(crate::ResampleSettings { method: stat_signal::Interpolation::Pchip, points: 5 }),
                    ..crate::CalculusRequest::new(0, 1)
                }),
            },
            SavedReport::Signal {
                request: crate::SignalRequest::Spectrum(crate::SpectrumRequest {
                    fit: Some(crate::PeakFitSettings { count: Some(1), ..Default::default() }),
                    ..crate::SpectrumRequest::new(0, 1)
                }),
            },
            SavedReport::DistributionCalculator {
                request: crate::CalculatorRequest {
                    region: crate::ProbabilityRegion::Outside,
                    values: (0.5, 2.5),
                    probability: 0.1,
                    ..crate::CalculatorRequest::new(stat_core::catalog::Family::Weibull)
                },
            },
            SavedReport::Bootstrap {
                request: crate::BootstrapRequest {
                    rows: vec!["Mean".to_owned(), "Standard deviation".to_owned()],
                    replicates: 40,
                    seed: 9,
                    level: 0.9,
                    stratify: Some(0),
                    ..crate::BootstrapRequest::new(
                        SavedReport::Distribution {
                            request: DistributionRequest {
                                column: 1,
                                quantile_method: Default::default(),
                                censoring: None,
                            },
                            options: DistributionOptions::default(),
                        },
                        "Summary Statistics",
                        vec!["Value".to_owned()],
                    )
                },
            },
            SavedReport::PermutationTest {
                request: crate::PermutationRequest {
                    alternative: Alternative::Greater,
                    resamples: 200,
                    seed: 3,
                    exact_limit: 100,
                    ..crate::PermutationRequest::new(crate::PermutationDesign::Correlation {
                        x: 1,
                        y: 2,
                        statistic: stat_inference::permutation::CorrelationStatistic::Spearman,
                    })
                },
            },
            SavedReport::PermutationTest {
                request: crate::PermutationRequest::new(crate::PermutationDesign::Paired { first: 1, second: 2 }),
            },
            SavedReport::ExponentialSmoothing {
                request: crate::SmoothingRequest {
                    model: crate::SmoothingModel::HoltLinear,
                    ..crate::SmoothingRequest::new(table.n_cols() - 1)
                },
            },
            SavedReport::Arima {
                request: crate::ArimaRequest {
                    orders: crate::ArimaOrders::Chosen { regular: (0, 1, 1), seasonal: (0, 0, 0), constant: true },
                    ..crate::ArimaRequest::new(table.n_cols() - 1)
                },
            },
        ]
    }

    /// Every sample's reports (which use non-default settings of most
    /// report kinds) come back unchanged from a project file.
    #[test]
    fn every_sample_report_round_trips() {
        let mut failures = Vec::new();
        for info in crate::sample::catalog() {
            let sample = (info.build)();
            let rows = RowStateManager::new(sample.table.n_rows());
            let project = Project {
                external: None,
                origin: None,
                scripts: Vec::new(),
                reports: sample.reports.clone(),
                table: sample.table,
                rows,
            };
            let text = save_project(&project);
            let back = match load_project(&text) {
                Ok(b) => b,
                Err(e) => {
                    failures.push(format!("{}: {e}", info.name));
                    continue;
                }
            };
            for (a, b) in project.reports.iter().zip(&back.reports) {
                if a != b {
                    failures.push(format!("{}:\n  saved  {a:?}\n  loaded {b:?}", info.name));
                }
            }
            if back.reports.len() != project.reports.len() {
                failures.push(format!("{}: report count", info.name));
            }
            if save_project(&back) != text {
                failures.push(format!("{}: second save differs", info.name));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn round_trip_preserves_data_rows_and_reports() {
        let mut table = longley();
        table.set_level(0, MeasurementLevel::Ordinal).unwrap();
        let mut rows = RowStateManager::new(table.n_rows());
        rows.set_excluded([1, 5], true);
        rows.set_hidden([2], true);
        rows.select([3, 4, 9], stat_data::SelectionMode::Replace);
        rows.set_color([7], Some(RowColor { r: 0x12, g: 0xab, b: 0xff }));
        let project =
            Project { external: None, origin: None, scripts: Vec::new(), reports: every_report(&table), table, rows };

        let text = save_project(&project);
        let back = load_project(&text).unwrap();
        assert_eq!(back.table, project.table, "values, names, levels, table name");
        for r in 0..project.rows.len() {
            assert_eq!(back.rows.state(r), project.rows.state(r), "row {r}");
        }
        assert_eq!(back.reports, project.reports);
        // Saving again gives identical text.
        assert_eq!(save_project(&back), text);
    }

    /// Value labels, the hover choice, row markers and the column the row
    /// styles came from are saved only when set and read back exactly.
    #[test]
    fn value_labels_hover_and_row_styles_round_trip() {
        let mut table = longley();
        let labels: std::collections::BTreeMap<String, String> =
            [("1947".to_owned(), "First year".to_owned())].into_iter().collect();
        table.set_value_labels(0, labels.clone()).unwrap();
        table.set_hover(1, true).unwrap();
        let mut rows = RowStateManager::new(table.n_rows());
        let plain = save_project(&Project {
            external: None,
            origin: None,
            scripts: Vec::new(),
            reports: Vec::new(),
            table: longley(),
            rows: rows.clone(),
        });
        assert!(!plain.contains("value_labels") && !plain.contains("markers") && !plain.contains("\"hover\""));
        rows.set_marker([2, 4], Some(stat_data::RowMarker::Diamond));
        rows.set_style_column(Some(table.columns()[0].name.clone()));
        let project = Project { external: None, origin: None, scripts: Vec::new(), reports: Vec::new(), table, rows };
        let text = save_project(&project);
        let back = load_project(&text).unwrap();
        assert_eq!(back.table.columns()[0].value_labels, labels);
        assert!(back.table.columns()[1].hover && !back.table.columns()[0].hover);
        assert_eq!(back.rows.marker(4), Some(stat_data::RowMarker::Diamond));
        assert_eq!(back.rows.marker(3), None);
        assert_eq!(back.rows.style_column(), project.rows.style_column());
        assert_eq!(save_project(&back), text);
        assert!(load_project(&text.replace("\"diamond\"", "\"hexagon\"")).is_err(), "an unknown marker is refused");
    }

    #[test]
    fn linear_pairwise_method_saves_and_reads_older_files() {
        let table = longley();
        let rows = RowStateManager::new(table.n_rows());
        let linear = |pairwise| SavedReport::Linear {
            spec: LinearModelSpec { response: 1, predictors: vec![0], effects: Vec::new(), splines: Vec::new() },
            options: ReportOptions { pairwise, ..ReportOptions::default() },
        };
        let method = |text: &str| match &load_project(text).unwrap().reports[0] {
            SavedReport::Linear { options, .. } => options.pairwise,
            _ => panic!(),
        };
        for m in [None, Some(ComparisonMethod::TukeyKramer), Some(ComparisonMethod::HolmT)] {
            let text = save_project(&Project {
                external: None,
                origin: None,
                scripts: Vec::new(),
                reports: vec![linear(m)],
                table: table.clone(),
                rows: rows.clone(),
            });
            assert_eq!(method(&text), m);
        }
        // Files from before the choice carry show_games_howell (or nothing).
        let text = save_project(&Project {
            external: None,
            origin: None,
            scripts: Vec::new(),
            reports: vec![linear(None)],
            table,
            rows,
        });
        let key = text.find("\"pairwise\"").unwrap();
        let end = key + text[key..].find("\"none\"").unwrap() + "\"none\"".len();
        let legacy = |v: &str| format!("{}\"show_games_howell\": {v}{}", &text[..key], &text[end..]);
        assert_eq!(method(&legacy("true")), Some(ComparisonMethod::GamesHowell));
        assert_eq!(method(&legacy("false")), None);
        let none = format!("{}\"unused\": 0{}", &text[..key], &text[end..]);
        assert_eq!(method(&none), Some(ComparisonMethod::TukeyKramer), "default when absent");
    }

    #[test]
    fn quality_diagrams_read_with_default_options() {
        let table = longley();
        let name = |i: usize| table.column(i).unwrap().name.clone();
        let text = format!(
            r#"[{{"analysis": "pareto-chart", "category": "{a}"}},
                {{"analysis": "cause-and-effect", "causes": ["{a}", "{b}"]}},
                {{"analysis": "multi-vari", "response": "{c}", "factors": ["{a}"], "options": {{"show_cells": true}}}}]"#,
            a = name(1),
            b = name(2),
            c = name(6)
        );
        let v = stat_data::parse_json(&text).unwrap();
        let reports: Vec<SavedReport> =
            v.as_array().unwrap().iter().map(|r| report_from_json(r, &table).unwrap()).collect();
        assert_eq!(
            reports[0],
            SavedReport::Pareto {
                request: crate::ParetoRequest { category: 1, frequency: None, group: None },
                options: crate::ParetoChartOptions::default(),
            }
        );
        assert_eq!(
            reports[1],
            SavedReport::CauseEffect {
                request: crate::CauseEffectRequest { causes: vec![1, 2], effect: String::new() }
            }
        );
        let SavedReport::MultiVari { options, .. } = &reports[2] else { panic!() };
        assert_eq!(*options, crate::MultiVariOptions { show_cells: true, ..crate::MultiVariOptions::default() });
        let bad = stat_data::parse_json(&format!(
            r#"{{"analysis": "pareto-chart", "category": "{}", "options": {{"other_percent": 120}}}}"#,
            name(1)
        ))
        .unwrap();
        assert!(report_from_json(&bad, &table).is_err());
    }

    #[test]
    fn new_chart_and_capability_settings_round_trip_and_default() {
        let table = longley();
        let reports = [
            SavedReport::Attribute {
                request: AttributeChartRequest {
                    phase: Some(1),
                    rules: RuleSet::from_numbers([1, 2]),
                    ..AttributeChartRequest::new(AttributeChartKind::LaneyU, 2, SubgroupSize::Column(3))
                },
            },
            SavedReport::ControlChart {
                request: ControlChartRequest {
                    span: 7,
                    part: Some(1),
                    ..ControlChartRequest::new(2, ChartKind::ShortRun, 1)
                },
                options: ControlChartOptions { within_rules: RuleSet::ALL, ..ControlChartOptions::default() },
            },
            SavedReport::Capability {
                request: CapabilityRequest {
                    summary: true,
                    bootstrap: Some(crate::BootstrapSettings { resamples: 500, seed: 11 }),
                    model: CapabilityModel::Johnson,
                    ..CapabilityRequest::new(2, SpecLimits { lower: Some(1.0), upper: None, target: None })
                },
            },
        ];
        for r in &reports {
            let json = report_to_json(r, &table);
            assert_eq!(&report_from_json(&json, &table).unwrap(), r);
        }
        // Files written before these settings existed load with the defaults.
        let old =
            parse_json(r#"{"analysis": "attribute-chart", "chart": "p", "count": "GNP", "size": "Population 14+"}"#)
                .unwrap();
        let SavedReport::Attribute { request } = report_from_json(&old, &table).unwrap() else { panic!() };
        assert_eq!((request.phase, request.rules), (None, RuleSet::LIMITS_ONLY));
        let old = parse_json(r#"{"analysis": "process-capability", "column": "GNP", "upper": 600}"#).unwrap();
        let SavedReport::Capability { request } = report_from_json(&old, &table).unwrap() else { panic!() };
        assert!(!request.summary && request.bootstrap.is_none());
    }

    #[test]
    fn control_chart_roles_and_rules_round_trip() {
        let table = crate::sample::fill_weights();
        let report = SavedReport::ControlChart {
            request: ControlChartRequest {
                subgroup: Some(0),
                phase: Some(2),
                sigma_multiple: 2.5,
                range_span: 3,
                standard: Some(stat_quality::StandardValues { center: 10.5, sigma: 0.25 }),
                last: Some(crate::LastSubgroups { count: 20, limits_from_last: true }),
                ..ControlChartRequest::new(1, ChartKind::MeanAndRange, 5)
            },
            options: ControlChartOptions {
                location_rules: RuleSet::from_numbers([1, 5]),
                dispersion_rules: RuleSet::ALL,
                show_run_chart: true,
                run_center: crate::RunCenter::Mean,
                ..ControlChartOptions::default()
            },
        };
        let json = report_to_json(&report, &table);
        assert_eq!(report_from_json(&json, &table).unwrap(), report);
        // Older files: one switch for all rules on the dispersion chart, no role columns.
        let old = stat_data::parse_json(
            r#"{"analysis": "control-chart", "column": "Weight (g)", "chart": "mean-range", "subgroup_size": 5,
                "options": {"all_rules_on_dispersion": true}}"#,
        )
        .unwrap();
        let SavedReport::ControlChart { request, options } = report_from_json(&old, &table).unwrap() else { panic!() };
        assert_eq!(request, ControlChartRequest::new(1, ChartKind::MeanAndRange, 5));
        assert_eq!(options.dispersion_rules, RuleSet::ALL);
        assert_eq!(options.location_rules, RuleSet::ALL);
    }

    #[test]
    fn reloaded_reports_reproduce_identical_results() {
        // A missing value and exclusions must survive and affect the refit identically.
        let mut columns = longley().columns().to_vec();
        if let ColumnData::Numeric(v) = &mut columns[3].data {
            v[2] = f64::NAN;
        }
        let table = DataTable::new("with missing", columns).unwrap();
        let mut rows = RowStateManager::new(table.n_rows());
        rows.set_excluded([0, 3], true);
        let project =
            Project { external: None, origin: None, scripts: Vec::new(), reports: every_report(&table), table, rows };
        let back = load_project(&save_project(&project)).unwrap();
        for (a, b) in project.reports.iter().zip(&back.reports) {
            let sections = |r: &SavedReport, p: &Project| match r {
                SavedReport::Pareto { .. }
                | SavedReport::CauseEffect { .. }
                | SavedReport::MultiVari { .. }
                | SavedReport::Scoped { .. }
                | SavedReport::AdjustPValues { .. }
                | SavedReport::AreaUnderCurve { .. }
                | SavedReport::ManyTests { .. }
                | SavedReport::TreatmentEffects { .. }
                | SavedReport::Screening { .. } => crate::run_report(r, &p.table, &p.rows).unwrap().sections,
                SavedReport::Bootstrap { .. } | SavedReport::PermutationTest { .. } => {
                    crate::run_report(r, &p.table, &p.rows).unwrap().sections
                }
                // The test table's exclusions leave a gap in the series, so these fail the same way twice.
                SavedReport::ExponentialSmoothing { .. } | SavedReport::Arima { .. } => {
                    crate::run_report(r, &p.table, &p.rows).map(|c| c.sections).unwrap_or_default()
                }
                SavedReport::PrincipalCoordinates { .. }
                | SavedReport::Pharmacokinetics { .. }
                | SavedReport::Fatigue { .. } => crate::run_report(r, &p.table, &p.rows).unwrap().sections,
                SavedReport::Linear { spec, options } => {
                    LinearModelAnalysis::run(&p.table, &p.rows, spec).unwrap().sections(options)
                }
                SavedReport::ControlChart { request, options } => {
                    ControlChartAnalysis::run(&p.table, &p.rows, request).unwrap().sections(options)
                }
                SavedReport::Gauge { request, options } => {
                    GaugeAnalysis::run(&p.table, &p.rows, request).unwrap().sections(options)
                }
                SavedReport::Distribution { request, options } => {
                    DistributionAnalysis::run(&p.table, &p.rows, request).unwrap().sections(options)
                }
                SavedReport::Bivariate { request } => {
                    BivariateAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::Pca { request } => crate::PcaAnalysis::run(&p.table, &p.rows, request).unwrap().sections(),
                SavedReport::Correlations { request } => {
                    crate::CorrelationAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::OutlierDistances { request } => {
                    crate::OutlierAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::ItemAnalysis { request } => {
                    crate::ItemAnalysisReport::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::Manova { request } => {
                    crate::ManovaAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::FactorAnalysis { request } => crate::FactorAnalysisReport::run(&p.table, &p.rows, request)
                    .map(|a| a.sections())
                    .unwrap_or_default(),
                SavedReport::DiscriminantAnalysis { request } => {
                    crate::DiscriminantAnalysis::run(&p.table, &p.rows, request)
                        .map(|a| a.sections())
                        .unwrap_or_default()
                }
                SavedReport::GaussianMixture { request } => {
                    crate::GaussianMixtureAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                // Numbers make no terms, so the text reports fail the same way twice.
                SavedReport::TextExploration { request } => {
                    crate::TextExploration::run(&p.table, &p.rows, request).map(|a| a.sections()).unwrap_or_default()
                }
                SavedReport::TopicModel { request } => {
                    crate::TopicModelAnalysis::run(&p.table, &p.rows, request).map(|a| a.sections()).unwrap_or_default()
                }
                SavedReport::Sentiment { request } => {
                    crate::SentimentAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                // The test table's columns are continuous, so this report fails the same way twice.
                SavedReport::LatentClass { request } => crate::LatentClassAnalysis::run(&p.table, &p.rows, request)
                    .map(|a| a.sections())
                    .unwrap_or_default(),
                SavedReport::Embedding { request } => {
                    crate::EmbeddingAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::Imputation { request } => {
                    crate::ImputationReport::run(&p.table, &p.rows, request).unwrap().sections()
                }
                // The excluded interior row is a gap in a time series (an error, the same
                // after reloading); `series_explore` checks reloading on complete series.
                SavedReport::SeriesExplore { request } => crate::SeriesExploreAnalysis::run(&p.table, &p.rows, request)
                    .map(|a| a.sections())
                    .unwrap_or_default(),
                SavedReport::SignalProcessing { request } => {
                    crate::SignalAnalysis::run(&p.table, &p.rows, request).map(|a| a.sections()).unwrap_or_default()
                }
                SavedReport::ImageAnalysis { request } => {
                    crate::ImageAnalysis::run(&p.table, &p.rows, request).map(|a| a.sections()).unwrap_or_default()
                }
                SavedReport::Correspondence { request } => {
                    crate::CorrespondenceReport::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::Cluster { request } => {
                    crate::ClusterAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::Glm { request, .. } => {
                    crate::GlmAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::Regularized { request, .. } => {
                    crate::RegularizedAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::DecisionTree { request, .. } => {
                    crate::TreeAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::RandomForest { request, .. } => {
                    crate::ForestAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::BoostedTrees { request, .. } => {
                    crate::BoostAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::PartialLeastSquares { request, .. } => {
                    crate::PlsAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::SupportVectorMachine { request, .. } => {
                    crate::SvmAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::Mars { request, .. } => {
                    crate::MarsAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::NeuralNetwork { request, .. } => {
                    crate::NeuralAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::GaussianProcess { request, .. } => {
                    crate::GaussianProcessAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::SequentialOptimization { request } => {
                    crate::SequentialAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::ModelSearch { request } => {
                    crate::ModelSearch::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::FunctionalData { request, .. } => {
                    crate::FunctionalAnalysis::run(&p.table, &p.rows, request).map(|a| a.sections()).unwrap_or_default()
                }
                SavedReport::ModelComparison { request } => {
                    crate::ModelComparison::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::MultipleResponses { request } => {
                    crate::MultipleResponsesAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::Mixed { request, .. } => {
                    crate::MixedAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::Survival { request, options } => {
                    crate::SurvivalAnalysis::run(&p.table, &p.rows, request).unwrap().sections(options)
                }
                SavedReport::Growth { request } => {
                    crate::GrowthAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::Recurrence { request } => {
                    crate::RecurrenceAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::LifeDistribution { request } => {
                    crate::LifeDistributionAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::CompetingRisks { request } => {
                    crate::CompetingRisksAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::Warranty { request } => {
                    crate::WarrantyAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::LifeStress { request } => {
                    crate::LifeStressAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::Degradation { request } => {
                    crate::DegradationAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::ProportionalHazards { request, .. } => {
                    crate::ProportionalHazardsAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::SurvivalRegression { request, .. } => {
                    crate::SurvivalRegressionAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::TestPlan { request } => crate::TestPlanAnalysis::run(request).unwrap().sections(),
                SavedReport::Surface { request, options } => {
                    crate::SurfaceAnalysis::run(&p.table, &p.rows, request).unwrap().sections(options)
                }
                SavedReport::TimeWeighted { request } => {
                    crate::TimeWeightedAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::Capability { request } => {
                    crate::CapabilityAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::Attribute { request } => {
                    crate::AttributeChartAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::MultivariateChart { request } => {
                    crate::MultivariateChartAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::Anom { request } => {
                    crate::AnomAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::DoseResponse { request } => {
                    crate::DoseResponseAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::CategoricalLogistic { request } => {
                    crate::CategoricalLogisticAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::Nonlinear { request, .. } => {
                    crate::NonlinearAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::MethodComparison { request } => {
                    crate::MethodComparisonAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::ShelfLife { request } => {
                    crate::ShelfLifeAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::Stepwise { request, .. } => {
                    crate::StepwiseAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::BestSubsets { request } => {
                    crate::BestSubsetsAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::DistributionFit { request } => {
                    crate::DistributionFitAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::Mixture { request, .. } => {
                    crate::MixtureAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::Scatter3d { request, .. } => {
                    crate::Scatter3dAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::DataSurface { request, options } => {
                    crate::DataSurfaceAnalysis::run(&p.table, &p.rows, request).unwrap().sections(options)
                }
                SavedReport::Graph { spec } => {
                    let frame = spec.frame(&p.table, &p.rows).unwrap();
                    let layers = spec.layers(&frame, &p.rows);
                    vec![crate::ReportSection {
                        kind: crate::SectionKind::FitStatistics,
                        title: format!("{frame:?} {layers:?}"),
                        content: crate::SectionContent::Note(String::new()),
                    }]
                }
                SavedReport::OneSample { request } => {
                    crate::OneSampleAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::Groups { request } => {
                    crate::GroupComparison::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::Paired { request } => {
                    crate::PairedComparison::run(&p.table, &p.rows, request).map(|a| a.sections()).unwrap_or_default()
                }
                SavedReport::Proportions { request } => {
                    crate::ProportionAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::Rates { request } => {
                    crate::RateAnalysis::run(&p.table, &p.rows, request).map(|a| a.sections()).unwrap_or_default()
                }
                SavedReport::Power { request } => crate::PowerAnalysis::run(request).unwrap().sections(),
                SavedReport::AttributeCapability { request } => {
                    crate::AttributeCapabilityAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::BetweenWithinCapability { request } => {
                    crate::BetweenWithinAnalysis::run(&p.table, &p.rows, request).unwrap().sections()
                }
                SavedReport::AcceptanceSampling { request } => {
                    crate::AcceptanceAnalysis::run(request).unwrap().sections()
                }
                SavedReport::GaugeLinearity { .. }
                | SavedReport::GaugeRepeatability { .. }
                | SavedReport::IntraclassCorrelation { .. }
                | SavedReport::AttributeAgreement { .. }
                | SavedReport::AttributeGauge { .. }
                | SavedReport::PcaChart { .. }
                | SavedReport::MonitoringLimits { .. }
                | SavedReport::MultiwayTable { .. }
                | SavedReport::StratifiedTables { .. }
                | SavedReport::SignalToNoise { .. }
                | SavedReport::DeclaredDesign { .. }
                | SavedReport::DesignEvaluation { .. }
                | SavedReport::FactorialAnalysis { .. }
                | SavedReport::Bayesian { .. }
                | SavedReport::MultipleResponseItems { .. }
                | SavedReport::Signal { .. }
                | SavedReport::ImagePlot { .. } => crate::ComparisonReport::run(r, &p.table, &p.rows)
                    .and_then(Result::ok)
                    .map(|a| a.sections())
                    .unwrap_or_default(),
                SavedReport::GoodnessOfFit { request } => {
                    crate::GoodnessAnalysis::run(&p.table, &p.rows, request).map(|a| a.sections()).unwrap_or_default()
                }
                SavedReport::Crossover { request } => {
                    crate::CrossoverReport::run(&p.table, &p.rows, request).map(|a| a.sections()).unwrap_or_default()
                }
                SavedReport::FactorialRepeated { request } => {
                    crate::FactorialRepeatedReport::run(&p.table, &p.rows, request)
                        .map(|a| a.sections())
                        .unwrap_or_default()
                }
                SavedReport::Nested { request } => {
                    crate::NestedReport::run(&p.table, &p.rows, request).map(|a| a.sections()).unwrap_or_default()
                }
                SavedReport::DistributionCalculator { request } => {
                    crate::CalculatorReport::run(request).unwrap().sections()
                }
            };
            assert_eq!(format!("{:?}", sections(a, &project)), format!("{:?}", sections(b, &back)));
        }
    }

    /// Every report type on degenerate data (one or two rows, constant
    /// columns, an empty column, every row excluded, extreme or non-positive
    /// values) either reports an error or produces sections; it never panics.
    #[test]
    fn degenerate_data_never_panics() {
        fn run_any(r: &SavedReport, t: &DataTable, rows: &RowStateManager) -> Option<Vec<crate::ReportSection>> {
            match r {
                SavedReport::Pareto { .. }
                | SavedReport::CauseEffect { .. }
                | SavedReport::MultiVari { .. }
                | SavedReport::Scoped { .. }
                | SavedReport::AdjustPValues { .. }
                | SavedReport::AreaUnderCurve { .. }
                | SavedReport::ManyTests { .. }
                | SavedReport::TreatmentEffects { .. }
                | SavedReport::Screening { .. } => crate::run_report(r, t, rows).ok().map(|c| c.sections),
                SavedReport::Bootstrap { .. } | SavedReport::PermutationTest { .. } => {
                    crate::run_report(r, t, rows).ok().map(|c| c.sections)
                }
                SavedReport::Linear { spec, options } => {
                    LinearModelAnalysis::run(t, rows, spec).ok().map(|a| a.sections(options))
                }
                SavedReport::ControlChart { request, options } => {
                    ControlChartAnalysis::run(t, rows, request).ok().map(|a| a.sections(options))
                }
                SavedReport::Gauge { request, options } => {
                    GaugeAnalysis::run(t, rows, request).ok().map(|a| a.sections(options))
                }
                SavedReport::Distribution { request, options } => {
                    let all = DistributionOptions {
                        show_quantile_plot: true,
                        show_box_cox: true,
                        tolerance: Some(ToleranceSettings::default()),
                        outliers: Some(OutlierSettings {
                            method: OutlierMethod::GeneralizedEsd,
                            ..OutlierSettings::default()
                        }),
                        runs: Some(RunsSettings::default()),
                        ..*options
                    };
                    DistributionAnalysis::run(t, rows, request).ok().map(|a| a.sections(&all))
                }
                SavedReport::Bivariate { request } => {
                    BivariateAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::Pca { request } => crate::PcaAnalysis::run(t, rows, request).ok().map(|a| a.sections()),
                SavedReport::Correlations { request } => {
                    crate::CorrelationAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::OutlierDistances { request } => {
                    crate::OutlierAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::ItemAnalysis { request } => {
                    crate::ItemAnalysisReport::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::Manova { request } => {
                    crate::ManovaAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::FactorAnalysis { request } => {
                    crate::FactorAnalysisReport::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::DiscriminantAnalysis { request } => {
                    crate::DiscriminantAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::GaussianMixture { request } => {
                    crate::GaussianMixtureAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::LatentClass { request } => {
                    crate::LatentClassAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::TextExploration { request } => {
                    crate::TextExploration::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::TopicModel { request } => {
                    crate::TopicModelAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::Sentiment { request } => {
                    crate::SentimentAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::Embedding { request } => {
                    crate::EmbeddingAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::Imputation { request } => {
                    crate::ImputationReport::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::SeriesExplore { request } => {
                    crate::SeriesExploreAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::SignalProcessing { request } => {
                    crate::SignalAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::ImageAnalysis { request } => {
                    crate::ImageAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::Correspondence { request } => {
                    crate::CorrespondenceReport::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::Cluster { request } => {
                    crate::ClusterAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::Glm { request, .. } => {
                    crate::GlmAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::Regularized { request, .. } => {
                    crate::RegularizedAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::DecisionTree { request, .. } => {
                    crate::TreeAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::RandomForest { request, .. } => {
                    crate::ForestAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::BoostedTrees { request, .. } => {
                    crate::BoostAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::PartialLeastSquares { request, .. } => {
                    crate::PlsAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::SupportVectorMachine { request, .. } => {
                    crate::SvmAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::Mars { request, .. } => {
                    crate::MarsAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::NeuralNetwork { request, .. } => {
                    crate::NeuralAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::GaussianProcess { request, .. } => {
                    crate::GaussianProcessAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::SequentialOptimization { request } => {
                    crate::SequentialAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::ModelSearch { request } => {
                    crate::ModelSearch::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::FunctionalData { request, .. } => {
                    crate::FunctionalAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::ModelComparison { request } => {
                    crate::ModelComparison::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::MultipleResponses { request } => {
                    crate::MultipleResponsesAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::Mixed { request, .. } => {
                    crate::MixedAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::Survival { request, options } => {
                    crate::SurvivalAnalysis::run(t, rows, request).ok().map(|a| a.sections(options))
                }
                SavedReport::Growth { request } => {
                    crate::GrowthAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::Recurrence { request } => {
                    crate::RecurrenceAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::LifeDistribution { request } => {
                    crate::LifeDistributionAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::CompetingRisks { request } => {
                    crate::CompetingRisksAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::Warranty { request } => {
                    crate::WarrantyAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::ExponentialSmoothing { .. }
                | SavedReport::Arima { .. }
                | SavedReport::PrincipalCoordinates { .. }
                | SavedReport::Pharmacokinetics { .. }
                | SavedReport::Fatigue { .. } => crate::run_report(r, t, rows).ok().map(|c| c.sections),
                SavedReport::LifeStress { request } => {
                    crate::LifeStressAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::Degradation { request } => {
                    crate::DegradationAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::ProportionalHazards { request, .. } => {
                    crate::ProportionalHazardsAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::SurvivalRegression { request, .. } => {
                    crate::SurvivalRegressionAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::TestPlan { request } => crate::TestPlanAnalysis::run(request).ok().map(|a| a.sections()),
                SavedReport::Surface { request, options } => {
                    crate::SurfaceAnalysis::run(t, rows, request).ok().map(|a| a.sections(options))
                }
                SavedReport::TimeWeighted { request } => {
                    crate::TimeWeightedAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::Capability { request } => {
                    crate::CapabilityAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::Attribute { request } => {
                    crate::AttributeChartAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::MultivariateChart { request } => {
                    crate::MultivariateChartAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::Anom { request } => crate::AnomAnalysis::run(t, rows, request).ok().map(|a| a.sections()),
                SavedReport::DoseResponse { request } => {
                    crate::DoseResponseAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::CategoricalLogistic { request } => {
                    crate::CategoricalLogisticAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::Nonlinear { request, .. } => {
                    crate::NonlinearAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::MethodComparison { request } => {
                    crate::MethodComparisonAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::ShelfLife { request } => {
                    crate::ShelfLifeAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::Stepwise { request, .. } => {
                    crate::StepwiseAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::BestSubsets { request } => {
                    crate::BestSubsetsAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::DistributionFit { request } => {
                    crate::DistributionFitAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::Mixture { request, .. } => {
                    crate::MixtureAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::Scatter3d { request, .. } => {
                    crate::Scatter3dAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::DataSurface { request, options } => {
                    crate::DataSurfaceAnalysis::run(t, rows, request).ok().map(|a| a.sections(options))
                }
                SavedReport::Graph { spec } => {
                    let frame = spec.frame(t, rows).ok()?;
                    let layers = spec.layers(&frame, rows);
                    assert!(layers.iter().flatten().all(stat_viz::Layer::all_finite));
                    Some(Vec::new())
                }
                SavedReport::OneSample { request } => {
                    crate::OneSampleAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::Groups { request } => {
                    crate::GroupComparison::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::Paired { request } => {
                    crate::PairedComparison::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::Proportions { request } => {
                    crate::ProportionAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::Rates { request } => crate::RateAnalysis::run(t, rows, request).ok().map(|a| a.sections()),
                SavedReport::Power { request } => crate::PowerAnalysis::run(request).ok().map(|a| a.sections()),
                SavedReport::AttributeCapability { request } => {
                    crate::AttributeCapabilityAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::BetweenWithinCapability { request } => {
                    crate::BetweenWithinAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::AcceptanceSampling { request } => {
                    crate::AcceptanceAnalysis::run(request).ok().map(|a| a.sections())
                }
                SavedReport::GaugeLinearity { .. }
                | SavedReport::GaugeRepeatability { .. }
                | SavedReport::IntraclassCorrelation { .. }
                | SavedReport::AttributeAgreement { .. }
                | SavedReport::AttributeGauge { .. }
                | SavedReport::PcaChart { .. }
                | SavedReport::MonitoringLimits { .. }
                | SavedReport::MultiwayTable { .. }
                | SavedReport::StratifiedTables { .. }
                | SavedReport::SignalToNoise { .. }
                | SavedReport::DeclaredDesign { .. }
                | SavedReport::DesignEvaluation { .. }
                | SavedReport::FactorialAnalysis { .. }
                | SavedReport::Bayesian { .. }
                | SavedReport::MultipleResponseItems { .. }
                | SavedReport::Signal { .. }
                | SavedReport::ImagePlot { .. } => {
                    crate::ComparisonReport::run(r, t, rows).and_then(Result::ok).map(|a| a.sections())
                }
                SavedReport::GoodnessOfFit { request } => {
                    crate::GoodnessAnalysis::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::Crossover { request } => {
                    crate::CrossoverReport::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::FactorialRepeated { request } => {
                    crate::FactorialRepeatedReport::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::Nested { request } => {
                    crate::NestedReport::run(t, rows, request).ok().map(|a| a.sections())
                }
                SavedReport::DistributionCalculator { request } => {
                    crate::CalculatorReport::run(request).ok().map(|a| a.sections())
                }
            }
        }
        let base = longley();
        let reports = every_report(&base);
        let n = base.n_rows();
        let map_values = |f: &dyn Fn(usize, usize, f64) -> f64| {
            let cols = base
                .columns()
                .iter()
                .enumerate()
                .map(|(j, c)| {
                    let mut c = c.clone();
                    if let ColumnData::Numeric(v) = &mut c.data {
                        for (r, x) in v.iter_mut().enumerate() {
                            *x = f(j, r, *x);
                        }
                    }
                    c
                })
                .collect();
            DataTable::new("variant", cols).unwrap()
        };
        let keep_rows = |k: usize| map_values(&|_, r, x| if r < k { x } else { f64::NAN });
        let first: Vec<f64> = base.columns().iter().map(|c| c.as_numeric().map_or(0.0, |v| v[0])).collect();
        let variants: Vec<(&str, DataTable, RowStateManager)> = vec![
            ("one row", keep_rows(1), RowStateManager::new(n)),
            ("two rows", keep_rows(2), RowStateManager::new(n)),
            ("three rows", keep_rows(3), RowStateManager::new(n)),
            ("constant columns", map_values(&|j, _, _| first[j]), RowStateManager::new(n)),
            ("empty column", map_values(&|j, _, x| if j == 1 { f64::NAN } else { x }), RowStateManager::new(n)),
            (
                "negative and zero",
                map_values(&|_, r, x| {
                    if r % 3 == 0 {
                        -x
                    } else if r % 3 == 1 {
                        0.0
                    } else {
                        x
                    }
                }),
                RowStateManager::new(n),
            ),
            ("huge", map_values(&|_, _, x| x * 1e300), RowStateManager::new(n)),
            ("tiny", map_values(&|_, _, x| x * 1e-300), RowStateManager::new(n)),
            ("all excluded", base.clone(), {
                let mut s = RowStateManager::new(n);
                s.set_excluded(0..n, true);
                s
            }),
        ];
        let mut panics = Vec::new();
        for (name, table, rows) in &variants {
            for r in &reports {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_any(r, table, rows)));
                if result.is_err() {
                    panics.push(format!("{name}: {:?}", r.columns()));
                }
            }
        }
        assert!(panics.is_empty(), "panicked on: {panics:#?}");
    }

    #[test]
    fn gauge_study_and_text_columns_round_trip() {
        let table = gauge_study();
        let request = GaugeStudyRequest {
            part: 0,
            operator: 1,
            measurement: 2,
            tolerance: Some(0.5),
            pool_interaction: false,
            study_multiplier: 5.15,
            design: crate::gauge_design::GaugeStudyDesign::default(),
        };
        let options = GaugeReportOptions { show_operator_plot: true, ..GaugeReportOptions::default() };
        let rows = RowStateManager::new(table.n_rows());
        let project = Project {
            external: None,
            origin: None,
            scripts: Vec::new(),
            table,
            rows,
            reports: vec![SavedReport::Gauge { request, options }],
        };
        let back = load_project(&save_project(&project)).unwrap();
        assert_eq!(back.table, project.table);
        assert_eq!(back.reports, project.reports);
        let (a, b) = match (&project.reports[0], &back.reports[0]) {
            (SavedReport::Gauge { request: x, .. }, SavedReport::Gauge { request: y, .. }) => (
                GaugeAnalysis::run(&project.table, &project.rows, x).unwrap(),
                GaugeAnalysis::run(&back.table, &back.rows, y).unwrap(),
            ),
            _ => unreachable!(),
        };
        assert_eq!(format!("{:?}", a.sections(&options)), format!("{:?}", b.sections(&options)));
    }

    #[test]
    fn multivariate_and_anom_options_default_when_missing() {
        let table = longley();
        let mv =
            parse_json(r#"{"analysis": "multivariate-chart", "chart": "mewma", "columns": ["GNP", "Year"]}"#).unwrap();
        let SavedReport::MultivariateChart { request } = report_from_json(&mv, &table).unwrap() else { panic!() };
        assert_eq!(request, crate::MultivariateChartRequest::new(vec![2, 6], crate::MultivariateChartKind::Mewma));
        let anom = parse_json(r#"{"analysis": "analysis-of-means", "response": "GNP", "group": "Year"}"#).unwrap();
        let SavedReport::Anom { request } = report_from_json(&anom, &table).unwrap() else { panic!() };
        assert_eq!(request, crate::AnomRequest::means(2, 6));
        let bad = parse_json(r#"{"analysis": "multivariate-chart", "chart": "x", "columns": ["GNP"]}"#).unwrap();
        assert!(report_from_json(&bad, &table).is_err());
    }

    #[test]
    fn regression_extras_default_when_missing() {
        let table = longley();
        let dr = parse_json(r#"{"analysis": "dose-response", "dose": "Year", "response": "GNP"}"#).unwrap();
        let SavedReport::DoseResponse { request } = report_from_json(&dr, &table).unwrap() else { panic!() };
        assert_eq!(request, crate::DoseResponseRequest::probit(6, 2, None));
        let mc = parse_json(r#"{"analysis": "method-comparison", "reference": "GNP", "test": "Year"}"#).unwrap();
        let SavedReport::MethodComparison { request } = report_from_json(&mc, &table).unwrap() else { panic!() };
        assert_eq!(request, crate::MethodComparisonRequest::new(2, 6));
        let sl = parse_json(r#"{"analysis": "shelf-life", "time": "Year", "response": "GNP", "lower": 1}"#).unwrap();
        let SavedReport::ShelfLife { request } = report_from_json(&sl, &table).unwrap() else { panic!() };
        // Without a time unit (as in projects saved before it existed) only the
        // twice-the-data extrapolation limit applies.
        assert_eq!(
            request,
            crate::ShelfLifeRequest { time_unit: None, ..crate::ShelfLifeRequest::lower(6, 2, None, 1.0) }
        );
        let bad = parse_json(r#"{"analysis": "shelf-life", "time": "Year", "response": "GNP", "time_unit": "aeons"}"#)
            .unwrap();
        assert!(report_from_json(&bad, &table).is_err());
        let glm = parse_json(
            r#"{"analysis": "generalized-linear-model", "response": "GNP", "predictors": ["Year"],
                "family": "binomial", "link": "probit"}"#,
        )
        .unwrap();
        let SavedReport::Glm { request, .. } = report_from_json(&glm, &table).unwrap() else { panic!() };
        assert_eq!(request.link, Some(crate::Link::Probit));
        assert_eq!(request.event, None);
        let glm = parse_json(
            r#"{"analysis": "generalized-linear-model", "response": "GNP", "predictors": ["Year"],
                "family": "binomial", "event": "1"}"#,
        )
        .unwrap();
        let SavedReport::Glm { request, .. } = report_from_json(&glm, &table).unwrap() else { panic!() };
        assert_eq!(request.event.as_deref(), Some("1"));
        let bad =
            parse_json(r#"{"analysis": "dose-response", "dose": "Year", "response": "GNP", "link": "log"}"#).unwrap();
        assert!(report_from_json(&bad, &table).is_err());
    }

    #[test]
    fn row_subsets_round_trip_and_keep_exclusions_apart() {
        let table = DataTable::new("t", vec![Column::numeric("x", vec![1.0, 2.0, 3.0, 4.0])]).unwrap();
        let mut rows = RowStateManager::new(4);
        rows.set_excluded([3], true);
        rows.save_subset("low", [0, 1]);
        rows.save_subset("high", [2, 3]);
        rows.set_active_subset(Some(0));
        let project =
            Project { external: None, origin: None, scripts: Vec::new(), table: table.clone(), rows, reports: vec![] };
        let text = save_project(&project);
        assert!(text.contains("\"active_subset\": \"low\""));
        let back = load_project(&text).unwrap();
        assert_eq!(back.rows.subset_names(), ["low", "high"]);
        assert_eq!(back.rows.subset_rows(1), [2, 3]);
        assert_eq!(back.rows.active_subset(), Some(0));
        // Rows outside the active subset are not saved as excluded.
        assert_eq!((0..4).filter(|&r| back.rows.is_excluded_flag(r)).collect::<Vec<_>>(), [3]);
        assert_eq!(back.rows.included_rows(), [0, 1]);
        // Without subsets nothing is written.
        let plain = save_project(&Project { rows: RowStateManager::new(4), ..project });
        assert!(!plain.contains("subsets"));
        let bad = text.replace("\"active_subset\": \"low\"", "\"active_subset\": \"none\"");
        assert!(load_project(&bad).is_err());
    }

    #[test]
    fn scripts_round_trip_and_stay_optional() {
        let table = DataTable::new("t", vec![Column::numeric("x", vec![1.0, 2.0])]).unwrap();
        let rows = RowStateManager::new(2);
        let scripts = vec![
            ProjectScript {
                name: "check".into(),
                source: "print(\"hi \\\"there\\\"\");\n// done".into(),
                run_on_open: true,
            },
            ProjectScript { name: "tidy".into(), source: String::new(), run_on_open: false },
        ];
        let project = Project {
            external: None,
            origin: None,
            scripts: scripts.clone(),
            table: table.clone(),
            rows: rows.clone(),
            reports: vec![],
        };
        let text = save_project(&project);
        assert_eq!(load_project(&text).unwrap().scripts, scripts);
        // Without scripts the member is left out, so older builds still open the file.
        let plain = save_project(&Project { scripts: Vec::new(), ..project });
        assert!(!plain.contains("\"scripts\""));
        assert!(load_project(&plain).unwrap().scripts.is_empty());
        let bad = text.replace("\"run_on_open\": true", "\"run_on_open\": \"yes\"");
        assert!(load_project(&bad).is_err());
    }

    /// Formula columns keep their formula and their computed values (so
    /// readers that ignore formulas still see the data).
    #[test]
    fn formula_columns_round_trip() {
        let mut doubled = Column::numeric("doubled", vec![2.0, 4.0]);
        doubled.formula = Some("x * 2 // \"twice\"".into());
        let table = DataTable::new("t", vec![Column::numeric("x", vec![1.0, 2.0]), doubled]).unwrap();
        let project = Project {
            external: None,
            origin: None,
            scripts: Vec::new(),
            table,
            rows: RowStateManager::new(2),
            reports: vec![],
        };
        let text = save_project(&project);
        assert!(text.contains("\"formula\": \"x * 2 // \\\"twice\\\"\""), "{text}");
        assert_eq!(load_project(&text).unwrap().table, project.table);
        let plain = text.replace("\"formula\": \"x * 2 // \\\"twice\\\"\",", "");
        assert_eq!(load_project(&plain).unwrap().table.column(1).unwrap().formula, None);
        assert!(load_project(&text.replace("\"formula\": \"x", "\"formula\": 3, \"y\": \"x")).is_err());
    }

    /// A formula column may leave out its values: loading computes them.
    /// Saved values that disagree with the formula are recomputed too.
    #[test]
    fn formula_columns_without_values_are_computed_on_load() {
        let text = format!(
            r#"{{"format": "{PROJECT_FORMAT}", "version": 1,
              "table": {{"name": "t", "columns": [
                {{"name": "x", "type": "numeric", "level": "continuous", "values": [1, 2, 3]}},
                {{"name": "d", "type": "numeric", "level": "continuous", "formula": "x * 2"}},
                {{"name": "s", "type": "numeric", "level": "continuous", "formula": "x + 1", "values": [0, 0, 0]}}]}},
              "rows": {{"excluded": [], "hidden": [], "selected": [], "colors": []}},
              "reports": []}}"#
        );
        let p = load_project(&text).unwrap();
        assert_eq!(p.table.n_rows(), 3);
        let d = p.table.column(1).unwrap();
        assert_eq!(d.formula.as_deref(), Some("x * 2"));
        let values = d.as_numeric().unwrap();
        for (v, want) in values.iter().zip([2.0, 4.0, 6.0]) {
            assert!((v - want).abs() < 1e-12, "{values:?}");
        }
        let s = p.table.column(2).unwrap().as_numeric().unwrap();
        for (v, want) in s.iter().zip([2.0, 3.0, 4.0]) {
            assert!((v - want).abs() < 1e-12, "{s:?}");
        }
    }

    #[test]
    fn date_time_columns_round_trip_as_version_2() {
        use stat_data::{TemporalDisplay, TemporalFormat};
        let display = TemporalDisplay { format: TemporalFormat::DateTime, fraction_digits: 3, offset_minutes: -330 };
        let mut stamps = Column::date_time("when", vec![1_791_907_078.125, f64::NAN, -86_400.5], display);
        stamps.level = MeasurementLevel::Ordinal;
        let lengths = Column::date_time("took", vec![90.0, 0.0, -1.5], TemporalDisplay::new(TemporalFormat::Duration));
        let table = DataTable::new("t", vec![stamps, lengths, Column::numeric("x", vec![1.0, 2.0, 3.0])]).unwrap();
        let rows = RowStateManager::new(3);
        let text = save_project(&Project {
            external: None,
            origin: None,
            scripts: Vec::new(),
            table: table.clone(),
            rows,
            reports: vec![],
        });
        assert!(text.contains("\"version\": 2"), "{text}");
        assert!(text.contains("\"type\": \"datetime\"") && text.contains("\"format\": \"duration\""));
        let back = load_project(&text).unwrap();
        assert_eq!(back.table, table);
        // Without date columns a project is still written as version 1.
        let plain = DataTable::new("t", vec![Column::numeric("x", vec![1.0])]).unwrap();
        let text1 = save_project(&Project {
            external: None,
            origin: None,
            scripts: Vec::new(),
            table: plain,
            rows: RowStateManager::new(1),
            reports: vec![],
        });
        assert!(text1.contains("\"version\": 1"));
        // Malformed date-time members are rejected.
        for (from, to) in [
            ("\"format\": \"duration\"", "\"format\": \"weeks\""),
            ("\"offset_minutes\": -330", "\"offset_minutes\": 5000"),
            ("\"offset_minutes\": -330", "\"offset_minutes\": 1.5"),
            ("\"fraction_digits\": 3", "\"fraction_digits\": 9"),
        ] {
            assert!(text.contains(from));
            assert!(load_project(&text.replacen(from, to, 1)).is_err(), "{to}");
        }
    }

    #[test]
    fn rejects_bad_files() {
        assert!(load_project("[]").is_err());
        assert!(load_project(r#"{"format": "something-else", "version": 1}"#).is_err());
        let table = longley();
        let rows = RowStateManager::new(table.n_rows());
        let good =
            save_project(&Project { external: None, origin: None, scripts: Vec::new(), table, rows, reports: vec![] });
        assert!(load_project(&good).is_ok());
        let newer = good.replace("\"version\": 1", "\"version\": 99");
        assert!(load_project(&newer).unwrap_err().to_string().contains("newer"));
        let bad_level = good.replacen("\"continuous\"", "\"interval\"", 1);
        assert!(load_project(&bad_level).is_err());
        let bad_row = good.replace("\"excluded\": []", "\"excluded\": [9999]");
        assert!(load_project(&bad_row).is_err());
    }

    /// One report naming a column the table does not have is left out with
    /// a note; the table and the other reports open.
    #[test]
    fn an_unreadable_report_is_left_out_with_a_note() {
        let table = longley();
        let rows = RowStateManager::new(table.n_rows());
        let n_rows = table.n_rows();
        let good =
            save_project(&Project { external: None, origin: None, scripts: Vec::new(), table, rows, reports: vec![] });
        let text = good.replace(
            "\"reports\": []",
            r#""reports": [{"analysis": "distribution", "column": "Year"},
                {"analysis": "bivariate", "x": "nope", "y": "Year"},
                {"analysis": "distribution", "column": "GNP"}]"#,
        );
        let (p, notes) = load_project_noting(&text).unwrap();
        assert_eq!(p.table.n_rows(), n_rows);
        assert_eq!(p.reports.len(), 2);
        assert!(matches!(&p.reports[1], SavedReport::Distribution { request, .. }
            if p.table.column(request.column).unwrap().name == "GNP"));
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].contains("report 2") && notes[0].contains("nope"), "{notes:?}");
        assert_eq!(load_project(&text).unwrap().reports.len(), 2);
        // The same in a version-3 file, where a window showing a later report
        // follows it to its new position.
        let mut file = tables::load_project_file(&text).unwrap();
        file.notes = "keep the version-3 form".into();
        let id = file.tables[0].id.clone();
        file.layout.windows.push(PoppedOutWindow {
            table: id,
            content: WindowContent::Report(1),
            position: None,
            size: None,
        });
        let v3 = save_project_file(&file);
        // Make the first report unreadable.
        let bad = v3.replacen("\"column\": \"Year\"", "\"column\": \"nope\"", 1);
        assert_ne!(bad, v3);
        let (back, notes) = load_project_file_noting(&bad).unwrap();
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert_eq!(back.tables[0].reports.len(), 1);
        assert_eq!(back.layout.windows.len(), 1);
        assert_eq!(back.layout.windows[0].content, WindowContent::Report(0));
        // Saving never writes an empty column name for an out-of-range index.
        let mut out_of_range = load_project(&text).unwrap();
        out_of_range.reports.push(SavedReport::Distribution {
            request: crate::DistributionRequest { column: 999, quantile_method: Default::default(), censoring: None },
            options: crate::DistributionOptions::default(),
        });
        let saved = save_project(&out_of_range);
        assert!(!saved.contains("\"column\": \"\""), "{saved}");
        assert!(checked_report_to_json(out_of_range.reports.last().unwrap(), &out_of_range.table).is_err());
    }

    #[test]
    fn column_mapping() {
        let r = SavedReport::Linear {
            spec: LinearModelSpec { response: 0, predictors: vec![2, 3], effects: Vec::new(), splines: Vec::new() },
            options: ReportOptions::default(),
        };
        assert_eq!(r.columns(), [0, 2, 3]);
        assert_eq!(r.map_columns(|c| c + 1).columns(), [1, 3, 4]);
        let g = SavedReport::Gauge {
            request: GaugeStudyRequest {
                part: 0,
                operator: 1,
                measurement: 2,
                tolerance: None,
                pool_interaction: true,
                study_multiplier: 6.0,
                design: crate::gauge_design::GaugeStudyDesign::default(),
            },
            options: GaugeReportOptions::default(),
        };
        assert_eq!(g.map_columns(|c| 2 - c).columns(), [2, 1, 0]);
    }

    #[test]
    fn missing_options_take_defaults() {
        let table = longley();
        let rows = RowStateManager::new(table.n_rows());
        let x = table.column(0).unwrap().name.clone();
        let text =
            save_project(&Project { external: None, origin: None, scripts: Vec::new(), table, rows, reports: vec![] })
                .replace(
                    "\"reports\": []",
                    &format!(r#""reports": [{{"analysis": "distribution", "column": "{x}"}}]"#),
                );
        let p = load_project(&text).unwrap();
        assert_eq!(
            p.reports,
            [SavedReport::Distribution {
                request: DistributionRequest { column: 0, quantile_method: QuantileMethod::default(), censoring: None },
                options: DistributionOptions::default(),
            }]
        );
    }
}
