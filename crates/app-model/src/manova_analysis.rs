//! Multivariate analysis of variance bound to the row state
//! ([`stat_linear::manova`]).
//!
//! Several continuous responses are modelled on the same effects, coded as
//! in the linear model report (categorical predictors as indicators, or
//! sum-to-zero in models with interactions; continuous predictors centred
//! in higher-order effects). The rows used are those not excluded with
//! every response and predictor present.
//!
//! The report gives each effect's Wilks, Pillai, Hotelling–Lawley and Roy
//! tests, an Effect Pareto by the chosen statistic, univariate F-tests of
//! each response, any contrasts among the levels of a categorical factor,
//! the canonical analysis of each effect (eigenvalues of `E⁻¹H`, canonical
//! correlations, standardized coefficients), a canonical plot of the rows
//! for one categorical main effect, the residual (partial) correlations of
//! the responses, and Box's M test of equal covariance matrices across the
//! cells of the categorical factors when every predictor is categorical.

use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_linear::manova::{BoxMTest, HypothesisTest, ManovaFit, MultivariateStatistic, box_m_test, fit_manova};
use stat_linear::{CategoricalCoding, ModelDesign, TermKind};

use crate::format::{format_number, format_optional, format_p_value};
use crate::linear_analysis::{Coded, LinearModelSpec, PredictorData};
use crate::report::{
    Annotations, EffectPareto, PlotSpec, PointGroups, ReportSection, RowPoint, SectionContent, SectionKind, TextTable,
};

const DIGITS: usize = 5;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: &str, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.to_owned(), content }
}

/// A contrast among the levels of one categorical factor: `Σ cⱼ μⱼ = 0`,
/// the `μⱼ` being least-squares means (each level's mean with the other
/// factors balanced). Weights must sum to zero; unnamed levels weigh 0.
#[derive(Debug, Clone, PartialEq)]
pub struct ManovaContrast {
    /// Name shown in the report (e.g. `"A vs B"`).
    pub name: String,
    /// The factor, as a column index (it must be a predictor with a main
    /// effect in the model).
    pub factor: usize,
    /// `(level, weight)` pairs.
    pub weights: Vec<(String, f64)>,
}

/// Responses, model and report settings.
#[derive(Debug, Clone, PartialEq)]
pub struct ManovaRequest {
    /// Continuous response columns (two or more).
    pub responses: Vec<usize>,
    /// Predictor columns.
    pub predictors: Vec<usize>,
    /// Effects as `(position in predictors, power)` products; empty means
    /// main effects only (as in [`LinearModelSpec`]).
    pub effects: Vec<Vec<(usize, u8)>>,
    /// The statistic ranking effects in the Effect Pareto.
    pub statistic: MultivariateStatistic,
    /// Contrasts tested after the effects.
    pub contrasts: Vec<ManovaContrast>,
    /// The categorical factor (column) whose canonical plot is drawn
    /// (`None`: the first categorical main effect).
    pub plot_factor: Option<usize>,
}

impl ManovaRequest {
    /// Main effects of `predictors` on `responses`, ranked by Pillai's trace.
    #[must_use]
    pub fn new(responses: Vec<usize>, predictors: Vec<usize>) -> Self {
        Self {
            responses,
            predictors,
            effects: Vec::new(),
            statistic: MultivariateStatistic::Pillai,
            contrasts: Vec::new(),
            plot_factor: None,
        }
    }

    /// The effects to fit: the listed ones, or every main effect.
    #[must_use]
    pub fn effective_effects(&self) -> Vec<Vec<(usize, u8)>> {
        if self.effects.is_empty() { stat_linear::main_effects(self.predictors.len()) } else { self.effects.clone() }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut c = self.responses.clone();
        c.extend(&self.predictors);
        c
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.responses.iter_mut().for_each(|c| *c = f(*c));
        self.predictors.iter_mut().for_each(|c| *c = f(*c));
        self.contrasts.iter_mut().for_each(|c| c.factor = f(c.factor));
        if let Some(p) = &mut self.plot_factor {
            *p = f(*p);
        }
    }

