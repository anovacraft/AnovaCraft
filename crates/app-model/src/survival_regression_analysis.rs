//! Parametric survival regression (accelerated failure time models) bound to
//! a dataset ([`stat_reliability::survival_regression`]).
//!
//! **Layouts** (as for life distributions): a time column with an optional
//! censor column, or start and end columns of the interval in which each
//! unit failed (empty start: failed by the end; empty end: still running at
//! the start; equal: an exact failure); an optional frequency column counts
//! units per row. Predictors may be continuous or categorical, with effects
//! coded as in the linear model.
//!
//! The report compares the Weibull, lognormal, log-logistic and exponential
//! models by AIC, and for the chosen one gives the whole-model and per-effect
//! likelihood-ratio tests with the Effect Pareto, coefficients with Wald and
//! profile-likelihood limits, time ratios (acceleration factors), predicted
//! quantiles and failure probabilities at chosen covariate settings, fitted
//! survival curves with the nonparametric estimate of each group, and a
//! Cox–Snell residual check. The profiler shows a predicted quantile.

use crate::format::{format_number, format_p_value};
use crate::life_distribution::LifeDataLayout;
use crate::model_data::{ModelData, PredictorData};
use crate::project::{SavedReport, invalid, s};
use crate::report::{
    LinePattern, PlotSpec, Polyline, ReportSection, RowPoint, SectionContent, SectionKind, SurvivalCurveSpec,
    SurvivalPlotSpec, TextTable,
};
use crate::report_columns::ReportColumn;
use crate::surface_profiler::{Limited, ProfileFactor, ProfilerOptions};
use crate::survival::StatusCoding;
use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_linear::{ModelDesign, TermKind, TermValue};
use stat_reliability::{
    AftDistribution, CensorKind, ChiSquareTest, LifeData, LifeObservation, SurvivalRegressionFit,
    SurvivalRegressionOptions, fit_survival_regression, turnbull,
};

const DIGITS: usize = 5;
const CURVE_POINTS: usize = 120;

