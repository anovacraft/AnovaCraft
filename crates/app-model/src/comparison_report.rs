//! One entry point for the comparison, proportion, rate and power reports —
//! and the attribute capability, principal-components chart, monitoring limits, between/within capability and acceptance
//! sampling reports, and the measurement-study and design-study reports
//! ([`crate::gauge_studies`], [`crate::robust_design`],
//! [`crate::declared_design`], [`crate::design_evaluation`]), which work the same way — so front ends can
//! treat them uniformly (their settings are edited in place and the report re-run).

use crate::compare::{GroupComparison, OneSampleAnalysis, PairedComparison};
use crate::counts::{ProportionAnalysis, RateAnalysis};
use crate::power_analysis::PowerAnalysis;
use crate::project::SavedReport;
use crate::report::ReportSection;
use crate::{
    AcceptanceAnalysis, AttributeCapabilityAnalysis, BetweenWithinAnalysis, CalculatorReport, CrossoverReport,
    GoodnessAnalysis,
};
use stat_core::Result;
use stat_data::{DataTable, RowStateManager};

/// A computed comparison-type report.
#[derive(Debug, Clone)]
pub enum ComparisonReport {
    /// One-sample tests.
    OneSample(OneSampleAnalysis),
    /// Independent groups.
    Groups(GroupComparison),
    /// Paired or repeated measures.
    Paired(PairedComparison),
    /// Proportions.
    Proportions(ProportionAnalysis),
    /// Event rates.
    Rates(RateAnalysis),
    /// Power and sample size.
    Power(PowerAnalysis),
    /// Binomial or Poisson capability.
    AttributeCapability(AttributeCapabilityAnalysis),
    /// Between/within capability.
    BetweenWithin(BetweenWithinAnalysis),
    /// Acceptance sampling plans.
    Acceptance(AcceptanceAnalysis),
    /// Gauge bias and linearity.
    Linearity(crate::LinearityAnalysis),
    /// Single-gauge repeatability (Cg, Cgk).
    Repeatability(crate::RepeatabilityAnalysis),
    /// Intraclass-correlation evaluation.
    Icc(crate::IccAnalysis),
    /// Attribute agreement study.
    Agreement(Box<crate::AgreementAnalysis>),
    /// Go/no-go gauge bias and repeatability.
    AttributeGauge(crate::AttributeGaugeAnalysis),
    /// Principal-components control chart.
    PcaChart(Box<crate::PcaChartAnalysis>),
    /// Environmental monitoring limits.
    MonitoringLimits(Box<crate::MonitoringAnalysis>),
    /// Multi-way table.
    MultiwayTable(crate::MultiwayTableAnalysis),
    /// Stratified contingency tables.
    Stratified(crate::StratifiedAnalysis),
    /// Signal-to-noise analysis.
    SignalToNoise(crate::SignalToNoiseAnalysis),
    /// A table declared as a design.
    DeclaredDesign(crate::DeclaredDesign),
    /// A design evaluated before data collection.
    DesignEvaluation(Box<crate::DesignEvaluationAnalysis>),
    /// A two-level factorial analysis.
    FactorialAnalysis(Box<crate::FactorialAnalysis>),
    /// Next runs of an experiment by expected improvement.
    Sequential(Box<crate::SequentialAnalysis>),
    /// Goodness of fit of counts.
    GoodnessOfFit(GoodnessAnalysis),
    /// Two-period crossover.
    Crossover(CrossoverReport),
    /// Factorial repeated-measures ANOVA.
    FactorialRepeated(Box<crate::FactorialRepeatedReport>),
    /// Nested t test or nested one-way ANOVA.
    Nested(Box<crate::NestedReport>),
    /// Distribution calculator.
    Calculator(CalculatorReport),
    /// Multiple-testing adjustment of a column of p-values.
    PValues(crate::PValueAnalysis),
    /// Many two-group tests.
    ManyTests(crate::ManyTestsAnalysis),
    /// Treatment effects from observational data.
    TreatmentEffects(Box<crate::TreatmentEffectsAnalysis>),
    /// Screening of many responses.
    Screening(Box<crate::ScreeningAnalysis>),
    /// Area under a curve.
    Area(crate::AreaAnalysis),
    /// A Bayesian analysis.
    Bayesian(Box<crate::BayesReport>),
    /// Multiple-response frequencies and cross-tab.
    MultipleResponseItems(Box<crate::MultipleResponseAnalysis>),
    /// Signal smoothing, derivatives and integrals, or spectrum peaks.
    Signal(Box<crate::SignalReport>),
    /// A value column drawn as a picture.
    ImagePlot(Box<crate::image_plot::ImagePlotAnalysis>),
}

