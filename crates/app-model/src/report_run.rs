//! Run a saved report request: the one place that turns a [`SavedReport`]
//! into its title and sections, shared by the desktop application, project
//! reloading, scripts (`stat-script`) and the headless runner.
//!
//! A report request is what a dialog produces and what a project file stores
//! (see [`crate::project`] for the JSON form). Running it is deterministic, so
//! the same request on the same data and row states gives identical sections
//! whether it came from a dialog, a project file or a script.

use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_viz::export::{Figure, GridRules, RightView, VectorFormat, export_graph_full, panel_views};
use stat_viz::{GraphForm, GraphSpec, SharedRanges};

use crate::project::SavedReport;
use crate::{
    AttributeChartAnalysis, BivariateAnalysis, CapabilityAnalysis, ClusterAnalysis, ComparisonReport,
    ControlChartAnalysis, CorrespondenceReport, DistributionAnalysis, GaugeAnalysis, GlmAnalysis, LinearModelAnalysis,
    MixedAnalysis, MixtureAnalysis, PcaAnalysis, RegularizedAnalysis, ReportSection, StepwiseAnalysis, SurfaceAnalysis,
    SurvivalAnalysis, TimeWeightedAnalysis,
};

/// A report request together with what running it produced.
#[derive(Debug, Clone, PartialEq)]
pub struct ComputedReport {
    /// The request (analysis, columns, settings and display options).
    pub saved: SavedReport,
    /// Report heading, e.g. `"Linear Model: y"`.
    pub title: String,
    /// Result sections, in display order. A graph canvas has none: it is
    /// drawn from the data (see [`graph_figure`]).
    pub sections: Vec<ReportSection>,
}