fn num(v: f64) -> String {
    format_number(v, DIGITS)
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

fn value(table: &DataTable, col: usize, row: usize) -> f64 {
    table.column(col).and_then(|c| c.as_numeric()).map_or(f64::NAN, |v| v[row])
}

/// Columns, effects and options of a parametric survival regression.
#[derive(Debug, Clone, PartialEq)]
pub struct SurvivalRegressionRequest {
    /// Where the failure times come from.
    pub layout: LifeDataLayout,
    /// Optional frequency column (units per row).
    pub frequency: Option<usize>,
    /// Predictor columns (at least one).
    pub predictors: Vec<usize>,
    /// Effects as `(position in predictors, power)` products; empty means
    /// main effects.
    pub effects: Vec<Vec<(usize, u8)>>,
    /// Distribution of the failure time.
    pub distribution: AftDistribution,
    /// Confidence level of every interval.
    pub confidence: f64,
    /// Predictor whose levels (categorical) or 10th, 50th and 90th
    /// percentiles (continuous) get their own curve and predictions, the
    /// others at their average; `None`: the average unit only.
    pub curve_by: Option<usize>,
    /// Fractions failing whose times (quantiles) are predicted.
    pub probabilities: Vec<f64>,
    /// Times whose failure probabilities are predicted.
    pub times: Vec<f64>,
    /// Fraction failing whose time the profiler predicts.
    pub profile_probability: f64,
}

impl SurvivalRegressionRequest {
    /// Default confidence level.
    pub const DEFAULT_CONFIDENCE: f64 = 0.95;
    /// Default quantiles: 10% and 50% failing.
    pub const DEFAULT_PROBABILITIES: [f64; 2] = [0.1, 0.5];

    /// A main-effects Weibull model with default settings.
    #[must_use]
    pub fn new(layout: LifeDataLayout, predictors: Vec<usize>) -> Self {
        Self {
            layout,
            frequency: None,
            predictors,
            effects: Vec::new(),
            distribution: AftDistribution::Weibull,
            confidence: Self::DEFAULT_CONFIDENCE,
            curve_by: None,
            probabilities: Self::DEFAULT_PROBABILITIES.to_vec(),
            times: Vec::new(),
            profile_probability: 0.5,
        }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut out = match self.layout {
            LifeDataLayout::Times { time, censor, .. } => std::iter::once(time).chain(censor).collect(),
            LifeDataLayout::Intervals { start, end } => vec![start, end],
        };
        out.extend(self.frequency);
        out.extend(self.predictors.iter().copied());
        out
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        match &mut self.layout {
            LifeDataLayout::Times { time, censor, .. } => {
                *time = f(*time);
                *censor = censor.map(f);
            }
            LifeDataLayout::Intervals { start, end } => {
                *start = f(*start);
                *end = f(*end);
            }
        }
        self.frequency = self.frequency.map(f);
        self.predictors.iter_mut().for_each(|c| *c = f(*c));
        self.curve_by = self.curve_by.map(f);
    }

    pub(crate) fn effective_effects(&self) -> Vec<Vec<(usize, u8)>> {
        if self.effects.is_empty() { stat_linear::main_effects(self.predictors.len()) } else { self.effects.clone() }
    }
}

/// One curve: its label, settings, and the rows it describes (for the
/// nonparametric estimate; empty when not a categorical level).
#[derive(Debug, Clone)]
struct CurveSetting {
    label: String,
    settings: Vec<TermValue>,
    rows: Vec<usize>,
}

/// A fitted accelerated failure time model and what its report needs.
#[derive(Debug, Clone)]
pub struct SurvivalRegressionAnalysis {
    request: SurvivalRegressionRequest,
    data: ModelData,
    design: ModelDesign,
    /// Observation of each row used.
    observations: Vec<LifeObservation>,
    fit: SurvivalRegressionFit,
    /// The other distributions' fits, for the comparison (in `ALL` order).
    others: Vec<(AftDistribution, std::result::Result<SurvivalRegressionFit, String>)>,
    time_name: String,
    whole_model: ChiSquareTest,
    effect_tests: Vec<(String, ChiSquareTest)>,
    curves: Vec<CurveSetting>,
    profile_name: String,
}

/// The observation in row `r`, or `None` when it is unusable.
fn observation(table: &DataTable, request: &SurvivalRegressionRequest, r: usize) -> Option<LifeObservation> {
    let weight = request.frequency.map_or(1.0, |c| value(table, c, r));
    if !(weight.is_finite() && weight > 0.0) {
        return None;
    }
    match request.layout {
        LifeDataLayout::Times { time, censor, coding } => {
            let t = value(table, time, r);
            if !(t.is_finite() && t > 0.0) {
                return None;
            }
            let censored = match censor {
                Some(c) => {
                    let s = value(table, c, r);
                    if !s.is_finite() {
                        return None;
                    }
                    (s != 0.0) == (coding == StatusCoding::NonzeroIsCensored)
                }
                None => false,
            };
            Some(if censored { LifeObservation::right(t, weight) } else { LifeObservation::exact(t, weight) })
        }
        LifeDataLayout::Intervals { start, end } => {
            let (a, b) = (value(table, start, r), value(table, end, r));
            let lo = if a.is_finite() { a } else { f64::NEG_INFINITY };
            let hi = if b.is_finite() { b } else { f64::INFINITY };
            if (!lo.is_finite() && !hi.is_finite()) || lo > hi || (lo == hi && lo <= 0.0) || hi <= 0.0 {
                return None;
            }
            Some(LifeObservation::interval(lo, hi, weight))
        }
    }
}

impl SurvivalRegressionAnalysis {
    /// Fit the model to the complete, included rows.
    ///
    /// # Errors
    /// No predictors, a column used twice, non-numeric time columns, a
    /// confidence level outside (0.5, 1), a profile probability outside
    /// (0, 1), invalid effects, or errors from the fit.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &SurvivalRegressionRequest) -> Result<Self> {
        if request.predictors.is_empty() {
            return Err(invalid("choose at least one predictor"));
        }
        let cols = request.columns();
        if (1..cols.len()).any(|i| cols[..i].contains(&cols[i])) {
            return Err(invalid("choose a different column for each role"));
        }
        let column = |i: usize| table.column(i).ok_or_else(|| invalid(format!("no column {i}")));
        let numeric: Vec<usize> = match request.layout {
            LifeDataLayout::Times { time, censor, .. } => std::iter::once(time).chain(censor).collect(),
            LifeDataLayout::Intervals { start, end } => vec![start, end],
        };
        for c in numeric.iter().copied().chain(request.frequency) {
            let col = column(c)?;
            if col.as_numeric().is_none() {
                return Err(invalid(format!("{} must be numeric", col.name)));
            }
        }
        if !(request.confidence > 0.5 && request.confidence < 1.0) {
            return Err(invalid("the confidence level must be between 0.5 and 1"));
        }
        if !(request.profile_probability > 0.0 && request.profile_probability < 1.0) {
            return Err(invalid("the profiled fraction failing must be between 0 and 1"));
        }
        if request.probabilities.iter().any(|p| !(*p > 0.0 && *p < 1.0)) {
            return Err(invalid("fractions failing must be between 0 and 1"));
        }
        if request.curve_by.is_some_and(|c| !request.predictors.contains(&c)) {
            return Err(invalid("the column giving separate curves must be one of the predictors"));
        }
        let time_name = match request.layout {
            LifeDataLayout::Times { time, .. } => column(time)?.name.clone(),
            LifeDataLayout::Intervals { start, end } => format!("{}–{}", column(start)?.name, column(end)?.name),
        };
        let obs_all: Vec<Option<LifeObservation>> =
            (0..table.n_rows()).map(|r| observation(table, request, r)).collect();
        let y_all = obs_all.iter().map(|o| if o.is_some() { 1.0 } else { f64::NAN }).collect();
        let data = ModelData::gather_with_response(table, states, time_name.clone(), y_all, &request.predictors)?;
        let rows = data.rows_used.clone();
        if rows.is_empty() {
            return Err(invalid("no rows have usable times and values for every predictor"));
        }
        let observations: Vec<LifeObservation> = rows.iter().filter_map(|&r| obs_all[r]).collect();
        let preds = data.borrowed();
        let design = ModelDesign::with_effects(&preds, &rows, &request.effective_effects())?;
        let x = design.matrix(&preds, &rows)?;
        let options = |distribution, profile_intervals| SurvivalRegressionOptions {
            distribution,
            names: design.column_names().to_vec(),
            level: request.confidence,
            profile_intervals,
        };
        let fit = fit_survival_regression(&observations, &x, &options(request.distribution, true))?;
        let others = AftDistribution::ALL
            .into_iter()
            .filter(|&d| d != request.distribution)
            .map(|d| (d, fit_survival_regression(&observations, &x, &options(d, false)).map_err(|e| e.to_string())))
            .collect();
        let all: Vec<usize> = (0..design.column_names().len()).collect();
        let whole_model = fit.drop_columns_test(&all)?;
        let effect_tests = design
            .effects()
            .iter()
            .map(|e| Ok((e.name.clone(), fit.drop_columns_test(&e.columns.clone().collect::<Vec<_>>())?)))
            .collect::<Result<Vec<_>>>()?;
        let mut out = Self {
            request: request.clone(),
            data,
            design,
            observations,
            fit,
            others,
            time_name,
            whole_model,
            effect_tests,
            curves: Vec::new(),
            profile_name: format!("Time to {}% failing", format_number(request.profile_probability * 100.0, 4)),
        };
        out.curves = out.curve_settings();
        Ok(out)
    }

    fn curve_settings(&self) -> Vec<CurveSetting> {
        let defaults = self.data.default_settings(&self.design);
        let position = self.request.curve_by.and_then(|c| self.request.predictors.iter().position(|&p| p == c));
        let Some(j) = position else {
            return vec![CurveSetting { label: "Average unit".to_owned(), settings: defaults, rows: Vec::new() }];
        };
        let term = &self.design.terms()[j];
        let mut out = Vec::new();
        match (&term.kind, &self.data.predictors[j]) {
            (TermKind::Categorical { levels }, PredictorData::Categorical(labels)) => {
                for (l, level) in levels.iter().enumerate() {
                    let mut s = defaults.clone();
                    s[j] = TermValue::Level(l);
                    let rows =
                        (0..self.data.rows_used.len()).filter(|&k| labels[self.data.rows_used[k]] == *level).collect();
                    out.push(CurveSetting { label: format!("{} = {level}", term.name), settings: s, rows });
                }
            }
            (_, PredictorData::Continuous(v)) => {
                let mut vals: Vec<f64> = self.data.rows_used.iter().map(|&r| v[r]).collect();
                vals.sort_by(f64::total_cmp);
                vals.dedup();
                // Few distinct values (stress levels): one curve each.
                let picks: Vec<f64> = if vals.len() <= 5 {
                    vals.clone()
                } else {
                    let mut p: Vec<f64> =
                        [0.1, 0.5, 0.9].iter().map(|q| vals[((vals.len() - 1) as f64 * q).round() as usize]).collect();
                    p.dedup();
                    p
                };
                for x in picks {
                    let mut s = defaults.clone();
                    s[j] = TermValue::Number(x);
                    let rows = if vals.len() <= 5 {
                        (0..self.data.rows_used.len()).filter(|&k| v[self.data.rows_used[k]] == x).collect()
                    } else {
                        Vec::new()
                    };
                    out.push(CurveSetting {
                        label: format!("{} = {}", term.name, format_number(x, 4)),
                        settings: s,
                        rows,
                    });
                }
            }
            _ => out.push(CurveSetting { label: "Average unit".to_owned(), settings: defaults, rows: Vec::new() }),
        }
        out
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &SurvivalRegressionRequest {
        &self.request
    }

    /// The fit of the chosen distribution.
    #[must_use]
    pub fn fit(&self) -> &SurvivalRegressionFit {
        &self.fit
    }

    /// Likelihood-ratio test of each effect.
    #[must_use]
    pub fn effect_tests(&self) -> &[(String, ChiSquareTest)] {
        &self.effect_tests
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let names: Vec<&str> = self.data.predictor_names.iter().map(String::as_str).collect();
        let effects: Vec<String> =
            self.request.effective_effects().iter().map(|e| stat_linear::effect_name(&names, e)).collect();
        format!(
            "Parametric Survival Regression ({}): {} ~ {}",
            self.request.distribution.label(),
            self.time_name,
            effects.join(" + ")
        )
    }

    fn level(&self) -> String {
        format!("{}%", format_number(self.request.confidence * 100.0, 4))
    }

    fn encoded(&self, settings: &[TermValue]) -> Vec<f64> {
        self.design.encode(settings).unwrap_or_else(|_| vec![f64::NAN; self.design.column_names().len()])
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary(), self.comparison()];
        out.extend(
            crate::report::EffectPareto::new(
                self.effect_tests.iter().map(|(name, t)| (name.clone(), t.p_value)),
                "likelihood-ratio tests",
            )
            .section(),
        );
        out.extend(crate::report::hierarchy_note(&self.data.predictor_names, &self.request.effective_effects()));
        out.push(self.effect_table());
        out.push(self.estimates());
        out.push(self.time_ratios());
        if !self.request.probabilities.is_empty() {
            out.push(self.quantiles());
        }
        if !self.request.times.is_empty() {
            out.push(self.probabilities());
        }
        out.push(self.survival_plot());
        out.push(self.residual_plot());
        out
    }

    fn summary(&self) -> ReportSection {
        let f = &self.fit;
        let weight = |k: CensorKind| self.observations.iter().filter(|o| o.kind() == k).map(|o| o.weight).sum::<f64>();
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut row = |a: &str, b: String| t.push([a.to_owned(), b]);
        let d = self.request.distribution;
        row("Distribution", format!("{} (log time = location + σ·W)", d.label()));
        row("Units", num(f.n_units()));
        row("Exact failures", num(weight(CensorKind::Exact)));
        row("Right-censored", num(weight(CensorKind::Right)));
        row("Left-censored", num(weight(CensorKind::Left)));
        row("Interval-censored", num(weight(CensorKind::Interval)));
        if self.data.n_excluded > 0 {
            row("Rows excluded", self.data.n_excluded.to_string());
        }
        if self.data.n_missing > 0 {
            row("Rows with missing or unusable values", self.data.n_missing.to_string());
        }
        row("Log-likelihood", num(f.log_likelihood));
        row("AIC", num(f.aic()));
        row("BIC", num(f.bic()));
        let w = &self.whole_model;
        row(
            "Whole-model test (likelihood ratio)",
            format!("χ² = {} on {} df, p = {}", format_number(w.statistic, DIGITS), w.df, format_p_value(w.p_value)),
        );
        section(SectionKind::ModelSummary, "Model Summary", SectionContent::Table(t))
    }

    fn comparison(&self) -> ReportSection {
        let mut t = TextTable::new(["Distribution", "Parameters", "Log-likelihood", "AIC", "BIC"]);
        let mut rows: Vec<(AftDistribution, std::result::Result<&SurvivalRegressionFit, String>)> =
            vec![(self.request.distribution, Ok(&self.fit))];
        rows.extend(self.others.iter().map(|(d, f)| (*d, f.as_ref().map_err(Clone::clone))));
        rows.sort_by(|a, b| match (&a.1, &b.1) {
            (Ok(x), Ok(y)) => x.aic().total_cmp(&y.aic()),
            (Ok(_), Err(_)) => std::cmp::Ordering::Less,
            (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        });
        for (d, f) in rows {
            let name =
                if d == self.request.distribution { format!("{} (shown)", d.label()) } else { d.label().to_owned() };
            match f {
                Ok(f) => {
                    t.push([name, f.n_parameters().to_string(), num(f.log_likelihood), num(f.aic()), num(f.bic())])
                }
                Err(e) => t.push([name, String::new(), e, String::new(), String::new()]),
            }
        }
        section(
            SectionKind::SurvivalRegressionComparison,
            "Distribution Comparison (same effects; smaller AIC and BIC are better)",
            SectionContent::Table(t),
        )
    }

    fn effect_table(&self) -> ReportSection {
        let mut t = TextTable::new(["Effect", "DF", "Likelihood-ratio χ²", "p-value"]);
        for (name, test) in &self.effect_tests {
            t.push([
                name.clone(),
                test.df.to_string(),
                format_number(test.statistic, DIGITS),
                format_p_value(test.p_value),
            ]);
        }
        section(
            SectionKind::SurvivalRegressionEffectTests,
            "Tests by Effect (likelihood ratio, each effect removed)",
            SectionContent::Table(t),
        )
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
        if refs.is_empty() { String::new() } else { format!("; reference levels: {}", refs.join(", ")) }
    }

    fn interval_heads(&self, first: &[&str]) -> Vec<String> {
        let level = self.level();
        let mut h: Vec<String> = first.iter().map(|s| (*s).to_owned()).collect();
        h.extend([
            format!("Wald lower {level}"),
            format!("Wald upper {level}"),
            format!("Likelihood-ratio lower {level}"),
            format!("Likelihood-ratio upper {level}"),
        ]);
        h
    }

    fn estimates(&self) -> ReportSection {
        let mut t = TextTable::new(self.interval_heads(&["Term", "Estimate", "Std error", "z", "p-value"]));
        for c in &self.fit.coefficients {
            let (pl, pu) = c.profile.unwrap_or((f64::NAN, f64::NAN));
            t.push([
                c.name.clone(),
                num(c.estimate),
                format_number(c.std_error, 4),
                format_number(c.z, 4),
                format_p_value(c.p_value),
                num(c.wald.0),
                num(c.wald.1),
                num(pl),
                num(pu),
            ]);
        }
        if let Some(sc) = &self.fit.scale {
            let (pl, pu) = sc.profile.unwrap_or((f64::NAN, f64::NAN));
            t.push([
                sc.name.clone(),
                num(sc.estimate),
                format_number(sc.std_error, 4),
                String::new(),
                String::new(),
                num(sc.wald.0),
                num(sc.wald.1),
                num(pl),
                num(pu),
            ]);
            if self.request.distribution == AftDistribution::Weibull {
                t.push([
                    "Weibull shape β = 1/σ".to_owned(),
                    num(1.0 / sc.estimate),
                    format_number(sc.std_error / (sc.estimate * sc.estimate), 4),
                    String::new(),
                    String::new(),
                    num(1.0 / sc.wald.1),
                    num(1.0 / sc.wald.0),
                    num(1.0 / pu),
                    num(1.0 / pl),
                ]);
            }
        } else {
            t.push([
                "σ (fixed: exponential)".to_owned(),
                "1".to_owned(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
            ]);
        }
        section(
            SectionKind::SurvivalRegressionEstimates,
            format!("Parameter Estimates (location of log time; maximum likelihood{})", self.reference_levels()),
            SectionContent::Table(t),
        )
    }

    fn time_ratios(&self) -> ReportSection {
        let mut t = TextTable::new(self.interval_heads(&["Term", "Time ratio"]));
        for c in self.fit.coefficients.iter().skip(1) {
            let (pl, pu) = c.profile.unwrap_or((f64::NAN, f64::NAN));
            t.push([
                c.name.clone(),
                num(c.estimate.exp()),
                num(c.wald.0.exp()),
                num(c.wald.1.exp()),
                num(pl.exp()),
                num(pu.exp()),
            ]);
        }
        let extra = if self.request.distribution == AftDistribution::Weibull {
            "; as a Weibull proportional hazards model the hazard ratio is time ratio^(−β)"
        } else {
            ""
        };
        section(
            SectionKind::SurvivalRegressionTimeRatios,
            format!("Time Ratios (acceleration factors: every quantile of life multiplied per unit increase{extra})"),
            SectionContent::Table(t),
        )
    }

    fn quantiles(&self) -> ReportSection {
        let level = self.level();
        let mut t = TextTable::new([
            "Setting".to_owned(),
            "Fraction failing".to_owned(),
            "Time".to_owned(),
            format!("Lower {level}"),
            format!("Upper {level}"),
        ]);
        for c in &self.curves {
            let x = self.encoded(&c.settings);
            for &p in &self.request.probabilities {
                let q = self.fit.quantile(&x, p, self.request.confidence);
                t.push([
                    c.label.clone(),
                    format!("{}%", format_number(p * 100.0, 4)),
                    num(q.estimate),
                    num(q.lower),
                    num(q.upper),
                ]);
            }
        }
        section(
            SectionKind::SurvivalRegressionPredictions,
            format!(
                "Predicted Quantiles (time by which a fraction fails; other predictors at their mean or most frequent level; Wald {level} limits)"
            ),
            SectionContent::Table(t),
        )
    }

    fn probabilities(&self) -> ReportSection {
        let level = self.level();
        let mut t = TextTable::new([
            "Setting".to_owned(),
            "Time".to_owned(),
            "Failure probability".to_owned(),
            format!("Lower {level}"),
            format!("Upper {level}"),
        ]);
        for c in &self.curves {
            let x = self.encoded(&c.settings);
            for &time in &self.request.times {
                let f = self.fit.failure_probability(&x, time, self.request.confidence);
                t.push([
                    c.label.clone(),
                    num(time),
                    format_number(f.estimate, 4),
                    format_number(f.lower, 4),
                    format_number(f.upper, 4),
                ]);
            }
        }
        section(
            SectionKind::SurvivalRegressionProbabilities,
            "Predicted Failure Probabilities (reliability = 1 − failure probability)",
            SectionContent::Table(t),
        )
    }

    fn survival_plot(&self) -> ReportSection {
        let level = self.request.confidence;
        let (lo, hi) = self.observations.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), o| {
            let a = if o.lower.is_finite() { a.min(o.lower) } else { a };
            let a = if o.upper.is_finite() { a.min(o.upper) } else { a };
            let b = if o.upper.is_finite() { b.max(o.upper) } else { b.max(o.lower) };
            (a, b)
        });
        let end = if hi.is_finite() { hi } else { 1.0 };
        let start = 0.0_f64.min(if lo.is_finite() { lo } else { 0.0 });
        let grid: Vec<f64> =
            (0..=CURVE_POINTS).map(|i| start + (end - start) * i as f64 / CURVE_POINTS as f64).collect();
        let (mut points, mut censored, mut membership, mut curves) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for (k, c) in self.curves.iter().enumerate() {
            let x = self.encoded(&c.settings);
            let fitted: Polyline = grid.iter().map(|&t| [t, self.fit.survival(&x, t)]).collect();
            let band = grid
                .windows(2)
                .filter(|w| w[0] > 0.0)
                .map(|w| {
                    let f = self.fit.failure_probability(&x, 0.5 * (w[0] + w[1]), level);
                    [w[0], w[1], 1.0 - f.upper, 1.0 - f.lower]
                })
                .collect();
            let mut steps = Vec::new();
            if !c.rows.is_empty() {
                let obs: Vec<LifeObservation> = c.rows.iter().map(|&k| self.observations[k]).collect();
                if let Ok(est) = LifeData::new(obs).and_then(|d| turnbull(&d)) {
                    steps = est.survival_polyline(start);
                    for &kk in &c.rows {
                        let o = self.observations[kk];
                        let t = match o.kind() {
                            CensorKind::Exact | CensorKind::Right => o.lower,
                            CensorKind::Left | CensorKind::Interval => o.upper,
                        };
                        if let Some(f) = est.cdf_at(t) {
                            points.push(RowPoint { row: self.data.rows_used[kk], x: t, y: 1.0 - f });
                            censored.push(o.kind() != CensorKind::Exact);
                            membership.push(k);
                        }
                    }
                }
            }
            curves.push(SurvivalCurveSpec {
                label: c.label.clone(),
                steps,
                band,
                fitted: vec![(format!("{} fit", self.request.distribution.label()), LinePattern::Dashed, fitted)],
            });
        }
        let steps_note = if self.curves.iter().any(|c| !c.rows.is_empty()) {
            "steps: Turnbull estimate of each group's units; "
        } else {
            ""
        };
        section(
            SectionKind::SurvivalRegressionSurvivalPlot,
            format!(
                "Fitted Survival ({steps_note}dashed: model at each setting, other predictors at their average; shaded: pointwise {} limits)",
                self.level()
            ),
            SectionContent::Survival(Box::new(SurvivalPlotSpec {
                x_label: self.time_name.clone(),
                y_label: "Surviving fraction".to_owned(),
                curves,
                points,
                censored,
                membership,
                estimate: None,
                risk_table: None,
            })),
        )
    }

    /// Cox–Snell residuals against the cumulative hazard of their own
    /// nonparametric estimate: near the line `y = x` for a good model.
    fn residual_plot(&self) -> ReportSection {
        let resid = self.fit.cox_snell_residuals();
        let obs: Vec<LifeObservation> = resid
            .iter()
            .zip(&self.observations)
            .map(|(&(a, b), o)| {
                if o.kind() == CensorKind::Exact {
                    LifeObservation::exact(a, o.weight)
                } else {
                    let lo = if a > 0.0 { a } else { f64::NEG_INFINITY };
                    LifeObservation::interval(lo, b, o.weight)
                }
            })
            .collect();
        let mut points = Vec::new();
        let mut top = 0.0_f64;
        if let Ok(est) = LifeData::new(obs.clone()).and_then(|d| turnbull(&d)) {
            for (k, o) in obs.iter().enumerate() {
                if !o.upper.is_finite() {
                    continue;
                }
                if let Some(f) = est.cdf_at(o.upper).filter(|f| *f < 1.0) {
                    let y = -(1.0 - f).ln();
                    points.push(RowPoint { row: self.data.rows_used[k], x: o.upper, y });
                    top = top.max(o.upper).max(y);
                }
            }
        }
        let spec = PlotSpec {
            x_label: "Cox–Snell residual (fitted cumulative hazard at the failure time or inspection)".to_owned(),
            y_label: "Cumulative hazard of the residuals (nonparametric)".to_owned(),
            points,
            line: (top > 0.0).then(|| vec![[0.0, 0.0], [top, top]]),
            line_name: Some("Unit exponential (y = x)".to_owned()),
            point_name: Some("Unit".to_owned()),
            ..PlotSpec::default()
        };
        section(
            SectionKind::SurvivalRegressionResiduals,
            "Cox–Snell Residual Check (points near the line when the distribution and effects fit)",
            SectionContent::Plot(Box::new(spec)),
        )
    }

    /// Columns the report can add: the predicted time to the profiled
    /// fraction failing for every row with its predictors, and the Cox–Snell
    /// residual of each row used (the failure time's, the censoring time's
    /// for a unit still running, the midpoint of an interval's).
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        let n = self.data.y_all.len();
        let preds = self.data.borrowed();
        let p = self.request.profile_probability;
        let mut q = vec![f64::NAN; n];
        for (r, v) in q.iter_mut().enumerate() {
            if let Some(x) = self.design.row(&preds, r).filter(|x| x.iter().all(|v| v.is_finite())) {
                *v = self.fit.quantile(&x, p, self.request.confidence).estimate;
            }
        }
        let mut cs = vec![f64::NAN; n];
        for ((&r, &(a, b)), o) in
            self.data.rows_used.iter().zip(&self.fit.cox_snell_residuals()).zip(&self.observations)
        {
            cs[r] = match o.kind() {
                CensorKind::Exact => a,
                CensorKind::Right => a,
                CensorKind::Left => b,
                CensorKind::Interval => 0.5 * (a + b),
            };
        }
        vec![
            ReportColumn::continuous(
                "predicted",
                "Save predicted quantile",
                format!("Time to {}% failing", format_number(p * 100.0, 4)),
                q,
            ),
            ReportColumn::continuous(
                "cox_snell",
                "Save Cox–Snell residuals",
                format!("Cox–Snell residual {}", self.time_name),
                cs,
            ),
        ]
    }
}

