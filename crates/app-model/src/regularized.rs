//! Penalized (regularized) regression analysis bound to a dataset: lasso,
//! ridge or elastic net ([`stat_predictive::elastic_net`]), with the penalty
//! chosen by cross-validation. Categorical predictors are indicator coded.

use crate::format::format_number;
use crate::model_data::ModelData;
use crate::report::{PlotSpec, PointGroups, ReportSection, SectionContent, SectionKind, TextTable};
use stat_core::Result;
use stat_data::{DataTable, RowStateManager};
use stat_predictive::{ElasticNetOptions, ElasticNetPath, fit_elastic_net};

const DIGITS: usize = 6;

/// Columns and settings for a penalized regression.
#[derive(Debug, Clone, PartialEq)]
pub struct RegularizedRequest {
    /// Response column (numeric).
    pub response: usize,
    /// Predictor columns.
    pub predictors: Vec<usize>,
    /// Mixing parameter: 1 = lasso, 0 = ridge.
    pub alpha: f64,
    /// Cross-validation folds.
    pub folds: usize,
    /// Seed of the fold assignment.
    pub seed: u64,
}

/// A fitted penalty path with cross-validation.
#[derive(Debug, Clone)]
pub struct RegularizedAnalysis {
    request: RegularizedRequest,
    response_name: String,
    path: ElasticNetPath,
    n: usize,
    data: ModelData,
    design: stat_linear::ModelDesign,
}

fn kind_name(alpha: f64) -> String {
    if alpha >= 1.0 {
        "Lasso".to_owned()
    } else if alpha <= 0.0 {
        "Ridge".to_owned()
    } else {
        format!("Elastic Net (α = {alpha})")
    }
}

impl RegularizedAnalysis {
    /// Fit the path to the complete, included rows.
    ///
    /// # Errors
    /// Invalid columns or settings, or errors from [`fit_elastic_net`].
    pub fn run(table: &DataTable, states: &RowStateManager, request: &RegularizedRequest) -> Result<Self> {
        let data = ModelData::gather(table, states, request.response, &request.predictors, false)?;
        let (design, x, y) = data.design()?;
        let n = y.len();
        let options = ElasticNetOptions {
            folds: request.folds.min(n),
            seed: request.seed,
            names: design.column_names().to_vec(),
            ..ElasticNetOptions::new(request.alpha)
        };
        let path = fit_elastic_net(&x, &y, &options)?;
        Ok(Self { request: request.clone(), response_name: data.response_name.clone(), path, n, data, design })
    }

    /// The reported (λ_1SE) fit as a formula column (see
    /// [`crate::prediction_formula`]).
    ///
    /// # Errors
    /// If the formula cannot be written (e.g. a date-time column used as a category).
    pub fn prediction_formula(&self) -> Result<String> {
        let l = self.path.index_1se;
        crate::prediction_formula::prediction_formula(
            &self.design,
            &self.data.factors,
            self.path.intercepts[l],
            &self.path.coefficients[l],
            crate::prediction_formula::Link::Identity,
        )
    }

    /// Columns the report can add: predictions of the reported (λ_1SE) fit
    /// for every row with its predictors present (`predicted`) and the same
    /// as a formula column (`prediction_formula`).
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<crate::ReportColumn> {
        let l = self.path.index_1se;
        let predicted = self
            .data
            .predict_all(&self.design, |x| Ok(self.path.predict(x, l)))
            .unwrap_or_else(|_| vec![f64::NAN; self.data.y_all.len()]);
        let name = format!("Predicted {}", self.response_name);
        let mut out = vec![crate::ReportColumn::continuous(
            "predicted",
            "Save predicted values",
            name.clone(),
            predicted.clone(),
        )];
        out.extend(crate::report_columns::prediction_formula_column(self.prediction_formula(), &name, predicted));
        out
    }

