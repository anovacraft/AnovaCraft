//! Columns a report can add to its table: predicted values and residuals of
//! models, principal component scores, cluster numbers, a Box–Cox
//! transformed column. The report's options menu and scripts
//! (`report.save_columns(...)`) both offer exactly these.

use stat_core::Result;
use stat_data::{Column, DataTable, MeasurementLevel, RowStateManager};

use crate::project::SavedReport;
use crate::{
    CapabilityAnalysis, ClusterAnalysis, DistributionAnalysis, GlmAnalysis, LinearModelAnalysis, MixedAnalysis,
    PcaAnalysis, RegularizedAnalysis,
};

/// A column a report offers to add to its table.
#[derive(Debug, Clone, PartialEq)]
pub struct ReportColumn {
    /// Identifier scripts use, e.g. `"residual"` or `"pc1"`.
    pub key: String,
    /// Menu item, e.g. `"Save residuals"`.
    pub label: String,
    /// Name of the new column, e.g. `"Residual y"`.
    pub name: String,
    /// One value per table row; NaN for rows the analysis did not use.
    pub values: Vec<f64>,
    /// Measurement level of the new column.
    pub level: MeasurementLevel,
    /// For a formula column (see [`crate::formula`]), its formula; `values`
    /// are then what it computes on the table as it is.
    pub formula: Option<String>,
    /// For a text column (e.g. a predicted class), its values; `values` is
    /// then all NaN.
    pub text: Option<Vec<String>>,
}

impl ReportColumn {
    /// A continuous column.
    #[must_use]
    pub fn continuous(key: &str, label: &str, name: String, values: Vec<f64>) -> Self {
        Self {
            key: key.to_owned(),
            label: label.to_owned(),
            name,
            values,
            level: MeasurementLevel::Continuous,
            formula: None,
            text: None,
        }
    }

    /// A nominal text column.
    #[must_use]
    pub fn text(key: &str, label: &str, name: String, values: Vec<String>) -> Self {
        Self {
            key: key.to_owned(),
            label: label.to_owned(),
            name,
            values: vec![f64::NAN; values.len()],
            level: MeasurementLevel::Nominal,
            formula: None,
            text: Some(values),
        }
    }

    /// The column to add, named `name`.
    #[must_use]
    pub fn into_column(self, name: String) -> Column {
        let mut column = match self.text {
            Some(text) => Column::text(name, text),
            None => Column::numeric(name, self.values),
        };
        column.level = self.level;
        column.formula = self.formula;
        column
    }
}

/// The "Save prediction formula" column of a model whose predicted values
/// are `predicted` and whose prediction formula is `formula` (no column when
/// the formula cannot be written).
pub(crate) fn prediction_formula_column(
    formula: Result<String>,
    name: &str,
    predicted: Vec<f64>,
) -> Option<ReportColumn> {
    let formula = formula.ok()?;
    Some(ReportColumn {
        formula: Some(formula),
        ..ReportColumn::continuous(
            "prediction_formula",
            "Save prediction formula",
            format!("{name} formula"),
            predicted,
        )
    })
}