impl ComparisonReport {
    /// `true` for the saved-report variants this type handles.
    #[must_use]
    pub fn handles(saved: &SavedReport) -> bool {
        matches!(
            saved,
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
                | SavedReport::SequentialOptimization { .. }
                | SavedReport::GoodnessOfFit { .. }
                | SavedReport::Crossover { .. }
                | SavedReport::FactorialRepeated { .. }
                | SavedReport::Nested { .. }
                | SavedReport::DistributionCalculator { .. }
                | SavedReport::AdjustPValues { .. }
                | SavedReport::ManyTests { .. }
                | SavedReport::TreatmentEffects { .. }
                | SavedReport::Screening { .. }
                | SavedReport::AreaUnderCurve { .. }
                | SavedReport::Bayesian { .. }
                | SavedReport::MultipleResponseItems { .. }
                | SavedReport::Signal { .. }
                | SavedReport::ImagePlot { .. }
        )
    }

    /// Run `saved`; `None` if it is not a comparison-type report.
    #[must_use]
    pub fn run(saved: &SavedReport, table: &DataTable, rows: &RowStateManager) -> Option<Result<Self>> {
        Some(match saved {
            SavedReport::OneSample { request } => OneSampleAnalysis::run(table, rows, request).map(Self::OneSample),
            SavedReport::Groups { request } => GroupComparison::run(table, rows, request).map(Self::Groups),
            SavedReport::Paired { request } => PairedComparison::run(table, rows, request).map(Self::Paired),
            SavedReport::Proportions { request } => {
                ProportionAnalysis::run(table, rows, request).map(Self::Proportions)
            }
            SavedReport::Rates { request } => RateAnalysis::run(table, rows, request).map(Self::Rates),
            SavedReport::Power { request } => PowerAnalysis::run(request).map(Self::Power),
            SavedReport::AttributeCapability { request } => {
                AttributeCapabilityAnalysis::run(table, rows, request).map(Self::AttributeCapability)
            }
            SavedReport::BetweenWithinCapability { request } => {
                BetweenWithinAnalysis::run(table, rows, request).map(Self::BetweenWithin)
            }
            SavedReport::AcceptanceSampling { request } => AcceptanceAnalysis::run(request).map(Self::Acceptance),
            SavedReport::GaugeLinearity { request } => {
                crate::LinearityAnalysis::run(table, rows, request).map(Self::Linearity)
            }
            SavedReport::GaugeRepeatability { request } => {
                crate::RepeatabilityAnalysis::run(table, rows, request).map(Self::Repeatability)
            }
            SavedReport::IntraclassCorrelation { request } => {
                crate::IccAnalysis::run(table, rows, request).map(Self::Icc)
            }
            SavedReport::AttributeAgreement { request } => {
                crate::AgreementAnalysis::run(table, rows, request).map(|a| Self::Agreement(Box::new(a)))
            }
            SavedReport::AttributeGauge { request } => {
                crate::AttributeGaugeAnalysis::run(table, rows, request).map(Self::AttributeGauge)
            }
            SavedReport::PcaChart { request } => {
                crate::PcaChartAnalysis::run(table, rows, request).map(|a| Self::PcaChart(Box::new(a)))
            }
            SavedReport::MonitoringLimits { request } => {
                crate::MonitoringAnalysis::run(table, rows, request).map(|a| Self::MonitoringLimits(Box::new(a)))
            }
            SavedReport::MultiwayTable { request } => {
                crate::MultiwayTableAnalysis::run(table, rows, request).map(Self::MultiwayTable)
            }
            SavedReport::StratifiedTables { request } => {
                crate::StratifiedAnalysis::run(table, rows, request).map(Self::Stratified)
            }
            SavedReport::SignalToNoise { request } => {
                crate::SignalToNoiseAnalysis::run(table, rows, request).map(Self::SignalToNoise)
            }
            SavedReport::DeclaredDesign { request } => {
                crate::DeclaredDesign::run(table, rows, request).map(Self::DeclaredDesign)
            }
            SavedReport::DesignEvaluation { request } => {
                crate::DesignEvaluationAnalysis::run(table, rows, request).map(|a| Self::DesignEvaluation(Box::new(a)))
            }
            SavedReport::FactorialAnalysis { request } => {
                crate::FactorialAnalysis::run(table, rows, request).map(|a| Self::FactorialAnalysis(Box::new(a)))
            }
            SavedReport::SequentialOptimization { request } => {
                crate::SequentialAnalysis::run(table, rows, request).map(|a| Self::Sequential(Box::new(a)))
            }
            SavedReport::GoodnessOfFit { request } => {
                GoodnessAnalysis::run(table, rows, request).map(Self::GoodnessOfFit)
            }
            SavedReport::Crossover { request } => CrossoverReport::run(table, rows, request).map(Self::Crossover),
            SavedReport::FactorialRepeated { request } => {
                crate::FactorialRepeatedReport::run(table, rows, request).map(|a| Self::FactorialRepeated(Box::new(a)))
            }
            SavedReport::Nested { request } => {
                crate::NestedReport::run(table, rows, request).map(|a| Self::Nested(Box::new(a)))
            }
            SavedReport::DistributionCalculator { request } => CalculatorReport::run(request).map(Self::Calculator),
            SavedReport::AdjustPValues { request } => {
                crate::PValueAnalysis::run(table, rows, request).map(Self::PValues)
            }
            SavedReport::ManyTests { request } => {
                crate::ManyTestsAnalysis::run(table, rows, request).map(Self::ManyTests)
            }
            SavedReport::TreatmentEffects { request } => {
                crate::TreatmentEffectsAnalysis::run(table, rows, request).map(|a| Self::TreatmentEffects(Box::new(a)))
            }
            SavedReport::Screening { request } => {
                crate::ScreeningAnalysis::run(table, rows, request).map(|a| Self::Screening(Box::new(a)))
            }
            SavedReport::AreaUnderCurve { request } => crate::AreaAnalysis::run(table, rows, request).map(Self::Area),
            SavedReport::MultipleResponseItems { request } => {
                crate::MultipleResponseAnalysis::run(table, rows, request)
                    .map(|a| Self::MultipleResponseItems(Box::new(a)))
            }
            SavedReport::Bayesian { request } => {
                crate::BayesReport::run(table, rows, request).map(|a| Self::Bayesian(Box::new(a)))
            }
            SavedReport::Signal { request } => {
                crate::SignalReport::run(table, rows, request).map(|a| Self::Signal(Box::new(a)))
            }
            SavedReport::ImagePlot { request } => {
                crate::image_plot::ImagePlotAnalysis::run(table, rows, request).map(|a| Self::ImagePlot(Box::new(a)))
            }
            _ => return None,
        })
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        match self {
            Self::OneSample(a) => a.title(),
            Self::Groups(a) => a.title(),
            Self::Paired(a) => a.title(),
            Self::Proportions(a) => a.title(),
            Self::Rates(a) => a.title(),
            Self::Power(a) => a.title(),
            Self::AttributeCapability(a) => a.title(),
            Self::BetweenWithin(a) => a.title(),
            Self::Acceptance(a) => a.title(),
            Self::Linearity(a) => a.title(),
            Self::Repeatability(a) => a.title(),
            Self::Icc(a) => a.title(),
            Self::Agreement(a) => a.title(),
            Self::AttributeGauge(a) => a.title(),
            Self::PcaChart(a) => a.title(),
            Self::MonitoringLimits(a) => a.title(),
            Self::MultiwayTable(a) => a.title(),
            Self::Stratified(a) => a.title(),
            Self::SignalToNoise(a) => a.title(),
            Self::DeclaredDesign(a) => a.title(),
            Self::DesignEvaluation(a) => a.title(),
            Self::FactorialAnalysis(a) => a.title(),
            Self::Sequential(a) => a.title(),
            Self::GoodnessOfFit(a) => a.title(),
            Self::Crossover(a) => a.title(),
            Self::FactorialRepeated(a) => a.title(),
            Self::Nested(a) => a.title(),
            Self::Calculator(a) => a.title(),
            Self::PValues(a) => a.title(),
            Self::ManyTests(a) => a.title(),
            Self::TreatmentEffects(a) => a.title(),
            Self::Screening(a) => a.title(),
            Self::Area(a) => a.title(),
            Self::Bayesian(a) => a.title(),
            Self::MultipleResponseItems(a) => a.title(),
            Self::Signal(a) => a.title(),
            Self::ImagePlot(a) => a.title(),
        }
    }

