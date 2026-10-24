//! Generalized linear model analysis bound to a dataset ([`stat_discrete::glm`]).
//!
//! Predictors may be continuous or categorical (indicator coding, as in the
//! linear model). For the binomial family the response may be numbers 0/1 or
//! any two labels; the second label in natural order is modelled as the event,
//! so the model is for `P(response = event)`. Each term gets a likelihood-ratio
//! test from refitting without it.

use crate::format::{format_number, format_p_value};
use crate::model_data::ModelData;
use crate::report::{PlotSpec, ReportSection, RowPoint, SectionContent, SectionKind, TextTable};
use crate::report_columns::{ReportColumn, prediction_formula_column};
use stat_core::distributions::{ChiSquared, ContinuousDistribution};
use stat_core::{Matrix, Result, StatError};
use stat_data::{DataTable, RowStateManager};
use stat_discrete::{Family, GlmFit, GlmOptions, Link, fit_glm};
use stat_linear::{ModelDesign, TermValue};

const DIGITS: usize = 6;
const CURVE_POINTS: usize = 80;

/// Columns and family for a generalized linear model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlmRequest {
    /// Response column.
    pub response: usize,
    /// Predictor columns.
    pub predictors: Vec<usize>,
    /// Response distribution.
    pub family: Family,
    /// Link function; `None` for the family's default
    /// ([`Family::default_link`]).
    pub link: Option<Link>,
    /// Binomial only: the response value modelled as the event (coded 1);
    /// `None` for the second of its two values in natural order.
    pub event: Option<String>,
    /// Model effects as `(position in predictors, power)` products; empty
    /// means main effects only.
    pub effects: Vec<Vec<(usize, u8)>>,
    /// Continuous predictors entered as splines, by position in
    /// `predictors` (missing entries: none).
    pub splines: Vec<Option<stat_linear::SplineSpec>>,
    /// Comparisons within the slices of each two-factor interaction of
    /// categorical predictors, pairs adjusted by this method (`None`: off).
    pub slices: Option<stat_linear::marginal::PairAdjust>,
    /// Slopes of each `factor × continuous` effect by level (`None`: off).
    pub slopes: Option<stat_linear::marginal::PairAdjust>,
}

impl GlmRequest {
    /// The effects to fit: the listed ones, or every main effect.
    #[must_use]
    pub fn effective_effects(&self) -> Vec<Vec<(usize, u8)>> {
        if self.effects.is_empty() { stat_linear::main_effects(self.predictors.len()) } else { self.effects.clone() }
    }

    /// The link used: the requested one or the family's default.
    #[must_use]
    pub fn link(&self) -> Link {
        self.link.unwrap_or_else(|| self.family.default_link())
    }
}

/// A fitted GLM and what its report needs.
#[derive(Debug, Clone)]
pub struct GlmAnalysis {
    request: GlmRequest,
    data: ModelData,
    design: ModelDesign,
    fit: GlmFit,
    /// (term, df, χ², p) for each term.
    term_tests: Vec<(String, usize, f64, f64)>,
    y: Vec<f64>,
    /// Name of the profiled mean (`P(… = …)` for a binomial response).
    profile_name: String,
}

impl GlmAnalysis {
    /// Fit the model to the complete, included rows.
    ///
    /// # Errors
    /// Invalid columns, a response unsuited to the family, or errors from
    /// [`fit_glm`].
    pub fn run(table: &DataTable, states: &RowStateManager, request: &GlmRequest) -> Result<Self> {
        let binary = request.family == Family::Binomial;
        let data = if binary {
            ModelData::gather_binary(table, states, request.response, &request.predictors, request.event.as_deref())?
        } else {
            ModelData::gather(table, states, request.response, &request.predictors, false)?
        };
        let (design, x, y) = data.design_with(&request.effective_effects(), &request.splines)?;
        let base = GlmOptions { link: request.link, ..GlmOptions::new(request.family) };
        let options = GlmOptions { names: design.column_names().to_vec(), ..base.clone() };
        let fit = fit_glm(&x, &y, &options)?;
        let mut term_tests = Vec::new();
        for term in design.effects() {
            let keep: Vec<usize> = (0..x.ncols()).filter(|j| !term.columns.contains(j)).collect();
            let reduced_deviance = if keep.is_empty() {
                fit.null_deviance
            } else {
                let cols: Vec<&[f64]> = keep.iter().map(|&j| x.col(j)).collect();
                let names = keep.iter().map(|&j| design.column_names()[j].clone()).collect();
                fit_glm(&Matrix::from_columns(&cols)?, &y, &GlmOptions { names, ..base.clone() })?.deviance
            };
            let df = term.columns.len();
            let chi = ((reduced_deviance - fit.deviance) / fit.dispersion).max(0.0);
            let p = ChiSquared::new(df as f64)?.sf(chi)?;
            term_tests.push((term.name.clone(), df, chi, p));
        }
        let mut out = Self { request: request.clone(), data, design, fit, term_tests, y, profile_name: String::new() };
        out.profile_name = out.mean_name();
        Ok(out)
    }