    /// The project-file form (`"analysis": "manova"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        use crate::project::{obj, s};
        let name = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        let contrasts = self
            .contrasts
            .iter()
            .map(|c| {
                obj([
                    ("name", s(&c.name)),
                    ("factor", name(c.factor)),
                    (
                        "weights",
                        JsonValue::Object(c.weights.iter().map(|(l, w)| (l.clone(), JsonValue::number(*w))).collect()),
                    ),
                ])
            })
            .collect();
        obj([
            ("analysis", s("manova")),
            ("responses", crate::correlation_analysis::columns_json(&self.responses, table)),
            ("predictors", crate::correlation_analysis::columns_json(&self.predictors, table)),
            ("effects", crate::project::effects_json(&self.effects)),
            ("statistic", s(self.statistic.key())),
            ("contrasts", JsonValue::Array(contrasts)),
            ("plot_factor", self.plot_factor.map_or(JsonValue::Null, name)),
        ])
    }

    /// Read the project-file form; missing settings take their defaults.
    ///
    /// # Errors
    /// Missing or unknown columns, an unknown statistic, malformed effects or
    /// contrasts.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let responses = crate::correlation_analysis::json_columns(v, "responses", table)?;
        let predictors = crate::correlation_analysis::json_columns(v, "predictors", table)?;
        let d = Self::new(responses, predictors);
        let statistic = match v.get("statistic").and_then(JsonValue::as_str) {
            None => d.statistic,
            Some(key) => MultivariateStatistic::from_key(key)
                .ok_or_else(|| invalid(format!("manova: unknown statistic {key:?}")))?,
        };
        let column =
            |name: &str| table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")));
        let mut contrasts = Vec::new();
        if let Some(list) = v.get("contrasts").and_then(JsonValue::as_array) {
            for c in list {
                let bad = || invalid("manova: a contrast needs a factor and weights by level");
                let factor = column(c.get("factor").and_then(JsonValue::as_str).ok_or_else(bad)?)?;
                let JsonValue::Object(members) = c.get("weights").ok_or_else(bad)? else { return Err(bad()) };
                let weights = members
                    .iter()
                    .map(|(level, w)| Ok((level.clone(), w.as_f64().ok_or_else(bad)?)))
                    .collect::<Result<_>>()?;
                let name = c.get("name").and_then(JsonValue::as_str).unwrap_or("Contrast").to_owned();
                contrasts.push(ManovaContrast { name, factor, weights });
            }
        }
        let plot_factor = match v.get("plot_factor").and_then(JsonValue::as_str) {
            Some(name) => Some(column(name)?),
            None => None,
        };
        Ok(Self { effects: crate::project::linear_effects(v)?, statistic, contrasts, plot_factor, ..d })
    }
}

/// A contrast's multivariate test and its estimate for each response.
type ContrastResult = (HypothesisTest, Vec<f64>);

/// A computed MANOVA report.
#[derive(Debug, Clone)]
pub struct ManovaAnalysis {
    request: ManovaRequest,
    response_names: Vec<String>,
    predictor_names: Vec<String>,
    design: ModelDesign,
    fit: ManovaFit,
    /// The rows used.
    rows: Vec<usize>,
    /// Responses of the rows used, one vector per response.
    y: Vec<Vec<f64>>,
    /// Labels of each categorical predictor for every table row (`None`
    /// for continuous predictors).
    labels: Vec<Option<Vec<String>>>,
    contrasts: Vec<(ManovaContrast, Result<ContrastResult>)>,
    box_m: Option<std::result::Result<BoxMTest, String>>,
    n_excluded: usize,
    n_missing: usize,
}

