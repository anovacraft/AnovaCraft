//! Ordinal and nominal logistic regression bound to a dataset
//! ([`stat_discrete::polytomous`]).
//!
//! The response is a column of categories (text or numbers). For the
//! **ordinal** (proportional-odds) model they are ordered — by default in
//! natural order (numerically when every label is a number), or in the
//! order the request lists — and the model is
//! `P(Y ≤ j) = logistic(αⱼ − xᵀβ)`, so a positive coefficient favours higher
//! categories. For the **nominal** (baseline-category) model each category
//! but a reference one gets its own equation
//! `ln(P(Y = j)/P(Y = reference)) = β₀ⱼ + xᵀβⱼ`.
//!
//! Predictors and effects are coded as in the linear model (continuous
//! predictors as they are, categorical ones as indicators, interactions and
//! powers centred, sum-to-zero coding when a categorical predictor enters a
//! higher-order effect). Each effect gets a likelihood-ratio test from
//! refitting without its columns.

use crate::format::{format_number, format_p_value};
use crate::model_data::ModelData;
use crate::project::{SavedReport, invalid, s};
use crate::report::{PlotSpec, PointGroups, ReportSection, RowPoint, SectionContent, SectionKind, TextTable};
use crate::report_columns::ReportColumn;
use crate::script_text::{number, string};
use stat_core::distributions::{ChiSquared, ContinuousDistribution};
use stat_core::{Matrix, Result};
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_discrete::{
    GlmCoefficient, NominalLogitFit, OrdinalLogitFit, PolytomousOptions, PolytomousStatistics, fit_nominal_logistic,
    fit_ordinal_logistic,
};
use stat_linear::{ModelDesign, TermKind, TermValue};

const DIGITS: usize = 6;
const CURVE_POINTS: usize = 80;

/// Which model a categorical response gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CategoricalModel {
    /// Proportional odds (cumulative logit) for ordered categories.
    Ordinal,
    /// Baseline-category (multinomial) logits for unordered categories.
    Nominal,
}

impl CategoricalModel {
    /// The `analysis` name in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Ordinal => "ordinal-logistic",
            Self::Nominal => "nominal-logistic",
        }
    }

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Ordinal => "Ordinal logistic regression",
            Self::Nominal => "Nominal logistic regression",
        }
    }
}

/// Columns, effects and options of an ordinal or nominal logistic regression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoricalLogisticRequest {
    /// Response column (categories).
    pub response: usize,
    /// Predictor columns (at least one).
    pub predictors: Vec<usize>,
    /// Effects as `(position in predictors, power)` products; empty means
    /// main effects.
    pub effects: Vec<Vec<(usize, u8)>>,
    /// Ordinal or nominal model.
    pub model: CategoricalModel,
    /// Order of the response categories, lowest first (categories it does
    /// not list follow in natural order; empty: natural order).
    pub order: Vec<String>,
    /// Nominal model: the reference category (`None`: the first category).
    pub reference: Option<String>,
}

impl CategoricalLogisticRequest {
    /// A main-effects model of `response` on `predictors`.
    #[must_use]
    pub fn new(model: CategoricalModel, response: usize, predictors: Vec<usize>) -> Self {
        Self { response, predictors, effects: Vec::new(), model, order: Vec::new(), reference: None }
    }

    /// Every column the model uses.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        std::iter::once(self.response).chain(self.predictors.iter().copied()).collect()
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.response = f(self.response);
        self.predictors.iter_mut().for_each(|c| *c = f(*c));
    }

    fn effective_effects(&self) -> Vec<Vec<(usize, u8)>> {
        if self.effects.is_empty() { stat_linear::main_effects(self.predictors.len()) } else { self.effects.clone() }
    }
}

#[derive(Debug, Clone)]
enum Fit {
    Ordinal(OrdinalLogitFit),
    Nominal(NominalLogitFit),
}

/// A fitted ordinal or nominal logistic regression and what its report needs.
#[derive(Debug, Clone)]
pub struct CategoricalLogisticAnalysis {
    request: CategoricalLogisticRequest,
    data: ModelData,
    design: ModelDesign,
    fit: Fit,
    /// Category code of each row used.
    y: Vec<usize>,
    /// (effect, df, χ², p) for each effect.
    term_tests: Vec<(String, usize, f64, f64)>,
    /// Names of the table's columns, so formula variables avoid them.
    column_names: Vec<String>,
}

