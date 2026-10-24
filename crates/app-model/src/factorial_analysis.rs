//! Analysis of a two-level factorial experiment held in a table
//! ([`stat_doe::factorial_analysis`]): effects in coded units with `t`
//! tests or Lenth's method, normal and half-normal plots of the effects,
//! main-effect, interaction and cube plots of the data means or the fitted
//! model, and dispersion effects.
//!
//! Factors are coded as in [`crate::declared_design`]: a numeric factor's
//! low and high settings (by default the data's smallest and largest values)
//! become −1 and +1, and a two-level categorical factor is coded −1 and +1 in
//! sorted order. The model is fitted at the coded settings actually run, so a
//! run that missed its target keeps the effects unbiased (the report says so).
//!
//! A continuous response is fitted by least squares. A binary response (0/1,
//! or any two-level column) is fitted by logistic regression and a count by
//! Poisson regression ([`stat_discrete::fit_glm`]); their effects are on the
//! link scale (log odds, log mean) with Wald `z` tests. A saturated Poisson
//! model is fitted exactly, and its effects are also judged by Lenth's
//! method.

use crate::declared_design::DeclaredFactor;
use crate::format::{format_number, format_p_value};
use crate::project::{invalid, member, obj, s};
use crate::report::{
    Annotations, EffectPareto, PlotPane, PlotSpec, PointGroups, ReportSection, RowPoint, SectionContent, SectionKind,
    TextTable,
};
use stat_core::{Matrix, Result};
use stat_data::{DataTable, JsonValue, MeasurementLevel, RowStateManager};
use stat_discrete::{Family, GlmOptions, Link, fit_glm};
use stat_doe::ModelFormula;
use stat_doe::factorial_analysis::{
    Activity, DispersionTest, EffectSet, EffectTest, OffTarget, ReplicatedDispersion, cube_means, estimable_terms,
    fit_factorial, half_normal_plot_positions, interaction_means, level_means, normal_plot_positions, off_target,
    replicated_dispersion, saturated_poisson, term_labels, unreplicated_dispersion,
};

/// Significant digits in report tables.
const DIGITS: usize = 5;

/// Most factors shown in interaction plots (their pairs).
const MAX_INTERACTION_FACTORS: usize = 5;

/// Which terms the model has.
#[derive(Debug, Clone, PartialEq)]
pub enum FactorialTerms {
    /// Every interaction the runs can estimate, up to the saturated model.
    All,
    /// Interactions of at most this many factors (1: main effects only).
    Order(usize),
    /// A formula over the factor column names, e.g. `"A + B + C + A*B"`.
    Formula(String),
}

/// The kind of response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactorialResponse {
    /// Continuous: least squares.
    Continuous,
    /// Binary (0/1 or two levels): logistic regression.
    Binary,
    /// Count: Poisson regression with the log link.
    Count,
}

impl FactorialResponse {
    /// Every kind, in menu order.
    pub const ALL: [Self; 3] = [Self::Continuous, Self::Binary, Self::Count];

    /// Project-file key.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Continuous => "continuous",
            Self::Binary => "binary",
            Self::Count => "count",
        }
    }

    /// Read a project-file key.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.key() == key)
    }

    /// Display name with the fitting method.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Continuous => "Continuous (least squares)",
            Self::Binary => "Binary (logistic regression, logit link)",
            Self::Count => "Count (Poisson regression, log link)",
        }
    }
}

/// Whether plots show data means or model predictions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeansSource {
    /// Means of the observed responses.
    Data,
    /// Predictions of the fitted model (factors not shown at their centre).
    Model,
}

impl MeansSource {
    /// Project-file key.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Data => "data",
            Self::Model => "model",
        }
    }
}

/// A factorial analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct FactorialAnalysisRequest {
    /// Factor columns with optional low and high settings.
    pub factors: Vec<DeclaredFactor>,
    /// Response column.
    pub response: usize,
    /// Model terms.
    pub terms: FactorialTerms,
    /// Kind of response.
    pub response_type: FactorialResponse,
    /// Significance level for marking active effects.
    pub alpha: f64,
    /// Means in the main-effect, interaction and cube plots.
    pub means: MeansSource,
    /// Factor columns of the cube plot (empty: the first three factors).
    pub cube: Vec<usize>,
}

impl FactorialAnalysisRequest {
    /// Factors with ranges from the data, a continuous response, every
    /// estimable term, α = 0.05, data means.
    #[must_use]
    pub fn new(factors: &[usize], response: usize) -> Self {
        Self {
            factors: factors.iter().map(|&column| DeclaredFactor { column, low: None, high: None }).collect(),
            response,
            terms: FactorialTerms::All,
            response_type: FactorialResponse::Continuous,
            alpha: 0.05,
            means: MeansSource::Data,
            cube: Vec::new(),
        }
    }