impl ManovaAnalysis {
    /// Fit the model and test every effect and contrast.
    ///
    /// # Errors
    /// Fewer than two responses, a response that is not numeric or is also
    /// a predictor, no predictors, and any error from coding the design or
    /// from [`fit_manova`] (too few rows, collinear predictors, a singular
    /// error matrix). A contrast that cannot be tested is reported in its
    /// section instead.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &ManovaRequest) -> Result<Self> {
        if request.responses.len() < 2 {
            return Err(invalid("choose at least two responses"));
        }
        if request.responses.iter().any(|r| request.predictors.contains(r)) {
            return Err(invalid("a response cannot also be a predictor"));
        }
        let mut values = Vec::new();
        let mut response_names = Vec::new();
        for &i in &request.responses {
            let c = table.column(i).ok_or_else(|| invalid(format!("no column {i}")))?;
            values.push(c.as_numeric().ok_or_else(|| invalid(format!("the response {:?} must be numeric", c.name)))?);
            response_names.push(c.name.clone());
        }
        let spec = LinearModelSpec {
            response: request.responses[0],
            predictors: request.predictors.clone(),
            effects: request.effects.clone(),
            splines: Vec::new(),
        };
        let all_present = |r: usize| values.iter().all(|v| v[r].is_finite());
        let coded = Coded::with_rows(table, states, &spec, &all_present)?;
        let rows = coded.rows_used.clone();
        let y: Vec<Vec<f64>> = values.iter().map(|v| rows.iter().map(|&r| v[r]).collect()).collect();
        let effects: Vec<(String, std::ops::Range<usize>)> =
            coded.design.effects().iter().map(|e| (e.name.clone(), e.columns.clone())).collect();
        let fit = fit_manova(&coded.x, &y, &effects)?;
        let labels: Vec<Option<Vec<String>>> = coded
            .predictor_data
            .iter()
            .map(|d| match d {
                PredictorData::Categorical(l) => Some(l.clone()),
                PredictorData::Continuous(_) => None,
            })
            .collect();
        let mut analysis = Self {
            request: request.clone(),
            response_names,
            predictor_names: coded.predictor_names.clone(),
            design: coded.design,
            fit,
            rows,
            y,
            labels,
            contrasts: Vec::new(),
            box_m: None,
            n_excluded: coded.n_excluded,
            n_missing: coded.n_missing,
        };
        analysis.contrasts = request.contrasts.iter().map(|c| (c.clone(), analysis.contrast(c))).collect();
        analysis.box_m = analysis.box_m();
        Ok(analysis)
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &ManovaRequest {
        &self.request
    }

    /// The fitted model.
    #[must_use]
    pub fn fit(&self) -> &ManovaFit {
        &self.fit
    }

    /// Report title, e.g. `"MANOVA: tear, gloss, opacity"`.
    #[must_use]
    pub fn title(&self) -> String {
        format!("MANOVA: {}", self.response_names.join(", "))
    }

    /// Levels of predictor `k` in model order (`None` for a continuous one).
    fn levels(&self, k: usize) -> Option<&[String]> {
        match &self.design.terms().get(k)?.kind {
            TermKind::Categorical { levels } => Some(levels),
            TermKind::Continuous => None,
        }
    }