impl CategoricalLogisticAnalysis {
    /// Fit the model to the complete, included rows.
    ///
    /// # Errors
    /// No predictors, a response with fewer than two categories among the rows
    /// used, an unknown reference category, invalid effects, or errors from
    /// the fit (e.g. linearly dependent columns).
    pub fn run(table: &DataTable, states: &RowStateManager, request: &CategoricalLogisticRequest) -> Result<Self> {
        if request.predictors.is_empty() {
            return Err(invalid("choose at least one predictor"));
        }
        let data = ModelData::gather_categorical(table, states, request.response, &request.predictors, &request.order)?;
        let rows = data.rows_used.clone();
        if rows.is_empty() {
            return Err(invalid("no rows have values for every column of the model"));
        }
        let levels = data.categories.clone();
        if levels.len() < 2 {
            return Err(invalid(format!(
                "the response {:?} needs at least two categories among the rows used",
                data.response_name
            )));
        }
        let preds = data.borrowed();
        let design = ModelDesign::with_effects(&preds, &rows, &request.effective_effects())?;
        let x = design.matrix(&preds, &rows)?;
        let y: Vec<usize> = rows.iter().map(|&r| data.y_all[r] as usize).collect();
        let reference = match (&request.model, &request.reference) {
            (CategoricalModel::Nominal, Some(label)) => levels.iter().position(|l| l == label).ok_or_else(|| {
                invalid(format!("the reference category {label:?} is not a category of {:?}", data.response_name))
            })?,
            _ => 0,
        };
        let names = design.column_names().to_vec();
        let options = PolytomousOptions { names: names.clone(), ..PolytomousOptions::default() };
        let fit_with = |x: &Matrix, names: Vec<String>| -> Result<(Fit, f64)> {
            let options = PolytomousOptions { names, ..PolytomousOptions::default() };
            Ok(match request.model {
                CategoricalModel::Ordinal => {
                    let f = fit_ordinal_logistic(x, &y, &levels, &options)?;
                    let ll = f.statistics.log_likelihood;
                    (Fit::Ordinal(f), ll)
                }
                CategoricalModel::Nominal => {
                    let f = fit_nominal_logistic(x, &y, &levels, reference, &options)?;
                    let ll = f.statistics.log_likelihood;
                    (Fit::Nominal(f), ll)
                }
            })
        };
        let (fit, ll) = fit_with(&x, options.names)?;
        let per_column = match request.model {
            CategoricalModel::Ordinal => 1,
            CategoricalModel::Nominal => levels.len() - 1,
        };
        let mut term_tests = Vec::new();
        for effect in design.effects() {
            let keep: Vec<usize> = (0..x.ncols()).filter(|j| !effect.columns.contains(j)).collect();
            let cols: Vec<&[f64]> = keep.iter().map(|&j| x.col(j)).collect();
            let reduced_x = if cols.is_empty() { Matrix::zeros(rows.len(), 0) } else { Matrix::from_columns(&cols)? };
            let reduced_names = keep.iter().map(|&j| names[j].clone()).collect();
            let (_, reduced_ll) = fit_with(&reduced_x, reduced_names)?;
            let df = effect.columns.len() * per_column;
            let chi = (2.0 * (ll - reduced_ll)).max(0.0);
            term_tests.push((effect.name.clone(), df, chi, ChiSquared::new(df as f64)?.sf(chi)?));
        }
        Ok(Self {
            request: request.clone(),
            data,
            design,
            fit,
            y,
            term_tests,
            column_names: table.columns().iter().map(|c| c.name.clone()).collect(),
        })
    }

    /// The response categories, in model order.
    #[must_use]
    pub fn categories(&self) -> &[String] {
        &self.data.categories
    }

    /// The proportional-odds fit (ordinal model only).
    #[must_use]
    pub fn ordinal_fit(&self) -> Option<&OrdinalLogitFit> {
        match &self.fit {
            Fit::Ordinal(f) => Some(f),
            Fit::Nominal(_) => None,
        }
    }

    /// The baseline-category fit (nominal model only).
    #[must_use]
    pub fn nominal_fit(&self) -> Option<&NominalLogitFit> {
        match &self.fit {
            Fit::Nominal(f) => Some(f),
            Fit::Ordinal(_) => None,
        }
    }

    /// Likelihood-ratio test of each effect: `(effect, df, χ², p)`.
    #[must_use]
    pub fn term_tests(&self) -> &[(String, usize, f64, f64)] {
        &self.term_tests
    }