    /// Columns used.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        self.factors.iter().map(|f| f.column).chain(std::iter::once(self.response)).collect()
    }

    /// The same request with columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(mut self, f: impl Fn(usize) -> usize) -> Self {
        self.factors.iter_mut().for_each(|d| d.column = f(d.column));
        self.response = f(self.response);
        self.cube.iter_mut().for_each(|c| *c = f(*c));
        self
    }

    /// The project-file form (`"analysis": "factorial-analysis"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let name = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        let num = |v: Option<f64>| v.map_or(JsonValue::Null, JsonValue::number);
        let terms = match &self.terms {
            FactorialTerms::All => s("all"),
            FactorialTerms::Order(n) => JsonValue::number(*n as f64),
            FactorialTerms::Formula(f) => s(f),
        };
        obj([
            ("analysis", s("factorial-analysis")),
            (
                "factors",
                JsonValue::Array(
                    self.factors
                        .iter()
                        .map(|f| obj([("column", name(f.column)), ("low", num(f.low)), ("high", num(f.high))]))
                        .collect(),
                ),
            ),
            ("response", name(self.response)),
            ("terms", terms),
            ("response-type", s(self.response_type.key())),
            ("alpha", JsonValue::number(self.alpha)),
            ("means", s(self.means.key())),
            ("cube", JsonValue::Array(self.cube.iter().map(|&c| name(c)).collect())),
        ])
    }

    /// Read the project-file form. Factors may be plain column names; every
    /// member but `factors` and `response` is optional.
    ///
    /// # Errors
    /// Missing or unknown columns, or unknown keys.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let find = |name: Option<&str>, what: &str| -> Result<usize> {
            let name = name.ok_or_else(|| invalid(format!("{what}: expected column names")))?;
            table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
        };
        let list = member(v, "factors")?.as_array().ok_or_else(|| invalid("factors: expected a list"))?;
        let factors = list
            .iter()
            .map(|f| match f {
                JsonValue::String(n) => Ok(DeclaredFactor { column: find(Some(n), "factors")?, low: None, high: None }),
                other => Ok(DeclaredFactor {
                    column: find(other.get("column").and_then(JsonValue::as_str), "factors")?,
                    low: other.get("low").and_then(JsonValue::as_f64),
                    high: other.get("high").and_then(JsonValue::as_f64),
                }),
            })
            .collect::<Result<_>>()?;
        let response = find(member(v, "response")?.as_str(), "response")?;
        let terms = match v.get("terms") {
            None | Some(JsonValue::Null) => FactorialTerms::All,
            Some(t) => match (t.as_f64(), t.as_str()) {
                (Some(n), _) if n >= 1.0 => FactorialTerms::Order(n as usize),
                (_, Some("all")) => FactorialTerms::All,
                (_, Some(f)) => FactorialTerms::Formula(f.to_owned()),
                _ => return Err(invalid("terms: expected \"all\", an interaction order or a formula")),
            },
        };
        let response_type = match v.get("response-type").and_then(JsonValue::as_str) {
            None => FactorialResponse::Continuous,
            Some(k) => FactorialResponse::from_key(k)
                .ok_or_else(|| invalid(format!("factorial-analysis: unknown response type {k:?}")))?,
        };
        let means = match v.get("means").and_then(JsonValue::as_str) {
            None | Some("data") => MeansSource::Data,
            Some("model") => MeansSource::Model,
            Some(k) => return Err(invalid(format!("factorial-analysis: unknown means {k:?}"))),
        };
        let cube = match v.get("cube").and_then(JsonValue::as_array) {
            None => Vec::new(),
            Some(c) => c.iter().map(|n| find(n.as_str(), "cube")).collect::<Result<_>>()?,
        };
        Ok(Self {
            factors,
            response,
            terms,
            response_type,
            alpha: v.get("alpha").and_then(JsonValue::as_f64).unwrap_or(0.05),
            means,
            cube,
        })
    }
}

/// How the effects were estimated.
#[derive(Debug, Clone)]
enum Fit {
    /// Least squares.
    LeastSquares { rss: f64 },
    /// A GLM (iteratively reweighted least squares or a saturated Poisson fit).
    Glm { link: Link, deviance: Option<f64>, warnings: Vec<String> },
}

/// Dispersion analysis of a continuous response.
#[derive(Debug, Clone)]
enum Dispersion {
    Replicated(ReplicatedDispersion),
    Unreplicated { location: ModelFormula, tests: Vec<DispersionTest> },
    Unavailable(String),
}

/// A computed factorial analysis.
#[derive(Debug, Clone)]
pub struct FactorialAnalysis {
    request: FactorialAnalysisRequest,
    factor_names: Vec<String>,
    /// Labels of the −1 and +1 settings of each factor.
    level_labels: Vec<(String, String)>,
    response_name: String,
    /// The level coded 1 of a binary response.
    event: Option<String>,
    rows: Vec<usize>,
    runs: Vec<Vec<f64>>,
    y: Vec<f64>,
    formula: ModelFormula,
    dropped: Vec<String>,
    coefficients: Vec<f64>,
    effects: EffectSet,
    error_df: usize,
    fit: Fit,
    off_target: OffTarget,
    dispersion: Option<Dispersion>,
    n_excluded: usize,
}