/// Run `saved` on `table` with row states `rows`.
///
/// The sections are exactly those the desktop report shows for the same
/// request and display options.
///
/// # Errors
/// Whatever the analysis reports (for example too few rows, a column of the
/// wrong type, or a singular model), as [`StatError`].
pub fn run_report(saved: &SavedReport, table: &DataTable, rows: &RowStateManager) -> Result<ComputedReport> {
    let (title, sections) = match saved {
        SavedReport::Scoped { scope, report } => {
            let pooled = scope.pooled_sections(report, table, rows);
            return match scope.apply(table, rows) {
                Some(filtered) => run_report(report, table, &filtered),
                None => run_report(report, table, rows),
            }
            .map(|mut computed| {
                if !pooled.is_empty() {
                    computed.sections.splice(0..0, pooled);
                }
                scope.style_contours(&mut computed.sections);
                ComputedReport { saved: saved.clone(), ..computed }
            });
        }
        SavedReport::Linear { spec, options } => {
            let a = LinearModelAnalysis::run(table, rows, spec)?;
            (a.title(), a.sections(options))
        }
        SavedReport::ControlChart { request, options } => {
            let a = ControlChartAnalysis::run(table, rows, request)?;
            (a.title(), a.sections(options))
        }
        SavedReport::Gauge { request, options } => {
            let a = GaugeAnalysis::run(table, rows, request)?;
            (a.title(), a.sections(options))
        }
        SavedReport::Distribution { request, options } => {
            let a = DistributionAnalysis::run(table, rows, request)?;
            (a.title(), a.sections(options))
        }
        SavedReport::Bivariate { request } => {
            let a = BivariateAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::Pca { request } => {
            let a = PcaAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::Cluster { request } => {
            let a = ClusterAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::Glm { request, .. } => {
            let a = GlmAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::Mixed { request, .. } => {
            let a = MixedAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::Regularized { request, .. } => {
            let a = RegularizedAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::MultipleResponses { request } => {
            let a = crate::multiple_responses::MultipleResponsesAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::DecisionTree { request, .. } => {
            let a = crate::TreeAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::RandomForest { request, .. } => {
            let a = crate::ForestAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::BoostedTrees { request, .. } => {
            let a = crate::BoostAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::PartialLeastSquares { request, .. } => {
            let a = crate::PlsAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::SupportVectorMachine { request, .. } => {
            let a = crate::SvmAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::FunctionalData { request, .. } => {
            let a = crate::FunctionalAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::Mars { request, .. } => {
            let a = crate::MarsAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::NeuralNetwork { request, .. } => {
            let a = crate::NeuralAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::GaussianProcess { request, .. } => {
            let a = crate::GaussianProcessAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::SequentialOptimization { request } => {
            let a = crate::SequentialAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::ModelSearch { request } => {
            let a = crate::ModelSearch::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::ModelComparison { request } => {
            let a = crate::ModelComparison::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::Correspondence { request } => {
            let a = CorrespondenceReport::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::Correlations { request } => {
            let a = crate::CorrelationAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::OutlierDistances { request } => {
            let a = crate::OutlierAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::ItemAnalysis { request } => {
            let a = crate::ItemAnalysisReport::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::Manova { request } => {
            let a = crate::ManovaAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::FactorAnalysis { request } => {
            let a = crate::FactorAnalysisReport::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::DiscriminantAnalysis { request } => {
            let a = crate::DiscriminantAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::GaussianMixture { request } => {
            let a = crate::GaussianMixtureAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::LatentClass { request } => {
            let a = crate::LatentClassAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::TextExploration { request } => {
            let a = crate::TextExploration::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::TopicModel { request } => {
            let a = crate::TopicModelAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::Sentiment { request } => {
            let a = crate::SentimentAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::Embedding { request } => {
            let a = crate::EmbeddingAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::Imputation { request } => {
            let a = crate::ImputationReport::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::SeriesExplore { request } => {
            let a = crate::SeriesExploreAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::SignalProcessing { request } => {
            let a = crate::SignalAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::ImageAnalysis { request } => {
            let a = crate::ImageAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::Survival { request, options } => {
            let a = SurvivalAnalysis::run(table, rows, request)?;
            (a.title(), a.sections(options))
        }
        SavedReport::Growth { request } => {
            let a = crate::GrowthAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::Recurrence { request } => {
            let a = crate::RecurrenceAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::LifeDistribution { request } => {
            let a = crate::LifeDistributionAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::CompetingRisks { request } => {
            let a = crate::CompetingRisksAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::Warranty { request } => {
            let a = crate::WarrantyAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::ExponentialSmoothing { request } => {
            let a = crate::SmoothingAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::Arima { request } => {
            let a = crate::ArimaAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::PrincipalCoordinates { request } => {
            let a = crate::PcoaAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::Pharmacokinetics { request } => {
            let a = crate::PkAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::Fatigue { request } => {
            let a = crate::FatigueAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::LifeStress { request } => {
            let a = crate::LifeStressAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::Degradation { request } => {
            let a = crate::DegradationAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::ProportionalHazards { request, .. } => {
            let a = crate::ProportionalHazardsAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::SurvivalRegression { request, .. } => {
            let a = crate::SurvivalRegressionAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::TestPlan { request } => {
            let a = crate::TestPlanAnalysis::run(request)?;
            (a.title(), a.sections())
        }
        SavedReport::Surface { request, options } => {
            let a = SurfaceAnalysis::run(table, rows, request)?;
            (a.title(), a.sections(options))
        }
        SavedReport::TimeWeighted { request } => {
            let a = TimeWeightedAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::Capability { request } => {
            let a = CapabilityAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::Attribute { request } => {
            let a = AttributeChartAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::Pareto { request, options } => {
            let a = crate::ParetoAnalysis::run(table, rows, request)?;
            (a.title(), a.sections(options))
        }
        SavedReport::CauseEffect { request } => {
            let a = crate::CauseEffectAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::MultiVari { request, options } => {
            let a = crate::MultiVariAnalysis::run(table, rows, request)?;
            (a.title(), a.sections(options))
        }
        SavedReport::MultivariateChart { request } => {
            let a = crate::MultivariateChartAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::Anom { request } => {
            let a = crate::AnomAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::DoseResponse { request } => {
            let a = crate::DoseResponseAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::CategoricalLogistic { request } => {
            let a = crate::CategoricalLogisticAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::Nonlinear { request, .. } => {
            let a = crate::NonlinearAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::MethodComparison { request } => {
            let a = crate::MethodComparisonAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::ShelfLife { request } => {
            let a = crate::ShelfLifeAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::Stepwise { request, .. } => {
            let a = StepwiseAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::BestSubsets { request } => {
            let a = crate::BestSubsetsAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::DistributionFit { request } => {
            let a = crate::DistributionFitAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::Mixture { request, .. } => {
            let a = MixtureAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::Bootstrap { request } => {
            let a = crate::BootstrapAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::PermutationTest { request } => {
            let a = crate::PermutationAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::OneSample { .. }
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
        | SavedReport::AttributeGauge { .. }
        | SavedReport::PcaChart { .. }
        | SavedReport::MonitoringLimits { .. }
        | SavedReport::MultiwayTable { .. }
        | SavedReport::StratifiedTables { .. }
        | SavedReport::SignalToNoise { .. }
        | SavedReport::DeclaredDesign { .. }
        | SavedReport::DesignEvaluation { .. }
        | SavedReport::FactorialAnalysis { .. }
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
        | SavedReport::Bayesian { .. }
        | SavedReport::MultipleResponseItems { .. }
        | SavedReport::Signal { .. }
        | SavedReport::ImagePlot { .. } => {
            let a = ComparisonReport::run(saved, table, rows).expect("a comparison report")?;
            (a.title(), a.sections())
        }
        SavedReport::Scatter3d { request, .. } => {
            let a = crate::Scatter3dAnalysis::run(table, rows, request)?;
            (a.title(), a.sections())
        }
        SavedReport::DataSurface { request, options } => {
            let a = crate::DataSurfaceAnalysis::run(table, rows, request)?;
            (a.title(), a.sections(options))
        }
        SavedReport::Graph { spec } => {
            // Building the frame checks the columns and row states.
            spec.frame(table, rows)?;
            (graph_title(spec), Vec::new())
        }
    };
    Ok(ComputedReport { saved: saved.clone(), title, sections })
}

/// The identifier of a report's analysis in project files and scripts, e.g.
/// `"linear-model"` (the `"analysis"` member of its JSON).
#[must_use]
pub fn analysis_key(saved: &SavedReport, table: &DataTable) -> String {
    crate::project::report_to_json(saved, table).get("analysis").and_then(JsonValue::as_str).unwrap_or("").to_owned()
}

/// A chart form's name as a title: `Wind rose` → `Wind Rose`.
fn title_case(label: &str) -> &'static str {
    match label {
        "Pie chart" => "Pie Chart",
        "Radar chart" => "Radar Chart",
        "Polar chart" => "Polar Chart",
        "Wind rose" => "Wind Rose",
        "Ternary plot" => "Ternary Plot",
        "Waterfall chart" => "Waterfall Chart",
        "Ridgeline plot" => "Ridgeline Plot",
        _ => "Vector Field",
    }
}

/// Title of a graph report: "Y by X", with any panel split.
#[must_use]
pub fn graph_title(spec: &GraphSpec) -> String {
    let e = &spec.encoding;
    if e.is_empty() {
        return "Graph Canvas".to_owned();
    }
    match spec.form {
        GraphForm::Panels => {}
        GraphForm::Treemap(_) => return format!("Treemap: {}", e.x.join(" › ")),
        GraphForm::Parallel => {
            let cols: Vec<&str> = e.x.iter().chain(&e.y).map(String::as_str).collect();
            return format!("Parallel Coordinates: {}", cols.join(", "));
        }
        GraphForm::MultiVari => {
            let y = e.y.first().map_or("", String::as_str);
            return format!("Multi-Vari Chart: {y} by {}", e.x.join(" › "));
        }
        GraphForm::ScatterMatrix => {
            let mut cols: Vec<&str> = Vec::new();
            for c in e.x.iter().chain(&e.y) {
                if !cols.contains(&c.as_str()) {
                    cols.push(c);
                }
            }
            return format!("Scatterplot Matrix: {}", cols.join(", "));
        }
        GraphForm::ClusterMap(_) => return format!("Clustered Heat Map: {}", e.x.join(", ")),
        GraphForm::Estimation => {
            let y = e.y.first().map_or("", String::as_str);
            let x = e.x.first().map_or("", String::as_str);
            return format!("Estimation Plot: {y} by {x}");
        }
        GraphForm::Sankey(_) => return format!("Sankey Diagram: {}", e.x.join(" › ")),
        GraphForm::Chord(_) => {
            return format!("Chord Diagram: {}", e.x.iter().take(2).cloned().collect::<Vec<_>>().join(" → "));
        }
        GraphForm::Network(_) => {
            return format!("Network: {}", e.x.iter().take(2).cloned().collect::<Vec<_>>().join(" – "));
        }
        form @ (GraphForm::Pie(_)
        | GraphForm::Radar(_)
        | GraphForm::Polar(_)
        | GraphForm::WindRose(_)
        | GraphForm::Ternary
        | GraphForm::Waterfall(_)
        | GraphForm::Ridgeline(_)
        | GraphForm::VectorField(_)) => {
            let first = |v: &[String]| v.first().cloned().unwrap_or_default();
            let what = match form {
                GraphForm::Pie(p) if p.size_by_sum && !e.y.is_empty() => format!("{} by {}", first(&e.y), first(&e.x)),
                GraphForm::Pie(_) => first(&e.x),
                GraphForm::Polar(_) | GraphForm::Waterfall(_) | GraphForm::Ridgeline(_) => {
                    format!("{} by {}", first(&e.y), first(&e.x)).trim_start_matches(" by ").to_owned()
                }
                GraphForm::WindRose(_) => match e.y.first() {
                    Some(y) => format!("{y} by {}", first(&e.x)),
                    None => first(&e.x),
                },
                _ => {
                    let mut cols: Vec<&str> = Vec::new();
                    for c in e.x.iter().chain(&e.y) {
                        if !cols.contains(&c.as_str()) {
                            cols.push(c);
                        }
                    }
                    cols.join(", ")
                }
            };
            let name =
                if matches!(form, GraphForm::Pie(p) if p.donut) { "Donut Chart" } else { title_case(form.label()) };
            return format!("{name}: {what}");
        }
    }
    let mut t = match (e.y.is_empty(), e.x.is_empty()) {
        (false, false) => format!("Graph: {} by {}", e.y.join(", "), e.x.join(", ")),
        (false, true) => format!("Graph: {}", e.y.join(", ")),
        _ => format!("Graph: {}", e.x.join(", ")),
    };
    let splits: Vec<&str> = [&e.group_x, &e.group_y, &e.wrap].into_iter().filter_map(|o| o.as_deref()).collect();
    if !splits.is_empty() {
        t.push_str(&format!(" · panels by {}", splits.join(" × ")));
    }
    t
}

/// Page size of an exported graph, in points: 7.5 × 5 inches per panel row,
/// up to a letter-size page.
#[must_use]
pub fn graph_page_size(panel_rows: usize) -> (f64, f64) {
    (540.0, (360.0 + 180.0 * panel_rows.saturating_sub(1) as f64).min(760.0))
}

/// A graph canvas drawn as a vector figure (SVG, PDF or EPS) at its default
/// view (every axis fitted to its data), titled with [`graph_title`] (or the
/// graph's own title), with its right axis and axis breaks.
///
/// # Errors
/// [`StatError`] when the graph cannot be built from the data (an unknown
/// column, or row states that do not match the table).
pub fn graph_figure(
    spec: &GraphSpec,
    table: &DataTable,
    rows: &RowStateManager,
    format: VectorFormat,
) -> Result<Vec<u8>> {
    if rows.len() != table.n_rows() {
        return Err(StatError::InvalidArgument { message: "the row states are out of step with the data".into() });
    }
    let frame = spec.frame(table, rows)?;
    let layers = spec.layers(&frame, rows);
    let ranges = SharedRanges::of(&frame, &layers);
    let views = panel_views(&frame, &ranges, &|_| None, &|_| None);
    let (width, height) = graph_page_size(frame.n_rows);
    let title = graph_title(spec);
    let right = spec.right_axis(table, rows)?;
    let right = right.as_ref().map(|axis| RightView { axis, range: axis.range });
    let figure = Figure { title: &title, format, width, height };
    Ok(export_graph_full(spec, &frame, &layers, &views, figure, GridRules::default(), right))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::catalog;

    #[test]
    fn every_sample_report_runs_and_has_a_key() {
        for info in catalog() {
            let s = (info.build)();
            let rows = RowStateManager::new(s.table.n_rows());
            for saved in &s.reports {
                let r = run_report(saved, &s.table, &rows).unwrap_or_else(|e| panic!("{}: {e}", info.name));
                assert!(!r.title.is_empty());
                assert!(!analysis_key(saved, &s.table).is_empty());
                assert!(matches!(saved, SavedReport::Graph { .. }) || !r.sections.is_empty(), "{}", r.title);
            }
        }
    }

    #[test]
    fn chart_form_titles_name_the_form_and_columns() {
        let spec = |form, x: &[&str], y: &[&str]| GraphSpec {
            encoding: stat_viz::EncodingPlan {
                x: x.iter().map(|s| (*s).to_owned()).collect(),
                y: y.iter().map(|s| (*s).to_owned()).collect(),
                ..Default::default()
            },
            form,
            ..GraphSpec::default()
        };
        let pie = stat_viz::PieSettings::default();
        assert_eq!(graph_title(&spec(GraphForm::Pie(pie), &["Sky"], &[])), "Pie Chart: Sky");
        let donut = stat_viz::PieSettings { donut: true, size_by_sum: true, labels: true };
        assert_eq!(graph_title(&spec(GraphForm::Pie(donut), &["Item"], &["Cost"])), "Donut Chart: Cost by Item");
        let rose = GraphForm::WindRose(stat_viz::WindRoseSettings::default());
        assert_eq!(graph_title(&spec(rose, &["Direction"], &["Speed"])), "Wind Rose: Speed by Direction");
        assert_eq!(graph_title(&spec(rose, &["Direction"], &[])), "Wind Rose: Direction");
        assert_eq!(graph_title(&spec(GraphForm::Ternary, &["a", "b"], &["c"])), "Ternary Plot: a, b, c");
        let ridges = GraphForm::Ridgeline(stat_viz::RidgelineSettings::default());
        assert_eq!(graph_title(&spec(ridges, &["Temp"], &[])), "Ridgeline Plot: Temp");
        let field = GraphForm::VectorField(stat_viz::VectorSettings::default());
        assert_eq!(graph_title(&spec(field, &["x", "u"], &["y", "v"])), "Vector Field: x, u, y, v");
    }

    #[test]
    fn graph_figures_are_vector_files() {
        let t = crate::sample::longley();
        let rows = RowStateManager::new(t.n_rows());
        let mut spec = GraphSpec::default();
        spec.encoding.x = vec![t.columns()[1].name.clone()];
        spec.encoding.y = vec![t.columns()[0].name.clone()];
        let svg = graph_figure(&spec, &t, &rows, VectorFormat::Svg).unwrap();
        assert!(String::from_utf8(svg).unwrap().contains("<svg"));
        let pdf = graph_figure(&spec, &t, &rows, VectorFormat::Pdf).unwrap();
        assert!(pdf.starts_with(b"%PDF"));
        let wrong = RowStateManager::new(t.n_rows() + 1);
        assert!(graph_figure(&spec, &t, &wrong, VectorFormat::Svg).is_err());
    }
}