    /// The fitted path.
    #[must_use]
    pub fn path(&self) -> &ElasticNetPath {
        &self.path
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!(
            "{} Regression: {} ~ {}",
            kind_name(self.request.alpha),
            self.response_name,
            self.path.names.join(" + ")
        )
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let p = &self.path;
        let has_cv = !p.cv_mse.is_empty();
        let (lmin, l1se) = (p.index_min, p.index_1se);
        let mut summary = TextTable::new(["Statistic", "Value"]);
        let mut row = |a: &str, b: String| summary.push([a.to_owned(), b]);
        row("Penalty", kind_name(self.request.alpha));
        row("Observations used", self.n.to_string());
        if has_cv {
            row("Cross-validation", format!("{} folds (seed {})", self.request.folds.min(self.n), self.request.seed));
            row("λ with minimum CV error", format_number(p.lambdas[lmin], 4));
            row(
                "CV mean squared error at that λ",
                format!("{} ± {}", format_number(p.cv_mse[lmin], DIGITS), format_number(p.cv_se[lmin], 3)),
            );
            row("Largest λ within 1 SE of the minimum (λ_1SE)", format_number(p.lambdas[l1se], 4));
            row("CV mean squared error at λ_1SE", format_number(p.cv_mse[l1se], DIGITS));
        }
        row("Non-zero coefficients at λ_1SE", format!("{} of {}", p.n_nonzero(l1se), p.names.len()));
        let mut out = vec![ReportSection {
            kind: SectionKind::PenaltySummary,
            title: "Penalty Selection".to_owned(),
            content: SectionContent::Table(summary),
        }];

        let mut coefs = TextTable::new(["Term", "At λ_1SE", "At λ_min", "Standardized at λ_1SE"]);
        coefs.push([
            "Intercept".to_owned(),
            format_number(p.intercepts[l1se], DIGITS),
            format_number(p.intercepts[lmin], DIGITS),
            String::new(),
        ]);
        for (j, name) in p.names.iter().enumerate() {
            let show = |v: f64| if v == 0.0 { "0 (dropped)".to_owned() } else { format_number(v, DIGITS) };
            coefs.push([
                name.clone(),
                show(p.coefficients[l1se][j]),
                show(p.coefficients[lmin][j]),
                format_number(p.standardized[l1se][j], 4),
            ]);
        }
        out.push(ReportSection {
            kind: SectionKind::PenalizedCoefficients,
            title: "Coefficients (original units; standardized coefficients are comparable across terms)".to_owned(),
            content: SectionContent::Table(coefs),
        });

        let log_l: Vec<f64> = p.lambdas.iter().map(|l| l.log10()).collect();
        if has_cv {
            let line = log_l.iter().zip(&p.cv_mse).map(|(&x, &y)| [x, y]).collect();
            let lower = log_l.iter().zip(p.cv_mse.iter().zip(&p.cv_se)).map(|(&x, (&m, &s))| [x, m - s]).collect();
            let upper = log_l.iter().zip(p.cv_mse.iter().zip(&p.cv_se)).map(|(&x, (&m, &s))| [x, m + s]).collect();
            let (lo, hi) = p
                .cv_mse
                .iter()
                .zip(&p.cv_se)
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), (&m, &s)| (a.min(m - s), b.max(m + s)));
            let spec = PlotSpec {
                x_time: None,
                y_time: None,
                x_label: "log₁₀ λ".to_owned(),
                y_label: "CV mean squared error".to_owned(),
                points: Vec::new(),
                line: Some(line),
                line_name: Some("Cross-validated error".to_owned()),
                band: Some((lower, upper)),
                band_name: Some("± 1 standard error".to_owned()),
                groups: None,
                x_ticks: None,
                panes: Vec::new(),
                annotations: None,
                segments: vec![vec![[log_l[lmin], lo], [log_l[lmin], hi]], vec![[log_l[l1se], lo], [log_l[l1se], hi]]],
                ..PlotSpec::default()
            };
            out.push(ReportSection {
                kind: SectionKind::CrossValidation,
                title: "Cross-Validation (vertical lines at λ_min and λ_1SE; band ± 1 SE)".to_owned(),
                content: SectionContent::Plot(Box::new(spec)),
            });
        }
        let lines: Vec<Vec<[f64; 2]>> =
            (0..p.names.len()).map(|j| log_l.iter().zip(&p.standardized).map(|(&x, b)| [x, b[j]]).collect()).collect();
        let (lo, hi) = p.standardized.iter().flatten().fold((0.0f64, 0.0f64), |(a, b), &v| (a.min(v), b.max(v)));
        let spec = PlotSpec {
            x_time: None,
            y_time: None,
            x_label: "log₁₀ λ".to_owned(),
            y_label: "Standardized coefficient".to_owned(),
            points: Vec::new(),
            line: None,
            band: None,
            groups: Some(PointGroups {
                title: "Term".to_owned(),
                labels: p.names.clone(),
                membership: Vec::new(),
                lines,
            }),
            x_ticks: None,
            panes: Vec::new(),
            annotations: None,
            segments: vec![vec![[log_l[l1se], lo], [log_l[l1se], hi]]],
            ..PlotSpec::default()
        };
        out.push(ReportSection {
            kind: SectionKind::CoefficientPath,
            title: "Coefficient Paths (vertical line at λ_1SE)".to_owned(),
            content: SectionContent::Plot(Box::new(spec)),
        });
        out.push(crate::report::prediction_formula_section(self.prediction_formula()));
        out
    }
}