impl FactorialAnalysis {
    /// Fit the model to the non-excluded rows with every factor setting and a
    /// response.
    ///
    /// # Errors
    /// No factors, a column used twice, a categorical factor without exactly
    /// two levels, a factor that does not vary, a response of the wrong kind,
    /// a bad formula, too few runs, or a fit that fails.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &FactorialAnalysisRequest) -> Result<Self> {
        if request.factors.is_empty() {
            return Err(invalid("choose at least one factor"));
        }
        let all = request.columns();
        if all.iter().enumerate().any(|(i, c)| all[..i].contains(c)) {
            return Err(invalid("each column can have only one role"));
        }
        if !(request.alpha > 0.0 && request.alpha < 0.5) {
            return Err(invalid("α must lie between 0 and 0.5"));
        }
        let col = |i: usize| table.column(i).ok_or_else(|| invalid(format!("no column {i}")));
        // Factor values: numeric continuous columns as they are; categorical
        // ones with two levels as 0/1 in sorted order.
        let mut values: Vec<Vec<f64>> = Vec::new();
        let mut categorical: Vec<Option<(String, String)>> = Vec::new();
        for f in &request.factors {
            let c = col(f.column)?;
            match c.as_numeric().filter(|_| c.level == MeasurementLevel::Continuous) {
                Some(v) => {
                    values.push(v.to_vec());
                    categorical.push(None);
                }
                None => {
                    let (codes, levels) = two_levels(c, table.n_rows())?;
                    values.push(codes);
                    categorical.push(Some(levels));
                }
            }
        }
        // Response.
        let rc = col(request.response)?;
        let (yv, event): (Vec<f64>, Option<String>) = match request.response_type {
            FactorialResponse::Binary => {
                let numeric01 = rc.as_numeric().filter(|v| v.iter().all(|x| x.is_nan() || *x == 0.0 || *x == 1.0));
                match numeric01 {
                    Some(v) => (v.to_vec(), Some("1".to_owned())),
                    None => {
                        let (codes, (_, hi)) = two_levels(rc, table.n_rows())
                            .map_err(|_| invalid(format!("a binary response needs two levels: {}", rc.name)))?;
                        (codes, Some(hi))
                    }
                }
            }
            _ => {
                let v = rc.as_numeric().ok_or_else(|| invalid(format!("the response {} must be numeric", rc.name)))?;
                if request.response_type == FactorialResponse::Count
                    && v.iter().any(|x| x.is_finite() && (*x < 0.0 || x.fract() != 0.0))
                {
                    return Err(invalid(format!("{} must hold counts (whole numbers, not negative)", rc.name)));
                }
                (v.to_vec(), None)
            }
        };
        let complete: Vec<usize> =
            (0..table.n_rows()).filter(|&r| values.iter().all(|v| v[r].is_finite()) && yv[r].is_finite()).collect();
        let rows: Vec<usize> = complete.iter().copied().filter(|&r| !states.is_excluded(r)).collect();
        let n_excluded = complete.len() - rows.len();
        if rows.len() < 2 {
            return Err(invalid("fewer than two runs have every factor setting and a response"));
        }
        let mut factor_names = Vec::new();
        let mut level_labels = Vec::new();
        let mut ranges = Vec::new();
        for ((f, v), cat) in request.factors.iter().zip(&values).zip(&categorical) {
            let name = col(f.column)?.name.clone();
            let (lo, hi) =
                rows.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &r| (a.min(v[r]), b.max(v[r])));
            let (low, high) = if cat.is_some() { (0.0, 1.0) } else { (f.low.unwrap_or(lo), f.high.unwrap_or(hi)) };
            if !(low.is_finite() && high.is_finite()) || low == high {
                return Err(invalid(format!("{name} needs different low and high settings (it may not vary)")));
            }
            level_labels.push(match cat {
                Some((a, b)) => (a.clone(), b.clone()),
                None => (format_number(low, DIGITS), format_number(high, DIGITS)),
            });
            factor_names.push(name);
            ranges.push((low, high));
        }
        let runs: Vec<Vec<f64>> = rows
            .iter()
            .map(|&r| values.iter().zip(&ranges).map(|(v, &(lo, hi))| (2.0 * v[r] - (lo + hi)) / (hi - lo)).collect())
            .collect();
        let y: Vec<f64> = rows.iter().map(|&r| yv[r]).collect();
        let k = factor_names.len();
        let wanted = match &request.terms {
            FactorialTerms::All => ModelFormula::interactions_up_to(k, k),
            FactorialTerms::Order(n) => ModelFormula::interactions_up_to(k, (*n).max(1)),
            FactorialTerms::Formula(text) => ModelFormula::parse(text, &factor_names)?,
        };
        stat_doe::factorial_analysis::check_two_level(&wanted, &factor_names)?;
        let (formula, dropped) = estimable_terms(&runs, &wanted, &factor_names);
        if formula.n_terms() < 2 {
            return Err(invalid("the runs cannot estimate any effect (does every factor vary?)"));
        }
        let p = formula.n_terms();
        let n = runs.len();
        let terms = term_labels(&formula, &factor_names);
        let alpha = request.alpha;
        let (coefficients, effects, error_df, fit) = match request.response_type {
            FactorialResponse::Continuous => {
                let f = fit_factorial(&runs, &y, &formula, &factor_names, alpha)?;
                (f.coefficients, f.effects, f.error_df, Fit::LeastSquares { rss: f.rss })
            }
            FactorialResponse::Count if n == p => {
                let (b, e) = saturated_poisson(&runs, &y, &formula, &factor_names, alpha)?;
                (b, e, 0, Fit::Glm { link: Link::Log, deviance: Some(0.0), warnings: Vec::new() })
            }
            FactorialResponse::Binary if n == p => {
                return Err(invalid(
                    "a binary response needs more runs than model terms (a saturated logistic model has infinite \
                     estimates): choose a lower interaction order",
                ));
            }
            kind => {
                let family = if kind == FactorialResponse::Binary { Family::Binomial } else { Family::Poisson };
                let cols: Vec<Vec<f64>> =
                    formula.terms()[1..].iter().map(|t| runs.iter().map(|r| t.eval(r)).collect()).collect();
                let x = Matrix::from_columns(&cols)?;
                let mut options = GlmOptions::new(family);
                options.names = terms.iter().map(|t| t.0.clone()).collect();
                let g = fit_glm(&x, &y, &options)?;
                let b: Vec<f64> = g.coefficients.iter().map(|c| c.estimate).collect();
                let se: Vec<f64> = g.coefficients[1..].iter().map(|c| 2.0 * c.std_error).collect();
                let e = EffectSet::new(&terms, &b[1..], Some((&se, EffectTest::Z)), false, alpha)?;
                let fit = Fit::Glm { link: g.link, deviance: Some(g.deviance), warnings: g.warnings.clone() };
                (b, e, g.df_residual, fit)
            }
        };
        let dispersion = (request.response_type == FactorialResponse::Continuous).then(|| {
            let replicated = settings_with_replicates(&runs) >= 3;
            if replicated {
                match replicated_dispersion(&runs, &y, &formula, &factor_names, alpha) {
                    Ok(d) => Dispersion::Replicated(d),
                    Err(e) => Dispersion::Unavailable(crate::clipboard::error_text(&e)),
                }
            } else {
                let location = if error_df > 0 {
                    formula.clone()
                } else {
                    let active: Vec<Vec<u8>> = formula.terms()[1..]
                        .iter()
                        .zip(&effects.effects)
                        .filter(|(_, e)| e.activity != Activity::Inactive)
                        .map(|(t, _)| t.powers.clone())
                        .collect();
                    ModelFormula::from_powers(k, active)
                };
                match unreplicated_dispersion(&runs, &y, &location, &factor_names) {
                    Ok(tests) => Dispersion::Unreplicated { location, tests },
                    Err(e) => Dispersion::Unavailable(crate::clipboard::error_text(&e)),
                }
            }
        });
        Ok(Self {
            request: request.clone(),
            factor_names,
            level_labels,
            response_name: rc.name.clone(),
            event,
            off_target: off_target(&runs),
            rows,
            runs,
            y,
            formula,
            dropped,
            coefficients,
            effects,
            error_df,
            fit,
            dispersion,
            n_excluded,
        })
    }

    /// The estimated effects.
    #[must_use]
    pub fn effects(&self) -> &EffectSet {
        &self.effects
    }

    /// The fitted model's terms.
    #[must_use]
    pub fn formula(&self) -> &ModelFormula {
        &self.formula
    }

    /// Prediction at coded point `x` on the response scale (a probability or
    /// mean count for a GLM).
    #[must_use]
    pub fn predict(&self, x: &[f64]) -> f64 {
        let eta: f64 = self.formula.expand(x).iter().zip(&self.coefficients).map(|(f, b)| f * b).sum();
        match &self.fit {
            Fit::LeastSquares { .. } => eta,
            Fit::Glm { link, .. } => link.inverse(eta),
        }
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Factorial Analysis: {}", self.response_name)
    }

    fn scale_note(&self) -> &'static str {
        match self.request.response_type {
            FactorialResponse::Continuous => "",
            FactorialResponse::Binary => " (log odds)",
            FactorialResponse::Count => " (log mean)",
        }
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![
            section(SectionKind::FactorialSummary, "Factorial Summary", SectionContent::Table(self.summary())),
            section(
                SectionKind::FactorialEffects,
                &format!("Effect Estimates{}", self.scale_note()),
                SectionContent::Table(effects_table(&self.effects, self.request.response_type)),
            ),
        ];
        let test = match (&self.effects.test, &self.effects.lenth) {
            (Some(EffectTest::T(df)), _) => Some(format!("t tests on {df} df")),
            (Some(EffectTest::Z), _) => Some("Wald z tests".to_owned()),
            (None, Some(_)) => Some("Lenth's pseudo standard error".to_owned()),
            (None, None) => None,
        };
        if let Some(test) = test {
            let p = |e: &stat_doe::factorial_analysis::EffectEstimate| {
                if e.p_value.is_finite() { e.p_value } else { e.lenth_p }
            };
            if let Some(s) =
                EffectPareto::new(self.effects.effects.iter().map(|e| (e.term.clone(), p(e))), test).section()
            {
                out.push(s);
            }
        }
        if self.effects.effects.len() >= 2 {
            let what = format!("Effect{}", self.scale_note());
            out.push(section(
                SectionKind::FactorialNormalPlot,
                "Normal Plot of Effects",
                effect_plot(&self.effects, false, &what),
            ));
            out.push(section(
                SectionKind::FactorialHalfNormalPlot,
                "Half-Normal Plot of Effects",
                effect_plot(&self.effects, true, &what),
            ));
        }
        let source = match self.request.means {
            MeansSource::Data => "data means",
            MeansSource::Model => "model predictions",
        };
        out.push(section(SectionKind::FactorialMainEffects, &format!("Main Effects ({source})"), self.main_effects()));
        let k = self.factor_names.len().min(MAX_INTERACTION_FACTORS);
        for a in 0..k {
            for b in a + 1..k {
                out.push(section(
                    SectionKind::FactorialInteraction,
                    &format!("Interaction: {} × {} ({source})", self.factor_names[a], self.factor_names[b]),
                    self.interaction_plot(a, b),
                ));
            }
        }
        if let Some((cube, title)) = self.cube() {
            out.push(section(SectionKind::FactorialCube, &format!("{title} ({source})"), cube));
        }
        match &self.dispersion {
            Some(Dispersion::Replicated(d)) => {
                out.push(section(
                    SectionKind::FactorialDispersion,
                    "Standard Deviation at Each Setting",
                    SectionContent::Table(self.settings_table(d)),
                ));
                out.push(section(
                    SectionKind::FactorialDispersion,
                    "Dispersion Effects (ln s)",
                    SectionContent::Table(effects_table(&d.effects, FactorialResponse::Continuous)),
                ));
                if d.effects.effects.len() >= 3 {
                    out.push(section(
                        SectionKind::FactorialDispersionPlot,
                        "Normal Plot of Dispersion Effects",
                        effect_plot(&d.effects, false, "Effect on ln s"),
                    ));
                }
            }
            Some(Dispersion::Unreplicated { location, tests }) => {
                out.push(section(
                    SectionKind::FactorialDispersion,
                    "Dispersion Effects (unreplicated)",
                    SectionContent::Table(self.dispersion_tests(tests)),
                ));
                out.push(section(
                    SectionKind::FactorialNote,
                    "Dispersion Method",
                    SectionContent::Note(format!(
                        "Location model: {}. For each factor the location model is fitted separately to the runs at \
                         its low and its high setting; D = (RSS₊/df₊)/(RSS₋/df₋) is compared with F(df₊, df₋), \
                         two-sided (Bergman & Hynén 1997). Box–Meyer ln ratio = ln(Σr²₊/Σr²₋) from the location \
                         model's residuals (Box & Meyer 1986). Values far from 1 (or from 0 for the ln ratio) \
                         suggest the factor changes the variability.",
                        location.describe(&self.factor_names)
                    )),
                ));
            }
            Some(Dispersion::Unavailable(why)) => out.push(section(
                SectionKind::FactorialNote,
                "Dispersion Effects",
                SectionContent::Note(format!("Dispersion effects are not available: {why}")),
            )),
            None => {}
        }
        if let Some(note) = self.notes() {
            out.push(section(SectionKind::FactorialNote, "Notes", SectionContent::Note(note)));
        }
        out
    }

    fn summary(&self) -> TextTable {
        let mut t = TextTable::new(["Item", "Value"]);
        let mut row = |a: &str, b: String| t.push([a.to_owned(), b]);
        let response = match &self.event {
            Some(e) => format!("{} (probability of {e})", self.response_name),
            None => self.response_name.clone(),
        };
        row("Response", response);
        row("Response type", self.request.response_type.label().to_owned());
        row("Model", self.formula.describe(&self.factor_names));
        row("Runs", self.runs.len().to_string());
        row("Effects estimated", self.effects.effects.len().to_string());
        if !self.dropped.is_empty() {
            row("Terms left out (aliased or beyond the saturated model)", self.dropped.join(", "));
        }
        row("Error degrees of freedom", self.error_df.to_string());
        match &self.fit {
            Fit::LeastSquares { rss } if self.error_df > 0 => {
                row("Root mean square error", format_number((rss / self.error_df as f64).sqrt(), DIGITS));
            }
            Fit::Glm { deviance: Some(d), .. } => row("Residual deviance", format_number(*d, DIGITS)),
            _ => {}
        }
        if let Some(l) = &self.effects.lenth {
            row("Lenth's pseudo standard error (PSE)", format_number(l.pse, DIGITS));
            row(&format!("Margin of error ME (t on {} df)", format_number(l.df, 4)), format_number(l.me, DIGITS));
            row("Simultaneous margin of error SME", format_number(l.sme, DIGITS));
        }
        row("α", format_number(self.effects.alpha, 4));
        let active = self.effects.effects.iter().filter(|e| e.activity != Activity::Inactive).count();
        row("Active effects", active.to_string());
        if self.n_excluded > 0 {
            row("Rows excluded", self.n_excluded.to_string());
        }
        t
    }

    fn notes(&self) -> Option<String> {
        let mut notes = Vec::new();
        if self.off_target.runs > 0 {
            notes.push(format!(
                "{} run{} missed {} target settings (largest deviation {} in coded units). The effects are \
                 estimated at the settings actually run, so they stay unbiased, but the design is no longer exactly \
                 orthogonal: standard errors differ a little between effects, and the main-effect, interaction and \
                 cube means group runs by the sign of their coded settings.",
                self.off_target.runs,
                if self.off_target.runs == 1 { "" } else { "s" },
                if self.off_target.runs == 1 { "its" } else { "their" },
                format_number(self.off_target.max_deviation, 3)
            ));
        }
        if self.effects.lenth.is_some() && self.effects.test.is_none() {
            notes.push(
                "No error degrees of freedom: effects are judged by Lenth's method, which assumes most effects are \
                 inert. Effects beyond ME are marked active; beyond SME, active allowing for all comparisons."
                    .to_owned(),
            );
        }
        if let Fit::Glm { warnings, .. } = &self.fit {
            notes.extend(warnings.iter().cloned());
            if self.effects.lenth.is_some() {
                notes.push(
                    "The Poisson model is saturated: Wald tests are shown, and Lenth's method marks the active \
                     effects."
                        .to_owned(),
                );
            }
        }
        (!notes.is_empty()).then(|| notes.join(" "))
    }

    /// Mean (data or model) at the low/high setting of factor `j`.
    fn level_values(&self, j: usize) -> [f64; 2] {
        match self.request.means {
            MeansSource::Data => {
                let m = &level_means(&self.runs, &self.y)[j];
                [m[0].mean, m[1].mean]
            }
            MeansSource::Model => [-1.0, 1.0].map(|s| self.predict(&self.point(&[(j, s)]))),
        }
    }

    /// A coded point with the given factors set and the rest at their centre.
    fn point(&self, set: &[(usize, f64)]) -> Vec<f64> {
        let mut x = vec![0.0; self.factor_names.len()];
        set.iter().for_each(|&(j, v)| x[j] = v);
        x
    }

    fn main_effects(&self) -> SectionContent {
        let k = self.factor_names.len();
        let offsets: Vec<f64> = (0..k).map(|j| 3.0 * j as f64).collect();
        let mut points = Vec::new();
        let mut membership = Vec::new();
        for (j, &off) in offsets.iter().enumerate() {
            for (i, r) in self.runs.iter().enumerate() {
                let x = if r[j] > 0.5 {
                    2.0
                } else if r[j] < -0.5 {
                    1.0
                } else {
                    1.5
                };
                points.push(RowPoint { row: self.rows[i], x: off + x, y: self.y[i] });
                membership.push(j);
            }
        }
        let lines = offsets
            .iter()
            .enumerate()
            .map(|(j, &off)| {
                let m = self.level_values(j);
                [[off + 1.0, m[0]], [off + 2.0, m[1]]].into_iter().filter(|p| p[1].is_finite()).collect()
            })
            .collect();
        let x_ticks = offsets
            .iter()
            .zip(&self.level_labels)
            .flat_map(|(&off, (lo, hi))| [(off + 1.0, lo.clone()), (off + 2.0, hi.clone())])
            .collect();
        let panes = offsets
            .iter()
            .zip(&self.factor_names)
            .map(|(&off, name)| PlotPane { label: name.clone(), x_min: off + 0.5, x_max: off + 2.5 })
            .collect();
        SectionContent::Plot(Box::new(PlotSpec {
            x_label: "Factor setting".to_owned(),
            y_label: self.response_name.clone(),
            points,
            groups: Some(PointGroups {
                title: "Factor".to_owned(),
                labels: self.factor_names.clone(),
                membership,
                lines,
            }),
            x_ticks: Some(x_ticks),
            panes,
            ..PlotSpec::default()
        }))
    }

    fn interaction_plot(&self, a: usize, b: usize) -> SectionContent {
        let mut points = Vec::new();
        let mut membership = Vec::new();
        for (i, r) in self.runs.iter().enumerate() {
            if r[a].abs() > 0.5 && r[b].abs() > 0.5 {
                points.push(RowPoint { row: self.rows[i], x: if r[a] > 0.0 { 2.0 } else { 1.0 }, y: self.y[i] });
                membership.push(usize::from(r[b] > 0.0));
            }
        }
        let cell = |sa: f64, sb: f64| -> f64 {
            match self.request.means {
                MeansSource::Data => {
                    let m = interaction_means(&self.runs, &self.y, a, b);
                    m[usize::from(sa > 0.0)][usize::from(sb > 0.0)].mean
                }
                MeansSource::Model => self.predict(&self.point(&[(a, sa), (b, sb)])),
            }
        };
        let lines = [-1.0, 1.0]
            .iter()
            .map(|&sb| [[1.0, cell(-1.0, sb)], [2.0, cell(1.0, sb)]].into_iter().filter(|p| p[1].is_finite()).collect())
            .collect();
        let (lo, hi) = &self.level_labels[b];
        let name = &self.factor_names[b];
        SectionContent::Plot(Box::new(PlotSpec {
            x_label: self.factor_names[a].clone(),
            y_label: self.response_name.clone(),
            points,
            groups: Some(PointGroups {
                title: name.clone(),
                labels: vec![format!("{name} = {lo}"), format!("{name} = {hi}")],
                membership,
                lines,
            }),
            x_ticks: Some(vec![(1.0, self.level_labels[a].0.clone()), (2.0, self.level_labels[a].1.clone())]),
            ..PlotSpec::default()
        }))
    }

    /// The factors of the cube plot (indices into the factors).
    fn cube_factors(&self) -> Vec<usize> {
        let chosen: Vec<usize> = self
            .request
            .cube
            .iter()
            .filter_map(|c| self.request.factors.iter().position(|f| f.column == *c))
            .take(3)
            .collect();
        if chosen.len() >= 2 { chosen } else { (0..self.factor_names.len().min(3)).collect() }
    }

    /// The cube (three factors) or square (two) of means, drawn in an oblique
    /// projection: corner (a, b, c) at (a + 0.6·d, b + 0.45·d) with d = 0 for
    /// c low and 1 for c high.
    fn cube(&self) -> Option<(SectionContent, &'static str)> {
        let f = self.cube_factors();
        if f.len() < 2 {
            return None;
        }
        let place = |c: &[f64]| -> [f64; 2] {
            let d = if c.len() > 2 && c[2] > 0.0 { 1.0 } else { 0.0 };
            [c[0] + 0.6 * d, c[1] + 0.45 * d]
        };
        let corners: Vec<(Vec<f64>, f64)> = match self.request.means {
            MeansSource::Data => cube_means(&self.runs, &self.y, &f).into_iter().map(|(c, m)| (c, m.mean)).collect(),
            MeansSource::Model => stat_doe::factorial_analysis::corners(f.len())
                .into_iter()
                .map(|c| {
                    let set: Vec<(usize, f64)> = f.iter().copied().zip(c.iter().copied()).collect();
                    let v = self.predict(&self.point(&set));
                    (c, v)
                })
                .collect(),
        };
        let mut segments = Vec::new();
        for (i, (a, _)) in corners.iter().enumerate() {
            for (b, _) in &corners[i + 1..] {
                let differ = a.iter().zip(b).filter(|(x, y)| x != y).count();
                if differ == 1 {
                    segments.push(vec![place(a), place(b)]);
                }
            }
        }
        let legend = match self.request.means {
            MeansSource::Data => "Corner mean",
            MeansSource::Model => "Prediction",
        };
        let items = corners
            .iter()
            .filter(|(_, v)| v.is_finite())
            .map(|(c, v)| {
                let p = place(c);
                (p[0], p[1], format_number(*v, 4), 0)
            })
            .collect();
        let mut labels = Vec::new();
        if f.len() > 2 {
            let (lo, hi) = &self.level_labels[f[2]];
            let name = &self.factor_names[f[2]];
            labels.push((1.0, -1.25, format!("{name} = {lo}")));
            labels.push((1.6, -0.8, format!("{name} = {hi}")));
        }
        let (la, lb) = (&self.level_labels[f[0]], &self.level_labels[f[1]]);
        let spec = PlotSpec {
            x_label: self.factor_names[f[0]].clone(),
            y_label: self.factor_names[f[1]].clone(),
            segments,
            annotations: Some(Annotations { groups: vec![legend.to_owned()], items, labels }),
            x_ticks: Some(vec![(-1.0, la.0.clone()), (1.0, la.1.clone())]),
            y_ticks: Some(vec![(-1.0, lb.0.clone()), (1.0, lb.1.clone())]),
            ..PlotSpec::default()
        };
        Some((SectionContent::Plot(Box::new(spec)), if f.len() > 2 { "Cube Plot" } else { "Square Plot" }))
    }

    fn settings_table(&self, d: &ReplicatedDispersion) -> TextTable {
        let mut cols = self.factor_names.clone();
        cols.extend(["Runs", "Mean", "Std Dev", "ln s"].map(String::from));
        let mut t = TextTable::new(cols);
        for st in &d.settings {
            let mut line: Vec<String> = st
                .setting
                .iter()
                .zip(&self.level_labels)
                .map(|(x, (lo, hi))| {
                    if *x > 0.5 {
                        hi.clone()
                    } else if *x < -0.5 {
                        lo.clone()
                    } else {
                        format_number(*x, 3)
                    }
                })
                .collect();
            line.extend([
                st.n.to_string(),
                format_number(st.mean, DIGITS),
                format_number(st.sd, DIGITS),
                format_number(st.sd.ln(), DIGITS),
            ]);
            t.push(line);
        }
        t
    }

    fn dispersion_tests(&self, tests: &[DispersionTest]) -> TextTable {
        let mut t = TextTable::new([
            "Factor",
            "RSS at high (df)",
            "RSS at low (df)",
            "D (Bergman–Hynén)",
            "p-value",
            "Box–Meyer ln ratio",
        ]);
        for d in tests {
            let rss = |v: f64, df: usize| {
                if df == 0 { "—".to_owned() } else { format!("{} ({df})", format_number(v, DIGITS)) }
            };
            t.push([
                d.factor.clone(),
                rss(d.rss_high, d.df_high),
                rss(d.rss_low, d.df_low),
                format_number(d.statistic, DIGITS),
                format_p_value(d.p_value),
                format_number(d.box_meyer, DIGITS),
            ]);
        }
        t
    }
}