    /// The fitted mean as a formula column (see
    /// [`crate::prediction_formula`]): the inverse link applied to `η` —
    /// `logistic(η)`, `normal_cdf(η)`, `1.0 - exp(-exp(η))`, `exp(η)`,
    /// `1.0 / η` or `η` itself.
    ///
    /// # Errors
    /// If the formula cannot be written (e.g. a date-time column used as a category).
    pub fn prediction_formula(&self) -> Result<String> {
        use crate::prediction_formula::{Link as FormulaLink, prediction_formula};
        let b: Vec<f64> = self.fit.coefficients.iter().map(|c| c.estimate).collect();
        let link = match self.fit.link {
            Link::Logit => FormulaLink::Logit,
            Link::Probit => FormulaLink::Probit,
            Link::ComplementaryLogLog => FormulaLink::ComplementaryLogLog,
            Link::Log => FormulaLink::Log,
            Link::Identity => FormulaLink::Identity,
            Link::Inverse => FormulaLink::Inverse,
        };
        match b.split_first() {
            Some((&b0, slopes)) if slopes.len() == self.design.column_names().len() => {
                prediction_formula(&self.design, &self.data.factors, b0, slopes, link)
            }
            _ => Err(StatError::InvalidArgument { message: "the model has no intercept".into() }),
        }
    }