impl crate::surface_profiler::SurfaceModel for SurvivalRegressionAnalysis {
    fn response_name(&self) -> &str {
        &self.profile_name
    }

    fn profile_factors(&self) -> Vec<ProfileFactor> {
        self.data.profile_factors(&self.design)
    }

    fn default_profile_settings(&self) -> Vec<TermValue> {
        self.data.default_settings(&self.design)
    }

    fn predict_settings(&self, points: &[Vec<TermValue>]) -> Result<Vec<f64>> {
        Ok(self.predict_with_limits(points)?.into_iter().map(|p| p.0).collect())
    }

    fn predict_with_limits(&self, points: &[Vec<TermValue>]) -> Result<Vec<Limited>> {
        points
            .iter()
            .map(|s| {
                let x = self.design.encode(s)?;
                let q = self.fit.quantile(&x, self.request.profile_probability, self.request.confidence);
                Ok((q.estimate, Some((q.lower, q.upper))))
            })
            .collect()
    }

    fn confidence_level(&self) -> Option<f64> {
        Some(self.request.confidence)
    }

    fn profile_note(&self) -> Option<String> {
        Some(format!(
            "Predicted time by which {}% of units fail ({} model), with Wald limits on the log scale.",
            format_number(self.request.profile_probability * 100.0, 4),
            self.request.distribution.label()
        ))
    }
}

