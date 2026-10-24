//! Proportional hazards (Cox) regression bound to a dataset
//! ([`stat_reliability::proportional_hazards`]).
//!
//! **Layout:** a time column, an optional numeric censor column (coded as in
//! survival curves: by default a non-zero value marks a unit still running
//! at its time) and predictors, continuous or categorical, with effects
//! (interactions and powers) coded as in the linear model; an optional
//! strata column gives each of its levels its own baseline hazard. Excluded
//! rows and rows missing any value are left out.
//!
//! The report gives the model summary with the likelihood-ratio, Wald and
//! score tests of all effects, the Effect Pareto and likelihood-ratio test of
//! each effect (a refit without its columns), coefficients and hazard ratios
//! with Wald and profile-likelihood limits, the Grambsch–Therneau tests of
//! proportional hazards with a plot of scaled Schoenfeld residuals per
//! coefficient, predicted survival curves with pointwise limits at chosen
//! covariate settings, survival at chosen times, the Breslow baseline, and
//! deviance residuals. The profiler shows the hazard ratio against the
//! average unit, or the survival probability at a chosen time.

use crate::format::{format_number, format_p_value};
use crate::model_data::ModelData;
use crate::project::{SavedReport, invalid, s};
use crate::report::{
    PlotSpec, Polyline, ReportSection, RowPoint, SectionContent, SectionKind, SurvivalCurveSpec, SurvivalPlotSpec,
    TextTable,
};
use crate::report_columns::ReportColumn;
use crate::surface_profiler::{Limited, ProfileFactor, ProfilerOptions};
use crate::survival::StatusCoding;
use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_linear::{ModelDesign, TermKind, TermValue, natural_order};
use stat_reliability::{
    ChiSquareTest, ProportionalHazardsData, ProportionalHazardsFit, ProportionalHazardsOptions, TieMethod,
    TimeTransform, fit_proportional_hazards,
};

const DIGITS: usize = 5;

/// What the profiler of a proportional hazards report predicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum HazardProfile {
    /// Hazard ratio against the average unit (continuous predictors at their
    /// means, categorical ones at their most frequent level).
    #[default]
    HazardRatio,
    /// Probability of surviving past the profile time.
    Survival,
}

impl HazardProfile {
    /// Key in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::HazardRatio => "hazard-ratio",
            Self::Survival => "survival",
        }
    }

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::HazardRatio => "Hazard ratio",
            Self::Survival => "Survival probability",
        }
    }

    /// The profile with key `key`.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        [Self::HazardRatio, Self::Survival].into_iter().find(|p| p.key() == key)
    }
}

/// Columns, effects and options of a proportional hazards regression.
#[derive(Debug, Clone, PartialEq)]
pub struct ProportionalHazardsRequest {
    /// Time of failure or censoring.
    pub time: usize,
    /// Optional censor column (`None`: every time is a failure).
    pub censor: Option<usize>,
    /// Which censor values mean "censored".
    pub coding: StatusCoding,
    /// Predictor columns (at least one).
    pub predictors: Vec<usize>,
    /// Effects as `(position in predictors, power)` products; empty means
    /// main effects.
    pub effects: Vec<Vec<(usize, u8)>>,
    /// Optional strata column (a baseline hazard per level).
    pub strata: Option<usize>,
    /// Tied failure times.
    pub ties: TieMethod,
    /// Confidence level of every interval.
    pub confidence: f64,
    /// Transform of time in the test of proportional hazards.
    pub transform: TimeTransform,
    /// Predictor (a column of `predictors`) whose levels (categorical) or
    /// 10th, 50th and 90th percentiles (continuous) get their own survival
    /// curve, the others at their average; `None`: one curve at the average.
    pub curve_by: Option<usize>,
    /// Times at which the survival of each curve is tabulated.
    pub times: Vec<f64>,
    /// What the profiler predicts.
    pub profile: HazardProfile,
    /// Time of the survival profile (`None`: the median observed time).
    pub profile_time: Option<f64>,
}

impl ProportionalHazardsRequest {
    /// Default confidence level.
    pub const DEFAULT_CONFIDENCE: f64 = 0.95;

    /// A main-effects model with default settings.
    #[must_use]
    pub fn new(time: usize, censor: Option<usize>, predictors: Vec<usize>) -> Self {
        Self {
            time,
            censor,
            coding: StatusCoding::NonzeroIsCensored,
            predictors,
            effects: Vec::new(),
            strata: None,
            ties: TieMethod::Efron,
            confidence: Self::DEFAULT_CONFIDENCE,
            transform: TimeTransform::KaplanMeier,
            curve_by: None,
            times: Vec::new(),
            profile: HazardProfile::HazardRatio,
            profile_time: None,
        }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut out = vec![self.time];
        out.extend(self.censor);
        out.extend(self.predictors.iter().copied());
        out.extend(self.strata);
        out
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.time = f(self.time);
        self.censor = self.censor.map(f);
        self.predictors.iter_mut().for_each(|c| *c = f(*c));
        self.strata = self.strata.map(f);
        self.curve_by = self.curve_by.map(f);
    }

    pub(crate) fn effective_effects(&self) -> Vec<Vec<(usize, u8)>> {
        if self.effects.is_empty() { stat_linear::main_effects(self.predictors.len()) } else { self.effects.clone() }
    }
}