/// The columns report `saved` offers on `table` with row states `rows`
/// (empty for reports that offer none).
///
/// Linear models: `predicted`, `prediction_formula`, `residual`.
/// Generalized linear and penalized (lasso, ridge, elastic net) models:
/// `predicted`, `prediction_formula`. Ordinal and nominal logistic
/// regression: `probability_1`, `probability_2`, … and `most_likely` (formula
/// columns). Mixed models: `conditional`,
/// `marginal`, `residual`. Principal components: `pc1`, `pc2`, …
/// Clustering: `cluster`. Distribution of positive continuous data:
/// `box_cox`. Outlier distances: `mahalanobis`, `jackknife`, `t2`. Item
/// analysis: `total`. Adjusted p-values: `bonferroni`, `sidak`, `holm`,
/// `hochberg`, `bh`, `by`, `q`. Factor analysis: `factor1`, `factor2`, … (scores).
/// Discriminant analysis: `probability_1`, `probability_2`, …, `most_likely`,
/// `validation` and `canonical1`, `canonical2`, …. Gaussian mixtures:
/// `probability_1`, `probability_2`, …, `cluster`, `uncertainty`. Embeddings:
/// `dim1`, `dim2`, …. Proportional hazards: `linear_predictor`, `hazard_ratio`,
/// `survival`, `martingale`, `deviance`. Parametric survival regression:
/// `predicted`, `cox_snell`. Nonlinear regression: `predicted`,
/// `prediction_formula`, `residual`. Decision trees, random forests, boosted trees and partial
/// least squares: `predicted` (continuous response) or `probability_1`, `probability_2`, … and `most_likely`,
/// plus `validation` when the rows were split. Multivariate adaptive regression splines:
/// `predicted`, `prediction_formula`, `validation`. Treatment effects: `propensity`, `weight` and, for
/// matching without replacement, `matched_set`. Process capability: `in_spec` (formula column, 1 inside
/// the specification limits).
///
/// # Errors
/// Whatever running the analysis reports.
pub fn report_columns(saved: &SavedReport, table: &DataTable, rows: &RowStateManager) -> Result<Vec<ReportColumn>> {
    Ok(match saved {
        SavedReport::Linear { spec, .. } => LinearModelAnalysis::run(table, rows, spec)?.saveable_columns(),
        SavedReport::Mixed { request, .. } => MixedAnalysis::run(table, rows, request)?.saveable_columns(),
        SavedReport::Pca { request } => PcaAnalysis::run(table, rows, request)?.saveable_columns(),
        SavedReport::PrincipalCoordinates { request } => {
            crate::PcoaAnalysis::run(table, rows, request)?.saveable_columns()
        }
        SavedReport::Cluster { request } => ClusterAnalysis::run(table, rows, request)?.saveable_columns(),
        SavedReport::Glm { request, .. } => GlmAnalysis::run(table, rows, request)?.saveable_columns(),
        SavedReport::CategoricalLogistic { request } => {
            crate::CategoricalLogisticAnalysis::run(table, rows, request)?.saveable_columns()
        }
        SavedReport::Regularized { request, .. } => RegularizedAnalysis::run(table, rows, request)?.saveable_columns(),
        SavedReport::DecisionTree { request, .. } => crate::TreeAnalysis::run(table, rows, request)?.saveable_columns(),
        SavedReport::RandomForest { request, .. } => {
            crate::ForestAnalysis::run(table, rows, request)?.saveable_columns()
        }
        SavedReport::BoostedTrees { request, .. } => {
            crate::BoostAnalysis::run(table, rows, request)?.saveable_columns()
        }
        SavedReport::PartialLeastSquares { request, .. } => {
            crate::PlsAnalysis::run(table, rows, request)?.saveable_columns()
        }
        SavedReport::SupportVectorMachine { request, .. } => {
            crate::SvmAnalysis::run(table, rows, request)?.saveable_columns()
        }
        SavedReport::Mars { request, .. } => crate::MarsAnalysis::run(table, rows, request)?.saveable_columns(),
        SavedReport::NeuralNetwork { request, .. } => {
            crate::NeuralAnalysis::run(table, rows, request)?.saveable_columns()
        }
        SavedReport::GaussianProcess { request, .. } => {
            crate::GaussianProcessAnalysis::run(table, rows, request)?.saveable_columns()
        }
        SavedReport::FunctionalData { request, .. } => {
            crate::FunctionalAnalysis::run(table, rows, request)?.saveable_columns()
        }
        SavedReport::Distribution { request, .. } => {
            DistributionAnalysis::run(table, rows, request)?.saveable_columns()
        }
        SavedReport::OutlierDistances { request } => {
            crate::OutlierAnalysis::run(table, rows, request)?.saveable_columns()
        }
        SavedReport::AdjustPValues { request } => crate::PValueAnalysis::run(table, rows, request)?.saveable_columns(),
        SavedReport::TreatmentEffects { request } => {
            crate::TreatmentEffectsAnalysis::run(table, rows, request)?.saveable_columns()
        }
        SavedReport::AreaUnderCurve { request } => crate::AreaAnalysis::run(table, rows, request)?.saveable_columns(),
        SavedReport::Signal { request } => crate::SignalReport::run(table, rows, request)?.saveable_columns(),
        SavedReport::Nonlinear { request, .. } => {
            crate::NonlinearAnalysis::run(table, rows, request)?.saveable_columns()
        }
        SavedReport::ProportionalHazards { request, .. } => {
            crate::ProportionalHazardsAnalysis::run(table, rows, request)?.saveable_columns()
        }
        SavedReport::SurvivalRegression { request, .. } => {
            crate::SurvivalRegressionAnalysis::run(table, rows, request)?.saveable_columns()
        }
        SavedReport::ItemAnalysis { request } => {
            crate::ItemAnalysisReport::run(table, rows, request)?.saveable_columns()
        }
        SavedReport::FactorAnalysis { request } => {
            crate::FactorAnalysisReport::run(table, rows, request)?.saveable_columns()
        }
        SavedReport::DiscriminantAnalysis { request } => {
            crate::DiscriminantAnalysis::run(table, rows, request)?.saveable_columns()
        }
        SavedReport::GaussianMixture { request } => {
            crate::GaussianMixtureAnalysis::run(table, rows, request)?.saveable_columns()
        }
        SavedReport::LatentClass { request } => {
            crate::LatentClassAnalysis::run(table, rows, request)?.saveable_columns()
        }
        SavedReport::TextExploration { request } => {
            crate::TextExploration::run(table, rows, request)?.saveable_columns()
        }
        SavedReport::TopicModel { request } => crate::TopicModelAnalysis::run(table, rows, request)?.saveable_columns(),
        SavedReport::Sentiment { request } => crate::SentimentAnalysis::run(table, rows, request)?.saveable_columns(),
        SavedReport::Embedding { request } => crate::EmbeddingAnalysis::run(table, rows, request)?.saveable_columns(),
        SavedReport::Imputation { request } => crate::ImputationReport::run(table, rows, request)?.saveable_columns(),
        SavedReport::Capability { request } => CapabilityAnalysis::run(table, rows, request)?.saveable_columns(),
        SavedReport::SeriesExplore { request } => {
            crate::SeriesExploreAnalysis::run(table, rows, request)?.saveable_columns()
        }
        SavedReport::SignalProcessing { request } => {
            crate::SignalAnalysis::run(table, rows, request)?.saveable_columns()
        }
        SavedReport::ImageAnalysis { request } => crate::ImageAnalysis::run(table, rows, request)?.saveable_columns(),
        _ => Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::catalog;

    fn sample(name: &str) -> crate::sample::Sample {
        let info = catalog().into_iter().find(|s| s.name.starts_with(name)).unwrap();
        (info.build)()
    }

    #[test]
    fn linear_model_residuals_are_observed_minus_predicted() {
        let s = sample("Norris");
        let mut rows = RowStateManager::new(s.table.n_rows());
        rows.set_excluded([3], true);
        let saved = s.reports.iter().find(|r| matches!(r, SavedReport::Linear { .. })).unwrap();
        let cols = report_columns(saved, &s.table, &rows).unwrap();
        let keys: Vec<&str> = cols.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys, ["predicted", "prediction_formula", "residual"]);
        let SavedReport::Linear { spec, .. } = saved else { unreachable!() };
        let y = s.table.column(spec.response).unwrap().as_numeric().unwrap();
        let a = LinearModelAnalysis::run(&s.table, &rows, spec).unwrap();
        // Residuals of the fitted rows equal the fit's own residuals.
        for (i, &r) in a.rows_used().iter().enumerate() {
            assert!((cols[2].values[r] - a.fit().residuals()[i]).abs() < 1e-9);
            assert!((cols[0].values[r] + cols[2].values[r] - y[r]).abs() < 1e-9);
        }
        // The excluded row is still predicted.
        assert!(cols[0].values[3].is_finite());
        assert_eq!(cols[0].values.len(), s.table.n_rows());
    }

    #[test]
    fn other_reports_offer_their_columns() {
        let mut found = Vec::new();
        for info in catalog() {
            let s = (info.build)();
            let rows = RowStateManager::new(s.table.n_rows());
            for saved in &s.reports {
                for c in report_columns(saved, &s.table, &rows).unwrap() {
                    assert_eq!(c.values.len(), s.table.n_rows(), "{}", c.name);
                    found.push(c.key);
                }
            }
        }
        for key in ["pc1", "cluster", "box_cox", "conditional", "marginal"] {
            assert!(found.iter().any(|k| k == key), "{key} not offered by any sample");
        }
    }
}