/// Code a categorical column's two levels 0 and 1 in sorted order.
fn two_levels(c: &stat_data::Column, n: usize) -> Result<(Vec<f64>, (String, String))> {
    let text: Vec<String> = (0..n).map(|r| c.level_text(r)).collect();
    let mut distinct: Vec<&String> = text.iter().filter(|t| !t.is_empty()).collect();
    distinct.sort_by(|a, b| stat_data::table_ops::natural_order(a, b));
    distinct.dedup();
    if distinct.len() != 2 {
        return Err(invalid(format!(
            "{} is categorical with {} levels: a two-level factor needs exactly two",
            c.name,
            distinct.len()
        )));
    }
    let (lo, hi) = (distinct[0].clone(), distinct[1].clone());
    let codes = text
        .iter()
        .map(|t| {
            if t.is_empty() {
                f64::NAN
            } else if *t == lo {
                0.0
            } else {
                1.0
            }
        })
        .collect();
    Ok((codes, (lo, hi)))
}

/// Number of distinct settings run at least twice.
fn settings_with_replicates(runs: &[Vec<f64>]) -> usize {
    let mut keys: Vec<Vec<i64>> = runs.iter().map(|r| r.iter().map(|x| (x * 1e6).round() as i64).collect()).collect();
    keys.sort();
    let mut count = 0;
    let mut i = 0;
    while i < keys.len() {
        let j = (i..keys.len()).find(|&j| keys[j] != keys[i]).unwrap_or(keys.len());
        if j - i > 1 {
            count += 1;
        }
        i = j;
    }
    count
}