/// The JSON form of a parametric survival regression:
///
/// ```text
/// {"analysis": "survival-regression", "layout": "intervals", "start": "From",
///  "end": "To", "frequency": "Units", "predictors": ["Temperature", "Lot"],
///  "effects": [[[0, 1]], [[1, 1]]], "distribution": "weibull",
///  "confidence": 0.95, "curve_by": "Temperature", "probabilities": [0.1, 0.5],
///  "times": [1000], "profile_probability": 0.5, "show_profiler": true, …}
/// ```
/// (or `"layout": "times", "time": …, "censor": …, "censor_coding": …`).
pub(crate) fn to_json(
    request: &SurvivalRegressionRequest,
    profilers: &ProfilerOptions,
    table: &DataTable,
) -> JsonValue {
    let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
    let num = JsonValue::number;
    let mut m = vec![("analysis", s("survival-regression"))];
    match request.layout {
        LifeDataLayout::Times { time, censor, coding } => {
            m.extend([("layout", s("times")), ("time", col(time))]);
            if let Some(c) = censor {
                m.push(("censor", col(c)));
            }
            let coding = match coding {
                StatusCoding::NonzeroIsCensored => "nonzero-censored",
                StatusCoding::NonzeroIsEvent => "nonzero-event",
            };
            m.push(("censor_coding", s(coding)));
        }
        LifeDataLayout::Intervals { start, end } => {
            m.extend([("layout", s("intervals")), ("start", col(start)), ("end", col(end))]);
        }
    }
    if let Some(c) = request.frequency {
        m.push(("frequency", col(c)));
    }
    m.push(("predictors", JsonValue::Array(request.predictors.iter().map(|&c| col(c)).collect())));
    if !request.effects.is_empty() {
        m.push(("effects", crate::project::effects_json(&request.effects)));
    }
    m.push(("distribution", s(request.distribution.key())));
    m.push(("confidence", num(request.confidence)));
    if let Some(c) = request.curve_by {
        m.push(("curve_by", col(c)));
    }
    m.push(("probabilities", JsonValue::Array(request.probabilities.iter().map(|&p| num(p)).collect())));
    m.push(("times", JsonValue::Array(request.times.iter().map(|&t| num(t)).collect())));
    m.push(("profile_probability", num(request.profile_probability)));
    m.push(("show_profiler", JsonValue::Bool(profilers.show_profiler)));
    m.push(("show_surface_profiler", JsonValue::Bool(profilers.show_surface_profiler)));
    m.push(("show_observed", JsonValue::Bool(profilers.show_observed)));
    JsonValue::Object(m.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

/// Read a request written by [`to_json`].
pub(crate) fn from_json(v: &JsonValue, table: &DataTable) -> Result<SavedReport> {
    let find = |item: &JsonValue| -> Result<usize> {
        let name = item.as_str().ok_or_else(|| invalid("expected a column name"))?;
        table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
    };
    let col = |key: &str| -> Result<usize> {
        find(v.get(key).ok_or_else(|| invalid(format!("survival regression: no {key} column")))?)
    };
    let optional = |key: &str| -> Result<Option<usize>> {
        match v.get(key) {
            None | Some(JsonValue::Null) => Ok(None),
            Some(item) => find(item).map(Some),
        }
    };
    let text = |key: &str| v.get(key).and_then(JsonValue::as_str);
    let layout = match text("layout") {
        Some("intervals") => LifeDataLayout::Intervals { start: col("start")?, end: col("end")? },
        None | Some("times") => {
            let coding = match text("censor_coding") {
                None | Some("nonzero-censored") => StatusCoding::NonzeroIsCensored,
                Some("nonzero-event") => StatusCoding::NonzeroIsEvent,
                Some(other) => return Err(invalid(format!("survival regression: unknown coding {other:?}"))),
            };
            LifeDataLayout::Times { time: col("time")?, censor: optional("censor")?, coding }
        }
        Some(other) => return Err(invalid(format!("survival regression: unknown layout {other:?}"))),
    };
    let predictors = match v.get("predictors") {
        Some(JsonValue::Array(items)) => items.iter().map(find).collect::<Result<_>>()?,
        _ => return Err(invalid("predictors must be a list of column names")),
    };
    let distribution = match text("distribution") {
        None => AftDistribution::Weibull,
        Some(k) => AftDistribution::from_key(k)
            .ok_or_else(|| invalid(format!("survival regression: unknown distribution {k:?}")))?,
    };
    let numbers = |key: &str| -> Option<Vec<f64>> {
        v.get(key).and_then(JsonValue::as_array).map(|a| a.iter().filter_map(JsonValue::as_f64).collect())
    };
    let d = SurvivalRegressionRequest::new(layout, Vec::new());
    let request = SurvivalRegressionRequest {
        frequency: optional("frequency")?,
        predictors,
        effects: crate::project::linear_effects(v)?,
        distribution,
        confidence: v.get("confidence").and_then(JsonValue::as_f64).unwrap_or(d.confidence),
        curve_by: optional("curve_by")?,
        probabilities: numbers("probabilities").unwrap_or_else(|| d.probabilities.clone()),
        times: numbers("times").unwrap_or_default(),
        profile_probability: v.get("profile_probability").and_then(JsonValue::as_f64).unwrap_or(d.profile_probability),
        ..d
    };
    Ok(SavedReport::SurvivalRegression {
        request,
        profilers: crate::proportional_hazards_analysis::read_profiler_flags(v),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::capacitor_life_test;

    fn col(t: &DataTable, name: &str) -> usize {
        t.find(name).unwrap()
    }

    fn request(t: &DataTable) -> SurvivalRegressionRequest {
        SurvivalRegressionRequest {
            frequency: Some(col(t, "Units")),
            curve_by: Some(col(t, "Temperature (°C)")),
            times: vec![1000.0],
            ..SurvivalRegressionRequest::new(
                LifeDataLayout::Intervals { start: col(t, "Last pass (h)"), end: col(t, "Found failed (h)") },
                vec![col(t, "Temperature (°C)"), col(t, "Lot")],
            )
        }
    }

    #[test]
    fn sample_report_matches_reference_and_has_every_section() {
        let t = capacitor_life_test();
        let rows = RowStateManager::new(t.n_rows());
        let a = SurvivalRegressionAnalysis::run(&t, &rows, &request(&t)).unwrap();
        let kinds: Vec<SectionKind> = a.sections().iter().map(|s| s.kind).collect();
        for k in [
            SectionKind::ModelSummary,
            SectionKind::SurvivalRegressionComparison,
            SectionKind::EffectPareto,
            SectionKind::SurvivalRegressionEffectTests,
            SectionKind::SurvivalRegressionEstimates,
            SectionKind::SurvivalRegressionTimeRatios,
            SectionKind::SurvivalRegressionPredictions,
            SectionKind::SurvivalRegressionProbabilities,
            SectionKind::SurvivalRegressionSurvivalPlot,
            SectionKind::SurvivalRegressionResiduals,
        ] {
            assert!(kinds.contains(&k), "{k:?} missing");
        }
        // The SciPy reference fit of these data (survival_regression_reference.py):
        // location 11.89506 − 0.0434042·T + 0.218456·[lot B], ln σ = −0.662198.
        let th = a.fit().theta();
        assert!((th[0] - 11.895_064_88).abs() < 1e-5, "{th:?}");
        assert!((th[1] + 0.043_404_216).abs() < 1e-7);
        assert!((th[2] - 0.218_456_246).abs() < 1e-6);
        assert!((th[3] + 0.662_197_843).abs() < 1e-6);
        // One fitted curve per temperature, each with its Turnbull steps.
        let s = a.sections();
        let plot = s.iter().find(|s| s.kind == SectionKind::SurvivalRegressionSurvivalPlot).unwrap();
        let SectionContent::Survival(p) = &plot.content else { panic!() };
        assert_eq!(p.curves.len(), 3);
        assert!(p.curves.iter().all(|c| !c.steps.is_empty() && !c.fitted.is_empty()));
        let cols = a.saveable_columns();
        assert_eq!(cols.len(), 2);
    }

    #[test]
    fn time_layout_distributions_profiler_and_errors() {
        let t = crate::sample::leukemia_covariates();
        let rows = RowStateManager::new(t.n_rows());
        let mut r = SurvivalRegressionRequest::new(
            LifeDataLayout::Times {
                time: col(&t, "Weeks"),
                censor: Some(col(&t, "Relapse")),
                coding: StatusCoding::NonzeroIsEvent,
            },
            vec![col(&t, "Treatment"), col(&t, "log WBC")],
        );
        r.curve_by = Some(col(&t, "Treatment"));
        for d in AftDistribution::ALL {
            r.distribution = d;
            let a = SurvivalRegressionAnalysis::run(&t, &rows, &r).unwrap();
            // Placebo shortens remission: a negative log-time coefficient.
            let c = a.fit().coefficients.iter().find(|c| c.name.contains("Placebo")).unwrap();
            assert!(c.estimate < 0.0, "{d:?}");
            use crate::surface_profiler::SurfaceModel;
            let p = a.predict_with_limits(&[a.default_profile_settings()]).unwrap();
            let (v, Some((lo, hi))) = p[0] else { panic!() };
            assert!(lo < v && v < hi);
        }
        let bad = SurvivalRegressionRequest { profile_probability: 1.5, ..r.clone() };
        assert!(SurvivalRegressionAnalysis::run(&t, &rows, &bad).is_err());
        let none = SurvivalRegressionRequest { predictors: Vec::new(), curve_by: None, ..r };
        assert!(SurvivalRegressionAnalysis::run(&t, &rows, &none).is_err());
    }
}