impl crate::surface_profiler::SurfaceModel for RegularizedAnalysis {
    fn response_name(&self) -> &str {
        &self.response_name
    }

    fn profile_factors(&self) -> Vec<crate::surface_profiler::ProfileFactor> {
        self.data.profile_factors(&self.design)
    }

    fn observations(&self) -> Vec<crate::surface_profiler::Observation> {
        self.data.observations(&self.design)
    }

    fn default_profile_settings(&self) -> Vec<stat_linear::TermValue> {
        self.data.default_settings(&self.design)
    }

    /// Predictions of the reported model, at λ_1SE.
    fn predict_settings(&self, points: &[Vec<stat_linear::TermValue>]) -> Result<Vec<f64>> {
        if points.is_empty() {
            return Ok(Vec::new());
        }
        let rows: Vec<Vec<f64>> = points.iter().map(|s| self.design.encode(s)).collect::<Result<_>>()?;
        Ok(self.path.predict(&stat_core::Matrix::from_rows(&rows)?, self.path.index_1se))
    }

    fn profile_note(&self) -> Option<String> {
        Some(
            "Predictions of the penalized fit at λ_1SE (the coefficients reported above). No confidence bands: \
             ordinary intervals are not valid after penalized selection."
                .to_owned(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    #[test]
    fn lasso_report_with_a_categorical_predictor() {
        let n = 40;
        let x1: Vec<f64> = (0..n).map(|i| f64::from((i * 7 % 11) as u32) - 5.0).collect();
        let x2: Vec<f64> = (0..n).map(|i| f64::from((i * 3 % 7) as u32) - 3.0).collect();
        let g: Vec<String> = (0..n).map(|i| ["p", "q", "r"][i % 3].to_owned()).collect();
        let y: Vec<f64> = (0..n)
            .map(|i| {
                3.0 + 2.0 * x1[i] + if i % 3 == 1 { 4.0 } else { 0.0 } + 0.3 * (f64::from((i * 5 % 9) as u32) - 4.0)
            })
            .collect();
        let t = DataTable::new(
            "t",
            vec![Column::numeric("x1", x1), Column::numeric("x2", x2), Column::text("g", g), Column::numeric("y", y)],
        )
        .unwrap();
        let req = RegularizedRequest { response: 3, predictors: vec![0, 1, 2], alpha: 1.0, folds: 5, seed: 1 };
        let a = RegularizedAnalysis::run(&t, &RowStateManager::new(n), &req).unwrap();
        assert!(a.title().starts_with("Lasso Regression: y ~ x1 + x2 + g: q + g: r"));
        let p = a.path();
        let at = p.index_min;
        assert!((p.coefficients[at][0] - 2.0).abs() < 0.1, "x1 effect recovered: {}", p.coefficients[at][0]);
        assert!(p.coefficients[at][2] > 3.0, "level q shift recovered");
        let s = a.sections();
        let kinds: Vec<_> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::PenaltySummary,
                SectionKind::PenalizedCoefficients,
                SectionKind::CrossValidation,
                SectionKind::CoefficientPath,
                SectionKind::PredictionFormula
            ]
        );
        let SectionContent::Plot(path) = &s[3].content else { panic!() };
        assert_eq!(path.groups.as_ref().unwrap().lines.len(), 4);
        let ridge =
            RegularizedAnalysis::run(&t, &RowStateManager::new(n), &RegularizedRequest { alpha: 0.0, ..req }).unwrap();
        assert!(ridge.title().starts_with("Ridge"));
        assert_eq!(ridge.path().n_nonzero(ridge.path().index_min), 4, "ridge keeps every term");
    }
}