/// The effects table: tests when there are error df, Lenth's columns when
/// Lenth's method is used, and the activity mark.
fn effects_table(set: &EffectSet, kind: FactorialResponse) -> TextTable {
    let tested = set.test.is_some();
    let lenth = set.lenth.is_some();
    let stat = match set.test {
        Some(EffectTest::Z) => "z Ratio",
        _ => "t Ratio",
    };
    let mut cols = vec!["Term", "Effect", "Coefficient"];
    if tested {
        cols.extend(["Std Error", stat, "p-value"]);
    }
    if lenth {
        cols.extend(["Lenth t", "Lenth p-value"]);
    }
    cols.push("Active");
    if kind != FactorialResponse::Continuous {
        cols.push(match kind {
            FactorialResponse::Binary => "Odds ratio (low → high)",
            _ => "Rate ratio (low → high)",
        });
    }
    let mut t = TextTable::new(cols);
    for e in &set.effects {
        let mut line = vec![e.term.clone(), format_number(e.effect, DIGITS), format_number(e.coefficient, DIGITS)];
        if tested {
            line.extend([format_number(e.std_error, DIGITS), format_number(e.statistic, 4), format_p_value(e.p_value)]);
        }
        if lenth {
            line.extend([format_number(e.lenth_t, 4), format_p_value(e.lenth_p)]);
        }
        line.push(
            match e.activity {
                Activity::Inactive => "",
                Activity::Active if lenth => "Active (beyond ME)",
                Activity::Active => "Active (p < α)",
                Activity::SimultaneouslyActive => "Active (beyond SME)",
            }
            .to_owned(),
        );
        if kind != FactorialResponse::Continuous {
            line.push(format_number(e.effect.exp(), DIGITS));
        }
        t.push(line);
    }
    t
}