    /// The hypothesis row of a contrast and its estimate for each response.
    fn contrast(&self, c: &ManovaContrast) -> Result<ContrastResult> {
        let k = self
            .request
            .predictors
            .iter()
            .position(|&p| p == c.factor)
            .ok_or_else(|| invalid(format!("{}: the factor is not a predictor of the model", c.name)))?;
        let levels = self.levels(k).ok_or_else(|| invalid(format!("{}: the factor must be categorical", c.name)))?;
        let effect = self
            .design
            .effects()
            .iter()
            .find(|e| e.factors == [(k, 1)])
            .ok_or_else(|| invalid(format!("{}: the factor needs a main effect in the model", c.name)))?;
        let mut w = vec![0.0; levels.len()];
        for (level, weight) in &c.weights {
            let j = levels.iter().position(|l| l == level).ok_or_else(|| {
                invalid(format!("{}: {level:?} is not a level of the factor in the rows used", c.name))
            })?;
            w[j] = *weight;
        }
        if w.iter().all(|v| *v == 0.0) {
            return Err(invalid(format!("{}: give at least one nonzero weight", c.name)));
        }
        let total: f64 = w.iter().sum();
        let scale: f64 = w.iter().map(|v| v.abs()).sum();
        if total.abs() > 1e-9 * scale {
            return Err(invalid(format!("{}: the weights must add to zero", c.name)));
        }
        // Σ cⱼ μⱼ in terms of the effect's columns (levels 2…k).
        let mut l = vec![0.0; 1 + self.design.column_names().len()];
        for (j, col) in effect.columns.clone().enumerate() {
            l[1 + col] = match self.design.coding() {
                CategoricalCoding::Reference => w[j + 1],
                CategoricalCoding::SumToZero => w[j + 1] - w[0],
            };
        }
        let test = self.fit.test_hypothesis(&c.name, std::slice::from_ref(&l))?;
        let b = self.fit.coefficients();
        let estimates =
            (0..self.response_names.len()).map(|r| l.iter().zip(b).map(|(a, row)| a * row[r]).sum()).collect();
        Ok((test, estimates))
    }

    /// Box's M across the cells of the categorical predictors, when every
    /// predictor is categorical.
    fn box_m(&self) -> Option<std::result::Result<BoxMTest, String>> {
        if self.labels.iter().any(Option::is_none) {
            return None;
        }
        let mut cells: Vec<(Vec<&str>, Vec<Vec<f64>>)> = Vec::new();
        for (i, &r) in self.rows.iter().enumerate() {
            let key: Vec<&str> = self.labels.iter().flatten().map(|l| l[r].as_str()).collect();
            let row: Vec<f64> = self.y.iter().map(|v| v[i]).collect();
            match cells.iter_mut().find(|(k, _)| *k == key) {
                Some((_, rows)) => rows.push(row),
                None => cells.push((key, vec![row])),
            }
        }
        let groups: Vec<Vec<Vec<f64>>> = cells.into_iter().map(|(_, rows)| rows).collect();
        Some(box_m_test(&groups).map_err(|e| e.to_string()))
    }