    /// Title when the report could not be computed.
    #[must_use]
    pub fn failed_title(saved: &SavedReport) -> String {
        let name = match saved {
            SavedReport::OneSample { .. } => "One-Sample Tests",
            SavedReport::Groups { .. } => "Compare Groups",
            SavedReport::Paired { .. } => "Paired Comparison",
            SavedReport::Proportions { .. } => "Proportions",
            SavedReport::Rates { .. } => "Event Rates",
            SavedReport::Power { .. } => "Power and Sample Size",
            SavedReport::AttributeCapability { .. } => "Attribute Capability",
            SavedReport::BetweenWithinCapability { .. } => "Between/Within Capability",
            SavedReport::AcceptanceSampling { .. } => "Acceptance Sampling",
            SavedReport::GaugeLinearity { .. } => "Bias and Linearity",
            SavedReport::GaugeRepeatability { .. } => "Gauge Repeatability",
            SavedReport::IntraclassCorrelation { .. } => "Intraclass Correlation",
            SavedReport::AttributeAgreement { .. } => "Attribute Agreement",
            SavedReport::AttributeGauge { .. } => "Attribute Gauge Bias and Repeatability",
            SavedReport::PcaChart { .. } => "Principal-Components Control Chart",
            SavedReport::MonitoringLimits { .. } => "Monitoring Limits",
            SavedReport::MultiwayTable { .. } => "Multi-way Table",
            SavedReport::StratifiedTables { .. } => "Stratified Analysis",
            SavedReport::SignalToNoise { .. } => "Signal-to-Noise Analysis",
            SavedReport::DeclaredDesign { .. } => "Declared Design",
            SavedReport::DesignEvaluation { .. } => "Design Evaluation",
            SavedReport::FactorialAnalysis { .. } => "Factorial Analysis",
            SavedReport::SequentialOptimization { .. } => "Sequential Optimization",
            SavedReport::GoodnessOfFit { .. } => "Goodness of Fit",
            SavedReport::Crossover { .. } => "Crossover Equivalence",
            SavedReport::FactorialRepeated { .. } => "Repeated-Measures ANOVA",
            SavedReport::Nested { .. } => "Nested Comparison",
            SavedReport::DistributionCalculator { .. } => "Distribution Calculator",
            SavedReport::AdjustPValues { .. } => "Adjusted p-values",
            SavedReport::ManyTests { .. } => "Many Tests",
            SavedReport::TreatmentEffects { .. } => "Treatment Effect",
            SavedReport::Screening { .. } => "Screening",
            SavedReport::AreaUnderCurve { .. } => "Area under the Curve",
            SavedReport::Bayesian { request } => request.name(),
            SavedReport::MultipleResponseItems { .. } => "Multiple Response",
            SavedReport::Signal { request } => request.name(),
            SavedReport::ImagePlot { .. } => "Image Plot",
            _ => "Report",
        };
        format!("{name} (could not be computed)")
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        match self {
            Self::OneSample(a) => a.sections(),
            Self::Groups(a) => a.sections(),
            Self::Paired(a) => a.sections(),
            Self::Proportions(a) => a.sections(),
            Self::Rates(a) => a.sections(),
            Self::Power(a) => a.sections(),
            Self::AttributeCapability(a) => a.sections(),
            Self::BetweenWithin(a) => a.sections(),
            Self::Acceptance(a) => a.sections(),
            Self::Linearity(a) => a.sections(),
            Self::Repeatability(a) => a.sections(),
            Self::Icc(a) => a.sections(),
            Self::Agreement(a) => a.sections(),
            Self::AttributeGauge(a) => a.sections(),
            Self::PcaChart(a) => a.sections(),
            Self::MonitoringLimits(a) => a.sections(),
            Self::MultiwayTable(a) => a.sections(),
            Self::Stratified(a) => a.sections(),
            Self::SignalToNoise(a) => a.sections(),
            Self::DeclaredDesign(a) => a.sections(),
            Self::DesignEvaluation(a) => a.sections(),
            Self::FactorialAnalysis(a) => a.sections(),
            Self::Sequential(a) => a.sections(),
            Self::GoodnessOfFit(a) => a.sections(),
            Self::Crossover(a) => a.sections(),
            Self::FactorialRepeated(a) => a.sections(),
            Self::Nested(a) => a.sections(),
            Self::Calculator(a) => a.sections(),
            Self::PValues(a) => a.sections(),
            Self::ManyTests(a) => a.sections(),
            Self::TreatmentEffects(a) => a.sections(),
            Self::Screening(a) => a.sections(),
            Self::Area(a) => a.sections(),
            Self::Bayesian(a) => a.sections(),
            Self::MultipleResponseItems(a) => a.sections(),
            Self::Signal(a) => a.sections(),
            Self::ImagePlot(a) => a.sections(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatches_every_comparison_report() {
        let s = (crate::sample::catalog()[15].build)();
        let rows = RowStateManager::new(s.table.n_rows());
        for r in &s.reports {
            assert!(ComparisonReport::handles(r));
            let a = ComparisonReport::run(r, &s.table, &rows).unwrap().unwrap();
            assert!(!a.sections().is_empty());
            assert!(!a.title().is_empty());
        }
        let longley = crate::sample::longley();
        let other = SavedReport::Bivariate { request: crate::BivariateRequest::new(0, 1) };
        assert!(ComparisonReport::run(&other, &longley, &rows).is_none());
        assert!(ComparisonReport::failed_title(&s.reports[0]).starts_with("Paired"));
    }
}