/// A normal (or half-normal) plot of effects: labelled points grouped by
/// activity (marker shape and colour), the line of inert effects through the
/// origin with slope equal to their standard error, and ME/SME marks.
fn effect_plot(set: &EffectSet, half: bool, what: &str) -> SectionContent {
    let values = set.values();
    let scores = if half { half_normal_plot_positions(&values) } else { normal_plot_positions(&values) };
    let lenth = set.lenth.is_some();
    let groups: Vec<String> = if lenth {
        vec!["Inactive".into(), "Active (beyond ME)".into(), "Active (beyond SME)".into()]
    } else {
        vec!["Not significant".into(), "Active (p < α)".into()]
    };
    let x_of = |e: f64| if half { e.abs() } else { e };
    let items = set
        .effects
        .iter()
        .zip(&scores)
        .map(|(e, &z)| {
            let g = match e.activity {
                Activity::Inactive => 0,
                Activity::Active => 1,
                Activity::SimultaneouslyActive => 2,
            };
            (x_of(e.effect), z, e.term.clone(), g)
        })
        .collect();
    let (z_lo, z_hi) = scores.iter().fold((0.0f64, 0.0f64), |(a, b), &z| (a.min(z), b.max(z)));
    let (z_lo, z_hi) = (z_lo - 0.2, z_hi + 0.2);
    let scale = set.reference_scale();
    let z0 = if half { 0.0 } else { z_lo };
    let line = scale.is_finite().then(|| vec![[scale * z0, z0], [scale * z_hi, z_hi]]);
    let mut segments = Vec::new();
    let mut labels = Vec::new();
    if let Some(l) = &set.lenth {
        for (v, name) in [(l.me, "ME"), (l.sme, "SME")] {
            let sides: &[f64] = if half { &[1.0] } else { &[-1.0, 1.0] };
            for &s in sides {
                segments.push(vec![[s * v, z_lo], [s * v, z_hi]]);
                labels.push((s * v, z_hi + 0.15, name.to_owned()));
            }
        }
    }
    SectionContent::Plot(Box::new(PlotSpec {
        x_label: if half { format!("|{what}|") } else { what.to_owned() },
        y_label: if half { "Half-normal quantile".to_owned() } else { "Normal quantile".to_owned() },
        line,
        line_name: Some("Inert effects (slope = standard error)".to_owned()),
        segments,
        annotations: Some(Annotations { groups, items, labels }),
        ..PlotSpec::default()
    }))
}