    /// Columns the report can add: the fitted mean of every row with its
    /// predictors present (`predicted`) and the same as a formula column
    /// (`prediction_formula`).
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        let predicted = self
            .data
            .predict_all(&self.design, |x| self.fit.predict(x))
            .unwrap_or_else(|_| vec![f64::NAN; self.data.y_all.len()]);
        let name = format!("Predicted {}", self.mean_name());
        let mut out =
            vec![ReportColumn::continuous("predicted", "Save predicted values", name.clone(), predicted.clone())];
        out.extend(prediction_formula_column(self.prediction_formula(), &name, predicted));
        out
    }

    /// Name of the modelled mean: `P(response = event)` for a binomial
    /// response, otherwise the response.
    #[must_use]
    pub fn mean_name(&self) -> String {
        match &self.data.binary {
            Some(b) => format!("P({} = {})", self.data.response_name, b.event),
            None => self.data.response_name.clone(),
        }
    }

    fn encoded(&self, points: &[Vec<TermValue>]) -> Result<Matrix> {
        let rows: Vec<Vec<f64>> = points.iter().map(|s| self.design.encode(s)).collect::<Result<_>>()?;
        Matrix::from_rows(&rows)
    }

    /// The fit.
    #[must_use]
    pub fn fit(&self) -> &GlmFit {
        &self.fit
    }

    /// Each term's name and its coefficients' positions (the intercept is 0).
    pub(crate) fn effect_columns(&self) -> Vec<(String, Vec<usize>)> {
        self.design.effects().iter().map(|t| (t.name.clone(), t.columns.clone().map(|j| j + 1).collect())).collect()
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let kind = match (self.request.family, self.fit.link) {
            (Family::Binomial, Link::Probit) => "Probit Regression".to_owned(),
            (Family::Binomial, Link::ComplementaryLogLog) => "Complementary Log-Log Regression".to_owned(),
            (Family::Binomial, _) => "Logistic Regression".to_owned(),
            (Family::Poisson, Link::Log) => "Poisson Regression".to_owned(),
            (Family::Gamma, _) => format!("Gamma Regression ({} link)", self.fit.link.label()),
            _ => format!("Generalized Linear Model ({})", self.fit.label()),
        };
        let response = match &self.data.binary {
            Some(b) => format!("P({} = {})", self.data.response_name, b.event),
            None => self.data.response_name.clone(),
        };
        let effects: Vec<&str> = self.design.effects().iter().map(|e| e.name.as_str()).collect();
        format!("{kind}: {response} ~ {}", effects.join(" + "))
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary()];
        out.extend(
            crate::report::EffectPareto::new(
                self.term_tests.iter().map(|(name, _, _, p)| (name.clone(), *p)),
                "likelihood-ratio tests",
            )
            .section(),
        );
        if !self.request.effects.is_empty() {
            out.extend(crate::report::hierarchy_note(&self.data.predictor_names, &self.request.effects));
        }
        out.push(self.estimates());
        out.extend(crate::marginal_sections::spline_knots_section(&self.design));
        out.push(self.term_table());
        if self.request.slices.is_some() || self.request.slopes.is_some() {
            let beta: Vec<f64> = self.fit.coefficients.iter().map(|c| c.estimate).collect();
            let df = self.fit.family.estimates_dispersion().then_some(self.fit.df_residual as f64);
            let inference = stat_linear::marginal::WaldInference { beta: &beta, covariance: &self.fit.covariance, df };
            let scale = (self.fit.link != Link::Identity).then(|| format!("{} link scale", self.fit.link.label()));
            out.extend(crate::marginal_sections::sections(
                &self.design,
                &inference,
                self.request.slices,
                self.request.slopes,
                self.fit.confidence_level,
                scale.as_deref(),
            ));
        }
        out.extend([self.fitted_plot(), crate::report::prediction_formula_section(self.prediction_formula())]);
        out
    }

    fn summary(&self) -> ReportSection {
        let f = &self.fit;
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut row = |a: &str, b: String| t.push([a.to_owned(), b]);
        row("Distribution", f.label());
        if let Some(b) = &self.data.binary {
            row("Event modelled", format!("{} = {} (versus {})", self.data.response_name, b.event, b.other));
        }
        row("Observations used", self.y.len().to_string());
        if self.data.n_excluded > 0 {
            row("Rows excluded", self.data.n_excluded.to_string());
        }
        if self.data.n_missing > 0 {
            row("Rows with missing values", self.data.n_missing.to_string());
        }
        row("Residual deviance", format!("{} on {} df", format_number(f.deviance, DIGITS), f.df_residual));
        row("Null deviance", format!("{} on {} df", format_number(f.null_deviance, DIGITS), f.df_null));
        let df = f.df_null - f.df_residual;
        let chi = ((f.null_deviance - f.deviance) / f.dispersion).max(0.0);
        let p = ChiSquared::new(df as f64).and_then(|c| c.sf(chi)).unwrap_or(f64::NAN);
        row("Whole-model test (χ²)", format!("{} on {df} df, p = {}", format_number(chi, DIGITS), format_p_value(p)));
        if f.family == Family::Binomial {
            row("Pseudo-R² (McFadden)", format_number(1.0 - f.deviance / f.null_deviance, 4));
        }
        row("AIC", format_number(f.aic, DIGITS));
        row("BIC", format_number(f.bic, DIGITS));
        if f.family == Family::Gaussian {
            row("Dispersion (σ²)", format_number(f.dispersion, DIGITS));
        }
        if f.family == Family::Gamma {
            row("Dispersion φ (Pearson)", format_number(f.dispersion, DIGITS));
            row("Shape 1/φ", format_number(1.0 / f.dispersion, DIGITS));
        }
        for w in &f.warnings {
            row("Warning", w.clone());
        }
        ReportSection {
            kind: SectionKind::ModelSummary,
            title: "Model Summary".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    fn estimates(&self) -> ReportSection {
        let f = &self.fit;
        let level = format_number(100.0 * f.confidence_level, 4);
        let stat = if f.family.estimates_dispersion() { "t" } else { "z" };
        let ratio = match (f.family, f.link) {
            (Family::Binomial, Link::Logit) => Some("Odds ratio"),
            (Family::Poisson, Link::Log) => Some("Rate ratio"),
            (_, Link::Log) => Some("Mean ratio"),
            _ => None,
        };
        let mut heads: Vec<String> =
            vec!["Term".into(), "Estimate".into(), "Std Error".into(), stat.into(), "p-value".into()];
        heads.push(format!("Lower {level}%"));
        heads.push(format!("Upper {level}%"));
        if let Some(r) = ratio {
            heads.push(r.to_owned());
            heads.push(format!("{r} lower {level}%"));
            heads.push(format!("{r} upper {level}%"));
        }
        let mut t = TextTable::new(heads);
        for (i, c) in f.coefficients.iter().enumerate() {
            let mut row = vec![
                c.name.clone(),
                format_number(c.estimate, DIGITS),
                format_number(c.std_error, DIGITS),
                format_number(c.statistic, 4),
                format_p_value(c.p_value),
                format_number(c.confidence_interval.0, DIGITS),
                format_number(c.confidence_interval.1, DIGITS),
            ];
            if ratio.is_some() {
                if i == 0 && c.name == "Intercept" {
                    row.extend([String::new(), String::new(), String::new()]);
                } else {
                    row.push(format_number(c.estimate.exp(), DIGITS));
                    row.push(format_number(c.confidence_interval.0.exp(), DIGITS));
                    row.push(format_number(c.confidence_interval.1.exp(), DIGITS));
                }
            }
            t.push(row);
        }
        let mut title = "Parameter Estimates (Wald tests)".to_owned();
        let refs: Vec<String> = self
            .design
            .terms()
            .iter()
            .filter_map(|term| match &term.kind {
                stat_linear::TermKind::Categorical { levels } => Some(format!("{} = {}", term.name, levels[0])),
                stat_linear::TermKind::Continuous => None,
            })
            .collect();
        if !refs.is_empty() {
            title = format!("{title}; reference levels: {}", refs.join(", "));
        }
        ReportSection { kind: SectionKind::GlmEstimates, title, content: SectionContent::Table(t) }
    }

    fn term_table(&self) -> ReportSection {
        let mut t = TextTable::new(["Term", "DF", "χ²", "p-value"]);
        for (name, df, chi, p) in &self.term_tests {
            t.push([name.clone(), df.to_string(), format_number(*chi, DIGITS), format_p_value(*p)]);
        }
        ReportSection {
            kind: SectionKind::LikelihoodRatioTests,
            title: "Likelihood-Ratio Tests (each term removed)".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    fn fitted_plot(&self) -> ReportSection {
        let f = &self.fit;
        let rows = &self.data.rows_used;
        let response = match &self.data.binary {
            Some(b) => format!("{} = {} (1) or {} (0)", self.data.response_name, b.event, b.other),
            None => self.data.response_name.clone(),
        };
        if let Some(xs) = self.data.single_continuous() {
            // The fitted curve over the predictor's range, with the data.
            let (lo, hi) = xs.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
            let grid: Vec<f64> =
                (0..CURVE_POINTS).map(|i| lo + (hi - lo) * i as f64 / (CURVE_POINTS - 1) as f64).collect();
            let points: Vec<Vec<TermValue>> = grid.iter().map(|&g| vec![TermValue::Number(g)]).collect();
            let line = self
                .encoded(&points)
                .and_then(|g| f.predict(&g))
                .map(|p| grid.iter().zip(p).map(|(&x, y)| [x, y]).collect())
                .ok();
            let spec = PlotSpec {
                x_time: None,
                y_time: None,
                x_label: self.data.predictor_names[0].clone(),
                y_label: response,
                points: rows
                    .iter()
                    .zip(xs.iter().zip(&self.y))
                    .map(|(&row, (&x, &y))| RowPoint { row, x, y })
                    .collect(),
                line,
                line_name: Some("Fitted mean".to_owned()),
                band: None,
                groups: None,
                x_ticks: None,
                panes: Vec::new(),
                annotations: None,
                segments: Vec::new(),
                ..PlotSpec::default()
            };
            return ReportSection {
                kind: SectionKind::FittedValues,
                title: "Fitted Curve".to_owned(),
                content: SectionContent::Plot(Box::new(spec)),
            };
        }
        let fitted = &f.fitted;
        let (lo, hi) =
            fitted.iter().chain(&self.y).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
        let spec = PlotSpec {
            x_time: None,
            y_time: None,
            x_label: "Fitted mean".to_owned(),
            y_label: response,
            points: rows
                .iter()
                .zip(fitted.iter().zip(&self.y))
                .map(|(&row, (&x, &y))| RowPoint { row, x, y })
                .collect(),
            line: (lo < hi).then(|| vec![[lo, lo], [hi, hi]]),
            line_name: Some("Reference line y = x".to_owned()),
            band: None,
            groups: None,
            x_ticks: None,
            panes: Vec::new(),
            annotations: None,
            segments: Vec::new(),
            ..PlotSpec::default()
        };
        ReportSection {
            kind: SectionKind::FittedValues,
            title: "Observed vs Fitted".to_owned(),
            content: SectionContent::Plot(Box::new(spec)),
        }
    }
}

impl crate::surface_profiler::SurfaceModel for GlmAnalysis {
    fn response_name(&self) -> &str {
        &self.profile_name
    }

    fn profile_factors(&self) -> Vec<crate::surface_profiler::ProfileFactor> {
        self.data.profile_factors(&self.design)
    }

    fn observations(&self) -> Vec<crate::surface_profiler::Observation> {
        self.data.observations(&self.design)
    }

    fn default_profile_settings(&self) -> Vec<TermValue> {
        self.data.default_settings(&self.design)
    }

    fn predict_settings(&self, points: &[Vec<TermValue>]) -> Result<Vec<f64>> {
        if points.is_empty() {
            return Ok(Vec::new());
        }
        self.fit.predict(&self.encoded(points)?)
    }

    fn predict_with_limits(&self, points: &[Vec<TermValue>]) -> Result<Vec<crate::surface_profiler::Limited>> {
        if points.is_empty() {
            return Ok(Vec::new());
        }
        Ok(self
            .fit
            .predict_with_limits(&self.encoded(points)?)?
            .into_iter()
            // NaN limits (an inverse-link interval through η = 0) have no band.
            .map(|(m, lo, hi)| (m, (lo.is_finite() && hi.is_finite()).then_some((lo, hi))))
            .collect())
    }

    fn confidence_level(&self) -> Option<f64> {
        Some(self.fit.confidence_level)
    }

    fn profile_note(&self) -> Option<String> {
        Some(match (self.request.family, self.fit.link) {
            (Family::Binomial, Link::Logit) => {
                "Predicted probability, with Wald limits computed on the log-odds scale and transformed.".to_owned()
            }
            (Family::Poisson, Link::Log) => {
                "Predicted mean count, with Wald limits computed on the log scale and transformed.".to_owned()
            }
            (_, Link::Identity) => "Predicted mean, with Wald limits.".to_owned(),
            (family, link) => format!(
                "Predicted {}, with Wald limits computed on the {} scale and transformed.",
                if family == Family::Binomial { "probability" } else { "mean" },
                link.label()
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::{CellValue, Column};

    fn table() -> DataTable {
        let dose = vec![1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0, 4.5, 5.0, 5.5, 6.0, 6.5, 7.0, 7.5, 8.0, 8.5];
        let group: Vec<String> = (0..16).map(|i| if i % 2 == 0 { "a" } else { "b" }.to_owned()).collect();
        let outcome: Vec<String> = [0, 0, 0, 1, 0, 0, 1, 0, 1, 1, 0, 1, 1, 1, 1, 1]
            .iter()
            .map(|&v| if v == 1 { "yes" } else { "no" }.to_owned())
            .collect();
        let counts = vec![2.0, 3.0, 6.0, 7.0, 8.0, 9.0, 10.0, 12.0, 15.0, 1.0, 2.0, 4.0, 5.0, 7.0, 8.0, 11.0];
        DataTable::new(
            "t",
            vec![
                Column::numeric("dose", dose),
                Column::text("group", group),
                Column::text("outcome", outcome),
                Column::numeric("count", counts),
            ],
        )
        .unwrap()
    }

    #[test]
    fn logistic_with_text_response_and_factor() {
        let t = table();
        let req = GlmRequest {
            response: 2,
            predictors: vec![0, 1],
            family: Family::Binomial,
            link: None,
            event: None,
            effects: Vec::new(),
            splines: Vec::new(),
            slices: None,
            slopes: None,
        };
        let a = GlmAnalysis::run(&t, &RowStateManager::new(16), &req).unwrap();
        assert_eq!(a.title(), "Logistic Regression: P(outcome = yes) ~ dose + group");
        // Same data as stat-discrete's reference test: group b coded as the x2 indicator.
        assert!((a.fit().coefficients[1].estimate - 0.812_576_13).abs() < 1e-6);
        let s = a.sections();
        let SectionContent::Table(est) = &s[2].content else { panic!() };
        assert!(est.columns.contains(&"Odds ratio".to_owned()));
        assert_eq!(est.cell(2, "Term"), Some("group: b"));
        let SectionContent::Table(lr) = &s[3].content else { panic!() };
        assert_eq!(lr.cell(1, "Term"), Some("group"));
        assert!(s[2].title.contains("reference levels: group = a"));
        let fitted = s.iter().find(|x| x.kind == SectionKind::FittedValues).unwrap();
        assert_eq!(fitted.title, "Observed vs Fitted");
        let SectionContent::Plot(p) = &fitted.content else { panic!() };
        assert_eq!(p.line_label(), "Reference line y = x");
        assert!(p.line.as_ref().unwrap().iter().all(|q| (q[0] - q[1]).abs() < 1e-12));
    }

    /// Choosing the other value as the event models its probability: every
    /// logit coefficient changes sign (the log odds of "no" are minus those
    /// of "yes"), the deviance is unchanged, and an unknown event is refused.
    #[test]
    fn interval_headers_name_the_fits_level() {
        let t = table();
        let req = GlmRequest {
            response: 2,
            predictors: vec![0, 1],
            family: Family::Binomial,
            link: None,
            event: None,
            effects: Vec::new(),
            splines: Vec::new(),
            slices: None,
            slopes: None,
        };
        let mut a = GlmAnalysis::run(&t, &RowStateManager::new(16), &req).unwrap();
        let SectionContent::Table(est) = &a.sections()[2].content else { panic!() };
        assert!(est.columns.contains(&"Lower 95%".to_owned()));
        a.fit.confidence_level = 0.90;
        let SectionContent::Table(est) = &a.sections()[2].content else { panic!() };
        assert!(est.columns.contains(&"Lower 90%".to_owned()) && est.columns.contains(&"Upper 90%".to_owned()));
    }

    #[test]
    fn chosen_event_reverses_the_logit() {
        let t = table();
        let rows = RowStateManager::new(16);
        let yes = GlmRequest {
            response: 2,
            predictors: vec![0, 1],
            family: Family::Binomial,
            link: None,
            event: None,
            effects: Vec::new(),
            splines: Vec::new(),
            slices: None,
            slopes: None,
        };
        let no = GlmRequest { event: Some("no".to_owned()), ..yes.clone() };
        let (a, b) = (GlmAnalysis::run(&t, &rows, &yes).unwrap(), GlmAnalysis::run(&t, &rows, &no).unwrap());
        assert_eq!(b.title(), "Logistic Regression: P(outcome = no) ~ dose + group");
        for (x, y) in a.fit().coefficients.iter().zip(&b.fit().coefficients) {
            assert!((x.estimate + y.estimate).abs() < 1e-8, "{} vs {}", x.estimate, y.estimate);
        }
        assert!((a.fit().deviance - b.fit().deviance).abs() < 1e-8);
        // Naming the default event changes nothing.
        let explicit = GlmRequest { event: Some("yes".to_owned()), ..yes.clone() };
        let c = GlmAnalysis::run(&t, &rows, &explicit).unwrap();
        assert!((c.fit().coefficients[1].estimate - a.fit().coefficients[1].estimate).abs() < 1e-12);
        let unknown = GlmRequest { event: Some("maybe".to_owned()), ..yes };
        let err = GlmAnalysis::run(&t, &rows, &unknown).unwrap_err().to_string();
        assert!(err.contains("maybe") && err.contains("no and yes"), "{err}");
    }

    /// A third response label in an excluded row, or in a row missing a
    /// predictor, does not stop a binomial fit; the fit is that of the
    /// other rows.
    #[test]
    fn binomial_levels_come_from_the_rows_used() {
        let t = table();
        let req = GlmRequest {
            response: 2,
            predictors: vec![0],
            family: Family::Binomial,
            link: None,
            event: None,
            effects: Vec::new(),
            splines: Vec::new(),
            slices: None,
            slopes: None,
        };
        let reference = GlmAnalysis::run(&t, &RowStateManager::new(16), &req).unwrap();
        let mut extended = t.clone();
        extended
            .insert_rows(vec![
                (
                    16,
                    vec![
                        CellValue::Number(2.0),
                        CellValue::Text("a".into()),
                        CellValue::Text("unsure".into()),
                        CellValue::Number(1.0),
                    ],
                ),
                (
                    17,
                    vec![
                        CellValue::Number(f64::NAN),
                        CellValue::Text("b".into()),
                        CellValue::Text("maybe".into()),
                        CellValue::Number(1.0),
                    ],
                ),
            ])
            .unwrap();
        let mut rows = RowStateManager::new(18);
        rows.set_excluded([16], true);
        let a = GlmAnalysis::run(&extended, &rows, &req).unwrap();
        assert_eq!(a.title(), reference.title());
        for (c, r) in a.fit().coefficients.iter().zip(&reference.fit().coefficients) {
            assert!((c.estimate - r.estimate).abs() < 1e-9, "{} vs {}", c.estimate, r.estimate);
        }
        // An included, complete row with a third label is still refused.
        assert!(GlmAnalysis::run(&extended, &RowStateManager::new(18), &req).is_err());
    }

    #[test]
    fn poisson_and_fitted_curve() {
        let t = table();
        let req = GlmRequest {
            response: 3,
            predictors: vec![0],
            family: Family::Poisson,
            link: None,
            event: None,
            effects: Vec::new(),
            splines: Vec::new(),
            slices: None,
            slopes: None,
        };
        let a = GlmAnalysis::run(&t, &RowStateManager::new(16), &req).unwrap();
        let s = a.sections();
        assert_eq!(s[4].title, "Fitted Curve");
        let SectionContent::Plot(p) = &s[4].content else { panic!() };
        let line = p.line.as_ref().unwrap();
        assert!(line.windows(2).all(|w| w[1][1] > w[0][1]), "counts increase with dose here");
        assert_eq!(p.line_label(), "Fitted mean");
        let bad = GlmRequest {
            response: 1,
            predictors: vec![0],
            family: Family::Poisson,
            link: None,
            event: None,
            effects: Vec::new(),
            splines: Vec::new(),
            slices: None,
            slopes: None,
        };
        assert!(GlmAnalysis::run(&t, &RowStateManager::new(16), &bad).is_err(), "text response");
    }

    #[test]
    fn probit_and_gamma_requests() {
        let t = table();
        let req = GlmRequest {
            response: 2,
            predictors: vec![0],
            family: Family::Binomial,
            link: Some(Link::Probit),
            event: None,
            effects: Vec::new(),
            splines: Vec::new(),
            slices: None,
            slopes: None,
        };
        let a = GlmAnalysis::run(&t, &RowStateManager::new(16), &req).unwrap();
        assert_eq!(a.title(), "Probit Regression: P(outcome = yes) ~ dose");
        assert_eq!(a.fit().link, Link::Probit);
        let formula = a.prediction_formula().unwrap();
        assert!(formula.starts_with("normal_cdf("), "{formula}");
        let s = a.sections();
        let SectionContent::Table(est) = &s[2].content else { panic!() };
        assert!(!est.columns.contains(&"Odds ratio".to_owned()), "odds ratios are for the logit link");
        let req = GlmRequest {
            response: 3,
            predictors: vec![0],
            family: Family::Gamma,
            link: Some(Link::Log),
            event: None,
            effects: Vec::new(),
            splines: Vec::new(),
            slices: None,
            slopes: None,
        };
        let a = GlmAnalysis::run(&t, &RowStateManager::new(16), &req).unwrap();
        assert_eq!(a.title(), "Gamma Regression (log link): count ~ dose");
        assert!(a.prediction_formula().unwrap().starts_with("exp("));
        let s = a.sections();
        let SectionContent::Table(sum) = &s[0].content else { panic!() };
        assert_eq!(sum.cell(0, "Value"), Some("Gamma (log link)"));
    }
}
