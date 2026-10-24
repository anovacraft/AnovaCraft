//! Analysis reports: a collapsible tree of sections, each with an options menu,
//! and plots that read and write the shared row state (linked brushing).
//!
//! Plots do not move with scrolling. Each axis is draggable: near either end a
//! drag moves that limit (rescaling), elsewhere it shifts the whole axis
//! ([`app_model::plot_view`]). Every plot has a "Reset view" button, the report
//! options menu a "Reset all plot views" item, and double-clicking a plot also
//! re-fits it to its data.

use crate::dialog_kit::ComboExt as _;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use app_model::report::{SIGNIFICANCE_LEVEL, is_p_value_heading, is_significant};
use app_model::report_figure::FigureLayout;

use app_model::brushing::{
    PlotRegion, bar_at, bar_rows_in_region, box_rows_in_region, brush_click, brush_region, chart_point_selected,
    chart_point_visible, chart_rows_in_region, nearest_row, selected_in_bar, selection_mode,
};
use app_model::editing::ColumnShift;
use app_model::gauge_analysis::POOLING_LEVEL;
use app_model::plot_view::{Axis, AxisDrag, AxisHandle, END_HANDLE_FRACTION, ViewBounds, handle_at};
use app_model::{AnomAnalysis, AnomRequest, MultivariateChartAnalysis, MultivariateChartRequest};
use app_model::{AttributeChartAnalysis, AttributeChartRequest, CapabilityAnalysis, CapabilityRequest};
use app_model::{
    BandTransform, BarChartSpec, BivariateAnalysis, BivariateRequest, BoxPlotSpec, ClusterAnalysis, ClusterRequest,
    ComparisonReport, ControlChartAnalysis, ControlChartOptions, ControlChartRequest, ControlChartSpec,
    CorrespondenceReport, CorrespondenceRequest, DistributionAnalysis, DistributionOptions, DistributionRequest,
    GaugeAnalysis, GaugeReportOptions, GaugeStudyRequest, GlmAnalysis, GlmRequest, LinePattern, LinearModelAnalysis,
    LinearModelSpec, MixedAnalysis, MixedRequest, PcaAnalysis, PcaRequest, PlotSpec, RegularizedAnalysis,
    RegularizedRequest, ReportOptions, ReportScope, ReportSection, SavedReport, SectionContent, SectionKind,
    SurfaceAnalysis, SurfaceOptions, SurfaceRequest, SurvivalAnalysis, SurvivalOptions, SurvivalPlotSpec,
    SurvivalRequest, TermValue, TextTable, TimeWeightedAnalysis, TimeWeightedRequest,
};
use app_model::{BestSubsetsAnalysis, BestSubsetsRequest};
use app_model::{
    BoostAnalysis, BoostRequest, ForestAnalysis, ForestRequest, ModelComparison, ModelComparisonRequest, PlsAnalysis,
    PlsRequest, SvmAnalysis, SvmRequest,
};
use app_model::{
    CauseEffectAnalysis, CauseEffectRequest, MultiVariAnalysis, MultiVariOptions, MultiVariRequest, ParetoAnalysis,
    ParetoChartOptions, ParetoRequest,
};
use app_model::{
    DataSurfaceAnalysis, DataSurfaceOptions, DataSurfaceRequest, Scatter3dAnalysis, Scatter3dOptions, Scatter3dRequest,
};
use app_model::{DistributionFitAnalysis, DistributionFitRequest};
use app_model::{
    DoseResponseAnalysis, DoseResponseRequest, MethodComparisonAnalysis, MethodComparisonRequest, ShelfLifeAnalysis,
    ShelfLifeRequest,
};
use app_model::{GraphSpec, MixtureAnalysis, MixtureRequest, ProfilerOptions, StepwiseAnalysis, StepwiseRequest};
use app_model::{MarsAnalysis, MarsRequest};
use app_model::{ModelSearch, ModelSearchRequest, NeuralAnalysis, NeuralRequest};
use app_model::{MultipleResponsesAnalysis, MultipleResponsesRequest, TreeAnalysis, TreeRequest};
use app_model::{NonlinearAnalysis, NonlinearRequest};
use eframe::egui::{self, PointerButton, RichText, Stroke};
use egui_plot::{
    GridMark, HLine, Line, LineStyle, MarkerShape, Plot, PlotBounds, PlotImage, PlotPoint, PlotPoints, PlotResponse, Points,
    Polygon, Text, VLine,
};
use stat_data::{DataTable, RowStateManager, SelectionMode};

mod jobs;
pub use jobs::{Done, ReportJobs};

use crate::plot_axes::{self, AxisInfo, PlotChrome, with_axes};
use crate::plot_size::{PlotSize, report_frame_id, size_grip};
use crate::several_responses_view::{self, SeveralView};
use crate::surface_profiler::SurfaceProfilerState;
use crate::{compare_ui, response_explorer, surface_profiler, theme};
use app_model::axis_settings::{AxisSide, GridLines, PlotAxes};
use stat_quality::constants::MAX_RANGE_SUBGROUP;
use stat_quality::{ChartKind, RunRule, SubgroupUse};
use stat_viz::export::VectorFormat;

/// Pixel radius within which a click selects the nearest point.
const CLICK_RADIUS: f32 = 8.0;

/// What a new report should analyse (with default display options).
#[derive(Debug, Clone)]
pub enum ReportRequest {
    /// A linear model.
    Linear(LinearModelSpec),
    /// A control chart.
    ControlChart(ControlChartRequest),
    /// A gauge R&R study.
    Gauge(GaugeStudyRequest),
    /// The distribution of one column.
    Distribution(DistributionRequest),
    /// Two columns together.
    Bivariate(BivariateRequest),
    /// Principal components.
    Pca(PcaRequest),
    /// Clustering.
    Cluster(ClusterRequest),
    /// Generalized linear model.
    Glm(GlmRequest),
    /// Linear or generalized linear mixed model.
    Mixed(Box<MixedRequest>),
    /// Penalized regression.
    Regularized(RegularizedRequest),
    /// Classification or regression tree.
    DecisionTree(TreeRequest),
    /// Random forest.
    RandomForest(ForestRequest),
    /// Gradient-boosted trees.
    BoostedTrees(BoostRequest),
    /// Partial least squares.
    PartialLeastSquares(PlsRequest),
    /// Support vector machine.
    SupportVectorMachine(SvmRequest),
    /// Multivariate adaptive regression splines.
    Mars(MarsRequest),
    /// Neural network.
    NeuralNetwork(NeuralRequest),
    /// Gaussian-process regression.
    GaussianProcess(app_model::GaussianProcessRequest),
    /// Every applicable predictive model fitted and ranked.
    ModelSearch(ModelSearchRequest),
    /// Functional data analysis.
    FunctionalData(app_model::FunctionalRequest),
    /// Comparison of saved prediction columns.
    ModelComparison(ModelComparisonRequest),
    /// Correspondence analysis.
    Correspondence(CorrespondenceRequest),
    /// Correlations of several columns.
    Correlations(app_model::CorrelationRequest),
    /// Multivariate outlier distances.
    OutlierDistances(app_model::OutlierRequest),
    /// Item analysis of a scale.
    ItemAnalysis(app_model::ItemAnalysisRequest),
    Manova(app_model::ManovaRequest),
    /// Exploratory factor analysis.
    FactorAnalysis(app_model::FactorRequest),
    /// Linear or quadratic discriminant analysis.
    DiscriminantAnalysis(app_model::DiscriminantRequest),
    /// Gaussian mixture clustering.
    GaussianMixture(app_model::GaussianMixtureRequest),
    /// Latent class analysis.
    LatentClass(app_model::LatentClassRequest),
    /// Text exploration of a text column.
    TextExploration(app_model::TextExplorationRequest),
    /// Topic model of a text column.
    TopicModel(app_model::TopicModelRequest),
    /// Sentiment scores of a text column.
    Sentiment(app_model::SentimentRequest),
    /// Multidimensional scaling, t-SNE or UMAP map.
    Embedding(app_model::EmbeddingRequest),
    /// Missing-value imputation.
    Imputation(app_model::ImputationRequest),
    /// Series and Signals ▸ Explore: description, decomposition, trend model or cross-correlation.
    SeriesExplore(app_model::SeriesExploreRequest),
    /// Series and Signals ▸ Signals and spectra: spectrum, filter, convolution and correlation, or wavelets.
    SignalProcessing(app_model::SignalProcessingRequest),
    /// Series and Signals ▸ Signals and spectra ▸ Images: spectrum, frequency filter, or registration and deblurring.
    ImageAnalysis(app_model::ImageAnalysisRequest),
    /// Survival curves and life-distribution fits.
    Survival(SurvivalRequest),
    /// Second-order response surface.
    Surface(SurfaceRequest),
    /// EWMA or CUSUM chart.
    TimeWeighted(TimeWeightedRequest),
    /// Process capability.
    Capability(CapabilityRequest),
    /// p, np, c or u chart.
    Attribute(AttributeChartRequest),
    /// Pareto chart (with its options, chosen in the dialog).
    Pareto(ParetoRequest, ParetoChartOptions),
    /// Cause-and-effect diagram.
    CauseEffect(CauseEffectRequest),
    /// Multi-vari chart.
    MultiVari(MultiVariRequest),
    /// Hotelling T², generalized variance or MEWMA chart.
    MultivariateChart(MultivariateChartRequest),
    /// Analysis of means.
    Anom(AnomRequest),
    /// Ordinal or nominal logistic regression.
    CategoricalLogistic(app_model::CategoricalLogisticRequest),
    /// Dose–response (probit) analysis.
    DoseResponse(DoseResponseRequest),
    /// Nonlinear regression.
    Nonlinear(NonlinearRequest),
    /// Deming and Passing–Bablok method comparison.
    MethodComparison(MethodComparisonRequest),
    /// Shelf life from stability data.
    ShelfLife(ShelfLifeRequest),
    /// Reliability growth, repairable systems or a test plan.
    Reliability(Box<SavedReport>),
    /// Stepwise selection.
    Stepwise(StepwiseRequest),
    /// Best-subsets regression.
    BestSubsets(BestSubsetsRequest),
    /// Distribution identification.
    DistributionFit(DistributionFitRequest),
    /// Scheffé mixture model.
    Mixture(MixtureRequest),
    /// A comparison, proportion, rate or power report.
    Comparison(SavedReport),
    /// A graph canvas.
    Graph(Box<GraphSpec>),
    /// Several responses: linked explorer, desirability and simulation.
    MultipleResponses(MultipleResponsesRequest),
    /// A bootstrap of a report's table, or a permutation test.
    Resampling(Box<SavedReport>),
    /// A 3-D scatter plot.
    Scatter3d(Scatter3dRequest),
    /// A contour and surface plot of data.
    DataSurface(DataSurfaceRequest),
}

/// `new` with the display options of `old` when both are reports of the
/// same kind (an edited analysis keeps what its report showed).
pub fn keep_display_options(new: SavedReport, old: &SavedReport) -> SavedReport {
    use SavedReport as S;
    match (new, old) {
        (S::Linear { spec, .. }, S::Linear { options, .. }) => S::Linear { spec, options: *options },
        (S::ControlChart { request, .. }, S::ControlChart { options, .. }) => {
            S::ControlChart { request, options: *options }
        }
        (S::Gauge { request, .. }, S::Gauge { options, .. }) => S::Gauge { request, options: *options },
        (S::Distribution { request, .. }, S::Distribution { options, .. }) => {
            S::Distribution { request, options: *options }
        }
        (S::Survival { request, .. }, S::Survival { options, .. }) => S::Survival { request, options: *options },
        (S::Surface { request, .. }, S::Surface { options, .. }) => S::Surface { request, options: *options },
        (S::Stepwise { request, .. }, S::Stepwise { options, .. }) => S::Stepwise { request, options: *options },
        // A Pareto chart's options ("Other" threshold, bar scale) are set in
        // its dialog, so an edited chart takes the dialog's.
        (S::MultiVari { request, .. }, S::MultiVari { options, .. }) => S::MultiVari { request, options: *options },
        (S::Glm { request, .. }, S::Glm { profilers, .. }) => S::Glm { request, profilers: *profilers },
        (S::Mixed { request, .. }, S::Mixed { profilers, .. }) => S::Mixed { request, profilers: *profilers },
        (S::ProportionalHazards { request, .. }, S::ProportionalHazards { profilers, .. }) => {
            S::ProportionalHazards { request, profilers: *profilers }
        }
        (S::SurvivalRegression { request, .. }, S::SurvivalRegression { profilers, .. }) => {
            S::SurvivalRegression { request, profilers: *profilers }
        }
        (S::Regularized { request, .. }, S::Regularized { profilers, .. }) => {
            S::Regularized { request, profilers: *profilers }
        }
        (S::DecisionTree { request, .. }, S::DecisionTree { profilers, .. }) => {
            S::DecisionTree { request, profilers: *profilers }
        }
        (S::RandomForest { request, .. }, S::RandomForest { profilers, .. }) => {
            S::RandomForest { request, profilers: *profilers }
        }
        (S::BoostedTrees { request, .. }, S::BoostedTrees { profilers, .. }) => {
            S::BoostedTrees { request, profilers: *profilers }
        }
        (S::PartialLeastSquares { request, .. }, S::PartialLeastSquares { profilers, .. }) => {
            S::PartialLeastSquares { request, profilers: *profilers }
        }
        (S::SupportVectorMachine { request, .. }, S::SupportVectorMachine { profilers, .. }) => {
            S::SupportVectorMachine { request, profilers: *profilers }
        }
        (S::Mars { request, .. }, S::Mars { profilers, .. }) => S::Mars { request, profilers: *profilers },
        (S::Nonlinear { request, .. }, S::Nonlinear { profilers, .. }) => {
            S::Nonlinear { request, profilers: *profilers }
        }
        (S::NeuralNetwork { request, .. }, S::NeuralNetwork { profilers, .. }) => {
            S::NeuralNetwork { request, profilers: *profilers }
        }
        (S::GaussianProcess { request, .. }, S::GaussianProcess { profilers, .. }) => {
            S::GaussianProcess { request, profilers: *profilers }
        }
        (S::FunctionalData { request, .. }, S::FunctionalData { profilers, .. }) => {
            S::FunctionalData { request, profilers: *profilers }
        }
        (S::Mixture { request, .. }, S::Mixture { profilers, .. }) => S::Mixture { request, profilers: *profilers },
        (S::Scatter3d { request, .. }, S::Scatter3d { options, .. }) => S::Scatter3d { request, options: *options },
        (S::DataSurface { request, .. }, S::DataSurface { options, .. }) => {
            S::DataSurface { request, options: *options }
        }
        (new, _) => new,
    }
}

/// A column a report asks to add to the data table (e.g. cluster numbers,
/// or a model's prediction formula).
pub struct NewColumn(pub app_model::ReportColumn);

impl From<ReportRequest> for SavedReport {
    fn from(request: ReportRequest) -> Self {
        match request {
            ReportRequest::Linear(spec) => Self::Linear { spec, options: ReportOptions::default() },
            ReportRequest::ControlChart(request) => {
                Self::ControlChart { request, options: ControlChartOptions::default() }
            }
            ReportRequest::Gauge(request) => Self::Gauge { request, options: GaugeReportOptions::default() },
            ReportRequest::Distribution(request) => {
                Self::Distribution { request, options: DistributionOptions::default() }
            }
            ReportRequest::Bivariate(request) => Self::Bivariate { request },
            ReportRequest::Pca(request) => Self::Pca { request },
            ReportRequest::Cluster(request) => Self::Cluster { request },
            ReportRequest::Glm(request) => Self::Glm { request, profilers: ProfilerOptions::default() },
            ReportRequest::Mixed(request) => Self::Mixed { request: *request, profilers: ProfilerOptions::default() },
            ReportRequest::Regularized(request) => Self::Regularized { request, profilers: ProfilerOptions::default() },
            ReportRequest::DecisionTree(request) => {
                Self::DecisionTree { request, profilers: ProfilerOptions::default() }
            }
            ReportRequest::RandomForest(request) => {
                Self::RandomForest { request, profilers: ProfilerOptions::default() }
            }
            ReportRequest::BoostedTrees(request) => {
                Self::BoostedTrees { request, profilers: ProfilerOptions::default() }
            }
            ReportRequest::PartialLeastSquares(request) => {
                Self::PartialLeastSquares { request, profilers: ProfilerOptions::default() }
            }
            ReportRequest::SupportVectorMachine(request) => {
                Self::SupportVectorMachine { request, profilers: ProfilerOptions::default() }
            }
            ReportRequest::Mars(request) => Self::Mars { request, profilers: ProfilerOptions::default() },
            ReportRequest::NeuralNetwork(request) => {
                Self::NeuralNetwork { request, profilers: ProfilerOptions::default() }
            }
            ReportRequest::GaussianProcess(request) => {
                Self::GaussianProcess { request, profilers: ProfilerOptions::default() }
            }
            ReportRequest::ModelSearch(request) => Self::ModelSearch { request },
            ReportRequest::FunctionalData(request) => {
                Self::FunctionalData { request, profilers: ProfilerOptions::default() }
            }
            ReportRequest::ModelComparison(request) => Self::ModelComparison { request },
            ReportRequest::Correspondence(request) => Self::Correspondence { request },
            ReportRequest::Correlations(request) => Self::Correlations { request },
            ReportRequest::OutlierDistances(request) => Self::OutlierDistances { request },
            ReportRequest::ItemAnalysis(request) => Self::ItemAnalysis { request },
            ReportRequest::Manova(request) => Self::Manova { request },
            ReportRequest::FactorAnalysis(request) => Self::FactorAnalysis { request },
            ReportRequest::DiscriminantAnalysis(request) => Self::DiscriminantAnalysis { request },
            ReportRequest::GaussianMixture(request) => Self::GaussianMixture { request },
            ReportRequest::LatentClass(request) => Self::LatentClass { request },
            ReportRequest::TextExploration(request) => Self::TextExploration { request },
            ReportRequest::TopicModel(request) => Self::TopicModel { request },
            ReportRequest::Sentiment(request) => Self::Sentiment { request },
            ReportRequest::Embedding(request) => Self::Embedding { request },
            ReportRequest::Imputation(request) => Self::Imputation { request },
            ReportRequest::SeriesExplore(request) => Self::SeriesExplore { request },
            ReportRequest::SignalProcessing(request) => Self::SignalProcessing { request },
            ReportRequest::ImageAnalysis(request) => Self::ImageAnalysis { request },
            ReportRequest::Survival(request) => Self::Survival { request, options: SurvivalOptions::default() },
            ReportRequest::Surface(request) => Self::Surface { request, options: SurfaceOptions::default() },
            ReportRequest::TimeWeighted(request) => Self::TimeWeighted { request },
            ReportRequest::Capability(request) => Self::Capability { request },
            ReportRequest::Attribute(request) => Self::Attribute { request },
            ReportRequest::Pareto(request, options) => Self::Pareto { request, options },
            ReportRequest::CauseEffect(request) => Self::CauseEffect { request },
            ReportRequest::MultiVari(request) => Self::MultiVari { request, options: MultiVariOptions::default() },
            ReportRequest::MultivariateChart(request) => Self::MultivariateChart { request },
            ReportRequest::Anom(request) => Self::Anom { request },
            ReportRequest::DoseResponse(request) => Self::DoseResponse { request },
            ReportRequest::CategoricalLogistic(request) => Self::CategoricalLogistic { request },
            ReportRequest::Nonlinear(request) => {
                Self::Nonlinear { request, profilers: NonlinearAnalysis::default_profilers() }
            }
            ReportRequest::MethodComparison(request) => Self::MethodComparison { request },
            ReportRequest::ShelfLife(request) => Self::ShelfLife { request },
            ReportRequest::Reliability(saved) => *saved,
            ReportRequest::Stepwise(request) => Self::Stepwise { request, options: ProfilerOptions::default() },
            ReportRequest::BestSubsets(request) => Self::BestSubsets { request },
            ReportRequest::DistributionFit(request) => Self::DistributionFit { request },
            ReportRequest::Mixture(request) => Self::Mixture { request, profilers: ProfilerOptions::default() },
            ReportRequest::Comparison(saved) => saved,
            ReportRequest::Graph(spec) => Self::Graph { spec },
            ReportRequest::MultipleResponses(request) => Self::MultipleResponses { request: Box::new(request) },
            ReportRequest::Resampling(saved) => *saved,
            ReportRequest::Scatter3d(request) => Self::Scatter3d { request, options: Scatter3dOptions::default() },
            ReportRequest::DataSurface(request) => {
                Self::DataSurface { request, options: DataSurfaceOptions::default() }
            }
        }
    }
}

/// The analysis behind a report, with its options and latest result.
// One per open report, kept in a `Vec`: variant sizes differ, but boxing the
// large ones would only add indirection.
#[allow(clippy::large_enum_variant)]
enum ReportBody {
    /// The analysis panicked (a bug): the report shows `message` in place of
    /// its sections and computes `saved` again when the data or its
    /// settings change.
    Failed {
        saved: SavedReport,
        message: String,
    },
    Linear {
        spec: LinearModelSpec,
        options: ReportOptions,
        analysis: Result<LinearModelAnalysis, String>,
        /// Response Explorer settings, one per model term (shared with the 3-D profiler).
        settings: Vec<TermValue>,
        surface: SurfaceProfilerState,
    },
    Chart {
        /// Current settings (the options menu can switch the chart type in place).
        request: ControlChartRequest,
        /// The settings the analysis was computed with.
        computed_with: ControlChartRequest,
        options: ControlChartOptions,
        analysis: Result<ControlChartAnalysis, String>,
    },
    Gauge {
        /// Current settings (the options menu can change pooling).
        request: GaugeStudyRequest,
        /// Settings the current result was computed with.
        computed_with: GaugeStudyRequest,
        options: GaugeReportOptions,
        analysis: Result<GaugeAnalysis, String>,
    },
    Distribution {
        request: DistributionRequest,
        options: DistributionOptions,
        analysis: Result<DistributionAnalysis, String>,
    },
    Bivariate {
        request: BivariateRequest,
        analysis: Result<BivariateAnalysis, String>,
    },
    Pca {
        request: PcaRequest,
        analysis: Result<PcaAnalysis, String>,
    },
    Cluster {
        request: ClusterRequest,
        analysis: Result<ClusterAnalysis, String>,
    },
    Mixed {
        request: MixedRequest,
        profilers: ProfilerOptions,
        analysis: Result<MixedAnalysis, String>,
        /// Profiler settings (one per factor).
        settings: Vec<TermValue>,
        surface: SurfaceProfilerState,
    },
    ProportionalHazards {
        request: app_model::ProportionalHazardsRequest,
        profilers: ProfilerOptions,
        analysis: Result<app_model::ProportionalHazardsAnalysis, String>,
        /// Profiler settings (one per factor).
        settings: Vec<TermValue>,
        surface: SurfaceProfilerState,
    },
    SurvivalRegression {
        request: app_model::SurvivalRegressionRequest,
        profilers: ProfilerOptions,
        analysis: Result<app_model::SurvivalRegressionAnalysis, String>,
        /// Profiler settings (one per factor).
        settings: Vec<TermValue>,
        surface: SurfaceProfilerState,
    },
    Glm {
        request: GlmRequest,
        profilers: ProfilerOptions,
        analysis: Result<GlmAnalysis, String>,
        /// Profiler settings (one per factor).
        settings: Vec<TermValue>,
        surface: SurfaceProfilerState,
    },
    Regularized {
        request: RegularizedRequest,
        profilers: ProfilerOptions,
        analysis: Result<RegularizedAnalysis, String>,
        /// Profiler settings (one per factor).
        settings: Vec<TermValue>,
        surface: SurfaceProfilerState,
    },
    DecisionTree {
        request: TreeRequest,
        profilers: ProfilerOptions,
        analysis: Result<TreeAnalysis, String>,
        /// Profiler settings (one per factor).
        settings: Vec<TermValue>,
        surface: SurfaceProfilerState,
    },
    RandomForest {
        request: ForestRequest,
        profilers: ProfilerOptions,
        analysis: Result<ForestAnalysis, String>,
        /// Profiler settings (one per factor).
        settings: Vec<TermValue>,
        surface: SurfaceProfilerState,
    },
    BoostedTrees {
        request: BoostRequest,
        profilers: ProfilerOptions,
        analysis: Result<BoostAnalysis, String>,
        /// Profiler settings (one per factor).
        settings: Vec<TermValue>,
        surface: SurfaceProfilerState,
    },
    PartialLeastSquares {
        request: PlsRequest,
        profilers: ProfilerOptions,
        analysis: Result<PlsAnalysis, String>,
        /// Profiler settings (one per factor).
        settings: Vec<TermValue>,
        surface: SurfaceProfilerState,
    },
    SupportVectorMachine {
        request: SvmRequest,
        profilers: ProfilerOptions,
        analysis: Result<SvmAnalysis, String>,
        /// Profiler settings (one per factor).
        settings: Vec<TermValue>,
        surface: SurfaceProfilerState,
    },
    Mars {
        request: MarsRequest,
        profilers: ProfilerOptions,
        analysis: Result<MarsAnalysis, String>,
        /// Profiler settings (one per factor).
        settings: Vec<TermValue>,
        surface: SurfaceProfilerState,
    },
    NeuralNetwork {
        request: NeuralRequest,
        profilers: ProfilerOptions,
        analysis: Result<NeuralAnalysis, String>,
        /// Profiler settings (one per factor).
        settings: Vec<TermValue>,
        surface: SurfaceProfilerState,
    },
    GaussianProcess {
        request: app_model::GaussianProcessRequest,
        profilers: ProfilerOptions,
        analysis: Result<Box<app_model::GaussianProcessAnalysis>, String>,
        /// Profiler settings (one per factor).
        settings: Vec<TermValue>,
        surface: SurfaceProfilerState,
    },
    FunctionalData {
        request: app_model::FunctionalRequest,
        profilers: ProfilerOptions,
        analysis: Result<Box<app_model::FunctionalAnalysis>, String>,
        /// Profiler settings (one per factor, then time).
        settings: Vec<TermValue>,
        surface: SurfaceProfilerState,
    },
    ModelComparison {
        request: ModelComparisonRequest,
        analysis: Result<ModelComparison, String>,
    },
    ModelSearch {
        request: ModelSearchRequest,
        analysis: Result<ModelSearch, String>,
    },
    Correspondence {
        request: CorrespondenceRequest,
        analysis: Result<CorrespondenceReport, String>,
    },
    Correlations {
        request: app_model::CorrelationRequest,
        analysis: Result<app_model::CorrelationAnalysis, String>,
    },
    OutlierDistances {
        request: app_model::OutlierRequest,
        analysis: Result<app_model::OutlierAnalysis, String>,
    },
    Manova {
        request: app_model::ManovaRequest,
        analysis: Result<app_model::ManovaAnalysis, String>,
    },
    ItemAnalysis {
        request: app_model::ItemAnalysisRequest,
        analysis: Result<app_model::ItemAnalysisReport, String>,
    },
    FactorAnalysis {
        request: app_model::FactorRequest,
        analysis: Result<app_model::FactorAnalysisReport, String>,
    },
    DiscriminantAnalysis {
        request: app_model::DiscriminantRequest,
        analysis: Result<app_model::DiscriminantAnalysis, String>,
    },
    GaussianMixture {
        request: app_model::GaussianMixtureRequest,
        analysis: Result<app_model::GaussianMixtureAnalysis, String>,
    },
    LatentClass {
        request: app_model::LatentClassRequest,
        analysis: Result<app_model::LatentClassAnalysis, String>,
    },
    TextExploration {
        request: app_model::TextExplorationRequest,
        analysis: Result<app_model::TextExploration, String>,
    },
    TopicModel {
        request: app_model::TopicModelRequest,
        analysis: Result<app_model::TopicModelAnalysis, String>,
    },
    Sentiment {
        request: app_model::SentimentRequest,
        analysis: Result<app_model::SentimentAnalysis, String>,
    },
    Embedding {
        request: app_model::EmbeddingRequest,
        analysis: Result<app_model::EmbeddingAnalysis, String>,
    },
    SeriesExplore {
        /// Edited in place by the options menu (display settings are part of the request).
        request: app_model::SeriesExploreRequest,
        computed_with: app_model::SeriesExploreRequest,
        analysis: Result<app_model::SeriesExploreAnalysis, String>,
    },
    SignalProcessing {
        /// Edited in place by the options menu (display settings are part of the request).
        request: app_model::SignalProcessingRequest,
        computed_with: app_model::SignalProcessingRequest,
        analysis: Result<app_model::SignalAnalysis, String>,
    },
    ImageAnalysis {
        /// Edited in place by the options menu (display settings are part of the request).
        request: app_model::ImageAnalysisRequest,
        computed_with: app_model::ImageAnalysisRequest,
        analysis: Result<app_model::ImageAnalysis, String>,
    },
    Imputation {
        request: app_model::ImputationRequest,
        analysis: Result<app_model::ImputationReport, String>,
    },
    Survival {
        request: SurvivalRequest,
        options: SurvivalOptions,
        analysis: Result<SurvivalAnalysis, String>,
    },
    Surface {
        request: SurfaceRequest,
        options: SurfaceOptions,
        analysis: Result<SurfaceAnalysis, String>,
        /// 3-D profiler factor settings and view.
        profile_settings: Vec<TermValue>,
        surface: SurfaceProfilerState,
    },
    TimeWeighted {
        request: TimeWeightedRequest,
        analysis: Result<TimeWeightedAnalysis, String>,
    },
    Capability {
        request: CapabilityRequest,
        /// The request the analysis was computed from.
        computed_with: CapabilityRequest,
        analysis: Result<CapabilityAnalysis, String>,
    },
    Attribute {
        request: AttributeChartRequest,
        /// The request the analysis was computed from.
        computed_with: AttributeChartRequest,
        analysis: Result<AttributeChartAnalysis, String>,
    },
    Pareto {
        request: ParetoRequest,
        options: ParetoChartOptions,
        analysis: Result<ParetoAnalysis, String>,
    },
    CauseEffect {
        request: CauseEffectRequest,
        analysis: Result<CauseEffectAnalysis, String>,
    },
    MultiVari {
        request: MultiVariRequest,
        options: MultiVariOptions,
        analysis: Result<MultiVariAnalysis, String>,
    },
    MultivariateChart {
        request: MultivariateChartRequest,
        analysis: Result<MultivariateChartAnalysis, String>,
    },
    Anom {
        request: AnomRequest,
        analysis: Result<AnomAnalysis, String>,
    },
    DoseResponse {
        request: DoseResponseRequest,
        analysis: Result<DoseResponseAnalysis, String>,
    },
    CategoricalLogistic {
        request: app_model::CategoricalLogisticRequest,
        analysis: Result<app_model::CategoricalLogisticAnalysis, String>,
    },
    Nonlinear {
        request: NonlinearRequest,
        profilers: ProfilerOptions,
        analysis: Result<NonlinearAnalysis, String>,
        /// Profiler settings (one per factor).
        settings: Vec<TermValue>,
        surface: SurfaceProfilerState,
    },
    MethodComparison {
        request: MethodComparisonRequest,
        analysis: Result<MethodComparisonAnalysis, String>,
    },
    ShelfLife {
        request: ShelfLifeRequest,
        analysis: Result<ShelfLifeAnalysis, String>,
    },
    /// Reliability growth, repairable systems and test plans, run through
    /// [`app_model::run_report`].
    Reliability {
        saved: SavedReport,
        result: Result<app_model::report_run::ComputedReport, String>,
    },
    Stepwise {
        request: StepwiseRequest,
        options: ProfilerOptions,
        analysis: Result<StepwiseAnalysis, String>,
        /// Profiler settings for the selected model.
        settings: Vec<TermValue>,
        surface: SurfaceProfilerState,
    },
    BestSubsets {
        request: BestSubsetsRequest,
        analysis: Result<BestSubsetsAnalysis, String>,
    },
    DistributionFit {
        request: DistributionFitRequest,
        analysis: Result<DistributionFitAnalysis, String>,
    },
    Mixture {
        request: MixtureRequest,
        profilers: ProfilerOptions,
        analysis: Result<MixtureAnalysis, String>,
        /// Response Explorer settings (one proportion per component).
        settings: Vec<TermValue>,
    },
    /// Comparison, proportion, rate and power reports, whose settings are
    /// edited in place from the options menu.
    Comparison {
        /// Current settings.
        current: SavedReport,
        /// Settings the result was computed with.
        computed_with: SavedReport,
        analysis: Result<ComparisonReport, String>,
    },
    /// Several responses fitted on the same factors.
    MultipleResponses {
        /// Current request (settings, goals and simulation edited in place).
        request: MultipleResponsesRequest,
        /// The request the models were fitted with, without settings.
        computed_with: MultipleResponsesRequest,
        analysis: Result<MultipleResponsesAnalysis, String>,
        view: Box<SeveralView>,
    },
    /// A 3-D scatter plot.
    Scatter3d {
        request: Scatter3dRequest,
        options: Scatter3dOptions,
        analysis: Result<Scatter3dAnalysis, String>,
        view: crate::scatter3d_view::Scatter3dView,
    },
    /// A contour and surface plot of data.
    DataSurface {
        /// Current request (the method can change from the options menu).
        request: DataSurfaceRequest,
        /// The request the surface was computed with.
        computed_with: DataSurfaceRequest,
        options: DataSurfaceOptions,
        analysis: Result<DataSurfaceAnalysis, String>,
        /// 3-D view state (its display settings mirror `options`).
        surface: SurfaceProfilerState,
    },
    /// A graph canvas (drawn from the data each frame; nothing to compute).
    Graph {
        spec: Box<GraphSpec>,
        view: Box<graph_view::GraphView>,
    },
    /// Not computed yet: the request waits for a background worker.
    Waiting {
        saved: Box<SavedReport>,
    },
}

fn run_comparison(saved: &SavedReport, table: &DataTable, rows: &RowStateManager) -> Result<ComparisonReport, String> {
    match ComparisonReport::run(saved, table, rows) {
        Some(r) => r.map_err(|e| e.to_string()),
        None => Err("not a comparison report".to_owned()),
    }
}

impl ReportBody {
    /// [`Self::compute`], with a panic inside the analysis turned into a
    /// [`Self::Failed`] report instead of ending the program.
    fn compute_guarded(saved: SavedReport, table: &DataTable, rows: &RowStateManager) -> Self {
        let request = saved.clone();
        crate::app::crash::guarded(|| Self::compute(request, table, rows))
            .unwrap_or_else(|message| Self::Failed { saved, message })
    }

    /// [`Self::recompute`], with a panic turned into a [`Self::Failed`]
    /// report; a failed report computes its request afresh.
    fn recompute_guarded(&mut self, table: &DataTable, rows: &RowStateManager) {
        if let Self::Failed { saved, .. } = self {
            *self = Self::compute_guarded(saved.clone(), table, rows);
            return;
        }
        if let Err(message) = crate::app::crash::guarded(|| self.recompute(table, rows)) {
            *self = Self::Failed { saved: self.saved(), message };
        }
    }

    fn compute(saved: SavedReport, table: &DataTable, rows: &RowStateManager) -> Self {
        match saved {
            // `Report` keeps the scope itself and passes filtered rows.
            SavedReport::Scoped { report, .. } => Self::compute(*report, table, rows),
            SavedReport::Linear { spec, options } => {
                let analysis = LinearModelAnalysis::run(table, rows, &spec).map_err(|e| e.to_string());
                let settings = analysis.as_ref().map(LinearModelAnalysis::default_settings).unwrap_or_default();
                Self::Linear { spec, options, analysis, settings, surface: SurfaceProfilerState::default() }
            }
            SavedReport::ControlChart { request, options } => {
                let analysis = ControlChartAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::Chart { computed_with: request.clone(), request, options, analysis }
            }
            SavedReport::Gauge { request, options } => {
                let analysis = GaugeAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::Gauge { computed_with: request.clone(), request, options, analysis }
            }
            SavedReport::Distribution { request, options } => {
                let analysis = DistributionAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::Distribution { request, options, analysis }
            }
            SavedReport::Bivariate { request } => {
                let analysis = BivariateAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::Bivariate { request, analysis }
            }
            SavedReport::Pca { request } => {
                let analysis = PcaAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::Pca { request, analysis }
            }
            SavedReport::Cluster { request } => {
                let analysis = ClusterAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::Cluster { request, analysis }
            }
            SavedReport::Mixed { request, profilers } => {
                let analysis = MixedAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::Mixed {
                    request,
                    profilers,
                    analysis,
                    settings: Vec::new(),
                    surface: SurfaceProfilerState::default(),
                }
            }
            SavedReport::ProportionalHazards { request, profilers } => {
                let analysis =
                    app_model::ProportionalHazardsAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::ProportionalHazards {
                    request,
                    profilers,
                    analysis,
                    settings: Vec::new(),
                    surface: SurfaceProfilerState::default(),
                }
            }
            SavedReport::SurvivalRegression { request, profilers } => {
                let analysis =
                    app_model::SurvivalRegressionAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::SurvivalRegression {
                    request,
                    profilers,
                    analysis,
                    settings: Vec::new(),
                    surface: SurfaceProfilerState::default(),
                }
            }
            SavedReport::Glm { request, profilers } => {
                let analysis = GlmAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::Glm {
                    request,
                    profilers,
                    analysis,
                    settings: Vec::new(),
                    surface: SurfaceProfilerState::default(),
                }
            }
            SavedReport::Regularized { request, profilers } => {
                let analysis = RegularizedAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::Regularized {
                    request,
                    profilers,
                    analysis,
                    settings: Vec::new(),
                    surface: SurfaceProfilerState::default(),
                }
            }
            SavedReport::DecisionTree { request, profilers } => {
                let analysis = TreeAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::DecisionTree {
                    request,
                    profilers,
                    analysis,
                    settings: Vec::new(),
                    surface: SurfaceProfilerState::default(),
                }
            }
            SavedReport::RandomForest { request, profilers } => {
                let analysis = ForestAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::RandomForest {
                    request,
                    profilers,
                    analysis,
                    settings: Vec::new(),
                    surface: SurfaceProfilerState::default(),
                }
            }
            SavedReport::BoostedTrees { request, profilers } => {
                let analysis = BoostAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::BoostedTrees {
                    request,
                    profilers,
                    analysis,
                    settings: Vec::new(),
                    surface: SurfaceProfilerState::default(),
                }
            }
            SavedReport::PartialLeastSquares { request, profilers } => {
                let analysis = PlsAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::PartialLeastSquares {
                    request,
                    profilers,
                    analysis,
                    settings: Vec::new(),
                    surface: SurfaceProfilerState::default(),
                }
            }
            SavedReport::SupportVectorMachine { request, profilers } => {
                let analysis = SvmAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::SupportVectorMachine {
                    request,
                    profilers,
                    analysis,
                    settings: Vec::new(),
                    surface: SurfaceProfilerState::default(),
                }
            }
            SavedReport::FunctionalData { request, profilers } => {
                let analysis =
                    app_model::FunctionalAnalysis::run(table, rows, &request).map(Box::new).map_err(|e| e.to_string());
                Self::FunctionalData {
                    request,
                    profilers,
                    analysis,
                    settings: Vec::new(),
                    surface: SurfaceProfilerState::default(),
                }
            }
            SavedReport::Mars { request, profilers } => {
                let analysis = MarsAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::Mars {
                    request,
                    profilers,
                    analysis,
                    settings: Vec::new(),
                    surface: SurfaceProfilerState::default(),
                }
            }
            SavedReport::NeuralNetwork { request, profilers } => {
                let analysis = NeuralAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::NeuralNetwork {
                    request,
                    profilers,
                    analysis,
                    settings: Vec::new(),
                    surface: SurfaceProfilerState::default(),
                }
            }
            SavedReport::GaussianProcess { request, profilers } => {
                let analysis = app_model::GaussianProcessAnalysis::run(table, rows, &request)
                    .map(Box::new)
                    .map_err(|e| e.to_string());
                Self::GaussianProcess {
                    request,
                    profilers,
                    analysis,
                    settings: Vec::new(),
                    surface: SurfaceProfilerState::default(),
                }
            }
            SavedReport::ModelComparison { request } => {
                let analysis = ModelComparison::run(table, rows, &request).map_err(|e| e.to_string());
                Self::ModelComparison { request, analysis }
            }
            SavedReport::ModelSearch { request } => {
                let analysis = ModelSearch::run(table, rows, &request).map_err(|e| e.to_string());
                Self::ModelSearch { request, analysis }
            }
            SavedReport::Correlations { request } => {
                let analysis = app_model::CorrelationAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::Correlations { request, analysis }
            }
            SavedReport::OutlierDistances { request } => {
                let analysis = app_model::OutlierAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::OutlierDistances { request, analysis }
            }
            SavedReport::Manova { request } => {
                let analysis = app_model::ManovaAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::Manova { request, analysis }
            }
            SavedReport::ItemAnalysis { request } => {
                let analysis = app_model::ItemAnalysisReport::run(table, rows, &request).map_err(|e| e.to_string());
                Self::ItemAnalysis { request, analysis }
            }
            SavedReport::FactorAnalysis { request } => {
                let analysis = app_model::FactorAnalysisReport::run(table, rows, &request).map_err(|e| e.to_string());
                Self::FactorAnalysis { request, analysis }
            }
            SavedReport::DiscriminantAnalysis { request } => {
                let analysis = app_model::DiscriminantAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::DiscriminantAnalysis { request, analysis }
            }
            SavedReport::GaussianMixture { request } => {
                let analysis =
                    app_model::GaussianMixtureAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::GaussianMixture { request, analysis }
            }
            SavedReport::LatentClass { request } => {
                let analysis = app_model::LatentClassAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::LatentClass { request, analysis }
            }
            SavedReport::TextExploration { request } => {
                let analysis = app_model::TextExploration::run(table, rows, &request).map_err(|e| e.to_string());
                Self::TextExploration { request, analysis }
            }
            SavedReport::TopicModel { request } => {
                let analysis = app_model::TopicModelAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::TopicModel { request, analysis }
            }
            SavedReport::Sentiment { request } => {
                let analysis = app_model::SentimentAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::Sentiment { request, analysis }
            }
            SavedReport::Embedding { request } => {
                let analysis = app_model::EmbeddingAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::Embedding { request, analysis }
            }
            SavedReport::SeriesExplore { request } => {
                let analysis = app_model::SeriesExploreAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::SeriesExplore { computed_with: request.clone(), request, analysis }
            }
            SavedReport::SignalProcessing { request } => {
                let analysis = app_model::SignalAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::SignalProcessing { computed_with: request.clone(), request, analysis }
            }
            SavedReport::ImageAnalysis { request } => {
                let analysis = app_model::ImageAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::ImageAnalysis { computed_with: request.clone(), request, analysis }
            }
            SavedReport::Imputation { request } => {
                let analysis = app_model::ImputationReport::run(table, rows, &request).map_err(|e| e.to_string());
                Self::Imputation { request, analysis }
            }
            SavedReport::Correspondence { request } => {
                let analysis = CorrespondenceReport::run(table, rows, &request).map_err(|e| e.to_string());
                Self::Correspondence { request, analysis }
            }
            SavedReport::Survival { request, options } => {
                let analysis = SurvivalAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::Survival { request, options, analysis }
            }
            SavedReport::Surface { request, options } => {
                let analysis = SurfaceAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::Surface {
                    options: options.for_factors(request.factors.len()),
                    request,
                    analysis,
                    profile_settings: Vec::new(),
                    surface: SurfaceProfilerState::default(),
                }
            }
            SavedReport::TimeWeighted { request } => {
                let analysis = TimeWeightedAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::TimeWeighted { request, analysis }
            }
            SavedReport::Capability { request } => {
                let analysis = CapabilityAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::Capability { request, computed_with: request, analysis }
            }
            SavedReport::Attribute { request } => {
                let analysis = AttributeChartAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::Attribute { request, computed_with: request, analysis }
            }
            SavedReport::Pareto { request, options } => {
                let analysis = ParetoAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::Pareto { request, options, analysis }
            }
            SavedReport::CauseEffect { request } => {
                let analysis = CauseEffectAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::CauseEffect { request, analysis }
            }
            SavedReport::MultiVari { request, options } => {
                let analysis = MultiVariAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::MultiVari { request, options, analysis }
            }
            SavedReport::MultivariateChart { request } => {
                let analysis = MultivariateChartAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::MultivariateChart { request, analysis }
            }
            SavedReport::Anom { request } => {
                let analysis = AnomAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::Anom { request, analysis }
            }
            SavedReport::DoseResponse { request } => {
                let analysis = DoseResponseAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::DoseResponse { request, analysis }
            }
            SavedReport::CategoricalLogistic { request } => {
                let analysis =
                    app_model::CategoricalLogisticAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::CategoricalLogistic { request, analysis }
            }
            SavedReport::Nonlinear { request, profilers } => {
                let analysis = NonlinearAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::Nonlinear {
                    request,
                    profilers,
                    analysis,
                    settings: Vec::new(),
                    surface: SurfaceProfilerState::default(),
                }
            }
            SavedReport::MethodComparison { request } => {
                let analysis = MethodComparisonAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::MethodComparison { request, analysis }
            }
            SavedReport::ShelfLife { request } => {
                let analysis = ShelfLifeAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::ShelfLife { request, analysis }
            }
            saved @ (SavedReport::Growth { .. }
            | SavedReport::Recurrence { .. }
            | SavedReport::LifeDistribution { .. }
            | SavedReport::CompetingRisks { .. }
            | SavedReport::Warranty { .. }
            | SavedReport::ExponentialSmoothing { .. }
            | SavedReport::Arima { .. }
            | SavedReport::PrincipalCoordinates { .. }
            | SavedReport::Pharmacokinetics { .. }
            | SavedReport::Fatigue { .. }
            | SavedReport::LifeStress { .. }
            | SavedReport::Degradation { .. }
            | SavedReport::TestPlan { .. }
            | SavedReport::Bootstrap { .. }
            | SavedReport::PermutationTest { .. }) => {
                let result = app_model::run_report(&saved, table, rows).map_err(|e| e.to_string());
                Self::Reliability { saved, result }
            }
            SavedReport::Stepwise { request, options } => {
                let analysis = StepwiseAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::Stepwise {
                    request,
                    options,
                    analysis,
                    settings: Vec::new(),
                    surface: SurfaceProfilerState::default(),
                }
            }
            SavedReport::BestSubsets { request } => {
                let analysis = BestSubsetsAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::BestSubsets { request, analysis }
            }
            SavedReport::DistributionFit { request } => {
                let analysis = DistributionFitAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::DistributionFit { request, analysis }
            }
            SavedReport::Mixture { request, profilers } => {
                let analysis = MixtureAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::Mixture { request, profilers, analysis, settings: Vec::new() }
            }
            saved @ (SavedReport::OneSample { .. }
            | SavedReport::Groups { .. }
            | SavedReport::Paired { .. }
            | SavedReport::Proportions { .. }
            | SavedReport::Rates { .. }
            | SavedReport::Power { .. }
            | SavedReport::AttributeCapability { .. }
            | SavedReport::BetweenWithinCapability { .. }
            | SavedReport::AcceptanceSampling { .. }
            | SavedReport::GaugeLinearity { .. }
            | SavedReport::GaugeRepeatability { .. }
            | SavedReport::IntraclassCorrelation { .. }
            | SavedReport::AttributeAgreement { .. }
            | SavedReport::Bayesian { .. }
            | SavedReport::Signal { .. }
            | SavedReport::ImagePlot { .. }
            | SavedReport::AttributeGauge { .. }
            | SavedReport::PcaChart { .. }
            | SavedReport::MonitoringLimits { .. }
            | SavedReport::MultiwayTable { .. }
            | SavedReport::StratifiedTables { .. }
            | SavedReport::SignalToNoise { .. }
            | SavedReport::DeclaredDesign { .. }
            | SavedReport::DesignEvaluation { .. }
            | SavedReport::FactorialAnalysis { .. }
            | SavedReport::SequentialOptimization { .. }
            | SavedReport::GoodnessOfFit { .. }
            | SavedReport::Crossover { .. }
            | SavedReport::FactorialRepeated { .. }
            | SavedReport::Nested { .. }
            | SavedReport::DistributionCalculator { .. }
            | SavedReport::AdjustPValues { .. }
            | SavedReport::AreaUnderCurve { .. }
            | SavedReport::ManyTests { .. }
            | SavedReport::TreatmentEffects { .. }
            | SavedReport::Screening { .. }
            | SavedReport::MultipleResponseItems { .. }) => {
                let analysis = run_comparison(&saved, table, rows);
                Self::Comparison { computed_with: saved.clone(), current: saved, analysis }
            }
            SavedReport::Graph { spec } => Self::Graph { spec, view: Box::default() },
            SavedReport::Scatter3d { request, options } => {
                let analysis = Scatter3dAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::Scatter3d { request, options, analysis, view: Default::default() }
            }
            SavedReport::DataSurface { request, options } => {
                let analysis = DataSurfaceAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                Self::DataSurface {
                    computed_with: request,
                    request,
                    options,
                    analysis,
                    surface: SurfaceProfilerState::default(),
                }
            }
            SavedReport::MultipleResponses { request } => {
                let analysis = MultipleResponsesAnalysis::run(table, rows, &request).map_err(|e| e.to_string());
                let mut view = Box::<SeveralView>::default();
                view.refitted(&analysis, (table.revision(), rows.exclusion_revision()));
                Self::MultipleResponses {
                    computed_with: several_responses_view::without_settings(&request),
                    request: *request,
                    analysis,
                    view,
                }
            }
        }
    }

    /// What to save so that the report can be re-created.
    fn saved(&self) -> SavedReport {
        match self {
            Self::Waiting { saved } => (**saved).clone(),
            Self::Failed { saved, .. } => saved.clone(),
            Self::Linear { spec, options, .. } => SavedReport::Linear { spec: spec.clone(), options: *options },
            Self::Chart { request, options, .. } => {
                SavedReport::ControlChart { request: request.clone(), options: *options }
            }
            Self::Gauge { request, options, .. } => SavedReport::Gauge { request: request.clone(), options: *options },
            Self::Distribution { request, options, .. } => {
                SavedReport::Distribution { request: request.clone(), options: *options }
            }
            Self::Bivariate { request, .. } => SavedReport::Bivariate { request: request.clone() },
            Self::Pca { request, .. } => SavedReport::Pca { request: request.clone() },
            Self::Cluster { request, .. } => SavedReport::Cluster { request: request.clone() },
            Self::Mixed { request, profilers, .. } => {
                SavedReport::Mixed { request: request.clone(), profilers: *profilers }
            }
            Self::Glm { request, profilers, .. } => {
                SavedReport::Glm { request: request.clone(), profilers: *profilers }
            }
            Self::ProportionalHazards { request, profilers, .. } => {
                SavedReport::ProportionalHazards { request: request.clone(), profilers: *profilers }
            }
            Self::SurvivalRegression { request, profilers, .. } => {
                SavedReport::SurvivalRegression { request: request.clone(), profilers: *profilers }
            }
            Self::Regularized { request, profilers, .. } => {
                SavedReport::Regularized { request: request.clone(), profilers: *profilers }
            }
            Self::DecisionTree { request, profilers, .. } => {
                SavedReport::DecisionTree { request: request.clone(), profilers: *profilers }
            }
            Self::RandomForest { request, profilers, .. } => {
                SavedReport::RandomForest { request: request.clone(), profilers: *profilers }
            }
            Self::BoostedTrees { request, profilers, .. } => {
                SavedReport::BoostedTrees { request: request.clone(), profilers: *profilers }
            }
            Self::PartialLeastSquares { request, profilers, .. } => {
                SavedReport::PartialLeastSquares { request: request.clone(), profilers: *profilers }
            }
            Self::SupportVectorMachine { request, profilers, .. } => {
                SavedReport::SupportVectorMachine { request: request.clone(), profilers: *profilers }
            }
            Self::Mars { request, profilers, .. } => {
                SavedReport::Mars { request: request.clone(), profilers: *profilers }
            }
            Self::NeuralNetwork { request, profilers, .. } => {
                SavedReport::NeuralNetwork { request: request.clone(), profilers: *profilers }
            }
            Self::GaussianProcess { request, profilers, .. } => {
                SavedReport::GaussianProcess { request: request.clone(), profilers: *profilers }
            }
            Self::FunctionalData { request, profilers, .. } => {
                SavedReport::FunctionalData { request: request.clone(), profilers: *profilers }
            }
            Self::ModelComparison { request, .. } => SavedReport::ModelComparison { request: request.clone() },
            Self::ModelSearch { request, .. } => SavedReport::ModelSearch { request: request.clone() },
            Self::Correspondence { request, .. } => SavedReport::Correspondence { request: request.clone() },
            Self::Correlations { request, .. } => SavedReport::Correlations { request: request.clone() },
            Self::OutlierDistances { request, .. } => SavedReport::OutlierDistances { request: request.clone() },
            Self::ItemAnalysis { request, .. } => SavedReport::ItemAnalysis { request: request.clone() },
            Self::Manova { request, .. } => SavedReport::Manova { request: request.clone() },
            Self::FactorAnalysis { request, .. } => SavedReport::FactorAnalysis { request: request.clone() },
            Self::DiscriminantAnalysis { request, .. } => {
                SavedReport::DiscriminantAnalysis { request: request.clone() }
            }
            Self::GaussianMixture { request, .. } => SavedReport::GaussianMixture { request: request.clone() },
            Self::LatentClass { request, .. } => SavedReport::LatentClass { request: request.clone() },
            Self::TextExploration { request, .. } => SavedReport::TextExploration { request: request.clone() },
            Self::TopicModel { request, .. } => SavedReport::TopicModel { request: request.clone() },
            Self::Sentiment { request, .. } => SavedReport::Sentiment { request: request.clone() },
            Self::Embedding { request, .. } => SavedReport::Embedding { request: request.clone() },
            Self::Imputation { request, .. } => SavedReport::Imputation { request: request.clone() },
            Self::SeriesExplore { request, .. } => SavedReport::SeriesExplore { request: request.clone() },
            Self::SignalProcessing { request, .. } => SavedReport::SignalProcessing { request: request.clone() },
            Self::ImageAnalysis { request, .. } => SavedReport::ImageAnalysis { request: request.clone() },
            Self::Survival { request, options, .. } => {
                SavedReport::Survival { request: request.clone(), options: *options }
            }
            Self::Surface { request, options, .. } => {
                SavedReport::Surface { request: request.clone(), options: *options }
            }
            Self::TimeWeighted { request, .. } => SavedReport::TimeWeighted { request: *request },
            Self::Capability { request, .. } => SavedReport::Capability { request: *request },
            Self::Attribute { request, .. } => SavedReport::Attribute { request: *request },
            Self::Pareto { request, options, .. } => SavedReport::Pareto { request: *request, options: *options },
            Self::CauseEffect { request, .. } => SavedReport::CauseEffect { request: request.clone() },
            Self::MultiVari { request, options, .. } => {
                SavedReport::MultiVari { request: request.clone(), options: *options }
            }
            Self::MultivariateChart { request, .. } => SavedReport::MultivariateChart { request: request.clone() },
            Self::Anom { request, .. } => SavedReport::Anom { request: *request },
            Self::DoseResponse { request, .. } => SavedReport::DoseResponse { request: request.clone() },
            Self::CategoricalLogistic { request, .. } => SavedReport::CategoricalLogistic { request: request.clone() },
            Self::Nonlinear { request, profilers, .. } => {
                SavedReport::Nonlinear { request: request.clone(), profilers: *profilers }
            }
            Self::MethodComparison { request, .. } => SavedReport::MethodComparison { request: request.clone() },
            Self::ShelfLife { request, .. } => SavedReport::ShelfLife { request: request.clone() },
            Self::Reliability { saved, .. } => saved.clone(),
            Self::Stepwise { request, options, .. } => {
                SavedReport::Stepwise { request: request.clone(), options: *options }
            }
            Self::BestSubsets { request, .. } => SavedReport::BestSubsets { request: request.clone() },
            Self::DistributionFit { request, .. } => SavedReport::DistributionFit { request: request.clone() },
            Self::Mixture { request, profilers, .. } => {
                SavedReport::Mixture { request: request.clone(), profilers: *profilers }
            }
            Self::Comparison { current, .. } => current.clone(),
            Self::Graph { spec, .. } => SavedReport::Graph { spec: spec.clone() },
            Self::MultipleResponses { request, .. } => {
                SavedReport::MultipleResponses { request: Box::new(request.clone()) }
            }
            Self::Scatter3d { request, options, .. } => {
                SavedReport::Scatter3d { request: request.clone(), options: *options }
            }
            Self::DataSurface { request, options, .. } => {
                SavedReport::DataSurface { request: *request, options: *options }
            }
        }
    }

    fn recompute(&mut self, table: &DataTable, rows: &RowStateManager) {
        match self {
            Self::Waiting { saved } => *self = Self::compute((**saved).clone(), table, rows),
            Self::Failed { saved, .. } => *self = Self::compute(saved.clone(), table, rows),
            Self::Linear { spec, analysis, settings, .. } => {
                *analysis = LinearModelAnalysis::run(table, rows, spec).map_err(|e| e.to_string());
                // Keep the explorer settings while they still fit the model.
                if let Ok(a) = analysis {
                    if !a.settings_fit(settings) {
                        *settings = a.default_settings();
                    }
                }
            }
            Self::Chart { request, computed_with, analysis, .. } => {
                *analysis = ControlChartAnalysis::run(table, rows, request).map_err(|e| e.to_string());
                *computed_with = request.clone();
            }
            Self::Gauge { request, computed_with, analysis, .. } => {
                *analysis = GaugeAnalysis::run(table, rows, request).map_err(|e| e.to_string());
                *computed_with = request.clone();
            }
            Self::Distribution { request, analysis, .. } => {
                *analysis = DistributionAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::Bivariate { request, analysis } => {
                *analysis = BivariateAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::Pca { request, analysis } => {
                *analysis = PcaAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::Cluster { request, analysis } => {
                *analysis = ClusterAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::Mixed { request, analysis, .. } => {
                *analysis = MixedAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::Glm { request, analysis, .. } => {
                *analysis = GlmAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::ProportionalHazards { request, analysis, .. } => {
                *analysis =
                    app_model::ProportionalHazardsAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::SurvivalRegression { request, analysis, .. } => {
                *analysis = app_model::SurvivalRegressionAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::Regularized { request, analysis, .. } => {
                *analysis = RegularizedAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::DecisionTree { request, analysis, .. } => {
                *analysis = TreeAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::RandomForest { request, analysis, .. } => {
                *analysis = ForestAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::BoostedTrees { request, analysis, .. } => {
                *analysis = BoostAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::PartialLeastSquares { request, analysis, .. } => {
                *analysis = PlsAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::SupportVectorMachine { request, analysis, .. } => {
                *analysis = SvmAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::Mars { request, analysis, .. } => {
                *analysis = MarsAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::NeuralNetwork { request, analysis, .. } => {
                *analysis = NeuralAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::GaussianProcess { request, analysis, .. } => {
                *analysis = app_model::GaussianProcessAnalysis::run(table, rows, request)
                    .map(Box::new)
                    .map_err(|e| e.to_string());
            }
            Self::FunctionalData { request, analysis, .. } => {
                *analysis =
                    app_model::FunctionalAnalysis::run(table, rows, request).map(Box::new).map_err(|e| e.to_string());
            }
            Self::ModelComparison { request, analysis } => {
                *analysis = ModelComparison::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::ModelSearch { request, analysis } => {
                *analysis = ModelSearch::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::Correlations { request, analysis } => {
                *analysis = app_model::CorrelationAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::OutlierDistances { request, analysis } => {
                *analysis = app_model::OutlierAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::Manova { request, analysis } => {
                *analysis = app_model::ManovaAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::FactorAnalysis { request, analysis } => {
                *analysis = app_model::FactorAnalysisReport::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::DiscriminantAnalysis { request, analysis } => {
                *analysis = app_model::DiscriminantAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::ItemAnalysis { request, analysis } => {
                *analysis = app_model::ItemAnalysisReport::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::GaussianMixture { request, analysis } => {
                *analysis = app_model::GaussianMixtureAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::LatentClass { request, analysis } => {
                *analysis = app_model::LatentClassAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::TextExploration { request, analysis } => {
                *analysis = app_model::TextExploration::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::TopicModel { request, analysis } => {
                *analysis = app_model::TopicModelAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::Sentiment { request, analysis } => {
                *analysis = app_model::SentimentAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::Embedding { request, analysis } => {
                *analysis = app_model::EmbeddingAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::SeriesExplore { request, computed_with, analysis } => {
                *analysis = app_model::SeriesExploreAnalysis::run(table, rows, request).map_err(|e| e.to_string());
                *computed_with = request.clone();
            }
            Self::SignalProcessing { request, computed_with, analysis } => {
                *analysis = app_model::SignalAnalysis::run(table, rows, request).map_err(|e| e.to_string());
                *computed_with = request.clone();
            }
            Self::ImageAnalysis { request, computed_with, analysis } => {
                *analysis = app_model::ImageAnalysis::run(table, rows, request).map_err(|e| e.to_string());
                *computed_with = request.clone();
            }
            Self::Imputation { request, analysis } => {
                *analysis = app_model::ImputationReport::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::Correspondence { request, analysis } => {
                *analysis = CorrespondenceReport::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::Survival { request, analysis, .. } => {
                *analysis = SurvivalAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::Surface { request, analysis, .. } => {
                *analysis = SurfaceAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::TimeWeighted { request, analysis } => {
                *analysis = TimeWeightedAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::Capability { request, computed_with, analysis } => {
                *analysis = CapabilityAnalysis::run(table, rows, request).map_err(|e| e.to_string());
                *computed_with = *request;
            }
            Self::Attribute { request, computed_with, analysis } => {
                *analysis = AttributeChartAnalysis::run(table, rows, request).map_err(|e| e.to_string());
                *computed_with = *request;
            }
            Self::Pareto { request, analysis, .. } => {
                *analysis = ParetoAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::CauseEffect { request, analysis } => {
                *analysis = CauseEffectAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::MultiVari { request, analysis, .. } => {
                *analysis = MultiVariAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::MultivariateChart { request, analysis } => {
                *analysis = MultivariateChartAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::Anom { request, analysis } => {
                *analysis = AnomAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::DoseResponse { request, analysis } => {
                *analysis = DoseResponseAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::CategoricalLogistic { request, analysis } => {
                *analysis =
                    app_model::CategoricalLogisticAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::Nonlinear { request, analysis, .. } => {
                *analysis = NonlinearAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::MethodComparison { request, analysis } => {
                *analysis = MethodComparisonAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::ShelfLife { request, analysis } => {
                *analysis = ShelfLifeAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::Reliability { saved, result } => {
                *result = app_model::run_report(saved, table, rows).map_err(|e| e.to_string());
            }
            Self::Stepwise { request, analysis, .. } => {
                *analysis = StepwiseAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::BestSubsets { request, analysis } => {
                *analysis = BestSubsetsAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::DistributionFit { request, analysis } => {
                *analysis = DistributionFitAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::Mixture { request, analysis, .. } => {
                *analysis = MixtureAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::Comparison { current, computed_with, analysis } => {
                *analysis = run_comparison(current, table, rows);
                *computed_with = current.clone();
            }
            // The canvas rebuilds itself whenever the data or row states change.
            Self::Graph { .. } => {}
            Self::Scatter3d { request, analysis, .. } => {
                *analysis = Scatter3dAnalysis::run(table, rows, request).map_err(|e| e.to_string());
            }
            Self::DataSurface { request, computed_with, analysis, .. } => {
                *analysis = DataSurfaceAnalysis::run(table, rows, request).map_err(|e| e.to_string());
                *computed_with = *request;
            }
            Self::MultipleResponses { request, computed_with, analysis, view } => {
                *analysis = MultipleResponsesAnalysis::run(table, rows, request).map_err(|e| e.to_string());
                *computed_with = several_responses_view::without_settings(request);
                view.refitted(analysis, (table.revision(), rows.exclusion_revision()));
            }
        }
    }

    /// `true` if a setting that changes the computation was edited.
    /// Take the results of `fresh` (the same request computed again, in the
    /// background) and keep this body's view state: the explorer and
    /// profiler settings, 3-D views and the several-responses view, as
    /// [`Self::recompute`] does. `at` is the (data, exclusion) revision
    /// `fresh` was computed at.
    fn adopt(&mut self, fresh: Self, at: (u64, u64)) {
        match (self, fresh) {
            (Self::Linear { analysis, settings, .. }, Self::Linear { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
                // Keep the explorer settings while they still fit the model.
                if let Ok(a) = analysis {
                    if !a.settings_fit(settings) {
                        *settings = a.default_settings();
                    }
                }
            }
            (
                Self::MultipleResponses { computed_with, analysis, view, .. },
                Self::MultipleResponses { computed_with: fresh_computed_with, analysis: fresh_analysis, .. },
            ) => {
                *analysis = fresh_analysis;
                *computed_with = fresh_computed_with;
                view.refitted(analysis, at);
            }
            (Self::Graph { .. }, Self::Graph { .. }) => {}
            (Self::Chart { computed_with, analysis, .. }, Self::Chart { computed_with: fresh_computed_with, analysis: fresh_analysis, .. }) => {
                *computed_with = fresh_computed_with; *analysis = fresh_analysis;
            }
            (Self::Gauge { computed_with, analysis, .. }, Self::Gauge { computed_with: fresh_computed_with, analysis: fresh_analysis, .. }) => {
                *computed_with = fresh_computed_with; *analysis = fresh_analysis;
            }
            (Self::Distribution { analysis, .. }, Self::Distribution { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::Bivariate { analysis, .. }, Self::Bivariate { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::Pca { analysis, .. }, Self::Pca { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::Cluster { analysis, .. }, Self::Cluster { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::Mixed { analysis, .. }, Self::Mixed { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::Glm { analysis, .. }, Self::Glm { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::ProportionalHazards { analysis, .. }, Self::ProportionalHazards { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::SurvivalRegression { analysis, .. }, Self::SurvivalRegression { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::Regularized { analysis, .. }, Self::Regularized { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::DecisionTree { analysis, .. }, Self::DecisionTree { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::RandomForest { analysis, .. }, Self::RandomForest { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::BoostedTrees { analysis, .. }, Self::BoostedTrees { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::PartialLeastSquares { analysis, .. }, Self::PartialLeastSquares { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::SupportVectorMachine { analysis, .. }, Self::SupportVectorMachine { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::Mars { analysis, .. }, Self::Mars { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::NeuralNetwork { analysis, .. }, Self::NeuralNetwork { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::GaussianProcess { analysis, .. }, Self::GaussianProcess { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::FunctionalData { analysis, .. }, Self::FunctionalData { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::ModelComparison { analysis, .. }, Self::ModelComparison { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::ModelSearch { analysis, .. }, Self::ModelSearch { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::Correlations { analysis, .. }, Self::Correlations { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::OutlierDistances { analysis, .. }, Self::OutlierDistances { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::Manova { analysis, .. }, Self::Manova { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::FactorAnalysis { analysis, .. }, Self::FactorAnalysis { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::DiscriminantAnalysis { analysis, .. }, Self::DiscriminantAnalysis { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::ItemAnalysis { analysis, .. }, Self::ItemAnalysis { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::GaussianMixture { analysis, .. }, Self::GaussianMixture { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::LatentClass { analysis, .. }, Self::LatentClass { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::TextExploration { analysis, .. }, Self::TextExploration { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::TopicModel { analysis, .. }, Self::TopicModel { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::Sentiment { analysis, .. }, Self::Sentiment { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::Embedding { analysis, .. }, Self::Embedding { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::SeriesExplore { computed_with, analysis, .. }, Self::SeriesExplore { computed_with: fresh_computed_with, analysis: fresh_analysis, .. }) => {
                *computed_with = fresh_computed_with; *analysis = fresh_analysis;
            }
            (Self::SignalProcessing { computed_with, analysis, .. }, Self::SignalProcessing { computed_with: fresh_computed_with, analysis: fresh_analysis, .. }) => {
                *computed_with = fresh_computed_with; *analysis = fresh_analysis;
            }
            (Self::ImageAnalysis { computed_with, analysis, .. }, Self::ImageAnalysis { computed_with: fresh_computed_with, analysis: fresh_analysis, .. }) => {
                *computed_with = fresh_computed_with; *analysis = fresh_analysis;
            }
            (Self::Imputation { analysis, .. }, Self::Imputation { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::Correspondence { analysis, .. }, Self::Correspondence { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::Survival { analysis, .. }, Self::Survival { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::Surface { analysis, .. }, Self::Surface { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::TimeWeighted { analysis, .. }, Self::TimeWeighted { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::Capability { computed_with, analysis, .. }, Self::Capability { computed_with: fresh_computed_with, analysis: fresh_analysis, .. }) => {
                *computed_with = fresh_computed_with; *analysis = fresh_analysis;
            }
            (Self::Attribute { computed_with, analysis, .. }, Self::Attribute { computed_with: fresh_computed_with, analysis: fresh_analysis, .. }) => {
                *computed_with = fresh_computed_with; *analysis = fresh_analysis;
            }
            (Self::Pareto { analysis, .. }, Self::Pareto { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::CauseEffect { analysis, .. }, Self::CauseEffect { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::MultiVari { analysis, .. }, Self::MultiVari { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::MultivariateChart { analysis, .. }, Self::MultivariateChart { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::Anom { analysis, .. }, Self::Anom { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::DoseResponse { analysis, .. }, Self::DoseResponse { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::CategoricalLogistic { analysis, .. }, Self::CategoricalLogistic { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::Nonlinear { analysis, .. }, Self::Nonlinear { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::MethodComparison { analysis, .. }, Self::MethodComparison { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::ShelfLife { analysis, .. }, Self::ShelfLife { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::Reliability { result, .. }, Self::Reliability { result: fresh_result, .. }) => {
                *result = fresh_result;
            }
            (Self::Stepwise { analysis, .. }, Self::Stepwise { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::BestSubsets { analysis, .. }, Self::BestSubsets { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::DistributionFit { analysis, .. }, Self::DistributionFit { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::Mixture { analysis, .. }, Self::Mixture { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::Comparison { computed_with, analysis, .. }, Self::Comparison { computed_with: fresh_computed_with, analysis: fresh_analysis, .. }) => {
                *computed_with = fresh_computed_with; *analysis = fresh_analysis;
            }
            (Self::Scatter3d { analysis, .. }, Self::Scatter3d { analysis: fresh_analysis, .. }) => {
                *analysis = fresh_analysis;
            }
            (Self::DataSurface { computed_with, analysis, .. }, Self::DataSurface { computed_with: fresh_computed_with, analysis: fresh_analysis, .. }) => {
                *computed_with = fresh_computed_with; *analysis = fresh_analysis;
            }
            // A different kind of report (not expected: the request is the same).
            (this, fresh) => *this = fresh,
        }
    }

    fn settings_changed(&self) -> bool {
        matches!(self, Self::Gauge { request, computed_with, .. } if request != computed_with)
            || matches!(self, Self::Chart { request, computed_with, .. } if request != computed_with)
            || matches!(self, Self::Capability { request, computed_with, .. } if request != computed_with)
            || matches!(self, Self::Attribute { request, computed_with, .. } if request != computed_with)
            || matches!(self, Self::Comparison { current, computed_with, .. } if current != computed_with)
            || matches!(self, Self::DataSurface { request, computed_with, .. } if request != computed_with)
            || matches!(self, Self::SeriesExplore { request, computed_with, .. } if request != computed_with)
            || matches!(self, Self::SignalProcessing { request, computed_with, .. } if request != computed_with)
            || matches!(self, Self::ImageAnalysis { request, computed_with, .. } if request != computed_with)
            || matches!(self, Self::MultipleResponses { request, computed_with, .. }
                if several_responses_view::without_settings(request) != *computed_with)
    }

    fn title(&self) -> String {
        match self {
            Self::Failed { .. } => "Analysis (stopped with an internal error)".to_owned(),
            Self::Linear { analysis: Ok(a), .. } => a.title(),
            Self::Linear { analysis: Err(_), .. } => "Linear Model (could not be computed)".to_owned(),
            Self::Chart { analysis: Ok(a), .. } => a.title(),
            Self::Chart { analysis: Err(_), .. } => "Control Chart (could not be computed)".to_owned(),
            Self::Gauge { analysis: Ok(a), .. } => a.title(),
            Self::Gauge { analysis: Err(_), .. } => "Gauge R&R (could not be computed)".to_owned(),
            Self::Distribution { analysis: Ok(a), .. } => a.title(),
            Self::Distribution { analysis: Err(_), .. } => "Distribution (could not be computed)".to_owned(),
            Self::Bivariate { analysis: Ok(a), .. } => a.title(),
            Self::Bivariate { analysis: Err(_), .. } => "Bivariate (could not be computed)".to_owned(),
            Self::Pca { analysis: Ok(a), .. } => a.title(),
            Self::Pca { analysis: Err(_), .. } => "Principal Components (could not be computed)".to_owned(),
            Self::Cluster { analysis: Ok(a), .. } => a.title(),
            Self::Cluster { analysis: Err(_), .. } => "Clusters (could not be computed)".to_owned(),
            Self::Mixed { analysis: Ok(a), .. } => a.title(),
            Self::Mixed { analysis: Err(_), .. } => "Mixed Model (could not be computed)".to_owned(),
            Self::Glm { analysis: Ok(a), .. } => a.title(),
            Self::Glm { analysis: Err(_), .. } => "Generalized Linear Model (could not be computed)".to_owned(),
            Self::ProportionalHazards { analysis: Ok(a), .. } => a.title(),
            Self::ProportionalHazards { analysis: Err(_), .. } => {
                "Proportional Hazards (could not be computed)".to_owned()
            }
            Self::SurvivalRegression { analysis: Ok(a), .. } => a.title(),
            Self::SurvivalRegression { analysis: Err(_), .. } => {
                "Parametric Survival Regression (could not be computed)".to_owned()
            }
            Self::Regularized { analysis: Ok(a), .. } => a.title(),
            Self::Regularized { analysis: Err(_), .. } => "Penalized Regression (could not be computed)".to_owned(),
            Self::DecisionTree { analysis: Ok(a), .. } => a.title(),
            Self::DecisionTree { analysis: Err(_), .. } => "Decision Tree (could not be grown)".to_owned(),
            Self::RandomForest { analysis: Ok(a), .. } => a.title(),
            Self::RandomForest { analysis: Err(_), .. } => "Random forest (could not be grown)".to_owned(),
            Self::BoostedTrees { analysis: Ok(a), .. } => a.title(),
            Self::BoostedTrees { analysis: Err(_), .. } => "Boosted Trees (could not be computed)".to_owned(),
            Self::PartialLeastSquares { analysis: Ok(a), .. } => a.title(),
            Self::SupportVectorMachine { analysis: Ok(a), .. } => a.title(),
            Self::PartialLeastSquares { analysis: Err(_), .. } => {
                "Partial Least Squares (could not be computed)".to_owned()
            }
            Self::SupportVectorMachine { analysis: Err(_), .. } => {
                "Support Vector Machine (could not be computed)".to_owned()
            }
            Self::Mars { analysis: Ok(a), .. } => a.title(),
            Self::Mars { analysis: Err(_), .. } => "Adaptive splines (could not be computed)".to_owned(),
            Self::NeuralNetwork { analysis: Ok(a), .. } => a.title(),
            Self::NeuralNetwork { analysis: Err(_), .. } => "Neural Network (could not be computed)".to_owned(),
            Self::GaussianProcess { analysis: Ok(a), .. } => a.title(),
            Self::GaussianProcess { analysis: Err(_), .. } => "Gaussian Process (could not be computed)".to_owned(),
            Self::FunctionalData { analysis: Ok(a), .. } => a.title(),
            Self::FunctionalData { analysis: Err(_), .. } => "Functional Data (could not be computed)".to_owned(),
            Self::ModelComparison { analysis: Ok(a), .. } => a.title(),
            Self::ModelComparison { analysis: Err(_), .. } => "Model Comparison (could not be computed)".to_owned(),
            Self::ModelSearch { analysis: Ok(a), .. } => a.title(),
            Self::ModelSearch { analysis: Err(_), .. } => "Model Search (could not be computed)".to_owned(),
            Self::Correlations { analysis: Ok(a), .. } => a.title(),
            Self::Correlations { analysis: Err(_), .. } => "Correlations (could not be computed)".to_owned(),
            Self::OutlierDistances { analysis: Ok(a), .. } => a.title(),
            Self::OutlierDistances { analysis: Err(_), .. } => "Outlier Distances (could not be computed)".to_owned(),
            Self::ItemAnalysis { analysis: Ok(a), .. } => a.title(),
            Self::ItemAnalysis { analysis: Err(_), .. } => "Item Analysis (could not be computed)".to_owned(),
            Self::Manova { analysis: Ok(a), .. } => a.title(),
            Self::Manova { analysis: Err(_), .. } => "MANOVA (could not be computed)".to_owned(),
            Self::FactorAnalysis { analysis: Ok(a), .. } => a.title(),
            Self::FactorAnalysis { analysis: Err(_), .. } => "Factor Analysis (could not be computed)".to_owned(),
            Self::DiscriminantAnalysis { analysis: Ok(a), .. } => a.title(),
            Self::DiscriminantAnalysis { analysis: Err(_), .. } => {
                "Discriminant Analysis (could not be computed)".to_owned()
            }
            Self::GaussianMixture { analysis: Ok(a), .. } => a.title(),
            Self::GaussianMixture { analysis: Err(_), .. } => "Gaussian Mixture (could not be fitted)".to_owned(),
            Self::LatentClass { analysis: Ok(a), .. } => a.title(),
            Self::LatentClass { analysis: Err(_), .. } => "Latent Class Analysis (could not be fitted)".to_owned(),
            Self::TextExploration { analysis: Ok(a), .. } => a.title(),
            Self::TextExploration { analysis: Err(_), .. } => "Text Exploration (could not be computed)".to_owned(),
            Self::TopicModel { analysis: Ok(a), .. } => a.title(),
            Self::TopicModel { analysis: Err(_), .. } => "Topic Model (could not be fitted)".to_owned(),
            Self::Sentiment { analysis: Ok(a), .. } => a.title(),
            Self::Sentiment { analysis: Err(_), .. } => "Sentiment Scores (could not be computed)".to_owned(),
            Self::Embedding { analysis: Ok(a), .. } => a.title(),
            Self::Embedding { request, analysis: Err(_) } => {
                format!("{} (could not be computed)", request.method.label())
            }
            Self::Imputation { analysis: Ok(a), .. } => a.title(),
            Self::SeriesExplore { analysis: Ok(a), .. } => a.title(),
            Self::SeriesExplore { request, analysis: Err(_), .. } => {
                format!("{} (could not be computed)", request.method.label())
            }
            Self::SignalProcessing { analysis: Ok(a), .. } => a.title(),
            Self::SignalProcessing { analysis: Err(_), .. } => "Signal Analysis (could not be computed)".to_owned(),
            Self::ImageAnalysis { analysis: Ok(a), .. } => a.title(),
            Self::ImageAnalysis { analysis: Err(_), .. } => "Image Analysis (could not be computed)".to_owned(),
            Self::Imputation { analysis: Err(_), .. } => "Missing-Value Imputation (could not be computed)".to_owned(),
            Self::Correspondence { analysis: Ok(a), .. } => a.title(),
            Self::Correspondence { analysis: Err(_), .. } => {
                "Correspondence Analysis (could not be computed)".to_owned()
            }
            Self::Survival { analysis: Ok(a), .. } => a.title(),
            Self::Survival { analysis: Err(_), .. } => "Survival (could not be computed)".to_owned(),
            Self::Surface { analysis: Ok(a), .. } => a.title(),
            Self::Surface { analysis: Err(_), .. } => "Response Surface (could not be fitted)".to_owned(),
            Self::TimeWeighted { analysis: Ok(a), .. } => a.title(),
            Self::TimeWeighted { analysis: Err(_), .. } => "EWMA / CUSUM Chart (could not be computed)".to_owned(),
            Self::Capability { analysis: Ok(a), .. } => a.title(),
            Self::Capability { analysis: Err(_), .. } => "Process Capability (could not be computed)".to_owned(),
            Self::Attribute { analysis: Ok(a), .. } => a.title(),
            Self::Attribute { analysis: Err(_), .. } => "Attribute Chart (could not be computed)".to_owned(),
            Self::Pareto { analysis: Ok(a), .. } => a.title(),
            Self::Pareto { analysis: Err(_), .. } => "Pareto Chart (could not be computed)".to_owned(),
            Self::CauseEffect { analysis: Ok(a), .. } => a.title(),
            Self::CauseEffect { analysis: Err(_), .. } => "Cause and Effect (could not be drawn)".to_owned(),
            Self::MultiVari { analysis: Ok(a), .. } => a.title(),
            Self::MultiVari { analysis: Err(_), .. } => "Multi-Vari Chart (could not be computed)".to_owned(),
            Self::MultivariateChart { analysis: Ok(a), .. } => a.title(),
            Self::MultivariateChart { analysis: Err(_), .. } => {
                "Multivariate Control Chart (could not be computed)".to_owned()
            }
            Self::Anom { analysis: Ok(a), .. } => a.title(),
            Self::Anom { analysis: Err(_), .. } => "Analysis of Means (could not be computed)".to_owned(),
            Self::DoseResponse { analysis: Ok(a), .. } => a.title(),
            Self::DoseResponse { analysis: Err(_), .. } => "Dose–Response Analysis (could not be computed)".to_owned(),
            Self::CategoricalLogistic { analysis: Ok(a), .. } => a.title(),
            Self::CategoricalLogistic { request, analysis: Err(_) } => {
                format!("{} (could not be computed)", request.model.label())
            }
            Self::Nonlinear { analysis: Ok(a), .. } => a.title(),
            Self::Nonlinear { analysis: Err(_), .. } => "Nonlinear Regression (could not be computed)".to_owned(),
            Self::MethodComparison { analysis: Ok(a), .. } => a.title(),
            Self::MethodComparison { analysis: Err(_), .. } => "Method Comparison (could not be computed)".to_owned(),
            Self::ShelfLife { analysis: Ok(a), .. } => a.title(),
            Self::ShelfLife { analysis: Err(_), .. } => "Shelf Life (could not be computed)".to_owned(),
            Self::Reliability { result: Ok(c), .. } => c.title.clone(),
            Self::Reliability { saved, result: Err(_) } => match saved {
                SavedReport::Growth { .. } => "Reliability Growth (could not be computed)",
                SavedReport::Recurrence { .. } => "Repairable Systems (could not be computed)",
                SavedReport::LifeDistribution { .. } => "Life Distributions (could not be computed)",
                SavedReport::CompetingRisks { .. } => "Competing Risks (could not be computed)",
                SavedReport::Warranty { .. } => "Warranty Returns (could not be computed)",
                SavedReport::ExponentialSmoothing { .. } => "Exponential Smoothing (could not be computed)",
                SavedReport::Arima { .. } => "ARIMA (could not be computed)",
                SavedReport::PrincipalCoordinates { .. } => "Principal Coordinates (could not be computed)",
                SavedReport::Pharmacokinetics { .. } => "Pharmacokinetic Parameters (could not be computed)",
                SavedReport::Fatigue { .. } => "Fatigue S–N Curves (could not be computed)",
                SavedReport::LifeStress { .. } => "Life–Stress Model (could not be computed)",
                SavedReport::Degradation { .. } => "Degradation (could not be computed)",
                SavedReport::Bootstrap { .. } => "Bootstrap (could not be computed)",
                SavedReport::PermutationTest { .. } => "Permutation Test (could not be computed)",
                _ => "Reliability Test Plan (could not be computed)",
            }
            .to_owned(),
            Self::Stepwise { analysis: Ok(a), .. } => a.title(),
            Self::Stepwise { analysis: Err(_), .. } => "Stepwise Selection (could not be run)".to_owned(),
            Self::BestSubsets { analysis: Ok(a), .. } => a.title(),
            Self::BestSubsets { analysis: Err(_), .. } => "Best Subsets (could not be run)".to_owned(),
            Self::DistributionFit { analysis: Ok(a), .. } => a.title(),
            Self::DistributionFit { analysis: Err(_), .. } => {
                "Distribution Identification (could not be run)".to_owned()
            }
            Self::Mixture { analysis: Ok(a), .. } => a.title(),
            Self::Mixture { analysis: Err(_), .. } => "Mixture Model (could not be fitted)".to_owned(),
            Self::Comparison { analysis: Ok(a), .. } => a.title(),
            Self::Comparison { analysis: Err(_), current, .. } => ComparisonReport::failed_title(current),
            Self::Graph { spec, .. } => graph_view::title(spec),
            Self::Waiting { .. } => "Computing report…".to_owned(),
            Self::Scatter3d { analysis: Ok(a), .. } => a.title(),
            Self::Scatter3d { analysis: Err(_), .. } => "3-D Scatter Plot (could not be drawn)".to_owned(),
            Self::DataSurface { analysis: Ok(a), .. } => a.title(),
            Self::DataSurface { analysis: Err(_), .. } => "Contour and Surface (could not be made)".to_owned(),
            Self::MultipleResponses { analysis: Ok(a), .. } => a.title(),
            Self::MultipleResponses { analysis: Err(_), .. } => "Several Responses (could not be fitted)".to_owned(),
        }
    }

    fn sections(&self) -> Result<Vec<ReportSection>, String> {
        match self {
            Self::Waiting { .. } => Ok(Vec::new()),
            Self::Failed { message, .. } => Err(message.clone()),
            Self::Linear { analysis, options, .. } => {
                analysis.as_ref().map(|a| a.sections(options)).map_err(Clone::clone)
            }
            Self::Chart { analysis, options, .. } => {
                analysis.as_ref().map(|a| a.sections(options)).map_err(Clone::clone)
            }
            Self::Gauge { analysis, options, .. } => {
                analysis.as_ref().map(|a| a.sections(options)).map_err(Clone::clone)
            }
            Self::Distribution { analysis, options, .. } => {
                analysis.as_ref().map(|a| a.sections(options)).map_err(Clone::clone)
            }
            Self::Bivariate { analysis, .. } => {
                analysis.as_ref().map(BivariateAnalysis::sections).map_err(Clone::clone)
            }
            Self::Pca { analysis, .. } => analysis.as_ref().map(PcaAnalysis::sections).map_err(Clone::clone),
            Self::Cluster { analysis, .. } => analysis.as_ref().map(ClusterAnalysis::sections).map_err(Clone::clone),
            Self::Glm { analysis, .. } => analysis.as_ref().map(GlmAnalysis::sections).map_err(Clone::clone),
            Self::ProportionalHazards { analysis, .. } => {
                analysis.as_ref().map(app_model::ProportionalHazardsAnalysis::sections).map_err(Clone::clone)
            }
            Self::SurvivalRegression { analysis, .. } => {
                analysis.as_ref().map(app_model::SurvivalRegressionAnalysis::sections).map_err(Clone::clone)
            }
            Self::Mixed { analysis, .. } => analysis.as_ref().map(MixedAnalysis::sections).map_err(Clone::clone),
            Self::Regularized { analysis, .. } => {
                analysis.as_ref().map(RegularizedAnalysis::sections).map_err(Clone::clone)
            }
            Self::DecisionTree { analysis, .. } => analysis.as_ref().map(TreeAnalysis::sections).map_err(Clone::clone),
            Self::RandomForest { analysis, .. } => {
                analysis.as_ref().map(ForestAnalysis::sections).map_err(Clone::clone)
            }
            Self::BoostedTrees { analysis, .. } => analysis.as_ref().map(BoostAnalysis::sections).map_err(Clone::clone),
            Self::PartialLeastSquares { analysis, .. } => {
                analysis.as_ref().map(PlsAnalysis::sections).map_err(Clone::clone)
            }
            Self::SupportVectorMachine { analysis, .. } => {
                analysis.as_ref().map(SvmAnalysis::sections).map_err(Clone::clone)
            }
            Self::Mars { analysis, .. } => analysis.as_ref().map(MarsAnalysis::sections).map_err(Clone::clone),
            Self::NeuralNetwork { analysis, .. } => {
                analysis.as_ref().map(NeuralAnalysis::sections).map_err(Clone::clone)
            }
            Self::GaussianProcess { analysis, .. } => analysis.as_ref().map(|a| a.sections()).map_err(Clone::clone),
            Self::FunctionalData { analysis, .. } => analysis.as_ref().map(|a| a.sections()).map_err(Clone::clone),
            Self::ModelComparison { analysis, .. } => {
                analysis.as_ref().map(ModelComparison::sections).map_err(Clone::clone)
            }
            Self::ModelSearch { analysis, .. } => analysis.as_ref().map(ModelSearch::sections).map_err(Clone::clone),
            Self::Correspondence { analysis, .. } => {
                analysis.as_ref().map(CorrespondenceReport::sections).map_err(Clone::clone)
            }
            Self::Correlations { analysis, .. } => {
                analysis.as_ref().map(app_model::CorrelationAnalysis::sections).map_err(Clone::clone)
            }
            Self::OutlierDistances { analysis, .. } => {
                analysis.as_ref().map(app_model::OutlierAnalysis::sections).map_err(Clone::clone)
            }
            Self::Manova { analysis, .. } => {
                analysis.as_ref().map(app_model::ManovaAnalysis::sections).map_err(Clone::clone)
            }
            Self::ItemAnalysis { analysis, .. } => {
                analysis.as_ref().map(app_model::ItemAnalysisReport::sections).map_err(Clone::clone)
            }
            Self::FactorAnalysis { analysis, .. } => {
                analysis.as_ref().map(app_model::FactorAnalysisReport::sections).map_err(Clone::clone)
            }
            Self::DiscriminantAnalysis { analysis, .. } => {
                analysis.as_ref().map(app_model::DiscriminantAnalysis::sections).map_err(Clone::clone)
            }
            Self::GaussianMixture { analysis, .. } => {
                analysis.as_ref().map(app_model::GaussianMixtureAnalysis::sections).map_err(Clone::clone)
            }
            Self::LatentClass { analysis, .. } => {
                analysis.as_ref().map(app_model::LatentClassAnalysis::sections).map_err(Clone::clone)
            }
            Self::TextExploration { analysis, .. } => {
                analysis.as_ref().map(app_model::TextExploration::sections).map_err(Clone::clone)
            }
            Self::TopicModel { analysis, .. } => {
                analysis.as_ref().map(app_model::TopicModelAnalysis::sections).map_err(Clone::clone)
            }
            Self::Sentiment { analysis, .. } => {
                analysis.as_ref().map(app_model::SentimentAnalysis::sections).map_err(Clone::clone)
            }
            Self::Embedding { analysis, .. } => {
                analysis.as_ref().map(app_model::EmbeddingAnalysis::sections).map_err(Clone::clone)
            }
            Self::SeriesExplore { analysis, .. } => {
                analysis.as_ref().map(app_model::SeriesExploreAnalysis::sections).map_err(Clone::clone)
            }
            Self::SignalProcessing { analysis, .. } => {
                analysis.as_ref().map(app_model::SignalAnalysis::sections).map_err(Clone::clone)
            }
            Self::ImageAnalysis { analysis, .. } => {
                analysis.as_ref().map(app_model::ImageAnalysis::sections).map_err(Clone::clone)
            }
            Self::Imputation { analysis, .. } => {
                analysis.as_ref().map(app_model::ImputationReport::sections).map_err(Clone::clone)
            }
            Self::Survival { analysis, options, .. } => {
                analysis.as_ref().map(|a| a.sections(options)).map_err(Clone::clone)
            }
            Self::Surface { analysis, options, .. } => {
                analysis.as_ref().map(|a| a.sections(options)).map_err(Clone::clone)
            }
            Self::TimeWeighted { analysis, .. } => {
                analysis.as_ref().map(TimeWeightedAnalysis::sections).map_err(Clone::clone)
            }
            Self::Capability { analysis, .. } => {
                analysis.as_ref().map(CapabilityAnalysis::sections).map_err(Clone::clone)
            }
            Self::Attribute { analysis, .. } => {
                analysis.as_ref().map(AttributeChartAnalysis::sections).map_err(Clone::clone)
            }
            Self::Pareto { analysis, options, .. } => {
                analysis.as_ref().map(|a| a.sections(options)).map_err(Clone::clone)
            }
            Self::CauseEffect { analysis, .. } => {
                analysis.as_ref().map(CauseEffectAnalysis::sections).map_err(Clone::clone)
            }
            Self::MultiVari { analysis, options, .. } => {
                analysis.as_ref().map(|a| a.sections(options)).map_err(Clone::clone)
            }
            Self::MultivariateChart { analysis, .. } => {
                analysis.as_ref().map(MultivariateChartAnalysis::sections).map_err(Clone::clone)
            }
            Self::Anom { analysis, .. } => analysis.as_ref().map(AnomAnalysis::sections).map_err(Clone::clone),
            Self::DoseResponse { analysis, .. } => {
                analysis.as_ref().map(DoseResponseAnalysis::sections).map_err(Clone::clone)
            }
            Self::CategoricalLogistic { analysis, .. } => {
                analysis.as_ref().map(app_model::CategoricalLogisticAnalysis::sections).map_err(Clone::clone)
            }
            Self::Nonlinear { analysis, .. } => {
                analysis.as_ref().map(NonlinearAnalysis::sections).map_err(Clone::clone)
            }
            Self::MethodComparison { analysis, .. } => {
                analysis.as_ref().map(MethodComparisonAnalysis::sections).map_err(Clone::clone)
            }
            Self::ShelfLife { analysis, .. } => {
                analysis.as_ref().map(ShelfLifeAnalysis::sections).map_err(Clone::clone)
            }
            Self::Reliability { result, .. } => result.as_ref().map(|c| c.sections.clone()).map_err(Clone::clone),
            Self::Stepwise { analysis, .. } => analysis.as_ref().map(StepwiseAnalysis::sections).map_err(Clone::clone),
            Self::BestSubsets { analysis, .. } => {
                analysis.as_ref().map(BestSubsetsAnalysis::sections).map_err(Clone::clone)
            }
            Self::DistributionFit { analysis, .. } => {
                analysis.as_ref().map(DistributionFitAnalysis::sections).map_err(Clone::clone)
            }
            Self::Mixture { analysis, .. } => analysis.as_ref().map(MixtureAnalysis::sections).map_err(Clone::clone),
            Self::Comparison { analysis, .. } => {
                analysis.as_ref().map(ComparisonReport::sections).map_err(Clone::clone)
            }
            Self::Graph { .. } => Ok(Vec::new()),
            Self::Scatter3d { analysis, .. } => {
                analysis.as_ref().map(Scatter3dAnalysis::sections).map_err(Clone::clone)
            }
            Self::DataSurface { analysis, options, .. } => {
                analysis.as_ref().map(|a| a.sections(options)).map_err(Clone::clone)
            }
            Self::MultipleResponses { analysis, view, .. } => {
                analysis.as_ref().map(|_| view.sections()).map_err(Clone::clone)
            }
        }
    }

    fn show_zones(&self) -> bool {
        matches!(self, Self::Chart { options, .. } if options.show_zones)
            || matches!(self, Self::Attribute { request, .. } if request.rules != app_model::RuleSet::LIMITS_ONLY)
    }
}

/// One analysis report and its view state.
pub struct Report {
    id: u64,
    body: ReportBody,
    /// Data and exclusion revisions the analysis (or its error) was computed at.
    computed_at: (u64, u64),
    /// Interaction state of each plot (brush, axis limits), by section.
    /// Per-section plot state, by section kind and its occurrence among
    /// sections of that kind.
    plots: HashMap<(SectionKind, usize), PlotState>,
    /// Sections collapsed on screen (kind, occurrence), which exports can leave out.
    closed_sections: HashSet<(SectionKind, usize)>,
    /// The whole report is collapsed on screen.
    collapsed: bool,
    /// Re-fit every plot of this report to its data on the next frame.
    reset_all_views: bool,
    /// Open or close the report's headings on the next frame.
    fold: Option<Fold>,
    pub remove_requested: bool,
    /// "Edit analysis…" was chosen (taken by the app, which opens the dialog).
    pub edit_requested: bool,
    /// Columns the report asked to add to the data table (taken by the app).
    pub new_columns: Vec<NewColumn>,
    /// "Add proposed runs to the table" was chosen (taken by the app, which
    /// appends a sequential-optimization report's proposals as new rows).
    pub runs_requested: bool,
    /// Reports this report asked to open (taken by the app).
    pub new_reports: Vec<ReportRequest>,
    /// Tables this report asked to open (taken by the app), each with
    /// whether it joins the report's project (`true`) or opens as a
    /// separate document.
    pub new_tables: Vec<(DataTable, bool)>,
    /// Derived tables this report asked to make from its document (taken by
    /// the app, which runs each operation and keeps the result in the
    /// report's project, linked to its source).
    pub new_derived: Vec<app_model::table_ops::TableOperation>,
    /// The report is shown in a window of its own (set by the app each
    /// frame); the results pane then shows a placeholder in its place.
    pub in_own_window: bool,
    /// "Open in new window" (or, in its own window, "Back to main window")
    /// was chosen (taken by the app).
    pub window_requested: bool,
    /// "Show window" was chosen on the placeholder (taken by the app, which
    /// brings the report's window to the front).
    pub show_window_requested: bool,
    /// "Add to dashboard" was chosen (taken by the app): `Some(Some(id))`
    /// for dashboard `id`, `Some(None)` for a new dashboard.
    pub dashboard_requested: Option<Option<u64>>,
    /// "Export…" or "Export report…" was chosen (taken by the app, which
    /// opens the export dialog for this report).
    pub export_requested: bool,
    /// "Copy plot style" or "Paste plot style" was chosen (taken by the app).
    pub style_request: Option<crate::plot_style_menu::StyleRequest>,
    /// "Duplicate on another table…" was chosen (taken by the app, which
    /// opens the dialog).
    pub duplicate_requested: bool,
    /// The report is a dashboard tile's: the items that move or edit a
    /// table's report (edit, own window, add to dashboard) are left out.
    pub in_dashboard: bool,
    /// On a dashboard: the rows its filters leave out (and the table itself
    /// does not exclude), which the report calls "filtered out" rather than
    /// "excluded".
    pub filtered_out: Option<Arc<Vec<bool>>>,
    /// The report's own row filter and column picker.
    scope: ReportScope,
    /// The filter the analysis was computed with.
    computed_filter: Vec<app_model::FilterCondition>,
    /// The imputation column it was pooled by.
    computed_pool: Option<String>,
    /// Sections pooled across imputations, shown before the analysis's own
    /// (empty unless the report is pooled).
    pooled: Vec<ReportSection>,
    /// Which rows enter the analysis: the table revision, filter and pooling
    /// it was worked out for, and the mask.
    filter_mask: Option<FilterMask>,
    /// Filter strip and switcher state on screen.
    scope_ui: crate::report_scope_ui::ScopeUi,
    /// The sections shown, built once per computation and display options
    /// rather than every frame (see [`Report::cached_sections`]).
    section_cache: Option<SectionCache>,
    /// The size the report took in the reports pane when last drawn, kept
    /// for it while it is scrolled out of view.
    drawn_size: Option<egui::Vec2>,
    /// The computation running in the background, if one is.
    pending: Option<jobs::Pending>,
    /// Rows were added, removed or reordered since the result shown was
    /// computed: its plots point at old row numbers, so it is hidden while
    /// the new one computes.
    rows_moved: bool,
    /// Why the last background computation failed (the previous result
    /// stays on screen).
    job_error: Option<String>,
    /// The computation cancelled last: the report keeps its previous result
    /// and is not computed again until "Compute again".
    cancelled: Option<jobs::JobKey>,
    /// Advanced whenever what the report saves (its request, display
    /// options and scope) changes; see [`Report::options_revision`].
    options_revision: u64,
    /// Hash of the saved form at the current options revision.
    options_hash: u64,
}

/// Built report sections and the request, display options and scope they
/// were built from. Cleared whenever the analysis is computed again.
struct SectionCache {
    saved: SavedReport,
    scope: ReportScope,
    sections: Arc<Result<Vec<ReportSection>, String>>,
}

/// The sections a report shows: the pooled ones first, then the body's own,
/// with contour plots styled by the scope.
fn assemble_sections(
    body: &ReportBody,
    scope: &ReportScope,
    pooled: &[ReportSection],
) -> Result<Vec<ReportSection>, String> {
    let mut own = body.sections()?;
    scope.style_contours(&mut own);
    if pooled.is_empty() {
        return Ok(own);
    }
    Ok(pooled.iter().cloned().chain(own).collect())
}

/// A cached mask of the rows entering a report's analysis: the table
/// revision, filter and pooling column it was worked out for, and the mask.
type FilterMask = (u64, Vec<app_model::FilterCondition>, Option<String>, Arc<Vec<bool>>);

/// The dialog request of a predictive model's report (the kinds a model
/// search fits).
/// Most responses listed in a screening report's menu.
const SCREENING_MENU_LIMIT: usize = 25;

/// "Open a response's full report": the responses with the smallest
/// q-values first.
fn screening_menu(ui: &mut egui::Ui, a: &app_model::ScreeningAnalysis, new_reports: &mut Vec<ReportRequest>) {
    ui.menu_button("Open a response's full report", |ui| {
        let ranked = a.ranked_responses();
        ui.label(RichText::new("Smallest q-value first").weak());
        egui::ScrollArea::vertical().max_height(crate::dialog_kit::em(ui) * 20.0).show(ui, |ui| {
            for (column, name, q) in ranked.iter().take(SCREENING_MENU_LIMIT) {
                let label = if q.is_nan() {
                    name.clone()
                } else {
                    format!("{name} (q = {})", app_model::format::format_p_value(*q))
                };
                if ui.button(label).clicked() {
                    new_reports.push(match a.response_report(*column) {
                        SavedReport::Linear { spec, .. } => ReportRequest::Linear(spec),
                        SavedReport::Glm { request, .. } => ReportRequest::Glm(request),
                        other => ReportRequest::Comparison(other),
                    });
                    ui.close_menu();
                }
            }
            if ranked.len() > SCREENING_MENU_LIMIT {
                ui.label(
                    RichText::new(format!(
                        "{} more: sort the table by p-value to find them",
                        ranked.len() - SCREENING_MENU_LIMIT
                    ))
                    .weak(),
                );
            }
        });
    });
}

pub fn model_report_request(saved: SavedReport) -> Option<ReportRequest> {
    Some(match saved {
        SavedReport::DecisionTree { request, .. } => ReportRequest::DecisionTree(request),
        SavedReport::RandomForest { request, .. } => ReportRequest::RandomForest(request),
        SavedReport::BoostedTrees { request, .. } => ReportRequest::BoostedTrees(request),
        SavedReport::NeuralNetwork { request, .. } => ReportRequest::NeuralNetwork(request),
        SavedReport::GaussianProcess { request, .. } => ReportRequest::GaussianProcess(request),
        SavedReport::SupportVectorMachine { request, .. } => ReportRequest::SupportVectorMachine(request),
        SavedReport::Mars { request, .. } => ReportRequest::Mars(request),
        SavedReport::PartialLeastSquares { request, .. } => ReportRequest::PartialLeastSquares(request),
        SavedReport::DiscriminantAnalysis { request } => ReportRequest::DiscriminantAnalysis(request),
        _ => return None,
    })
}

impl Report {
    /// The full report of the best model, for a model search that ranked one.
    pub fn best_model_report(&self) -> Option<ReportRequest> {
        let ReportBody::ModelSearch { analysis: Ok(a), .. } = &self.body else { return None };
        model_report_request(a.model_report(a.best()?))
    }

    /// The style of the plot with style key `key`, or of the graph canvas
    /// (`None`), for "Copy plot style"; `None` when there is no such plot.
    pub fn copy_plot_style(&self, key: Option<&str>) -> Option<app_model::style_paste::CopiedStyle> {
        use app_model::style_paste::CopiedStyle;
        if let ReportBody::Graph { spec, .. } = &self.body {
            return Some(CopiedStyle {
                from: self.title(),
                style: spec.axes.style.clone().unwrap_or_default(),
                markers: Some(spec.options.style.clone()),
                legend: Some(spec.legend),
            });
        }
        let key = key?;
        let sections = self.shown_sections().ok()?;
        let (plot, section) = plot_sections(&sections).into_iter().find(|(_, k, _)| k == key).map(|(p, _, s)| (p, s))?;
        let state = self.plots.get(&plot);
        let mut style = self
            .scope
            .plot_style(key)
            .cloned()
            .or_else(|| state.and_then(|s| s.default_style.clone()))
            .unwrap_or_default();
        if let Some(state) = state {
            style.axes = app_model::plot_style::user_axes(&state.axes);
            style.reference.clone_from(&state.reference);
        }
        Some(CopiedStyle { from: section.title.clone(), style, markers: None, legend: None })
    }

    /// Give the plot with style key `only` (or every plot of the report, or
    /// the graph canvas) the `parts` of `copied`; returns how many plots
    /// took it.
    pub fn paste_plot_style(
        &mut self,
        copied: &app_model::style_paste::CopiedStyle,
        parts: app_model::style_paste::StyleParts,
        only: Option<&str>,
    ) -> usize {
        use app_model::style_paste::{paste_markers, paste_style};
        if let ReportBody::Graph { spec, view } = &mut self.body {
            let mut style = spec.axes.style.clone().unwrap_or_default();
            paste_style(parts, &copied.style, &mut style);
            spec.axes.style = (style != app_model::plot_style::PlotStyle::default()).then_some(style);
            paste_markers(parts, copied, &mut spec.options.style, &mut spec.legend);
            if parts.ranges {
                view.forget_views();
            }
            return 1;
        }
        let Ok(sections) = self.shown_sections() else { return 0 };
        let mut n = 0;
        for (plot, key, section) in plot_sections(&sections) {
            if only.is_some_and(|o| o != key) {
                continue;
            }
            let state = self.plots.entry(plot).or_insert_with(|| PlotState::for_section(section));
            state.load_style(&self.scope, &key);
            let Some(default) = state.default_style.clone() else { continue };
            let mut style = self.scope.plot_style(&key).cloned().unwrap_or_else(|| default.clone());
            style.axes = app_model::plot_style::user_axes(&state.axes);
            style.reference.clone_from(&state.reference);
            paste_style(parts, &copied.style, &mut style);
            style.apply_to(&mut state.axes);
            state.reference.clone_from(&style.reference);
            if parts.ranges {
                state.bounds = None;
            }
            self.scope.set_plot_style(&key, (style != default).then_some(style));
            n += 1;
        }
        n
    }

    /// Sections that have plot state (kind, occurrence of that kind).
    #[cfg(test)]
    pub(crate) fn plot_keys(&self) -> Vec<(SectionKind, usize)> {
        self.plots.keys().copied().collect()
    }

    /// A report computing `request` (a [`ReportRequest`] or a [`SavedReport`]).
    pub fn new(id: u64, request: impl Into<SavedReport>, table: &DataTable, rows: &RowStateManager) -> Self {
        let (request, scope) = request.into().split_scope();
        let filtered = scope.apply(table, rows);
        let pooled = scope.pooled_sections(&request, table, rows);
        let mut report = Self {
            id,
            body: ReportBody::compute_guarded(request, table, filtered.as_ref().unwrap_or(rows)),
            computed_at: (table.revision(), rows.exclusion_revision()),
            plots: HashMap::new(),
            closed_sections: HashSet::new(),
            collapsed: false,
            reset_all_views: false,
            fold: None,
            remove_requested: false,
            edit_requested: false,
            new_columns: Vec::new(),
            runs_requested: false,
            new_reports: Vec::new(),
            new_tables: Vec::new(),
            new_derived: Vec::new(),
            in_own_window: false,
            window_requested: false,
            show_window_requested: false,
            dashboard_requested: None,
            export_requested: false,
            style_request: None,
            duplicate_requested: false,
            in_dashboard: false,
            filtered_out: None,
            computed_filter: scope.filter.clone(),
            computed_pool: scope.pool.clone(),
            pooled,
            scope,
            filter_mask: None,
            scope_ui: crate::report_scope_ui::ScopeUi::default(),
            section_cache: None,
            drawn_size: None,
            pending: None,
            rows_moved: false,
            job_error: None,
            cancelled: None,
            options_revision: 0,
            options_hash: 0,
        };
        report.sync_options_revision();
        report
    }

    /// A report for `request` that is computed in the background: it shows
    /// as computing until [`Self::install`] gives it its result.
    pub fn new_waiting(id: u64, request: impl Into<SavedReport>) -> Self {
        let (request, scope) = request.into().split_scope();
        let mut report = Self {
            id,
            body: ReportBody::Waiting { saved: Box::new(request) },
            // Never a real revision, so the report asks to be computed.
            computed_at: (u64::MAX, u64::MAX),
            plots: HashMap::new(),
            closed_sections: HashSet::new(),
            collapsed: false,
            reset_all_views: false,
            fold: None,
            remove_requested: false,
            edit_requested: false,
            new_columns: Vec::new(),
            runs_requested: false,
            new_reports: Vec::new(),
            new_tables: Vec::new(),
            new_derived: Vec::new(),
            in_own_window: false,
            window_requested: false,
            show_window_requested: false,
            dashboard_requested: None,
            export_requested: false,
            style_request: None,
            duplicate_requested: false,
            in_dashboard: false,
            filtered_out: None,
            computed_filter: scope.filter.clone(),
            computed_pool: scope.pool.clone(),
            pooled: Vec::new(),
            scope,
            filter_mask: None,
            scope_ui: crate::report_scope_ui::ScopeUi::default(),
            section_cache: None,
            drawn_size: None,
            pending: None,
            rows_moved: false,
            job_error: None,
            cancelled: None,
            options_revision: 0,
            options_hash: 0,
        };
        report.sync_options_revision();
        report
    }

    /// A number that changes whenever what the report saves changes: its
    /// request, display options, row filter or column picker. It is
    /// brought up to date each time the report is drawn and by
    /// [`sync_options_revision`](Self::sync_options_revision), so a project
    /// can tell an option change from no change by comparing numbers.
    pub fn options_revision(&self) -> u64 {
        self.options_revision
    }

    /// Bring [`options_revision`](Self::options_revision) up to date: it
    /// advances when the saved form differs from when it was last brought
    /// up to date. Compares a hash of the form's debug text (cheap next to
    /// the analysis, and steady for missing values, which equality is not).
    pub fn sync_options_revision(&mut self) {
        let hash = saved_form_hash(&self.saved());
        if hash != self.options_hash {
            self.options_hash = hash;
            self.options_revision += 1;
        }
    }

    /// The view and the plot frame drawn last of the plot of section `kind`
    /// (its first occurrence), for tests.
    #[cfg(test)]
    pub fn plot_view(&self, kind: SectionKind) -> Option<(Option<ViewBounds>, Option<egui::Rect>)> {
        self.plots.get(&(kind, 0)).map(|s| (s.last_view, s.last_frame))
    }

    /// Set the axis settings of the plot of section `kind`, for tests.
    #[cfg(test)]
    pub fn plot_axes_mut(&mut self, kind: SectionKind) -> Option<&mut PlotAxes> {
        self.plots.get_mut(&(kind, 0)).map(|s| &mut s.axes)
    }

    /// The first plot of kind `kind`'s view state (once drawn).
    #[cfg(test)]
    pub fn plot_state_mut(&mut self, kind: SectionKind) -> Option<&mut PlotState> {
        self.plots.get_mut(&(kind, 0))
    }

    /// The report's row filter and column picker.
    #[cfg(test)]
    pub fn scope(&self) -> &ReportScope {
        &self.scope
    }

    /// The analysis request without the report's filter and switcher, as
    /// the analysis dialogs read it ("Edit analysis…").
    pub fn request(&self) -> SavedReport {
        self.body.saved()
    }

    /// Which rows enter the analysis: those passing the report's filter
    /// and, when pooled, of imputation 0 (`None` when the scope narrows
    /// nothing), cached while the table and scope stay the same.
    fn filter_mask(&mut self, table: &DataTable) -> Option<Arc<Vec<bool>>> {
        if !self.scope.narrows() {
            return None;
        }
        let fresh = self.filter_mask.as_ref().is_some_and(|(rev, f, pool, _)| {
            *rev == table.revision() && *f == self.scope.filter && *pool == self.scope.pool
        });
        if !fresh {
            let mask = Arc::new(self.scope.matches(table));
            self.filter_mask = Some((table.revision(), self.scope.filter.clone(), self.scope.pool.clone(), mask));
        }
        self.filter_mask.as_ref().map(|(_, _, _, m)| Arc::clone(m))
    }

    /// Note what the analysis was computed from, and recompute the pooled
    /// sections (the filtered rows of every imputation).
    fn mark_computed(&mut self, table: &DataTable, rows: &RowStateManager) {
        self.computed_at = (table.revision(), rows.exclusion_revision());
        self.computed_filter = self.scope.filter.clone();
        self.computed_pool = self.scope.pool.clone();
        self.pooled = self.scope.pooled_sections(&self.body.saved(), table, rows);
        self.section_cache = None;
    }

    /// `true` when the filter or pooling changed since the last computation.
    fn scope_changed(&self) -> bool {
        self.computed_filter != self.scope.filter || self.computed_pool != self.scope.pool
    }

    /// The sections shown: the pooled ones first, then the analysis's own.
    fn shown_sections(&self) -> Result<Vec<ReportSection>, String> {
        assemble_sections(&self.body, &self.scope, &self.pooled)
    }

    /// [`Self::shown_sections`], rebuilt only when the analysis was computed
    /// again or its display options or scope changed since the last call.
    /// Building sections formats every table and copies every plot's points
    /// (some analyses also run their tests there), far too much for every
    /// frame on a large table.
    fn cached_sections(&mut self) -> Arc<Result<Vec<ReportSection>, String>> {
        // The several-responses view builds its sections from view state
        // that changes while the explorer is dragged.
        if matches!(self.body, ReportBody::MultipleResponses { .. }) {
            return Arc::new(self.shown_sections());
        }
        let saved = self.body.saved();
        if let Some(c) = &self.section_cache {
            if c.saved == saved && c.scope == self.scope {
                return Arc::clone(&c.sections);
            }
        }
        let sections = Arc::new(self.shown_sections());
        self.section_cache = Some(SectionCache { saved, scope: self.scope.clone(), sections: Arc::clone(&sections) });
        sections
    }

    /// The row states the analysis uses: `rows` with the rows outside the
    /// report's filter also excluded and hidden (`None` without a filter).
    fn local_rows(&mut self, table: &DataTable, rows: &RowStateManager) -> Option<RowStateManager> {
        self.filter_mask(table).map(|mask| app_model::report_scope::apply_mask(&mask, rows))
    }

    /// Recompute the analysis now, on `rows` narrowed by the filter.
    fn recompute_now(&mut self, table: &DataTable, rows: &RowStateManager) {
        let local = self.local_rows(table, rows);
        self.body.recompute_guarded(table, local.as_ref().unwrap_or(rows));
        self.mark_computed(table, rows);
    }

    /// Put `column` in the report's switch slot and recompute; plot views
    /// re-fit to the new column.
    fn switch_to(&mut self, column: usize, table: &DataTable, rows: &RowStateManager, background: bool) {
        let Some(next) = self.saved().switched_to(column, table) else { return };
        let (request, scope) = next.split_scope();
        self.scope = scope;
        if background {
            // Computed by the workers, as a new report is.
            self.body = ReportBody::Waiting { saved: Box::new(request) };
            self.computed_at = (u64::MAX, u64::MAX);
            self.computed_filter = self.scope.filter.clone();
            self.computed_pool = self.scope.pool.clone();
            self.pooled.clear();
            self.section_cache = None;
            self.pending = None;
        } else {
            let local = self.local_rows(table, rows);
            self.body = ReportBody::compute_guarded(request, table, local.as_ref().unwrap_or(rows));
            self.mark_computed(table, rows);
        }
        for plot in self.plots.values_mut() {
            plot.bounds = None;
            plot.last_view = None;
        }
    }

    /// `true` for a graph canvas (filled by dragging columns onto shelves).
    pub fn is_graph(&self) -> bool {
        matches!(self.body, ReportBody::Graph { .. })
    }

    /// Identifies the report for the life of the application.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// The report's title.
    pub fn title(&self) -> String {
        self.body.title()
    }

    /// The analysis and display options, for saving in a project.
    pub fn saved(&self) -> SavedReport {
        self.body.saved().with_scope(self.scope.clone())
    }

    /// The report as an HTML export sees it: title, request, sections (or
    /// error) and the sections collapsed on screen.
    pub fn for_export(&self) -> crate::report_export_ui::ReportForExport {
        let sections = self.shown_sections();
        let closed = sections
            .as_ref()
            .map_or_else(|_| Vec::new(), |s| crate::report_export_ui::closed_indices(s, &self.closed_sections));
        crate::report_export_ui::ReportForExport {
            title: self.title(),
            saved: self.saved(),
            sections,
            closed,
            collapsed: self.collapsed,
        }
    }

    /// Recompute if rows were excluded or re-included, or a setting that affects
    /// the computation changed, since the last run.
    pub fn refresh_if_stale(&mut self, table: &DataTable, rows: &RowStateManager) {
        if self.is_stale(table, rows) {
            self.recompute_now(table, rows);
        }
    }

    /// `true` when the data, exclusions, filter or a setting that affects the
    /// computation changed since the result shown was computed.
    fn is_stale(&self, table: &DataTable, rows: &RowStateManager) -> bool {
        let now = (table.revision(), rows.exclusion_revision());
        self.computed_at != now || self.scope_changed() || self.body.settings_changed()
    }

    /// What computing the report now would use.
    fn job_key(&self, table: &DataTable, rows: &RowStateManager) -> jobs::JobKey {
        jobs::JobKey {
            at: (table.revision(), rows.exclusion_revision()),
            saved: self.body.saved(),
            scope: self.scope.clone(),
        }
    }

    /// `true` when the report is out of date and has no computation running
    /// (call [`Self::queue_refresh`] then). A graph canvas, which rebuilds
    /// itself as it draws, is brought up to date here.
    pub fn wants_refresh(&mut self, table: &DataTable, rows: &RowStateManager) -> bool {
        if self.pending.is_some() || !self.is_stale(table, rows) {
            return false;
        }
        // A cancelled computation stays cancelled until the data, the row
        // states or the report change again.
        if self.cancelled.is_some() {
            if self.cancelled.as_ref() == Some(&self.job_key(table, rows)) {
                return false;
            }
            self.cancelled = None;
        }
        if matches!(self.body, ReportBody::Graph { .. }) {
            self.recompute_now(table, rows);
            return false;
        }
        true
    }

    /// Compute the report again in the background, on snapshots of the data
    /// and row states.
    pub fn queue_refresh(&mut self, jobs: &mut ReportJobs, table: Arc<DataTable>, rows: Arc<RowStateManager>) {
        let key = self.job_key(&table, &rows);
        self.pending = Some(jobs.submit(self.id, key, table, rows));
    }

    /// Take a finished background computation: shown if the report still
    /// wants what was computed (otherwise dropped, and [`Self::wants_refresh`]
    /// asks for another). Returns `true` if it was installed.
    pub fn install(&mut self, done: Done, table: &DataTable, rows: &RowStateManager) -> bool {
        if self.pending.as_ref().is_none_or(|p| p.generation != done.generation) {
            return false;
        }
        self.pending = None;
        if self.job_key(table, rows) != done.key {
            return false;
        }
        let jobs::JobKey { at, saved, scope } = done.key;
        self.computed_at = at;
        self.computed_filter = scope.filter.clone();
        self.computed_pool = scope.pool.clone();
        self.rows_moved = false;
        match done.outcome {
            Ok(computed) => {
                let jobs::Computed { body, pooled, sections } = *computed;
                self.body.adopt(body, at);
                self.pooled = pooled;
                self.section_cache = Some(SectionCache { saved, scope, sections });
                self.job_error = None;
            }
            // A report with no result yet shows the error in its place;
            // otherwise the previous result stays, under the error.
            Err(message) => match &self.body {
                ReportBody::Waiting { saved } => {
                    self.body = ReportBody::Failed { saved: (**saved).clone(), message };
                    self.section_cache = None;
                    self.job_error = None;
                }
                _ => self.job_error = Some(format!("{message} The previous result is shown.")),
            },
        }
        true
    }

    /// Compute the report here and now if it is out of date, waiting or
    /// computing in the background (whose result is then dropped).
    pub fn bring_up_to_date(&mut self, table: &DataTable, rows: &RowStateManager) {
        if self.pending.is_some() || self.cancelled.is_some() || self.is_stale(table, rows) {
            self.pending = None;
            self.cancelled = None;
            self.rows_moved = false;
            self.recompute_now(table, rows);
        }
    }

    /// `true` while a background computation of the report is running.
    #[cfg(test)]
    pub fn is_updating(&self) -> bool {
        self.pending.is_some()
    }

    /// Rows of the table were added, removed or reordered: until the report
    /// is computed again, its plots would point at the wrong rows.
    pub fn note_rows_moved(&mut self) {
        self.rows_moved = true;
    }

    /// The edit appending a sequential-optimization report's proposed runs
    /// to `table`, with how many there are; `None` for other reports.
    pub fn proposed_runs(&self, table: &DataTable) -> Option<Result<(app_model::editing::Edit, usize), String>> {
        let ReportBody::Comparison { analysis: Ok(ComparisonReport::Sequential(a)), .. } = &self.body else {
            return None;
        };
        Some(a.new_runs_edit(table).map(|e| (e, a.proposals().len())).map_err(|e| e.to_string()))
    }

    /// `true` if the analysis uses column `index` of `table`.
    pub fn uses_column(&self, index: usize, table: &DataTable) -> bool {
        let saved = self.saved();
        saved.columns().contains(&index)
            || table.column(index).is_some_and(|c| saved.column_names().contains(&c.name.as_str()))
    }

    /// Follow a column renamed from `old` to `new` (reports that name their
    /// columns, such as graphs).
    pub fn rename_column(&mut self, old: &str, new: &str) {
        if let ReportBody::Graph { spec, .. } = &mut self.body {
            spec.encoding.rename(old, new);
        }
        self.scope.rename(old, new);
    }

    /// Follow columns that moved because one was inserted or removed, and
    /// recompute. Plot view states are kept.
    /// Recompute the report from `saved` (an edited request), keeping its
    /// place, id, and plot sizes and axis settings; display options carry
    /// over when the analysis is of the same kind. Plot views re-fit to the
    /// new data.
    /// The report keeps its row filter and column picker, unless `saved`
    /// carries its own.
    pub fn replace(&mut self, saved: SavedReport, table: &DataTable, rows: &RowStateManager) {
        let (saved, scope) = saved.split_scope();
        if !scope.is_empty() {
            self.scope = scope;
            self.restyle_plots();
        }
        self.rebuild(keep_display_options(saved, &self.body.saved()), table, rows);
    }

    /// Recompute the report from `saved` exactly: its request, display
    /// options and scope (none when it has none), keeping the report's
    /// place, id and plot settings. Plot views re-fit to the new data.
    pub fn replace_all(&mut self, saved: SavedReport, table: &DataTable, rows: &RowStateManager) {
        let (saved, scope) = saved.split_scope();
        self.scope = scope;
        self.restyle_plots();
        self.rebuild(saved, table, rows);
    }

    /// Draw every plot in the style its scope keeps for it (its default
    /// style when none), as after an undo that changed a plot's settings.
    fn restyle_plots(&mut self) {
        for (&(kind, nth), plot) in &mut self.plots {
            if let Some(default) = &plot.default_style {
                default.apply_to(&mut plot.axes);
                if let Some(saved) = self.scope.plot_style(&app_model::plot_style_key(kind, nth)) {
                    saved.apply_to(&mut plot.axes);
                }
            }
        }
    }

    fn rebuild(&mut self, saved: SavedReport, table: &DataTable, rows: &RowStateManager) {
        let local = self.local_rows(table, rows);
        self.body = ReportBody::compute_guarded(saved, table, local.as_ref().unwrap_or(rows));
        self.mark_computed(table, rows);
        for plot in self.plots.values_mut() {
            plot.bounds = None;
            plot.last_view = None;
        }
    }

    pub fn remap_columns(&mut self, shift: ColumnShift, table: &DataTable, rows: &RowStateManager) {
        if matches!(self.body, ReportBody::Graph { .. }) {
            // Graphs refer to columns by name, so nothing moves.
            return;
        }
        let saved = self.body.saved().map_columns(|c| shift.map(c).unwrap_or(c));
        let local = self.local_rows(table, rows);
        self.body = ReportBody::compute_guarded(saved, table, local.as_ref().unwrap_or(rows));
        self.mark_computed(table, rows);
    }
}

/// `true` for the sections pooled across imputations (not bootstrapped).
fn is_pooled_section(kind: SectionKind) -> bool {
    matches!(
        kind,
        SectionKind::PooledNote
            | SectionKind::PooledEstimates
            | SectionKind::PooledVariance
            | SectionKind::PooledTests
            | SectionKind::PooledFit
            | SectionKind::PooledByImputation
    )
}

/// A multiple imputation's "Make stacked table" item: the original rows and
/// every completed copy in a new table of the project, linked to its source.
fn stacked_table_item(
    ui: &mut egui::Ui,
    body: &ReportBody,
    table: &DataTable,
    new_derived: &mut Vec<app_model::table_ops::TableOperation>,
) {
    let ReportBody::Imputation { analysis: Ok(a), .. } = body else { return };
    let Some(op) = a.stacked_operation(table) else { return };
    ui.separator();
    if ui
        .button("Make stacked table")
        .on_hover_text(
            "A new table holding the original rows (Imputation 0) and each completed copy under them, for \
             analyses pooled across the imputations",
        )
        .clicked()
    {
        new_derived.push(op);
        ui.close_menu();
    }
}

/// How to work a report plot, shown above the reports while the
/// getting-started tips are on, and always on each plot's "?".
const PLOT_HINT: &str = "Plots: drag an axis end to rescale · drag the axis middle to shift · right-click an axis for \
                         its settings · drag in the plot to select · hover a point for its row";

/// Opening or closing a report's collapsible headings at once.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fold {
    /// Close the report down to its title.
    CollapseReport,
    /// Open the report and every section.
    ExpandAll,
    /// Keep the report open but close each section to its heading.
    CollapseSections,
    /// Open the report, leaving its sections as they are.
    OpenReport,
}

/// Most reports listed as links in the outline line; past this many they go
/// in the Go to drop-down list.
const OUTLINE_LINKS: usize = 4;

/// Context key: `true` while reports are drawn in the read-only dashboard
/// viewer, which offers no way to remove them.
pub fn read_only_id() -> egui::Id {
    egui::Id::new("reports_read_only")
}

/// Where the app puts the getting-started tips setting, which also shows
/// the plot hint line above the reports.
pub fn show_plot_hint_id() -> egui::Id {
    egui::Id::new("show_plot_hint")
}

/// `title`, cut to `max` characters with "…".
pub(crate) fn short_title(title: &str, max: usize) -> String {
    if title.chars().count() > max { title.chars().take(max - 1).chain(['…']).collect() } else { title.to_owned() }
}

/// Open `report` and scroll the reports pane to it.
fn jump_to(ui: &egui::Ui, report: &mut Report) {
    jump_to_report(ui.ctx(), report);
}

/// Open `report` and scroll the reports pane showing it to it on the next
/// frame (or bring its window to the front when it has one of its own).
pub fn jump_to_report(ctx: &egui::Context, report: &mut Report) {
    if report.in_own_window {
        report.show_window_requested = true;
        return;
    }
    report.fold = Some(Fold::OpenReport);
    let id = report.id;
    ctx.data_mut(|d| d.insert_temp(jump_to_report_id(), id));
}

/// "Go to ▾": every report's title in a list, newest first; picking one
/// scrolls to it.
fn go_to_menu(ui: &mut egui::Ui, reports: &mut [Report]) {
    let count = reports.len();
    crate::menus::menu_button(ui, "Go to ▾", |ui| {
        egui::ScrollArea::vertical().max_height(ui.ctx().screen_rect().height() * 0.6).show(ui, |ui| {
            for report in reports.iter_mut() {
                let title = report.title();
                if ui.button(short_title(&title, 70)).on_hover_text(&title).clicked() {
                    jump_to(ui, report);
                    ui.close_menu();
                }
            }
        });
    })
    .response
    .on_hover_text(format!("Jump to one of the {count} reports"));
}

/// Where the outline asks the reports pane to scroll (a report id), for one frame.
fn jump_to_report_id() -> egui::Id {
    egui::Id::new("jump_to_report")
}

/// The reports pane: a toolbar (collapse, expand, export, go to) above every
/// report, newest first. Returns `true` when the toolbar's Export reports
/// button was clicked, so the caller opens the export dialog for all of the
/// table's reports.
#[must_use]
pub fn show_reports(ui: &mut egui::Ui, reports: &mut [Report], table: &DataTable, rows: &mut RowStateManager) -> bool {
    let mut export = false;
    if reports.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(
                RichText::new(
                    "No results yet. Use the Describe, Compare, Model, Quality or Reliability menu to run an analysis, \
                     or Visualize ▸ Graph canvas to build a graph.",
                )
                .weak(),
            );
        });
        return export;
    }
    ui.horizontal(|ui| {
        let fold = |reports: &mut [Report], fold| reports.iter_mut().for_each(|r| r.fold = Some(fold));
        if ui.small_button("Collapse all").on_hover_text("Close every report down to its title").clicked() {
            fold(reports, Fold::CollapseReport);
        }
        if ui.small_button("Expand all").on_hover_text("Open every report and all of its sections").clicked() {
            fold(reports, Fold::ExpandAll);
        }
        // Export works with every report collapsed, unlike each report's own button.
        export = ui
            .small_button("Export all…")
            .on_hover_text(
                "Save these reports, or some of them, as a PDF, Word, PowerPoint, HTML, Markdown or Excel file",
            )
            .clicked();
        // A few reports are listed as links beside the buttons; more go in a
        // drop-down list so the line never wraps into a paragraph.
        if reports.len() > OUTLINE_LINKS {
            go_to_menu(ui, reports);
        }
        if ui.ctx().data(|d| d.get_temp::<bool>(show_plot_hint_id())).unwrap_or(true) {
            ui.add(egui::Label::new(RichText::new(PLOT_HINT).weak().small()).truncate());
        }
    });
    // An outline of the reports (newest first), each title jumping to its report.
    if (2..=OUTLINE_LINKS).contains(&reports.len()) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Go to").weak().small());
            for report in reports.iter_mut() {
                let title = report.title();
                if ui.link(RichText::new(short_title(&title, 40)).small()).on_hover_text(title).clicked() {
                    jump_to(ui, report);
                }
            }
        });
    }
    // Both directions: a report wider than the pane scrolls sideways instead of
    // widening the pane.
    egui::ScrollArea::both().id_salt("reports_scroll").auto_shrink([false, false]).show(ui, |ui| {
        // Where the scrolled content starts and how wide the viewport is, so
        // plots size to the viewport whatever the horizontal scroll position.
        let viewport_width = ui.clip_rect().width() - 2.0 * ui.visuals().clip_rect_margin;
        let frame = (ui.max_rect().left(), viewport_width);
        ui.ctx().data_mut(|d| d.insert_temp(report_frame_id(), frame));
        // Reports well outside the visible part of the pane keep their
        // place without being drawn (their tables and plots cost time every
        // frame even when nothing of them is seen).
        let visible = ui.clip_rect().expand2(egui::vec2(0.0, ui.clip_rect().height()));
        let jump = ui.ctx().data(|d| d.get_temp::<u64>(jump_to_report_id()));
        for report in reports.iter_mut() {
            if report.in_own_window {
                own_window_placeholder(ui, report);
            } else {
                let skip = report.drawn_size.filter(|&size| {
                    let rect = egui::Rect::from_min_size(ui.cursor().min, size);
                    !visible.intersects(rect) && report.fold.is_none() && !report.reset_all_views && jump != Some(report.id)
                });
                if let Some(size) = skip {
                    ui.allocate_space(size);
                } else {
                    let started = std::time::Instant::now();
                    let drawn = ui.scope(|ui| show_report(ui, report, table, rows)).response.rect;
                    note_report_time(ui.ctx(), report, started.elapsed());
                    report.drawn_size = Some(drawn.size());
                }
            }
            ui.add_space(10.0);
        }
    });
    export
}

/// One report in a window of its own: the report in a scroll area that
/// fills the window, its plots sized to the window.
pub fn show_single_report(ui: &mut egui::Ui, report: &mut Report, table: &DataTable, rows: &mut RowStateManager) {
    egui::ScrollArea::both().id_salt(("report_window_scroll", report.id)).auto_shrink([false, false]).show(ui, |ui| {
        let viewport_width = ui.clip_rect().width() - 2.0 * ui.visuals().clip_rect_margin;
        let frame = (ui.max_rect().left(), viewport_width);
        ui.ctx().data_mut(|d| d.insert_temp(report_frame_id(), frame));
        let started = std::time::Instant::now();
        show_report(ui, report, table, rows);
        note_report_time(ui.ctx(), report, started.elapsed());
    });
}

/// The report that took longest to draw this frame: its title and time,
/// with the frame it was drawn in.
type SlowestReport = (u64, String, std::time::Duration);

fn slowest_report_id() -> egui::Id {
    egui::Id::new("slowest_report")
}

/// Keep `took` for `report` if it is the slowest report of this frame so far.
fn note_report_time(ctx: &egui::Context, report: &Report, took: std::time::Duration) {
    let frame = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let slowest = d.get_temp_mut_or_insert_with(slowest_report_id(), || (frame, String::new(), std::time::Duration::ZERO));
        let slowest: &mut SlowestReport = slowest;
        if slowest.0 != frame || took > slowest.2 {
            *slowest = (frame, report.title(), took);
        }
    });
}

/// The report that took longest to draw in the last frame that drew any
/// (its title and time), for the frame-time readout.
pub fn slowest_report(ctx: &egui::Context) -> Option<(String, std::time::Duration)> {
    let frame = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<SlowestReport>(slowest_report_id()))
        .filter(|(at, _, _)| at + 2 >= frame)
        .map(|(_, title, took)| (title, took))
}

/// Where a report shown in its own window would be: its title, a note, and
/// buttons to bring the window forward or the report back.
fn own_window_placeholder(ui: &mut egui::Ui, report: &mut Report) {
    let title = report.title();
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            crate::theme::window_glyph(ui, ui.visuals().weak_text_color());
            ui.label(RichText::new(&title).strong());
        });
        ui.label(RichText::new("This report is in its own window.").weak());
        ui.horizontal(|ui| {
            if ui.button("Show window").on_hover_text("Bring the report's window to the front").clicked() {
                report.show_window_requested = true;
            }
            if ui.button("Bring back here").on_hover_text("Close the window and show the report here").clicked() {
                report.window_requested = true;
            }
        });
    });
}

pub(crate) fn figure_status_id(report: u64) -> egui::Id {
    egui::Id::new(("figure_status", report))
}

/// "Save plot as PNG/SVG/PDF/EPS…" items for a plot's and its section
/// heading's right-click menus, drawn at `layout` (the plot's size and view
/// on screen). The outcome shows under the report's options button.
fn save_figure_menu(
    ui: &mut egui::Ui,
    section: &ReportSection,
    rows: &RowStateManager,
    report: u64,
    layout: FigureLayout,
    look: Option<&(app_model::plot_style::ExportLook, egui::Id)>,
    reference: &stat_viz::plot_style::ReferenceLook,
) {
    let ctx = ui.ctx().clone();
    // Report plots drawn by `section_figure_with` redraw at any size on the
    // figure page, from a copy of the section and its row states.
    let sized = !matches!(section.content, SectionContent::Tree(_) | SectionContent::CauseEffect(_));
    let maker = || -> crate::figure_page_window::PanelMaker {
        let (section, rows, layout, reference) = (section.clone(), rows.clone(), layout.clone(), reference.clone());
        std::sync::Arc::new(move |format, request: crate::figure_page_window::PanelRequest| {
            let mut section = section.clone();
            if !request.titled {
                section.title.clear();
            }
            let layout = FigureLayout { width: request.width, height: request.height, ..layout.clone() };
            app_model::report_figure::section_figure_styled(&section, Some(&rows), format, layout, &reference)
        })
    };
    let about = "The plot drawn at its on-screen proportions and current axis ranges, titled with the section \
                 title; excluded rows are marked.";
    let make = |format| match &section.content {
            SectionContent::Tree(d) => {
                Some(crate::tree_view::figure(&ctx, d, report, section.kind, &section.title, format))
            }
            SectionContent::CauseEffect(d) => {
                Some(app_model::report_figure::fishbone_figure(d, &section.title, format))
            }
            _ => app_model::report_figure::section_figure_styled(section, Some(rows), format, layout.clone(), reference),
    };
    let maker = sized.then_some(&maker as &dyn Fn() -> crate::figure_page_window::PanelMaker);
    match look {
        Some((look, target)) => save_figure_items_styled(ui, report, &section.title, about, &make, maker, look, *target),
        None => save_figure_items_with(ui, report, &section.title, about, &make, maker),
    }
}

/// "Export report…": asks the app to open the export dialog for this report.
fn export_item(ui: &mut egui::Ui, export_requested: &mut bool) {
    let hover = "Save this report, or every report of the table, as a PDF, Word, PowerPoint, HTML, Markdown or \
                 Excel file: tables, plots and notes";
    if ui.button("Export report…").on_hover_text(hover).clicked() {
        *export_requested = true;
        ui.close_menu();
    }
}

/// "Save plot as PNG/SVG/PDF/EPS…" menu items: `make` draws the figure in
/// the chosen format (`None` when there is nothing to draw). The outcome
/// shows under report `report`'s options button. `about` starts the hover
/// text.
pub(crate) fn save_figure_items(
    ui: &mut egui::Ui,
    report: u64,
    name: &str,
    about: &str,
    make: &dyn Fn(VectorFormat) -> Option<Vec<u8>>,
) {
    save_figure_items_with(ui, report, name, about, make, None);
}

/// [`save_figure_items`] plus "Add to figure page": `maker` (when given)
/// makes an owned copy of the plot that redraws at any size, otherwise the
/// page keeps the picture `make` draws. The figure look is kept for the
/// session (plots that keep a style use [`save_figure_items_styled`]).
pub(crate) fn save_figure_items_with(
    ui: &mut egui::Ui,
    report: u64,
    name: &str,
    about: &str,
    make: &dyn Fn(VectorFormat) -> Option<Vec<u8>>,
    maker: Option<&dyn Fn() -> crate::figure_page_window::PanelMaker>,
) {
    let target = crate::plot_export::target((report, name));
    let look = crate::plot_export::session_look(ui.ctx(), target);
    save_figure_items_styled(ui, report, name, about, make, maker, &look, target);
}

/// [`save_figure_items_with`] for a plot whose figure look `look` is kept by
/// its owner, which takes changes posted under `target`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn save_figure_items_styled(
    ui: &mut egui::Ui,
    report: u64,
    name: &str,
    about: &str,
    make: &dyn Fn(VectorFormat) -> Option<Vec<u8>>,
    maker: Option<&dyn Fn() -> crate::figure_page_window::PanelMaker>,
    look: &app_model::plot_style::ExportLook,
    target: egui::Id,
) {
    let source = crate::plot_export::FigureSource { report, name, make, maker };
    if let Some(status) = crate::plot_export::figure_items(ui, &source, "Save plot as", about, look, target) {
        ui.ctx().data_mut(|d| d.insert_temp(figure_status_id(report), status));
    }
    if let Some(status) = crate::figure_page_window::add_item(ui, name, make, maker) {
        ui.ctx().data_mut(|d| d.insert_temp(figure_status_id(report), status));
    }
}

/// One report: its own row filter narrows the rows it computes from and
/// draws, and selections made in it go to the table's own row states.
fn show_report(ui: &mut egui::Ui, report: &mut Report, table: &DataTable, rows: &mut RowStateManager) {
    crate::axis_drop::begin(ui.ctx(), report, table);
    show_scoped_report(ui, report, table, rows);
    if let Some(column) = crate::axis_drop::end(ui.ctx(), report.id) {
        report.switch_to(column, table, rows, background_reports(ui.ctx()));
    }
    // Options change only through input, so frames without any leave the
    // revision as it is.
    if ui.ctx().input(|i| !i.events.is_empty() || i.pointer.any_down() || i.pointer.any_released()) {
        report.sync_options_revision();
    }
}

/// A hash of `saved`'s debug text, fed to the hasher as it is written.
fn saved_form_hash(saved: &SavedReport) -> u64 {
    use std::hash::Hasher as _;
    struct Feed(std::collections::hash_map::DefaultHasher);
    impl std::fmt::Write for Feed {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            self.0.write(s.as_bytes());
            Ok(())
        }
    }
    let mut feed = Feed(std::collections::hash_map::DefaultHasher::new());
    // Writing into a hasher cannot fail.
    let _ = std::fmt::write(&mut feed, format_args!("{saved:?}"));
    feed.0.finish()
}

/// [`show_report`] without the axis drops.
fn show_scoped_report(ui: &mut egui::Ui, report: &mut Report, table: &DataTable, rows: &mut RowStateManager) {
    let dashboard_out = report.filtered_out.clone();
    let Some(mask) = report.filter_mask(table) else {
        show_report_contents(ui, report, table, rows, dashboard_out);
        apply_scope_changes(ui.ctx(), report, table, rows);
        return;
    };
    let mut local = app_model::report_scope::apply_mask(&mask, rows);
    // "Filtered out": outside the report's (or a dashboard's) filter and not
    // excluded by the table itself.
    let filtered_out: Vec<bool> = (0..rows.len())
        .map(|r| {
            dashboard_out.as_ref().is_some_and(|m| m.get(r).copied().unwrap_or(false))
                || (!mask.get(r).copied().unwrap_or(true) && !rows.is_excluded(r))
        })
        .collect();
    let before = local.revision();
    show_report_contents(ui, report, table, &mut local, Some(Arc::new(filtered_out)));
    if local.revision() != before {
        rows.select(local.selected_rows(), SelectionMode::Replace);
    }
    apply_scope_changes(ui.ctx(), report, table, rows);
}

/// Carry out a column picked in the switcher, or recompute after the
/// filter changed.
fn apply_scope_changes(ctx: &egui::Context, report: &mut Report, table: &DataTable, rows: &RowStateManager) {
    let background = background_reports(ctx);
    if let Some(column) = report.scope_ui.switch_to.take() {
        report.switch_to(column, table, rows, background);
        ctx.request_repaint();
    } else if report.scope_changed() {
        if background {
            // The app queues it on the next frame.
            ctx.request_repaint();
        } else {
            report.recompute_now(table, rows);
        }
    }
}

fn background_reports_id() -> egui::Id {
    egui::Id::new("background_reports")
}

/// Say whether reports are computed by background workers (set by the app
/// each frame; off in headless tests).
pub fn set_background_reports(ctx: &egui::Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(background_reports_id(), on));
}

/// `true` when reports are computed by background workers.
fn background_reports(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(background_reports_id())).unwrap_or(false)
}

fn show_report_contents(
    ui: &mut egui::Ui,
    report: &mut Report,
    table: &DataTable,
    rows: &mut RowStateManager,
    filtered_out: Option<Arc<Vec<bool>>>,
) {
    if let ReportBody::MultipleResponses { analysis: Ok(a), view, .. } = &mut report.body {
        view.prepare(a, ui.input(|i| i.pointer.any_down()));
    }
    let title = report.body.title();
    let sections = report.cached_sections();
    let show_zones = report.body.show_zones();
    let reset_all = std::mem::take(&mut report.reset_all_views);
    let fold = report.fold.take();
    let section_open = match fold {
        Some(Fold::ExpandAll) => Some(true),
        Some(Fold::CollapseSections) => Some(false),
        _ => None,
    };
    let mut fold_next = None;
    let Report {
        id,
        body,
        plots,
        closed_sections,
        collapsed,
        remove_requested,
        edit_requested,
        reset_all_views,
        new_columns,
        runs_requested,
        new_reports,
        new_tables,
        new_derived,
        fold: fold_slot,
        in_own_window,
        window_requested,
        dashboard_requested,
        export_requested,
        style_request,
        duplicate_requested,
        in_dashboard,
        scope,
        scope_ui,
        pending,
        rows_moved,
        job_error,
        cancelled,
        ..
    } = report;
    let updating = pending.is_some();
    // A result whose rows moved would brush the wrong rows: hide it until
    // the new one is ready.
    let hide_stale = (updating || cancelled.is_some()) && *rows_moved;
    let window = (*in_own_window, window_requested);
    let in_dashboard = *in_dashboard;
    let id = *id;
    // The report as a bootstrap source, when its tables can be bootstrapped.
    let bootstrap: Option<(SavedReport, String)> = {
        let saved = body.saved().with_scope(scope.clone());
        app_model::resampling::can_bootstrap(&saved).then(|| (saved, title.clone()))
    };
    let root = egui::CollapsingHeader::new(RichText::new(title).strong().size(15.0 * theme::text_scale(ui)))
        .id_salt(("report", id))
        .default_open(true)
        .open(match fold {
            Some(Fold::CollapseReport) => Some(false),
            Some(Fold::ExpandAll | Fold::OpenReport) => Some(true),
            _ => None,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                crate::menus::menu_button(ui, "⋯ Options", |ui| {
                    if !in_dashboard {
                        edit_item(ui, body, edit_requested, (window.0, &mut *window.1), dashboard_requested);
                        if ui
                            .button("Duplicate on another table…")
                            .on_hover_text(
                                "The same analysis or graph, with its options and plot styles, on another open \
                                 table; columns are matched by name, or chosen when names differ",
                            )
                            .clicked()
                        {
                            *duplicate_requested = true;
                            ui.close_menu();
                        }
                    }
                    options_menu(ui, body, reset_all_views, remove_requested, new_columns, runs_requested, new_reports, rows);
                    if matches!(body, ReportBody::Graph { .. }) {
                        ui.separator();
                        crate::plot_style_menu::menu_items(ui, None, style_request);
                    }
                    stacked_table_item(ui, body, table, new_derived);
                    ui.separator();
                    // Graphs have a row filter and column pickers of their own.
                    if !matches!(body, ReportBody::Graph { .. }) {
                        crate::report_scope_ui::menu_items(ui, &body.saved(), table, scope, scope_ui);
                        ui.separator();
                    }
                    if !in_dashboard {
                        export_item(ui, export_requested);
                    }
                    if ui
                        .button("Copy as script")
                        .on_hover_text(
                            "Copy the script call that makes this report, settings and display options included \
                             (paste it into the Script window to run it again)",
                        )
                        .clicked()
                    {
                        let saved = body.saved().with_scope(scope.clone());
                        let call = app_model::action::analysis_call("t", &saved, table);
                        ui.ctx().copy_text(format!("let t = current_table();\n{call}\n"));
                        ui.close_menu();
                    }
                    forecast_save_item(ui, id, &body.saved(), table, rows, new_tables);
                    limits_save_item(ui, id, &body.saved(), table, rows, new_tables);
                    if let (Some((saved, report_title)), Ok(sections)) = (&bootstrap, &*sections) {
                        let tables: Vec<(&str, &TextTable)> = sections
                            .iter()
                            .filter(|s| !is_pooled_section(s.kind))
                            .filter_map(|s| match &s.content {
                                SectionContent::Table(t) => Some((s.title.as_str(), t)),
                                _ => None,
                            })
                            .collect();
                        if !tables.is_empty() {
                            ui.menu_button("Bootstrap a table", |ui| {
                                for (section, t) in tables {
                                    bootstrap_item(ui, &format!("{section}…"), saved, report_title, section, t);
                                }
                            });
                        }
                    }
                    ui.separator();
                    if ui.button("Collapse all sections").clicked() {
                        fold_next = Some(Fold::CollapseSections);
                        ui.close_menu();
                    }
                    if ui.button("Expand all sections").clicked() {
                        fold_next = Some(Fold::ExpandAll);
                        ui.close_menu();
                    }
                });
                if !in_dashboard
                    && ui
                        .small_button("Export…")
                        .on_hover_text("Save this report as a PDF, Word, PowerPoint, HTML, Markdown or Excel file")
                        .clicked()
                {
                    *export_requested = true;
                }
                let read_only = ui.ctx().data(|d| d.get_temp::<bool>(read_only_id())).unwrap_or(false);
                if !read_only && ui.small_button("×").on_hover_text("Remove this report").clicked() {
                    *remove_requested = true;
                }
                let counts = rows.counts();
                // On a dashboard, rows its filters leave out are "filtered out", not "excluded".
                let filtered = |r: usize| filtered_out.as_ref().is_some_and(|m| m.get(r).copied().unwrap_or(false));
                let n_filtered = filtered_out.as_ref().map_or(0, |m| m.iter().filter(|f| **f).count());
                let excluded = counts.excluded.saturating_sub(n_filtered);
                if excluded > 0 {
                    let note = format!("{excluded} of {} rows excluded", counts.total);
                    let link = ui
                        .link(RichText::new(note).weak().small())
                        .on_hover_text("Excluded rows are left out of the analysis. Click to select them.");
                    if link.clicked() {
                        let excluded: Vec<usize> =
                            (0..rows.len()).filter(|&r| rows.is_excluded(r) && !filtered(r)).collect();
                        rows.select(excluded, SelectionMode::Replace);
                    }
                }
                if n_filtered > 0 {
                    ui.label(
                        RichText::new(format!("{n_filtered} of {} rows filtered out", counts.total)).weak().small(),
                    )
                    .on_hover_text(
                        "A row filter (the report's own, or a dashboard's) leaves these rows out of this report. \
                         The table's own rows are not changed: its grid and other reports still use them.",
                    );
                }
                if let Some(p) = pending.as_ref() {
                    ui.add(egui::Spinner::new().size(12.0));
                    let (done, total) = p.interrupt.progress();
                    let text = if total > 0 {
                        format!("Updating… {} of {total}", done.min(total))
                    } else {
                        "Updating…".to_owned()
                    };
                    ui.label(RichText::new(text).weak().small()).on_hover_text(
                        "The report is being computed again for the changed data; the previous result is shown \
                         dimmed until the new one is ready",
                    );
                    if ui.small_button("Cancel").on_hover_text("Stop computing and keep the previous result").clicked()
                    {
                        p.interrupt.cancel();
                        *cancelled = Some(p.key.clone());
                        *pending = None;
                    } else {
                        // Keep the progress count moving.
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
                    }
                } else if cancelled.is_some() {
                    ui.label(RichText::new("Computation cancelled").weak().small()).on_hover_text(
                        "The report shows its result from before the last change and is not computed again until \
                         you ask",
                    );
                    if ui.small_button("Compute again").clicked() {
                        *cancelled = None;
                        ui.ctx().request_repaint();
                    }
                }
            });
            if let Some(message) = job_error {
                ui.colored_label(theme::ERROR, format!("⚠ {message}"));
            }
            crate::report_scope_ui::filter_strip(ui, scope, scope_ui, table, id);
            if scope.switcher.is_some() {
                let request = body.saved();
                crate::report_scope_ui::switcher_bar(ui, &request, scope, scope_ui, table, id);
                if let ReportBody::Capability { request, .. } = &mut *body {
                    let column = table.column(request.column).map(|c| c.name.clone()).unwrap_or_default();
                    crate::report_scope_ui::limits_editor(ui, &mut request.limits, &column, id);
                }
            }
            if let Some((ok, message)) = ui.ctx().data(|d| d.get_temp::<(bool, String)>(figure_status_id(id))) {
                if ok {
                    ui.label(RichText::new(message).weak());
                } else {
                    ui.colored_label(theme::ERROR, format!("⚠ {message}"));
                }
            }
            if let (false, ReportBody::Scatter3d { analysis: Ok(a), options, view, .. }) = (hide_stale, &mut *body) {
                egui::CollapsingHeader::new(RichText::new("3-D Scatter Plot").strong())
                    .id_salt(("section", id, "scatter3d"))
                    .default_open(true)
                    .open(section_open)
                    .show(ui, |ui| crate::scatter3d_view::show(ui, a, options, view, rows, id));
            }
            if updating {
                ui.multiply_opacity(0.55);
            }
            match &*sections {
                _ if hide_stale => {
                    ui.label(RichText::new("Rows were added, removed or reordered; updating the report…").weak());
                }
                Err(message) => {
                    ui.colored_label(theme::ERROR, message.as_str());
                }
                Ok(sections) => {
                    // Sections of the same kind (e.g. one quantile plot per
                    // fitted distribution) need their own ids and plot state.
                    let mut seen: HashMap<SectionKind, usize> = HashMap::new();
                    for section in sections {
                        let kind = section.kind;
                        let nth = {
                            let n = seen.entry(kind).or_default();
                            *n += 1;
                            *n - 1
                        };
                        let key = (kind, nth);
                        let style_key = app_model::plot_style_key(kind, nth);
                        if app_model::report_figure::is_plot(&section.content) {
                            let state = plots.entry(key).or_insert_with(|| PlotState::for_section(section));
                            state.load_style(scope, &style_key);
                            state.prepare_reference(&section.content, rows);
                        }
                        // The plot's own font and text size, and its figure look
                        // for the output items (plots that keep a style).
                        let _text = scope.plot_style(&style_key).map(|st| crate::plot_axes::ScreenText::set(ui.ctx(), &st.export));
                        let look_target = crate::plot_export::target((id, style_key.as_str()));
                        let figure_look = plots.get(&key).filter(|st| st.default_style.is_some()).map(|_| {
                            (scope.plot_style(&style_key).map(|st| st.export.clone()).unwrap_or_default(), look_target)
                        });
                        ui.push_id(nth, |ui| {
                            if let ReportBody::MultipleResponses { request, analysis: Ok(a), view, .. } = &mut *body {
                                several_responses_view::before_section(ui, kind, request, a, view, id);
                            }
                            let shown = egui::CollapsingHeader::new(RichText::new(&section.title).strong())
                                .id_salt(("section", id, kind))
                                .default_open(!kind.starts_closed())
                                .open(section_open)
                                .show(ui, |ui| match &section.content {
                                    SectionContent::Table(t) => {
                                        text_table(ui, t, ("table", id, kind)).context_menu(|ui| {
                                            table_menu_items(ui, t, &section.title, id, new_tables, bootstrap.as_ref());
                                        });
                                    }
                                    SectionContent::Plot(p) => {
                                        scatter_plot(
                                            ui,
                                            &section.title,
                                            p,
                                            table,
                                            rows,
                                            plots.entry(key).or_insert_with(|| PlotState::for_plot(p, &section.title)),
                                            (id, kind),
                                            reset_all,
                                            p.contour.is_some().then_some(&mut scope.contour),
                                        );
                                    }
                                    SectionContent::ControlChart(c) => {
                                        let state = plots.entry(key).or_default();
                                        control_chart(
                                            ui,
                                            &section.title,
                                            c,
                                            rows,
                                            state,
                                            (id, kind),
                                            show_zones,
                                            reset_all,
                                        );
                                    }
                                    SectionContent::Bars(b) => {
                                        bar_chart(
                                            ui,
                                            &section.title,
                                            b,
                                            rows,
                                            plots.entry(key).or_default(),
                                            (id, kind),
                                            reset_all,
                                        );
                                    }
                                    SectionContent::BoxPlot(b) => {
                                        box_plot(
                                            ui,
                                            &section.title,
                                            b,
                                            rows,
                                            plots.entry(key).or_default(),
                                            (id, kind),
                                            reset_all,
                                        );
                                    }
                                    SectionContent::Survival(p) => {
                                        survival_plot(
                                            ui,
                                            &section.title,
                                            p,
                                            rows,
                                            plots.entry(key).or_default(),
                                            (id, kind),
                                            reset_all,
                                        );
                                    }
                                    SectionContent::Note(n) => {
                                        ui.label(n.as_str());
                                    }
                                    SectionContent::Formula(f) => formula_block(ui, f, ("formula", id, kind)),
                                    SectionContent::EffectPareto(e) => effect_pareto(ui, e, ("pareto", id, kind)),
                                    SectionContent::Tree(d) => {
                                        crate::tree_view::show(ui, d, rows, id, kind, &section.title);
                                    }
                                    SectionContent::Pareto(p) => {
                                        pareto_chart(
                                            ui,
                                            &section.title,
                                            p,
                                            rows,
                                            plots.entry(key).or_default(),
                                            (id, kind),
                                            reset_all,
                                        );
                                    }
                                    SectionContent::CauseEffect(d) => {
                                        crate::diagram_view::show(ui, d, rows, id, kind, &section.title);
                                    }
                                    SectionContent::HeatMap(h) => crate::heat_map_view::show(ui, h),
                                    SectionContent::IntensityMap(m) => {
                                        intensity_map_chart(
                                            ui,
                                            &section.title,
                                            m,
                                            plots.entry(key).or_default(),
                                            (id, kind),
                                            reset_all,
                                        );
                                    }
                                    SectionContent::WordCloud(w) => {
                                        crate::word_cloud_view::show(ui, w, rows, id, section);
                                    }
                                    SectionContent::Raster(r) => {
                                        raster_chart(
                                            ui,
                                            &section.title,
                                            r,
                                            rows,
                                            plots.entry(key).or_default(),
                                            (id, kind),
                                            reset_all,
                                        );
                                    }
                                });
                            // Whether the section is (or is closing to) collapsed.
                            let open =
                                egui::collapsing_header::CollapsingState::load(ui.ctx(), shown.header_response.id)
                                    .is_none_or(|state| state.is_open());
                            if open {
                                closed_sections.remove(&key);
                            } else {
                                closed_sections.insert(key);
                            }
                            if let Some(state) = plots.get_mut(&key) {
                                if let Some(look) = crate::plot_export::take_change(ui.ctx(), look_target) {
                                    state.set_look(scope, &style_key, look);
                                }
                                state.keep_style(scope, &style_key);
                            }
                            // Right-clicking the plot itself offers the same figure items.
                            let plot_area = plots.get_mut(&key).and_then(|s| s.plot_response.take());
                            let layout = match (&plot_area, plots.get(&key)) {
                                (Some(area), Some(state)) => state.screen_layout(area.rect),
                                _ => FigureLayout::default(),
                            };
                            let reference = plots.get(&key).map(|s| s.reference.clone()).unwrap_or_default();
                            if let Some(area) =
                                plot_area.filter(|_| app_model::report_figure::is_plot(&section.content))
                            {
                                let mut zoom = None;
                                let mut lasso = plots.get(&key).is_some_and(|s| s.lasso);
                                let mut open_reference = false;
                                area.context_menu(|ui| {
                                    if let SectionContent::Plot(p) = &section.content {
                                        zoom = zoom_to_selection_item(ui, p, rows);
                                        if ui
                                            .checkbox(&mut lasso, "Lasso selection")
                                            .on_hover_text("Drag a freehand outline instead of a box to select points")
                                            .clicked()
                                        {
                                            ui.close_menu();
                                        }
                                        ui.separator();
                                    }
                                    if matches!(app_model::report_figure::chart_kind(&section.content), Some(true))
                                        && ui
                                            .button(crate::reference_lines_ui::OPEN_LABEL)
                                            .on_hover_text(crate::reference_lines_ui::OPEN_HINT)
                                            .clicked()
                                    {
                                        open_reference = true;
                                        ui.close_menu();
                                    }
                                    save_figure_menu(ui, section, rows, id, layout.clone(), figure_look.as_ref(), &reference);
                                    if figure_look.is_some() {
                                        ui.separator();
                                        crate::plot_style_menu::menu_items(ui, Some(&style_key), style_request);
                                    }
                                });
                                if let Some(state) = plots.get_mut(&key) {
                                    state.reference_open |= open_reference;
                                    if let Some(view) = zoom {
                                        state.bounds = Some(view);
                                        state.axis_drag = None;
                                    }
                                    state.lasso = lasso;
                                }
                            }
                            shown.header_response.context_menu(|ui| {
                                if app_model::report_figure::is_plot(&section.content) {
                                    save_figure_menu(ui, section, rows, id, layout, figure_look.as_ref(), &reference);
                                    ui.separator();
                                    if figure_look.is_some() {
                                        crate::plot_style_menu::menu_items(ui, Some(&style_key), style_request);
                                        ui.separator();
                                    }
                                }
                                if let SectionContent::Table(t) = &section.content {
                                    table_menu_items(ui, t, &section.title, id, new_tables, bootstrap.as_ref());
                                    ui.separator();
                                }
                                options_menu(
                                    ui,
                                    body,
                                    reset_all_views,
                                    remove_requested,
                                    new_columns,
                                    runs_requested,
                                    new_reports,
                                    rows,
                                );
                            });
                            if let ReportBody::MultipleResponses { request, analysis: Ok(a), view, .. } = &mut *body {
                                several_responses_view::after_section(ui, kind, request, a, view, new_tables);
                            }
                        });
                    }
                }
            }
            profiler_sections(ui, body, rows, id);
            if let ReportBody::DataSurface { analysis: Ok(a), options, surface, .. } = body {
                if options.surface {
                    egui::CollapsingHeader::new(RichText::new("3-D Surface").strong())
                        .id_salt(("section", id, "data_surface"))
                        .default_open(true)
                        .open(section_open)
                        .show(ui, |ui| {
                            crate::surface_view::show(ui, a, options, surface, rows, id, scope.contour.as_ref())
                        });
                }
            }
            if let ReportBody::Graph { spec, view } = body {
                graph_view::show(ui, spec, view, table, rows, id, reset_all);
                if let Some(r) = crate::plot_style_menu::take_canvas_request(ui.ctx(), id) {
                    *style_request = Some(r);
                }
            }
            if let ReportBody::Stepwise { analysis: Ok(a), options, .. } = body {
                if let Some(model) = a.selected() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Selected model:").strong());
                        ui.toggle_value(&mut options.show_profiler, "Response Explorer")
                            .on_hover_text("One panel per term of the selected model; drag to change settings");
                        ui.toggle_value(&mut options.show_surface_profiler, "3-D surface profiler")
                            .on_hover_text("Rotatable surface of the prediction over two continuous terms");
                        if ui
                            .button("Open as linear model report")
                            .on_hover_text(
                                "The selected model as a full linear model report, with plots and diagnostics",
                            )
                            .clicked()
                        {
                            new_reports.push(ReportRequest::Linear(model.spec().clone()));
                        }
                    });
                }
            }
            if let ReportBody::BestSubsets { analysis: Ok(a), .. } = body {
                if let Some(spec) = a.best_index().and_then(|i| a.model_spec(i)) {
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .button(format!(
                                "Open the best model by {} as a linear model report",
                                a.request().criterion.label()
                            ))
                            .on_hover_text("Any other listed model opens from the report's options menu")
                            .clicked()
                        {
                            new_reports.push(ReportRequest::Linear(spec));
                        }
                    });
                }
            }
        });
    if ui.ctx().data(|d| d.get_temp::<u64>(jump_to_report_id())) == Some(id) {
        ui.ctx().data_mut(|d| d.remove::<u64>(jump_to_report_id()));
        root.header_response.scroll_to_me(Some(egui::Align::TOP));
    }
    *collapsed = egui::collapsing_header::CollapsingState::load(ui.ctx(), root.header_response.id)
        .is_some_and(|state| !state.is_open());
    root.header_response.context_menu(|ui| {
        if !in_dashboard {
            edit_item(ui, body, edit_requested, (window.0, &mut *window.1), dashboard_requested);
        }
        options_menu(ui, body, reset_all_views, remove_requested, new_columns, runs_requested, new_reports, rows);
        if !in_dashboard {
            ui.separator();
            export_item(ui, export_requested);
        }
    });
    if fold_next.is_some() {
        *fold_slot = fold_next;
    }
}

/// The interactive profilers below a report's sections: the Response Explorer
/// and 3-D surface profiler of a linear model or of a stepwise search's
/// selected model, and the 3-D profiler of a response surface.
fn profiler_sections(ui: &mut egui::Ui, body: &mut ReportBody, rows: &mut RowStateManager, id: u64) {
    let header = |title: &str| egui::CollapsingHeader::new(RichText::new(title).strong()).default_open(true);
    match body {
        ReportBody::Linear { options, analysis: Ok(analysis), settings, surface, .. } => {
            if options.show_profiler {
                header("Response Explorer")
                    .id_salt(("section", id, "response_explorer"))
                    .show(ui, |ui| response_explorer::show(ui, analysis, settings, id));
            }
            if options.show_surface_profiler {
                header("3-D Response Surface").id_salt(("section", id, "surface_profiler")).show(ui, |ui| {
                    surface_profiler::show(
                        ui,
                        analysis,
                        settings,
                        &mut options.show_surface_observed,
                        surface,
                        rows,
                        id,
                    )
                });
            }
        }
        ReportBody::Stepwise { options, analysis: Ok(a), settings, surface, .. } => {
            let Some(model) = a.selected() else { return };
            if options.show_profiler {
                header("Selected Model: Response Explorer")
                    .id_salt(("section", id, "response_explorer"))
                    .show(ui, |ui| response_explorer::show(ui, model, settings, id));
            }
            if options.show_surface_profiler {
                header("Selected Model: 3-D Response Surface")
                    .id_salt(("section", id, "surface_profiler"))
                    .show(ui, |ui| {
                        surface_profiler::show(ui, model, settings, &mut options.show_observed, surface, rows, id)
                    });
            }
        }
        ReportBody::Surface { options, analysis: Ok(a), profile_settings, surface, .. } => {
            if options.show_profiler {
                header("Response Explorer")
                    .id_salt(("section", id, "response_explorer"))
                    .show(ui, |ui| response_explorer::show(ui, a, profile_settings, id));
            }
            if options.show_surface_profiler {
                header("3-D Response Surface").id_salt(("section", id, "surface_profiler")).show(ui, |ui| {
                    surface_profiler::show(
                        ui,
                        a,
                        profile_settings,
                        &mut options.show_surface_observed,
                        surface,
                        rows,
                        id,
                    )
                });
            }
        }
        ReportBody::Mixed { profilers, analysis: Ok(a), settings, surface, .. }
            if !a.request().predictors.is_empty() =>
        {
            model_profilers(ui, a, profilers, settings, surface, rows, id);
        }
        ReportBody::Glm { profilers, analysis: Ok(a), settings, surface, .. } => {
            model_profilers(ui, a, profilers, settings, surface, rows, id);
        }
        ReportBody::ProportionalHazards { profilers, analysis: Ok(a), settings, surface, .. } => {
            model_profilers(ui, a, profilers, settings, surface, rows, id);
        }
        ReportBody::SurvivalRegression { profilers, analysis: Ok(a), settings, surface, .. }
            if !a.request().predictors.is_empty() =>
        {
            model_profilers(ui, a, profilers, settings, surface, rows, id);
        }
        ReportBody::Regularized { profilers, analysis: Ok(a), settings, surface, .. } => {
            model_profilers(ui, a, profilers, settings, surface, rows, id);
        }
        ReportBody::DecisionTree { profilers, analysis: Ok(a), settings, surface, .. } => {
            model_profilers(ui, a, profilers, settings, surface, rows, id);
        }
        ReportBody::RandomForest { profilers, analysis: Ok(a), settings, surface, .. } => {
            model_profilers(ui, a, profilers, settings, surface, rows, id);
        }
        ReportBody::BoostedTrees { profilers, analysis: Ok(a), settings, surface, .. } => {
            model_profilers(ui, a, profilers, settings, surface, rows, id);
        }
        ReportBody::PartialLeastSquares { profilers, analysis: Ok(a), settings, surface, .. } => {
            model_profilers(ui, a, profilers, settings, surface, rows, id);
        }
        ReportBody::SupportVectorMachine { profilers, analysis: Ok(a), settings, surface, .. } => {
            model_profilers(ui, a, profilers, settings, surface, rows, id);
        }
        ReportBody::Mars { profilers, analysis: Ok(a), settings, surface, .. } => {
            model_profilers(ui, a, profilers, settings, surface, rows, id);
        }
        ReportBody::Nonlinear { profilers, analysis: Ok(a), settings, surface, .. } => {
            if let Some(model) = a.surface_model() {
                model_profilers(ui, model, profilers, settings, surface, rows, id);
            }
        }
        ReportBody::NeuralNetwork { profilers, analysis: Ok(a), settings, surface, .. } => {
            model_profilers(ui, a, profilers, settings, surface, rows, id);
        }
        ReportBody::GaussianProcess { profilers, analysis: Ok(a), settings, surface, .. } => {
            model_profilers(ui, a.as_ref(), profilers, settings, surface, rows, id);
        }
        ReportBody::FunctionalData { profilers, analysis: Ok(a), settings, surface, .. } if a.has_model() => {
            if profilers.show_profiler {
                egui::CollapsingHeader::new(RichText::new("Curve Explorer").strong())
                    .id_salt(("section", id, "response_explorer"))
                    .default_open(true)
                    .show(ui, |ui| response_explorer::show(ui, a.as_ref(), settings, id));
            }
            if profilers.show_surface_profiler {
                egui::CollapsingHeader::new(RichText::new("3-D Response Surface").strong())
                    .id_salt(("section", id, "surface_profiler"))
                    .default_open(true)
                    .show(ui, |ui| {
                        surface_profiler::show(
                            ui,
                            a.as_ref(),
                            settings,
                            &mut profilers.show_observed,
                            surface,
                            rows,
                            id,
                        )
                    });
            }
        }
        ReportBody::Mixture { profilers, analysis: Ok(a), settings, .. } if profilers.show_profiler => {
            header("Response Explorer (mixture)")
                .id_salt(("section", id, "response_explorer"))
                .show(ui, |ui| response_explorer::show(ui, a, settings, id));
        }
        _ => {}
    }
}

/// The Response Explorer and 3-D profiler of a model report, as toggled.
fn model_profilers(
    ui: &mut egui::Ui,
    model: &dyn app_model::SurfaceModel,
    profilers: &mut ProfilerOptions,
    settings: &mut Vec<TermValue>,
    surface: &mut SurfaceProfilerState,
    rows: &mut RowStateManager,
    id: u64,
) {
    if profilers.show_profiler {
        egui::CollapsingHeader::new(RichText::new("Response Explorer").strong())
            .id_salt(("section", id, "response_explorer"))
            .default_open(true)
            .show(ui, |ui| response_explorer::show(ui, model, settings, id));
    }
    if profilers.show_surface_profiler {
        egui::CollapsingHeader::new(RichText::new("3-D Response Surface").strong())
            .id_salt(("section", id, "surface_profiler"))
            .default_open(true)
            .show(ui, |ui| {
                surface_profiler::show(ui, model, settings, &mut profilers.show_observed, surface, rows, id)
            });
    }
}

/// The strip above each plot: a "Reset view" button and a reminder of the
/// gestures. Returns `true` when the button was clicked this frame.
fn plot_toolbar(ui: &mut egui::Ui, state: &mut PlotState, info: &AxisInfo<'_>) -> bool {
    plot_toolbar_with(ui, state, info, None)
}

/// [`plot_toolbar`] whose Plot settings menu ends with `extra` (the
/// contour settings of a contour plot).
fn plot_toolbar_with(
    ui: &mut egui::Ui,
    state: &mut PlotState,
    info: &AxisInfo<'_>,
    mut extra: Option<&mut dyn FnMut(&mut egui::Ui)>,
) -> bool {
    ui.horizontal(|ui| {
        let clicked = ui
            .small_button("Reset view")
            .on_hover_text("Fit the plot to its data again (double-clicking the plot also works)")
            .clicked();
        let PlotState { axes, bounds, last_view, reference_open, reference_supported, .. } = state;
        crate::menus::menu_button(ui, "Plot settings ▾", |ui| {
            plot_axes::axes_menu(ui, axes, bounds, *last_view, info);
            if let Some(extra) = extra.as_mut() {
                ui.separator();
                extra(ui);
            }
            if *reference_supported {
                ui.separator();
                if ui
                    .button(crate::reference_lines_ui::OPEN_LABEL)
                    .on_hover_text(crate::reference_lines_ui::OPEN_HINT)
                    .clicked()
                {
                    *reference_open = true;
                    ui.close_menu();
                }
            }
        })
        .response
        .on_hover_text("Title, ticks, grid lines, ranges, number format and axis titles (or right-click an axis)");
        ui.label(RichText::new("?").weak()).on_hover_text(PLOT_HINT);
        let window = ui.make_persistent_id("reference_lines_window");
        crate::reference_lines_ui::window(
            ui.ctx(),
            window,
            info.title,
            &mut state.reference,
            &mut state.reference_open,
            state.reference_scopes,
        );
        clicked
    })
    .inner
}

/// Key entries of a plot's reference lines and bands.
fn reference_key(key: &mut PlotKey, refs: &stat_viz::element::reference::ReferenceLayers, dark: bool) {
    let c32 = |c: stat_viz::Rgba| stat_viz::render::color32(if dark { stat_viz::palette::for_dark(c) } else { c });
    for e in refs.back.key.iter().chain(&refs.front.key) {
        match e.glyph {
            stat_viz::KeyGlyph::Area(fill) => key.area(e.label.clone(), c32(fill), c32(e.color)),
            stat_viz::KeyGlyph::Line(p) => key.line(e.label.clone(), c32(e.color), LinePattern::from_viz(p)),
            stat_viz::KeyGlyph::Marker(..) => {}
        }
    }
}

/// `view` widened to show the plot's reference lines and bands.
fn reference_view(view: Option<ViewBounds>, refs: &stat_viz::element::reference::ReferenceLayers) -> Option<ViewBounds> {
    use stat_viz::element::reference::widen_range;
    let v = view?;
    let [ex, ey] = refs.extent();
    let (x_min, x_max) = widen_range((v.x_min, v.x_max), ex);
    let (y_min, y_max) = widen_range((v.y_min, v.y_max), ey);
    Some(ViewBounds { x_min, x_max, y_min, y_max })
}

/// Notes about reference lines that could not be drawn, under the plot.
fn reference_notes(ui: &mut egui::Ui, refs: &stat_viz::element::reference::ReferenceLayers) {
    for n in &refs.front.notes {
        ui.label(RichText::new(n).weak().small());
    }
}

/// Formula text in a fixed-width font, selectable, scrolling sideways when
/// lines are long, with a button that copies it.
fn formula_block(ui: &mut egui::Ui, formula: &str, id: impl std::hash::Hash) {
    if ui.button("Copy formula").on_hover_text("Copy the formula text to the clipboard").clicked() {
        ui.ctx().copy_text(formula.to_owned());
    }
    egui::ScrollArea::horizontal()
        .id_salt(id)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(RichText::new(formula).text_style(egui::TextStyle::Monospace))
                    .extend()
                    .selectable(true),
            );
        });
}

/// A button per column the report can add to the table.
fn save_column_buttons(ui: &mut egui::Ui, columns: Vec<app_model::ReportColumn>, new_columns: &mut Vec<NewColumn>) {
    for c in columns {
        if ui.button(&c.label).clicked() {
            new_columns.push(NewColumn(c));
            ui.close_menu();
        }
    }
}

/// One checkbox per run rule, toggling it in `rules`.
fn rule_checkboxes(ui: &mut egui::Ui, rules: &mut app_model::RuleSet) {
    for rule in RunRule::ALL {
        let mut on = rules.contains(rule);
        if ui.checkbox(&mut on, format!("{}. {}", rule.number(), rule.description())).changed() {
            rules.set(rule, on);
        }
    }
    ui.horizontal(|ui| {
        if ui.small_button("All").clicked() {
            *rules = app_model::RuleSet::ALL;
        }
        if ui.small_button("Limits only").clicked() {
            *rules = app_model::RuleSet::LIMITS_ONLY;
        }
    });
}

/// Where the app puts the dashboards of the project whose reports are being
/// drawn, as (id, name), for the "Add to dashboard" menu.
pub fn dashboard_choices_id() -> egui::Id {
    egui::Id::new("report_dashboard_choices")
}

/// "Edit analysis…": reopen the report's dialog filled from its settings.
///
/// Also "Open in new window" (or, for a report already in its own window,
/// "Back to main window"): `window` is whether it is in its own window, and
/// the flag that asks the app to move it; and "Add to dashboard", which
/// sets `dashboard` to the dashboard chosen (`Some(None)`: a new one).
fn edit_item(
    ui: &mut egui::Ui,
    body: &ReportBody,
    edit: &mut bool,
    window: (bool, &mut bool),
    dashboard: &mut Option<Option<u64>>,
) {
    let saved = body.saved();
    let enabled = crate::app::editable(&saved);
    let button = ui.add_enabled(enabled, egui::Button::new("Edit analysis…"));
    let button = if enabled {
        button.on_hover_text("Change the columns or settings in the dialog; the report is updated in place")
    } else {
        button.on_disabled_hover_text(match saved {
            SavedReport::Graph { .. } => "Change a graph on its canvas: drag columns onto the shelves",
            _ => "This report is changed from its own options",
        })
    };
    if button.clicked() {
        *edit = true;
        ui.close_menu();
    }
    let (in_own_window, requested) = window;
    let (label, tip) = if in_own_window {
        ("Back to main window", "Close this window and show the report in the main window's results again")
    } else {
        ("Open in new window", "Show this report in a window of its own, which can go on another screen")
    };
    if ui.button(label).on_hover_text(tip).clicked() {
        *requested = true;
        ui.close_menu();
    }
    let choices: Vec<(u64, String)> = ui
        .ctx()
        .data(|d| d.get_temp::<Arc<Vec<(u64, String)>>>(dashboard_choices_id()))
        .map_or_else(Vec::new, |c| (*c).clone());
    ui.menu_button("Add to dashboard", |ui| {
        for (id, name) in &choices {
            if ui.button(name).on_hover_text("A copy of this report as a tile of this dashboard").clicked() {
                *dashboard = Some(Some(*id));
                ui.close_menu();
            }
        }
        if !choices.is_empty() {
            ui.separator();
        }
        if ui
            .button("New dashboard")
            .on_hover_text("A new dashboard in this table's project, starting with a copy of this report")
            .clicked()
        {
            *dashboard = Some(None);
            ui.close_menu();
        }
    });
    ui.separator();
}

/// Report options, shown from the "⋯" button and from right-clicking any heading.
#[allow(clippy::too_many_arguments)]
fn options_menu(
    ui: &mut egui::Ui,
    body: &mut ReportBody,
    reset_views: &mut bool,
    remove: &mut bool,
    new_columns: &mut Vec<NewColumn>,
    runs_requested: &mut bool,
    new_reports: &mut Vec<ReportRequest>,
    rows: &mut RowStateManager,
) {
    match body {
        ReportBody::Linear { options: o, analysis, .. } => {
            ui.checkbox(&mut o.show_anova, "Analysis of variance");
            let one_way = analysis.as_ref().is_ok_and(LinearModelAnalysis::is_one_way);
            let levels = analysis.as_ref().ok().and_then(LinearModelAnalysis::one_way_levels).unwrap_or_default();
            ui.add_enabled_ui(one_way, |ui| one_way_options(ui, o, levels))
                .response
                .on_disabled_hover_text("Available when the model has one categorical factor only (one-way ANOVA)");
            ui.checkbox(&mut o.show_effect_sizes, "Effect sizes (η², partial η², ω², Cohen's f)")
                .on_hover_text("For every effect, with intervals from the noncentral F distribution");
            ui.menu_button("Comparisons within slices", |ui| {
                ui.label(RichText::new("Each factor of a two-factor interaction at every level of the other").weak());
                crate::model_terms_ui::adjust_radios(ui, &mut o.slices);
            });
            ui.menu_button("Slopes by group", |ui| {
                ui.label(RichText::new("Each group's slope for a factor × continuous effect").weak());
                crate::model_terms_ui::adjust_radios(ui, &mut o.slopes);
            });
            ui.checkbox(&mut o.show_confidence_limits, "Confidence limits for parameters");
            ui.checkbox(&mut o.show_fit_plot, "Fit plot");
            ui.add_enabled(o.show_fit_plot, egui::Checkbox::new(&mut o.show_confidence_band, "Confidence band"));
            ui.checkbox(&mut o.show_residual_plot, "Residual plot");
            ui.checkbox(&mut o.show_diagnostics, "Influence diagnostics (leverage, Cook's distance)");
            ui.checkbox(&mut o.show_profiler, "Response Explorer");
            ui.checkbox(&mut o.show_surface_profiler, "3-D surface profiler");
            if let Ok(a) = analysis {
                ui.separator();
                save_column_buttons(ui, a.saveable_columns(), new_columns);
            }
        }
        ReportBody::ModelSearch { analysis: Ok(a), .. } => {
            ui.label(RichText::new("Open a model's full report").weak());
            for kind in a.ranked() {
                let label = format!("{}{}", kind.label(), if a.best() == Some(kind) { " (best)" } else { "" });
                if ui.button(label).on_hover_text(a.model_title(kind).unwrap_or_default()).clicked() {
                    new_reports.extend(model_report_request(a.model_report(kind)));
                    ui.close_menu();
                }
            }
        }
        ReportBody::Stepwise { options: o, analysis, .. } => {
            let selected = analysis.as_ref().ok().and_then(StepwiseAnalysis::selected);
            ui.add_enabled_ui(selected.is_some(), |ui| {
                ui.checkbox(&mut o.show_profiler, "Response Explorer for the selected model");
                ui.checkbox(&mut o.show_surface_profiler, "3-D surface profiler for the selected model");
            });
            if let Some(model) = selected {
                if ui.button("Open selected model as a linear model report").clicked() {
                    new_reports.push(ReportRequest::Linear(model.spec().clone()));
                    ui.close_menu();
                }
            }
        }
        ReportBody::Chart { options: o, request, analysis, .. } => {
            ui.menu_button("Chart type", |ui| {
                for kind in ChartKind::ALL {
                    let enabled = kind != ChartKind::ShortRun || request.part.is_some();
                    let button = ui
                        .add_enabled(enabled, egui::RadioButton::new(request.kind == kind, kind.label()))
                        .on_disabled_hover_text("Needs a part-number column: create it from Quality ▸ Control chart");
                    if button.clicked() {
                        request.kind = kind;
                        if kind.subgroup_use() == SubgroupUse::Subgroups && request.subgroup_size < 2 {
                            request.subgroup_size = 5;
                        }
                        // σ multiples, wide moving ranges and standard values apply to three kinds only.
                        if !matches!(
                            kind,
                            ChartKind::IndividualsAndMovingRange | ChartKind::MeanAndRange | ChartKind::MeanAndStdDev
                        ) {
                            (request.sigma_multiple, request.standard) = (3.0, None);
                        }
                        if kind != ChartKind::IndividualsAndMovingRange {
                            request.range_span = 2;
                        }
                        ui.close_menu();
                    }
                }
            });
            if request.kind.subgroup_use() != SubgroupUse::Individuals && request.subgroup.is_none() {
                let min = if request.kind.subgroup_use() == SubgroupUse::Either { 1 } else { 2 };
                ui.horizontal(|ui| {
                    ui.label("Subgroup size:");
                    ui.add(egui::DragValue::new(&mut request.subgroup_size).range(min..=MAX_RANGE_SUBGROUP));
                })
                .response
                .on_hover_text("1 plots individual values");
            }
            if request.kind == ChartKind::MovingAverage {
                ui.horizontal(|ui| {
                    ui.label("Moving-average span:");
                    ui.add(egui::DragValue::new(&mut request.span).range(2..=50));
                });
            }
            ui.separator();
            let dispersion_label = if request.kind.is_three_way() {
                "Moving range and within-subgroup charts"
            } else {
                "Dispersion chart"
            };
            ui.checkbox(&mut o.show_dispersion_chart, dispersion_label);
            ui.checkbox(&mut o.show_zones, "1σ and 2σ zone lines");
            ui.checkbox(&mut o.show_violation_table, "Table of flagged points");
            ui.checkbox(&mut o.show_run_chart, "Run chart of every value");
            if o.show_run_chart {
                ui.horizontal(|ui| {
                    ui.label("Join and center on:");
                    ui.radio_value(&mut o.run_center, app_model::RunCenter::Median, "medians");
                    ui.radio_value(&mut o.run_center, app_model::RunCenter::Mean, "means");
                });
            }
            ui.separator();
            let (location, dispersion, within) =
                analysis.as_ref().map_or(("Location chart", "Dispersion chart", None), |a| {
                    let c = a.charts();
                    (
                        c.location.statistic.label(),
                        c.dispersion.statistic.label(),
                        c.within.as_ref().map(|w| w.statistic.label()),
                    )
                });
            if request.kind.pattern_rules_apply() {
                ui.menu_button(format!("Run rules: {location} chart"), |ui| {
                    rule_checkboxes(ui, &mut o.location_rules);
                });
            } else {
                let mut on = o.location_rules.contains(RunRule::BeyondLimits);
                let hint = if request.kind == ChartKind::Zone {
                    "The zone scores replace the pattern rules (2–8) on a zone chart"
                } else {
                    "Moving averages are correlated, so only rule 1 applies"
                };
                if ui.checkbox(&mut on, "Rule 1: point beyond a limit").on_hover_text(hint).changed() {
                    o.location_rules.set(RunRule::BeyondLimits, on);
                }
            }
            ui.menu_button(format!("Run rules: {dispersion} chart"), |ui| {
                rule_checkboxes(ui, &mut o.dispersion_rules);
            });
            if let Some(within) = within {
                ui.menu_button(format!("Run rules: within-subgroup {within} chart"), |ui| {
                    rule_checkboxes(ui, &mut o.within_rules);
                });
            }
        }
        ReportBody::Attribute { request, .. } => {
            ui.menu_button("Chart type", |ui| {
                let has_size = request.size != app_model::SubgroupSize::None;
                for kind in app_model::AttributeChartKind::ALL {
                    let enabled = has_size || !kind.needs_size();
                    let button = ui
                        .add_enabled(enabled, egui::RadioButton::new(request.kind == kind, kind.label()))
                        .on_disabled_hover_text("Needs subgroup sizes: create it from Quality ▸ Attribute chart");
                    if button.clicked() {
                        request.kind = kind;
                        ui.close_menu();
                    }
                }
            });
            ui.menu_button("Run rules", |ui| rule_checkboxes(ui, &mut request.rules));
        }
        ReportBody::Capability { request, analysis, .. } => {
            if let Ok(a) = analysis {
                save_column_buttons(ui, a.saveable_columns(), new_columns);
                ui.separator();
            }
            ui.checkbox(&mut request.summary, "Summary page layout")
                .on_hover_text("Indices, control charts, last subgroups, histogram and probability plot on one page");
            crate::capability_dialog::report_options(ui, request);
            let percentile = request.model != app_model::CapabilityModel::Normal;
            let mut on = request.bootstrap.is_some();
            if ui
                .add_enabled(percentile, egui::Checkbox::new(&mut on, "Bootstrap intervals for percentile indices"))
                .on_disabled_hover_text("Available with a non-normal distribution model")
                .changed()
            {
                request.bootstrap = on.then(app_model::BootstrapSettings::default);
            }
            if let Some(b) = request.bootstrap.as_mut().filter(|_| percentile) {
                ui.horizontal(|ui| {
                    ui.label("Resamples:");
                    ui.add(egui::DragValue::new(&mut b.resamples).range(20..=100_000).speed(10.0));
                    ui.label("Seed:");
                    ui.add(crate::dialog_kit::seed_value(&mut b.seed));
                });
            }
        }
        ReportBody::Gauge { options: o, request, .. } => {
            if request.design.kind != app_model::GaugeStudyKind::Expanded {
                ui.checkbox(&mut o.show_anova, "Analysis of variance");
            }
            ui.checkbox(&mut o.show_part_plot, "Measurements by part");
            ui.checkbox(&mut o.show_operator_plot, "Measurements by operator");
            ui.checkbox(&mut o.show_run_chart, "Gauge run chart");
            if request.design.kind == app_model::GaugeStudyKind::Crossed {
                ui.separator();
                ui.checkbox(
                    &mut request.pool_interaction,
                    format!("Pool interaction into error when p > {POOLING_LEVEL}"),
                );
            } else if request.design.kind == app_model::GaugeStudyKind::Expanded {
                ui.separator();
                ui.checkbox(&mut request.design.interactions, "Two-factor interactions");
            }
        }
        ReportBody::Distribution { options: o, analysis, .. } => {
            if matches!(analysis, Ok(a) if a.censored().is_some()) {
                ui.checkbox(&mut o.show_chart, "Distribution function plot");
            } else if matches!(analysis, Ok(a) if a.is_continuous()) {
                ui.checkbox(&mut o.show_quantiles, "Quantiles");
                ui.checkbox(&mut o.show_robust, "Robust estimates");
                ui.checkbox(&mut o.show_chart, "Histogram");
                ui.checkbox(&mut o.show_box_plot, "Box plot");
                ui.checkbox(&mut o.show_normal_fit, "Normal fit and normality tests");
                ui.checkbox(&mut o.show_quantile_plot, "Normal quantile plot");
                ui.checkbox(&mut o.show_box_cox, "Box–Cox transformation (positive data)");
                ui.separator();
                distribution_test_options(ui, o);
                if let Ok(a) = analysis {
                    if o.outliers.is_some() {
                        let flagged = a.flagged_rows(o);
                        if ui
                            .add_enabled(!flagged.is_empty(), egui::Button::new("Select flagged rows"))
                            .on_hover_text("Select the rows the outlier test declares outliers")
                            .on_disabled_hover_text("The outlier test flags no rows")
                            .clicked()
                        {
                            rows.select(flagged, SelectionMode::Replace);
                            ui.close_menu();
                        }
                    }
                    ui.separator();
                    save_column_buttons(ui, a.saveable_columns(), new_columns);
                }
            } else {
                ui.checkbox(&mut o.show_chart, "Frequency chart");
            }
        }
        ReportBody::Bivariate { request, analysis } => {
            let grouped = analysis.as_ref().is_ok_and(BivariateAnalysis::is_grouped);
            ui.add_enabled_ui(grouped, |ui| pairwise_menu(ui, &mut request.pairwise))
                .response
                .on_disabled_hover_text("Available when one column is continuous and the other categorical");
            if let Ok(a) = analysis {
                a.set_pairwise(request.pairwise);
            }
        }
        ReportBody::BestSubsets { analysis: Ok(a), .. } => {
            ui.label(RichText::new("Open a model as a linear model report:").weak());
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for i in 0..a.models().len() {
                    let size = a.models()[i].size();
                    if ui.button(format!("{size}: {}", a.model_label(i))).clicked() {
                        if let Some(spec) = a.model_spec(i) {
                            new_reports.push(ReportRequest::Linear(spec));
                        }
                        ui.close_menu();
                    }
                }
            });
        }
        ReportBody::Pareto { options: o, .. } => {
            ui.horizontal(|ui| {
                ui.label("Pool categories below")
                    .on_hover_text("Categories with a smaller share of their chart's total share one \"Other\" bar");
                ui.add(egui::DragValue::new(&mut o.other_percent).range(0.0..=50.0).speed(0.5).suffix(" %"));
                ui.label("into \"Other\" (0 = off)");
            });
            ui.checkbox(&mut o.percent_scale, "Bar heights as percentages of the total");
        }
        ReportBody::MultiVari { options: o, .. } => {
            ui.checkbox(&mut o.show_values, "Individual values");
            ui.checkbox(&mut o.show_ranges, "Ranges view (range within each cell, with limits)");
            ui.checkbox(&mut o.show_cells, "Cell summary table");
            ui.add_enabled_ui(o.show_values, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Replicates:");
                    for layout in app_model::ReplicateLayout::ALL {
                        ui.radio_value(&mut o.replicates, layout, layout.label());
                    }
                })
                .response
                .on_hover_text("In order: each cell's values side by side in row order, joined like a short run chart");
            });
        }
        ReportBody::Survival { options: o, .. } => {
            ui.checkbox(&mut o.show_band, "Confidence bands");
            ui.add_enabled_ui(o.show_band || o.show_table, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Band scale:");
                    for t in [BandTransform::LogLog, BandTransform::Log, BandTransform::Plain] {
                        ui.radio_value(&mut o.band, t, t.label());
                    }
                });
            });
            ui.checkbox(&mut o.show_weibull, "Weibull fit (dashed)");
            ui.checkbox(&mut o.show_lognormal, "Lognormal fit (dotted)");
            ui.checkbox(&mut o.show_table, "Product-limit table");
            ui.checkbox(&mut o.risk_table, "Number at risk under the curves");
            ui.add_enabled_ui(o.risk_table, |ui| {
                ui.checkbox(&mut o.risk_censored, "Also count censored rows");
            });
            ui.checkbox(&mut o.trend, "Log-rank test for trend (groups in order)")
                .on_hover_text("Scores are the group labels when they are numbers, else 1, 2, 3, …");
        }
        ReportBody::Surface { options: o, analysis, .. } => {
            ui.checkbox(&mut o.show_coefficients, "Coefficient table");
            ui.checkbox(&mut o.show_ridge, "Ridge tables (maximum and minimum response)");
            ui.checkbox(&mut o.show_profiler, "Response Explorer");
            ui.checkbox(&mut o.show_surface_profiler, "3-D surface profiler");
            if let Ok(a) = analysis {
                let names = a.factor_names();
                if names.len() >= 2 {
                    ui.separator();
                    ui.label("Contour plot axes:");
                    let (mut cx, mut cy) = (o.contour_x, o.contour_y);
                    contour_axis_choice(ui, "Horizontal", &mut cx, cy, names);
                    contour_axis_choice(ui, "Vertical", &mut cy, cx, names);
                    o.contour_x = cx;
                    o.contour_y = cy;
                }
            }
        }
        ReportBody::Mixed { profilers: o, analysis, .. } => {
            let has_predictors = matches!(analysis, Ok(a) if !a.request().predictors.is_empty());
            ui.add_enabled_ui(has_predictors, |ui| {
                ui.checkbox(&mut o.show_profiler, "Response Explorer (population level)");
                ui.checkbox(&mut o.show_surface_profiler, "3-D surface profiler (population level)");
            });
            if let Ok(a) = analysis {
                ui.separator();
                save_column_buttons(ui, a.saveable_columns(), new_columns);
            }
        }
        ReportBody::Nonlinear { profilers: o, analysis: Ok(a), .. } => {
            ui.add_enabled_ui(a.surface_model().is_some(), |ui| {
                ui.checkbox(&mut o.show_profiler, "Response Explorer");
                ui.checkbox(&mut o.show_surface_profiler, "3-D surface profiler");
            });
            ui.separator();
            save_column_buttons(ui, a.saveable_columns(), new_columns);
        }
        ReportBody::Glm { profilers: o, analysis, .. } => {
            ui.checkbox(&mut o.show_profiler, "Response Explorer");
            ui.checkbox(&mut o.show_surface_profiler, "3-D surface profiler");
            if let Ok(a) = analysis {
                ui.separator();
                save_column_buttons(ui, a.saveable_columns(), new_columns);
            }
        }
        ReportBody::ProportionalHazards { profilers: o, analysis, .. } => {
            ui.checkbox(&mut o.show_profiler, "Response Explorer");
            ui.checkbox(&mut o.show_surface_profiler, "3-D surface profiler");
            if let Ok(a) = analysis {
                ui.separator();
                save_column_buttons(ui, a.saveable_columns(), new_columns);
            }
        }
        ReportBody::SurvivalRegression { profilers: o, analysis, .. } => {
            let has_predictors = matches!(analysis, Ok(a) if !a.request().predictors.is_empty());
            ui.add_enabled_ui(has_predictors, |ui| {
                ui.checkbox(&mut o.show_profiler, "Response Explorer");
                ui.checkbox(&mut o.show_surface_profiler, "3-D surface profiler");
            });
            if let Ok(a) = analysis {
                ui.separator();
                save_column_buttons(ui, a.saveable_columns(), new_columns);
            }
        }
        ReportBody::Regularized { profilers: o, analysis, .. } => {
            ui.checkbox(&mut o.show_profiler, "Response Explorer");
            ui.checkbox(&mut o.show_surface_profiler, "3-D surface profiler");
            if let Ok(a) = analysis {
                ui.separator();
                save_column_buttons(ui, a.saveable_columns(), new_columns);
            }
        }
        ReportBody::DecisionTree { profilers: o, analysis, .. } => {
            ui.checkbox(&mut o.show_profiler, "Response Explorer");
            ui.checkbox(&mut o.show_surface_profiler, "3-D surface profiler");
            if let Ok(a) = analysis {
                ui.separator();
                save_column_buttons(ui, a.saveable_columns(), new_columns);
            }
        }
        ReportBody::PartialLeastSquares { profilers: o, analysis, .. } => {
            ui.checkbox(&mut o.show_profiler, "Response Explorer");
            ui.checkbox(&mut o.show_surface_profiler, "3-D surface profiler");
            if let Ok(a) = analysis {
                ui.separator();
                save_column_buttons(ui, a.saveable_columns(), new_columns);
            }
        }
        ReportBody::FunctionalData { profilers: o, analysis, .. } => {
            if analysis.as_ref().is_ok_and(|a| a.has_model()) {
                ui.checkbox(&mut o.show_profiler, "Curve Explorer");
                ui.checkbox(&mut o.show_surface_profiler, "3-D surface profiler");
                ui.separator();
            }
            if let Ok(a) = analysis {
                save_column_buttons(ui, a.saveable_columns(), new_columns);
            }
        }
        ReportBody::Mars { profilers: o, analysis, .. } => {
            ui.checkbox(&mut o.show_profiler, "Response Explorer");
            ui.checkbox(&mut o.show_surface_profiler, "3-D surface profiler");
            if let Ok(a) = analysis {
                ui.separator();
                save_column_buttons(ui, a.saveable_columns(), new_columns);
            }
        }
        ReportBody::NeuralNetwork { profilers: o, analysis, .. } => {
            ui.checkbox(&mut o.show_profiler, "Response Explorer");
            ui.checkbox(&mut o.show_surface_profiler, "3-D surface profiler");
            if let Ok(a) = analysis {
                ui.separator();
                save_column_buttons(ui, a.saveable_columns(), new_columns);
            }
        }
        ReportBody::GaussianProcess { profilers: o, analysis, .. } => {
            ui.checkbox(&mut o.show_profiler, "Response Explorer");
            ui.checkbox(&mut o.show_surface_profiler, "3-D surface profiler");
            if let Ok(a) = analysis {
                ui.separator();
                save_column_buttons(ui, a.saveable_columns(), new_columns);
            }
        }
        ReportBody::SupportVectorMachine { profilers: o, analysis, .. } => {
            ui.checkbox(&mut o.show_profiler, "Response Explorer");
            ui.checkbox(&mut o.show_surface_profiler, "3-D surface profiler");
            if let Ok(a) = analysis {
                ui.separator();
                save_column_buttons(ui, a.saveable_columns(), new_columns);
            }
        }
        ReportBody::BoostedTrees { profilers: o, analysis, .. } => {
            ui.checkbox(&mut o.show_profiler, "Response Explorer");
            ui.checkbox(&mut o.show_surface_profiler, "3-D surface profiler");
            if let Ok(a) = analysis {
                ui.separator();
                save_column_buttons(ui, a.saveable_columns(), new_columns);
            }
        }
        ReportBody::RandomForest { profilers: o, analysis, .. } => {
            ui.checkbox(&mut o.show_profiler, "Response Explorer");
            ui.checkbox(&mut o.show_surface_profiler, "3-D surface profiler");
            if let Ok(a) = analysis {
                ui.separator();
                save_column_buttons(ui, a.saveable_columns(), new_columns);
            }
        }
        ReportBody::Mixture { profilers: o, .. } => {
            ui.checkbox(&mut o.show_profiler, "Response Explorer (moving one component rescales the others)");
        }
        ReportBody::Comparison { current, analysis, .. } => {
            ui.label(RichText::new("Settings (the report updates as you change them)").strong());
            let labels = match analysis {
                Ok(ComparisonReport::Groups(a)) => a.labels().to_vec(),
                Ok(ComparisonReport::Crossover(a)) => a.treatments().to_vec(),
                Ok(ComparisonReport::Bayesian(a)) => a.parameters.clone(),
                _ => Vec::new(),
            };
            compare_ui::report_settings(ui, current, &labels);
            match analysis {
                Ok(ComparisonReport::PValues(a)) => {
                    ui.separator();
                    save_column_buttons(ui, a.saveable_columns(), new_columns);
                }
                Ok(ComparisonReport::Area(a)) => {
                    ui.separator();
                    save_column_buttons(ui, a.saveable_columns(), new_columns);
                }
                Ok(ComparisonReport::Screening(a)) => {
                    ui.separator();
                    screening_menu(ui, a, new_reports);
                }
                Ok(ComparisonReport::Sequential(a)) if !a.proposals().is_empty() => {
                    ui.separator();
                    if ui
                        .button("Add proposed runs to the table")
                        .on_hover_text(
                            "Append the proposed runs as new rows with their factor settings and the next round; \
                             measure the responses, type them in, and the report proposes the next batch",
                        )
                        .clicked()
                    {
                        *runs_requested = true;
                        ui.close_menu();
                    }
                }
                Ok(ComparisonReport::Signal(a)) => {
                    ui.separator();
                    save_column_buttons(ui, a.saveable_columns(), new_columns);
                }
                _ => {}
            }
        }
        ReportBody::Pca { analysis: Ok(a), .. } => {
            // The first three components; scripts can save any.
            let mut columns = a.saveable_columns();
            columns.truncate(3);
            save_column_buttons(ui, columns, new_columns);
        }
        ReportBody::Graph { spec, view } => graph_view::output::options_menu(ui, spec, view),
        ReportBody::Scatter3d { options: o, .. } => {
            ui.checkbox(&mut o.drop_lines, "Drop lines");
            if ui.button("Reset view").clicked() {
                o.camera = app_model::Camera::default();
                ui.close_menu();
            }
        }
        ReportBody::DataSurface { request, options: o, .. } => {
            ui.checkbox(&mut o.contour_plot, "Contour plot");
            ui.checkbox(&mut o.surface, "3-D surface");
            ui.separator();
            ui.label("Surface through the data:");
            for m in app_model::SurfaceFit::ALL {
                ui.radio_value(&mut request.method, m, m.label());
            }
        }
        ReportBody::MultipleResponses { analysis: Ok(a), .. } => {
            if ui
                .button("Open each response's linear model report")
                .on_hover_text("Full reports with estimates, tests and diagnostics, one per response")
                .clicked()
            {
                // A response predicted by its mean has no model to open.
                new_reports.extend(
                    a.models().iter().filter_map(|m| m.linear()).map(|m| ReportRequest::Linear(m.spec().clone())),
                );
                ui.close_menu();
            }
        }
        ReportBody::Cluster { analysis: Ok(a), .. } => save_column_buttons(ui, a.saveable_columns(), new_columns),
        ReportBody::OutlierDistances { analysis: Ok(a), .. } => {
            save_column_buttons(ui, a.saveable_columns(), new_columns);
        }
        ReportBody::ItemAnalysis { analysis: Ok(a), .. } => save_column_buttons(ui, a.saveable_columns(), new_columns),
        ReportBody::FactorAnalysis { analysis: Ok(a), .. } => {
            save_column_buttons(ui, a.saveable_columns(), new_columns);
        }
        ReportBody::DiscriminantAnalysis { analysis: Ok(a), .. } => {
            save_column_buttons(ui, a.saveable_columns(), new_columns);
        }
        ReportBody::GaussianMixture { analysis: Ok(a), .. } => {
            save_column_buttons(ui, a.saveable_columns(), new_columns);
        }
        ReportBody::LatentClass { analysis: Ok(a), .. } => save_column_buttons(ui, a.saveable_columns(), new_columns),
        ReportBody::TextExploration { analysis: Ok(a), .. } => {
            save_column_buttons(ui, a.saveable_columns(), new_columns)
        }
        ReportBody::TopicModel { analysis: Ok(a), .. } => save_column_buttons(ui, a.saveable_columns(), new_columns),
        ReportBody::Sentiment { analysis: Ok(a), .. } => save_column_buttons(ui, a.saveable_columns(), new_columns),
        ReportBody::Embedding { analysis: Ok(a), .. } => save_column_buttons(ui, a.saveable_columns(), new_columns),
        ReportBody::Imputation { analysis: Ok(a), .. } => save_column_buttons(ui, a.saveable_columns(), new_columns),
        ReportBody::ImageAnalysis { request, analysis, .. } => {
            crate::image_analysis_dialog::options_menu(ui, request);
            if let Ok(a) = analysis {
                let columns = a.saveable_columns();
                if !columns.is_empty() {
                    ui.separator();
                    save_column_buttons(ui, columns, new_columns);
                }
            }
        }
        ReportBody::SignalProcessing { request, analysis, .. } => {
            crate::signal_processing_dialog::options_menu(ui, request);
            if let Ok(a) = analysis {
                let columns = a.saveable_columns();
                if !columns.is_empty() {
                    ui.separator();
                    save_column_buttons(ui, columns, new_columns);
                }
            }
        }
        ReportBody::SeriesExplore { request, analysis, .. } => {
            crate::series_explore_dialog::options_menu(ui, request);
            if let Ok(a) = analysis {
                let columns = a.saveable_columns();
                if !columns.is_empty() {
                    ui.separator();
                    save_column_buttons(ui, columns, new_columns);
                }
            }
        }
        _ => {}
    }
    ui.separator();
    if ui.button("Reset all plot views").clicked() {
        *reset_views = true;
        ui.close_menu();
    }
    if ui.button("Remove report").clicked() {
        *remove = true;
        ui.close_menu();
    }
}

/// A drop-down choosing the factor on one axis of a contour plot (any factor
/// but the one on the other axis).
fn contour_axis_choice(ui: &mut egui::Ui, label: &str, axis: &mut usize, other: usize, names: &[String]) {
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(("contour_axis", label))
            .selected_text(names.get(*axis).map_or("", String::as_str))
            .show_list(ui, |ui| {
                for (j, n) in names.iter().enumerate() {
                    if j != other {
                        ui.selectable_value(axis, j, n.as_str());
                    }
                }
            });
    });
}

/// A report table. Tables wider than the pane scroll horizontally on their own,
/// so they never spill into neighbouring panes.
/// Width of the bar column of an effect Pareto chart.
const PARETO_BAR_WIDTH: f32 = 260.0;

/// Effects as horizontal bars of `−log₁₀ p`, most significant first, with
/// dashed guides at p = 0.05 and p = 0.01 and the p-value and significance
/// stars beside each bar (so significance never rests on the bar shading).
fn effect_pareto(ui: &mut egui::Ui, e: &app_model::EffectPareto, id: impl std::hash::Hash + Copy) {
    let pal = theme::plot_palette(ui);
    let values: Vec<f64> = e.effects.iter().map(app_model::EffectSource::neg_log10_p).collect();
    let top = values.iter().copied().filter(|v| v.is_finite()).fold(2.5_f64, f64::max).ceil();
    let row_h = ui.text_style_height(&egui::TextStyle::Body).max(14.0);
    let guides = [(0.05_f64, "p=.05"), (0.01, "p=.01")];
    let visuals = ui.visuals().clone();
    let guide_color = visuals.weak_text_color();
    let x_of = |rect: egui::Rect, v: f64| rect.left() + (v / top).clamp(0.0, 1.0) as f32 * rect.width();
    let dashed = |painter: &egui::Painter, x: f32, y0: f32, y1: f32| {
        let mut y = y0;
        while y < y1 {
            painter.line_segment(
                [egui::pos2(x, y), egui::pos2(x, (y + 3.0).min(y1))],
                egui::Stroke::new(1.0_f32, guide_color),
            );
            y += 6.0;
        }
    };
    egui::ScrollArea::horizontal().id_salt(("pareto_scroll", id)).show(ui, |ui| {
        egui::Grid::new(id).striped(true).spacing([14.0, 2.0]).show(ui, |ui| {
            ui.strong("Effect");
            // Axis: guide labels above the bars.
            let (rect, _) = ui.allocate_exact_size(egui::vec2(PARETO_BAR_WIDTH, row_h), egui::Sense::hover());
            let font = egui::TextStyle::Small.resolve(ui.style());
            let painter = ui.painter();
            // Guide labels beside their lines (0.05 to the left, 0.01 to the right), and the scale ends.
            for (((p, label), align), dx) in
                guides.iter().zip([egui::Align2::RIGHT_CENTER, egui::Align2::LEFT_CENTER]).zip([-3.0_f32, 3.0])
            {
                let x = x_of(rect, -p.log10());
                painter.text(egui::pos2(x + dx, rect.center().y), align, *label, font.clone(), guide_color);
            }
            painter.text(
                rect.right_center(),
                egui::Align2::RIGHT_CENTER,
                format!("{top:.0}"),
                font.clone(),
                guide_color,
            );
            ui.strong("−log₁₀ p");
            ui.strong("p-value");
            ui.strong("");
            ui.end_row();
            for (effect, &v) in e.effects.iter().zip(&values) {
                ui.label(&effect.name);
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(PARETO_BAR_WIDTH, row_h), egui::Sense::hover());
                let painter = ui.painter();
                if v.is_finite() {
                    let fill = if effect.p_value < 0.05 { pal.point } else { pal.bar_fill };
                    let bar = egui::Rect::from_min_max(
                        egui::pos2(rect.left(), rect.top() + 2.0),
                        egui::pos2(x_of(rect, v).max(rect.left() + 1.0), rect.bottom() - 2.0),
                    );
                    painter.rect_filled(bar, 1.0, fill);
                }
                for (p, _) in guides {
                    dashed(painter, x_of(rect, -p.log10()), rect.top(), rect.bottom());
                }
                response.on_hover_text(format!(
                    "{}: p = {}, −log₁₀ p = {}",
                    effect.name,
                    app_model::format::format_p_value(effect.p_value),
                    app_model::format::format_number(v, 4)
                ));
                ui.monospace(app_model::format::format_number(v, 4));
                ui.monospace(app_model::format::format_p_value(effect.p_value));
                ui.monospace(effect.stars());
                ui.end_row();
            }
        });
    });
    ui.label(
        RichText::new(format!(
            "Tests: {}. Bars past a dashed line are significant at that level; *** p < 0.001, ** p < 0.01, * p < 0.05, \
             . p < 0.1.",
            e.test
        ))
        .weak()
        .small(),
    );
}

/// `true` for a table cell holding a number (possibly signed, in scientific
/// notation, a percentage, a p-value bound such as `< 0.0001`, infinite,
/// flagged with `▲`, or `—`).
fn is_numeric_cell(cell: &str) -> bool {
    app_model::html_export::is_numeric_cell(cell)
}

/// Where the app puts the "mark significant p-values" setting for report tables.
pub fn mark_significant_id() -> egui::Id {
    egui::Id::new("mark_significant")
}

/// How report tables write their numbers (Settings ▸ Report tables). Copy
/// and Save as data table always use the numbers as computed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NumberFormat {
    /// Separate thousands with commas in numbers of five or more digits
    /// before the decimal point (184,150,146).
    pub group_thousands: bool,
    /// Line up the decimal points of each numeric column.
    pub align_decimals: bool,
}

/// Where the app puts the report tables' [`NumberFormat`].
pub fn number_format_id() -> egui::Id {
    egui::Id::new("number_format")
}

/// `cell` with commas between groups of three digits before the decimal
/// point, when it is a plain number (no exponent) with at least five such
/// digits; anything else unchanged.
fn group_thousands(cell: &str) -> String {
    let t = cell.trim();
    let start = t.find(|c: char| c.is_ascii_digit());
    let Some(start) = start else { return cell.to_owned() };
    let digits_end = t[start..].find(|c: char| !c.is_ascii_digit()).map_or(t.len(), |k| start + k);
    let rest = &t[digits_end..];
    let plain = t[..start].chars().all(|c| "+-− <>=±".contains(c))
        && !rest.contains(['e', 'E', ','])
        && rest.strip_prefix('.').unwrap_or(rest).chars().all(|c| c.is_ascii_digit() || " %▲▼".contains(c));
    let int = &t[start..digits_end];
    if !plain || int.len() < 5 {
        return cell.to_owned();
    }
    let mut grouped = String::with_capacity(int.len() + int.len() / 3);
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    format!("{}{grouped}{rest}", &t[..start])
}

/// Characters from a number's decimal point to its end (0 without one),
/// for lining up decimal points.
fn fraction_chars(cell: &str) -> usize {
    cell.find('.').map_or(0, |k| cell[k..].chars().count())
}

/// A report table. Numeric columns are right-aligned in the monospaced face
/// so digits line up (with decimal points lined up too when the setting is
/// on); words use the text face. Returns the table's area, for its
/// right-click menu.
fn text_table(ui: &mut egui::Ui, table: &TextTable, id: impl std::hash::Hash + Copy) -> egui::Response {
    let n = table.columns.len();
    // A column is numeric when every filled cell in it is a number (the first
    // column names rows).
    let numeric: Vec<bool> = (0..n)
        .map(|j| {
            let filled = || table.rows.iter().map(|r| r[j].trim()).filter(|c| !c.is_empty());
            j > 0 && filled().next().is_some() && filled().all(is_numeric_cell)
        })
        .collect();
    // p-value columns, when the setting to mark significant ones is on.
    let mark = ui.ctx().data(|d| d.get_temp::<bool>(mark_significant_id())).unwrap_or(true);
    let p_columns: Vec<bool> = table.columns.iter().map(|h| mark && is_p_value_heading(h)).collect();
    let format = ui.ctx().data(|d| d.get_temp::<NumberFormat>(number_format_id())).unwrap_or_default();
    // The text each numeric cell shows: marked when significant, grouped
    // when the setting is on.
    let shown: Vec<Vec<String>> = table
        .rows
        .iter()
        .map(|row| {
            row.iter()
                .enumerate()
                .map(|(j, cell)| {
                    if !numeric[j] {
                        return cell.clone();
                    }
                    let text = if format.group_thousands { group_thousands(cell) } else { cell.trim().to_owned() };
                    if p_columns[j] && is_significant(cell, SIGNIFICANCE_LEVEL) { format!("★ {text}") } else { text }
                })
                .collect()
        })
        .collect();
    let mono = egui::TextStyle::Monospace.resolve(ui.style());
    let text_font = egui::TextStyle::Body.resolve(ui.style());
    let measure = |text: &str, font: &egui::FontId| {
        ui.fonts(|f| f.layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::WHITE).size().x)
    };
    let digit = measure("0", &mono);
    // Per numeric column, the longest fraction: shorter ones are padded on
    // the right by the difference, so decimal points share one position.
    let fractions: Vec<usize> = (0..n)
        .map(|j| {
            if numeric[j] && format.align_decimals {
                shown.iter().map(|r| fraction_chars(&r[j])).max().unwrap_or(0)
            } else {
                0
            }
        })
        .collect();
    let pad = |j: usize, text: &str| (fractions[j] - fraction_chars(text).min(fractions[j])) as f32 * digit;
    let widths: Vec<f32> = (0..n)
        .map(|j| {
            let font = if numeric[j] { &mono } else { &text_font };
            let cells = shown.iter().map(|r| measure(&r[j], font) + pad(j, &r[j])).fold(0.0_f32, f32::max);
            cells.max(measure(&table.columns[j], &text_font))
        })
        .collect();
    let height = ui.text_style_height(&egui::TextStyle::Body);
    let right_aligned = |ui: &mut egui::Ui, text: &str, width: f32, font: &egui::FontId, color: egui::Color32| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        ui.painter().text(rect.right_center(), egui::Align2::RIGHT_CENTER, text, font.clone(), color);
    };
    let right_padded =
        |ui: &mut egui::Ui, text: &str, width: f32, pad: f32, font: &egui::FontId, color: egui::Color32| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
            let at = rect.right_center() - egui::vec2(pad, 0.0);
            ui.painter().text(at, egui::Align2::RIGHT_CENTER, text, font.clone(), color);
        };
    let area = egui::ScrollArea::horizontal()
        .id_salt(("table_scroll", id))
        // A table wider than the pane shows its scroll bar, so the cut-off
        // columns are not mistaken for the whole table.
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
        .show(ui, |ui| {
            egui::Grid::new(id)
                .striped(true)
                .spacing([18.0, 4.0])
                .show(ui, |ui| {
                    let strong = ui.visuals().strong_text_color();
                    for (j, heading) in table.columns.iter().enumerate() {
                        if numeric[j] {
                            right_aligned(ui, heading, widths[j], &text_font, strong);
                        } else {
                            ui.strong(heading);
                        }
                    }
                    ui.end_row();
                    let color = ui.visuals().text_color();
                    for (row, shown) in table.rows.iter().zip(&shown) {
                        for (j, cell) in row.iter().enumerate() {
                            if numeric[j] {
                                // A significant p-value gets a mark and strong
                                // text, so the cue never rests on colour alone.
                                let significant = p_columns[j] && is_significant(cell, SIGNIFICANCE_LEVEL);
                                // Cells flagged with a sign glyph (e.g. adjusted
                                // residuals) are emphasized too.
                                let flagged = cell.trim_end().ends_with(['▲', '▼']);
                                let text = &shown[j];
                                let ink = if significant || flagged { strong } else { color };
                                right_padded(ui, text, widths[j], pad(j, text), &mono, ink);
                            } else if j > 0 && is_numeric_cell(cell) {
                                ui.monospace(cell);
                            } else {
                                // Text cells keep their whole text on one line; a
                                // table wider than the pane scrolls.
                                ui.add(egui::Label::new(cell).extend());
                            }
                        }
                        ui.end_row();
                    }
                })
                .response
                .rect
        });
    ui.interact(area.inner, ui.make_persistent_id(("table_menu", id)), egui::Sense::click())
}

/// "Save forecasts as data table" for a forecasting report (exponential
/// smoothing or ARIMA): adds a table of time, actual, one-step fitted values,
/// forecasts and limits to the report's project.
fn forecast_save_item(
    ui: &mut egui::Ui,
    report: u64,
    saved: &SavedReport,
    table: &DataTable,
    rows: &RowStateManager,
    new_tables: &mut Vec<(DataTable, bool)>,
) {
    if !matches!(saved, SavedReport::ExponentialSmoothing { .. } | SavedReport::Arima { .. }) {
        return;
    }
    if ui
        .button("Save forecasts as data table")
        .on_hover_text(
            "Add a table to the project with the time, actual and one-step fitted values, then the forecasts \
             with their prediction limits",
        )
        .clicked()
    {
        match app_model::forecasting::saved_forecast_table(saved, table, rows) {
            Some(Ok(t)) => new_tables.push((t, true)),
            Some(Err(e)) => {
                let message = format!("Could not save the forecasts: {e}");
                ui.ctx().data_mut(|d| d.insert_temp(figure_status_id(report), (false, message)));
            }
            None => {}
        }
        ui.close_menu();
    }
}

/// "Save limits as data table" for a control chart: a table of each phase's
/// limits with its process center and σ, which the Control chart dialog can
/// read back as standard values.
fn limits_save_item(
    ui: &mut egui::Ui,
    report: u64,
    saved: &SavedReport,
    table: &DataTable,
    rows: &RowStateManager,
    new_tables: &mut Vec<(DataTable, bool)>,
) {
    if !matches!(saved, SavedReport::ControlChart { .. }) {
        return;
    }
    if ui
        .button("Save limits as data table")
        .on_hover_text(
            "Add a table to the project with each chart's center and limits and the process center and σ; \
             a later chart can use them as standard values",
        )
        .clicked()
    {
        match app_model::control_chart::saved_limits_table(saved, table, rows) {
            Some(Ok(t)) => new_tables.push((t, true)),
            Some(Err(e)) => {
                let message = format!("Could not save the limits: {e}");
                ui.ctx().data_mut(|d| d.insert_temp(figure_status_id(report), (false, message)));
            }
            None => {}
        }
        ui.close_menu();
    }
}

/// Right-click items for a report table: copy it, or open it as a new data
/// table. The outcome shows under report `report`'s options button.
fn table_menu_items(
    ui: &mut egui::Ui,
    table: &TextTable,
    title: &str,
    report: u64,
    new_tables: &mut Vec<(DataTable, bool)>,
    bootstrap: Option<&(SavedReport, String)>,
) {
    if ui
        .button("Copy table")
        .on_hover_text("Tab-separated, with its headings, to paste into a spreadsheet or document")
        .clicked()
    {
        ui.ctx().copy_text(table.to_tsv());
        let message = format!("Copied the {title} table ({} rows)", table.rows.len());
        ui.ctx().data_mut(|d| d.insert_temp(figure_status_id(report), (true, message)));
        ui.close_menu();
    }
    for (label, tip, keep) in [
        (
            "Save as data table",
            "Add this table to the project as a data table, with numeric columns where the cells are numbers",
            true,
        ),
        ("Save as data table in a new tab", "Open this table as a separate dataset, outside this project", false),
    ] {
        if ui.button(label).on_hover_text(tip).clicked() {
            match table.to_data_table(title) {
                Ok(t) => new_tables.push((t, keep)),
                Err(e) => {
                    let message = format!("Could not make a data table: {e}");
                    ui.ctx().data_mut(|d| d.insert_temp(figure_status_id(report), (false, message)));
                }
            }
            ui.close_menu();
        }
    }
    if let Some((saved, report_title)) = bootstrap {
        bootstrap_item(ui, "Bootstrap this table…", saved, report_title, title, table);
    }
}

/// A menu item opening the bootstrap dialog for `table` (section `section`
/// of the report `saved`, titled `report_title`); disabled when the table
/// has no numeric cells.
fn bootstrap_item(
    ui: &mut egui::Ui,
    label: &str,
    saved: &SavedReport,
    report_title: &str,
    section: &str,
    table: &TextTable,
) {
    let numeric = !app_model::resampling::numeric_cells(table).is_empty();
    if ui
        .add_enabled(numeric, egui::Button::new(label))
        .on_hover_text(
            "Re-run this report on resamples of its rows and give bootstrap standard errors, bias, and \
             percentile and BCa intervals for chosen cells of this table",
        )
        .on_disabled_hover_text("This table has no numeric cells")
        .clicked()
    {
        crate::resampling_dialogs::request_bootstrap(
            ui.ctx(),
            crate::resampling_dialogs::BootstrapStart {
                saved: saved.clone(),
                report_title: report_title.to_owned(),
                section: section.to_owned(),
                table: table.clone(),
            },
        );
        ui.close_menu();
    }
}

/// The plots among `sections` that keep a style: the key of their plot
/// state, their style key and the section.
fn plot_sections(sections: &[ReportSection]) -> Vec<((SectionKind, usize), String, &ReportSection)> {
    let mut seen: HashMap<SectionKind, usize> = HashMap::new();
    let mut out = Vec::new();
    for section in sections {
        let n = seen.entry(section.kind).or_default();
        let nth = *n;
        *n += 1;
        if app_model::report_figure::is_plot(&section.content) {
            out.push(((section.kind, nth), app_model::plot_style_key(section.kind, nth), section));
        }
    }
    out
}

/// Per-plot interaction state kept across frames.
#[derive(Default)]
pub struct PlotState {
    /// Plot-coordinate start of an in-progress box selection.
    brush_start: Option<[f64; 2]>,
    /// Limits set by axis dragging; `None` means fit to the data.
    bounds: Option<ViewBounds>,
    /// Axis drag in progress.
    axis_drag: Option<AxisDrag>,
    /// Re-fit the view on the next frame (requested by a double-click).
    reset_pending: bool,
    /// Size chosen with the resize grip.
    pub(crate) size: PlotSize,
    /// Axis settings (ticks, grid, format, titles, …).
    axes: PlotAxes,
    /// The plot's own default style, once its saved style (if any) has been
    /// put into `axes`; a style equal to it is not kept in the report.
    default_style: Option<app_model::plot_style::PlotStyle>,
    /// The view drawn last frame, for the range fields of the axis menu.
    last_view: Option<ViewBounds>,
    /// The plot area drawn this frame, for its right-click menu.
    plot_response: Option<egui::Response>,
    /// The plot frame (inside the axis strips) drawn last, whose proportions
    /// an exported figure keeps.
    last_frame: Option<egui::Rect>,
    /// Dragging draws a freehand outline instead of a box (scatter plots,
    /// chosen from the plot's right-click menu).
    lasso: bool,
    /// The outline being drawn (plot coordinates).
    lasso_path: Option<Vec<[f64; 2]>>,
    /// The contour map's drawing, kept while the surface and style stay put.
    contours: crate::contour_view::ContourCache,
    /// A time–frequency map's cells as one mesh, kept while the map and
    /// appearance stay the same (keyed by their hash).
    intensity: Option<(u64, Arc<stat_viz::render::FillMesh>)>,
    /// An image plot's picture and its selected cells as textures, kept
    /// while the grid, appearance and selection stay the same (keyed by
    /// their hash).
    raster: Option<(u64, egui::TextureHandle)>,
    raster_selection: Option<(u64, Option<egui::TextureHandle>)>,
    /// A tick label being written in place (double-click on a tick label,
    /// or "Edit tick label…" on an axis's menu).
    tick_edit: Option<plot_axes::TickEdit>,
    /// Where an axis was last right-clicked (axis, screen position), for
    /// "Edit tick label…".
    axis_menu_at: Option<(bool, egui::Pos2)>,
    /// The user's reference lines and bands (saved with the plot's style).
    pub(crate) reference: stat_viz::plot_style::ReferenceLook,
    /// This frame's marks of [`PlotState::reference`], computed from the
    /// section before the plot is drawn.
    reference_marks: stat_viz::element::reference::ReferenceLayers,
    /// The Reference lines window is open.
    pub(crate) reference_open: bool,
    /// The plot can carry reference lines (an x–y chart).
    pub(crate) reference_supported: bool,
    /// The plot has panels and subgroups (a graph canvas), so its
    /// statistics can be of the whole graph, each panel or each subgroup.
    pub(crate) reference_scopes: bool,
    /// The axis breaks the view was fitted for (`[x, y]`); new breaks
    /// change the drawn units, so the view is fitted again.
    drawn_breaks: [stat_viz::AxisBreaks; 2],
}

impl PlotState {
    /// The page for an exported figure matching this plot on screen: the plot
    /// frame drawn last (not the whole widget, whose axis strips would skew
    /// the proportions), or `widget` before a frame was drawn.
    fn screen_layout(&self, widget: egui::Rect) -> FigureLayout {
        let frame = self.last_frame.unwrap_or(widget);
        let (w, h) = (f64::from(frame.width()), f64::from(frame.height()));
        // A print scale (Plot settings) sets the page from the range shown.
        let page = self
            .axes
            .print_scale
            .zip(self.last_view)
            .and_then(|(mm, view)| FigureLayout::at_print_scale(w, h, view, mm))
            .unwrap_or_else(|| FigureLayout::matching_screen(w, h, self.last_view));
        FigureLayout { category_labels: self.axes.category_labels, ..page }.with_axes(self.axes.clone())
    }

    /// On the first frame: note the plot's default style and put the style
    /// saved under `key` in `scope` (if any) into the plot's settings.
    fn load_style(&mut self, scope: &app_model::ReportScope, key: &str) {
        if self.default_style.is_none() {
            self.default_style = Some(app_model::plot_style::PlotStyle::of(&self.axes));
            if let Some(saved) = scope.plot_style(key) {
                saved.apply_to(&mut self.axes);
                self.reference.clone_from(&saved.reference);
            }
        }
    }

    /// Give the plot figure look `look`, kept in its style in `scope` under
    /// `key` (see [`PlotState::keep_style`]).
    fn set_look(&self, scope: &mut app_model::ReportScope, key: &str, look: app_model::plot_style::ExportLook) {
        let Some(default) = &self.default_style else { return };
        let mut style = scope.plot_style(key).cloned().unwrap_or_default();
        style.axes = app_model::plot_style::user_axes(&self.axes);
        style.export = look;
        scope.set_plot_style(key, (style != *default).then_some(style));
    }

    /// Keep the plot's style in `scope` under `key` when it differs from the
    /// plot's default, else forget it, so it is saved with the report.
    fn keep_style(&self, scope: &mut app_model::ReportScope, key: &str) {
        let Some(default) = &self.default_style else { return };
        let mut style = scope.plot_style(key).cloned().unwrap_or_default();
        style.axes = app_model::plot_style::user_axes(&self.axes);
        style.reference.clone_from(&self.reference);
        scope.set_plot_style(key, (style != *default).then_some(style));
    }

    /// Work out this frame's reference marks for a plot showing `content`.
    fn prepare_reference(&mut self, content: &SectionContent, rows: &RowStateManager) {
        self.reference_supported = matches!(app_model::report_figure::chart_kind(content), Some(true));
        self.reference_marks = if self.reference.is_empty() || !self.reference_supported {
            stat_viz::element::reference::ReferenceLayers::default()
        } else {
            app_model::report_figure::reference_layers(content, Some(rows), &self.reference)
        };
    }

    /// A fresh state for the plot of `section`.
    fn for_section(section: &ReportSection) -> Self {
        match &section.content {
            SectionContent::Plot(p) => Self::for_plot(p, &section.title),
            // Square pixels stay square.
            SectionContent::Raster(r) => {
                let mut state = Self::default();
                state.axes.equal_scale = r.equal_aspect;
                state
            }
            _ => Self::default(),
        }
    }

    /// A fresh state for x–y plot `spec` titled `title`: plots whose geometry
    /// needs equal units on both axes start at equal scale (observed against
    /// predicted or fitted and probability plots, which draw the line y = x,
    /// and score plots); Plot settings can turn it off.
    fn for_plot(spec: &PlotSpec, title: &str) -> Self {
        let mut state = Self::default();
        let identity = spec.line_name.as_deref().is_some_and(|n| n.contains("y = x") || n == "Observed = predicted");
        state.axes.equal_scale = identity || title.starts_with("Score Plot");
        state
    }
}

/// The plot's title (strong) and subtitle (faint) centred above it, wrapped
/// to the plot's width; nothing when the title is turned off.
fn plot_title(ui: &mut egui::Ui, axes: &PlotAxes, heading: &str, width: f32) {
    let Some((main, sub)) = axes.plot_title(heading) else {
        return;
    };
    // A typed title is read as math markup; a plain one is drawn as always.
    let markup = axes.title.as_deref().map(stat_viz::MathText::parse).filter(|m| m.as_plain().is_none());
    ui.allocate_ui_with_layout(egui::vec2(width, 0.0), egui::Layout::top_down(egui::Align::Center), |ui| {
        ui.set_width(width);
        let size = 14.0 * theme::text_scale(ui);
        if let Some(m) = markup {
            let font = egui::FontId::proportional(size);
            let color = ui.visuals().strong_text_color();
            let galley = stat_viz::math_text::egui_text::galley(ui.ctx(), &m, &font, color);
            let name = m.plain_text();
            ui.add(egui::Label::new(egui::WidgetText::Galley(galley)))
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &name));
            return;
        }
        ui.add(egui::Label::new(RichText::new(main).strong().size(size)).wrap());
        if let Some(sub) = sub {
            ui.add(egui::Label::new(RichText::new(sub).weak().small()).wrap());
        }
    });
}

/// How an item in a plot's key is drawn.
enum Glyph {
    /// A line in the given pattern.
    Line(LinePattern),
    /// A marker, filled or hollow.
    Marker(MarkerShape, bool),
    /// A filled area with an outline.
    Area(egui::Color32),
}

/// One entry of a plot's key.
struct KeyItem {
    label: String,
    color: egui::Color32,
    glyph: Glyph,
}

/// The key (legend) of a plot, drawn below it so it never covers data.
#[derive(Default)]
struct PlotKey(Vec<KeyItem>, bool);

impl PlotKey {
    /// Read labels as math markup (`stat_viz::math_text`).
    fn read_markup(&mut self, on: bool) {
        self.1 = on;
    }

    /// One label: plain small text, or a markup galley named by its plain text.
    fn label(&self, ui: &mut egui::Ui, label: &str) {
        if self.1 {
            let m = stat_viz::MathText::parse(label);
            if m.as_plain().is_none() {
                let font = egui::TextStyle::Small.resolve(ui.style());
                let galley = stat_viz::math_text::egui_text::galley(ui.ctx(), &m, &font, ui.visuals().text_color());
                let name = m.plain_text();
                ui.add(egui::Label::new(egui::WidgetText::Galley(galley)))
                    .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &name));
                return;
            }
        }
        ui.add(egui::Label::new(RichText::new(label).small()).wrap());
    }

    fn line(&mut self, label: impl Into<String>, color: egui::Color32, pattern: LinePattern) {
        self.0.push(KeyItem { label: label.into(), color, glyph: Glyph::Line(pattern) });
    }

    fn marker(&mut self, label: impl Into<String>, color: egui::Color32, shape: MarkerShape, filled: bool) {
        self.0.push(KeyItem { label: label.into(), color, glyph: Glyph::Marker(shape, filled) });
    }

    fn area(&mut self, label: impl Into<String>, fill: egui::Color32, stroke: egui::Color32) {
        self.0.push(KeyItem { label: label.into(), color: stroke, glyph: Glyph::Area(fill) });
    }

    /// One entry per line (a key beside a plot).
    fn show_column(&self, ui: &mut egui::Ui) {
        for item in &self.0 {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(22.0, 12.0), egui::Sense::hover());
                paint_glyph(ui.painter(), rect, item);
                self.label(ui, &item.label);
            });
        }
    }

    fn show(&self, ui: &mut egui::Ui) {
        if self.0.is_empty() {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for item in &self.0 {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(22.0, 12.0), egui::Sense::hover());
                paint_glyph(ui.painter(), rect, item);
                if self.1 {
                    self.label(ui, &item.label);
                } else {
                    ui.label(RichText::new(&item.label).small());
                }
                ui.add_space(8.0);
            }
        });
    }
}

fn paint_glyph(painter: &egui::Painter, rect: egui::Rect, item: &KeyItem) {
    let c = rect.center();
    let stroke = egui::Stroke::new(1.5_f32, item.color);
    match item.glyph {
        Glyph::Line(pattern) => {
            let pts = [egui::pos2(rect.left(), c.y), egui::pos2(rect.right(), c.y)];
            match pattern {
                LinePattern::Solid => {
                    painter.line_segment(pts, egui::Stroke::new(2.0_f32, item.color));
                }
                LinePattern::Dashed => painter.extend(egui::Shape::dashed_line(&pts, stroke, 5.0, 3.0)),
                LinePattern::Dotted => painter.extend(egui::Shape::dotted_line(&pts, item.color, 4.0, 1.0)),
                LinePattern::DashDot => {
                    painter.extend(stat_viz::render::pattern_shapes(&pts, stroke, stat_viz::LinePattern::DashDot));
                }
            }
        }
        Glyph::Area(fill) => {
            painter.rect(rect.shrink2(egui::vec2(2.0, 1.0)), 2.0_f32, fill, stroke, egui::StrokeKind::Inside);
        }
        Glyph::Marker(shape, filled) => paint_marker(painter, c, 4.0, shape, item.color, filled),
    }
}

pub(crate) fn paint_marker(
    painter: &egui::Painter,
    c: egui::Pos2,
    r: f32,
    shape: MarkerShape,
    color: egui::Color32,
    filled: bool,
) {
    let stroke = egui::Stroke::new(1.5_f32, color);
    let poly = |pts: Vec<egui::Pos2>| {
        if filled {
            egui::Shape::convex_polygon(pts, color, egui::Stroke::NONE)
        } else {
            egui::Shape::closed_line(pts, stroke)
        }
    };
    let v = egui::vec2;
    let shape = match shape {
        MarkerShape::Circle => {
            if filled {
                egui::Shape::circle_filled(c, r, color)
            } else {
                egui::Shape::circle_stroke(c, r, stroke)
            }
        }
        MarkerShape::Square => poly(vec![c + v(-r, -r), c + v(r, -r), c + v(r, r), c + v(-r, r)]),
        MarkerShape::Diamond => poly(vec![c + v(0.0, -r), c + v(r, 0.0), c + v(0.0, r), c + v(-r, 0.0)]),
        MarkerShape::Up => poly(vec![c + v(0.0, -r), c + v(r, r), c + v(-r, r)]),
        MarkerShape::Down => poly(vec![c + v(-r, -r), c + v(r, -r), c + v(0.0, r)]),
        MarkerShape::Left => poly(vec![c + v(-r, 0.0), c + v(r, -r), c + v(r, r)]),
        MarkerShape::Right => poly(vec![c + v(r, 0.0), c + v(-r, r), c + v(-r, -r)]),
        MarkerShape::Cross => egui::Shape::Vec(vec![
            egui::Shape::line_segment([c + v(-r, -r), c + v(r, r)], stroke),
            egui::Shape::line_segment([c + v(-r, r), c + v(r, -r)], stroke),
        ]),
        MarkerShape::Plus => egui::Shape::Vec(vec![
            egui::Shape::line_segment([c + v(-r, 0.0), c + v(r, 0.0)], stroke),
            egui::Shape::line_segment([c + v(0.0, -r), c + v(0.0, r)], stroke),
        ]),
        MarkerShape::Asterisk => egui::Shape::Vec(vec![
            egui::Shape::line_segment([c + v(-r, 0.0), c + v(r, 0.0)], stroke),
            egui::Shape::line_segment([c + v(-r * 0.5, -r), c + v(r * 0.5, r)], stroke),
            egui::Shape::line_segment([c + v(-r * 0.5, r), c + v(r * 0.5, -r)], stroke),
        ]),
    };
    painter.add(shape);
}

/// Screen height of the draggable strip under a plot (the x axis).
const X_AXIS_STRIP: f32 = 28.0;
/// Screen width of the draggable strip left of a plot (the y axis).
const Y_AXIS_STRIP: f32 = 44.0;

/// Resolve a pending or requested view reset. Returns whether the plot should
/// re-fit to its data this frame, and the user-set limits (if any) to apply.
fn prepare_view(state: &mut PlotState, reset: bool) -> (bool, Option<ViewBounds>) {
    let reset = reset || std::mem::take(&mut state.reset_pending);
    if reset {
        state.bounds = None;
        state.axis_drag = None;
        // Ranges typed in Plot settings go back to automatic too.
        state.axes.x.range = None;
        state.axes.y.range = None;
    }
    (reset, state.bounds)
}

/// A plot configured for this application's interaction model: no scroll or
/// pinch navigation and no built-in drag. Dragging in the plot selects points;
/// the axes are dragged to rescale or shift (see [`axis_controls`]).
macro_rules! interactive_plot {
    ($id:expr, $reset:expr) => {{
        // No built-in legend: it floats over the data. Each plot draws a key
        // below itself instead (`PlotKey`).
        let plot = Plot::new($id)
            .allow_drag(false)
            .allow_boxed_zoom(false)
            .allow_scroll(false)
            .allow_zoom(false)
            .allow_double_click_reset(false);
        if $reset { plot.reset() } else { plot }
    }};
}

/// Replace a plot's numeric x axis with labelled ticks when `$ticks` (an
/// `&Option<Vec<(f64, String)>>`) is `Some`. Such plots draw no vertical grid
/// lines (one per tick would clutter them; the labels mark positions).
/// Show every how-many-th category label so labels `widest` points wide do
/// not overlap. `base_step_size` is egui_plot's smallest grid step, in data
/// units per 8 points.
fn category_label_step(positions: &[f64], widest: f64, base_step_size: f64) -> usize {
    let gap = positions.windows(2).map(|w| (w[1] - w[0]).abs()).filter(|g| *g > 0.0).fold(f64::INFINITY, f64::min);
    if !gap.is_finite() || base_step_size <= 0.0 {
        return 1;
    }
    let points_per_unit = 8.0 / base_step_size;
    let needed = widest + 10.0;
    ((needed / (gap * points_per_unit)).ceil() as usize).max(1)
}

macro_rules! with_x_ticks {
    ($ui:expr, $plot:expr, $ticks:expr) => {
        with_x_ticks!($ui, $plot, $ticks, None)
    };
    ($ui:expr, $plot:expr, $ticks:expr, $category:expr) => {{
        let mut plot = $plot;
        let category: Option<&crate::plot_axes::CategoryAxis> = $category;
        // Turned labels are painted after the plot, in a deeper axis strip.
        let turned = category.is_some_and(|c| c.turned());
        if let Some(c) = category.filter(|c| c.turned()) {
            plot = plot.custom_x_axes(vec![egui_plot::AxisHints::new_x().min_thickness(c.axis_thickness())]);
        }
        match $ticks {
            Some(ticks) => {
                let positions: Vec<f64> = ticks.iter().map(|t| t.0).collect();
                let labels = ticks.clone();
                // Measured in the tick-label font (egui_plot's body text).
                let font = egui::TextStyle::Body.resolve($ui.style());
                let widest = $ui.fonts(|f| {
                    ticks
                        .iter()
                        .map(|t| f.layout_no_wrap(t.1.clone(), font.clone(), egui::Color32::WHITE).size().x)
                        .fold(0.0_f32, f32::max)
                });
                let widest = f64::from(widest);
                plot.x_grid_spacer(move |input| {
                    // A large step size marks every tick as major, so every
                    // label shows; when they would overlap, every k-th does.
                    let step = if turned { 1 } else { category_label_step(&positions, widest, input.base_step_size) };
                    positions.iter().step_by(step).map(|&value| GridMark { value, step_size: 1e6 }).collect()
                })
                .x_axis_formatter(move |mark, _range| {
                    if turned {
                        return String::new();
                    }
                    labels.iter().find(|t| (t.0 - mark.value).abs() < 1e-9).map(|t| t.1.clone()).unwrap_or_default()
                })
            }
            None => plot,
        }
    }};
}

/// Replace the vertical axis's ticks with labelled positions (e.g. the
/// percentages of a probability scale).
macro_rules! with_y_ticks {
    ($plot:expr, $ticks:expr) => {{
        let plot = $plot;
        match $ticks {
            Some(ticks) => {
                let positions: Vec<f64> = ticks.iter().map(|t| t.0).collect();
                let labels = ticks.clone();
                plot.y_grid_spacer(move |_input| {
                    positions.iter().map(|&value| GridMark { value, step_size: 1e6 }).collect()
                })
                .y_axis_formatter(move |mark, _range| {
                    labels.iter().find(|t| (t.0 - mark.value).abs() < 1e-9).map(|t| t.1.clone()).unwrap_or_default()
                })
            }
            None => plot,
        }
    }};
}

// After the plot macros, which it uses.
#[path = "graph_view.rs"]
mod graph_view;
pub use graph_view::ColumnDrag;
pub(crate) use graph_view::condition_editor;
pub use graph_view::output::open_pipeline;

/// A soft halo under points in the dark appearance (nothing in the light one):
/// a wider, translucent disc of the point color.
/// Points grouped by their rows' own colour and marker.
type StyledPoints = Vec<((Option<stat_data::RowColor>, Option<stat_data::RowMarker>), Vec<[f64; 2]>)>;

/// The points of a plot above the point budget as one picture: excluded rows
/// at the bottom, then each group (`by_group.1`: coloured by group, else in
/// the point colour) and row style with opacity following density, then the
/// selected rows on top in the selection colour, a little larger so they
/// stand out of any group.
fn point_picture(
    pal: &theme::PlotPalette,
    revision: u64,
    by_group: (Vec<Vec<[f64; 2]>>, bool),
    by_style: StyledPoints,
    excluded: Vec<[f64; 2]>,
    selected: Vec<[f64; 2]>,
) -> stat_viz::render::PointRaster {
    use stat_viz::render::{RasterPoints, raster_splat};
    let splat = raster_splat(4.0);
    let mut layers = vec![RasterPoints { points: excluded, color: pal.excluded, density: false, splat }];
    let (groups, grouped) = by_group;
    for (g, points) in groups.into_iter().enumerate() {
        let color = if grouped { pal.group_style(g).0 } else { pal.point };
        layers.push(RasterPoints { points, color, density: true, splat });
    }
    for ((color, _), points) in by_style {
        let color = color.map_or(pal.point, |c| theme::row_color(c, pal.dark));
        layers.push(RasterPoints { points, color, density: true, splat });
    }
    layers.push(RasterPoints { points: selected, color: pal.selected, density: false, splat: splat + 1.5 });
    layers.retain(|l| !l.points.is_empty());
    stat_viz::render::PointRaster::new(layers, revision ^ u64::from(pal.dark) << 63).name("Points")
}

/// The note under a plot whose `n` points are drawn as a picture.
fn dense_points_note(n: usize) -> String {
    format!(
        "{} points, drawn as a density picture (more than {}): darker where points overlap",
        group_thousands(&n.to_string()),
        group_thousands(&stat_viz::point_raster::POINT_BUDGET.to_string())
    )
}

fn glow(
    plot: &mut egui_plot::PlotUi,
    points: &[[f64; 2]],
    radius: f32,
    color: egui::Color32,
    pal: &theme::PlotPalette,
) {
    if !pal.dark || points.is_empty() {
        return;
    }
    let halo = color.gamma_multiply(f32::from(pal.point_glow.a()) / 255.0);
    plot.points(Points::new(PlotPoints::from(points.to_vec())).radius(radius + 2.5).color(halo));
}

/// The pairwise comparison method (a submenu, so choosing keeps the options
/// menu open).
fn pairwise_menu(ui: &mut egui::Ui, pairwise: &mut Option<app_model::ComparisonMethod>) {
    use app_model::ComparisonMethod;
    let current = pairwise.map_or("None", ComparisonMethod::label);
    ui.menu_button(format!("Pairwise comparisons: {current}"), |ui| {
        for method in ComparisonMethod::MEANS {
            if ui.radio_value(pairwise, Some(method), method.label()).on_hover_text(pairwise_hint(method)).clicked() {
                ui.close_menu();
            }
        }
        if ui.radio_value(pairwise, None, "None").on_hover_text("No pairwise comparisons").clicked() {
            ui.close_menu();
        }
    });
}

/// What a pairwise comparison method is for, as a tooltip.
pub fn pairwise_hint(method: app_model::ComparisonMethod) -> &'static str {
    use app_model::ComparisonMethod as M;
    match method {
        M::TukeyKramer => "Pooled error variance; simultaneous intervals for all pairs. The usual choice",
        M::GamesHowell => "No equal-variance assumption: Welch standard errors per pair, studentized range",
        M::HolmT => "Pooled t tests, Holm step-down p-values; less conservative for few pairs",
        M::Bonferroni => "Pooled t tests, p-values multiplied by the number of pairs",
        M::Sidak => "Pooled t tests, Šidák adjustment 1 − (1 − p)^m; slightly less conservative than Bonferroni",
        M::HolmSidak => "Pooled t tests, step-down Šidák p-values; more powerful than Holm",
        M::FisherLsd => "Unadjusted pooled t tests; use only after a significant overall F test",
        M::NewmanKeuls => "Stepwise studentized range by how far apart the means rank; no intervals",
        M::TamhaneT2 => "Unequal variances: Welch t per pair with Šidák's adjustment",
        M::DunnettT3 => "Unequal variances: Welch t per pair against the studentized maximum modulus",
        M::Dunn => "Rank-based comparisons with Holm-adjusted p-values",
    }
}

/// Options of one-way models: tests of equal variances and the pairwise
/// comparison method (a submenu, so choosing keeps the options menu open).
fn one_way_options(ui: &mut egui::Ui, o: &mut app_model::ReportOptions, levels: &[String]) {
    ui.checkbox(&mut o.show_equal_variances, "Tests of equal variances (Levene, Brown–Forsythe, Bartlett)");
    pairwise_menu(ui, &mut o.pairwise);
    ui.add_enabled(levels.len() >= 3, egui::Checkbox::new(&mut o.linear_trend, "Linear trend across ordered levels"))
        .on_hover_text("Tests a straight-line trend in the means across the levels in their order (three or more)");
    let current = o.control_level.and_then(|c| levels.get(c)).map_or("off", String::as_str);
    ui.menu_button(format!("Comparisons with a control (Dunnett): {current}"), |ui| {
        ui.label(RichText::new("Compare every other level with:").weak());
        for (i, level) in levels.iter().enumerate() {
            if ui.radio_value(&mut o.control_level, Some(i), level).clicked() {
                ui.close_menu();
            }
        }
        if ui.radio_value(&mut o.control_level, None, "Off").clicked() {
            ui.close_menu();
        }
    })
    .response
    .on_hover_text("Each level against one control level, with simultaneous intervals (Dunnett 1955)");
    let current = match o.best {
        None => "off",
        Some(app_model::BestDirection::Largest) => "larger is better",
        Some(app_model::BestDirection::Smallest) => "smaller is better",
    };
    ui.menu_button(format!("Comparisons with the best (Hsu): {current}"), |ui| {
        for (choice, label) in [
            (Some(app_model::BestDirection::Largest), "Larger is better"),
            (Some(app_model::BestDirection::Smallest), "Smaller is better"),
            (None, "Off"),
        ] {
            if ui.radio_value(&mut o.best, choice, label).clicked() {
                ui.close_menu();
            }
        }
    })
    .response
    .on_hover_text(
        "Each level against the best of the others, with constrained simultaneous intervals that rule levels in or \
         out as the best (Hsu 1981)",
    );
}

/// Tolerance interval, outlier test and runs tests of a continuous
/// distribution report, each switched on with its settings beneath it.
fn distribution_test_options(ui: &mut egui::Ui, o: &mut DistributionOptions) {
    use app_model::{
        Alternative, DixonRatio, OutlierMethod, OutlierSettings, RunsSettings, ToleranceSettings, ToleranceSides,
    };
    let mut on = o.tolerance.is_some();
    if ui
        .checkbox(&mut on, "Tolerance interval")
        .on_hover_text("Limits that contain a stated proportion of the population with stated confidence")
        .changed()
    {
        o.tolerance = on.then(ToleranceSettings::default);
    }
    if let Some(t) = &mut o.tolerance {
        ui.indent("tolerance_settings", |ui| {
            ui.horizontal(|ui| {
                ui.label("Proportion contained:");
                ui.add(egui::DragValue::new(&mut t.proportion).range(0.5..=0.9999).speed(0.001).fixed_decimals(3));
                ui.label("Confidence:");
                ui.add(egui::DragValue::new(&mut t.confidence).range(0.5..=0.9999).speed(0.001).fixed_decimals(3));
            });
            ui.horizontal(|ui| {
                ui.label("Limits:");
                for side in [ToleranceSides::TwoSided, ToleranceSides::Lower, ToleranceSides::Upper] {
                    let label = match side {
                        ToleranceSides::TwoSided => "Two-sided",
                        ToleranceSides::Lower => "Lower only",
                        ToleranceSides::Upper => "Upper only",
                    };
                    ui.radio_value(&mut t.sides, side, label);
                }
            });
            ui.horizontal(|ui| {
                ui.label("Method:");
                ui.radio_value(&mut t.distribution_free, false, "Normal")
                    .on_hover_text("Assumes the values come from a normal distribution");
                ui.radio_value(&mut t.distribution_free, true, "Distribution-free")
                    .on_hover_text("Order statistics (Wilks); needs larger samples");
            });
        });
    }
    let mut on = o.outliers.is_some();
    if ui
        .checkbox(&mut on, "Outlier tests")
        .on_hover_text("Test whether the most extreme values are unusually far from the rest (assumes normal data)")
        .changed()
    {
        o.outliers = on.then(OutlierSettings::default);
    }
    if let Some(t) = &mut o.outliers {
        ui.indent("outlier_settings", |ui| {
            ui.horizontal(|ui| {
                ui.label("Test:");
                for m in OutlierMethod::ALL {
                    ui.radio_value(&mut t.method, m, m.label());
                }
            });
            ui.add_enabled_ui(t.method != OutlierMethod::GeneralizedEsd, |ui| {
                ui.horizontal(|ui| {
                    ui.label("End tested:");
                    ui.radio_value(&mut t.side, Alternative::TwoSided, "Either");
                    ui.radio_value(&mut t.side, Alternative::Greater, "Largest");
                    ui.radio_value(&mut t.side, Alternative::Less, "Smallest");
                });
            });
            ui.horizontal(|ui| {
                ui.label("Significance level:");
                ui.add(egui::DragValue::new(&mut t.alpha).range(0.001..=0.5).speed(0.001).fixed_decimals(3));
            });
            if t.method == OutlierMethod::Dixon {
                ui.horizontal(|ui| {
                    ui.label("Ratio:");
                    ui.radio_value(&mut t.dixon_ratio, None, "By sample size");
                    for r in DixonRatio::ALL {
                        ui.radio_value(&mut t.dixon_ratio, Some(r), r.label());
                    }
                });
            }
            if t.method == OutlierMethod::GeneralizedEsd {
                ui.horizontal(|ui| {
                    let mut set = t.max_outliers.is_some();
                    if ui
                        .checkbox(&mut set, "Most outliers:")
                        .on_hover_text("Default: a tenth of the values, at least 1")
                        .changed()
                    {
                        t.max_outliers = set.then_some(1);
                    }
                    if let Some(m) = &mut t.max_outliers {
                        ui.add(egui::DragValue::new(m).range(1..=100));
                    }
                });
            }
        });
    }
    let mut on = o.runs.is_some();
    if ui
        .checkbox(&mut on, "Runs tests for randomness")
        .on_hover_text("Runs above and below the median, and runs up and down, in row order")
        .changed()
    {
        o.runs = on.then(RunsSettings::default);
    }
    if let Some(r) = &mut o.runs {
        ui.indent("runs_settings", |ui| {
            ui.horizontal(|ui| {
                let mut fixed = r.reference.is_some();
                if ui.checkbox(&mut fixed, "Runs about a value instead of the median:").changed() {
                    r.reference = fixed.then_some(0.0);
                }
                if let Some(v) = &mut r.reference {
                    ui.add(egui::DragValue::new(v).speed(0.01));
                }
            });
            ui.horizontal(|ui| {
                ui.label("Alternative:");
                ui.radio_value(&mut r.alternative, Alternative::TwoSided, "Too few or too many runs");
                ui.radio_value(&mut r.alternative, Alternative::Less, "Too few").on_hover_text("Clustering or trend");
                ui.radio_value(&mut r.alternative, Alternative::Greater, "Too many").on_hover_text("Alternation");
            });
        });
    }
}

/// The plot colors for the appearance a plot is drawn in.
fn plot_palette_of(plot: &egui_plot::PlotUi) -> theme::PlotPalette {
    if plot.ctx().style().visuals.dark_mode { theme::PlotPalette::dark() } else { theme::PlotPalette::light() }
}

/// Apply user-set limits inside a plot's drawing closure.
fn apply_bounds(plot: &mut egui_plot::PlotUi, bounds: Option<ViewBounds>) {
    if let Some(b) = bounds {
        plot.set_plot_bounds(PlotBounds::from_min_max([b.x_min, b.y_min], [b.x_max, b.y_max]));
    }
}

/// Make both axes draggable. Near an axis's ends a drag moves that limit only
/// (the grabbed value follows the pointer); elsewhere it shifts the whole axis.
/// A double-click inside the plot re-fits it to the data.
fn axis_controls(ui: &egui::Ui, response: &PlotResponse<()>, state: &mut PlotState, id: egui::Id, info: &AxisInfo<'_>) {
    state.plot_response = Some(response.response.clone());
    if response.response.double_clicked() {
        state.reset_pending = true;
        ui.ctx().request_repaint();
    }
    let frame = *response.transform.frame();
    state.last_frame = Some(frame);
    let drawn = {
        let b = response.transform.bounds();
        ViewBounds { x_min: b.min()[0], x_max: b.max()[0], y_min: b.min()[1], y_max: b.max()[1] }
    };
    state.last_view = Some(drawn);
    // Drags start from the view actually drawn, which can differ from the
    // limits asked for (equal scale widens one axis to keep the aspect), so
    // the grabbed value stays under the pointer.
    let current = drawn;
    // The strips sit beside whichever side each axis is drawn on, starting
    // at the axis line (moved out by its offset).
    let (ox, oy) = (plot_axes::axis_offset(&state.axes.x), plot_axes::axis_offset(&state.axes.y));
    let x_strip = match state.axes.x.side {
        AxisSide::Standard => egui::Rect::from_min_max(
            egui::pos2(frame.left(), frame.bottom() + ox),
            egui::pos2(frame.right(), frame.bottom() + ox + X_AXIS_STRIP),
        ),
        AxisSide::Opposite => egui::Rect::from_min_max(
            egui::pos2(frame.left(), frame.top() - ox - X_AXIS_STRIP),
            egui::pos2(frame.right(), frame.top() - ox),
        ),
    };
    let y_strip = match state.axes.y.side {
        AxisSide::Standard => egui::Rect::from_min_max(
            egui::pos2(frame.left() - oy - Y_AXIS_STRIP, frame.top()),
            egui::pos2(frame.left() - oy, frame.bottom()),
        ),
        AxisSide::Opposite => egui::Rect::from_min_max(
            egui::pos2(frame.right() + oy, frame.top()),
            egui::pos2(frame.right() + oy + Y_AXIS_STRIP, frame.bottom()),
        ),
    };
    let strips = [(Axis::X, x_strip), (Axis::Y, y_strip)];
    for (axis, strip) in strips {
        let (length, along): (f64, Box<dyn Fn(egui::Pos2) -> f64>) = match axis {
            Axis::X => (f64::from(frame.width()), Box::new(move |p: egui::Pos2| f64::from(p.x - frame.left()))),
            Axis::Y => (f64::from(frame.height()), Box::new(move |p: egui::Pos2| f64::from(frame.bottom() - p.y))),
        };
        crate::axis_drop::axis_drop_zone(ui, strip, if axis == Axis::X { info.x_label } else { info.y_label });
        let resp = ui.interact(strip, id.with(("axis", axis == Axis::X)), egui::Sense::click_and_drag());
        // Named for screen readers (and found by name in the GUI tests).
        let name = if axis == Axis::X { "Horizontal axis ruler" } else { "Vertical axis ruler" };
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, name));
        if axis == Axis::X || info.has_y {
            let vertical = axis == Axis::Y;
            if resp.secondary_clicked() {
                state.axis_menu_at = resp.interact_pointer_pos().map(|p| (vertical, p));
            }
            let categorical = !vertical && info.categorical_x;
            if resp.double_clicked() && !categorical {
                if let Some(p) = resp.interact_pointer_pos() {
                    state.tick_edit = Some(plot_axes::TickEdit::at(&response.transform, &state.axes, vertical, p));
                }
            }
            let mut edit_label = false;
            resp.context_menu(|ui| {
                if !categorical
                    && ui
                        .button("Edit tick label…")
                        .on_hover_text("Write your own label for the tick nearest where you clicked")
                        .clicked()
                {
                    edit_label = true;
                    ui.close_menu();
                }
                let PlotState { axes, bounds, last_view, .. } = &mut *state;
                plot_axes::single_axis_menu(ui, vertical, axes, bounds, *last_view, info);
            });
            if let Some((_, p)) = state.axis_menu_at.filter(|(v, _)| edit_label && *v == vertical) {
                state.tick_edit = Some(plot_axes::TickEdit::at(&response.transform, &state.axes, vertical, p));
            }
        }

        if resp.drag_started() {
            if let Some(p) = ui.input(|i| i.pointer.press_origin()) {
                let start_along = along(p);
                state.axis_drag = Some(AxisDrag {
                    axis,
                    handle: handle_at(start_along, length),
                    start: current,
                    start_along,
                    length,
                });
            }
        }
        let active = state.axis_drag.filter(|d| d.axis == axis);
        if resp.dragged() {
            if let (Some(d), Some(p)) = (active, resp.interact_pointer_pos()) {
                state.bounds = Some(if let Some(aspect) = state.axes.data_aspect() {
                    d.bounds_at_aspect(along(p), f64::from(frame.width()), f64::from(frame.height()), aspect)
                } else {
                    d.bounds_at(along(p))
                });
                ui.ctx().request_repaint();
            }
        }
        if resp.drag_stopped() {
            state.axis_drag = None;
        }

        // Cursor and handle hints.
        let hovered_handle = resp.hover_pos().map(|p| handle_at(along(p), length));
        let shown = active.map(|d| d.handle).or(hovered_handle);
        if let Some(handle) = shown {
            let icon = match (handle, axis, active.is_some()) {
                (AxisHandle::Shift, _, true) => egui::CursorIcon::Grabbing,
                (AxisHandle::Shift, _, false) => egui::CursorIcon::Grab,
                (_, Axis::X, _) => egui::CursorIcon::ResizeHorizontal,
                (_, Axis::Y, _) => egui::CursorIcon::ResizeVertical,
            };
            ui.ctx().set_cursor_icon(icon);
        }
        let opposite = match axis {
            Axis::X => state.axes.x.side == AxisSide::Opposite,
            Axis::Y => state.axes.y.side == AxisSide::Opposite,
        };
        paint_axis_handles(ui, strip, axis, shown, opposite);
    }
    if let Some(edit) = &mut state.tick_edit {
        if !plot_axes::tick_label_editor(ui, id, edit, &mut state.axes) {
            state.tick_edit = None;
        }
    }
}

/// Small marks at both ends of an axis strip showing where the limit handles
/// are; brighter while the axis is hovered or dragged.
fn paint_axis_handles(ui: &egui::Ui, strip: egui::Rect, axis: Axis, active: Option<AxisHandle>, opposite: bool) {
    let pal = theme::plot_palette(ui);
    let painter = ui.painter_at(strip.expand(2.0));
    let reach = END_HANDLE_FRACTION as f32;
    let (lower, upper) = match axis {
        Axis::X => {
            let w = strip.width() * reach;
            // Along the edge that touches the plot frame.
            let y = if opposite { strip.bottom() - 2.0 } else { strip.top() };
            (
                egui::Rect::from_min_size(egui::pos2(strip.left(), y), egui::vec2(w, 2.0)),
                egui::Rect::from_min_size(egui::pos2(strip.right() - w, y), egui::vec2(w, 2.0)),
            )
        }
        Axis::Y => {
            let h = strip.height() * reach;
            let x = if opposite { strip.left() } else { strip.right() - 2.0 };
            (
                egui::Rect::from_min_size(egui::pos2(x, strip.bottom() - h), egui::vec2(2.0, h)),
                egui::Rect::from_min_size(egui::pos2(x, strip.top()), egui::vec2(2.0, h)),
            )
        }
    };
    // Faint at rest (a hint, not a feature of the data); clear only while in use.
    let rest = ui.visuals().weak_text_color().gamma_multiply(0.35);
    let color = |handle: AxisHandle| {
        if active == Some(handle) {
            pal.point.gamma_multiply(0.85)
        } else if active.is_some() {
            ui.visuals().weak_text_color().gamma_multiply(0.6)
        } else {
            rest
        }
    };
    painter.rect_filled(lower, 1.0_f32, color(AxisHandle::Lower));
    painter.rect_filled(upper, 1.0_f32, color(AxisHandle::Upper));
    if active == Some(AxisHandle::Shift) {
        let mid = match axis {
            Axis::X => egui::Rect::from_min_max(lower.right_top(), upper.left_bottom()),
            Axis::Y => egui::Rect::from_min_max(upper.left_bottom(), lower.right_top()),
        };
        painter.rect_filled(mid, 1.0_f32, pal.point.gamma_multiply(0.4));
    }
}

/// A completed pointer gesture on a plot, in plot or screen coordinates.
enum Gesture {
    /// Box selection finished (plot coordinates).
    Region(PlotRegion),
    /// Click without drag (screen position).
    Click(egui::Pos2),
}

/// Track box-drag and click gestures inside a plot. The drag start is kept in
/// the plot state across frames; the finished gesture is returned with the
/// selection mode implied by the held modifier keys.
fn plot_gesture(ui: &egui::Ui, response: &PlotResponse<()>, state: &mut PlotState) -> Option<(Gesture, SelectionMode)> {
    let modifiers = ui.input(|i| i.modifiers);
    let mode = selection_mode(modifiers.shift, modifiers.command, modifiers.alt);
    let r = &response.response;
    let transform = &response.transform;
    if r.drag_started_by(PointerButton::Primary) {
        if let Some(origin) = ui.input(|i| i.pointer.press_origin()) {
            let p = transform.value_from_position(origin);
            state.brush_start = Some([p.x, p.y]);
        }
        None
    } else if r.drag_stopped_by(PointerButton::Primary) {
        let end = r.interact_pointer_pos().or_else(|| r.hover_pos())?;
        let a = state.brush_start.take()?;
        let b = transform.value_from_position(end);
        Some((Gesture::Region(PlotRegion::from_corners(a, [b.x, b.y])), mode))
    } else if r.clicked_by(PointerButton::Primary) {
        r.interact_pointer_pos().map(|pos| (Gesture::Click(pos), mode))
    } else {
        None
    }
}

/// Draw the rubber-band rectangle of an in-progress box selection.
fn draw_rubber_band(plot: &mut egui_plot::PlotUi, start: Option<[f64; 2]>) {
    let pal = plot_palette_of(plot);
    if let (Some(a), Some(b)) = (start, plot.pointer_coordinate()) {
        let region = PlotRegion::from_corners(a, [b.x, b.y]);
        plot.polygon(
            Polygon::new(PlotPoints::from(region.corners().to_vec()))
                .fill_color(pal.brush_fill)
                .stroke(Stroke::new(1.0_f32, pal.selected)),
        );
    }
}

/// Divider lines between consecutive panes and each pane's heading, centred
/// above it at `heading_y` (the highest visible data value, so the heading never
/// enlarges the auto-fitted range).
fn draw_panes(plot: &mut egui_plot::PlotUi, panes: &[app_model::PlotPane], heading_y: f64) {
    let pal = plot_palette_of(plot);
    for w in panes.windows(2) {
        let x = 0.5 * (w[0].x_max + w[1].x_min);
        plot.vline(VLine::new(x).color(pal.divider).width(1.5_f32));
    }
    if heading_y.is_finite() {
        for pane in panes {
            let centre = 0.5 * (pane.x_min + pane.x_max);
            plot.text(
                Text::new(PlotPoint::new(centre, heading_y), RichText::new(&pane.label).strong())
                    .color(pal.divider)
                    .anchor(egui::Align2::CENTER_BOTTOM),
            );
        }
    }
}

/// How the default view frames one axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AxisFit {
    /// [`stat_viz::auto_range`]: 6% padding, never below zero for data that
    /// are all non-negative.
    Auto,
    /// A date-time axis ([`stat_viz::time_scale::time_range`]): padding with
    /// no floor at zero, a single value framed by a day or an hour.
    Time(stat_data::TemporalFormat),
    /// 6% padding on both sides, crossing zero freely (regions such as
    /// contour plots, whose edges carry no meaning).
    Free,
    /// As [`AxisFit::Auto`], except that points at or just above zero get
    /// the usual padding below them too: drawn on the frame's edge they would
    /// be cut in half and hidden by the axis line (a 0/1 response, dendrogram
    /// leaves, Cook's distances near zero).
    Points,
    /// Categories at whole-number positions: half a category beyond the
    /// first and last, so their points sit clear of the frame.
    Categories,
}

/// Running minimum and maximum of plotted coordinates.
struct Extent {
    min: [f64; 2],
    max: [f64; 2],
}

impl Extent {
    fn new() -> Self {
        Self { min: [f64::INFINITY; 2], max: [f64::NEG_INFINITY; 2] }
    }

    fn add(&mut self, p: [f64; 2]) {
        for d in 0..2 {
            if p[d].is_finite() {
                self.min[d] = self.min[d].min(p[d]);
                self.max[d] = self.max[d].max(p[d]);
            }
        }
    }

    /// Default view: each axis framed as `fit` says. `extra_high` adds further
    /// headroom (as a fraction of the range) above the top.
    fn view(&self, fit: [AxisFit; 2], extra_high: [f64; 2]) -> Option<ViewBounds> {
        let mut lo = [0.0; 2];
        let mut hi = [0.0; 2];
        for d in 0..2 {
            if !(self.min[d].is_finite() && self.max[d].is_finite()) {
                return None;
            }
            let (a, b) = match fit[d] {
                AxisFit::Time(format) => stat_viz::time_scale::time_range(self.min[d], self.max[d], format),
                _ => stat_viz::auto_range(self.min[d], self.max[d]),
            };
            // Points whose floor at zero leaves less than the usual padding
            // below them get the padding anyway.
            let crowded = self.min[d] >= 0.0 && a == 0.0 && self.min[d] < b - self.max[d];
            let free = fit[d] == AxisFit::Free || (fit[d] == AxisFit::Points && crowded);
            (lo[d], hi[d]) = if fit[d] == AxisFit::Categories {
                (self.min[d] - 0.5, self.max[d] + 0.5)
            } else if free && self.max[d] > self.min[d] {
                let pad = b - self.max[d];
                (self.min[d] - pad, b)
            } else {
                (a, b)
            };
            hi[d] += extra_high[d] * (hi[d] - lo[d]);
        }
        Some(ViewBounds { x_min: lo[0], x_max: hi[0], y_min: lo[1], y_max: hi[1] })
    }
}

/// "Zoom to selected points": the view framing the plot's selected, visible
/// points, when chosen. Reset view (or a double-click) goes back.
fn zoom_to_selection_item(ui: &mut egui::Ui, spec: &PlotSpec, rows: &RowStateManager) -> Option<ViewBounds> {
    let mut e = Extent::new();
    let selected = spec.points.iter().filter(|p| rows.is_selected(p.row) && !rows.is_hidden(p.row));
    let mut n = 0;
    for p in selected {
        e.add([p.x, p.y]);
        n += 1;
    }
    let clicked = ui
        .add_enabled(n > 0, egui::Button::new("Zoom to selected points"))
        .on_hover_text("Set the axes to frame the selected points; Reset view goes back")
        .on_disabled_hover_text("Select some points first (drag a box in the plot)")
        .clicked();
    ui.separator();
    if clicked {
        ui.close_menu();
        e.view([AxisFit::Free, AxisFit::Free], [0.0, 0.0])
    } else {
        None
    }
}

/// Extent of everything a scatter plot draws (visible points, curves, bands,
/// segments and labelled markers).
fn scatter_extent(spec: &PlotSpec, rows: &RowStateManager) -> Extent {
    let mut e = Extent::new();
    spec.points.iter().filter(|p| !rows.is_hidden(p.row)).for_each(|p| e.add([p.x, p.y]));
    let curves = spec.line.iter().chain(spec.band.iter().flat_map(|(a, b)| [a, b])).chain(&spec.segments);
    curves.chain(spec.groups.iter().flat_map(|g| g.lines.iter())).flatten().for_each(|&p| e.add(p));
    if let Some(notes) = &spec.annotations {
        notes.items.iter().for_each(|it| e.add([it.0, it.1]));
        notes.labels.iter().for_each(|l| e.add([l.0, l.1]));
    }
    if let Some(r) = spec.contour.as_ref().and_then(|m| m.field.extent()) {
        e.add([r[0], r[1]]);
        e.add([r[2], r[3]]);
    }
    e
}

/// Scatter plot whose points follow the shared row state, with box and click
/// selection written back to it. Grouped plots draw each group with its own
/// color and marker shape (and optional line); selected points get a ring.
#[allow(clippy::too_many_arguments)]
fn scatter_plot(
    ui: &mut egui::Ui,
    title: &str,
    spec: &PlotSpec,
    table: &DataTable,
    rows: &mut RowStateManager,
    state: &mut PlotState,
    id: (u64, SectionKind),
    reset: bool,
    contour_style: Option<&mut Option<stat_viz::ContourStyle>>,
) {
    let pal = theme::plot_palette(ui);
    // Axis breaks on continuous axes: the plot is drawn through them, and
    // new breaks fit the view again (its units changed).
    state.axes.breaks = app_model::plot_breaks::breakable(spec);
    let wanted = [state.axes.x.breaks.clone(), state.axes.y.breaks.clone()];
    let rebreak = state.drawn_breaks != wanted;
    if rebreak {
        state.drawn_breaks = wanted;
        state.bounds = None;
        state.axis_drag = None;
    }
    let breaks = app_model::plot_breaks::plot_breaks(spec, &state.axes);
    // Without breaks the spec is drawn as it is (no copy of every point each frame).
    let broken;
    let (spec, hidden) = if breaks.iter().all(Option::is_none) {
        (spec, 0)
    } else {
        let (b, hidden) = app_model::plot_breaks::broken_plot(spec, &breaks);
        broken = b;
        (&broken, hidden)
    };
    let break_notes = app_model::plot_breaks::breaks_notes(&state.axes, &breaks, hidden);
    let info = AxisInfo {
        title,
        x_label: &spec.x_label,
        y_label: &spec.y_label,
        categorical_x: spec.x_ticks.is_some(),
        has_y: true,
    };
    let reset = match (contour_style, &spec.contour) {
        (Some(style), Some(map)) => {
            let mut extra = |ui: &mut egui::Ui| crate::contour_view::settings(ui, style, map);
            plot_toolbar_with(ui, state, &info, Some(&mut extra))
        }
        _ => plot_toolbar(ui, state, &info),
    } || reset;
    let contours = spec.contour.as_ref().map(|map| state.contours.get(map, pal.dark));
    let rim = contours.as_ref().is_some_and(|c| c.filled());
    let mut axes = state.axes.clone();
    (axes.x.time, axes.y.time) = (spec.x_time, spec.y_time);
    if spec.simplex {
        // A triangle drawn true to shape, with no numeric axes.
        axes.equal_scale = true;
        for a in [&mut axes.x, &mut axes.y] {
            (a.tick_labels, a.tick_marks, a.grid) = (false, false, GridLines::None);
        }
    }
    // Partition visible points; hidden rows are not drawn.
    let n_groups = spec.groups.as_ref().map_or(1, |g| g.labels.len().max(1));
    let mut by_group: Vec<Vec<[f64; 2]>> = vec![Vec::new(); n_groups];
    let (mut excluded, mut selected) = (Vec::new(), Vec::new());
    // Ungrouped points draw the rows' own colours and markers, when set.
    let row_styled = spec.groups.is_none() && rows.has_row_styles();
    type RowStyle = (Option<stat_data::RowColor>, Option<stat_data::RowMarker>);
    let mut by_style: Vec<(RowStyle, Vec<[f64; 2]>)> = Vec::new();
    for (i, p) in spec.points.iter().enumerate() {
        if rows.is_hidden(p.row) {
            continue;
        }
        let xy = [p.x, p.y];
        if rows.is_excluded(p.row) {
            excluded.push(xy);
        } else if row_styled && p.row < rows.len() {
            let style = (rows.color(p.row), rows.marker(p.row));
            match by_style.iter_mut().find(|(s, _)| *s == style) {
                Some((_, points)) => points.push(xy),
                None => by_style.push((style, vec![xy])),
            }
        } else {
            let g = spec.groups.as_ref().and_then(|g| g.membership.get(i).copied()).unwrap_or(0);
            by_group[g.min(n_groups - 1)].push(xy);
        }
        if rows.is_selected(p.row) {
            selected.push(xy);
        }
    }
    // Above the point budget the points are drawn as one density picture
    // (one marker per row would overflow the GPU's buffers).
    let shown_points = by_group.iter().map(Vec::len).sum::<usize>()
        + by_style.iter().map(|(_, p)| p.len()).sum::<usize>()
        + excluded.len();
    let rastered = shown_points > stat_viz::point_raster::POINT_BUDGET;
    let revision = rows.revision();
    let mut key = PlotKey::default();
    if let Some(notes) = &spec.annotations {
        // An annotation group named like a point group (a tuning plot's chosen
        // setting) is already in the key.
        let point_groups: Vec<String> =
            spec.groups.as_ref().map(|g| (0..g.labels.len()).map(|k| g.key_label(k)).collect()).unwrap_or_default();
        for (g, legend) in notes.groups.iter().enumerate() {
            if point_groups.contains(legend) {
                continue;
            }
            let (color, shape) = pal.group_style(g + spec.annotation_style_offset);
            key.marker(legend.clone(), color, shape, true);
        }
    }
    if spec.band.is_some() {
        key.line(spec.band_label(), pal.band, LinePattern::Dashed);
    }
    // A flat line at zero (residual plots) is a baseline rather than a fit.
    let zero_line = spec.line.as_ref().is_some_and(|l| !l.is_empty() && l.iter().all(|p| p[1] == 0.0));
    if zero_line {
        key.line("Zero", pal.zero_baseline, LinePattern::Solid);
    } else if spec.line.is_some() {
        key.line(spec.line_label(), pal.fit_line, LinePattern::Solid);
    }
    match &spec.groups {
        Some(groups) => {
            for g in 0..n_groups {
                let (color, shape) = pal.group_style(g);
                key.marker(groups.key_label(g), color, shape, true);
            }
        }
        None if row_styled => {
            let shown: Vec<usize> = spec.points.iter().map(|p| p.row).filter(|&r| !rows.is_hidden(r)).collect();
            match stat_viz::row_style_key(table, rows, &shown) {
                Some(stat_viz::ColorKey::Levels { title, entries }) => {
                    for (level, color, shape) in entries {
                        let c = theme::row_color(stat_viz::row_styles::row_color(color), pal.dark);
                        key.marker(format!("{title} = {level}"), c, stat_viz::render::marker(shape), true);
                    }
                }
                Some(stat_viz::ColorKey::Gradient { title, min, max }) => {
                    let (lo, hi) = (stat_viz::palette::ramp(0.0), stat_viz::palette::ramp(1.0));
                    let fmt = |v: f64| app_model::format::format_number(v, 4);
                    key.marker(format!("{title} {}", fmt(min)), crate::contour_view::color32(lo), MarkerShape::Circle, true);
                    key.marker(format!("{title} {}", fmt(max)), crate::contour_view::color32(hi), MarkerShape::Circle, true);
                }
                None => {}
            }
            if by_group.first().is_some_and(|p| !p.is_empty())
                || by_style.iter().any(|((c, m), _)| c.is_none() && m.is_none())
            {
                key.marker(spec.point_label(), pal.point, MarkerShape::Circle, true);
            }
        }
        None if !spec.points.is_empty() => key.marker(spec.point_label(), pal.point, MarkerShape::Circle, true),
        None => {}
    }
    if !excluded.is_empty() {
        key.marker("Excluded", pal.excluded, MarkerShape::Cross, false);
    }
    if !selected.is_empty() {
        key.marker("Selected", pal.selected, MarkerShape::Circle, false);
    }
    let size = state.size.capped(ui, 300.0);
    plot_title(ui, &axes, title, size.x);
    // The colour bar takes its room from the plot's width.
    let bar_width = contours.as_ref().map_or(0.0, |c| c.bar_width(ui)).min(size.x / 3.0);
    let start = if state.lasso { None } else { state.brush_start };
    let outline = state.lasso_path.clone();
    let refs = std::mem::take(&mut state.reference_marks);
    let (reset, bounds) = prepare_view(state, reset);
    let plot = interactive_plot!(("plot", id.0, id.1), reset);
    let category = &spec.x_ticks.as_ref().map(|t| {
        let key = ui.make_persistent_id(("category_labels", "plot", id.0, id.1));
        plot_axes::CategoryAxis::new(ui, key, t, axes.category_labels)
    });
    let plot = with_x_ticks!(ui, plot, &spec.x_ticks, category.as_ref());
    let plot = with_axes!(plot, &axes, spec.x_ticks.is_some());
    let plot = with_y_ticks!(plot, &spec.y_ticks);
    let plot = plot_axes::with_broken_axes(plot, &breaks, &axes);
    let label_backing = pal.plot_bg.gamma_multiply(0.85);
    // Pane headings sit just above the highest visible point; leave headroom so
    // they are not clipped.
    let (low_y, heading_y) = spec
        .points
        .iter()
        .filter(|p| !rows.is_hidden(p.row))
        .map(|p| p.y)
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), y| (lo.min(y), hi.max(y)));
    // With the values hidden (a multi-vari chart of means only), the headings
    // stand on the highest marked mean instead.
    let (low_y, heading_y) = if heading_y.is_finite() {
        (low_y, heading_y)
    } else {
        spec.annotations
            .iter()
            .flat_map(|n| n.items.iter().map(|it| it.1))
            .filter(|y| y.is_finite())
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), y| (lo.min(y), hi.max(y)))
    };
    // Default view: 6% padding, never below zero for non-negative data
    // (categorical and paned axes keep their own layout), and headroom for
    // pane headings.
    let headroom = if spec.panes.is_empty() || !heading_y.is_finite() || heading_y <= low_y { 0.0 } else { 0.12 };
    // Contour plots (which carry contour-value labels) show the region of the
    // data rather than starting at the origin.
    let contour = spec.contour.is_some() || spec.annotations.as_ref().is_some_and(|n| !n.labels.is_empty());
    let fit = |auto: bool, time: Option<stat_data::TemporalDisplay>| match time {
        Some(t) => AxisFit::Time(t.format),
        None if auto => AxisFit::Points,
        None => AxisFit::Free,
    };
    let default_view = scatter_extent(spec, rows).view(
        [
            fit(spec.x_ticks.is_none() && spec.panes.is_empty() && !contour, spec.x_time),
            fit(!contour && spec.y_ticks.is_none(), spec.y_time),
        ],
        [0.0, headroom],
    );
    let chrome = if spec.simplex { PlotChrome::begin(ui).without_axes() } else { PlotChrome::begin(ui) };
    let response = plot
        .width(size.x - bar_width)
        .height(size.y + category.as_ref().map_or(0.0, plot_axes::CategoryAxis::extra_height))
        .x_axis_label(chrome.axis_title(&axes.x, &spec.x_label))
        .y_axis_label(chrome.axis_title(&axes.y, &spec.y_label))
        .show(ui, |plot| {
            let frame_size = plot.transform().frame().size();
            apply_bounds(plot, plot_axes::view_to_show(bounds, reference_view(default_view, &refs), &axes, frame_size, spec.x_ticks.is_some()));
            stat_viz::render::draw_layer_in(plot, &refs.back, pal.dark);
            if let Some(c) = &contours {
                c.draw_under(plot, &pal);
            }
            draw_panes(plot, &spec.panes, heading_y);
            for segment in &spec.segments {
                plot.line(Line::new(PlotPoints::from(segment.clone())).color(pal.chart_trace).width(1.2_f32));
            }
            for run in &spec.point_runs {
                plot.line(Line::new(PlotPoints::from(run.clone())).color(pal.point.gamma_multiply(0.6)).width(1.0_f32));
            }
            if let Some(notes) = &spec.annotations {
                for (g, legend) in notes.groups.iter().enumerate() {
                    let (color, shape) = pal.group_style(g + spec.annotation_style_offset);
                    let pts: Vec<[f64; 2]> = notes.items.iter().filter(|it| it.3 == g).map(|it| [it.0, it.1]).collect();
                    plot.points(
                        Points::new(PlotPoints::from(pts)).shape(shape).radius(5.0_f32).color(color).name(legend),
                    );
                }
                // Marker-free labels (e.g. contour values), on a backing so the line under them stays readable.
                for (x, y, text) in &notes.labels {
                    plot.text(
                        Text::new(
                            PlotPoint::new(*x, *y),
                            RichText::new(text.as_str()).small().background_color(label_backing),
                        )
                        .color(pal.chart_trace)
                        .anchor(egui::Align2::CENTER_CENTER),
                    );
                }
            }
            if let Some(c) = &contours {
                c.draw_labels(plot, &pal);
            }
            if let Some((lower, upper)) = &spec.band {
                for curve in [lower, upper] {
                    plot.line(
                        Line::new(PlotPoints::from(curve.clone()))
                            .color(pal.band)
                            .style(LineStyle::dashed_loose())
                            .name(spec.band_label()),
                    );
                }
            }
            if zero_line {
                plot.hline(HLine::new(0.0).color(pal.zero_baseline).width(1.0_f32).name("Zero"));
            } else if let Some(line) = &spec.line {
                plot.line(
                    Line::new(PlotPoints::from(line.clone()))
                        .color(pal.fit_line)
                        .width(1.75_f32)
                        .name(spec.line_label()),
                );
            }
            if rastered {
                if let Some(groups) = &spec.groups {
                    for (g, line) in groups.lines.iter().enumerate().filter(|(_, l)| !l.is_empty()) {
                        let name = groups.key_label(g);
                        plot.line(
                            Line::new(PlotPoints::from(line.clone())).color(pal.group_style(g).0).width(1.5_f32).name(&name),
                        );
                    }
                }
                let groups = spec.groups.is_some();
                plot.add(point_picture(&pal, revision, (by_group, groups), by_style, excluded, selected));
            } else {
                match &spec.groups {
                    Some(groups) => {
                        for (g, points) in by_group.into_iter().enumerate() {
                            let (color, shape) = pal.group_style(g);
                            let name = groups.key_label(g);
                            if let Some(line) = groups.lines.get(g).filter(|l| !l.is_empty()) {
                                plot.line(
                                    Line::new(PlotPoints::from(line.clone())).color(color).width(1.5_f32).name(&name),
                                );
                            }
                            glow(plot, &points, 4.0, color, &pal);
                            if rim {
                                plot.points(
                                    Points::new(PlotPoints::from(points.clone()))
                                        .shape(shape)
                                        .radius(5.6_f32)
                                        .color(pal.plot_bg),
                                );
                            }
                            plot.points(
                                Points::new(PlotPoints::from(points)).shape(shape).radius(4.0_f32).color(color).name(&name),
                            );
                        }
                    }
                    None => {
                        for ((color, marker), points) in by_style {
                            let color = color.map_or(pal.point, |c| theme::row_color(c, pal.dark));
                            let shape = marker.map_or(MarkerShape::Circle, theme::row_marker);
                            glow(plot, &points, 4.0, color, &pal);
                            if rim {
                                plot.points(
                                    Points::new(PlotPoints::from(points.clone()))
                                        .shape(shape)
                                        .radius(5.6_f32)
                                        .color(pal.plot_bg),
                                );
                            }
                            plot.points(Points::new(PlotPoints::from(points)).shape(shape).radius(4.0_f32).color(color));
                        }
                        let points = by_group.into_iter().next().unwrap_or_default();
                        glow(plot, &points, 4.0, pal.point, &pal);
                        if rim {
                            // A rim in the canvas colour keeps points visible on any band.
                            plot.points(Points::new(PlotPoints::from(points.clone())).radius(5.6_f32).color(pal.plot_bg));
                        }
                        plot.points(
                            Points::new(PlotPoints::from(points)).radius(4.0_f32).color(pal.point).name(spec.point_label()),
                        );
                    }
                }
                if !excluded.is_empty() {
                    plot.points(
                        Points::new(PlotPoints::from(excluded))
                            .shape(MarkerShape::Cross)
                            .radius(4.0_f32)
                            .color(pal.excluded)
                            .name("Excluded"),
                    );
                }
                if !selected.is_empty() {
                    plot.points(
                        Points::new(PlotPoints::from(selected))
                            .shape(MarkerShape::Circle)
                            .filled(false)
                            .radius(7.0_f32)
                            .color(pal.selected)
                            .name("Selected"),
                    );
                }
            }
            draw_rubber_band(plot, start);
            if let Some(mut path) = outline {
                if let Some(first) = path.first().copied() {
                    path.push(first);
                }
                plot.line(Line::new(PlotPoints::from(path)).color(pal.selected).width(1.5_f32));
            }
            stat_viz::render::draw_layer_in(plot, &refs.front, pal.dark);
        });

    // Labelled vertical ticks (a probability scale) rule at their own positions.
    let y_spacer: Option<plot_axes::Spacer> = spec.y_ticks.as_ref().map(|ticks| {
        let positions: Vec<f64> = ticks.iter().map(|t| t.0).collect();
        Box::new(move |_: egui_plot::GridInput| {
            positions.iter().map(|&value| GridMark { value, step_size: 1e6 }).collect()
        }) as plot_axes::Spacer
    });
    // A broken vertical axis rules at each piece's own ticks.
    let y_spacer = match &breaks[1] {
        Some(b) => Some(plot_axes::broken_spacer(plot_axes::linear_axis(), b.clone(), true)),
        None => y_spacer,
    };
    chrome.finish(ui, &response.transform, &axes, spec.x_ticks.is_some(), y_spacer.as_ref());
    if let (Some(category), Some(ticks)) = (&category, &spec.x_ticks) {
        category.paint(ui, &response.transform, ticks, axes.x.side);
    }
    if let Some(c) = &contours {
        c.paint_bar(ui, *response.transform.frame());
    }
    if let Some(notes) = &spec.annotations {
        // Labelled markers (categories on a correspondence map, say): each
        // name just right of its marker, moved off other markers and names.
        let mut labels = plot_axes::LabelLayer::new(response.transform.frame().shrink(2.0));
        let at = |x: f64, y: f64| response.transform.position_from_point(&PlotPoint::new(x, y));
        // Unlabelled markers (a Shepard diagram's pairs, say) get no text.
        let labelled = || notes.items.iter().filter(|it| !it.2.is_empty());
        for it in labelled() {
            labels.avoid(egui::Rect::from_center_size(at(it.0, it.1), egui::vec2(10.0, 10.0)));
        }
        let font = egui::TextStyle::Body.resolve(ui.style());
        for it in labelled() {
            let color = pal.group_style(it.3).0;
            let pos = at(it.0, it.1) + egui::vec2(7.0, 0.0);
            labels.text(ui, pos, egui::Align2::LEFT_CENTER, it.2.as_str(), font.clone(), color);
        }
        labels.paint(ui);
    }
    axis_controls(ui, &response, state, ui.make_persistent_id(("plot_axes", id.0, id.1)), &info);
    let mut tick_axes = axes.clone();
    if spec.y_ticks.is_some() {
        tick_axes.y.tick_marks = false;
    }
    // Broken axes draw their own tick marks, on every piece, and break marks.
    tick_axes.x.tick_marks &= breaks[0].is_none();
    tick_axes.y.tick_marks &= breaks[1].is_none();
    plot_axes::paint_tick_marks(ui, &response.transform, &tick_axes, spec.x_ticks.is_some());
    let linear = plot_axes::linear_axis();
    plot_axes::paint_broken_tick_marks(
        ui,
        &response.transform,
        &axes,
        breaks[0].as_ref().map(|b| (&linear, b)),
        breaks[1].as_ref().map(|b| (&linear, b)),
    );
    plot_axes::paint_break_marks(ui, &response.transform, &axes, breaks[0].as_ref(), breaks[1].as_ref(), false);
    size_grip(ui, &mut state.size, size, response.response.rect, ui.make_persistent_id(("plot_axes_size", id.0, id.1)));
    if axes.show_key {
        reference_key(&mut key, &refs, pal.dark);
        key.show(ui);
        reference_notes(ui, &refs);
    }
    for note in &break_notes {
        ui.label(RichText::new(note).weak().small());
    }
    if rastered {
        ui.label(RichText::new(dense_points_note(shown_points)).weak().small());
    }
    if state.lasso {
        lasso_gesture(ui, &response, state, spec, rows);
    }
    match plot_gesture(ui, &response, state) {
        Some((Gesture::Region(_), _)) if state.lasso => {}
        Some((Gesture::Region(region), mode)) => {
            brush_region(&spec.points, &region, mode, rows);
        }
        Some((Gesture::Click(pos), mode)) => {
            let screen: Vec<(usize, [f32; 2])> = spec
                .points
                .iter()
                .filter(|p| !rows.is_hidden(p.row))
                .map(|p| {
                    let s = response.transform.position_from_point(&PlotPoint::new(p.x, p.y));
                    (p.row, [s.x, s.y])
                })
                .collect();
            brush_click(nearest_row(screen, [pos.x, pos.y], CLICK_RADIUS), mode, rows);
        }
        None => {}
    }
    point_tooltip(ui, &response, spec, table, rows, &breaks);
}

/// Freehand selection in a report scatter plot: dragging draws an outline
/// (kept in plot coordinates); letting go selects the visible points inside
/// it, with the selection mode of the held keys.
fn lasso_gesture(
    ui: &egui::Ui,
    response: &egui_plot::PlotResponse<()>,
    state: &mut PlotState,
    spec: &PlotSpec,
    rows: &mut RowStateManager,
) {
    let modifiers = ui.input(|i| i.modifiers);
    let mode = selection_mode(modifiers.shift, modifiers.command, modifiers.alt);
    let r = &response.response;
    let transform = &response.transform;
    let at = |p: egui::Pos2| {
        let v = transform.value_from_position(p);
        [v.x, v.y]
    };
    if r.drag_started_by(PointerButton::Primary) {
        state.lasso_path = ui.input(|i| i.pointer.press_origin()).map(|origin| vec![at(origin)]);
    } else if r.dragged_by(PointerButton::Primary) {
        if let (Some(points), Some(p)) = (state.lasso_path.as_mut(), r.interact_pointer_pos()) {
            // Add a vertex once the pointer has moved a few points.
            let far = points
                .last()
                .is_none_or(|last| transform.position_from_point(&PlotPoint::new(last[0], last[1])).distance(p) > 3.0);
            if far {
                points.push(at(p));
            }
            ui.ctx().request_repaint();
        }
    } else if r.drag_stopped_by(PointerButton::Primary) {
        if let Some(points) = state.lasso_path.take() {
            let positions: Vec<(usize, [f64; 2])> =
                spec.points.iter().filter(|p| !rows.is_hidden(p.row)).map(|p| (p.row, [p.x, p.y])).collect();
            rows.select(stat_viz::rows_in_polygon(&positions, &points), mode);
        }
    }
}

/// While the pointer rests on a point (and no button is held), a tooltip
/// with the point's row number and its two values.
fn point_tooltip(
    ui: &egui::Ui,
    response: &egui_plot::PlotResponse<()>,
    spec: &PlotSpec,
    table: &DataTable,
    rows: &RowStateManager,
    breaks: &[Option<stat_viz::BrokenAxis>; 2],
) {
    if ui.input(|i| i.pointer.any_down()) {
        return;
    }
    let Some(pos) = response.response.hover_pos() else { return };
    // By point index: a row can be drawn more than once (one point per model
    // or per condition), and the tooltip must show the point under the pointer.
    let screen = spec.points.iter().enumerate().filter(|(_, p)| !rows.is_hidden(p.row)).map(|(i, p)| {
        let s = response.transform.position_from_point(&PlotPoint::new(p.x, p.y));
        (i, [s.x, s.y])
    });
    let Some(point) = nearest_row(screen, [pos.x, pos.y], CLICK_RADIUS).and_then(|i| spec.points.get(i)) else {
        return;
    };
    let row = point.row;
    let value = |v: f64, time: Option<stat_data::TemporalDisplay>, ticks: Option<&Vec<(f64, String)>>| {
        if let Some(t) = time {
            return t.text(v);
        }
        // A categorical axis: the category whose position the (jittered) point is near.
        if let Some((_, label)) = ticks.and_then(|ts| ts.iter().find(|(at, _)| (at - v).abs() < 0.5)) {
            return label.clone();
        }
        app_model::format::format_number(v, 6)
    };
    // On a broken axis the drawn position reads back as the data value.
    let data = |v: f64, b: &Option<stat_viz::BrokenAxis>| b.as_ref().map_or(v, |b| b.value_at(v));
    let x = value(data(point.x, &breaks[0]), spec.x_time, spec.x_ticks.as_ref());
    let y = value(data(point.y, &breaks[1]), spec.y_time, None);
    let mut text = format!("Row {}\n{}: {x}\n{}: {y}", row + 1, spec.x_label, spec.y_label);
    text.push_str(&hover_lines(table, row));
    if rows.is_excluded(row) {
        text.push_str("\nExcluded from analyses");
    }
    egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), response.response.id.with("point"), |ui| {
        ui.label(text);
    });
}

/// The tooltip lines of the columns shown on hover (column right-click ▸
/// Show on hover in plots) for `row`: one `\nName: value` per column, the
/// value as the column names its levels (by value label when there is one).
pub(crate) fn hover_lines(table: &DataTable, row: usize) -> String {
    if row >= table.n_rows() {
        return String::new();
    }
    table
        .columns()
        .iter()
        .filter(|c| c.hover)
        .map(|c| {
            let v = c.level_text(row);
            format!("\n{}: {}", c.name, if v.is_empty() { "missing" } else { v.as_str() })
        })
        .collect()
}

/// `text` with its first letter in upper case.
fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// Survival curves: each group's Kaplan–Meier step function (solid) with its
/// confidence band shaded, fitted distributions (dashed Weibull, dotted
/// lognormal), and a marker for every analysed row on its curve — filled for an
/// event, hollow for a censored row — in the group's color and shape. Each
/// curve is labelled with its group at its right end, so groups never rely on
/// color alone. Box and click selection write to the shared row state.
fn survival_plot(
    ui: &mut egui::Ui,
    title: &str,
    spec: &SurvivalPlotSpec,
    rows: &mut RowStateManager,
    state: &mut PlotState,
    id: (u64, SectionKind),
    reset: bool,
) {
    let pal = theme::plot_palette(ui);
    let info = AxisInfo { title, x_label: &spec.x_label, y_label: &spec.y_label, categorical_x: false, has_y: true };
    let reset = plot_toolbar(ui, state, &info) || reset;
    let axes = state.axes.clone();
    let grouped = spec.curves.len() > 1 || spec.curves.iter().any(|c| !c.label.is_empty());
    let n = spec.curves.len().max(1);
    let mut events: Vec<Vec<[f64; 2]>> = vec![Vec::new(); n];
    let mut censored: Vec<Vec<[f64; 2]>> = vec![Vec::new(); n];
    let mut selected = Vec::new();
    for (i, p) in spec.points.iter().enumerate() {
        if rows.is_hidden(p.row) {
            continue;
        }
        let g = spec.membership.get(i).copied().unwrap_or(0).min(n - 1);
        let xy = [p.x, p.y];
        if spec.censored.get(i).copied().unwrap_or(false) {
            censored[g].push(xy)
        } else {
            events[g].push(xy)
        }
        if rows.is_selected(p.row) {
            selected.push(xy);
        }
    }
    // A mean cumulative function names its curve itself, and its hollow
    // markers are ends of observation rather than censored lifetimes.
    let estimate = spec.estimate.as_deref().unwrap_or("Kaplan–Meier");
    let hollow = if spec.estimate.is_some() { "end of observation" } else { "censored" };
    let mut key = PlotKey::default();
    for (g, curve) in spec.curves.iter().enumerate() {
        let (color, shape) = if grouped { pal.group_style(g) } else { (pal.point, MarkerShape::Circle) };
        let label = if grouped { curve.label.clone() } else { estimate.to_owned() };
        key.line(label.clone(), color, LinePattern::Solid);
        for (name, pattern, _) in &curve.fitted {
            key.line(if grouped { format!("{name} ({label})") } else { name.clone() }, color, *pattern);
        }
        let has =
            |cens: bool| spec.membership.iter().zip(&spec.censored).any(|(&m, &c)| m.min(n - 1) == g && c == cens);
        if has(false) {
            key.marker(if grouped { format!("{label}: event") } else { "Event".to_owned() }, color, shape, true);
        }
        if has(true) {
            key.marker(if grouped { format!("{label}: {hollow}") } else { capitalized(hollow) }, color, shape, false);
        }
    }
    if !selected.is_empty() {
        key.marker("Selected", pal.selected, MarkerShape::Circle, false);
    }
    // Default view: origin at the bottom-left corner, with room at the right
    // for the group labels at the curve ends.
    let mut extent = Extent::new();
    for curve in &spec.curves {
        curve.steps.iter().for_each(|&p| extent.add(p));
        curve.fitted.iter().flat_map(|f| f.2.iter()).for_each(|&p| extent.add(p));
        curve.band.iter().for_each(|&[x0, x1, lo, hi]| {
            extent.add([x0, lo]);
            extent.add([x1, hi]);
        });
    }
    spec.points.iter().for_each(|p| extent.add([p.x, p.y]));
    let default_view = extent.view([AxisFit::Auto, AxisFit::Auto], [if grouped { 0.08 } else { 0.0 }, 0.0]);
    let size = state.size.capped(ui, 320.0);
    plot_title(ui, &axes, title, size.x);
    let start = state.brush_start;
    let refs = std::mem::take(&mut state.reference_marks);
    let (reset, bounds) = prepare_view(state, reset);
    let plot = interactive_plot!(("survival", id.0, id.1), reset);
    let plot = with_axes!(plot, &axes, false);
    let chrome = PlotChrome::begin(ui);
    let response = plot
        .width(size.x)
        .height(size.y)
        .include_y(0.0)
        .include_y(if spec.estimate.is_some() { 0.0 } else { 1.0 })
        .include_x(0.0)
        .x_axis_label(chrome.axis_title(&axes.x, &spec.x_label))
        .y_axis_label(chrome.axis_title(&axes.y, &spec.y_label))
        .show(ui, |plot| {
            let frame_size = plot.transform().frame().size();
            apply_bounds(plot, plot_axes::view_to_show(bounds, reference_view(default_view, &refs), &axes, frame_size, false));
            stat_viz::render::draw_layer_in(plot, &refs.back, pal.dark);
            for (g, curve) in spec.curves.iter().enumerate() {
                let (color, shape) = if grouped { pal.group_style(g) } else { (pal.point, MarkerShape::Circle) };
                let label = if grouped { curve.label.clone() } else { estimate.to_owned() };
                for piece in &curve.band {
                    let [x0, x1, lo, hi] = *piece;
                    if x1 > x0 && hi > lo {
                        plot.polygon(
                            Polygon::new(PlotPoints::from(vec![[x0, lo], [x1, lo], [x1, hi], [x0, hi]]))
                                .fill_color(color.gamma_multiply(0.14))
                                .stroke(Stroke::NONE),
                        );
                    }
                }
                plot.line(Line::new(PlotPoints::from(curve.steps.clone())).color(color).width(2.0_f32).name(&label));
                for (name, pattern, line) in &curve.fitted {
                    let style = match pattern {
                        LinePattern::Solid => LineStyle::Solid,
                        LinePattern::Dashed => LineStyle::dashed_loose(),
                        LinePattern::Dotted => LineStyle::dotted_dense(),
                        LinePattern::DashDot => LineStyle::dashed_loose(),
                    };
                    let legend = if grouped { format!("{name} ({label})") } else { name.clone() };
                    plot.line(
                        Line::new(PlotPoints::from(line.clone())).color(color).width(1.4_f32).style(style).name(legend),
                    );
                }
                let event_name = if grouped { format!("{label}: event") } else { "Event".to_owned() };
                let censor_name = if grouped { format!("{label}: {hollow}") } else { capitalized(hollow) };
                plot.points(
                    Points::new(PlotPoints::from(std::mem::take(&mut events[g])))
                        .shape(shape)
                        .radius(3.0_f32)
                        .color(color)
                        .name(event_name),
                );
                plot.points(
                    Points::new(PlotPoints::from(std::mem::take(&mut censored[g])))
                        .shape(shape)
                        .filled(false)
                        .radius(4.5_f32)
                        .color(color)
                        .name(censor_name),
                );
            }
            if !selected.is_empty() {
                plot.points(
                    Points::new(PlotPoints::from(selected))
                        .shape(MarkerShape::Circle)
                        .filled(false)
                        .radius(7.0_f32)
                        .color(pal.selected)
                        .name("Selected"),
                );
            }
            draw_rubber_band(plot, start);
            stat_viz::render::draw_layer_in(plot, &refs.front, pal.dark);
        });

    chrome.finish(ui, &response.transform, &axes, false, None);
    if grouped {
        // Each group named at its curve's end, moved apart where ends crowd
        // together and kept off the axes.
        let mut labels = plot_axes::LabelLayer::new(response.transform.frame().shrink(2.0));
        let font = egui::TextStyle::Body.resolve(ui.style());
        for (g, curve) in spec.curves.iter().enumerate() {
            if let Some(&[x, y]) = curve.steps.last() {
                let at = response.transform.position_from_point(&PlotPoint::new(x, y)) + egui::vec2(4.0, 0.0);
                labels.text(ui, at, egui::Align2::LEFT_CENTER, &curve.label, font.clone(), pal.group_style(g).0);
            }
        }
        labels.paint(ui);
    }
    axis_controls(ui, &response, state, ui.make_persistent_id(("survival_axes", id.0, id.1)), &info);
    plot_axes::paint_tick_marks(ui, &response.transform, &axes, false);
    size_grip(
        ui,
        &mut state.size,
        size,
        response.response.rect,
        ui.make_persistent_id(("survival_axes_size", id.0, id.1)),
    );
    if let Some(risk) = &spec.risk_table {
        number_at_risk(ui, &response.transform, &axes, spec, risk, grouped);
    }
    if axes.show_key {
        reference_key(&mut key, &refs, pal.dark);
        key.show(ui);
        reference_notes(ui, &refs);
    }
    match plot_gesture(ui, &response, state) {
        Some((Gesture::Region(region), mode)) => {
            brush_region(&spec.points, &region, mode, rows);
        }
        Some((Gesture::Click(pos), mode)) => {
            let screen: Vec<(usize, [f32; 2])> = spec
                .points
                .iter()
                .filter(|p| !rows.is_hidden(p.row))
                .map(|p| {
                    let s = response.transform.position_from_point(&PlotPoint::new(p.x, p.y));
                    (p.row, [s.x, s.y])
                })
                .collect();
            brush_click(nearest_row(screen, [pos.x, pos.y], CLICK_RADIUS), mode, rows);
        }
        None => {}
    }
}

/// The number-at-risk table under a survival plot: one row per group (and a
/// row of censored counts per group when asked), with each count centred under
/// the labelled time tick it belongs to, so the table follows the axis when it
/// is dragged. Group names carry their curve's color and are always written,
/// so the rows never rely on color alone.
fn number_at_risk(
    ui: &mut egui::Ui,
    transform: &egui_plot::PlotTransform,
    axes: &app_model::axis_settings::PlotAxes,
    spec: &SurvivalPlotSpec,
    risk: &app_model::RiskTable,
    grouped: bool,
) {
    let pal = theme::plot_palette(ui);
    let frame = *transform.frame();
    let ticks: Vec<(f64, f32)> = plot_axes::labelled_x_ticks(transform, axes)
        .into_iter()
        .filter(|&(_, x)| x >= frame.left() - 0.5 && x <= frame.right() + 0.5)
        .collect();
    // (label, color, counts) per table row.
    let mut lines: Vec<(String, egui::Color32, Vec<String>)> = Vec::new();
    for (g, curve) in spec.curves.iter().enumerate() {
        let color = if grouped { pal.group_style(g).0 } else { pal.point };
        let label = if grouped { curve.label.clone() } else { "At risk".to_owned() };
        lines.push((label, color, ticks.iter().map(|&(t, _)| risk.at_risk(g, t).to_string()).collect()));
        if risk.censored {
            let counts = ticks
                .iter()
                .enumerate()
                .map(|(k, &(t, _))| {
                    let after = if k == 0 { f64::NEG_INFINITY } else { ticks[k - 1].0 };
                    format!("({})", risk.censored_between(g, after, t))
                })
                .collect();
            lines.push((if grouped { "censored".to_owned() } else { "Censored".to_owned() }, pal.tick_text, counts));
        }
    }
    let row_height = 17.0;
    let height = row_height * (lines.len() + 1) as f32 + 6.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::hover());
    let summary: Vec<String> = lines.iter().map(|(l, _, c)| format!("{l}: {}", c.join(" "))).collect();
    let name = format!("Number at risk. {}", summary.join("; "));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &name));
    let painter = ui.painter_at(rect.expand2(egui::vec2(0.0, 1.0)));
    let text_font = egui::TextStyle::Small.resolve(ui.style());
    let number_font = egui::FontId::monospace(text_font.size);
    painter.text(
        egui::pos2(rect.left() + 2.0, rect.top() + 2.0),
        egui::Align2::LEFT_TOP,
        "Number at risk",
        egui::TextStyle::Body.resolve(ui.style()),
        pal.axis_label,
    );
    for (i, (label, color, counts)) in lines.iter().enumerate() {
        let y = rect.top() + 2.0 + row_height * (i + 1) as f32 + row_height / 2.0;
        // Labels end just left of the plot area, under the vertical axis.
        painter.text(egui::pos2(frame.left() - 8.0, y), egui::Align2::RIGHT_CENTER, label, text_font.clone(), *color);
        for (&(_, x), count) in ticks.iter().zip(counts) {
            painter.text(egui::pos2(x, y), egui::Align2::CENTER_CENTER, count, number_font.clone(), pal.tick_text);
        }
    }
}

/// Each phase's name in a label along the top edge of a control chart,
/// centred over the part of the phase in view (from its left end when the
/// name is wider than that); returns where the labels went.
fn phase_names(
    ui: &egui::Ui,
    spec: &ControlChartSpec,
    phase_end: f64,
    transform: &egui_plot::PlotTransform,
    pal: &theme::PlotPalette,
) -> Vec<egui::Rect> {
    let frame = *transform.frame();
    let painter = ui.painter().with_clip_rect(frame);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let mut placed = Vec::new();
    for (k, m) in spec.phases.iter().enumerate() {
        let end = spec.phases.get(k + 1).map_or(phase_end, |next| next.at);
        let x = |v: f64| transform.position_from_point(&PlotPoint::new(v, 0.0)).x;
        let (left, right) = (x(m.at).max(frame.left()), x(end).min(frame.right()));
        if right - left < 4.0 {
            continue;
        }
        let galley = painter.layout_no_wrap(m.label.clone(), font.clone(), pal.axis_label);
        let size = galley.size() + egui::vec2(10.0, 4.0);
        let centre = if size.x <= right - left { (left + right) / 2.0 } else { left + 4.0 + size.x / 2.0 };
        let rect = egui::Rect::from_center_size(egui::pos2(centre, frame.top() + 4.0 + size.y / 2.0), size);
        painter.rect(
            rect,
            3.0,
            pal.plot_bg.gamma_multiply(0.9),
            egui::Stroke::new(1.0_f32, pal.axis_spine),
            egui::StrokeKind::Inside,
        );
        painter.galley(rect.center() - galley.size() / 2.0, galley, pal.axis_label);
        placed.push(rect);
    }
    placed
}

/// A control chart: points joined in time order, center line, control limits,
/// optional 1σ/2σ zone lines, and run-rule violations marked with the numbers of
/// the rules broken. Limits that change by phase or subgroup size are drawn as
/// steps; phases are separated by a solid rule, every other phase is shaded,
/// each is named in a label centred above it, and the line joining the points
/// breaks between phases. Hovering a point shows its
/// label. Brushing a point selects every dataset row behind it.
#[allow(clippy::too_many_arguments)]
fn control_chart(
    ui: &mut egui::Ui,
    title: &str,
    spec: &ControlChartSpec,
    rows: &mut RowStateManager,
    state: &mut PlotState,
    id: (u64, SectionKind),
    show_zones: bool,
    reset: bool,
) {
    let pal = theme::plot_palette(ui);
    let categorical = spec.x_ticks.is_some();
    let info =
        AxisInfo { title, x_label: &spec.x_label, y_label: &spec.y_label, categorical_x: categorical, has_y: true };
    let reset = plot_toolbar(ui, state, &info) || reset;
    let mut axes = state.axes.clone();
    // Samples are numbered 1, 2, 3, …: no ticks between them.
    axes.x.whole_numbers = !categorical;
    let visible: Vec<&app_model::ChartPoint> = spec.points.iter().filter(|p| chart_point_visible(p, rows)).collect();
    // The joining line, broken at phase boundaries.
    let mut traces: Vec<Vec<[f64; 2]>> = vec![Vec::new()];
    for p in &visible {
        let x = p.sample as f64;
        let crosses = traces
            .last()
            .and_then(|t| t.last())
            .is_some_and(|last| spec.phases.iter().any(|m| last[0] < m.at && m.at < x));
        if crosses {
            traces.push(Vec::new());
        }
        traces.last_mut().expect("never empty").push([x, p.value]);
    }
    let steps = spec.limit_steps();
    let stepped = steps.len() > 1 || !spec.phases.is_empty();
    let zones = show_zones && steps.iter().any(|s| s.zone_sigma > 0.0);
    let (mut normal, mut flagged, mut selected) = (Vec::new(), Vec::new(), Vec::new());
    for p in &visible {
        let xy = [p.sample as f64, p.value];
        if chart_point_selected(p, rows) {
            selected.push(xy);
        } else if p.flagged() {
            flagged.push(xy);
        } else {
            normal.push(xy);
        }
    }
    let zone_scores = spec.points.iter().any(|p| p.note.is_some());
    let mut key = PlotKey::default();
    if steps.iter().any(|s| s.center.is_finite()) {
        key.line("Center line", pal.center_line, LinePattern::Solid);
    }
    key.line(spec.limits_label(), pal.limit, LinePattern::Dashed);
    if zones {
        key.line("1σ and 2σ zones", pal.zone, LinePattern::Dotted);
    }
    if spec.phases.len() > 1 {
        key.line("Phase boundary", pal.axis_label, LinePattern::Solid);
    }
    key.marker("Samples", pal.point, MarkerShape::Circle, true);
    if !flagged.is_empty() {
        let label = spec.flag_label.as_deref().unwrap_or(if zone_scores {
            "Signal: cumulative zone score ≥ 8 (scores shown)"
        } else {
            "Rule violation (rule numbers shown)"
        });
        key.marker(label, pal.violation, MarkerShape::Diamond, true);
    }
    if !selected.is_empty() {
        key.marker("Selected", pal.selected, MarkerShape::Circle, true);
    }
    let size = state.size.full(ui, 240.0);
    plot_title(ui, &axes, title, size.x);
    let start = state.brush_start;
    let refs = std::mem::take(&mut state.reference_marks);
    let (reset, bounds) = prepare_view(state, reset);
    let plot = interactive_plot!(("chart", id.0, id.1), reset);
    let category = &spec.x_ticks.as_ref().map(|t| {
        let key = ui.make_persistent_id(("category_labels", "chart", id.0, id.1));
        plot_axes::CategoryAxis::new(ui, key, t, axes.category_labels)
    });
    let plot = with_x_ticks!(ui, plot, &spec.x_ticks, category.as_ref());
    let plot = with_axes!(plot, &axes, categorical);
    // Default view: every sample and both control limits.
    let mut extent = Extent::new();
    visible.iter().for_each(|p| extent.add([p.sample as f64, p.value]));
    for s in &steps {
        for y in [s.center, s.lower, s.upper] {
            extent.add([f64::NAN, y]);
        }
    }
    let x_fit = if categorical { AxisFit::Categories } else { AxisFit::Free };
    // Headroom for the rule numbers written above flagged points, and more
    // for the phase names along the top edge when there are some.
    let headroom = if spec.phases.len() > 1 { 0.16 } else { 0.06 };
    let default_view = extent.view([x_fit, AxisFit::Auto], [0.0, headroom]);
    // Where the last phase ends.
    let phase_end = spec.points.iter().map(|p| p.sample as f64 + 0.5).fold(f64::NEG_INFINITY, f64::max);
    let chrome = PlotChrome::begin(ui);
    let response = plot
        .width(size.x)
        .height(size.y + category.as_ref().map_or(0.0, plot_axes::CategoryAxis::extra_height))
        .x_axis_label(chrome.axis_title(&axes.x, &spec.x_label))
        .y_axis_label(chrome.axis_title(&axes.y, &spec.y_label))
        .show(ui, |plot| {
            let frame_size = plot.transform().frame().size();
            apply_bounds(plot, plot_axes::view_to_show(bounds, reference_view(default_view, &refs), &axes, frame_size, categorical));
            // Every other phase shaded, behind everything else (the names
            // say which phase is which, so the shading is not the only cue).
            if spec.phases.len() > 1 {
                let view = plot.plot_bounds();
                let (y0, y1) = (view.min()[1], view.max()[1]);
                for (k, m) in spec.phases.iter().enumerate().filter(|(k, _)| k % 2 == 1) {
                    let end = spec.phases.get(k + 1).map_or(phase_end, |next| next.at);
                    if end > m.at && y1 > y0 {
                        let corners = vec![[m.at, y0], [end, y0], [end, y1], [m.at, y1]];
                        plot.polygon(
                            Polygon::new(PlotPoints::from(corners))
                                .fill_color(pal.phase_shade)
                                .stroke(egui::Stroke::NONE),
                        );
                    }
                }
            }
            stat_viz::render::draw_layer_in(plot, &refs.back, pal.dark);
            for s in &steps {
                // A segment over the step's samples, or a full-width rule when
                // one set of limits holds throughout.
                let mut rule = |y: f64, color: egui::Color32, style: LineStyle, width: f32, name: &str| {
                    // A chart may have no center line or no lower limit.
                    if !y.is_finite() {
                        return;
                    }
                    if stepped {
                        let line = Line::new(PlotPoints::from(vec![[s.from, y], [s.to, y]]));
                        plot.line(line.color(color).style(style).width(width).name(name));
                    } else {
                        plot.hline(HLine::new(y).color(color).style(style).width(width).name(name));
                    }
                };
                if zones && s.zone_sigma > 0.0 {
                    for k in [-2.0, -1.0, 1.0, 2.0] {
                        let y = s.center + k * s.zone_sigma;
                        if y > s.lower && y < s.upper {
                            rule(y, pal.zone, LineStyle::dotted_dense(), 1.0, "");
                        }
                    }
                }
                rule(s.center, pal.center_line, LineStyle::Solid, 1.5, "Center line");
                for limit in [s.upper, s.lower] {
                    rule(limit, pal.limit, LineStyle::dashed_loose(), 1.5, spec.limits_label());
                }
            }
            for m in spec.phases.iter().skip(1) {
                plot.vline(VLine::new(m.at).color(pal.axis_label).style(LineStyle::Solid).width(1.5_f32));
            }
            for trace in traces {
                plot.line(Line::new(PlotPoints::from(trace)).color(pal.chart_trace).width(1.0_f32));
            }
            glow(plot, &normal, 4.0, pal.point, &pal);
            plot.points(Points::new(PlotPoints::from(normal)).radius(4.0_f32).color(pal.point).name("Samples"));
            if !flagged.is_empty() {
                plot.points(
                    Points::new(PlotPoints::from(flagged))
                        .shape(MarkerShape::Diamond)
                        .radius(5.5_f32)
                        .color(pal.violation)
                        .name("Rule violation"),
                );
            }
            if !selected.is_empty() {
                plot.points(
                    Points::new(PlotPoints::from(selected)).radius(5.0_f32).color(pal.selected).name("Selected"),
                );
            }
            draw_rubber_band(plot, start);
            stat_viz::render::draw_layer_in(plot, &refs.front, pal.dark);
        });

    chrome.finish(ui, &response.transform, &axes, categorical, None);
    if let (Some(category), Some(ticks)) = (&category, &spec.x_ticks) {
        category.paint(ui, &response.transform, ticks, axes.x.side);
    }
    // Label each flagged point with the numbers of the rules it breaks; on a
    // zone chart every point carries its cumulative score instead. Labels sit
    // just above their points, moved off other points and labels.
    let mut labels = plot_axes::LabelLayer::new(*response.transform.frame());
    let at =
        |p: &app_model::ChartPoint| response.transform.position_from_point(&PlotPoint::new(p.sample as f64, p.value));
    for p in &visible {
        labels.avoid(egui::Rect::from_center_size(at(p), egui::vec2(9.0, 9.0)));
    }
    if spec.phases.len() > 1 {
        for rect in phase_names(ui, spec, phase_end, &response.transform, &pal) {
            labels.avoid(rect);
        }
    }
    let small = egui::TextStyle::Small.resolve(ui.style());
    for p in visible.iter().filter(|p| p.note.is_some() || (p.flagged() && spec.flag_label.is_none())) {
        let label = match &p.note {
            Some(n) => n.clone(),
            None => p.rules.iter().map(u8::to_string).collect::<Vec<_>>().join(","),
        };
        let color = if p.flagged() { pal.violation } else { pal.axis_label };
        labels.text(ui, at(p) - egui::vec2(0.0, 6.0), egui::Align2::CENTER_BOTTOM, label, small.clone(), color);
    }
    labels.paint(ui);
    if let Some(pos) = response.response.hover_pos().filter(|_| start.is_none()) {
        let screen = visible.iter().enumerate().map(|(i, p)| {
            let s = response.transform.position_from_point(&PlotPoint::new(p.sample as f64, p.value));
            (i, [s.x, s.y])
        });
        if let Some(p) = nearest_row(screen, [pos.x, pos.y], CLICK_RADIUS).map(|i| visible[i]) {
            let mut text = match (&spec.x_ticks, &p.label) {
                (Some(_), Some(label)) => label.clone(),
                (_, Some(label)) => format!("Sample {} ({label})", p.sample),
                (_, None) => format!("Sample {}", p.sample),
            };
            text.push_str(&format!("\n{}: {}", spec.y_label, app_model::format::format_number(p.value, 6)));
            if let Some(m) = spec.phases.iter().rev().find(|m| m.at < p.sample as f64) {
                text.push_str(&format!("\nPhase: {}", m.label));
            }
            if let (false, Some(flag)) = (p.rules.is_empty(), &spec.flag_label) {
                text.push_str(&format!("\n{flag}"));
            } else if !p.rules.is_empty() {
                let rules: Vec<String> = p.rules.iter().map(u8::to_string).collect();
                text.push_str(&format!("\nRun rules broken: {}", rules.join(", ")));
            }
            if let Some(n) = &p.note {
                text.push_str(&format!("\nCumulative zone score: {n}"));
            }
            if p.signal {
                text.push_str("\nZone signal (score ≥ 8)");
            }
            egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), response.response.id.with("chart_point"), |ui| {
                ui.label(text);
            });
        }
    }
    axis_controls(ui, &response, state, ui.make_persistent_id(("chart_axes", id.0, id.1)), &info);
    plot_axes::paint_tick_marks(ui, &response.transform, &axes, categorical);
    size_grip(
        ui,
        &mut state.size,
        size,
        response.response.rect,
        ui.make_persistent_id(("chart_axes_size", id.0, id.1)),
    );
    if axes.show_key {
        reference_key(&mut key, &refs, pal.dark);
        key.show(ui);
        reference_notes(ui, &refs);
    }
    match plot_gesture(ui, &response, state) {
        Some((Gesture::Region(region), mode)) => {
            let hit = chart_rows_in_region(&spec.points, &region, rows);
            rows.select(hit, mode);
        }
        Some((Gesture::Click(pos), mode)) => {
            let screen: Vec<(usize, [f32; 2])> = visible
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let s = response.transform.position_from_point(&PlotPoint::new(p.sample as f64, p.value));
                    (i, [s.x, s.y])
                })
                .collect();
            match nearest_row(screen, [pos.x, pos.y], CLICK_RADIUS) {
                Some(i) => {
                    rows.select(visible[i].rows.iter().copied(), mode);
                }
                None => {
                    brush_click(None, mode, rows);
                }
            }
        }
        None => {}
    }
}

/// A bar chart (histogram or categorical frequencies). Each bar's selected rows
/// are drawn as a highlighted bar of that height inside it; clicking a bar or
/// box-selecting bars selects the rows they count.
fn bar_chart(
    ui: &mut egui::Ui,
    title: &str,
    spec: &BarChartSpec,
    rows: &mut RowStateManager,
    state: &mut PlotState,
    id: (u64, SectionKind),
    reset: bool,
) {
    let pal = theme::plot_palette(ui);
    let info = AxisInfo {
        title,
        x_label: &spec.x_label,
        y_label: &spec.y_label,
        categorical_x: spec.x_ticks.is_some(),
        has_y: true,
    };
    let reset = plot_toolbar(ui, state, &info) || reset;
    let mut axes = state.axes.clone();
    axes.x.time = spec.x_time;
    let size = state.size.full(ui, 240.0);
    plot_title(ui, &axes, title, size.x);
    let start = state.brush_start;
    let refs = std::mem::take(&mut state.reference_marks);
    let (reset, bounds) = prepare_view(state, reset);
    let plot = interactive_plot!(("bars", id.0, id.1), reset);
    let category = &spec.x_ticks.as_ref().map(|t| {
        let key = ui.make_persistent_id(("category_labels", "bars", id.0, id.1));
        plot_axes::CategoryAxis::new(ui, key, t, axes.category_labels)
    });
    let plot = with_x_ticks!(ui, plot, &spec.x_ticks, category.as_ref());
    let plot = with_axes!(plot, &axes, spec.x_ticks.is_some());
    let max_count = spec.bars.iter().map(app_model::BarItem::count).max().unwrap_or(0) as f64;
    let selected: Vec<usize> = spec.bars.iter().map(|b| selected_in_bar(b, rows)).collect();
    let rect = |l: f64, r: f64, h: f64| PlotPoints::from(vec![[l, 0.0], [r, 0.0], [r, h], [l, h]]);
    // Default view: counts start exactly at 0 on the bottom edge.
    let mut extent = Extent::new();
    for bar in &spec.bars {
        extent.add([bar.center - bar.width / 2.0, 0.0]);
        extent.add([bar.center + bar.width / 2.0, bar.count() as f64]);
    }
    if let Some((_, curve)) = &spec.curve {
        // A fitted curve frames the view only where it is visibly above zero,
        // and never below zero when the bars are all at non-negative values.
        let non_negative = spec.bars.iter().all(|b| b.center - b.width / 2.0 >= 0.0);
        let peak = curve.iter().map(|p| p[1]).fold(0.0_f64, f64::max);
        curve.iter().filter(|p| p[1] >= 0.002 * peak && !(non_negative && p[0] < 0.0)).for_each(|&p| extent.add(p));
    }
    for (x, _) in &spec.references {
        extent.add([*x, 0.0]);
    }
    let x_fit = match (spec.x_ticks.is_some(), spec.x_time) {
        (true, _) => AxisFit::Free,
        (false, Some(t)) => AxisFit::Time(t.format),
        (false, None) => AxisFit::Auto,
    };
    // Headroom for the reference-line labels at the top.
    let headroom = if spec.references.is_empty() { 0.0 } else { 0.1 };
    let default_view = extent.view([x_fit, AxisFit::Auto], [0.0, headroom]);
    let mut key = PlotKey::default();
    key.area("Count", pal.bar_fill, pal.point);
    if selected.iter().any(|&n| n > 0) {
        key.area("Selected", pal.selected_fill, pal.selected);
    }
    if let Some((name, _)) = &spec.curve {
        key.line(name.clone(), pal.fit_line, LinePattern::Solid);
    }
    if !spec.references.is_empty() {
        key.line("Reference lines", pal.limit, LinePattern::Dashed);
    }
    let chrome = PlotChrome::begin(ui);
    let response = plot
        .width(size.x)
        .height(size.y + category.as_ref().map_or(0.0, plot_axes::CategoryAxis::extra_height))
        .include_y(0.0)
        .x_axis_label(chrome.axis_title(&axes.x, &spec.x_label))
        .y_axis_label(chrome.axis_title(&axes.y, &spec.y_label))
        .show(ui, |plot| {
            let frame_size = plot.transform().frame().size();
            apply_bounds(plot, plot_axes::view_to_show(bounds, reference_view(default_view, &refs), &axes, frame_size, spec.x_ticks.is_some()));
            stat_viz::render::draw_layer_in(plot, &refs.back, pal.dark);
            for (bar, &sel) in spec.bars.iter().zip(&selected) {
                let (l, r) = (bar.center - bar.width / 2.0, bar.center + bar.width / 2.0);
                if bar.count() > 0 {
                    plot.polygon(
                        Polygon::new(rect(l, r, bar.count() as f64))
                            .fill_color(pal.bar_fill)
                            .stroke(Stroke::new(1.0_f32, pal.point))
                            .name("Count"),
                    );
                }
                if sel > 0 {
                    plot.polygon(
                        Polygon::new(rect(l, r, sel as f64))
                            .fill_color(pal.selected_fill)
                            .stroke(Stroke::new(1.0_f32, pal.selected))
                            .name("Selected"),
                    );
                }
            }
            if let Some((name, curve)) = &spec.curve {
                plot.line(Line::new(PlotPoints::from(curve.clone())).color(pal.fit_line).width(2.0_f32).name(name));
            }
            for (x, _) in &spec.references {
                plot.vline(VLine::new(*x).color(pal.limit).width(1.5_f32).style(LineStyle::dashed_loose()));
            }
            draw_rubber_band(plot, start);
            stat_viz::render::draw_layer_in(plot, &refs.front, pal.dark);
        });

    chrome.finish(ui, &response.transform, &axes, spec.x_ticks.is_some(), None);
    if let (Some(category), Some(ticks)) = (&category, &spec.x_ticks) {
        category.paint(ui, &response.transform, ticks, axes.x.side);
    }
    // Reference-line labels beside their lines near the top (on the side with
    // more room), never across the dashes.
    let frame = *response.transform.frame();
    let mut labels = plot_axes::LabelLayer::new(frame.shrink(2.0));
    let small = egui::TextStyle::Small.resolve(ui.style());
    for (x, label) in &spec.references {
        let lx = response.transform.position_from_point_x(*x);
        if !(frame.left()..=frame.right()).contains(&lx) {
            continue;
        }
        let (at, anchor) = if lx < frame.center().x {
            (egui::pos2(lx + 4.0, frame.top() + 3.0), egui::Align2::LEFT_TOP)
        } else {
            (egui::pos2(lx - 4.0, frame.top() + 3.0), egui::Align2::RIGHT_TOP)
        };
        labels.text(ui, at, anchor, label.as_str(), small.clone(), pal.limit);
    }
    labels.paint(ui);
    axis_controls(ui, &response, state, ui.make_persistent_id(("bar_axes", id.0, id.1)), &info);
    plot_axes::paint_tick_marks(ui, &response.transform, &axes, spec.x_ticks.is_some());
    size_grip(ui, &mut state.size, size, response.response.rect, ui.make_persistent_id(("bar_axes_size", id.0, id.1)));
    if axes.show_key {
        reference_key(&mut key, &refs, pal.dark);
        key.show(ui);
        reference_notes(ui, &refs);
    }
    match plot_gesture(ui, &response, state) {
        Some((Gesture::Region(region), mode)) => {
            rows.select(bar_rows_in_region(&spec.bars, &region), mode);
        }
        Some((Gesture::Click(pos), mode)) => {
            let v = response.transform.value_from_position(pos);
            match bar_at(&spec.bars, v.x, v.y, 0.04 * max_count.max(1.0)) {
                Some(bar) => {
                    rows.select(bar.rows.iter().copied(), mode);
                }
                None => {
                    brush_click(None, mode, rows);
                }
            }
        }
        None => {}
    }
}

/// A Pareto chart: bars ranked largest first (the pooled "Other" bar
/// lighter and last), the cumulative line with square markers labelled with
/// their percentages, and a pane per group. Each bar's selected rows are a
/// highlighted part of it; clicking a bar or box-selecting bars selects
/// their rows.
fn pareto_chart(
    ui: &mut egui::Ui,
    title: &str,
    spec: &app_model::ParetoChartSpec,
    rows: &mut RowStateManager,
    state: &mut PlotState,
    id: (u64, SectionKind),
    reset: bool,
) {
    let pal = theme::plot_palette(ui);
    let info = AxisInfo { title, x_label: &spec.category, y_label: &spec.y_label, categorical_x: true, has_y: true };
    let reset = plot_toolbar(ui, state, &info) || reset;
    let axes = state.axes.clone();
    let size = state.size.full(ui, 280.0);
    plot_title(ui, &axes, title, size.x);
    let start = state.brush_start;
    let refs = std::mem::take(&mut state.reference_marks);
    let (reset, bounds) = prepare_view(state, reset);
    let ticks = Some(spec.ticks());
    let plot = interactive_plot!(("pareto", id.0, id.1), reset);
    let category = &ticks.as_ref().map(|t| {
        let key = ui.make_persistent_id(("category_labels", "pareto", id.0, id.1));
        plot_axes::CategoryAxis::new(ui, key, t, axes.category_labels)
    });
    let plot = with_x_ticks!(ui, plot, &ticks, category.as_ref());
    let plot = with_axes!(plot, &axes, true);
    let panes = spec.panes();
    let top = spec.top();
    let mut extent = Extent::new();
    for (_, _, x) in spec.placed() {
        extent.add([x - 0.5, 0.0]);
        extent.add([x + 0.5, top]);
    }
    let default_view = extent.view([AxisFit::Free, AxisFit::Auto], [0.0, if panes.is_empty() { 0.08 } else { 0.16 }]);
    let half = app_model::pareto_chart::PARETO_BAR_WIDTH / 2.0;
    let rect = |l: f64, r: f64, h: f64| PlotPoints::from(vec![[l, 0.0], [r, 0.0], [r, h], [l, h]]);
    let any_selected = spec.placed().any(|(_, b, _)| b.selected_value(rows) > 0.0);
    let any_other = spec.placed().any(|(_, b, _)| !b.pooled.is_empty());
    let mut key = PlotKey::default();
    key.area(spec.y_label.clone(), pal.bar_fill, pal.point);
    if any_other {
        key.area("Other (pooled categories)", pal.bar_fill.gamma_multiply(0.4), pal.point);
    }
    key.marker("Cumulative %", pal.fit_line, MarkerShape::Square, true);
    if any_selected {
        key.area("Selected", pal.selected_fill, pal.selected);
    }
    let chrome = PlotChrome::begin(ui);
    let response = plot
        .width(size.x)
        .height(size.y + category.as_ref().map_or(0.0, plot_axes::CategoryAxis::extra_height))
        .include_y(0.0)
        .x_axis_label(chrome.axis_title(&axes.x, &spec.category))
        .y_axis_label(chrome.axis_title(&axes.y, &spec.y_label))
        .show(ui, |plot| {
            let frame_size = plot.transform().frame().size();
            apply_bounds(plot, plot_axes::view_to_show(bounds, reference_view(default_view, &refs), &axes, frame_size, true));
            stat_viz::render::draw_layer_in(plot, &refs.back, pal.dark);
            draw_panes(plot, &panes, top * 1.07);
            for (panel, xs) in spec.panels.iter().zip(spec.bar_x()) {
                for (bar, &x) in panel.bars.iter().zip(&xs) {
                    let h = spec.height(bar);
                    let fill = if bar.pooled.is_empty() { pal.bar_fill } else { pal.bar_fill.gamma_multiply(0.4) };
                    let stroke = if bar.pooled.is_empty() {
                        Stroke::new(1.0_f32, pal.point)
                    } else {
                        Stroke::new(1.0_f32, pal.point.gamma_multiply(0.7))
                    };
                    plot.polygon(
                        Polygon::new(rect(x - half, x + half, h)).fill_color(fill).stroke(stroke).name(&bar.label),
                    );
                    let sel = spec.selected_height(bar, rows);
                    if sel > 0.0 {
                        plot.polygon(
                            Polygon::new(rect(x - half, x + half, sel))
                                .fill_color(pal.selected_fill)
                                .stroke(Stroke::new(1.0_f32, pal.selected))
                                .name("Selected"),
                        );
                    }
                }
                let line: Vec<[f64; 2]> =
                    panel.bars.iter().zip(&xs).map(|(b, &x)| [x, spec.cumulative_height(panel, b)]).collect();
                plot.line(
                    Line::new(PlotPoints::from(line.clone())).color(pal.fit_line).width(1.75_f32).name("Cumulative %"),
                );
                plot.points(
                    Points::new(PlotPoints::from(line.clone()))
                        .shape(MarkerShape::Square)
                        .radius(3.5_f32)
                        .color(pal.fit_line)
                        .name("Cumulative %"),
                );
            }
            draw_rubber_band(plot, start);
            stat_viz::render::draw_layer_in(plot, &refs.front, pal.dark);
        });
    chrome.finish(ui, &response.transform, &axes, true, None);
    if let (Some(category), Some(ticks)) = (&category, &ticks) {
        category.paint(ui, &response.transform, ticks, axes.x.side);
    }
    // Cumulative percentages above their markers, left to right; one that
    // would run into the label before it is left out (hovering its bar shows
    // it), so a long flat tail near 100% does not pile up.
    let mut labels = plot_axes::LabelLayer::new(*response.transform.frame());
    let font = egui::TextStyle::Small.resolve(ui.style());
    for (panel, b, x) in spec.placed() {
        let at = response.transform.position_from_point(&PlotPoint::new(x, spec.cumulative_height(panel, b)));
        let pct = app_model::format::format_number(100.0 * b.cumulative, 3);
        let at = at - egui::vec2(0.0, 4.0);
        labels.text_if_free(ui, at, egui::Align2::CENTER_BOTTOM, format!("{pct}%"), font.clone(), pal.fit_line);
    }
    labels.paint(ui);
    // Hovering a bar names it with its count, share and cumulative share.
    if let Some(pos) = response.response.hover_pos().filter(|_| start.is_none()) {
        let v = response.transform.value_from_position(pos);
        if let Some(b) = spec.bar_at(v.x, v.y, 0.0) {
            let mut text = format!(
                "{}\n{}: {}\nShare: {}%\nCumulative: {}%",
                b.label,
                if spec.percent_scale { "Count" } else { spec.y_label.as_str() },
                app_model::format::format_number(b.value, 6),
                app_model::format::format_number(100.0 * b.share, 4),
                app_model::format::format_number(100.0 * b.cumulative, 4)
            );
            if !b.pooled.is_empty() {
                text.push_str(&format!("\nPooled: {}", b.pooled.join(", ")));
            }
            text.push_str(&format!("\n{} rows · click to select them", b.rows.len()));
            egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), response.response.id.with("pareto_bar"), |ui| {
                ui.label(text);
            });
        }
    }
    axis_controls(ui, &response, state, ui.make_persistent_id(("pareto_axes", id.0, id.1)), &info);
    plot_axes::paint_tick_marks(ui, &response.transform, &axes, true);
    size_grip(
        ui,
        &mut state.size,
        size,
        response.response.rect,
        ui.make_persistent_id(("pareto_axes_size", id.0, id.1)),
    );
    if axes.show_key {
        reference_key(&mut key, &refs, pal.dark);
        key.show(ui);
        reference_notes(ui, &refs);
    }
    match plot_gesture(ui, &response, state) {
        Some((Gesture::Region(region), mode)) => {
            rows.select(spec.rows_in_region((region.x_min, region.x_max), (region.y_min, region.y_max)), mode);
        }
        Some((Gesture::Click(pos), mode)) => {
            let v = response.transform.value_from_position(pos);
            match spec.bar_at(v.x, v.y, 0.04 * top.max(1e-12)) {
                Some(bar) => {
                    rows.select(bar.rows.iter().copied(), mode);
                }
                None => {
                    brush_click(None, mode, rows);
                }
            }
        }
        None => {}
    }
}

/// A time–frequency map (spectrogram or scalogram): coloured cells with a
/// colour bar beside the plot; hovering a cell gives its coordinates and
/// value.
fn intensity_map_chart(
    ui: &mut egui::Ui,
    title: &str,
    spec: &app_model::intensity_map::IntensityMapSpec,
    state: &mut PlotState,
    id: (u64, SectionKind),
    reset: bool,
) {
    use app_model::intensity_map::IntensityMapSpec;
    use std::hash::{Hash, Hasher};
    let pal = theme::plot_palette(ui);
    let info = AxisInfo { title, x_label: &spec.x_label, y_label: &spec.y_label, categorical_x: false, has_y: true };
    let reset = plot_toolbar(ui, state, &info) || reset;
    let mut axes = state.axes.clone();
    axes.x.time = None;
    let key = {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        pal.dark.hash(&mut h);
        spec.value_label.hash(&mut h);
        for v in spec.values.iter().flatten().chain(&spec.x).chain(&spec.y) {
            v.to_bits().hash(&mut h);
        }
        h.finish()
    };
    let mesh = match &state.intensity {
        Some((k, mesh)) if *k == key => Arc::clone(mesh),
        _ => {
            let cells = spec.rectangles();
            let colors: Vec<egui::Color32> = (0..app_model::intensity_map::BANDS)
                .map(|k| crate::contour_view::color32(IntensityMapSpec::band_color(k, pal.dark)))
                .collect();
            let mesh = Arc::new(stat_viz::render::FillMesh::new(
                cells.iter().map(|(band, rect)| (std::slice::from_ref(rect), colors[*band])),
            ));
            state.intensity = Some((key, Arc::clone(&mesh)));
            mesh
        }
    };
    let bar = spec.color_bar(pal.dark);
    let size = state.size.capped(ui, 300.0);
    plot_title(ui, &axes, title, size.x);
    let bar_width = crate::contour_view::color_bar_width(ui, &bar).min(size.x / 3.0);
    let refs = std::mem::take(&mut state.reference_marks);
    let (reset, bounds) = prepare_view(state, reset);
    let plot = interactive_plot!(("intensity", id.0, id.1), reset);
    let plot = with_axes!(plot, &axes, false);
    let plot = with_y_ticks!(plot, &spec.y_ticks);
    let ([x0, x1], [y0, y1]) = spec.bounds();
    let default_view = Some(ViewBounds { x_min: x0, x_max: x1, y_min: y0, y_max: y1 });
    let chrome = PlotChrome::begin(ui);
    let response = plot
        .width(size.x - bar_width)
        .height(size.y)
        .x_axis_label(chrome.axis_title(&axes.x, &spec.x_label))
        .y_axis_label(chrome.axis_title(&axes.y, &spec.y_label))
        .show(ui, |plot| {
            let frame_size = plot.transform().frame().size();
            apply_bounds(plot, plot_axes::view_to_show(bounds, reference_view(default_view, &refs), &axes, frame_size, false));
            stat_viz::render::draw_layer_in(plot, &refs.back, pal.dark);
            plot.add(stat_viz::render::fill_item(Arc::clone(&mesh), &spec.value_label));
            stat_viz::render::draw_layer_in(plot, &refs.front, pal.dark);
        });
    let y_spacer: Option<plot_axes::Spacer> = spec.y_ticks.as_ref().map(|ticks| {
        let positions: Vec<f64> = ticks.iter().map(|t| t.0).collect();
        Box::new(move |_: egui_plot::GridInput| {
            positions.iter().map(|&value| GridMark { value, step_size: 1e6 }).collect()
        }) as plot_axes::Spacer
    });
    chrome.finish(ui, &response.transform, &axes, false, y_spacer.as_ref());
    crate::contour_view::paint_color_bar(ui, &bar, *response.transform.frame());
    if let Some(pos) = response.response.hover_pos() {
        let v = response.transform.value_from_position(pos);
        if let Some((i, j)) = spec.cell_at(v.x, v.y) {
            egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), response.response.id.with("intensity_cell"), |ui| {
                ui.label(spec.describe_cell(i, j));
            });
        }
    }
    axis_controls(ui, &response, state, ui.make_persistent_id(("intensity_axes", id.0, id.1)), &info);
    let mut tick_axes = axes.clone();
    if spec.y_ticks.is_some() {
        tick_axes.y.tick_marks = false;
    }
    plot_axes::paint_tick_marks(ui, &response.transform, &tick_axes, false);
    size_grip(ui, &mut state.size, size, response.response.rect, ui.make_persistent_id(("intensity_size", id.0, id.1)));
}

/// Where the last click on an image plot without table rows landed (axis
/// units), for the report that drew it to act on (placing a notch on a
/// spectrum, say). Taken once.
#[must_use]
#[allow(dead_code)] // read by the image analyses (spectrum notches), which build on this
pub fn take_raster_click(ctx: &egui::Context, report: u64, kind: SectionKind) -> Option<[f64; 2]> {
    ctx.data_mut(|d| d.remove_temp::<[f64; 2]>(raster_click_id(report, kind)))
}

fn raster_click_id(report: u64, kind: SectionKind) -> egui::Id {
    egui::Id::new(("raster_click", report, kind))
}

/// The label of tick value `-v` from the label of `v` (a vertical axis drawn
/// in negated units): the minus sign toggled, padding kept.
fn negated_label(text: String) -> String {
    let start = text.len() - text.trim_start().len();
    let (pad, body) = text.split_at(start);
    if let Some(rest) = body.strip_prefix('-') {
        format!("{pad}{rest}")
    } else if body.is_empty() || body.trim_end().chars().all(|c| c == '0' || c == '.' || c == '%') {
        text
    } else {
        format!("{pad}-{body}")
    }
}

/// An image plot: the cells as one texture (sharp-edged when enlarged), a
/// colour bar beside the plot, hover text with the cell's position and
/// value, selected rows' cells tinted, and the labelled marks. A box
/// selection selects the rows of the cells inside it; a click selects one
/// cell's row, or (for cells that are not table rows) is kept for the report
/// ([`take_raster_click`]). Images (`y_down`) draw their vertical axis
/// downwards: the plot works in negated units and labels them back.
fn raster_chart(
    ui: &mut egui::Ui,
    title: &str,
    spec: &stat_viz::RasterSpec,
    rows: &mut RowStateManager,
    state: &mut PlotState,
    id: (u64, SectionKind),
    reset: bool,
) {
    use std::hash::{Hash, Hasher};
    if !spec.is_valid() {
        ui.label("The grid has no cells to draw.");
        return;
    }
    let pal = theme::plot_palette(ui);
    let info = AxisInfo { title, x_label: &spec.x_title, y_label: &spec.y_title, categorical_x: false, has_y: true };
    let reset = plot_toolbar(ui, state, &info) || reset;
    let mut axes = state.axes.clone();
    axes.x.time = None;
    axes.y.time = None;
    // The grid's identity: its shape, appearance and a sample of its values
    // (hashing every cell each frame would cost more than drawing it).
    let grid_key = {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (spec.rows, spec.cols, spec.scale.key(), pal.dark, spec.values.len()).hash(&mut h);
        spec.limits.map(|(a, b)| (a.to_bits(), b.to_bits())).hash(&mut h);
        (spec.values.as_ptr() as usize).hash(&mut h);
        let stride = (spec.values.len() / 4096).max(1);
        for v in spec.values.iter().step_by(stride) {
            v.to_bits().hash(&mut h);
        }
        h.finish()
    };
    let texture = match &state.raster {
        Some((k, t)) if *k == grid_key => t.clone(),
        _ => {
            let b = spec.bitmap(pal.dark);
            let image = egui::ColorImage::from_rgba_unmultiplied([b.width as usize, b.height as usize], &b.rgba);
            let t = ui.ctx().load_texture(format!("raster-{}-{:?}", id.0, id.1), image, egui::TextureOptions::NEAREST);
            state.raster = Some((grid_key, t.clone()));
            t
        }
    };
    let selection_key = {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (grid_key, rows.revision()).hash(&mut h);
        h.finish()
    };
    let selection = match &state.raster_selection {
        Some((k, t)) if *k == selection_key => t.clone(),
        _ => {
            let [r, g, b, _] = pal.selected.to_array();
            let tint = stat_viz::Rgba([r, g, b, 140]);
            let t = spec.selection_bitmap(&|row| row < rows.len() && rows.is_selected(row), tint).map(|bm| {
                let image =
                    egui::ColorImage::from_rgba_unmultiplied([bm.width as usize, bm.height as usize], &bm.rgba);
                ui.ctx().load_texture(format!("raster-sel-{}-{:?}", id.0, id.1), image, egui::TextureOptions::NEAREST)
            });
            state.raster_selection = Some((selection_key, t.clone()));
            t
        }
    };
    let bar = spec.color_bar(pal.dark);
    let size = state.size.capped(ui, 360.0);
    plot_title(ui, &axes, title, size.x);
    let bar_width = crate::contour_view::color_bar_width(ui, &bar).min(size.x / 3.0);
    let start = state.brush_start;
    let refs = std::mem::take(&mut state.reference_marks);
    let (reset, bounds) = prepare_view(state, reset);
    let plot = interactive_plot!(("raster", id.0, id.1), reset);
    let plot = with_axes!(plot, &axes, false);
    let plot = if spec.y_down {
        let (_, formatter) = plot_axes::spacer_and_formatter(&axes.y, true);
        plot.y_axis_formatter(move |mark, range| negated_label(formatter(mark, range)))
    } else {
        plot
    };
    // The cell under the pointer is described below; plain coordinates
    // would show the negated vertical units.
    let plot = plot.show_x(false).show_y(false);
    let ([x0, x1], [y0, y1]) = spec.plot_bounds();
    let default_view = Some(ViewBounds { x_min: x0, x_max: x1, y_min: y0, y_max: y1 });
    let centre = PlotPoint::new((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let extent = egui::vec2((x1 - x0) as f32, (y1 - y0) as f32);
    let chrome = PlotChrome::begin(ui);
    let response = plot
        .width(size.x - bar_width)
        .height(size.y)
        .x_axis_label(chrome.axis_title(&axes.x, &spec.x_title))
        .y_axis_label(chrome.axis_title(&axes.y, &spec.y_title))
        .show(ui, |plot| {
            let frame_size = plot.transform().frame().size();
            apply_bounds(plot, plot_axes::view_to_show(bounds, reference_view(default_view, &refs), &axes, frame_size, false));
            stat_viz::render::draw_layer_in(plot, &refs.back, pal.dark);
            plot.image(PlotImage::new(texture.id(), centre, extent).name(&spec.value_title));
            if let Some(sel) = &selection {
                plot.image(PlotImage::new(sel.id(), centre, extent).name("Selected"));
            }
            for m in &spec.marks {
                let at = [m.at[0], spec.plot_y(m.at[1])];
                plot.points(
                    Points::new(PlotPoints::from(vec![at]))
                        .shape(stat_viz::render::marker(m.shape))
                        .filled(false)
                        .radius(5.0_f32)
                        .color(pal.axis_label)
                        .name(&m.label),
                );
                if !m.label.is_empty() {
                    plot.text(
                        Text::new(PlotPoint::new(at[0], at[1]), format!("   {}", m.label))
                            .color(pal.axis_label)
                            .anchor(egui::Align2::LEFT_CENTER),
                    );
                }
            }
            draw_rubber_band(plot, start);
            stat_viz::render::draw_layer_in(plot, &refs.front, pal.dark);
        });
    chrome.finish(ui, &response.transform, &axes, false, None);
    crate::contour_view::paint_color_bar(ui, &bar, *response.transform.frame());
    if let Some(pos) = response.response.hover_pos() {
        let v = response.transform.value_from_position(pos);
        if let Some((r, c)) = spec.cell_at(v.x, spec.axis_y(v.y)) {
            egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), response.response.id.with("raster_cell"), |ui| {
                ui.label(spec.describe_cell(r, c));
            });
        }
    }
    axis_controls(ui, &response, state, ui.make_persistent_id(("raster_axes", id.0, id.1)), &info);
    plot_axes::paint_tick_marks(ui, &response.transform, &axes, false);
    size_grip(ui, &mut state.size, size, response.response.rect, ui.make_persistent_id(("raster_size", id.0, id.1)));
    match plot_gesture(ui, &response, state) {
        Some((Gesture::Region(region), mode)) => {
            let hit = spec.rows_in(
                [region.x_min, region.x_max],
                [spec.axis_y(region.y_min), spec.axis_y(region.y_max)],
            );
            if spec.row_index.is_some() {
                let hit: Vec<usize> = hit.into_iter().filter(|&r| r < rows.len() && !rows.is_hidden(r)).collect();
                rows.select(hit, mode);
            }
        }
        Some((Gesture::Click(pos), mode)) => {
            let v = response.transform.value_from_position(pos);
            let at = [v.x, spec.axis_y(v.y)];
            match spec.cell_at(at[0], at[1]) {
                Some((r, c)) if spec.row_index.is_some() => {
                    brush_click(spec.table_row(r, c).filter(|&row| row < rows.len()), mode, rows);
                }
                _ => ui.ctx().data_mut(|d| d.insert_temp(raster_click_id(id.0, id.1), at)),
            }
        }
        None => {}
    }
}

/// A horizontal box plot: box from Q1 to Q3, median line, whiskers with caps,
/// the mean as a diamond, values beyond the whiskers as points, and selected
/// rows ringed. A box selection selects every value in its horizontal range.
fn box_plot(
    ui: &mut egui::Ui,
    title: &str,
    spec: &BoxPlotSpec,
    rows: &mut RowStateManager,
    state: &mut PlotState,
    id: (u64, SectionKind),
    reset: bool,
) {
    let pal = theme::plot_palette(ui);
    let info = AxisInfo { title, x_label: &spec.x_label, y_label: "", categorical_x: false, has_y: false };
    let reset = plot_toolbar(ui, state, &info) || reset;
    let mut axes = state.axes.clone();
    axes.x.time = spec.x_time;
    const HALF: f64 = 0.3;
    const CAP: f64 = 0.15;
    let size = state.size.full(ui, 130.0);
    plot_title(ui, &axes, title, size.x);
    let start = state.brush_start;
    let refs = std::mem::take(&mut state.reference_marks);
    let (reset, bounds) = prepare_view(state, reset);
    let plot = interactive_plot!(("box", id.0, id.1), reset);
    let plot = with_axes!(plot, &axes, false);
    // Default view: the whiskers, mean and every value, on the fixed band
    // the box sits in.
    let mut extent = Extent::new();
    for x in [spec.whisker_low, spec.whisker_high, spec.mean, spec.q1, spec.q3] {
        extent.add([x, 0.0]);
    }
    spec.points.iter().filter(|p| !rows.is_hidden(p.row)).for_each(|p| extent.add([p.x, 0.0]));
    let x_fit = spec.x_time.map_or(AxisFit::Auto, |t| AxisFit::Time(t.format));
    let default_view =
        extent.view([x_fit, AxisFit::Auto], [0.0, 0.0]).map(|v| ViewBounds { y_min: -1.0, y_max: 1.0, ..v });
    let visible = |p: &&app_model::RowPoint| !rows.is_hidden(p.row);
    let outliers: Vec<[f64; 2]> =
        spec.outliers.iter().map(|&i| &spec.points[i]).filter(visible).map(|p| [p.x, 0.0]).collect();
    let selected: Vec<[f64; 2]> =
        spec.points.iter().filter(visible).filter(|p| rows.is_selected(p.row)).map(|p| [p.x, 0.0]).collect();
    // Above the point budget the values are drawn as one picture.
    let drawn = outliers.len() + selected.len();
    let rastered = drawn > stat_viz::point_raster::POINT_BUDGET;
    let revision = rows.revision();
    let mut key = PlotKey::default();
    key.area("Quartiles (box)", pal.bar_fill, pal.point);
    key.line("Median", pal.point, LinePattern::Solid);
    key.marker("Mean", pal.fit_line, MarkerShape::Diamond, true);
    if !outliers.is_empty() {
        key.marker("Beyond 1.5 IQR", pal.point, MarkerShape::Circle, true);
    }
    if !selected.is_empty() {
        key.marker("Selected", pal.selected, MarkerShape::Circle, false);
    }
    let chrome = PlotChrome::begin(ui);
    let response = plot
        .width(size.x)
        .height(size.y)
        .include_y(-1.0)
        .include_y(1.0)
        .show_axes([true, false])
        .x_axis_label(chrome.axis_title(&axes.x, &spec.x_label))
        .show(ui, |plot| {
            let frame_size = plot.transform().frame().size();
            apply_bounds(plot, plot_axes::view_to_show(bounds, reference_view(default_view, &refs), &axes, frame_size, false));
            stat_viz::render::draw_layer_in(plot, &refs.back, pal.dark);
            plot.polygon(
                Polygon::new(PlotPoints::from(vec![
                    [spec.q1, -HALF],
                    [spec.q3, -HALF],
                    [spec.q3, HALF],
                    [spec.q1, HALF],
                ]))
                .fill_color(pal.bar_fill)
                .stroke(Stroke::new(1.5_f32, pal.point))
                .name("Quartiles"),
            );
            plot.line(
                Line::new(PlotPoints::from(vec![[spec.median, -HALF], [spec.median, HALF]]))
                    .color(pal.point)
                    .width(2.5_f32)
                    .name("Median"),
            );
            for segment in [
                vec![[spec.whisker_low, 0.0], [spec.q1, 0.0]],
                vec![[spec.q3, 0.0], [spec.whisker_high, 0.0]],
                vec![[spec.whisker_low, -CAP], [spec.whisker_low, CAP]],
                vec![[spec.whisker_high, -CAP], [spec.whisker_high, CAP]],
            ] {
                plot.line(Line::new(PlotPoints::from(segment)).color(pal.point).width(1.5_f32));
            }
            plot.points(
                Points::new(PlotPoints::from(vec![[spec.mean, 0.0]]))
                    .shape(MarkerShape::Diamond)
                    .radius(5.0_f32)
                    .color(pal.fit_line)
                    .name("Mean"),
            );
            if rastered {
                use stat_viz::render::{RasterPoints, raster_splat};
                let splat = raster_splat(3.5);
                let layers = vec![
                    RasterPoints { points: outliers, color: pal.point, density: true, splat },
                    RasterPoints { points: selected, color: pal.selected, density: false, splat: splat + 1.5 },
                ];
                plot.add(stat_viz::render::PointRaster::new(layers, revision ^ u64::from(pal.dark) << 63));
            } else {
                if !outliers.is_empty() {
                    plot.points(
                        Points::new(PlotPoints::from(outliers)).radius(3.5_f32).color(pal.point).name("Beyond 1.5 IQR"),
                    );
                }
                if !selected.is_empty() {
                    plot.points(
                        Points::new(PlotPoints::from(selected))
                            .shape(MarkerShape::Circle)
                            .filled(false)
                            .radius(6.0_f32)
                            .color(pal.selected)
                            .name("Selected"),
                    );
                }
            }
            draw_rubber_band(plot, start);
            stat_viz::render::draw_layer_in(plot, &refs.front, pal.dark);
        });

    chrome.without_vertical_axis().finish(ui, &response.transform, &axes, false, None);
    axis_controls(ui, &response, state, ui.make_persistent_id(("box_axes", id.0, id.1)), &info);
    plot_axes::paint_tick_marks(ui, &response.transform, &axes, false);
    size_grip(ui, &mut state.size, size, response.response.rect, ui.make_persistent_id(("box_axes_size", id.0, id.1)));
    if axes.show_key {
        reference_key(&mut key, &refs, pal.dark);
        key.show(ui);
        reference_notes(ui, &refs);
    }
    if rastered {
        ui.label(RichText::new(dense_points_note(drawn)).weak().small());
    }
    match plot_gesture(ui, &response, state) {
        Some((Gesture::Region(region), mode)) => {
            let hit = box_rows_in_region(spec, &region, rows);
            rows.select(hit, mode);
        }
        Some((Gesture::Click(pos), mode)) => {
            let screen: Vec<(usize, [f32; 2])> = spec
                .points
                .iter()
                .filter(|p| !rows.is_hidden(p.row))
                .map(|p| {
                    let s = response.transform.position_from_point(&PlotPoint::new(p.x, 0.0));
                    (p.row, [s.x, s.y])
                })
                .collect();
            brush_click(nearest_row(screen, [pos.x, pos.y], CLICK_RADIUS), mode, rows);
        }
        None => {}
    }
}

#[cfg(test)]
mod table_tests {
    /// Sixteen numeric levels on a main-effects axis drew their labels on
    /// top of each other; crowded category labels are thinned.
    #[test]
    fn crowded_category_labels_are_thinned() {
        let positions: Vec<f64> = (1..=16).map(f64::from).collect();
        // 400 points across 16 units: 25 points a level; 50-point labels need 60.
        let base = 8.0 * 16.0 / 400.0;
        assert_eq!(super::category_label_step(&positions, 50.0, base), 3);
        // Plenty of room: every label.
        assert_eq!(super::category_label_step(&positions, 12.0, 8.0 * 16.0 / 1600.0), 1);
        assert_eq!(super::category_label_step(&[1.0], 200.0, base), 1);
    }

    #[test]
    fn numeric_cells() {
        for n in
            ["500.61", "−2.5", "4.3e-6", "< 0.0001", "41.85", "12%", "—", "1,234", "0.0601922", "7.25 ▲", "-2.1 ▼", "∞"]
        {
            assert!(super::is_numeric_cell(n), "{n}");
        }
        for t in ["Normal (identity link)", "REML", "Fertilizer: B", "", "e", "Batch", "2 levels"] {
            assert!(!super::is_numeric_cell(t), "{t}");
        }
    }
}

#[cfg(test)]
mod several_responses_tests {
    use super::*;

    /// A report whose analysis panicked shows the error in place of its
    /// sections, keeps its request (so it saves as it was), and computes
    /// again once the data change.
    #[test]
    fn a_report_whose_analysis_panicked_shows_the_error_and_recovers() {
        let table = DataTable::new("t", vec![stat_data::Column::numeric("y", vec![1.0, 2.0, 4.0, 8.0])]).unwrap();
        let rows = RowStateManager::new(4);
        let request = SavedReport::Distribution { request: app_model::DistributionRequest::new(0), options: Default::default() };
        let mut report = Report::new(1, request, &table, &rows);
        let saved = report.saved();
        let message = crate::app::crash::guarded(|| panic!("index 9 out of range")).unwrap_err();
        report.body = ReportBody::Failed { saved: report.body.saved(), message };
        let err = report.shown_sections().unwrap_err();
        assert!(err.contains("internal error") && err.contains("index 9 out of range"), "{err}");
        assert!(report.title().contains("internal error"));
        assert_eq!(report.saved(), saved);
        // Nothing changed: it stays failed.
        report.refresh_if_stale(&table, &rows);
        assert!(matches!(report.body, ReportBody::Failed { .. }));
        let mut edited = table.clone();
        edited.set_value(0, 0, stat_data::CellValue::Number(1.5)).unwrap();
        report.refresh_if_stale(&edited, &rows);
        assert!(matches!(report.body, ReportBody::Distribution { analysis: Ok(_), .. }));
        assert!(report.shown_sections().is_ok());
    }

    /// The coating sample's several-responses report draws headlessly, keeps
    /// its controls anchored, and a best-settings search and the simulation
    /// show up as sections.
    #[test]
    fn several_responses_report_draws() {
        let info = app_model::sample::catalog().into_iter().find(|s| s.name.starts_with("Coating")).unwrap();
        let sample = (info.build)();
        let mut rows = RowStateManager::new(sample.table.n_rows());
        let saved = sample.reports.into_iter().find(|r| matches!(r, SavedReport::MultipleResponses { .. })).unwrap();
        let mut report = Report::new(1, saved, &sample.table, &rows);
        let ctx = egui::Context::default();
        let frame = |report: &mut Report, rows: &mut RowStateManager| {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| show_report(ui, report, &sample.table, rows));
            });
        };
        frame(&mut report, &mut rows);
        let kinds = |r: &Report| r.body.sections().unwrap().iter().map(|s| s.kind).collect::<Vec<_>>();
        let k = kinds(&report);
        assert_eq!(&k[..3], &[SectionKind::ResponseFits, SectionKind::BestSettings, SectionKind::SimulationSummary]);
        assert!(k.contains(&SectionKind::SimulatedResponse(2)));
        let ReportBody::MultipleResponses { analysis: Ok(a), view, request, .. } = &mut report.body else { panic!() };
        // What "Find best settings" does, then "Set to #1".
        let best = a.best_settings().unwrap();
        view.set_best(Ok(best.clone()));
        assert!(a.set_settings(&best[0].settings));
        request.settings = a.request().settings.clone();
        frame(&mut report, &mut rows);
        let sections = report.body.sections().unwrap();
        assert!(matches!(sections[1].content, SectionContent::Table(_)), "best settings table");
        // Moving the settings keeps the models (no refit) and re-runs the simulation.
        assert!(!report.body.settings_changed());
        let saved = report.saved();
        let SavedReport::MultipleResponses { request } = saved else { panic!() };
        assert_eq!(request.settings.len(), 3);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_are_grouped_in_plain_numbers_only() {
        assert_eq!(group_thousands("184150146"), "184,150,146");
        assert_eq!(group_thousands("-3598729"), "-3,598,729");
        assert_eq!(group_thousands("−12345.678"), "−12,345.678");
        assert_eq!(group_thousands("46037536 ▲"), "46,037,536 ▲");
        // Four digits (years, counts) and short numbers stay as they are.
        assert_eq!(group_thousands("1947"), "1947");
        assert_eq!(group_thousands("0.995359"), "0.995359");
        // Exponents, existing separators, words and dashes are left alone.
        assert_eq!(group_thousands("1.2e+12"), "1.2e+12");
        assert_eq!(group_thousands("12,345"), "12,345");
        assert_eq!(group_thousands("—"), "—");
        assert_eq!(group_thousands("< 0.0001"), "< 0.0001");
    }

    #[test]
    fn fraction_length_counts_from_the_decimal_point() {
        assert_eq!(fraction_chars("1887.41"), 3);
        assert_eq!(fraction_chars("-3598729"), 0);
        assert_eq!(fraction_chars("★ 1.7e-5"), 5);
        assert_eq!(fraction_chars("12.5%"), 3);
    }

    #[test]
    fn exported_figure_keeps_the_plot_frame_proportions() {
        // A 300 × 200 frame inside a 344 × 228 widget (the axis strips around it).
        let widget = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(344.0, 228.0));
        let state = PlotState {
            last_frame: Some(egui::Rect::from_min_size(egui::pos2(54.0, 10.0), egui::vec2(300.0, 200.0))),
            ..PlotState::default()
        };
        let around = FigureLayout::matching_screen(0.0, 0.0, None);
        let layout = state.screen_layout(widget);
        let ratio = (layout.width - around.width) / (layout.height - around.height);
        assert!((ratio - 1.5).abs() < 1e-6, "{ratio}");
        // Before a frame is drawn, the widget stands in.
        let fresh = PlotState::default().screen_layout(widget);
        assert!(((fresh.width - around.width) / (fresh.height - around.height) - 344.0 / 228.0).abs() < 1e-6);
    }

    #[test]
    fn identity_and_score_plots_open_at_equal_scale() {
        let identity = PlotSpec { line_name: Some("Reference line y = x".to_owned()), ..PlotSpec::default() };
        assert!(PlotState::for_plot(&identity, "Observed vs Predicted").axes.equal_scale);
        assert!(PlotState::for_plot(&PlotSpec::default(), "Score Plot (PC1 60%, PC2 20%)").axes.equal_scale);
        let fit = PlotSpec { line_name: Some("Fit".to_owned()), ..PlotSpec::default() };
        assert!(!PlotState::for_plot(&fit, "Residuals vs Predicted").axes.equal_scale);
    }
}

#[cfg(test)]
mod section_cache_tests {
    use super::*;
    use stat_data::Column;

    fn table() -> DataTable {
        let x: Vec<f64> = (0..200).map(|i| f64::from(i).sin() * 10.0 + 50.0).collect();
        DataTable::new("t", vec![Column::numeric("x", x)]).unwrap()
    }

    fn distribution(id: u64, table: &DataTable, rows: &RowStateManager) -> Report {
        Report::new(id, ReportRequest::Distribution(app_model::DistributionRequest::new(0)), table, rows)
    }

    /// Sections are built once and reused until the analysis is computed
    /// again or a display option changes.
    #[test]
    fn sections_are_reused_until_the_report_changes() {
        let table = table();
        let mut rows = RowStateManager::new(table.n_rows());
        let mut report = distribution(1, &table, &rows);
        let first = report.cached_sections();
        assert!(Arc::ptr_eq(&first, &report.cached_sections()), "the same sections on the next frame");
        assert_eq!(*first, report.shown_sections());

        let ReportBody::Distribution { options, .. } = &mut report.body else { panic!() };
        options.show_quantiles = false;
        let fewer = report.cached_sections();
        assert!(!Arc::ptr_eq(&first, &fewer), "an option change rebuilds them");
        assert_eq!(*fewer, report.shown_sections());
        assert!(fewer.as_ref().as_ref().unwrap().len() < first.as_ref().as_ref().unwrap().len());

        rows.set_excluded([0, 1, 2], true);
        report.refresh_if_stale(&table, &rows);
        let refit = report.cached_sections();
        assert!(!Arc::ptr_eq(&fewer, &refit), "a new computation rebuilds them");
        assert_eq!(*refit, report.shown_sections());
    }

    /// Reports far below the visible part of the pane keep their place
    /// without being drawn; jumping to one draws it.
    #[test]
    fn reports_out_of_view_are_not_drawn() {
        let table = table();
        let mut rows = RowStateManager::new(table.n_rows());
        let mut reports: Vec<Report> = (0..6).map(|i| distribution(i, &table, &rows)).collect();
        let ctx = egui::Context::default();
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0))),
            ..egui::RawInput::default()
        };
        let frame = |reports: &mut Vec<Report>, rows: &mut RowStateManager| {
            let _ = ctx.run(input(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let _ = show_reports(ui, reports, &table, rows);
                });
            });
        };
        frame(&mut reports, &mut rows);
        assert!(reports.iter().all(|r| r.drawn_size.is_some()), "every report is drawn once to learn its size");
        let marker = egui::vec2(1.0, 4000.0);
        for r in &mut reports {
            r.drawn_size = Some(marker);
        }
        frame(&mut reports, &mut rows);
        assert_ne!(reports[0].drawn_size, Some(marker), "the first report is on screen and drawn");
        assert_eq!(reports[5].drawn_size, Some(marker), "the last report is far below and not drawn");
        jump_to_report(&ctx, &mut reports[5]);
        frame(&mut reports, &mut rows);
        assert_ne!(reports[5].drawn_size, Some(marker), "a report jumped to is drawn");
    }
}

#[cfg(test)]
mod background_tests {
    use super::*;
    use std::time::Duration;

    /// Wait for every job and install those for `report`.
    fn finish(jobs: &mut ReportJobs, report: &mut Report, table: &DataTable, rows: &RowStateManager) -> Vec<bool> {
        let mut installed = Vec::new();
        while jobs.busy() {
            for done in jobs.take(Some(Duration::from_secs(60))) {
                installed.push(report.install(done, table, rows));
            }
        }
        installed
    }

    /// Every sample report computed in the background, after some rows were
    /// excluded, shows exactly what computing it in the frame shows.
    #[test]
    fn background_results_match_results_computed_in_the_frame() {
        let ctx = egui::Context::default();
        let mut jobs = ReportJobs::new(&ctx);
        let mut checked = 0;
        for info in app_model::sample::catalog() {
            let sample = (info.build)();
            let table = Arc::new(sample.table);
            let n = table.n_rows();
            let rows = RowStateManager::new(n);
            // Exclude every fifth row (a report may refuse: an error is a result too).
            let mut changed = rows.clone();
            changed.set_excluded((0..n).filter(|r| r % 5 == 1), true);
            for (k, saved) in sample.reports.into_iter().enumerate() {
                let mut background = Report::new(k as u64, saved.clone(), &table, &rows);
                if matches!(background.body, ReportBody::Graph { .. }) {
                    continue;
                }
                let mut in_frame = Report::new(k as u64, saved, &table, &rows);
                assert!(background.wants_refresh(&table, &changed), "{}", info.name);
                background.queue_refresh(&mut jobs, Arc::clone(&table), Arc::new(changed.clone()));
                assert_eq!(finish(&mut jobs, &mut background, &table, &changed), vec![true], "{}", info.name);
                in_frame.refresh_if_stale(&table, &changed);
                // Compared as text: some sections hold NaN, which is never equal to itself.
                assert_eq!(
                    format!("{:?}", background.cached_sections()),
                    format!("{:?}", Arc::new(in_frame.shown_sections())),
                    "{}: {}",
                    info.name,
                    in_frame.title()
                );
                assert_eq!(background.computed_at, in_frame.computed_at);
                assert!(!background.wants_refresh(&table, &changed));
                checked += 1;
            }
        }
        assert!(checked > 100, "only {checked} sample reports checked");
    }

    /// Changes made while a report computes are not lost: the result for
    /// the old state is dropped and the report asks again; while it runs it
    /// does not ask for a second job.
    #[test]
    fn a_result_for_an_older_state_is_dropped() {
        let ctx = egui::Context::default();
        let mut jobs = ReportJobs::new(&ctx);
        let x: Vec<f64> = (0..300).map(|i| f64::from(i % 17)).collect();
        let table = Arc::new(DataTable::new("t", vec![stat_data::Column::numeric("x", x)]).unwrap());
        let mut rows = RowStateManager::new(table.n_rows());
        let mut report =
            Report::new(1, ReportRequest::Distribution(app_model::DistributionRequest::new(0)), &table, &rows);
        rows.set_excluded([0, 1], true);
        assert!(report.wants_refresh(&table, &rows));
        report.queue_refresh(&mut jobs, Arc::clone(&table), Arc::new(rows.clone()));
        assert!(report.is_updating());
        rows.set_excluded([2, 3], true);
        assert!(!report.wants_refresh(&table, &rows), "one job at a time");
        assert_eq!(finish(&mut jobs, &mut report, &table, &rows), vec![false], "the older result is dropped");
        assert!(!report.is_updating());
        assert!(report.wants_refresh(&table, &rows), "and the report asks again");
        report.queue_refresh(&mut jobs, Arc::clone(&table), Arc::new(rows.clone()));
        assert_eq!(finish(&mut jobs, &mut report, &table, &rows), vec![true]);
        assert!(!report.wants_refresh(&table, &rows));
        let mut fresh =
            Report::new(2, ReportRequest::Distribution(app_model::DistributionRequest::new(0)), &table, &rows);
        assert_eq!(*report.cached_sections(), *fresh.cached_sections());
    }

    /// A panic inside an analysis becomes the report's error note; the
    /// previous result stays.
    #[test]
    fn a_panicking_job_keeps_the_previous_result() {
        let ctx = egui::Context::default();
        let mut jobs = ReportJobs::new(&ctx);
        let x: Vec<f64> = (0..50).map(f64::from).collect();
        let table = Arc::new(DataTable::new("t", vec![stat_data::Column::numeric("x", x)]).unwrap());
        let rows = RowStateManager::new(table.n_rows());
        let mut report =
            Report::new(1, ReportRequest::Distribution(app_model::DistributionRequest::new(0)), &table, &rows);
        let before = report.cached_sections();
        // Row states shorter than the table make the analysis index past their end.
        let mut short = RowStateManager::new(10);
        short.set_excluded([0], true);
        report.queue_refresh(&mut jobs, Arc::clone(&table), Arc::new(short.clone()));
        assert_eq!(finish(&mut jobs, &mut report, &table, &short), vec![true]);
        assert!(report.job_error.as_deref().is_some_and(|m| m.contains("internal error")), "{:?}", report.job_error);
        assert_eq!(*report.cached_sections(), *before);
        assert!(!report.wants_refresh(&table, &short), "a failed job is not retried until something changes");
    }

    /// A new report whose first computation panics shows the error in place
    /// of its sections, as a report computed in the frame does.
    #[test]
    fn a_new_report_whose_job_panics_shows_the_error() {
        let ctx = egui::Context::default();
        let mut jobs = ReportJobs::new(&ctx);
        let x: Vec<f64> = (0..50).map(f64::from).collect();
        let table = Arc::new(DataTable::new("t", vec![stat_data::Column::numeric("x", x)]).unwrap());
        let mut short = RowStateManager::new(10);
        short.set_excluded([0], true);
        let mut report = Report::new_waiting(1, ReportRequest::Distribution(app_model::DistributionRequest::new(0)));
        assert!(report.wants_refresh(&table, &short));
        report.queue_refresh(&mut jobs, Arc::clone(&table), Arc::new(short.clone()));
        assert_eq!(finish(&mut jobs, &mut report, &table, &short), vec![true]);
        assert!(matches!(report.body, ReportBody::Failed { .. }));
        assert!(report.job_error.is_none());
        let shown = report.cached_sections();
        assert!(shown.as_ref().as_ref().is_err_and(|m| m.contains("internal error")), "{shown:?}");
    }
}