    /// Report sections: summary, multivariate tests, Effect Pareto, any
    /// hierarchy note, univariate tests, contrasts, canonical analysis,
    /// canonical plot, residual correlations and Box's M.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary(), self.tests()];
        let s = self.request.statistic;
        out.extend(
            EffectPareto::new(
                self.fit.effect_tests().iter().map(|t| (t.name.clone(), t.test(s).p_value.unwrap_or(f64::NAN))),
                s.label(),
            )
            .section(),
        );
        out.extend(crate::report::hierarchy_note(&self.predictor_names, &self.request.effective_effects()));
        out.push(self.univariate());
        if !self.contrasts.is_empty() {
            out.extend(self.contrast_sections());
        }
        out.push(self.canonical());
        out.extend(self.canonical_plot());
        out.push(self.residuals());
        if let Some(b) = &self.box_m {
            out.push(self.box_m_section(b));
        }
        out
    }

    fn summary(&self) -> ReportSection {
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut push = |k: &str, v: String| t.push([k.to_owned(), v]);
        push("Responses", self.response_names.join(", "));
        let refs: Vec<&str> = self.predictor_names.iter().map(String::as_str).collect();
        let effects: Vec<String> =
            self.request.effective_effects().iter().map(|e| stat_linear::effect_name(&refs, e)).collect();
        push("Model effects", effects.join(", "));
        push("Rows used", self.rows.len().to_string());
        if self.n_excluded > 0 {
            push("Rows excluded", self.n_excluded.to_string());
        }
        if self.n_missing > 0 {
            push("Rows with a missing value", self.n_missing.to_string());
        }
        push("Error degrees of freedom", self.fit.df_error().to_string());
        push("Effect Pareto ranked by", self.request.statistic.label().to_owned());
        section(SectionKind::ManovaSummary, "Summary", SectionContent::Table(t))
    }

    fn test_rows(t: &mut TextTable, name: &str, h: &HypothesisTest) {
        for r in &h.tests {
            let kind = if r.exact {
                "exact"
            } else if r.statistic == MultivariateStatistic::Roy {
                "upper bound"
            } else {
                "approximate"
            };
            t.push([
                name.to_owned(),
                r.statistic.label().to_owned(),
                format_number(r.value, DIGITS),
                format_optional(r.f, DIGITS),
                format_number(r.df1, DIGITS),
                // McKeon's df2 is not positive when the error df barely exceeds p.
                if r.df2 > 0.0 { format_number(r.df2, DIGITS) } else { "—".to_owned() },
                r.p_value.map_or_else(|| "—".to_owned(), format_p_value),
                kind.to_owned(),
            ]);
        }
    }

    fn tests(&self) -> ReportSection {
        let mut t = TextTable::new(["Effect", "Statistic", "Value", "F", "Num DF", "Den DF", "p-value", "F is"]);
        for h in self.fit.effect_tests() {
            Self::test_rows(&mut t, &h.name, h);
        }
        section(SectionKind::ManovaTests, "Multivariate Tests", SectionContent::Table(t))
    }

    fn univariate(&self) -> ReportSection {
        let nu = self.fit.df_error();
        let mut t = TextTable::new(["Effect", "Response", "DF", "Sum of Squares", "F", "p-value"]);
        for h in self.fit.effect_tests() {
            for (name, u) in self.response_names.iter().zip(&h.univariate) {
                t.push([
                    h.name.clone(),
                    name.clone(),
                    format!("{}, {nu}", h.df),
                    format_number(u.sum_of_squares, DIGITS),
                    format_optional(u.f, DIGITS),
                    u.p_value.map_or_else(|| "—".to_owned(), format_p_value),
                ]);
            }
        }
        section(SectionKind::ManovaUnivariate, "Univariate Tests of Each Response", SectionContent::Table(t))
    }

    fn contrast_sections(&self) -> Vec<ReportSection> {
        let mut tests = TextTable::new(["Contrast", "Statistic", "Value", "F", "Num DF", "Den DF", "p-value", "F is"]);
        let mut header = vec!["Contrast".to_owned()];
        header.extend(self.response_names.iter().map(|n| format!("Estimate: {n}")));
        let mut estimates = TextTable::new(header);
        let mut notes = Vec::new();
        for (c, result) in &self.contrasts {
            match result {
                Ok((h, est)) => {
                    // One statistic suffices: with one hypothesis row all four agree.
                    let mut one = TextTable::new([""; 8]);
                    Self::test_rows(&mut one, &c.name, h);
                    tests.push(
                        one.rows[0]
                            .clone()
                            .into_iter()
                            .enumerate()
                            .map(|(i, v)| if i == 1 { "Wilks' Λ (all four agree)".to_owned() } else { v }),
                    );
                    let mut row = vec![c.name.clone()];
                    row.extend(est.iter().map(|v| format_number(*v, DIGITS)));
                    estimates.push(row);
                }
                Err(e) => notes.push(format!("{}: not tested ({e}).", c.name)),
            }
        }
        let mut out = Vec::new();
        if !tests.rows.is_empty() {
            out.push(section(SectionKind::ManovaContrasts, "Contrast Tests", SectionContent::Table(tests)));
            out.push(section(SectionKind::ManovaContrasts, "Contrast Estimates", SectionContent::Table(estimates)));
        }
        if !notes.is_empty() {
            out.push(section(
                SectionKind::ManovaContrasts,
                "Contrasts Not Tested",
                SectionContent::Note(notes.join(" ")),
            ));
        }
        out
    }

    /// Error standard deviation of each response, `√(E_jj/ν)`.
    fn error_sd(&self) -> Vec<f64> {
        let nu = self.fit.df_error() as f64;
        self.fit.error_sscp().iter().enumerate().map(|(j, r)| (r[j] / nu).sqrt()).collect()
    }

    fn canonical(&self) -> ReportSection {
        let mut header: Vec<String> =
            ["Effect", "Canonical variate", "Eigenvalue", "Percent", "Cumulative %", "Canonical correlation"]
                .map(str::to_owned)
                .to_vec();
        header.extend(self.response_names.iter().map(|n| format!("Std. coef.: {n}")));
        let mut t = TextTable::new(header);
        let sd = self.error_sd();
        for h in self.fit.effect_tests() {
            let total: f64 = h.eigenvalues.iter().sum();
            let mut cumulative = 0.0;
            for (k, ((l, r), a)) in h.eigenvalues.iter().zip(h.canonical_correlations()).zip(&h.canonical).enumerate() {
                let pct = if total > 0.0 { 100.0 * l / total } else { f64::NAN };
                cumulative += pct;
                let mut row = vec![
                    h.name.clone(),
                    (k + 1).to_string(),
                    format_number(*l, DIGITS),
                    format_number(pct, 4),
                    format_number(cumulative, 4),
                    format_number(r, DIGITS),
                ];
                row.extend(a.iter().zip(&sd).map(|(c, s)| format_number(c * s, DIGITS)));
                t.push(row);
            }
        }
        section(SectionKind::ManovaCanonical, "Canonical Analysis of Each Effect", SectionContent::Table(t))
    }

    /// The categorical main effect plotted: the requested factor, else the
    /// first categorical main effect.
    fn plot_effect(&self) -> Option<(usize, &HypothesisTest)> {
        let candidates: Vec<usize> = (0..self.request.predictors.len()).filter(|&k| self.levels(k).is_some()).collect();
        let k = match self.request.plot_factor {
            Some(c) => self.request.predictors.iter().position(|&p| p == c).filter(|k| candidates.contains(k))?,
            None => *candidates.iter().find(|&&k| self.design.effects().iter().any(|e| e.factors == [(k, 1)]))?,
        };
        let i = self.design.effects().iter().position(|e| e.factors == [(k, 1)])?;
        Some((k, &self.fit.effect_tests()[i]))
    }

    /// Canonical scores of the rows for `effect`: `aᵢᵀ(y − ȳ)` for each
    /// canonical variate.
    fn scores(&self, h: &HypothesisTest) -> Vec<Vec<f64>> {
        let means: Vec<f64> = self.y.iter().map(|v| v.iter().sum::<f64>() / v.len() as f64).collect();
        h.canonical
            .iter()
            .map(|a| {
                (0..self.rows.len())
                    .map(|i| a.iter().zip(&self.y).zip(&means).map(|((c, v), m)| c * (v[i] - m)).sum())
                    .collect()
            })
            .collect()
    }

    fn canonical_plot(&self) -> Option<ReportSection> {
        let (k, h) = self.plot_effect()?;
        let levels = self.levels(k)?.to_vec();
        let labels = self.labels[k].as_ref()?;
        let scores = self.scores(h);
        let membership: Vec<usize> =
            self.rows.iter().map(|&r| levels.iter().position(|l| *l == labels[r]).unwrap_or(0)).collect();
        let factor = &self.predictor_names[k];
        let mean_of = |v: &[f64], g: usize| {
            let (s, n) =
                v.iter().zip(&membership).filter(|(_, m)| **m == g).fold((0.0, 0), |(s, n), (x, _)| (s + x, n + 1));
            s / f64::from(n.max(1))
        };
        let plot = if scores.len() >= 2 {
            let points = self
                .rows
                .iter()
                .enumerate()
                .map(|(i, &row)| RowPoint { row, x: scores[0][i], y: scores[1][i] })
                .collect();
            let items = (0..levels.len())
                .map(|g| (mean_of(&scores[0], g), mean_of(&scores[1], g), format!("{} mean", levels[g]), 0))
                .collect();
            PlotSpec {
                x_label: "Canonical 1".to_owned(),
                y_label: "Canonical 2".to_owned(),
                points,
                groups: Some(PointGroups {
                    title: factor.clone(),
                    labels: levels.clone(),
                    membership,
                    lines: Vec::new(),
                }),
                annotations: Some(Annotations { groups: vec!["Level means".to_owned()], items, labels: Vec::new() }),
                ..PlotSpec::default()
            }
        } else {
            // One canonical variate: scores by level.
            let points = self
                .rows
                .iter()
                .enumerate()
                .map(|(i, &row)| RowPoint { row, x: membership[i] as f64 + 1.0, y: scores[0][i] })
                .collect();
            let lines = (0..levels.len())
                .map(|g| {
                    let m = mean_of(&scores[0], g);
                    let x = g as f64 + 1.0;
                    vec![[x - 0.3, m], [x + 0.3, m]]
                })
                .collect();
            PlotSpec {
                x_label: factor.clone(),
                y_label: "Canonical 1".to_owned(),
                points,
                groups: Some(PointGroups { title: factor.clone(), labels: levels.clone(), membership, lines }),
                x_ticks: Some(levels.iter().enumerate().map(|(g, l)| (g as f64 + 1.0, l.clone())).collect()),
                ..PlotSpec::default()
            }
        };
        let title = format!("Canonical Plot: {factor}");
        Some(section(SectionKind::ManovaCanonicalPlot, &title, SectionContent::Plot(Box::new(plot))))
    }

    fn residuals(&self) -> ReportSection {
        let mut header = vec!["Response".to_owned(), "Error SD".to_owned()];
        header.extend(self.response_names.iter().cloned());
        let mut t = TextTable::new(header);
        let r = self.fit.residual_correlations();
        for ((name, row), sd) in self.response_names.iter().zip(&r).zip(self.error_sd()) {
            let mut cells = vec![name.clone(), format_number(sd, DIGITS)];
            cells.extend(row.iter().map(|v| format_number(*v, 4)));
            t.push(cells);
        }
        section(
            SectionKind::ManovaResiduals,
            "Residual Correlations (Partial Correlations Given the Model)",
            SectionContent::Table(t),
        )
    }

    fn box_m_section(&self, b: &std::result::Result<BoxMTest, String>) -> ReportSection {
        let title = "Equal Covariance Matrices (Box's M)";
        match b {
            Ok(b) => {
                let mut t = TextTable::new(["Statistic", "Value"]);
                t.push(["Box's M".to_owned(), format_number(b.m, DIGITS)]);
                t.push(["Chi-square".to_owned(), format_number(b.chi_square, DIGITS)]);
                t.push(["DF".to_owned(), format_number(b.df, DIGITS)]);
                t.push(["p-value".to_owned(), format_p_value(b.p_value)]);
                section(SectionKind::ManovaBoxM, title, SectionContent::Table(t))
            }
            Err(e) => section(SectionKind::ManovaBoxM, title, SectionContent::Note(format!("Not available: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::plastic_film;

    fn request(t: &DataTable) -> ManovaRequest {
        let c = |n: &str| t.find(n).unwrap();
        ManovaRequest {
            effects: vec![vec![(0, 1)], vec![(1, 1)], vec![(0, 1), (1, 1)]],
            ..ManovaRequest::new(
                vec![c("Tear resistance"), c("Gloss"), c("Opacity")],
                vec![c("Extrusion rate"), c("Additive")],
            )
        }
    }

    #[test]
    fn plastic_film_report_matches_the_engine() {
        let t = plastic_film();
        let rows = RowStateManager::new(t.n_rows());
        let a = ManovaAnalysis::run(&t, &rows, &request(&t)).unwrap();
        let tests = a.fit().effect_tests();
        assert_eq!(tests.len(), 3);
        // Johnson & Wichern Example 6.13: Wilks' Λ for the extrusion rate.
        assert!((tests[0].test(MultivariateStatistic::Wilks).value - 0.381858384661).abs() < 1e-9);
        let s = a.sections();
        let kinds: Vec<SectionKind> = s.iter().map(|x| x.kind).collect();
        assert!(kinds.contains(&SectionKind::EffectPareto));
        assert!(kinds.contains(&SectionKind::ManovaBoxM));
        let SectionContent::Table(mt) = &s[1].content else { panic!("tests table") };
        assert_eq!(mt.rows.len(), 12);
        // Canonical plot of the first factor (two levels: one variate).
        let plot = s.iter().find(|x| x.kind == SectionKind::ManovaCanonicalPlot).unwrap();
        let SectionContent::Plot(p) = &plot.content else { panic!("plot") };
        assert_eq!(p.points.len(), 20);
        assert!(p.x_ticks.is_some());
        assert_eq!(p.groups.as_ref().unwrap().labels.len(), 2);
    }

    #[test]
    fn contrasts_json_and_exclusions() {
        let t = plastic_film();
        let mut rows = RowStateManager::new(t.n_rows());
        rows.set_excluded([0], true);
        let c = |n: &str| t.find(n).unwrap();
        let mut r = request(&t);
        r.contrasts = vec![
            ManovaContrast {
                name: "High vs Low rate".to_owned(),
                factor: c("Extrusion rate"),
                weights: vec![("High".to_owned(), 1.0), ("Low".to_owned(), -1.0)],
            },
            ManovaContrast { name: "Bad".to_owned(), factor: c("Additive"), weights: vec![("High".to_owned(), 1.0)] },
        ];
        r.statistic = MultivariateStatistic::Wilks;
        let a = ManovaAnalysis::run(&t, &rows, &r).unwrap();
        assert_eq!(a.fit().n(), 19);
        // A two-level contrast tests the same hypothesis as the effect.
        let (h, est) = a.contrasts[0].1.as_ref().unwrap();
        let effect = &a.fit().effect_tests()[0];
        assert!(
            (h.test(MultivariateStatistic::Wilks).value - effect.test(MultivariateStatistic::Wilks).value).abs()
                < 1e-10
        );
        assert_eq!(est.len(), 3);
        assert!(a.contrasts[1].1.is_err(), "weights must add to zero");
        let s = a.sections();
        assert!(s.iter().any(|x| x.title == "Contrasts Not Tested"));
        // Round trip through the project form.
        let back = ManovaRequest::from_json(&r.to_json(&t), &t).unwrap();
        assert_eq!(back, r);
        // One response is refused.
        let one = ManovaRequest { responses: vec![c("Gloss")], ..request(&t) };
        assert!(ManovaAnalysis::run(&t, &rows, &one).is_err());
    }

    /// Error df equal to the number of responses (5 rows, 3 groups, 2
    /// responses): the Hotelling–Lawley df2 = 2(s·N + 1) is 0 there, shown
    /// as "—" rather than a meaningless number.
    #[test]
    fn non_positive_denominator_df_shows_a_dash() {
        use stat_data::Column;
        let t = DataTable::new(
            "m",
            vec![
                Column::numeric("Y1", vec![1.0, 2.5, 4.0, 3.2, 7.1]),
                Column::numeric("Y2", vec![2.0, 1.1, 5.0, 6.3, 2.2]),
                Column::text("G", ["a", "a", "b", "b", "c"].iter().map(|s| (*s).to_owned()).collect()),
            ],
        )
        .unwrap();
        let a = ManovaAnalysis::run(&t, &RowStateManager::new(5), &ManovaRequest::new(vec![0, 1], vec![2])).unwrap();
        let s = a.sections();
        let tests = s.iter().find(|x| x.kind == SectionKind::ManovaTests).unwrap();
        let SectionContent::Table(mt) = &tests.content else { panic!("tests table") };
        let lawley = mt.rows.iter().find(|r| r[1] == MultivariateStatistic::HotellingLawley.label()).unwrap();
        assert_eq!(lawley[5], "—");
    }
}