/// One survival curve: its label, design row and stratum.
#[derive(Debug, Clone)]
struct CurveSetting {
    label: String,
    settings: Vec<TermValue>,
    stratum: usize,
}

/// A fitted proportional hazards model and what its report needs.
#[derive(Debug, Clone)]
pub struct ProportionalHazardsAnalysis {
    request: ProportionalHazardsRequest,
    data: ModelData,
    design: ModelDesign,
    fit: ProportionalHazardsFit,
    /// Stratum labels (empty without strata).
    strata_labels: Vec<String>,
    /// Name of the strata column.
    strata_name: Option<String>,
    /// Each table row's stratum, from its own strata cell (`None` for a
    /// level the fit never saw); every row is stratum 0 without strata.
    row_strata: Vec<Option<usize>>,
    time_name: String,
    /// `(effect, test)` for each effect.
    effect_tests: Vec<(String, ChiSquareTest)>,
    curves: Vec<CurveSetting>,
    profile_time: f64,
    profile_name: String,
    reference: Vec<f64>,
}

fn value(table: &DataTable, col: usize, row: usize) -> f64 {
    table.column(col).and_then(|c| c.as_numeric()).map_or(f64::NAN, |v| v[row])
}

fn num(v: f64) -> String {
    format_number(v, DIGITS)
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

impl ProportionalHazardsAnalysis {
    /// Fit the model to the complete, included rows.
    ///
    /// # Errors
    /// No predictors, a column used twice, non-numeric time or censor
    /// columns, a confidence level outside (0.5, 1), invalid effects, no
    /// failures, or errors from the fit (collinear predictors, a predictor
    /// that separates failures from survivors).
    pub fn run(table: &DataTable, states: &RowStateManager, request: &ProportionalHazardsRequest) -> Result<Self> {
        if request.predictors.is_empty() {
            return Err(invalid("choose at least one predictor"));
        }
        let cols = request.columns();
        if (1..cols.len()).any(|i| cols[..i].contains(&cols[i])) {
            return Err(invalid("choose a different column for each role"));
        }
        let column = |i: usize| table.column(i).ok_or_else(|| invalid(format!("no column {i}")));
        for c in std::iter::once(request.time).chain(request.censor) {
            let col = column(c)?;
            if col.as_numeric().is_none() {
                return Err(invalid(format!("{} must be numeric", col.name)));
            }
        }
        if !(request.confidence > 0.5 && request.confidence < 1.0) {
            return Err(invalid("the confidence level must be between 0.5 and 1"));
        }
        if request.curve_by.is_some_and(|c| !request.predictors.contains(&c)) {
            return Err(invalid("the column giving separate curves must be one of the predictors"));
        }
        let time_name = column(request.time)?.name.clone();
        let strata_col = request.strata.map(column).transpose()?;
        let y_all: Vec<f64> = (0..table.n_rows())
            .map(|r| {
                let t = value(table, request.time, r);
                let ok = t.is_finite()
                    && request.censor.is_none_or(|c| value(table, c, r).is_finite())
                    && strata_col.is_none_or(|c| !c.level_text(r).trim().is_empty());
                if ok { t } else { f64::NAN }
            })
            .collect();
        let data = ModelData::gather_with_response(table, states, time_name.clone(), y_all, &request.predictors)?;
        let rows = data.rows_used.clone();
        if rows.is_empty() {
            return Err(invalid("no rows have values for every column of the model"));
        }
        let preds = data.borrowed();
        let design = ModelDesign::with_effects(&preds, &rows, &request.effective_effects())?;
        let x = design.matrix(&preds, &rows)?;
        let time: Vec<f64> = rows.iter().map(|&r| data.y_all[r]).collect();
        let event: Vec<bool> = rows
            .iter()
            .map(|&r| match request.censor {
                Some(c) => (value(table, c, r) != 0.0) != (request.coding == StatusCoding::NonzeroIsCensored),
                None => true,
            })
            .collect();
        let (strata, strata_labels) = match strata_col {
            Some(c) => {
                let labels: Vec<String> = rows.iter().map(|&r| c.level_text(r)).collect();
                let mut levels = labels.clone();
                levels.sort_by(|a, b| natural_order(a, b));
                levels.dedup();
                (Some(labels.iter().map(|l| levels.iter().position(|v| v == l).unwrap_or(0)).collect()), levels)
            }
            None => (None, Vec::new()),
        };
        let row_strata: Vec<Option<usize>> = (0..table.n_rows())
            .map(|r| match strata_col {
                Some(c) => {
                    let label = c.level_text(r);
                    strata_labels.iter().position(|v| *v == label)
                }
                None => Some(0),
            })
            .collect();
        let ph_data = ProportionalHazardsData::new(time.clone(), event, &x, strata)?;
        let options = ProportionalHazardsOptions {
            ties: request.ties,
            level: request.confidence,
            names: design.column_names().to_vec(),
            profile_intervals: true,
        };
        let fit = fit_proportional_hazards(&ph_data, &options)?;
        let effect_tests = design
            .effects()
            .iter()
            .map(|e| {
                let drop: Vec<usize> = e.columns.clone().collect();
                Ok((e.name.clone(), fit.drop_columns_test(&drop)?))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut sorted = time.clone();
        sorted.sort_by(f64::total_cmp);
        let median = sorted[sorted.len() / 2];
        let profile_time = request.profile_time.unwrap_or(median);
        let mut out = Self {
            request: request.clone(),
            data,
            design,
            fit,
            strata_labels,
            strata_name: strata_col.map(|c| c.name.clone()),
            row_strata,
            time_name,
            effect_tests,
            curves: Vec::new(),
            profile_time,
            profile_name: String::new(),
            reference: Vec::new(),
        };
        let defaults = out.data.default_settings(&out.design);
        out.reference = out.design.encode(&defaults)?;
        out.curves = out.curve_settings(&defaults);
        out.profile_name = match request.profile {
            HazardProfile::HazardRatio => "Hazard ratio".to_owned(),
            HazardProfile::Survival => format!("Survival past {}", num(profile_time)),
        };
        Ok(out)
    }

    fn curve_settings(&self, defaults: &[TermValue]) -> Vec<CurveSetting> {
        let mut settings: Vec<(String, Vec<TermValue>)> = Vec::new();
        let position = self.request.curve_by.and_then(|c| self.request.predictors.iter().position(|&p| p == c));
        match position {
            Some(j) => {
                let term = &self.design.terms()[j];
                match &term.kind {
                    TermKind::Categorical { levels } => {
                        for (l, level) in levels.iter().enumerate() {
                            let mut s = defaults.to_vec();
                            s[j] = TermValue::Level(l);
                            settings.push((format!("{} = {level}", term.name), s));
                        }
                    }
                    TermKind::Continuous => {
                        let mut v: Vec<f64> = match &self.data.predictors[j] {
                            crate::model_data::PredictorData::Continuous(v) => {
                                self.data.rows_used.iter().map(|&r| v[r]).collect()
                            }
                            crate::model_data::PredictorData::Categorical(_) => Vec::new(),
                        };
                        v.sort_by(f64::total_cmp);
                        for q in [0.1, 0.5, 0.9] {
                            if v.is_empty() {
                                break;
                            }
                            let x = v[((v.len() - 1) as f64 * q).round() as usize];
                            let mut s = defaults.to_vec();
                            s[j] = TermValue::Number(x);
                            let label = format!("{} = {}", term.name, format_number(x, 4));
                            if !settings.iter().any(|(l, _)| *l == label) {
                                settings.push((label, s));
                            }
                        }
                    }
                }
            }
            None => settings.push(("Average unit".to_owned(), defaults.to_vec())),
        }
        let n_strata = self.fit.data().n_strata();
        let mut out = Vec::new();
        for (label, s) in settings {
            for stratum in 0..n_strata {
                let label = match &self.strata_name {
                    Some(name) => format!("{label}, {name} = {}", self.strata_labels[stratum]),
                    None => label.clone(),
                };
                out.push(CurveSetting { label, settings: s.clone(), stratum });
            }
        }
        out
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &ProportionalHazardsRequest {
        &self.request
    }

    /// The fit.
    #[must_use]
    pub fn fit(&self) -> &ProportionalHazardsFit {
        &self.fit
    }

    /// Likelihood-ratio test of each effect: `(effect, test)`.
    #[must_use]
    pub fn effect_tests(&self) -> &[(String, ChiSquareTest)] {
        &self.effect_tests
    }

    /// The time of the survival profile.
    #[must_use]
    pub fn profile_time(&self) -> f64 {
        self.profile_time
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let names: Vec<&str> = self.data.predictor_names.iter().map(String::as_str).collect();
        let effects: Vec<String> =
            self.request.effective_effects().iter().map(|e| stat_linear::effect_name(&names, e)).collect();
        let mut t = format!("Proportional Hazards: {} ~ {}", self.time_name, effects.join(" + "));
        if let Some(s) = &self.strata_name {
            t.push_str(&format!(" (strata: {s})"));
        }
        t
    }

    fn level(&self) -> String {
        format!("{}%", format_number(self.request.confidence * 100.0, 4))
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary()];
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
        out.push(self.proportionality());
        out.extend(self.schoenfeld_plots());
        out.push(self.survival_plot());
        if !self.request.times.is_empty() {
            out.push(self.survival_table());
        }
        out.push(self.residual_plot());
        out.push(self.baseline_table());
        out
    }

    fn summary(&self) -> ReportSection {
        let f = &self.fit;
        let d = f.data();
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut row = |a: &str, b: String| t.push([a.to_owned(), b]);
        row("Units", d.n().to_string());
        row("Failures", d.n_events().to_string());
        row("Censored", (d.n() - d.n_events()).to_string());
        if self.data.n_excluded > 0 {
            row("Rows excluded", self.data.n_excluded.to_string());
        }
        if self.data.n_missing > 0 {
            row("Rows with missing values", self.data.n_missing.to_string());
        }
        if let Some(sname) = &self.strata_name {
            row("Strata", format!("{sname}: {}", self.strata_labels.join(", ")));
        }
        row("Tied failure times", format!("{} approximation", self.request.ties.label()));
        row("Log partial likelihood", num(f.log_likelihood));
        row("Log partial likelihood (no effects)", num(f.null_log_likelihood));
        row("AIC (partial likelihood)", num(f.aic()));
        for (name, test) in
            [("Likelihood-ratio test", f.likelihood_ratio), ("Wald test", f.wald), ("Score test", f.score)]
        {
            row(
                name,
                format!(
                    "χ² = {} on {} df, p = {}",
                    format_number(test.statistic, DIGITS),
                    test.df,
                    format_p_value(test.p_value)
                ),
            );
        }
        section(
            SectionKind::ModelSummary,
            "Model Summary (tests that every coefficient is zero)",
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
            SectionKind::HazardEffectTests,
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

    fn estimates(&self) -> ReportSection {
        let level = self.level();
        let mut t = TextTable::new([
            "Term".to_owned(),
            "Coefficient".to_owned(),
            "Std error".to_owned(),
            "z".to_owned(),
            "p-value".to_owned(),
            "Hazard ratio".to_owned(),
            format!("Wald lower {level}"),
            format!("Wald upper {level}"),
            format!("Likelihood-ratio lower {level}"),
            format!("Likelihood-ratio upper {level}"),
        ]);
        for c in &self.fit.coefficients {
            let (pl, pu) = c.profile.unwrap_or((f64::NAN, f64::NAN));
            t.push([
                c.name.clone(),
                num(c.estimate),
                format_number(c.std_error, 4),
                format_number(c.z, 4),
                format_p_value(c.p_value),
                num(c.hazard_ratio()),
                num(c.wald.0.exp()),
                num(c.wald.1.exp()),
                num(pl.exp()),
                num(pu.exp()),
            ]);
        }
        let coding = if self.design.coding() == stat_linear::CategoricalCoding::SumToZero {
            "; categorical predictors in interactions use sum-to-zero coding"
        } else {
            ""
        };
        section(
            SectionKind::HazardEstimates,
            format!(
                "Coefficients and Hazard Ratios (hazard multiplied by exp(coefficient) per unit increase{}{coding})",
                self.reference_levels()
            ),
            SectionContent::Table(t),
        )
    }

    fn proportionality(&self) -> ReportSection {
        let test = self.fit.proportionality_test(self.request.transform);
        let mut t = TextTable::new(["Term", "χ²", "DF", "p-value"]);
        for (c, r) in self.fit.coefficients.iter().zip(&test.covariates) {
            t.push([c.name.clone(), format_number(r.statistic, 4), r.df.to_string(), format_p_value(r.p_value)]);
        }
        t.push([
            "Global".to_owned(),
            format_number(test.global.statistic, 4),
            test.global.df.to_string(),
            format_p_value(test.global.p_value),
        ]);
        section(
            SectionKind::HazardProportionality,
            format!(
                "Proportional Hazards Check (Grambsch–Therneau tests on scaled Schoenfeld residuals against {}; a small p-value suggests the effect changes over time)",
                self.request.transform.label().to_lowercase()
            ),
            SectionContent::Table(t),
        )
    }

    fn schoenfeld_plots(&self) -> Vec<ReportSection> {
        let residuals = self.fit.schoenfeld_residuals();
        let test = self.fit.proportionality_test(self.request.transform);
        let g = &test.transformed_times;
        let rows = &self.data.rows_used;
        self.fit
            .coefficients
            .iter()
            .enumerate()
            .map(|(j, c)| {
                let points: Vec<RowPoint> =
                    residuals.iter().map(|r| RowPoint { row: rows[r.unit], x: r.time, y: r.scaled[j] }).collect();
                // Least-squares line of the scaled residuals on transformed time,
                // drawn against time (the trend the test measures).
                let n = g.len() as f64;
                let gbar = g.iter().sum::<f64>() / n;
                let ybar = points.iter().map(|p| p.y).sum::<f64>() / n;
                let sxx: f64 = g.iter().map(|v| (v - gbar) * (v - gbar)).sum();
                let sxy: f64 = g.iter().zip(&points).map(|(v, p)| (v - gbar) * (p.y - ybar)).sum();
                let slope = if sxx > 0.0 { sxy / sxx } else { 0.0 };
                let mut line: Polyline = residuals
                    .iter()
                    .zip(g)
                    .map(|(r, gv)| [r.time, ybar + slope * (gv - gbar)])
                    .filter(|p| p[1].is_finite())
                    .collect();
                line.dedup_by(|a, b| a[0] == b[0]);
                let (t0, t1) = residuals.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), r| {
                    (a.min(r.time), b.max(r.time))
                });
                let spec = PlotSpec {
                    x_label: self.time_name.clone(),
                    y_label: format!("β(t) for {}", c.name),
                    points,
                    line: Some(line),
                    line_name: Some(format!("Trend against {}", self.request.transform.label().to_lowercase())),
                    point_name: Some("Scaled Schoenfeld residual".to_owned()),
                    segments: if t0 < t1 { vec![vec![[t0, c.estimate], [t1, c.estimate]]] } else { Vec::new() },
                    ..PlotSpec::default()
                };
                section(
                    SectionKind::HazardSchoenfeldPlot,
                    format!(
                        "Scaled Schoenfeld Residuals: {} (one point per failure; flat around the coefficient {} when hazards are proportional)",
                        c.name,
                        num(c.estimate)
                    ),
                    SectionContent::Plot(Box::new(spec)),
                )
            })
            .collect()
    }

    fn encoded(&self, settings: &[TermValue]) -> Vec<f64> {
        self.design.encode(settings).unwrap_or_else(|_| vec![f64::NAN; self.design.column_names().len()])
    }

    fn survival_plot(&self) -> ReportSection {
        let level = self.request.confidence;
        let end = self.fit.data().times().iter().copied().fold(0.0_f64, f64::max);
        let start = self.fit.data().times().iter().copied().fold(f64::INFINITY, f64::min).min(0.0);
        let curves = self
            .curves
            .iter()
            .map(|c| {
                let x = self.encoded(&c.settings);
                let pts = self.fit.survival_curve(&x, c.stratum, level);
                let mut steps: Polyline = vec![[start, 1.0]];
                let mut band = Vec::new();
                let mut prev = 1.0;
                for (k, p) in pts.iter().enumerate() {
                    steps.push([p.time, prev]);
                    steps.push([p.time, p.survival]);
                    prev = p.survival;
                    let next = pts.get(k + 1).map_or(end, |q| q.time);
                    if next > p.time {
                        band.push([p.time, next, p.lower, p.upper]);
                    }
                }
                steps.push([end, prev]);
                SurvivalCurveSpec { label: c.label.clone(), steps, band, fitted: Vec::new() }
            })
            .collect();
        section(
            SectionKind::HazardSurvivalPlot,
            format!(
                "Predicted Survival (Breslow baseline; other predictors at their mean or most frequent level; shaded: pointwise {} limits)",
                self.level()
            ),
            SectionContent::Survival(Box::new(SurvivalPlotSpec {
                x_label: self.time_name.clone(),
                y_label: "Surviving fraction".to_owned(),
                curves,
                points: Vec::new(),
                censored: Vec::new(),
                membership: Vec::new(),
                estimate: None,
                risk_table: None,
            })),
        )
    }

    fn survival_table(&self) -> ReportSection {
        let level = self.level();
        let mut t = TextTable::new([
            "Curve".to_owned(),
            "Time".to_owned(),
            "Survival".to_owned(),
            format!("Lower {level}"),
            format!("Upper {level}"),
            "Cumulative hazard".to_owned(),
        ]);
        for c in &self.curves {
            let x = self.encoded(&c.settings);
            for &time in &self.request.times {
                let p = self.fit.survival_at(&x, c.stratum, time, self.request.confidence);
                t.push([
                    c.label.clone(),
                    num(time),
                    format_number(p.survival, 4),
                    format_number(p.lower, 4),
                    format_number(p.upper, 4),
                    format_number(p.cumulative_hazard, 4),
                ]);
            }
        }
        section(SectionKind::HazardSurvivalTable, "Predicted Survival at Chosen Times", SectionContent::Table(t))
    }

    fn residual_plot(&self) -> ReportSection {
        let d = self.fit.data();
        let dev = self.fit.deviance_residuals();
        let beta = self.fit.beta();
        let means = d.means();
        let preds = self.data.borrowed();
        let points = self
            .data
            .rows_used
            .iter()
            .zip(&dev)
            .filter_map(|(&row, &y)| {
                let xr = self.design.row(&preds, row)?;
                let lp: f64 = xr.iter().zip(means).zip(&beta).map(|((x, m), b)| (x - m) * b).sum();
                Some(RowPoint { row, x: lp, y })
            })
            .collect::<Vec<_>>();
        let (lo, hi) = points.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.x), b.max(p.x)));
        let spec = PlotSpec {
            x_label: "Linear predictor (log hazard ratio against the average unit)".to_owned(),
            y_label: "Deviance residual".to_owned(),
            points,
            line: (lo < hi).then(|| vec![[lo, 0.0], [hi, 0.0]]),
            line_name: Some("Zero".to_owned()),
            point_name: Some("Unit".to_owned()),
            ..PlotSpec::default()
        };
        section(
            SectionKind::HazardResiduals,
            "Deviance Residuals (large values mark units that failed much earlier or later than predicted)",
            SectionContent::Plot(Box::new(spec)),
        )
    }

    fn baseline_table(&self) -> ReportSection {
        let mut heads = Vec::new();
        if let Some(name) = &self.strata_name {
            heads.push(name.clone());
        }
        heads.extend(
            ["Time", "At risk", "Failures", "Baseline cumulative hazard", "Baseline survival"].map(str::to_owned),
        );
        let mut t = TextTable::new(heads);
        for (s, steps) in self.fit.baseline.iter().enumerate() {
            for step in steps {
                let mut row = Vec::new();
                if self.strata_name.is_some() {
                    row.push(self.strata_labels[s].clone());
                }
                row.extend([
                    num(step.time),
                    step.at_risk.to_string(),
                    step.events.to_string(),
                    format_number(step.cumulative_hazard, 5),
                    format_number(step.survival(), 5),
                ]);
                t.push(row);
            }
        }
        section(
            SectionKind::HazardBaseline,
            "Baseline Survival (Breslow estimate with every model column at its mean)",
            SectionContent::Table(t),
        )
    }

    /// Columns the report can add: the linear predictor, the hazard ratio
    /// against the average unit, martingale and deviance residuals, and the
    /// predicted survival at the profile time in each row's own stratum
    /// (missing for a row whose stratum level was not in the fit).
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        let n = self.data.y_all.len();
        let preds = self.data.borrowed();
        let mut lp = vec![f64::NAN; n];
        let mut hr = vec![f64::NAN; n];
        let mut surv = vec![f64::NAN; n];
        for r in 0..n {
            let Some(x) = self.design.row(&preds, r) else { continue };
            if x.iter().any(|v| !v.is_finite()) {
                continue;
            }
            lp[r] = self.fit.linear_predictor(&x);
            hr[r] = self.fit.hazard_ratio(&x, &self.reference, self.request.confidence).0;
            if let Some(stratum) = self.row_strata.get(r).copied().flatten() {
                surv[r] = self.fit.survival_at(&x, stratum, self.profile_time, self.request.confidence).survival;
            }
        }
        let mut mart = vec![f64::NAN; n];
        let mut dev = vec![f64::NAN; n];
        for ((&r, m), d) in
            self.data.rows_used.iter().zip(self.fit.martingale_residuals()).zip(self.fit.deviance_residuals())
        {
            mart[r] = m;
            dev[r] = d;
        }
        let t = &self.time_name;
        vec![
            ReportColumn::continuous("linear_predictor", "Save linear predictor", format!("Linear predictor {t}"), lp),
            ReportColumn::continuous("hazard_ratio", "Save hazard ratios", format!("Hazard ratio {t}"), hr),
            ReportColumn::continuous(
                "survival",
                "Save predicted survival",
                format!("Survival past {} {t}", format_number(self.profile_time, 6)),
                surv,
            ),
            ReportColumn::continuous(
                "martingale",
                "Save martingale residuals",
                format!("Martingale residual {t}"),
                mart,
            ),
            ReportColumn::continuous("deviance", "Save deviance residuals", format!("Deviance residual {t}"), dev),
        ]
    }
}