fn section(kind: SectionKind, title: &str, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.to_owned(), content }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    /// Montgomery Example 6.2 (filtration rate) in standard order, with the
    /// factor settings in actual units.
    fn filtration() -> DataTable {
        let level = |j: usize, lo: f64, hi: f64| -> Vec<f64> {
            (0..16).map(|i| if (i >> j) & 1 == 1 { hi } else { lo }).collect()
        };
        let y = vec![45.0, 71.0, 48.0, 65.0, 68.0, 60.0, 80.0, 65.0, 43.0, 100.0, 45.0, 104.0, 75.0, 86.0, 70.0, 96.0];
        let pressure: Vec<String> =
            (0..16).map(|i| if (i >> 1) & 1 == 1 { "High" } else { "Low" }.to_owned()).collect();
        DataTable::new(
            "filtration",
            vec![
                Column::numeric("Temperature", level(0, 24.0, 35.0)),
                Column::text("Pressure", pressure),
                Column::numeric("Concentration", level(2, 2.0, 4.0)),
                Column::numeric("Stirring", level(3, 15.0, 30.0)),
                Column::numeric("Rate", y),
            ],
        )
        .unwrap()
    }

    #[test]
    fn filtration_report() {
        let t = filtration();
        let req = FactorialAnalysisRequest::new(&[0, 1, 2, 3], 4);
        let a = FactorialAnalysis::run(&t, &RowStateManager::new(16), &req).unwrap();
        let e = a.effects();
        assert_eq!(e.effects.len(), 15);
        // "Pressure" is text: High sorts before Low, so High is coded −1 and B's sign flips.
        assert!((e.effects[0].effect - 21.625).abs() < 1e-12);
        assert!((e.effects[1].effect + 3.125).abs() < 1e-12);
        assert!((e.effects[5].effect + 18.125).abs() < 1e-12, "{}", e.effects[5].term);
        assert!((e.lenth.unwrap().pse - 2.625).abs() < 1e-12);
        let s = a.sections();
        let kinds: Vec<SectionKind> = s.iter().map(|s| s.kind).collect();
        assert!(kinds.contains(&SectionKind::FactorialNormalPlot));
        assert!(kinds.contains(&SectionKind::FactorialCube));
        assert!(kinds.contains(&SectionKind::EffectPareto));
        assert_eq!(kinds.iter().filter(|k| **k == SectionKind::FactorialInteraction).count(), 6);
        let SectionContent::Table(effects) = &s[1].content else { panic!() };
        assert_eq!(effects.columns, ["Term", "Effect", "Coefficient", "Lenth t", "Lenth p-value", "Active"]);
        assert_eq!(effects.rows[0][5], "Active (beyond SME)");
        assert_eq!(effects.rows[2][5], "Active (beyond ME)");
        // Normal plot: labelled effects grouped by activity, ME/SME marks.
        let plot = s.iter().find(|s| s.kind == SectionKind::FactorialNormalPlot).unwrap();
        let SectionContent::Plot(p) = &plot.content else { panic!() };
        let notes = p.annotations.as_ref().unwrap();
        assert_eq!(notes.items.len(), 15);
        assert_eq!(notes.items.iter().filter(|i| i.3 == 2).count(), 4);
        assert_eq!(p.segments.len(), 4);
        // Cube of the first three factors: corner means over Stirring.
        let cube = s.iter().find(|s| s.kind == SectionKind::FactorialCube).unwrap();
        let SectionContent::Plot(c) = &cube.content else { panic!() };
        assert_eq!(c.segments.len(), 12);
        let notes = c.annotations.as_ref().unwrap();
        assert_eq!(notes.items.len(), 8);
        // Corner (Temperature low, Pressure "High" = −1, Concentration low): runs 3 and 11 → (48 + 45)/2.
        let first = notes.items.iter().find(|i| i.0 == -1.0 && i.1 == -1.0).unwrap();
        assert_eq!(first.2, "46.5");
        // Unreplicated: Bergman–Hynén with the active effects as location model.
        let disp = s.iter().find(|s| s.kind == SectionKind::FactorialDispersion).unwrap();
        let SectionContent::Table(d) = &disp.content else { panic!() };
        assert_eq!(d.rows.len(), 4);
        assert_eq!(d.rows[2][3], "4.5231");
        // Model predictions in the main-effects plot: intercept ± coefficient.
        let mut m = req.clone();
        m.means = MeansSource::Model;
        m.terms = FactorialTerms::Formula("Temperature + Concentration + Stirring".into());
        let b = FactorialAnalysis::run(&t, &RowStateManager::new(16), &m).unwrap();
        assert_eq!(b.effects().test, Some(EffectTest::T(12.0)));
        let bs = b.sections();
        let SectionContent::Plot(me) =
            &bs.iter().find(|s| s.kind == SectionKind::FactorialMainEffects).unwrap().content
        else {
            panic!()
        };
        let line = &me.groups.as_ref().unwrap().lines[0];
        assert!((line[1][1] - line[0][1] - 21.625).abs() < 1e-9);
        // JSON round trip.
        let mut r = req.clone();
        r.factors[0].low = Some(20.0);
        r.terms = FactorialTerms::Order(2);
        r.cube = vec![0, 2, 3];
        r.means = MeansSource::Model;
        assert_eq!(FactorialAnalysisRequest::from_json(&r.to_json(&t), &t).unwrap(), r);
    }

    #[test]
    fn binary_and_count_responses() {
        // A replicated 2² with counts: Poisson regression with Wald z tests.
        let a: Vec<f64> = (0..12).map(|i| if i % 2 == 0 { -1.0 } else { 1.0 }).collect();
        let b: Vec<f64> = (0..12).map(|i| if (i / 2) % 2 == 0 { -1.0 } else { 1.0 }).collect();
        let counts = vec![3.0, 8.0, 4.0, 11.0, 2.0, 9.0, 5.0, 14.0, 4.0, 7.0, 3.0, 12.0];
        let pass: Vec<String> =
            (0..12).map(|i| if [1, 3, 6, 7, 9, 10].contains(&i) { "pass" } else { "fail" }.to_owned()).collect();
        let t = DataTable::new(
            "t",
            vec![
                Column::numeric("A", a),
                Column::numeric("B", b),
                Column::numeric("Defects", counts),
                Column::text("Result", pass),
            ],
        )
        .unwrap();
        let rows = RowStateManager::new(12);
        let mut req = FactorialAnalysisRequest::new(&[0, 1], 2);
        req.response_type = FactorialResponse::Count;
        let an = FactorialAnalysis::run(&t, &rows, &req).unwrap();
        assert_eq!(an.effects().test, Some(EffectTest::Z));
        let sum: f64 = an.formula().n_terms() as f64;
        assert_eq!(sum, 4.0);
        // Poisson log link: the A effect is twice the coefficient of a GLM fit.
        assert!((an.predict(&[1.0, 1.0]) - (11.0 + 14.0 + 12.0) / 3.0).abs() < 1e-6, "saturated in the 4 cells");
        let SectionContent::Table(tab) = &an.sections()[1].content else { panic!() };
        assert_eq!(tab.columns.last().unwrap(), "Rate ratio (low → high)");
        req.response_type = FactorialResponse::Binary;
        req.response = 3;
        req.terms = FactorialTerms::Order(1);
        let bn = FactorialAnalysis::run(&t, &rows, &req).unwrap();
        assert!(bn.title().contains("Result"));
        let SectionContent::Table(sum) = &bn.sections()[0].content else { panic!() };
        assert!(sum.rows[0][1].contains("probability of pass"));
        // A saturated logistic model is refused.
        req.terms = FactorialTerms::All;
        let mut first_four = RowStateManager::new(12);
        first_four.set_excluded(4..12, true);
        let e = FactorialAnalysis::run(&t, &first_four, &req).unwrap_err();
        assert!(e.to_string().contains("saturated logistic"), "{e}");
    }

    #[test]
    fn errors() {
        let t = filtration();
        let rows = RowStateManager::new(16);
        assert!(FactorialAnalysis::run(&t, &rows, &FactorialAnalysisRequest::new(&[], 4)).is_err());
        assert!(FactorialAnalysis::run(&t, &rows, &FactorialAnalysisRequest::new(&[0, 0], 4)).is_err());
        let mut r = FactorialAnalysisRequest::new(&[0, 2], 4);
        r.terms = FactorialTerms::Formula("Temperature^2".into());
        assert!(FactorialAnalysis::run(&t, &rows, &r).unwrap_err().to_string().contains("power"));
        r.terms = FactorialTerms::Formula("Nope".into());
        assert!(FactorialAnalysis::run(&t, &rows, &r).is_err());
        let mut c = FactorialAnalysisRequest::new(&[0, 2], 1);
        c.response_type = FactorialResponse::Count;
        assert!(FactorialAnalysis::run(&t, &rows, &c).is_err(), "a text response is not a count");
    }
}