    fn statistics(&self) -> &PolytomousStatistics {
        match &self.fit {
            Fit::Ordinal(f) => &f.statistics,
            Fit::Nominal(f) => &f.statistics,
        }
    }

    fn probabilities(&self) -> &[Vec<f64>] {
        match &self.fit {
            Fit::Ordinal(f) => &f.probabilities,
            Fit::Nominal(f) => &f.probabilities,
        }
    }

    fn predict(&self, x: &Matrix) -> Result<Vec<Vec<f64>>> {
        match &self.fit {
            Fit::Ordinal(f) => f.predict(x),
            Fit::Nominal(f) => f.predict(x),
        }
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let kind = match self.request.model {
            CategoricalModel::Ordinal => "Ordinal Logistic Regression",
            CategoricalModel::Nominal => "Nominal Logistic Regression",
        };
        let names: Vec<&str> = self.data.predictor_names.iter().map(String::as_str).collect();
        let effects: Vec<String> =
            self.request.effective_effects().iter().map(|e| stat_linear::effect_name(&names, e)).collect();
        format!("{kind}: {} ~ {}", self.data.response_name, effects.join(" + "))
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
        out.extend(crate::report::hierarchy_note(&self.data.predictor_names, &self.request.effective_effects()));
        out.push(self.term_table());
        out.push(self.estimates());
        out.push(self.odds_ratios());
        out.extend(self.parallel_lines());
        out.push(self.classification());
        out.extend(self.probability_plot());
        out.push(match self.probability_formulas() {
            Ok(f) => ReportSection {
                kind: SectionKind::PredictionFormula,
                title: "Prediction Formulas".to_owned(),
                content: SectionContent::Formula(
                    self.data
                        .categories
                        .iter()
                        .zip(&f)
                        .map(|(c, f)| format!("Probability of {c}:\n{f}"))
                        .collect::<Vec<_>>()
                        .join("\n\n"),
                ),
            },
            Err(e) => crate::report::prediction_formula_section(Err(e)),
        });
        out
    }