impl crate::surface_profiler::SurfaceModel for ProportionalHazardsAnalysis {
    fn response_name(&self) -> &str {
        &self.profile_name
    }

    fn profile_factors(&self) -> Vec<ProfileFactor> {
        self.data.profile_factors(&self.design)
    }

    fn observations(&self) -> Vec<crate::surface_profiler::Observation> {
        Vec::new()
    }

    fn default_profile_settings(&self) -> Vec<TermValue> {
        self.data.default_settings(&self.design)
    }

    fn predict_settings(&self, points: &[Vec<TermValue>]) -> Result<Vec<f64>> {
        Ok(self.predict_with_limits(points)?.into_iter().map(|p| p.0).collect())
    }

    fn predict_with_limits(&self, points: &[Vec<TermValue>]) -> Result<Vec<Limited>> {
        let level = self.request.confidence;
        points
            .iter()
            .map(|s| {
                let x = self.design.encode(s)?;
                Ok(match self.request.profile {
                    HazardProfile::HazardRatio => {
                        let (h, lo, hi) = self.fit.hazard_ratio(&x, &self.reference, level);
                        (h, Some((lo, hi)))
                    }
                    HazardProfile::Survival => {
                        let p = self.fit.survival_at(&x, 0, self.profile_time, level);
                        (p.survival, Some((p.lower, p.upper)))
                    }
                })
            })
            .collect()
    }

    fn confidence_level(&self) -> Option<f64> {
        Some(self.request.confidence)
    }

    fn profile_note(&self) -> Option<String> {
        Some(match self.request.profile {
            HazardProfile::HazardRatio => {
                "Hazard ratio against the average unit (continuous predictors at their means, categorical ones at \
                 their most frequent level), with Wald limits on the log scale."
                    .to_owned()
            }
            HazardProfile::Survival => {
                let stratum = match &self.strata_name {
                    Some(name) => format!(" ({name} = {})", self.strata_labels.first().cloned().unwrap_or_default()),
                    None => String::new(),
                };
                format!(
                    "Probability of surviving past {}{stratum}, with pointwise limits from the log cumulative hazard.",
                    num(self.profile_time)
                )
            }
        })
    }
}

/// The JSON form of a proportional hazards report:
///
/// ```text
/// {"analysis": "proportional-hazards", "time": "Weeks", "censor": "Censored",
///  "censor_coding": "nonzero-censored", "predictors": ["Treatment", "log WBC"],
///  "effects": [[[0, 1]], [[1, 1]]], "strata": "Sex", "ties": "efron",
///  "confidence": 0.95, "ph_transform": "km", "curve_by": "Treatment",
///  "times": [10, 20], "profile": "survival", "profile_time": 12,
///  "show_profiler": true, "show_surface_profiler": false, "show_observed": false}
/// ```
pub(crate) fn to_json(
    request: &ProportionalHazardsRequest,
    profilers: &ProfilerOptions,
    table: &DataTable,
) -> JsonValue {
    let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
    let num = JsonValue::number;
    let mut m = vec![("analysis", s("proportional-hazards")), ("time", col(request.time))];
    if let Some(c) = request.censor {
        m.push(("censor", col(c)));
    }
    let coding = match request.coding {
        StatusCoding::NonzeroIsCensored => "nonzero-censored",
        StatusCoding::NonzeroIsEvent => "nonzero-event",
    };
    m.push(("censor_coding", s(coding)));
    m.push(("predictors", JsonValue::Array(request.predictors.iter().map(|&c| col(c)).collect())));
    if !request.effects.is_empty() {
        m.push(("effects", crate::project::effects_json(&request.effects)));
    }
    if let Some(c) = request.strata {
        m.push(("strata", col(c)));
    }
    m.push(("ties", s(request.ties.key())));
    m.push(("confidence", num(request.confidence)));
    m.push(("ph_transform", s(request.transform.key())));
    if let Some(c) = request.curve_by {
        m.push(("curve_by", col(c)));
    }
    m.push(("times", JsonValue::Array(request.times.iter().map(|&t| num(t)).collect())));
    m.push(("profile", s(request.profile.key())));
    if let Some(t) = request.profile_time {
        m.push(("profile_time", num(t)));
    }
    m.push(("show_profiler", JsonValue::Bool(profilers.show_profiler)));
    m.push(("show_surface_profiler", JsonValue::Bool(profilers.show_surface_profiler)));
    m.push(("show_observed", JsonValue::Bool(profilers.show_observed)));
    JsonValue::Object(m.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

/// Profiler toggles stored with a model report.
pub(crate) fn read_profiler_flags(v: &JsonValue) -> ProfilerOptions {
    let d = ProfilerOptions::default();
    let flag = |key: &str, default: bool| v.get(key).and_then(JsonValue::as_bool).unwrap_or(default);
    ProfilerOptions {
        show_profiler: flag("show_profiler", d.show_profiler),
        show_surface_profiler: flag("show_surface_profiler", d.show_surface_profiler),
        show_observed: flag("show_observed", d.show_observed),
    }
}

/// Read a request written by [`to_json`].
pub(crate) fn from_json(v: &JsonValue, table: &DataTable) -> Result<SavedReport> {
    let find = |item: &JsonValue| -> Result<usize> {
        let name = item.as_str().ok_or_else(|| invalid("expected a column name"))?;
        table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
    };
    let optional = |key: &str| -> Result<Option<usize>> {
        match v.get(key) {
            None | Some(JsonValue::Null) => Ok(None),
            Some(item) => find(item).map(Some),
        }
    };
    let text = |key: &str| v.get(key).and_then(JsonValue::as_str);
    let time = find(v.get("time").ok_or_else(|| invalid("proportional hazards: no time column"))?)?;
    let predictors = match v.get("predictors") {
        Some(JsonValue::Array(items)) => items.iter().map(find).collect::<Result<_>>()?,
        _ => return Err(invalid("predictors must be a list of column names")),
    };
    let coding = match text("censor_coding") {
        None | Some("nonzero-censored") => StatusCoding::NonzeroIsCensored,
        Some("nonzero-event") => StatusCoding::NonzeroIsEvent,
        Some(other) => return Err(invalid(format!("proportional hazards: unknown coding {other:?}"))),
    };
    let ties = match text("ties") {
        None => TieMethod::Efron,
        Some(k) => {
            TieMethod::from_key(k).ok_or_else(|| invalid(format!("proportional hazards: unknown ties {k:?}")))?
        }
    };
    let transform = match text("ph_transform") {
        None => TimeTransform::KaplanMeier,
        Some(k) => TimeTransform::from_key(k)
            .ok_or_else(|| invalid(format!("proportional hazards: unknown time transform {k:?}")))?,
    };
    let profile = match text("profile") {
        None => HazardProfile::HazardRatio,
        Some(k) => {
            HazardProfile::from_key(k).ok_or_else(|| invalid(format!("proportional hazards: unknown profile {k:?}")))?
        }
    };
    let d = ProportionalHazardsRequest::new(time, None, Vec::new());
    let request = ProportionalHazardsRequest {
        censor: optional("censor")?,
        coding,
        predictors,
        effects: crate::project::linear_effects(v)?,
        strata: optional("strata")?,
        ties,
        confidence: v.get("confidence").and_then(JsonValue::as_f64).unwrap_or(d.confidence),
        transform,
        curve_by: optional("curve_by")?,
        times: v
            .get("times")
            .and_then(JsonValue::as_array)
            .map(|a| a.iter().filter_map(JsonValue::as_f64).collect())
            .unwrap_or_default(),
        profile,
        profile_time: v.get("profile_time").and_then(JsonValue::as_f64),
        ..d
    };
    Ok(SavedReport::ProportionalHazards { request, profilers: read_profiler_flags(v) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::leukemia_covariates;

    fn col(t: &DataTable, name: &str) -> usize {
        t.find(name).unwrap()
    }

    fn request(t: &DataTable) -> ProportionalHazardsRequest {
        ProportionalHazardsRequest {
            curve_by: Some(col(t, "Treatment")),
            times: vec![10.0, 20.0],
            ..ProportionalHazardsRequest::new(
                col(t, "Weeks"),
                Some(col(t, "Relapse")),
                vec![col(t, "Treatment"), col(t, "log WBC"), col(t, "Sex")],
            )
        }
    }

    #[test]
    fn sample_report_has_every_section() {
        let t = leukemia_covariates();
        let rows = RowStateManager::new(t.n_rows());
        let mut r = request(&t);
        r.coding = StatusCoding::NonzeroIsEvent;
        let a = ProportionalHazardsAnalysis::run(&t, &rows, &r).unwrap();
        let kinds: Vec<SectionKind> = a.sections().iter().map(|s| s.kind).collect();
        for k in [
            SectionKind::ModelSummary,
            SectionKind::EffectPareto,
            SectionKind::HazardEffectTests,
            SectionKind::HazardEstimates,
            SectionKind::HazardProportionality,
            SectionKind::HazardSchoenfeldPlot,
            SectionKind::HazardSurvivalPlot,
            SectionKind::HazardSurvivalTable,
            SectionKind::HazardResiduals,
            SectionKind::HazardBaseline,
        ] {
            assert!(kinds.contains(&k), "{k:?} missing");
        }
        // The published fit of these data (lifelines CoxPHFitter, Efron):
        // log WBC 1.6819, treatment (placebo vs 6-MP) 1.5036, sex 0.3147.
        let f = a.fit();
        let by = |name: &str| f.coefficients.iter().find(|c| c.name.contains(name)).unwrap().estimate;
        assert!((by("log WBC") - 1.681_942).abs() < 1e-5, "{}", by("log WBC"));
        assert!((by("Treatment") - 1.503_591).abs() < 1e-5);
        assert!((by("Sex") - 0.314_678).abs() < 1e-5);
        // One survival curve per treatment.
        let s = a.sections();
        let plot = s.iter().find(|s| s.kind == SectionKind::HazardSurvivalPlot).unwrap();
        let SectionContent::Survival(p) = &plot.content else { panic!() };
        assert_eq!(p.curves.len(), 2);
        assert!(p.curves.iter().all(|c| !c.band.is_empty()));
        // Effect tests: single-column effects, so their LR tests are 1 df.
        assert!(a.effect_tests().iter().all(|(_, t)| t.df == 1));
        let cols = a.saveable_columns();
        assert_eq!(cols.len(), 5);
        assert!(cols.iter().all(|c| c.values.iter().filter(|v| v.is_finite()).count() == 42));
    }

    #[test]
    fn strata_interactions_profiler_and_errors() {
        let t = leukemia_covariates();
        let rows = RowStateManager::new(t.n_rows());
        let mut r = request(&t);
        r.coding = StatusCoding::NonzeroIsEvent;
        r.predictors = vec![col(&t, "Treatment"), col(&t, "log WBC")];
        r.effects = vec![vec![(0, 1)], vec![(1, 1)], vec![(0, 1), (1, 1)]];
        r.strata = Some(col(&t, "Sex"));
        r.ties = TieMethod::Breslow;
        r.profile = HazardProfile::Survival;
        let a = ProportionalHazardsAnalysis::run(&t, &rows, &r).unwrap();
        assert_eq!(a.fit().baseline.len(), 2);
        assert_eq!(a.effect_tests().len(), 3);
        assert!(a.title().contains("strata: Sex"));
        use crate::surface_profiler::SurfaceModel;
        let s = a.default_profile_settings();
        let p = a.predict_with_limits(&[s]).unwrap();
        let (v, Some((lo, hi))) = p[0] else { panic!() };
        assert!(lo <= v && v <= hi && v > 0.0 && v < 1.0);
        let same = ProportionalHazardsRequest { strata: Some(col(&t, "Weeks")), ..r.clone() };
        assert!(ProportionalHazardsAnalysis::run(&t, &rows, &same).is_err());
        let none = ProportionalHazardsRequest { predictors: Vec::new(), effects: Vec::new(), ..r };
        assert!(ProportionalHazardsAnalysis::run(&t, &rows, &none).is_err());
    }

    #[test]
    fn saved_survival_uses_each_rows_own_stratum() {
        let t = leukemia_covariates();
        let mut r = request(&t);
        r.coding = StatusCoding::NonzeroIsEvent;
        r.predictors = vec![col(&t, "Treatment"), col(&t, "log WBC")];
        r.effects = vec![vec![(0, 1)], vec![(1, 1)]];
        r.strata = Some(col(&t, "Sex"));
        let sex = t.column(col(&t, "Sex")).unwrap();
        let full = ProportionalHazardsAnalysis::run(&t, &RowStateManager::new(t.n_rows()), &r).unwrap();
        let second = full.strata_labels[1].clone();
        // Exclude one row of the second stratum: its saved survival must use
        // that stratum's baseline, not the first one.
        let row = (0..t.n_rows()).find(|&i| sex.level_text(i) == second).unwrap();
        let mut states = RowStateManager::new(t.n_rows());
        states.set_excluded([row], true);
        let a = ProportionalHazardsAnalysis::run(&t, &states, &r).unwrap();
        let x = a.design.row(&a.data.borrowed(), row).unwrap();
        let want = a.fit.survival_at(&x, 1, a.profile_time, r.confidence).survival;
        let other = a.fit.survival_at(&x, 0, a.profile_time, r.confidence).survival;
        assert!((want - other).abs() > 1e-6, "the strata differ");
        let cols = a.saveable_columns();
        let surv = &cols.iter().find(|c| c.key == "survival").unwrap().values;
        assert!((surv[row] - want).abs() < 1e-12, "{} vs {want}", surv[row]);
        // A level the fit never saw gives a missing value.
        let mut states = RowStateManager::new(t.n_rows());
        states.set_excluded((0..t.n_rows()).filter(|&i| sex.level_text(i) == second), true);
        let mut one = r.clone();
        one.strata = Some(col(&t, "Sex"));
        let b = ProportionalHazardsAnalysis::run(&t, &states, &one).unwrap();
        let surv = b.saveable_columns().into_iter().find(|c| c.key == "survival").unwrap().values;
        assert!(surv[row].is_nan());
        assert!(surv.iter().filter(|v| v.is_finite()).count() > 0);
    }
}