    fn summary(&self) -> ReportSection {
        let st = self.statistics();
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut row = |a: &str, b: String| t.push([a.to_owned(), b]);
        let cats = self.data.categories.join(", ");
        match &self.fit {
            Fit::Ordinal(_) => {
                row("Model", "Proportional odds (cumulative logit)".to_owned());
                row("Categories (lowest first)", cats);
                row(
                    "Modelled",
                    format!(
                        "P({} ≤ category); positive coefficients favour higher categories",
                        self.data.response_name
                    ),
                );
            }
            Fit::Nominal(f) => {
                row("Model", "Baseline-category logits (multinomial)".to_owned());
                row("Categories", cats);
                row("Reference category", f.levels[f.reference].clone());
            }
        }
        row("Observations used", self.y.len().to_string());
        if self.data.n_excluded > 0 {
            row("Rows excluded", self.data.n_excluded.to_string());
        }
        if self.data.n_missing > 0 {
            row("Rows with missing values", self.data.n_missing.to_string());
        }
        row("−2 log-likelihood", format_number(-2.0 * st.log_likelihood, DIGITS));
        row("−2 log-likelihood, no predictors", format_number(-2.0 * st.null_log_likelihood, DIGITS));
        let w = &st.whole_model;
        row(
            "Whole-model test (likelihood ratio χ²)",
            format!("{} on {} df, p = {}", format_number(w.chi_square, DIGITS), w.df, format_p_value(w.p_value)),
        );
        row("Pseudo-R² (McFadden)", format_number(st.mcfadden_r2, 4));
        row("Pseudo-R² (Nagelkerke)", format_number(st.nagelkerke_r2, 4));
        row("AIC", format_number(st.aic, DIGITS));
        row("BIC", format_number(st.bic, DIGITS));
        row("Parameters", st.n_parameters.to_string());
        row("Iterations", st.iterations.to_string());
        for warning in &st.warnings {
            row("Warning", warning.clone());
        }
        ReportSection {
            kind: SectionKind::ModelSummary,
            title: "Model Summary".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    fn term_table(&self) -> ReportSection {
        let mut t = TextTable::new(["Effect", "DF", "χ²", "p-value"]);
        for (name, df, chi, p) in &self.term_tests {
            t.push([name.clone(), df.to_string(), format_number(*chi, DIGITS), format_p_value(*p)]);
        }
        ReportSection {
            kind: SectionKind::LikelihoodRatioTests,
            title: "Likelihood-Ratio Tests (each effect removed)".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    fn reference_levels(&self) -> String {
        let refs: Vec<String> = self
            .design
            .terms()
            .iter()
            .filter_map(|term| match &term.kind {
                TermKind::Categorical { levels } => Some(format!("{} = {}", term.name, levels[0])),
                TermKind::Continuous => None,
            })
            .collect();
        if refs.is_empty() || self.design.coding() == stat_linear::CategoricalCoding::SumToZero {
            String::new()
        } else {
            format!("; reference levels: {}", refs.join(", "))
        }
    }

    fn coefficient_cells(c: &GlmCoefficient) -> [String; 6] {
        [
            format_number(c.estimate, DIGITS),
            format_number(c.std_error, DIGITS),
            format_number(c.statistic, 4),
            format_p_value(c.p_value),
            format_number(c.confidence_interval.0, DIGITS),
            format_number(c.confidence_interval.1, DIGITS),
        ]
    }

    fn level_heads(&self) -> [String; 6] {
        let level = (self.statistics().confidence_level * 100.0).round();
        [
            "Estimate".into(),
            "Std Error".into(),
            "z".into(),
            "p-value".into(),
            format!("Lower {level}%"),
            format!("Upper {level}%"),
        ]
    }

    fn estimates(&self) -> ReportSection {
        let t = match &self.fit {
            Fit::Ordinal(f) => {
                let mut t = TextTable::new(["Term".to_owned()].into_iter().chain(self.level_heads()));
                for c in &f.thresholds {
                    t.push([format!("Threshold {}", c.name)].into_iter().chain(Self::coefficient_cells(c)));
                }
                for c in &f.slopes {
                    t.push([c.name.clone()].into_iter().chain(Self::coefficient_cells(c)));
                }
                t
            }
            Fit::Nominal(f) => {
                let mut t =
                    TextTable::new(["Category".to_owned(), "Term".to_owned()].into_iter().chain(self.level_heads()));
                for e in &f.equations {
                    for (k, c) in e.coefficients.iter().enumerate() {
                        let label = if k == 0 { f.levels[e.level].clone() } else { String::new() };
                        t.push([label, c.name.clone()].into_iter().chain(Self::coefficient_cells(c)));
                    }
                }
                t
            }
        };
        ReportSection {
            kind: SectionKind::GlmEstimates,
            title: format!("Parameter Estimates (Wald tests){}", self.reference_levels()),
            content: SectionContent::Table(t),
        }
    }

    fn odds_ratios(&self) -> ReportSection {
        let level = (self.statistics().confidence_level * 100.0).round();
        let heads = ["Odds ratio".to_owned(), format!("Lower {level}%"), format!("Upper {level}%")];
        let cells = |c: &GlmCoefficient| {
            [
                format_number(c.estimate.exp(), DIGITS),
                format_number(c.confidence_interval.0.exp(), DIGITS),
                format_number(c.confidence_interval.1.exp(), DIGITS),
            ]
        };
        let (t, title) = match &self.fit {
            Fit::Ordinal(f) => {
                let mut t = TextTable::new(["Term".to_owned()].into_iter().chain(heads));
                for c in &f.slopes {
                    t.push([c.name.clone()].into_iter().chain(cells(c)));
                }
                (t, "Odds Ratios (odds of a higher category, per unit or against the reference level)".to_owned())
            }
            Fit::Nominal(f) => {
                let mut t = TextTable::new(["Category".to_owned(), "Term".to_owned()].into_iter().chain(heads));
                for e in &f.equations {
                    for (k, c) in e.coefficients.iter().enumerate().skip(1) {
                        let label = if k == 1 { f.levels[e.level].clone() } else { String::new() };
                        t.push([label, c.name.clone()].into_iter().chain(cells(c)));
                    }
                }
                (t, format!("Odds Ratios (odds of each category against {})", f.levels[f.reference]))
            }
        };
        ReportSection { kind: SectionKind::OddsRatios, title, content: SectionContent::Table(t) }
    }

    fn parallel_lines(&self) -> Option<ReportSection> {
        let f = self.ordinal_fit()?;
        if f.levels.len() < 3 {
            return None;
        }
        let content = match f.parallel_lines {
            Some(test) => {
                let mut t = TextTable::new(["Test", "χ²", "DF", "p-value"]);
                t.push([
                    "Score test".to_owned(),
                    format_number(test.chi_square, DIGITS),
                    test.df.to_string(),
                    format_p_value(test.p_value),
                ]);
                SectionContent::Table(t)
            }
            None => SectionContent::Note("The score test could not be computed for this fit.".to_owned()),
        };
        Some(ReportSection {
            kind: SectionKind::ParallelLinesTest,
            title: "Parallel-Lines Test (small p: the effects differ between cumulative splits)".to_owned(),
            content,
        })
    }

    fn most_likely(p: &[f64]) -> usize {
        p.iter().enumerate().fold(0, |best, (k, &v)| if v > p[best] { k } else { best })
    }

    fn classification(&self) -> ReportSection {
        let cats = &self.data.categories;
        let mut counts = vec![vec![0usize; cats.len()]; cats.len()];
        for (&k, p) in self.y.iter().zip(self.probabilities()) {
            counts[k][Self::most_likely(p)] += 1;
        }
        let headers: Vec<String> = std::iter::once("Actual".to_owned())
            .chain(cats.iter().map(|c| format!("Predicted {c}")))
            .chain(std::iter::once("Correct".to_owned()))
            .collect();
        let mut t = TextTable::new(headers);
        let mut correct = 0;
        for (k, row) in counts.iter().enumerate() {
            let n: usize = row.iter().sum();
            correct += row[k];
            let share = if n == 0 { String::new() } else { format!("{:.1}%", 100.0 * row[k] as f64 / n as f64) };
            t.push(
                std::iter::once(cats[k].clone())
                    .chain(row.iter().map(ToString::to_string))
                    .chain(std::iter::once(share))
                    .collect::<Vec<_>>(),
            );
        }
        let rate = 100.0 * correct as f64 / self.y.len().max(1) as f64;
        ReportSection {
            kind: SectionKind::ConfusionMatrix,
            title: format!("Classification (actual by most likely category; {rate:.1}% correct)"),
            content: SectionContent::Table(t),
        }
    }

    /// Fitted probabilities of each category against the first continuous
    /// predictor with a main effect, the others at their mean or most
    /// frequent level; each row at its fitted probability of the category
    /// it has.
    fn probability_plot(&self) -> Option<ReportSection> {
        let terms = self.design.terms();
        let (index, term) =
            terms.iter().enumerate().find(|(_, t)| t.kind == TermKind::Continuous && !t.columns.is_empty())?;
        let crate::model_data::PredictorData::Continuous(values) = &self.data.predictors[index] else { return None };
        let used: Vec<f64> = self.data.rows_used.iter().map(|&r| values[r]).collect();
        let (lo, hi) = used.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
        if lo.is_nan() || hi.is_nan() || lo >= hi {
            return None;
        }
        let settings = self.data.default_settings(&self.design);
        let grid: Vec<f64> = (0..CURVE_POINTS).map(|i| lo + (hi - lo) * i as f64 / (CURVE_POINTS - 1) as f64).collect();
        let rows: Vec<Vec<f64>> = grid
            .iter()
            .map(|&v| {
                let mut s = settings.clone();
                s[index] = TermValue::Number(v);
                self.design.encode(&s)
            })
            .collect::<Result<_>>()
            .ok()?;
        let curves = self.predict(&Matrix::from_rows(&rows).ok()?).ok()?;
        let cats = &self.data.categories;
        let lines = (0..cats.len()).map(|k| grid.iter().zip(&curves).map(|(&x, p)| [x, p[k]]).collect()).collect();
        let points: Vec<RowPoint> = self
            .data
            .rows_used
            .iter()
            .zip(used.iter().zip(self.y.iter().zip(self.probabilities())))
            .map(|(&row, (&x, (&k, p)))| RowPoint { row, x, y: p[k] })
            .collect();
        let others = if terms.len() > 1 { " (other predictors at their mean or most frequent level)" } else { "" };
        let spec = PlotSpec {
            x_label: term.name.clone(),
            y_label: "Probability".to_owned(),
            points,
            groups: Some(PointGroups {
                title: self.data.response_name.clone(),
                labels: cats.clone(),
                membership: self.y.clone(),
                lines,
            }),
            ..PlotSpec::default()
        };
        Some(ReportSection {
            kind: SectionKind::ClassProbabilityPlot,
            title: format!("Predicted Probabilities by {}{others}", term.name),
            content: SectionContent::Plot(Box::new(spec)),
        })
    }

    /// A variable name for formulas that is no column's name.
    fn variable(&self, base: &str) -> String {
        let mut name = base.to_owned();
        while self.column_names.contains(&name) {
            name.push('_');
        }
        name
    }

    /// Linear-predictor formula text with the given intercept and slopes.
    fn eta(&self, intercept: f64, slopes: &[f64]) -> Result<String> {
        use crate::prediction_formula::{Link, prediction_formula};
        let text = prediction_formula(&self.design, &self.data.factors, intercept, slopes, Link::Identity)?;
        Ok(text.lines().map(|l| format!("    {l}")).collect::<Vec<_>>().join("\n"))
    }

    /// The `let` lines defining each category's probability `p1, p2, …` (as
    /// variable names, returned alongside).
    fn probability_lets(&self) -> Result<(Vec<String>, Vec<String>)> {
        let n = self.data.categories.len();
        let probs: Vec<String> = (1..=n).map(|k| self.variable(&format!("p{k}"))).collect();
        let mut lines = Vec::new();
        match &self.fit {
            Fit::Ordinal(f) => {
                let eta = self.variable("eta");
                let slopes: Vec<f64> = f.slopes.iter().map(|c| c.estimate).collect();
                lines.push(format!("let {eta} = (\n{}\n);", self.eta(0.0, &slopes)?));
                let cum = |j: usize| format!("logistic({} - {eta})", number(f.thresholds[j].estimate));
                for k in 0..n {
                    let value = if k == 0 {
                        cum(0)
                    } else if k == n - 1 {
                        format!("1.0 - {}", cum(n - 2))
                    } else {
                        format!("{} - {}", cum(k), cum(k - 1))
                    };
                    lines.push(format!("let {} = {value};", probs[k]));
                }
            }
            Fit::Nominal(f) => {
                let mut etas = vec![None; n];
                for e in &f.equations {
                    let name = self.variable(&format!("eta{}", e.level + 1));
                    let b: Vec<f64> = e.coefficients.iter().map(|c| c.estimate).collect();
                    lines.push(format!("let {name} = (\n{}\n);", self.eta(b[0], &b[1..])?));
                    etas[e.level] = Some(name);
                }
                let total = self.variable("total");
                let terms: Vec<String> = etas.iter().flatten().map(|e| format!("exp({e})")).collect();
                lines.push(format!("let {total} = 1.0 + {};", terms.join(" + ")));
                for (k, eta) in etas.iter().enumerate() {
                    let value = match eta {
                        Some(e) => format!("exp({e}) / {total}"),
                        None => format!("1.0 / {total}"),
                    };
                    lines.push(format!("let {} = {value};", probs[k]));
                }
            }
        }
        Ok((lines, probs))
    }

    /// Formula columns' text: each category's probability, in category order.
    ///
    /// # Errors
    /// If the linear predictor cannot be written (e.g. a date-time column
    /// used as a category).
    pub fn probability_formulas(&self) -> Result<Vec<String>> {
        let (lines, probs) = self.probability_lets()?;
        let body = lines.join("\n");
        Ok(probs.iter().map(|p| format!("{body}\n{p}")).collect())
    }

    /// Formula of the most likely category (ties go to the earlier category).
    ///
    /// # Errors
    /// As [`Self::probability_formulas`].
    pub fn most_likely_formula(&self) -> Result<String> {
        let (mut lines, probs) = self.probability_lets()?;
        let (best, top) = (self.variable("best"), self.variable("top"));
        let cats = &self.data.categories;
        lines.push(format!("let {best} = {};", string(&cats[0])));
        lines.push(format!("let {top} = {};", probs[0]));
        for (k, p) in probs.iter().enumerate().skip(1) {
            lines.push(format!("if {p} > {top} {{ {best} = {}; {top} = {p}; }}", string(&cats[k])));
        }
        lines.push(best);
        Ok(lines.join("\n"))
    }

    /// Columns the report can add: each category's probability
    /// (`probability_1`, `probability_2`, …) and the most likely category
    /// (`most_likely`), as formula columns, for every row with its predictors
    /// present at levels the model knows.
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        let n_rows = self.data.y_all.len();
        let cats = &self.data.categories;
        let preds = self.data.borrowed();
        let mut probs: Vec<Option<Vec<f64>>> = vec![None; n_rows];
        let encoded: Vec<(usize, Vec<f64>)> = (0..n_rows)
            .filter_map(|r| self.design.row(&preds, r).filter(|v| v.iter().all(|x| x.is_finite())).map(|v| (r, v)))
            .collect();
        if !encoded.is_empty() {
            let rows: Vec<&Vec<f64>> = encoded.iter().map(|(_, v)| v).collect();
            if let Ok(p) = Matrix::from_rows(&rows).and_then(|m| self.predict(&m)) {
                for ((r, _), p) in encoded.iter().zip(p) {
                    probs[*r] = Some(p);
                }
            }
        }
        let formulas = self.probability_formulas().ok();
        let name = &self.data.response_name;
        let mut out = Vec::with_capacity(cats.len() + 1);
        for (k, c) in cats.iter().enumerate() {
            let values = probs.iter().map(|p| p.as_ref().map_or(f64::NAN, |p| p[k])).collect();
            out.push(ReportColumn {
                formula: formulas.as_ref().map(|f| f[k].clone()),
                ..ReportColumn::continuous(
                    &format!("probability_{}", k + 1),
                    &format!("Save probability of {c}"),
                    format!("Prob {name} = {c}"),
                    values,
                )
            });
        }
        let most: Vec<String> =
            probs.iter().map(|p| p.as_ref().map_or(String::new(), |p| cats[Self::most_likely(p)].clone())).collect();
        let mut column =
            ReportColumn::text("most_likely", "Save most likely category", format!("Most likely {name}"), most);
        column.formula = self.most_likely_formula().ok();
        if self.request.model == CategoricalModel::Ordinal {
            column.level = stat_data::MeasurementLevel::Ordinal;
        }
        out.push(column);
        out
    }
}

/// The project-file (JSON) form of a request:
///
/// ```text
/// {"analysis": "ordinal-logistic", "response": "Rating", "predictors": ["Price", "Brand"],
///  "effects": [[[0, 1]], [[1, 1]], [[0, 1], [1, 1]]], "order": ["Poor", "Fair", "Good"]}
/// {"analysis": "nominal-logistic", "response": "Choice", "predictors": ["Age"], "reference": "Bus"}
/// ```
///
/// `effects` (absent: main effects), `order` (absent: natural order) and
/// `reference` (absent: the first category) are optional.
pub(crate) fn to_json(request: &CategoricalLogisticRequest, table: &DataTable) -> JsonValue {
    let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
    let mut members = vec![
        ("analysis", s(request.model.key())),
        ("response", col(request.response)),
        ("predictors", JsonValue::Array(request.predictors.iter().map(|&c| col(c)).collect())),
    ];
    if !request.effects.is_empty() {
        members.push(("effects", crate::project::effects_json(&request.effects)));
    }
    if !request.order.is_empty() {
        members.push(("order", JsonValue::Array(request.order.iter().map(|l| s(l)).collect())));
    }
    if let Some(r) = &request.reference {
        members.push(("reference", s(r)));
    }
    JsonValue::Object(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

/// Read a request written by [`to_json`] (`model` from its `analysis` name).
pub(crate) fn from_json(model: CategoricalModel, v: &JsonValue, table: &DataTable) -> Result<SavedReport> {
    let find = |item: &JsonValue| -> Result<usize> {
        let name = item.as_str().ok_or_else(|| invalid("expected a column name"))?;
        table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
    };
    let response = find(v.get("response").ok_or_else(|| invalid("the report has no response"))?)?;
    let predictors = match v.get("predictors") {
        Some(JsonValue::Array(items)) => items.iter().map(find).collect::<Result<_>>()?,
        _ => return Err(invalid("predictors must be a list of column names")),
    };
    let labels = |key: &str| -> Result<Vec<String>> {
        match v.get(key) {
            None | Some(JsonValue::Null) => Ok(Vec::new()),
            Some(JsonValue::Array(items)) => items
                .iter()
                .map(|l| l.as_str().map(str::to_owned).ok_or_else(|| invalid(format!("{key} must be labels"))))
                .collect(),
            Some(_) => Err(invalid(format!("{key} must be a list of labels"))),
        }
    };
    Ok(SavedReport::CategoricalLogistic {
        request: CategoricalLogisticRequest {
            response,
            predictors,
            effects: crate::project::linear_effects(v)?,
            model,
            order: labels("order")?,
            reference: v.get("reference").and_then(JsonValue::as_str).map(str::to_owned),
        },
    })
}

#[cfg(test)]
mod tests;
